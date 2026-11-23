//! Critical-path scheduling engine.
//!
//! [`recalculate`] runs a calendar-aware forward and backward pass over the
//! leaf tasks of a project, honoring all four link types with lags/leads,
//! constraints, deadlines, actual progress, leveling delays, manual tasks,
//! milestone membership and cross-document (external) predecessors.  Results
//! are written into [`TaskSchedule`] and rolled up into summary tasks and the
//! project.
//!
//! Links attached to summary tasks apply to all of the summary's children,
//! like in Microsoft Project.
//!
//! A manually scheduled summary task (top-down planning) keeps the dates typed for it: its
//! subtasks don't move it, links into it stop at it, its auto-scheduled subtasks start no
//! earlier than its start, and its successors follow its own dates. The span of its subtasks is
//! kept separately ([`TaskSchedule::rollup_start`]).
//!
//! The engine never indexes out of bounds and clamps date arithmetic; as a last resort
//! [`recalculate`] runs behind [`crate::guard`], so a bug produces a warning instead of a crash.

use std::collections::HashMap;

use crate::dates::prelude::*;

use crate::calendar::{CalLayer, WorkCal, shift};
use crate::duration::{Lag, Minutes};
use crate::guard::{self, Panicked};
use crate::ids::{CalendarId, ResourceId, TaskId};
use crate::model::*;

/// Where a link's predecessor dates come from.
#[derive(Clone, Debug)]
enum PredSource {
    /// A leaf task index.
    Leaf(usize),
    /// A summary used as SS/SF predecessor: start = min of leaves' starts.
    SummaryStart { leaves: Vec<usize>, starts: Vec<DateTime>, finishes: Vec<DateTime> },
    /// A summary with one fixed side used as predecessor: one side is anchored, the other
    /// still rolls up from its leaf descendants.
    SummaryAnchored { leaves: Vec<usize>, anchor: DateTime, fixed_finish: bool, other_anchor: Option<DateTime> },
    /// Fixed dates of an external task.
    Fixed { start: DateTime, finish: DateTime, duration: Minutes },
}

#[derive(Clone, Debug)]
struct Edge {
    pred: PredSource,
    succ: usize,
    kind: LinkType,
    lag: Lag,
    drive: LinkDrive,
    /// When the dependency was inherited from an ancestor summary, that summary's
    /// `fixedStartCap` limits how late this drive may push the leaf (Swift: drive through
    /// capped `S(group)` before `S(group)→S(child)`).
    via_summary_cap: Option<DateTime>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Dates {
    es: Option<DateTime>,
    ef: Option<DateTime>,
    ls: Option<DateTime>,
    lf: Option<DateTime>,
    /// Completed and remaining parts of an in-progress task.
    parts: Option<Parts>,
}

/// The completed and remaining parts of an in-progress task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// End of the completed part (Microsoft Project's Stop field).
    pub stop: DateTime,
    /// Start of the remaining part: the next working moment after `stop`, or the start of the next
    /// working part when the task pauses at `stop` (Microsoft Project's Resume field).
    pub resume: DateTime,
    /// Working minutes left to do.
    pub remaining: Minutes,
    /// End of the last working part: the task's finish.
    pub finish: DateTime,
    /// Start of the last working part and the working minutes in it (the part that can still
    /// slip); the actual start and the whole duration when the task is not split.
    pub last: (DateTime, Minutes),
    /// The task has pauses (see [`segments_of`]).
    pub split: bool,
}

/// Most pauses one task can have; further ones are ignored.
pub const MAX_SPLITS: usize = 100;

/// The planned splits of a task that take effect: inside its work (`0 < work < dur`), with a
/// length, one per work point (the first), in work order, and together at most
/// [`MAX_MINUTES`](crate::limits::MAX_MINUTES) long (later ones are cut short).
pub fn planned_gaps(gaps: &[Gap], dur: Minutes) -> Vec<Gap> {
    let mut v: Vec<Gap> = gaps.iter().take(MAX_SPLITS).copied().filter(|g| g.work > 0 && g.work < dur && g.length > 0).collect();
    v.sort_by_key(|g| g.work);
    v.dedup_by_key(|g| g.work);
    let mut left = crate::limits::MAX_MINUTES;
    for g in &mut v {
        g.length = g.length.min(left);
        left -= g.length;
    }
    v.retain(|g| g.length > 0);
    v
}

/// Working time a task spans from start to finish: its duration plus its planned splits.
pub fn span_of(t: &Task, dur: Minutes) -> Minutes {
    planned_gaps(&t.gaps, dur).iter().fold(dur, |s, g| s.saturating_add(g.length))
}

/// The working parts `(start, finish, work before it)` of a task laid out from `start`. After a
/// planned split's work the task pauses for the split's length; after an in-progress split's work
/// it stops until that split's resume date. Splits that would not leave a gap (or lie past the
/// duration) are ignored, so a task with no real pause has one part.
pub fn segments_of(start: DateTime, splits: &[Split], gaps: &[Gap], cal: &WorkCal, dur: Minutes) -> Vec<(DateTime, DateTime, Minutes)> {
    enum Until {
        Date(DateTime),
        For(Minutes),
    }
    let mut breaks: Vec<(Minutes, Until)> = planned_gaps(gaps, dur).into_iter().map(|g| (g.work, Until::For(g.length))).collect();
    breaks.extend(splits.iter().take(MAX_SPLITS).map(|s| (s.work, Until::Date(s.resume))));
    breaks.sort_by_key(|b| b.0);
    let mut out = Vec::new();
    let (mut at, mut used) = (start, 0);
    let mut k = 0;
    while let Some(&(work, _)) = breaks.get(k) {
        // Every pause at this work point: work goes on at the latest of them.
        let same = breaks.iter().skip(k).take_while(|b| b.0 == work).count();
        let group = breaks.get(k..k + same).unwrap_or_default();
        k += same.max(1);
        if work <= used {
            continue;
        }
        if work >= dur {
            break;
        }
        let end = cal.add(at, work - used);
        let resume = group
            .iter()
            .map(|(_, until)| match until {
                Until::Date(d) => cal.snap_forward(*d),
                Until::For(len) => cal.snap_forward(cal.add(cal.snap_forward(end), *len)),
            })
            .max()
            .unwrap_or(end);
        if resume <= cal.snap_forward(end) {
            // No gap: the part simply goes on.
            continue;
        }
        out.push((at, end, used));
        at = resume;
        used = work;
    }
    out.push((at, cal.add(at, dur.saturating_sub(used)), used));
    out
}

/// Splits an in-progress task (actual start, 1-99% complete, no actual finish) into the part done
/// and the part left; `None` for any other task.
pub fn parts_of(t: &Task, cal: &WorkCal, dur: Minutes) -> Option<Parts> {
    let start = t.actual_start?;
    if t.actual_finish.is_some() || t.is_milestone() || dur <= 0 || t.mode == ScheduleMode::Manual {
        return None;
    }
    let pct = Minutes::from(t.percent_complete);
    if !(1..100).contains(&pct) {
        return None;
    }
    let done = dur.saturating_mul(pct) / 100;
    let remaining = dur.saturating_sub(done);
    if t.elapsed {
        // Elapsed time runs on without pauses (an elapsed task cannot be split).
        let stop = shift(start, done);
        return Some(Parts { stop, resume: stop, remaining, finish: shift(start, dur), last: (start, dur), split: false });
    }
    if t.splits.is_empty() && planned_gaps(&t.gaps, dur).is_empty() {
        let stop = cal.add(start, done);
        return Some(Parts {
            stop,
            resume: cal.snap_forward(stop),
            remaining,
            finish: cal.add(start, dur),
            last: (start, dur),
            split: false,
        });
    }
    let segs = segments_of(start, &t.splits, &t.gaps, cal, dur);
    let (stop, resume) = stop_and_resume(&segs, cal, done)?;
    let &(last_start, finish, before) = segs.last()?;
    Some(Parts { stop, resume, remaining, finish, last: (last_start, dur.saturating_sub(before)), split: segs.len() > 1 })
}

/// Where `done` working minutes end within `segs` (the Stop), and where the rest starts (the
/// Resume: the next part when the work done ends exactly at a pause).
fn stop_and_resume(segs: &[(DateTime, DateTime, Minutes)], cal: &WorkCal, done: Minutes) -> Option<(DateTime, DateTime)> {
    for (k, &(s, f, before)) in segs.iter().enumerate() {
        let next = segs.get(k + 1);
        let len = next.map_or(Minutes::MAX, |n| n.2.saturating_sub(before));
        if done.saturating_sub(before) < len {
            let stop = cal.add(s, done.saturating_sub(before).max(0));
            return Some((stop, cal.snap_forward(stop)));
        }
        if let Some(&(ns, _, nb)) = next
            && done == nb
        {
            return Some((f, ns));
        }
    }
    segs.last().map(|&(_, f, _)| (f, f))
}

/// Working minutes of `segs` done by `upto`.
pub fn work_done_by(segs: &[(DateTime, DateTime)], cal: &WorkCal, upto: DateTime) -> Minutes {
    segs.iter().filter(|(s, _)| *s < upto).map(|&(s, f)| cal.between(s, f.min(upto)).max(0)).fold(0, Minutes::saturating_add)
}

/// Summary of a recalculation.
#[derive(Clone, Debug, Default)]
pub struct ScheduleReport {
    pub warnings: Vec<String>,
    pub start: Option<DateTime>,
    pub finish: Option<DateTime>,
    pub critical_tasks: Vec<TaskId>,
}

/// Builds the effective working calendar of the task at `idx`.
///
/// Assigned work resources contribute their own calendars (base calendar plus
/// personal exceptions); the task is scheduled in time common to all of them.
pub fn task_calendar(p: &Project, idx: usize) -> WorkCal<'_> {
    let Some(task) = p.tasks.get(idx) else {
        return project_calendar(p);
    };
    let resources: Vec<&Resource> = p.assignments_for_task(task.id).filter_map(|a| p.resource(a.resource)).collect();
    calendar_for(p, task, &resources)
}

/// [`task_calendar`] with the task's assigned resources already looked up.
pub(crate) fn calendar_for<'a>(p: &'a Project, task: &Task, resources: &[&'a Resource]) -> WorkCal<'a> {
    let base = task.calendar.and_then(|c| p.calendar(c)).unwrap_or_else(|| p.base_calendar());
    let mut layers = Vec::new();
    if task.calendar.is_some() {
        layers.push(CalLayer::new(base, &[]));
    }
    if p.settings.use_resource_calendars && !task.ignore_resource_calendars {
        for r in resources.iter().filter(|r| r.kind == ResourceType::Work) {
            let cal = r.calendar.and_then(|c| p.calendar(c)).unwrap_or_else(|| p.base_calendar());
            layers.push(CalLayer::new(cal, &r.exceptions));
        }
    }
    if layers.is_empty() {
        return WorkCal::single(base);
    }
    let wc = WorkCal::from_layers(layers);
    // Fall back to the task/project calendar only when the resources share no working time at
    // all. Search well past a long absence (a month of leave at the project start must delay
    // the task, not drop the resource's calendar); the search stops at the first working day.
    let any = p.start.date().iter_days().take(RESOURCE_CALENDAR_SEARCH_DAYS).any(|d| wc.is_working_day(d));
    if any { wc } else { WorkCal::single(base) }
}

/// How far ahead [`calendar_for`] looks for working time shared by a task's resources.
const RESOURCE_CALENDAR_SEARCH_DAYS: usize = 3 * 366;

/// What makes two tasks' scheduling calendars identical (see [`calendar_for`]): the task
/// calendar and, when resource calendars apply, each work resource's layer: its base calendar,
/// and the resource itself only when it has personal exceptions. Resources on the same calendar
/// without time off of their own give the same working time, so their tasks share one calendar
/// (and one month-and-year cache): 20,000 resources on the Standard calendar used to fill 20,000
/// caches, 440 MB.
type CalKey = (Option<CalendarId>, Vec<(Option<CalendarId>, Option<ResourceId>)>);

fn cal_key(p: &Project, task: &Task, resources: &[&Resource]) -> CalKey {
    let mut layers: Vec<(Option<CalendarId>, Option<ResourceId>)> = if p.settings.use_resource_calendars && !task.ignore_resource_calendars
    {
        resources
            .iter()
            .filter(|r| r.kind == ResourceType::Work)
            .map(|r| (r.calendar, (!r.exceptions.is_empty()).then_some(r.id)))
            .collect()
    } else {
        Vec::new()
    };
    layers.sort();
    layers.dedup();
    (task.calendar, layers)
}

/// Each task's assigned resources, looked up once.
fn assigned_resources(p: &Project) -> HashMap<TaskId, Vec<&Resource>> {
    let resources: HashMap<ResourceId, &Resource> = p.resources.iter().map(|r| (r.id, r)).collect();
    let mut assigned: HashMap<TaskId, Vec<&Resource>> = HashMap::new();
    for a in &p.assignments {
        if let Some(r) = resources.get(&a.resource) {
            assigned.entry(a.task).or_default().push(r);
        }
    }
    assigned
}

/// The scheduling calendars of many tasks (drawing or printing a chart), one per distinct
/// calendar as in a recalculation, so each fills its month-and-year cache once instead of once
/// per task.
pub struct TaskCalendars<'a> {
    p: &'a Project,
    assigned: HashMap<TaskId, Vec<&'a Resource>>,
    keys: HashMap<CalKey, usize>,
    cals: Vec<WorkCal<'a>>,
}

impl<'a> TaskCalendars<'a> {
    pub fn new(p: &'a Project) -> Self {
        Self { p, assigned: assigned_resources(p), keys: HashMap::new(), cals: Vec::new() }
    }

    /// The scheduling calendar of the task at row `idx` (the same as [`task_calendar`]).
    pub fn of(&mut self, idx: usize) -> Option<&WorkCal<'a>> {
        let p = self.p;
        let task = p.tasks.get(idx)?;
        let resources = self.assigned.get(&task.id).map(Vec::as_slice).unwrap_or_default();
        let cals = &mut self.cals;
        let i = *self.keys.entry(cal_key(p, task, resources)).or_insert_with(|| {
            cals.push(calendar_for(p, task, resources));
            cals.len().saturating_sub(1)
        });
        self.cals.get(i)
    }
}

/// The project calendar used for lags and summary durations.
pub fn project_calendar(p: &Project) -> WorkCal<'_> {
    WorkCal::single(p.base_calendar())
}

/// The dates of a manually scheduled summary task: its typed start, and its typed finish or
/// (without one) its start plus its duration on the project calendar `cal`. `None` for any other
/// task, and for a manual summary without a typed start (it is scheduled like an automatic one).
pub fn manual_summary_dates(t: &Task, is_summary: bool, cal: &WorkCal) -> Option<(DateTime, DateTime)> {
    if !is_summary || t.mode != ScheduleMode::Manual {
        return None;
    }
    let start = t.manual_start?;
    let finish = t.manual_finish.unwrap_or_else(|| if t.duration > 0 { cal.add(start, t.duration) } else { start });
    Some((start, finish.max(start)))
}

fn percent_of(duration: Minutes, pct: i32) -> Minutes {
    duration.saturating_mul(Minutes::from(pct)) / 100
}

fn apply_lag(cal: &WorkCal, base: DateTime, lag: Lag, pred_duration: Minutes) -> DateTime {
    match lag {
        Lag::None | Lag::Working(0) => base,
        Lag::Working(m) => cal.add(base, m),
        Lag::Elapsed(m) => shift(base, m),
        Lag::Percent(pct) => match percent_of(pred_duration, pct) {
            0 => base,
            m => cal.add(base, m),
        },
    }
}

/// Pull a successor hard date backward across a link lag (working, elapsed, or percent of the
/// predecessor's stored duration). Shared with share successor-cap math so Auto/Fill budgets
/// match CPM.
pub(crate) fn unapply_lag(cal: &WorkCal, base: DateTime, lag: Lag, pred_duration: Minutes) -> DateTime {
    match lag {
        Lag::None | Lag::Working(0) => base,
        Lag::Working(m) => cal.sub(base, m),
        Lag::Elapsed(m) => shift(base, m.saturating_neg()),
        Lag::Percent(pct) => match percent_of(pred_duration, pct) {
            0 => base,
            m => cal.sub(base, m),
        },
    }
}

/// Index of each task's parent summary, computed in one pass.
pub(crate) fn parents(p: &Project) -> Vec<Option<usize>> {
    let mut stack: Vec<(usize, u16)> = Vec::new();
    p.tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            while stack.last().is_some_and(|&(_, lvl)| lvl >= t.outline_level) {
                stack.pop();
            }
            let parent = stack.last().map(|&(pi, _)| pi);
            stack.push((i, t.outline_level));
            parent
        })
        .collect()
}

struct Engine<'a> {
    p: &'a Project,
    proj_cal: WorkCal<'a>,
    /// Distinct scheduling calendars; tasks with the same calendar and resources share one, so
    /// its cached spans serve all of them.
    cals: Vec<WorkCal<'a>>,
    /// Index into `cals` for each task.
    cal_of: Vec<usize>,
    leaves: Vec<usize>,
    is_leaf: Vec<bool>,
    parent: Vec<Option<usize>>,
    position: HashMap<TaskId, usize>,
    edges: Vec<Edge>,
    /// Edges indexed by successor.
    incoming: Vec<Vec<usize>>,
    /// Edges indexed by predecessor leaf (only `Leaf` sources; summary starts are expanded).
    outgoing: Vec<Vec<usize>>,
    order: Vec<usize>,
    /// Scheduling warnings, each with the task it concerns (`None`: the plan as a whole).
    /// A task's own warnings end up in its `sched.warnings`, so its row, pane and agents
    /// see them — not just the Statistics window.
    warnings: Vec<(Option<usize>, String)>,
    dates: Vec<Dates>,
    /// Working parts of split tasks (by task index).
    segments: HashMap<usize, Vec<(DateTime, DateTime)>>,
    /// Per task: the latest Start No Earlier Than date and the earliest deadline set on its
    /// ancestor summaries (both apply to every subtask).
    inherited: Vec<Inherited>,
    /// The dates of each manually scheduled summary (see [`manual_summary_dates`]).
    manual_summary: Vec<Option<(DateTime, DateTime)>>,
    /// Working minutes resolved from stretch and percent-of-group. `None` uses the stored duration.
    resolved: Vec<Option<Minutes>>,
    /// Earliest fixed latest-start among the task and its descendants (Swift `fixedStartCap`).
    /// A group cannot start after a hard SNLT / fixed start-latest inside it.
    fixed_start_cap: Vec<Option<DateTime>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Inherited {
    snet: Option<DateTime>,
    finish_cap: Option<SummaryFinishCap>,
    deadline: Option<DateTime>,
}

#[derive(Clone, Copy, Debug)]
struct SummaryFinishCap {
    date: DateTime,
    source: usize,
}

#[derive(Clone, Copy, Debug)]
struct SummaryStartFloor {
    date: DateTime,
    source: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct SummaryAncestorLimits {
    start_floor: Option<SummaryStartFloor>,
    finish_cap: Option<SummaryFinishCap>,
}

/// Constraints a summary passes to its subtasks: its Start No Earlier Than date drives their
/// starts, and its deadline bounds their late finish (and so their slack). A manually scheduled
/// summary's start drives its subtasks' starts in the same way (its constraint is ignored, like
/// any manual task's).
fn inherited_from_summaries(p: &Project, parent: &[Option<usize>], manual_summary: &[Option<(DateTime, DateTime)>]) -> Vec<Inherited> {
    let mut out = vec![Inherited::default(); p.tasks.len()];
    // Parents come before their children, so each parent's value is final when a child reads it.
    for i in 0..p.tasks.len() {
        let Some(pi) = parent.get(i).copied().flatten() else { continue };
        let (Some(par), Some(up)) = (p.tasks.get(pi), out.get(pi).copied()) else { continue };
        let (start_min, _) = par.start_window();
        let (_, finish_max) = par.finish_window();
        let finish_mode = par.finish_mode();
        // An inactive summary passes on what its own summaries set, not its own date or deadline.
        let own_snet = if par.active {
            if let Some((start, _)) = manual_summary.get(pi).copied().flatten() {
                Some(start)
            } else {
                // Earliest summary starts flow down to descendants; no-later starts do not.
                start_min
            }
        } else {
            None
        };
        // A fixed summary finish is a hard cap for its descendants.
        let own_finish_cap = if par.active {
            if manual_summary.get(pi).copied().flatten().is_some() {
                // Legacy top-down planning keeps a manual summary's finish independent from its
                // subtasks: overrun is shown on the summary rollup instead of moving descendants.
                None
            } else if let Some(anchor) = par.side_anchor(true) {
                Some(SummaryFinishCap { date: anchor, source: pi })
            } else if par.bounds.finish_latest_fixed {
                finish_max.map(|date| SummaryFinishCap { date, source: pi })
            } else {
                None
            }
        } else {
            None
        };
        // An unfixed latest finish limits late dates the way a deadline does. A fixed one moves
        // the plan instead. "On" remains an exact anchor (handled above), not a soft deadline.
        let bound_deadline = if par.active && !par.bounds.finish_latest_fixed && finish_mode != DateMode::On { finish_max } else { None };
        let own_deadline = min_opt(par.deadline.filter(|_| par.active), bound_deadline);
        let v = Inherited {
            snet: up.snet.max(own_snet),
            finish_cap: min_cap(up.finish_cap, own_finish_cap),
            deadline: min_opt(up.deadline, own_deadline),
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = v;
        }
    }
    out
}

/// Applies both bounds on one task. Empty bounds leave `es` unchanged.
fn apply_date_bounds(
    t: &Task,
    mut es: DateTime,
    dur: Minutes,
    along: &dyn Fn(DateTime, bool) -> DateTime,
    warn: bool,
    warnings: &mut Vec<String>,
) -> DateTime {
    if let Some(date) = t.bounds.start_earliest {
        es = es.max(date);
    }
    if let Some(date) = t.bounds.finish_earliest {
        let shifted = if dur > 0 { along(date, false) } else { date };
        es = es.max(shifted);
    }
    if t.bounds.start_latest_fixed
        && let Some(date) = t.bounds.start_latest
        && es > date
    {
        if warn {
            warnings.push("Cannot start by its fixed no-later-than date".to_string());
        }
        es = match t.bounds.start_earliest {
            Some(floor) => date.max(floor),
            None => date,
        };
    }
    if t.bounds.finish_latest_fixed
        && let Some(date) = t.bounds.finish_latest
    {
        let s = if dur > 0 { along(date, false) } else { date };
        if es > s {
            if warn {
                warnings.push("Cannot finish by its fixed no-later-than date".to_string());
            }
            es = match t.bounds.start_earliest {
                Some(floor) => s.max(floor),
                None => s,
            };
        }
    }
    es
}

fn min_opt(a: Option<DateTime>, b: Option<DateTime>) -> Option<DateTime> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

fn min_cap(a: Option<SummaryFinishCap>, b: Option<SummaryFinishCap>) -> Option<SummaryFinishCap> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if x.date <= y.date { x } else { y }),
        (x, None) => x,
        (None, y) => y,
    }
}

fn max_floor(a: Option<SummaryStartFloor>, b: Option<SummaryStartFloor>) -> Option<SummaryStartFloor> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if x.date >= y.date { x } else { y }),
        (x, None) => x,
        (None, y) => y,
    }
}

fn inherited_finish_cap_warning(p: &Project, cap: SummaryFinishCap) -> String {
    let date = cap.date.format("%Y-%m-%d");
    if let Some(group) = p.tasks.get(cap.source) {
        return format!("Cannot finish by ancestor summary '{}' fixed finish date ({date})", group.name);
    }
    format!("Cannot finish by an ancestor summary's fixed finish date ({date})")
}

fn inherited_start_floor_warning(p: &Project, floor: SummaryStartFloor) -> String {
    let date = floor.date.format("%Y-%m-%d");
    if let Some(group) = p.tasks.get(floor.source) {
        return format!("Cannot start before ancestor summary '{}' constrained start date ({date})", group.name);
    }
    format!("Cannot start before an ancestor summary's constrained start date ({date})")
}

fn own_hard_finish_pin(t: &Task) -> Option<DateTime> {
    if t.bounds.finish_latest_fixed {
        return t.finish_window().1;
    }
    if matches!(t.constraint.kind, ConstraintKind::Mfo | ConstraintKind::Fnlt) {
        return t.constraint.date;
    }
    t.side_anchor(true)
}

fn summary_ancestor_limits(
    p: &Project,
    parent: &[Option<usize>],
    manual_summary: &[Option<(DateTime, DateTime)>],
) -> Vec<SummaryAncestorLimits> {
    let mut out = vec![SummaryAncestorLimits::default(); p.tasks.len()];
    for i in 0..p.tasks.len() {
        let Some(pi) = parent.get(i).copied().flatten() else { continue };
        let (Some(par), Some(up)) = (p.tasks.get(pi), out.get(pi).copied()) else { continue };
        let (start_min, _) = par.start_window();
        let (_, finish_max) = par.finish_window();
        let own_start_floor = if par.active {
            if let Some((start, _)) = manual_summary.get(pi).copied().flatten() {
                Some(SummaryStartFloor { date: start, source: pi })
            } else {
                start_min.map(|date| SummaryStartFloor { date, source: pi })
            }
        } else {
            None
        };
        let own_finish_cap = if par.active {
            if manual_summary.get(pi).copied().flatten().is_some() {
                None
            } else if let Some(anchor) = par.side_anchor(true) {
                Some(SummaryFinishCap { date: anchor, source: pi })
            } else if par.bounds.finish_latest_fixed {
                finish_max.map(|date| SummaryFinishCap { date, source: pi })
            } else {
                None
            }
        } else {
            None
        };
        let v = SummaryAncestorLimits {
            start_floor: max_floor(up.start_floor, own_start_floor),
            finish_cap: min_cap(up.finish_cap, own_finish_cap),
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = v;
        }
    }
    out
}

impl<'a> Engine<'a> {
    fn new(p: &'a Project) -> Self {
        let n = p.tasks.len();
        let is_leaf: Vec<bool> = (0..n).map(|i| !p.has_children(i)).collect();
        let leaves: Vec<usize> = is_leaf.iter().enumerate().filter(|(_, leaf)| **leaf).map(|(i, _)| i).collect();
        let assigned = assigned_resources(p);
        let mut cals: Vec<WorkCal<'a>> = Vec::new();
        let mut cal_keys: HashMap<CalKey, usize> = HashMap::new();
        let cal_of = p
            .tasks
            .iter()
            .map(|t| {
                let resources = assigned.get(&t.id).map(Vec::as_slice).unwrap_or_default();
                *cal_keys.entry(cal_key(p, t, resources)).or_insert_with(|| {
                    cals.push(calendar_for(p, t, resources));
                    cals.len().saturating_sub(1)
                })
            })
            .collect();
        let parent = parents(p);
        let proj_cal = project_calendar(p);
        let manual_summary: Vec<Option<(DateTime, DateTime)>> =
            p.tasks.iter().zip(&is_leaf).map(|(t, leaf)| manual_summary_dates(t, !leaf, &proj_cal)).collect();
        let inherited = inherited_from_summaries(p, &parent, &manual_summary);
        // Own fixed latest-start / SNLT / MSO, then roll min up so a child's hard start caps
        // every ancestor group (Swift `fixedStartCap`).
        let mut fixed_start_cap: Vec<Option<DateTime>> = (0..n).map(|i| p.tasks.get(i).and_then(own_fixed_start_cap)).collect();
        for i in (0..n).rev() {
            let Some(pi) = parent.get(i).copied().flatten() else { continue };
            let child = fixed_start_cap.get(i).copied().flatten();
            let slot = fixed_start_cap.get_mut(pi);
            if let (Some(c), Some(slot)) = (child, slot) {
                *slot = Some(slot.map_or(c, |p| p.min(c)));
            }
        }
        Self {
            p,
            proj_cal,
            cals,
            cal_of,
            leaves,
            is_leaf,
            parent,
            position: p.tasks.iter().enumerate().map(|(i, t)| (t.id, i)).collect(),
            edges: Vec::new(),
            incoming: vec![Vec::new(); n],
            outgoing: vec![Vec::new(); n],
            order: Vec::new(),
            warnings: Vec::new(),
            dates: vec![Dates::default(); n],
            segments: HashMap::new(),
            inherited,
            manual_summary,
            resolved: vec![None; n],
            fixed_start_cap,
        }
    }

    fn manual_summary(&self, i: usize) -> Option<(DateTime, DateTime)> {
        self.manual_summary.get(i).copied().flatten()
    }

    fn inherited(&self, i: usize) -> Inherited {
        self.inherited.get(i).copied().unwrap_or_default()
    }

    fn inherited_finish_cap_warning(&self, cap: SummaryFinishCap) -> String {
        inherited_finish_cap_warning(self.p, cap)
    }

    /// Whether a backward-pass or slack calculation should follow `edge` from leaf `i`.
    ///
    /// An inactive successor does not affect how other tasks are scheduled. A link from a
    /// summary's start constrains only the subtask that sets that start (the earliest), not
    /// every subtask, or all of them would get the same late start and negative slack.
    /// `group_start[e]` is the earliest start of a summary-start edge's group (see
    /// [`Engine::summary_group_starts`]).
    fn edge_constrains(&self, e: usize, edge: &Edge, i: usize, group_start: &[Option<DateTime>]) -> bool {
        if self.task(edge.succ).is_none_or(|t| !t.active) {
            return false;
        }
        match &edge.pred {
            PredSource::SummaryStart { .. } => {
                let earliest = group_start.get(e).copied().flatten();
                earliest.is_some() && self.date(i).es == earliest
            }
            _ => true,
        }
    }

    /// The earliest start within each summary-start edge's group (early dates are final once the
    /// forward pass is done), so each edge's group is scanned once per pass, not once per leaf.
    fn summary_group_starts(&self) -> Vec<Option<DateTime>> {
        self.edges
            .iter()
            .map(|edge| match &edge.pred {
                PredSource::SummaryStart { leaves, starts, .. } => {
                    leaves.iter().filter_map(|&g| self.date(g).es).chain(starts.iter().copied()).min()
                }
                _ => None,
            })
            .collect()
    }

    fn task(&self, i: usize) -> Option<&'a Task> {
        self.p.tasks.get(i)
    }

    fn leaf(&self, i: usize) -> bool {
        self.is_leaf.get(i).copied().unwrap_or(false)
    }

    fn cal(&self, i: usize) -> &WorkCal<'a> {
        self.cal_of.get(i).and_then(|&c| self.cals.get(c)).unwrap_or(&self.proj_cal)
    }

    fn date(&self, i: usize) -> Dates {
        self.dates.get(i).copied().unwrap_or_default()
    }

    fn set_early(&mut self, i: usize, es: DateTime, ef: DateTime) {
        if let Some(d) = self.dates.get_mut(i) {
            d.es = Some(es);
            d.ef = Some(ef);
        }
    }

    fn set_parts(&mut self, i: usize, parts: Option<Parts>) {
        if let Some(d) = self.dates.get_mut(i) {
            d.parts = parts;
        }
    }

    fn set_late(&mut self, i: usize, ls: DateTime, lf: DateTime) {
        if let Some(d) = self.dates.get_mut(i) {
            d.ls = Some(ls);
            d.lf = Some(lf);
        }
    }

    fn incoming(&self, i: usize) -> &[usize] {
        self.incoming.get(i).map(Vec::as_slice).unwrap_or_default()
    }

    fn outgoing(&self, i: usize) -> &[usize] {
        self.outgoing.get(i).map(Vec::as_slice).unwrap_or_default()
    }

    /// The leaf itself, or a summary's active leaf subtasks (inactive ones drive nothing).
    fn leaf_descendants(&self, idx: usize) -> Vec<usize> {
        if self.leaf(idx) {
            return vec![idx];
        }
        self.p.descendants_range(idx).filter(|&i| self.leaf(i) && self.task(i).is_some_and(|t| t.active)).collect()
    }

    fn resolve_pred(&mut self, r: &TaskRef, kind: LinkType, succ_name: &str) -> Vec<PredSource> {
        match r {
            TaskRef::Local(id) => {
                let Some(&pi) = self.position.get(id) else {
                    self.warnings.push((None, format!("'{succ_name}' references missing predecessor {id}")));
                    return Vec::new();
                };
                if self.task(pi).is_none_or(|t| !t.active) {
                    return Vec::new();
                }
                // A manually scheduled summary's successors follow its own (typed) dates.
                if let Some((start, finish)) = self.manual_summary(pi) {
                    return vec![PredSource::Fixed { start, finish, duration: self.proj_cal.between(start, finish) }];
                }
                let leaves = self.leaf_descendants(pi);
                let pred_is_start = matches!(kind, LinkType::SS | LinkType::SF);
                if !self.leaf(pi) {
                    if let Some(anchor) = self.task(pi).and_then(|t| t.side_anchor(!pred_is_start)) {
                        let other_anchor = self.task(pi).and_then(|t| t.side_anchor(pred_is_start));
                        return vec![PredSource::SummaryAnchored { leaves, anchor, fixed_finish: !pred_is_start, other_anchor }];
                    }
                    if pred_is_start {
                        let starts = self
                            .p
                            .descendants_range(pi)
                            .filter_map(|i| self.task(i).filter(|t| !self.leaf(i) && t.active)?.side_anchor(false))
                            .collect();
                        let finishes = self
                            .p
                            .descendants_range(pi)
                            .filter_map(|i| self.task(i).filter(|t| !self.leaf(i) && t.active)?.side_anchor(true))
                            .collect();
                        return vec![PredSource::SummaryStart { leaves, starts, finishes }];
                    }
                }
                let mut sources: Vec<_> = leaves.into_iter().map(PredSource::Leaf).collect();
                if !self.leaf(pi) {
                    // A dynamic outer finish includes anchored inner groups, not just raw leaves.
                    for i in self.p.descendants_range(pi) {
                        let Some(group) = self.task(i).filter(|t| !self.leaf(i) && t.active) else { continue };
                        if let Some(anchor) = group.side_anchor(true) {
                            sources.push(PredSource::SummaryAnchored {
                                leaves: self.leaf_descendants(i),
                                anchor,
                                fixed_finish: true,
                                other_anchor: group.side_anchor(false),
                            });
                        }
                    }
                }
                sources
            }
            TaskRef::External { project, task } => {
                let snap = self.p.external(*project).and_then(|e| e.task(*task));
                match snap {
                    Some(s) => match (s.start, s.finish) {
                        (Some(start), Some(finish)) => vec![PredSource::Fixed { start, finish, duration: s.duration }],
                        _ => {
                            self.warnings.push((None, format!("External predecessor '{}' of '{succ_name}' has no dates", s.name)));
                            Vec::new()
                        }
                    },
                    None => {
                        self.warnings.push((None, format!("'{succ_name}' references unknown external task {task}")));
                        Vec::new()
                    }
                }
            }
        }
    }

    fn build_edges(&mut self) {
        let p = self.p;
        // A finish-to-finish or start-to-finish link into an automatic summary would set the
        // summary's finish, which comes from its subtasks; such links (from older files, or on a
        // task that later became a summary) are ignored with a warning instead of being pushed
        // onto every subtask.
        for (i, t) in p.tasks.iter().enumerate() {
            if self.leaf(i) || self.manual_summary(i).is_some() || !t.active {
                continue;
            }
            for d in t.predecessors.iter().filter(|d| matches!(d.kind, LinkType::FF | LinkType::SF)) {
                let from = match d.predecessor {
                    TaskRef::Local(id) => {
                        self.position.get(&id).and_then(|&pi| self.task(pi)).map_or_else(|| id.to_string(), |pt| pt.name.clone())
                    }
                    TaskRef::External { task, .. } => task.to_string(),
                };
                self.warnings.push((
                    None,
                    format!(
                        "The {} link from '{from}' to summary task '{}' is ignored: a summary task's finish comes from its subtasks. Link to one of its subtasks instead.",
                        d.kind.code(),
                        t.name
                    ),
                ));
            }
        }
        for succ in self.leaves.clone() {
            let Some(task) = self.task(succ) else { continue };
            // Own links (no summary cap), then links inherited from ancestor summaries (capped by
            // that summary's fixedStartCap), then milestone membership.
            let mut deps: Vec<(Dependency, Option<DateTime>)> =
                task.predecessors.iter().copied().map(|d| (d, None)).collect();
            // Links into a manually scheduled summary stop at it (it doesn't move, and its
            // subtasks follow its start instead), and so do links into the summaries around it.
            let mut anc = self.parent.get(succ).copied().flatten();
            while let Some(a) = anc {
                if self.manual_summary(a).is_some() {
                    break;
                }
                if let Some(t) = p.tasks.get(a) {
                    let cap = self.fixed_start_cap.get(a).copied().flatten();
                    deps.extend(
                        t.predecessors
                            .iter()
                            .copied()
                            .filter(|d| !matches!(d.kind, LinkType::FF | LinkType::SF))
                            .map(|d| (d, cap)),
                    );
                }
                anc = self.parent.get(a).copied().flatten();
            }
            deps.extend(task.members.iter().map(|m| (Dependency::fs(*m), None)));
            for (d, via_summary_cap) in deps {
                for src in self.resolve_pred(&d.predecessor, d.kind, &task.name) {
                    if let PredSource::Leaf(pi) = src
                        && pi == succ
                    {
                        continue;
                    }
                    let e = self.edges.len();
                    let preds: &[usize] = match &src {
                        PredSource::Leaf(pi) => std::slice::from_ref(pi),
                        PredSource::SummaryStart { leaves, .. } => leaves,
                        PredSource::SummaryAnchored { leaves, .. } => leaves,
                        PredSource::Fixed { .. } => &[],
                    };
                    for pi in preds {
                        if let Some(out) = self.outgoing.get_mut(*pi) {
                            out.push(e);
                        }
                    }
                    if let Some(inc) = self.incoming.get_mut(succ) {
                        inc.push(e);
                    }
                    self.edges.push(Edge {
                        pred: src,
                        succ,
                        kind: d.kind,
                        lag: d.lag,
                        drive: d.drive,
                        via_summary_cap,
                    });
                }
            }
        }
    }

    fn pred_count(edge: &Edge) -> usize {
        match &edge.pred {
            PredSource::Leaf(_) => 1,
            PredSource::SummaryStart { leaves, .. } => leaves.len(),
            PredSource::SummaryAnchored { leaves, .. } => leaves.len(),
            PredSource::Fixed { .. } => 0,
        }
    }

    /// Kahn's algorithm over leaves. Cycles are reported and broken by index order.
    fn topo_sort(&mut self) {
        let n = self.p.tasks.len();
        let mut indeg = vec![0usize; n];
        for e in &self.edges {
            if let Some(d) = indeg.get_mut(e.succ) {
                *d += Self::pred_count(e);
            }
        }
        let mut ready: std::collections::VecDeque<usize> =
            self.leaves.iter().copied().filter(|&i| indeg.get(i).copied() == Some(0)).collect();
        let mut done = vec![false; n];
        let mut order = Vec::with_capacity(self.leaves.len());
        loop {
            while let Some(i) = ready.pop_front() {
                match done.get_mut(i) {
                    Some(d) if !*d => *d = true,
                    _ => continue,
                }
                order.push(i);
                for &e in self.outgoing(i) {
                    let Some(s) = self.edges.get(e).map(|e| e.succ) else { continue };
                    if let Some(d) = indeg.get_mut(s) {
                        *d = d.saturating_sub(1);
                        if *d == 0 && done.get(s) == Some(&false) {
                            ready.push_back(s);
                        }
                    }
                }
            }
            if order.len() >= self.leaves.len() {
                break;
            }
            // Cycle: force the first remaining leaf.
            let not_done = |i: &&usize| done.get(**i) == Some(&false);
            let Some(&stuck) = self.leaves.iter().find(not_done) else {
                break;
            };
            let cycle_names: Vec<String> =
                self.leaves.iter().filter(not_done).take(6).filter_map(|&i| self.task(i)).map(|t| t.name.clone()).collect();
            self.warnings.push((None, format!("Circular dependency involving: {}", cycle_names.join(", "))));
            if let Some(d) = indeg.get_mut(stuck) {
                *d = 0;
            }
            ready.push_back(stuck);
        }
        self.order = order;
    }

    fn pred_dates(&self, src: &PredSource) -> Option<(DateTime, DateTime, Minutes)> {
        match src {
            PredSource::Leaf(i) => {
                let d = self.date(*i);
                Some((d.es?, d.ef?, self.duration_of(*i)))
            }
            PredSource::SummaryStart { leaves, starts, finishes } => {
                let start = leaves.iter().filter_map(|i| self.date(*i).es).chain(starts.iter().copied()).min()?;
                let finish = leaves.iter().filter_map(|i| self.date(*i).ef).chain(finishes.iter().copied()).max()?;
                Some((start, finish, self.proj_cal.between(start, finish)))
            }
            PredSource::SummaryAnchored { leaves, anchor, fixed_finish, other_anchor } => {
                let dyn_start = leaves.iter().filter_map(|i| self.date(*i).es).min().unwrap_or(*anchor);
                let dyn_finish = leaves.iter().filter_map(|i| self.date(*i).ef).max().unwrap_or(*anchor);
                let (start, finish) = if *fixed_finish {
                    (other_anchor.unwrap_or(dyn_start), *anchor)
                } else {
                    (*anchor, other_anchor.unwrap_or(dyn_finish))
                };
                Some((start, finish, self.proj_cal.between(start, finish).max(0)))
            }
            PredSource::Fixed { start, finish, duration } => Some((*start, *finish, *duration)),
        }
    }

    fn duration_of(&self, i: usize) -> Minutes {
        if let Some(minutes) = self.resolved.get(i).copied().flatten() {
            return minutes.max(0);
        }
        match self.task(i) {
            Some(t) if !t.is_milestone() => t.duration.max(0),
            _ => 0,
        }
    }

    fn forward(&mut self, alap_pins: &HashMap<usize, DateTime>, warn: bool) {
        let p = self.p;
        let project_start = p.start;
        for i in self.order.clone() {
            let Some(t) = self.task(i) else { continue };
            let cal = self.cal(i);
            let dur = self.duration_of(i);
            // An elapsed duration runs around the clock, ignoring working time altogether: it starts
            // as soon as it may (a cure starts when the pour ends, at 17:00 on a Friday) and
            // finishes the elapsed time later (see `along`).
            let elapsed = t.elapsed && dur > 0;
            // Working time from start to finish: the duration plus any planned splits.
            let span = if t.mode == ScheduleMode::Auto && !elapsed { span_of(t, dur) } else { dur };
            // `from` moved by the task's span: forward (finish from start), or back (start from finish).
            let along = |from: DateTime, forward: bool| match (elapsed, forward) {
                (true, true) => shift(from, span),
                (true, false) => shift(from, span.saturating_neg()),
                (false, true) => cal.add(from, span),
                (false, false) => cal.sub(from, span),
            };

            let place = |start: DateTime| -> (DateTime, DateTime) {
                if elapsed {
                    (start, along(start, true))
                } else if dur > 0 {
                    let s = cal.snap_forward(start);
                    (s, along(s, true))
                } else {
                    let s = cal.snap_event(start);
                    (s, s)
                }
            };

            if t.mode == ScheduleMode::Manual {
                // Actual dates win over typed ones, at both ends. A task that started on another
                // day than typed (and hasn't finished) keeps its length from its actual start: its
                // typed finish moves with it instead of squeezing or stretching the task. The
                // length is that of its typed dates when both were typed (typing a date doesn't
                // change the Duration field), else its duration.
                let start = t.actual_start.or(t.manual_start).unwrap_or(project_start);
                let from_start = || if dur > 0 { along(start, true) } else { start };
                let finish = match (t.actual_finish, t.actual_start) {
                    (Some(f), _) => f,
                    (None, Some(a)) if Some(a) != t.manual_start => match (t.manual_start, t.manual_finish) {
                        (Some(ms), Some(mf)) if mf > ms => {
                            if t.elapsed {
                                shift(start, crate::dates::minutes_between(ms, mf))
                            } else {
                                cal.add(start, cal.between(ms, mf))
                            }
                        }
                        _ => from_start(),
                    },
                    _ => t.manual_finish.unwrap_or_else(from_start),
                };
                let finish = finish.max(start);
                if warn
                    && let Some(cap) = self.inherited(i).finish_cap
                    && finish > cap.date
                {
                    self.warnings.push((Some(i), self.inherited_finish_cap_warning(cap)));
                }
                self.set_early(i, start, finish);
                continue;
            }

            let mut es = project_start;
            let mut driven_by_link = false;
            let inherited = self.inherited(i);
            for &e in self.incoming(i) {
                let Some(edge) = self.edges.get(e) else { continue };
                // Successor-driven links pull the predecessor after this pass; they must not push
                // the successor's early start here.
                if edge.drive == LinkDrive::FromSuccessor {
                    continue;
                }
                let Some((ps, pf, pdur)) = self.pred_dates(&edge.pred) else {
                    continue;
                };
                let mut cand_start = match edge.kind {
                    LinkType::FS => apply_lag(&self.proj_cal, pf, edge.lag, pdur),
                    LinkType::SS => apply_lag(&self.proj_cal, ps, edge.lag, pdur),
                    LinkType::FF => along(apply_lag(&self.proj_cal, pf, edge.lag, pdur), false),
                    LinkType::SF => along(apply_lag(&self.proj_cal, ps, edge.lag, pdur), false),
                };
                // Inherited summary deps drive through the group's capped start (Swift fixedStartCap).
                if let Some(cap) = edge.via_summary_cap
                    && cand_start > cap
                {
                    cand_start = cap;
                }
                if cand_start > es {
                    es = cand_start;
                    driven_by_link = true;
                }
            }

            // A Start No Earlier Than date on a summary drives its subtasks' starts.
            if let Some(snet) = inherited.snet {
                es = es.max(snet);
            }

            // Nested share placement floors the start like a driven edge (parent window Ws / ALAP
            // We−L). Apply *before* hard date pullback so own/ancestor fixed finishes still win
            // (Merlin: fixed deadline overrides driven start). Applying this after
            // `apply_date_bounds` let a share floor undo Finish NLT / On and slide the bar.
            if let Some(&pin) = alap_pins.get(&i)
                && pin > es
            {
                es = pin;
                driven_by_link = true;
            }

            let mut warnings = Vec::new();
            if let Some(date) = t.constraint.date.filter(|_| t.constraint.kind.needs_date()) {
                let finish_of = |s: DateTime| place(s).1;
                match t.constraint.kind {
                    ConstraintKind::Snet => es = es.max(date),
                    ConstraintKind::Fnet => es = es.max(along(date, false)),
                    ConstraintKind::Mso => {
                        if warn && driven_by_link && place(es).0 > place(date).0 {
                            warnings.push("Links require a later start than its Must Start On date".to_string());
                        }
                        es = date;
                    }
                    ConstraintKind::Mfo => {
                        let s = if dur > 0 { along(date, false) } else { date };
                        if warn && driven_by_link && place(es).0 > place(s).0 {
                            warnings.push("Links require a later finish than its Must Finish On date".to_string());
                        }
                        es = s;
                    }
                    ConstraintKind::Snlt => {
                        if place(es).0 > date {
                            if warn {
                                warnings.push("Cannot start by its Start No Later Than date".to_string());
                            }
                            es = date;
                        }
                    }
                    ConstraintKind::Fnlt => {
                        if finish_of(es) > date {
                            if warn {
                                warnings.push("Cannot finish by its Finish No Later Than date".to_string());
                            }
                            es = if dur > 0 { along(date, false) } else { date };
                        }
                    }
                    ConstraintKind::Asap | ConstraintKind::Alap => {}
                }
            }

            es = apply_date_bounds(t, es, dur, &along, warn, &mut warnings);

            // Own fixedStartCap only. Ancestor caps reach unrestricted siblings through
            // inherited summary edges (`via_summary_cap`), not as a blanket upper bound —
            // otherwise an in-group MSO would wrongly pull every sibling earlier.
            if let Some(cap) = self.fixed_start_cap.get(i).copied().flatten()
                && es > cap
            {
                if warn && driven_by_link {
                    warnings.push("Cannot start by a fixed no-later-than date inside its group".to_string());
                }
                es = match t.bounds.start_earliest {
                    Some(floor) => cap.max(floor),
                    None => cap,
                };
            }

            let mut ancestor_cap_warned = false;
            if let Some(cap) = inherited.finish_cap {
                let latest_start = if dur > 0 { along(cap.date, false) } else { cap.date };
                if es > latest_start {
                    if warn {
                        warnings.push(self.inherited_finish_cap_warning(cap));
                        ancestor_cap_warned = true;
                    }
                    let keep_own_hard_finish = own_hard_finish_pin(t).is_some_and(|date| date > cap.date);
                    if !keep_own_hard_finish {
                        let floor = inherited.snet.max(t.bounds.start_earliest);
                        es = floor.map_or(latest_start, |f| latest_start.max(f));
                    }
                }
            }

            // Own delay plus ancestor summary delays (nested group delay shifts children ES).
            let mut delay = t.leveling_delay.max(0);
            let mut anc = self.parent.get(i).copied().flatten();
            while let Some(pi) = anc {
                delay = delay.saturating_add(self.task(pi).map(|a| a.leveling_delay.max(0)).unwrap_or(0));
                anc = self.parent.get(pi).copied().flatten();
            }
            if delay > 0 {
                es = cal.add(cal.snap_forward(es), delay);
            }

            let (mut s, mut f) = place(es);
            let mut parts = parts_of(t, cal, dur);
            if let Some(a) = t.actual_start {
                // A positioned nested share moves its children with the allotted block. This
                // is a group-window floor, not ALAP inherited by the leaf: the leaf remains
                // ASAP/own-placement inside that moved window.
                s = alap_pins.get(&i).copied().map_or(a, |pin| a.max(pin));
                if s != a {
                    // Progress parts were calculated from the old actual start. Translate every
                    // derived point with the moved group window so start/finish cannot invert.
                    let shift = cal.between(a, s);
                    parts = parts.map(|mut pt| {
                        pt.stop = cal.add(pt.stop, shift);
                        pt.resume = cal.add(pt.resume, shift);
                        pt.finish = cal.add(pt.finish, shift);
                        pt.last.0 = cal.add(pt.last.0, shift);
                        pt
                    });
                }
                f = match parts {
                    Some(pt) => pt.finish,
                    None if dur > 0 => along(s, true),
                    None => s,
                };
            }
            if let Some(af) = t.actual_finish {
                if t.actual_start.is_none() {
                    s = if dur > 0 { along(af, false) } else { af };
                }
                f = af.max(s);
            }
            if warn
                && let Some(cap) = inherited.finish_cap
                && f > cap.date
                && !ancestor_cap_warned
            {
                warnings.push(self.inherited_finish_cap_warning(cap));
            }
            let segments: Vec<(DateTime, DateTime)> = match (parts, t.actual_start, t.actual_finish) {
                (Some(pt), Some(_), _) if pt.split => {
                    segments_of(s, &t.splits, &t.gaps, cal, dur).into_iter().map(|(s, f, _)| (s, f)).collect()
                }
                // Planned splits in a task not under way (or finished as planned): its parts from its start.
                (None, _, None) if span > dur => {
                    let segs = segments_of(s, &[], &t.gaps, cal, dur);
                    if segs.len() > 1 { segs.into_iter().map(|(s, f, _)| (s, f)).collect() } else { Vec::new() }
                }
                _ => Vec::new(),
            };
            // The task's own warnings: a pinned no-later-than or must-on date that its links
            // cannot meet. They go to the task so its row shows an indicator.
            self.warnings.extend(warnings.into_iter().map(|w| (Some(i), w)));
            self.set_parts(i, parts);
            if segments.is_empty() {
                self.segments.remove(&i);
            } else {
                self.segments.insert(i, segments);
            }
            self.set_early(i, s, f);
        }
    }

    /// Pull predecessor early dates toward successors for [`LinkDrive::FromSuccessor`] edges.
    /// Runs after [`Self::forward`] so successor early dates (pins, constraints) are settled.
    fn pull_successor_driven(&mut self) {
        for e in (0..self.edges.len()).rev() {
            let Some(edge) = self.edges.get(e).cloned() else { continue };
            if edge.drive != LinkDrive::FromSuccessor {
                continue;
            }
            let Some(succ) = self.task(edge.succ) else { continue };
            if !succ.active {
                continue;
            }
            let (Some(succ_es), Some(succ_ef)) = (self.date(edge.succ).es, self.date(edge.succ).ef) else {
                continue;
            };
            let leaves: Vec<usize> = match &edge.pred {
                PredSource::Leaf(pi) => vec![*pi],
                PredSource::SummaryStart { leaves, .. } | PredSource::SummaryAnchored { leaves, .. } => leaves.clone(),
                PredSource::Fixed { .. } => continue,
            };
            for pi in leaves {
                let Some(pred) = self.task(pi) else { continue };
                if pred.mode == ScheduleMode::Manual || pred.actual_finish.is_some() || !pred.active {
                    continue;
                }
                if self.p.has_children(pi) {
                    continue;
                }
                let (Some(cur_es), Some(_cur_ef)) = (self.date(pi).es, self.date(pi).ef) else {
                    continue;
                };
                let cal = self.cal(pi);
                let dur = self.duration_of(pi);
                let elapsed = pred.elapsed && dur > 0;
                let span = if pred.mode == ScheduleMode::Auto && !elapsed { span_of(pred, dur) } else { dur };
                let along = |from: DateTime, forward: bool| match (elapsed, forward) {
                    (true, true) => shift(from, span),
                    (true, false) => shift(from, span.saturating_neg()),
                    (false, true) => cal.add(from, span),
                    (false, false) => cal.sub(from, span),
                };
                let (aligned_es, aligned_ef) = match edge.kind {
                    LinkType::FS => {
                        let ef = unapply_lag(&self.proj_cal, succ_es, edge.lag, dur);
                        let es = if dur > 0 { along(ef, false) } else { ef };
                        (es, ef)
                    }
                    LinkType::SS => {
                        let es = unapply_lag(&self.proj_cal, succ_es, edge.lag, dur);
                        let ef = if dur > 0 { along(es, true) } else { es };
                        (es, ef)
                    }
                    LinkType::FF => {
                        let ef = unapply_lag(&self.proj_cal, succ_ef, edge.lag, dur);
                        let es = if dur > 0 { along(ef, false) } else { ef };
                        (es, ef)
                    }
                    LinkType::SF => {
                        let es = unapply_lag(&self.proj_cal, succ_ef, edge.lag, dur);
                        let ef = if dur > 0 { along(es, true) } else { es };
                        (es, ef)
                    }
                };
                // Only slide later toward the successor so own predecessor floors stay intact.
                if aligned_es > cur_es {
                    let (es, ef) = if elapsed {
                        (aligned_es, aligned_ef)
                    } else if dur > 0 {
                        let s = cal.snap_forward(aligned_es);
                        (s, along(s, true))
                    } else {
                        let s = cal.snap_event(aligned_es);
                        (s, s)
                    };
                    self.set_early(pi, es, ef);
                }
            }
        }
    }

    /// The earliest start and latest finish of the active leaves and manually scheduled summaries.
    fn project_bounds(&self) -> (Option<DateTime>, Option<DateTime>) {
        let active = |i: &usize| self.task(*i).is_some_and(|t| t.active);
        let leaves = self.leaves.iter().copied().filter(active).filter_map(|i| self.date(i).es.zip(self.date(i).ef));
        let manual = (0..self.manual_summary.len()).filter(active).filter_map(|i| self.manual_summary(i));
        let dates: Vec<(DateTime, DateTime)> = leaves.chain(manual).collect();
        (dates.iter().map(|d| d.0).min(), dates.iter().map(|d| d.1).max())
    }

    fn backward(&mut self) {
        let p = self.p;
        let (_, finish) = self.project_bounds();
        let project_finish = finish.unwrap_or(p.start);
        let group_start = self.summary_group_starts();
        for i in self.order.clone().into_iter().rev() {
            let Some(t) = self.task(i) else { continue };
            let cal = self.cal(i);
            let dur = self.duration_of(i);
            let elapsed = t.elapsed && dur > 0;
            let span = if t.mode == ScheduleMode::Auto && !elapsed { span_of(t, dur) } else { dur };
            let along = |from: DateTime, forward: bool| match (elapsed, forward) {
                (true, true) => shift(from, span),
                (true, false) => shift(from, span.saturating_neg()),
                (false, true) => cal.add(from, span),
                (false, false) => cal.sub(from, span),
            };
            let (Some(es), Some(ef)) = (self.date(i).es, self.date(i).ef) else {
                continue;
            };

            if t.mode == ScheduleMode::Manual || t.actual_finish.is_some() {
                self.set_late(i, es, ef);
                continue;
            }

            let mut lf = project_finish.max(ef);
            // Deadlines (the task's own and its summaries') limit the late finish, so they
            // reduce total slack; they don't move the task.
            let inherited = self.inherited(i);
            if let Some(d) = min_opt(t.deadline, inherited.deadline) {
                lf = lf.min(d);
            }
            if let Some(cap) = inherited.finish_cap {
                lf = lf.min(cap.date);
            }
            if let Some(d) = t.bounds.finish_latest {
                lf = lf.min(d);
            }
            if let Some(d) = t.bounds.start_latest {
                let implied = if dur > 0 { along(d, true) } else { d };
                lf = lf.min(implied);
            }
            for &e in self.outgoing(i) {
                let Some(edge) = self.edges.get(e) else { continue };
                if !self.edge_constrains(e, edge, i, &group_start) {
                    continue;
                }
                let (Some(sls), Some(slf)) = (self.date(edge.succ).ls, self.date(edge.succ).lf) else {
                    continue;
                };
                let cand_lf = match edge.kind {
                    LinkType::FS => unapply_lag(&self.proj_cal, sls, edge.lag, dur),
                    LinkType::FF => unapply_lag(&self.proj_cal, slf, edge.lag, dur),
                    LinkType::SS => along(unapply_lag(&self.proj_cal, sls, edge.lag, dur), true),
                    LinkType::SF => along(unapply_lag(&self.proj_cal, slf, edge.lag, dur), true),
                };
                lf = lf.min(cand_lf);
            }
            if let Some(date) = t.constraint.date.filter(|_| t.constraint.kind.needs_date()) {
                match t.constraint.kind {
                    ConstraintKind::Snlt => lf = lf.min(if dur > 0 { along(date, true) } else { date }),
                    ConstraintKind::Fnlt => lf = lf.min(date),
                    ConstraintKind::Mso | ConstraintKind::Mfo => lf = ef,
                    _ => {}
                }
            }
            // Elapsed time may end outside working time (a Saturday morning, say).
            let lf = if elapsed { lf } else { cal.snap_backward(lf) };
            // A split task's last part is what can still move: its Late Start is that part's.
            let ls = match self.date(i).parts {
                Some(pt) if pt.split => cal.sub(lf, pt.last.1),
                _ if dur > 0 => along(lf, false),
                _ => lf,
            };
            self.set_late(i, ls, lf);
        }
    }

    /// Total and free slack of every leaf (indexed like the tasks).
    fn slack(&self) -> (Vec<Minutes>, Vec<Minutes>) {
        let n = self.p.tasks.len();
        let mut total_slack = vec![0; n];
        let mut free_slack = vec![0; n];
        let group_start = self.summary_group_starts();
        for &i in &self.order {
            let d = self.date(i);
            let cal = self.cal(i);
            let (Some(es), Some(ef), Some(ls), Some(lf)) = (d.es, d.ef, d.ls, d.lf) else {
                continue;
            };
            let ts = match d.parts {
                // The start of a split task is fixed; only its remaining part can slip.
                Some(pt) if pt.split => cal.between(ef, lf),
                _ => cal.between(es, ls).min(cal.between(ef, lf)),
            };
            let dur = self.duration_of(i);
            let mut fs = Minutes::MAX;
            for &e in self.outgoing(i) {
                let Some(edge) = self.edges.get(e) else { continue };
                if !self.edge_constrains(e, edge, i, &group_start) {
                    continue;
                }
                let sd = self.date(edge.succ);
                let (Some(ses), Some(sef)) = (sd.es, sd.ef) else {
                    continue;
                };
                let gap = match edge.kind {
                    LinkType::FS => cal.between(apply_lag(&self.proj_cal, ef, edge.lag, dur), ses),
                    LinkType::SS => cal.between(apply_lag(&self.proj_cal, es, edge.lag, dur), ses),
                    LinkType::FF => cal.between(apply_lag(&self.proj_cal, ef, edge.lag, dur), sef),
                    LinkType::SF => cal.between(apply_lag(&self.proj_cal, es, edge.lag, dur), sef),
                };
                fs = fs.min(gap);
            }
            if let Some(slot) = total_slack.get_mut(i) {
                *slot = ts;
            }
            if let Some(slot) = free_slack.get_mut(i) {
                *slot = if fs == Minutes::MAX { ts } else { fs.max(0).min(ts.max(0)) };
            }
        }
        (total_slack, free_slack)
    }
}

/// Re-derives work, actual work and cost of every leaf task from assignments and time entries.
fn rollup_tracking(p: &mut Project) {
    let mut entry_by_task: HashMap<TaskId, Minutes> = HashMap::new();
    for e in &p.time_entries {
        let slot = entry_by_task.entry(e.task).or_default();
        *slot = slot.saturating_add(e.minutes);
    }
    let resources: HashMap<ResourceId, &Resource> = p.resources.iter().map(|r| (r.id, r)).collect();
    // (work, actual work, cost, has a work assignment) per task.
    let mut by_task: HashMap<TaskId, (Minutes, Minutes, f64, bool)> = HashMap::new();
    for a in &p.assignments {
        let Some(r) = resources.get(&a.resource) else {
            continue;
        };
        let slot = by_task.entry(a.task).or_default();
        slot.1 = slot.1.saturating_add(a.actual_work);
        slot.2 += a.cost_with(r);
        if r.kind == ResourceType::Work {
            slot.3 = true;
            slot.0 = slot.0.saturating_add(a.work);
        }
    }
    let settings = p.settings.clone();
    for t in &mut p.tasks {
        let (mut work, assigned_actual, assigned_cost, has_work_assignment) = by_task.get(&t.id).copied().unwrap_or_default();
        let actual = entry_by_task.get(&t.id).copied().unwrap_or(0).saturating_add(assigned_actual);
        // A summary's work is its subtasks' (plus any assignments of its own), never an estimate.
        if !has_work_assignment
            && !t.sched.is_summary
            && let Some(est) = &t.estimate
        {
            work = est.expected_work(&settings);
        }
        t.sched.work = work;
        t.sched.actual_work = actual;
        t.sched.cost = t.fixed_cost + assigned_cost;
        t.sched.percent_complete = if settings.progress_from_time && work > 0 && t.actual_finish.is_none() {
            ((actual as f64 / work as f64) * 100.0).min(99.0) as f32
        } else {
            f32::from(t.percent_complete)
        };
    }
}

/// Whether the plan's links (summaries expanded, as the scheduler follows them) go round in a
/// circle somewhere. `false` if the check itself fails.
pub(crate) fn has_cycle(p: &Project) -> bool {
    guard::catch("cycle check", || {
        let mut eng = Engine::new(p);
        eng.build_edges();
        eng.topo_sort();
        eng.warnings.iter().any(|(_, w)| w.starts_with("Circular dependency"))
    })
    .unwrap_or(false)
}

/// Recalculates the whole schedule in place.
///
/// Never panics: if the engine hits a bug, the project keeps its previous dates and gets a
/// warning saying so (see [`try_recalculate`] to roll back instead).
pub fn recalculate(p: &mut Project) -> ScheduleReport {
    match try_recalculate(p) {
        Ok(report) => report,
        Err(e) => {
            let msg =
                format!("Scheduling stopped because of an internal error ({}); dates may be out of date. Please report this.", e.message);
            p.sched.warnings = vec![msg.clone()];
            ScheduleReport { warnings: vec![msg], ..ScheduleReport::default() }
        }
    }
}

/// Recalculates the schedule, turning an engine panic into an error.
///
/// On error the computed fields may be half-updated; `Document` edits restore their snapshot.
pub fn try_recalculate(p: &mut Project) -> Result<ScheduleReport, Panicked> {
    guard::catch("recalculate", || calculate(p))
}

/// When a successor is pinned by a fixed bound yet a predecessor still finishes after that
/// successor starts, both rows get a warning. Share resolution normally prevents this; the
/// leftover case is an impossible window that could not be compressed further.
fn flag_pinned_successor_overruns(p: &mut Project) {
    let n = p.tasks.len();
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for succ_i in 0..n {
        let Some(succ) = p.tasks.get(succ_i) else { continue };
        if !successor_is_pinned(succ) {
            continue;
        }
        let Some(succ_start) = succ.sched.start else { continue };
        for dep in &succ.predecessors {
            if dep.kind != LinkType::FS {
                continue;
            }
            let Some(pred_id) = dep.predecessor.local() else { continue };
            let Some(pred_i) = p.tasks.iter().position(|t| t.id == pred_id) else { continue };
            let Some(pred) = p.tasks.get(pred_i) else { continue };
            let Some(pred_finish) = pred.sched.finish else { continue };
            if pred_finish > succ_start {
                hits.push((pred_i, succ_i));
            }
        }
    }
    for (pred_i, succ_i) in hits {
        let succ_name = p.tasks.get(succ_i).map(|t| t.name.clone()).unwrap_or_default();
        let pred_name = p.tasks.get(pred_i).map(|t| t.name.clone()).unwrap_or_default();
        if let Some(pred) = p.tasks.get_mut(pred_i) {
            let msg = format!("Finishes after successor '{succ_name}' which is pinned by a fixed date");
            if !pred.sched.warnings.iter().any(|w| w == &msg) {
                pred.sched.warnings.push(msg);
            }
        }
        if let Some(succ) = p.tasks.get_mut(succ_i) {
            let msg = format!("Pinned date overrides the finish-to-start link from '{pred_name}'");
            if !succ.sched.warnings.iter().any(|w| w == &msg) {
                succ.sched.warnings.push(msg);
            }
        }
    }
}

fn successor_is_pinned(t: &Task) -> bool {
    if t.bounds.start_latest_fixed && t.bounds.start_latest.is_some() {
        return true;
    }
    if t.bounds.finish_latest_fixed && t.bounds.finish_latest.is_some() {
        return true;
    }
    matches!(t.constraint.kind, ConstraintKind::Mso | ConstraintKind::Mfo | ConstraintKind::Snlt | ConstraintKind::Fnlt)
        && t.constraint.date.is_some()
}

fn calculate(p: &mut Project) -> ScheduleReport {
    for t in &mut p.tasks {
        t.outline_level = t.outline_level.max(1);
    }
    let summary: Vec<bool> = (0..p.tasks.len()).map(|i| p.has_children(i)).collect();
    for (t, s) in p.tasks.iter_mut().zip(&summary) {
        t.sched.is_summary = *s;
    }
    rollup_tracking(p);

    let mut share = if crate::shares::needed(p) { crate::shares::resolve(p) } else { crate::shares::SharePlan::default() };
    // Paired On/On (and hard start-floor + finish-deadline) set working duration before CPM (Swift dateDurationBounds).
    apply_date_driven_durations(p, &mut share.durations);

    let (dates, eng_warnings, total_slack, free_slack, mut segments) = {
        let mut eng = Engine::new(p);
        eng.resolved.clone_from(&share.durations);
        eng.build_edges();
        eng.topo_sort();
        eng.forward(&HashMap::new(), false);
        eng.pull_successor_driven();
        eng.backward();
        // As-late-as-possible tasks are pinned to their late start and the forward pass is repeated
        // so successors stay consistent.
        let parent_idx = parents(p);
        let mut pins: HashMap<usize, DateTime> = eng
            .order
            .iter()
            .copied()
            .filter(|&i| {
                eng.task(i).is_some_and(|t| {
                    t.mode == ScheduleMode::Auto
                        && t.actual_start.is_none()
                        && !eng.p.has_children(i)
                        && t.constraint.kind == ConstraintKind::Alap
                })
            })
            .filter_map(|i| eng.date(i).ls.map(|ls| (i, ls)))
            .collect();
        // A nested summary's share is a positioned parent window, not merely a summary-bar pad.
        // Its positioned start is an S(parent)→S(child) floor at every depth. Children keep their
        // own placement inside that window; they do not inherit the parent's ASAP/ALAP policy.
        // Manual/rule-based parent starts already enter through summary edges; share placement
        // contributes the same kind of edge after resolving the parent window.
        let pc = project_calendar(eng.p);
        for &i in &eng.order {
            if eng.p.has_children(i) || eng.task(i).is_some_and(|t| t.actual_finish.is_some()) {
                continue;
            }
            let mut anc = parent_idx.get(i).copied().flatten();
            while let Some(a) = anc {
                let min = share.group_min.get(a).copied().flatten();
                if let Some(min) = min {
                    let positioned_start = if effective_alap(eng.p, &parent_idx, a) {
                        share.group_end.get(a).copied().flatten().map(|end| pc.sub(end, min))
                    } else {
                        share.group_start.get(a).copied().flatten()
                    };
                    if let Some(floor) = positioned_start {
                        pins.entry(i).and_modify(|old| *old = (*old).max(floor)).or_insert(floor);
                    }
                }
                anc = parent_idx.get(a).copied().flatten();
            }
        }
        eng.forward(&pins, true);
        eng.pull_successor_driven();
        eng.backward();
        let (total_slack, free_slack) = eng.slack();
        (eng.dates, eng.warnings, total_slack, free_slack, eng.segments)
    };

    // Warnings the engine tied to one task go to that task's own `sched.warnings` (its row
    // indicator, pane and agents); the rest describe the plan and stay project-level.
    let mut report = ScheduleReport::default();
    let mut task_warnings: HashMap<usize, Vec<String>> = HashMap::new();
    for (idx, w) in eng_warnings {
        match idx {
            Some(i) => task_warnings.entry(i).or_default().push(w),
            None => report.warnings.push(w),
        }
    }
    let crit = p.settings.critical_slack;

    for (i, t) in p.tasks.iter_mut().enumerate() {
        if t.sched.is_summary {
            continue;
        }
        let d = dates.get(i).copied().unwrap_or_default();
        let ts = total_slack.get(i).copied().unwrap_or(0);
        let mut warnings = task_warnings.remove(&i).unwrap_or_default();
        t.sched.start = d.es;
        t.sched.finish = d.ef;
        t.sched.early_start = d.es;
        t.sched.early_finish = d.ef;
        t.sched.late_start = d.ls;
        t.sched.late_finish = d.lf;
        t.sched.stop = d.parts.map(|pt| pt.stop);
        t.sched.resume = d.parts.map(|pt| pt.resume);
        t.sched.parts = segments.remove(&i).unwrap_or_default();
        t.sched.total_slack = ts;
        t.sched.free_slack = free_slack.get(i).copied().unwrap_or(0);
        t.sched.duration = if t.is_milestone() { 0 } else { share.durations.get(i).copied().flatten().unwrap_or(t.duration) };
        t.sched.rollup_start = None;
        t.sched.rollup_finish = None;
        t.sched.critical = t.active && !t.is_complete() && t.mode == ScheduleMode::Auto && ts <= crit;
        if let (Some(dl), Some(f)) = (t.deadline, d.ef)
            && f > dl
        {
            warnings.push(format!("Finishes after its deadline ({})", dl.format("%Y-%m-%d")));
        }
        if let (Some(latest), Some(f)) = (t.bounds.finish_latest, d.ef)
            && f > latest
            && t.deadline != Some(latest)
        {
            warnings.push(format!("Finishes after its no-later-than date ({})", latest.format("%Y-%m-%d")));
        }
        t.sched.warnings = warnings;
    }

    // Ruleset on: a fixed successor pin that still leaves a predecessor finishing after the
    // successor's start is an impossible FS chain — name it on both rows (Swift conflict + bar rule).
    if p.settings.rules.enabled() {
        flag_pinned_successor_overruns(p);
    }

    // Summaries' own fixed costs and assignments (active summaries only), counted once at the
    // project level on top of the leaves.
    let own = rollup_summaries(p, &share.group_min, &share.group_start, &share.group_end);
    for (task_id, warning) in &share.task_notes {
        if let Some(i) = p.task_index(*task_id) {
            if let Some(task) = p.tasks.get_mut(i) {
                task.sched.warnings.push(warning.clone());
            }
        } else {
            report.warnings.push(warning.clone());
        }
    }

    // Project-level rollup.
    let leaves: Vec<&Task> = p.tasks.iter().filter(|t| !t.sched.is_summary && t.active).collect();
    // The project spans its leaves and its manually scheduled summaries (whose dates are their own).
    let manual = p.tasks.iter().filter(|t| t.active && t.sched.rollup_start.is_some());
    let spans: Vec<&Task> = leaves.iter().copied().chain(manual).collect();
    let start = spans.iter().filter_map(|t| t.sched.start).min();
    let finish = spans.iter().filter_map(|t| t.sched.finish).max();
    let work = leaves.iter().fold(own.work, |acc: Minutes, t| acc.saturating_add(t.sched.work));
    let actual = leaves.iter().fold(own.actual, |acc: Minutes, t| acc.saturating_add(t.sched.actual_work));
    let cost: f64 = own.cost + leaves.iter().map(|t| t.sched.cost).sum::<f64>();
    let (num, den) = leaves.iter().fold((0.0f64, 0.0f64), |(n, d), t| {
        let w = percent_weight(t);
        (n + w * f64::from(t.sched.percent_complete), d + w)
    });
    let pct = if den > 0.0 { (num / den) as f32 } else { 0.0 };
    for t in &p.tasks {
        for w in &t.sched.warnings {
            report.warnings.push(format!("{}: {}", t.name, w));
        }
    }
    report.warnings.extend(share.notes.iter().cloned());
    report.start = start;
    report.finish = finish;
    report.critical_tasks = p.tasks.iter().filter(|t| t.sched.critical && !t.sched.is_summary).map(|t| t.id).collect();
    p.sched = ProjectSchedule {
        start,
        finish,
        work,
        actual_work: actual,
        cost,
        percent_complete: pct,
        warnings: report.warnings.clone(),
        calculated_at: Some(crate::dates::now().with().subsec_nanosecond(0).build().unwrap_or_else(|_| crate::dates::now())),
    };
    report
}

/// Rolled-up values of one summary task.
struct Rollup {
    start: Option<DateTime>,
    finish: Option<DateTime>,
    es: Option<DateTime>,
    ef: Option<DateTime>,
    ls: Option<DateTime>,
    lf: Option<DateTime>,
    work: Minutes,
    actual: Minutes,
    cost: f64,
    slack: Minutes,
    free: Minutes,
    critical: bool,
    percent: f32,
    /// (weighted progress, weight) over all subtasks below, for the parent's rollup.
    weight: (f64, f64),
}

/// Weight of a subtask in % complete rollups: its duration (a milestone counts as one minute),
/// so a summary's % complete is the duration-weighted progress of all subtasks below it.
fn percent_weight(t: &Task) -> f64 {
    if t.is_milestone() { 1.0 } else { t.duration.max(1) as f64 }
}

/// Work, actual work and cost a summary carries itself (fixed cost and its own assignments).
#[derive(Clone, Copy, Default)]
struct Own {
    work: Minutes,
    actual: Minutes,
    cost: f64,
}

/// `weights[c]` is the (weighted progress, weight) of task `c`: its own for a subtask, the sum
/// over its subtasks for a summary (so nested summaries weigh by subtask durations, not span).
fn rollup_of(p: &Project, children: &[usize], own: Own, weights: &[(f64, f64)]) -> Rollup {
    let kids: Vec<(usize, &Task)> = children.iter().filter_map(|&c| p.tasks.get(c).map(|t| (c, t))).collect();
    let active: Vec<(usize, &Task)> = kids.iter().copied().filter(|(_, t)| t.active).collect();
    let pairs = if active.is_empty() { &kids } else { &active };
    let src: Vec<&Task> = pairs.iter().map(|(_, t)| *t).collect();
    let (num, den) = pairs.iter().fold((0.0f64, 0.0f64), |(nn, d), (c, _)| {
        let (wn, wd) = weights.get(*c).copied().unwrap_or_default();
        (nn + wn, d + wd)
    });
    let start = src.iter().filter_map(|t| t.sched.start).min();
    let finish = src.iter().filter_map(|t| t.sched.finish).max();
    Rollup {
        start,
        finish,
        es: src.iter().filter_map(|t| t.sched.early_start).min(),
        ef: src.iter().filter_map(|t| t.sched.early_finish).max(),
        ls: src.iter().filter_map(|t| t.sched.late_start).min(),
        lf: src.iter().filter_map(|t| t.sched.late_finish).max(),
        work: src.iter().fold(own.work, |acc: Minutes, t| acc.saturating_add(t.sched.work)),
        actual: src.iter().fold(own.actual, |acc: Minutes, t| acc.saturating_add(t.sched.actual_work)),
        cost: src.iter().map(|t| t.sched.cost).sum::<f64>() + own.cost,
        slack: src.iter().map(|t| t.sched.total_slack).min().unwrap_or(0),
        free: src.iter().map(|t| t.sched.free_slack).min().unwrap_or(0),
        critical: src.iter().any(|t| t.sched.critical),
        percent: if den > 0.0 { (num / den) as f32 } else { 0.0 },
        weight: (num, den),
    }
}

/// Effective ALAP: own placement, or inherited from the nearest ancestor with ALAP (Swift §5.4).
fn effective_alap(p: &Project, parent: &[Option<usize>], mut i: usize) -> bool {
    loop {
        if p.tasks.get(i).is_some_and(|t| t.constraint.kind == ConstraintKind::Alap) {
            return true;
        }
        match parent.get(i).copied().flatten() {
            Some(pi) => i = pi,
            None => return false,
        }
    }
}

/// Own hard latest-start that seeds `fixedStartCap` (fixed start_latest, SNLT, or MSO).
fn own_fixed_start_cap(t: &Task) -> Option<DateTime> {
    if t.bounds.start_latest_fixed {
        return t.bounds.start_latest;
    }
    t.constraint.date.filter(|_| matches!(t.constraint.kind, ConstraintKind::Snlt | ConstraintKind::Mso))
}

/// Working duration implied by paired date rules on a leaf (before share / CPM).
fn date_driven_duration(t: &Task, cal: &WorkCal<'_>) -> Option<Minutes> {
    if t.is_milestone() || t.sched.is_summary {
        return None;
    }
    let (start, finish) = t.date_duration_bounds()?;
    Some(cal.between(start, finish).max(0))
}

fn apply_date_driven_durations(p: &Project, resolved: &mut Vec<Option<Minutes>>) {
    let cal = project_calendar(p);
    if resolved.len() < p.tasks.len() {
        resolved.resize(p.tasks.len(), None);
    }
    for (i, t) in p.tasks.iter().enumerate() {
        if p.has_children(i) {
            continue;
        }
        if let Some(mins) = date_driven_duration(t, &cal)
            && let Some(slot) = resolved.get_mut(i)
        {
            *slot = Some(mins);
        }
    }
}

/// Rolls subtasks up into their summaries. Returns the summaries' own work and cost (fixed costs
/// and assignments on summary rows), totalled over active summaries for the project level.
fn rollup_summaries(
    p: &mut Project,
    group_min: &[Option<Minutes>],
    group_start: &[Option<DateTime>],
    group_end: &[Option<DateTime>],
) -> Own {
    let parent = parents(p);
    let proj_cal = project_calendar(p);
    let manual_summary: Vec<Option<(DateTime, DateTime)>> = p
        .tasks
        .iter()
        .zip((0..p.tasks.len()).map(|i| !p.has_children(i)))
        .map(|(t, leaf)| manual_summary_dates(t, !leaf, &proj_cal))
        .collect();
    let ancestor_limits = summary_ancestor_limits(p, &parent, &manual_summary);
    let mut total_own = Own::default();
    // Leaves weigh themselves; summaries get the sum of their children below.
    let mut weights: Vec<(f64, f64)> = p
        .tasks
        .iter()
        .map(|t| if t.sched.is_summary { (0.0, 0.0) } else { (percent_weight(t) * f64::from(t.sched.percent_complete), percent_weight(t)) })
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); p.tasks.len()];
    for (c, par) in parent.iter().enumerate() {
        if let Some(list) = par.and_then(|pi| children.get_mut(pi)) {
            list.push(c);
        }
    }
    let proj_cal_owned = p.base_calendar().clone();
    let pc = WorkCal::single(&proj_cal_owned);
    // Deepest summaries first: a summary's children are rolled up before it.
    for i in (0..p.tasks.len()).rev() {
        let Some(t) = p.tasks.get(i) else { continue };
        if !t.sched.is_summary {
            continue;
        }
        let kids = children.get(i).map(Vec::as_slice).unwrap_or_default();
        // Before the rollup overwrites them, `sched.work/actual_work/cost` hold the summary's own
        // values (rollup_tracking: its assignments and fixed cost). Indenting a task under one
        // with assignments must not lose that work.
        let own = Own { work: t.sched.work, actual: t.sched.actual_work, cost: t.sched.cost };
        if t.active {
            total_own.work = total_own.work.saturating_add(own.work);
            total_own.actual = total_own.actual.saturating_add(own.actual);
            total_own.cost += own.cost;
        }
        let r = rollup_of(p, kids, own, &weights);
        if let Some(w) = weights.get_mut(i) {
            *w = r.weight;
        }
        // A manually scheduled summary keeps its own dates; its subtasks' span is kept apart.
        let manual = manual_summary_dates(t, true, &pc);
        let start_dynamic = t.side_is_dynamic(false);
        let finish_dynamic = t.side_is_dynamic(true);
        let (mut start, mut finish) = match manual {
            Some((s, f)) => (Some(s), Some(f)),
            None => {
                let start = if start_dynamic { r.start } else { t.side_anchor(false).or(r.start) };
                let finish = if finish_dynamic { r.finish } else { t.side_anchor(true).or(r.finish) };
                (start, finish)
            }
        };
        let mut duration = match (start, finish) {
            (Some(s), Some(f)) => pc.between(s, f),
            _ => 0,
        };
        // Nested share placement (Size of group % / Remaining): allotted length L inside the
        // parent window [Ws, We]. Alignment picks which end of the window the block occupies;
        // children shorter than L leave empty space on the other side. Children longer than L
        // already grew the rollup — no pad.
        //
        //   ASAP: S = Ws (earlier if children already start before Ws), F = S + L
        //   ALAP: F = We (later if children already finish after We),  S = F − L
        if manual.is_none()
            && let Some(min) = group_min.get(i).copied().flatten()
            // Equality still needs alignment: children can exactly fill L at the ASAP end,
            // while the nested summary itself is ALAP and must re-anchor that block at We.
            && duration <= min
            && finish_dynamic
            && start_dynamic
        {
            let alap = effective_alap(p, &parent, i);
            let window_end = group_end.get(i).copied().flatten();
            let window_start = group_start.get(i).copied().flatten();
            if alap {
                if let Some(f) = match (finish, window_end) {
                    (Some(child_f), Some(end)) => Some(child_f.max(end)),
                    (Some(child_f), None) => Some(child_f),
                    (None, Some(end)) => Some(end),
                    (None, None) => None,
                } {
                    let s = pc.sub(f, min);
                    duration = pc.between(s, f);
                    start = Some(s);
                    finish = Some(f);
                }
            } else if let Some(s) = match (start, window_start) {
                // Prefer the parent-window start; only pull earlier to cover children that began before Ws.
                (Some(child_s), Some(ws)) => Some(child_s.min(ws)),
                (None, Some(ws)) => Some(ws),
                (Some(child_s), None) => Some(child_s),
                (None, None) => None,
            } {
                let f = pc.add(s, min);
                duration = pc.between(s, f);
                start = Some(s);
                finish = Some(f);
            }
        }
        let limits = ancestor_limits.get(i).copied().unwrap_or_default();
        let mut summary_boundary_warnings = Vec::new();
        if let Some(cap) = limits.finish_cap
            && let Some(anchor) = t.side_anchor(true)
            && anchor > cap.date
        {
            summary_boundary_warnings.push(inherited_finish_cap_warning(p, cap));
        }
        if let Some(floor) = limits.start_floor
            && let Some(anchor) = t.side_anchor(false)
            && anchor < floor.date
        {
            summary_boundary_warnings.push(inherited_start_floor_warning(p, floor));
        }
        let Some(t) = p.tasks.get_mut(i) else { continue };
        t.sched.start = start;
        t.sched.finish = finish;
        if manual.is_some() {
            // Its early and late dates are its own dates, like a manual task's.
            (t.sched.early_start, t.sched.early_finish, t.sched.late_start, t.sched.late_finish) = (start, finish, start, finish);
            // Never `None` for a manual summary, so the span always says it is one.
            t.sched.rollup_start = r.start.or(start);
            t.sched.rollup_finish = r.finish.or(finish);
        } else {
            t.sched.early_start = if start_dynamic { r.es } else { start };
            t.sched.early_finish = if finish_dynamic { r.ef } else { finish };
            t.sched.late_start = if start_dynamic { r.ls } else { start };
            t.sched.late_finish = if finish_dynamic { r.lf } else { finish };
            t.sched.rollup_start = None;
            t.sched.rollup_finish = None;
        }
        t.sched.work = r.work;
        t.sched.actual_work = r.actual;
        t.sched.cost = r.cost;
        t.sched.total_slack = r.slack;
        t.sched.free_slack = r.free;
        t.sched.critical = r.critical;
        t.sched.duration = duration;
        t.sched.percent_complete = r.percent;
        t.sched.stop = None;
        t.sched.resume = None;
        t.sched.parts.clear();
        t.sched.warnings.clear();
        t.sched.warnings.extend(summary_boundary_warnings);
        if let (Some(dl), Some(f)) = (t.deadline, finish)
            && f > dl
        {
            t.sched.warnings.push(format!("Finishes after its deadline ({})", dl.format("%Y-%m-%d")));
        }
    }
    total_own
}

#[cfg(test)]
mod calendar_sharing_tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use crate::calendar::CalendarException;

    fn plan() -> Project {
        let mut p = Project::new("x", crate::dates::ymd(2026, 10, 19).unwrap());
        let (a, b, c) = (p.add_task("A", None), p.add_task("B", None), p.add_task("C", None));
        let (ana, ben, cy) = (p.add_resource("Ana"), p.add_resource("Ben"), p.add_resource("Cy"));
        p.assign(a, ana, None, None).unwrap();
        p.assign(b, ben, None, None).unwrap();
        p.assign(c, cy, None, None).unwrap();
        let off = crate::dates::ymd(2026, 10, 21).unwrap();
        p.resource_mut(cy).unwrap().exceptions.push(CalendarException::non_working("Leave", off, off));
        p
    }

    /// Resources on the same calendar without time off of their own give their tasks one shared
    /// calendar; a resource with time off gets its own.
    #[test]
    fn resources_on_the_same_calendar_share_one() {
        let p = plan();
        let assigned = assigned_resources(&p);
        let keys: Vec<CalKey> =
            p.tasks.iter().map(|t| cal_key(&p, t, assigned.get(&t.id).map(Vec::as_slice).unwrap_or_default())).collect();
        assert_eq!(keys[0], keys[1], "Ana and Ben work the same time");
        assert_ne!(keys[0], keys[2], "Cy has leave");
    }

    /// `TaskCalendars` gives each task the calendar `task_calendar` gives it.
    #[test]
    fn task_calendars_match_each_tasks_own() {
        let p = plan();
        let mut cals = TaskCalendars::new(&p);
        let from = crate::dates::ymd(2026, 10, 19).unwrap().and_hms_opt(8, 0, 0).unwrap();
        for idx in 0..p.tasks.len() {
            let own = task_calendar(&p, idx);
            for mins in [0, 480, 3 * 480, 400 * 480] {
                assert_eq!(cals.of(idx).unwrap().add(from, mins), own.add(from, mins), "task {idx}, {mins} minutes");
            }
        }
        assert_eq!(cals.cals.len(), 2);
        assert!(cals.of(99).is_none());
    }
}
