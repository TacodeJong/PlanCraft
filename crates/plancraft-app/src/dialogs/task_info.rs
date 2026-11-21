//! Task Information dialog.

use egui::{Context, RichText, Ui};
use plancraft_core::custom::CustomFieldKind;
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

use crate::edit::EditExt;
use crate::format;
use crate::theme;
use crate::views::properties::effective_duration_mode_for_ui;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Predecessors,
    Resources,
    Advanced,
    Estimate,
    Milestone,
    Custom,
    Notes,
}

/// One custom field in the dialog: the text it opened with and the text being edited.
#[derive(Clone, PartialEq)]
struct CustomRow {
    field: CustomFieldId,
    original: String,
    text: String,
}

#[derive(Clone, PartialEq)]
struct PredRow {
    pred: TaskRef,
    kind: LinkType,
    lag: String,
}

#[derive(Clone, PartialEq)]
struct ResRow {
    resource: ResourceId,
    units: String,
    work: String,
}

#[derive(Clone, Copy, PartialEq)]
enum EstKind {
    None,
    Complexity,
    ThreePoint,
    Work,
}

/// Size in group (Swift `GroupSizeChoice`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SizeChoice {
    Own,
    Percent,
    Fill,
}

#[derive(Clone, PartialEq)]
pub struct TaskInfo {
    /// The fields as the dialog opened: OK writes only what was changed since, so changes an
    /// agent (or the table) made meanwhile are kept.
    orig: Option<Box<TaskInfo>>,
    /// Set when Previous/Next moved to another task: the app moves the selection along.
    pub nav: Option<TaskId>,
    id: TaskId,
    /// Multiple Task Information: the other selected tasks, which get the same changes.
    others: Vec<TaskId>,
    tab: Tab,
    name: String,
    duration: String,
    duration_mode: DurationMode,
    estimated: bool,
    percent: String,
    status: Option<TaskStatus>,
    priority: String,
    mode: ScheduleMode,
    start: Option<DateTime>,
    finish: Option<DateTime>,
    milestone: bool,
    active: bool,
    preds: Vec<PredRow>,
    resources: Vec<ResRow>,
    constraint: ConstraintKind,
    constraint_date: String,
    deadline: String,
    start_earliest: Option<DateTime>,
    start_latest: Option<DateTime>,
    start_latest_fixed: bool,
    finish_earliest: Option<DateTime>,
    finish_latest: Option<DateTime>,
    finish_latest_fixed: bool,
    fills_group: bool,
    group_percent: String,
    task_type: TaskType,
    effort_driven: bool,
    calendar: Option<CalendarId>,
    ignore_res_cal: bool,
    fixed_cost: String,
    actual_start: String,
    actual_finish: String,
    /// Resume date of an in-progress task, and the text it opened with.
    resume: String,
    resume_orig: String,
    on_timeline: bool,
    est_kind: EstKind,
    points: String,
    optimistic: String,
    most_likely: String,
    pessimistic: String,
    est_work: String,
    deliverable: String,
    members: Vec<TaskRef>,
    notes: String,
    custom: Vec<CustomRow>,
    mark: Option<RowMark>,
    bar_color: Option<[u8; 3]>,
    error: Option<String>,
    summary: bool,
}

/// A custom value as text the dialog can edit and parse back (ISO dates, plain numbers).
fn custom_edit_text(p: &Project, field: CustomFieldId, v: Option<&CustomValue>) -> String {
    match v {
        None => String::new(),
        Some(CustomValue::Date(d)) => format::iso_date(Some(*d)),
        Some(CustomValue::Number(n)) => format!("{n}"),
        Some(other) => p.custom_value_text(field, other),
    }
}

impl TaskInfo {
    pub fn new(p: &Project, id: TaskId, notes_tab: bool) -> Option<Self> {
        let t = p.task(id)?;
        let (est_kind, points, o, m, pe, w) = match t.estimate {
            None => (EstKind::None, String::new(), String::new(), String::new(), String::new(), String::new()),
            Some(Estimate::Complexity { points }) => {
                (EstKind::Complexity, points.to_string(), String::new(), String::new(), String::new(), String::new())
            }
            Some(Estimate::ThreePoint { optimistic, most_likely, pessimistic }) => (
                EstKind::ThreePoint,
                String::new(),
                format_work(optimistic),
                format_work(most_likely),
                format_work(pessimistic),
                String::new(),
            ),
            Some(Estimate::Work { minutes }) => {
                (EstKind::Work, String::new(), String::new(), String::new(), String::new(), format_work(minutes))
            }
        };
        let mut info = Self {
            orig: None,
            nav: None,
            id,
            others: Vec::new(),
            tab: if notes_tab { Tab::Notes } else { Tab::General },
            name: t.name.clone(),
            duration: p.format_task_minutes(t, t.sched.duration, t.estimated),
            duration_mode: effective_duration_mode_for_ui(p, id, t).0,
            estimated: t.estimated,
            percent: format!("{}", t.percent_complete),
            status: Some(t.status),
            priority: t.priority.to_string(),
            mode: t.mode,
            start: t.sched.start.or(t.manual_start),
            finish: t.sched.finish.or(t.manual_finish),
            milestone: t.is_milestone(),
            active: t.active,
            preds: t
                .predecessors
                .iter()
                .map(|d| PredRow { pred: d.predecessor, kind: d.kind, lag: d.lag.format(&p.settings.time) })
                .collect(),
            resources: p
                .assignments_for_task(id)
                .map(|a| ResRow { resource: a.resource, units: format::units(a.units), work: format_work(a.work) })
                .collect(),
            constraint: t.constraint.kind,
            constraint_date: format::iso(t.constraint.date),
            deadline: format::iso(t.deadline),
            start_earliest: t.bounds.start_earliest,
            start_latest: t.bounds.start_latest,
            start_latest_fixed: t.bounds.start_latest_fixed,
            finish_earliest: t.bounds.finish_earliest,
            finish_latest: t.bounds.finish_latest,
            finish_latest_fixed: t.bounds.finish_latest_fixed,
            fills_group: t.fills_group,
            group_percent: t.duration_percent.map(|n| n.to_string()).unwrap_or_default(),
            task_type: t.task_type,
            effort_driven: t.effort_driven,
            calendar: t.calendar,
            ignore_res_cal: t.ignore_resource_calendars,
            fixed_cost: format!("{:.2}", t.fixed_cost),
            actual_start: format::iso(t.actual_start),
            actual_finish: format::iso(t.actual_finish),
            resume: format::iso(t.sched.resume),
            resume_orig: format::iso(t.sched.resume),
            on_timeline: t.show_on_timeline,
            est_kind,
            points,
            optimistic: o,
            most_likely: m,
            pessimistic: pe,
            est_work: w,
            deliverable: t.deliverable.clone(),
            members: t.members.clone(),
            notes: t.notes.clone(),
            custom: p
                .custom_fields
                .iter()
                .map(|f| {
                    let text = custom_edit_text(p, f.id, t.custom.get(&f.id));
                    CustomRow { field: f.id, original: text.clone(), text }
                })
                .collect(),
            mark: t.row_mark,
            bar_color: t.bar_color,
            error: None,
            summary: t.sched.is_summary,
        };
        info.orig = Some(Box::new(info.clone()));
        Some(info)
    }

    /// Multiple Task Information for `ids` (one task: plain Task Information). Text fields whose
    /// value differs between the tasks start empty; OK writes only the fields changed in the
    /// dialog, to every task, so the others keep their own values. Predecessors, resources and
    /// milestone members are left out: they belong to one task.
    pub fn new_multi(p: &Project, ids: &[TaskId]) -> Option<Self> {
        let infos: Vec<TaskInfo> = ids.iter().filter_map(|id| Self::new(p, *id, false)).collect();
        let (first, rest) = infos.split_first()?;
        let mut base = first.clone();
        if rest.is_empty() {
            return Some(base);
        }
        macro_rules! mixed {
            ($($field:ident),*) => {$(
                if rest.iter().any(|o| o.$field != base.$field) {
                    base.$field = String::new();
                }
            )*};
        }
        mixed!(
            name,
            duration,
            percent,
            priority,
            constraint_date,
            deadline,
            group_percent,
            fixed_cost,
            actual_start,
            actual_finish,
            points,
            optimistic,
            most_likely,
            pessimistic,
            est_work,
            deliverable,
            notes
        );
        if rest.iter().any(|o| o.status != base.status) {
            base.status = None;
        }
        if rest.iter().any(|o| o.start != base.start) {
            base.start = None;
        }
        if rest.iter().any(|o| o.finish != base.finish) {
            base.finish = None;
        }
        if rest.iter().any(|o| o.start_earliest != base.start_earliest) {
            base.start_earliest = None;
        }
        if rest.iter().any(|o| o.start_latest != base.start_latest) {
            base.start_latest = None;
        }
        if rest.iter().any(|o| o.finish_earliest != base.finish_earliest) {
            base.finish_earliest = None;
        }
        if rest.iter().any(|o| o.finish_latest != base.finish_latest) {
            base.finish_latest = None;
        }
        if rest.iter().any(|o| o.resume != base.resume) {
            base.resume = String::new();
            base.resume_orig = String::new();
        }
        for (k, row) in base.custom.iter_mut().enumerate() {
            if rest.iter().any(|o| o.custom.get(k).is_none_or(|r| r.text != row.text)) {
                row.text = String::new();
                row.original = String::new();
            }
        }
        base.others = rest.iter().map(|i| i.id).collect();
        base.summary = infos.iter().all(|i| i.summary);
        if matches!(base.tab, Tab::Predecessors | Tab::Resources | Tab::Milestone) {
            base.tab = Tab::General;
        }
        base.orig = None;
        base.orig = Some(Box::new(base.clone()));
        Some(base)
    }

    fn is_multi(&self) -> bool {
        !self.others.is_empty()
    }

    /// Writes the fields changed since the dialog opened to its task, or to each of its tasks
    /// (Multiple Task Information), in one edit.
    ///
    /// Each task starts from its own values, and takes from the dialog only the fields changed
    /// there: a choice or a box the dialog shows with the first task's value (constraint type,
    /// estimate kind, Estimated, task type, calendar…) is not written to the others unless it was
    /// changed. Typing only a constraint date keeps each task's own constraint type, unless that
    /// type takes no date (then it takes the dialog's, as typing a date in the table does);
    /// typing an estimate gives every task the dialog's kind of estimate.
    fn apply_all(&self, p: &mut Project) -> Result<(), String> {
        let Some(o) = self.orig.as_deref().filter(|_| self.is_multi()) else { return self.apply(p) };
        for id in std::iter::once(self.id).chain(self.others.iter().copied()) {
            // Deleted meanwhile (by an agent): nothing to change there.
            let Some(own) = Self::new(p, id, false) else { continue };
            let name = own.name.clone();
            let mut one = own.clone();
            macro_rules! changed {
                ($($field:ident),*) => {$(
                    if self.$field != o.$field {
                        one.$field = self.$field.clone();
                    }
                )*};
            }
            changed!(
                name,
                duration,
                duration_mode,
                estimated,
                percent,
                status,
                priority,
                mode,
                start,
                finish,
                milestone,
                active,
                constraint,
                constraint_date,
                deadline,
                start_earliest,
                start_latest,
                start_latest_fixed,
                finish_earliest,
                finish_latest,
                finish_latest_fixed,
                fills_group,
                group_percent,
                task_type,
                effort_driven,
                calendar,
                ignore_res_cal,
                fixed_cost,
                actual_start,
                actual_finish,
                on_timeline,
                est_kind,
                points,
                optimistic,
                most_likely,
                pessimistic,
                est_work,
                deliverable,
                notes
            );
            // A value typed where the task's own choice can't hold it is not dropped: the task
            // takes the dialog's choice for it.
            if self.constraint_date != o.constraint_date && self.constraint == o.constraint && !own.constraint.needs_date() {
                one.constraint = if self.constraint.needs_date() { self.constraint } else { ConstraintKind::Snet };
            }
            let estimate_typed = self.points != o.points
                || self.optimistic != o.optimistic
                || self.most_likely != o.most_likely
                || self.pessimistic != o.pessimistic
                || self.est_work != o.est_work;
            if estimate_typed && self.est_kind == o.est_kind {
                one.est_kind = self.est_kind;
            }
            if self.resume.trim() != self.resume_orig.trim() {
                one.resume = self.resume.clone();
            }
            // By field, not by position: an agent may have added or removed fields meanwhile.
            for mine in self.custom.iter().filter(|m| m.text.trim() != m.original.trim()) {
                if let Some(row) = one.custom.iter_mut().find(|r| r.field == mine.field) {
                    row.text = mine.text.clone();
                }
            }
            one.orig = Some(Box::new(own));
            one.apply(p).map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(())
    }

    /// Writes the fields changed since the dialog opened, in one edit.
    fn apply(&self, p: &mut Project) -> Result<(), String> {
        let id = self.id;
        if p.task(id).is_none() {
            return Err("task was deleted".into());
        }
        // Nothing to compare with means nothing was changed here.
        let Some(o) = self.orig.as_deref() else { return Ok(()) };
        let err = |e: PlanError| e.to_string();
        let opt_date = |p: &Project, s: &str, finish: bool| -> Result<Option<DateTime>, String> {
            if s.trim().is_empty() || s.trim().eq_ignore_ascii_case("na") {
                Ok(None)
            } else {
                p.parse_datetime(s, finish).map(Some).ok_or_else(|| format!("'{s}' is not a valid date"))
            }
        };
        if self.name != o.name && self.name.trim().is_empty() {
            return Err("The task name cannot be empty.".into());
        }
        let priority = if self.priority != o.priority {
            Some(self.priority.trim().parse::<u16>().map_err(|_| "Priority must be a number from 0 to 1000")?.min(1000))
        } else {
            None
        };
        let fixed_cost = if self.fixed_cost.trim() != o.fixed_cost.trim() {
            Some(format::parse_money(&self.fixed_cost).ok_or_else(|| format!("'{}' is not a valid fixed cost", self.fixed_cost))?)
        } else {
            None
        };
        if self.status != o.status
            && let Some(status) = self.status
        {
            p.set_task_status(id, status).map_err(err)?;
        }
        if self.mode != o.mode {
            p.set_schedule_mode(&[id], self.mode).map_err(err)?;
        }
        {
            let t = p.task_mut(id).ok_or("task was deleted")?;
            if self.name != o.name {
                t.name = self.name.trim().to_string();
            }
            if self.active != o.active {
                t.active = self.active;
            }
            if let Some(v) = priority {
                t.priority = v;
            }
            if self.task_type != o.task_type {
                t.task_type = self.task_type;
            }
            if self.effort_driven != o.effort_driven {
                t.effort_driven = self.effort_driven;
            }
            if self.calendar != o.calendar {
                t.calendar = self.calendar;
            }
            if self.ignore_res_cal != o.ignore_res_cal {
                t.ignore_resource_calendars = self.ignore_res_cal;
            }
            if self.on_timeline != o.on_timeline {
                t.show_on_timeline = self.on_timeline;
            }
            if self.notes != o.notes {
                t.notes = self.notes.clone();
            }
            if self.deliverable != o.deliverable {
                t.deliverable = self.deliverable.clone();
            }
            if let Some(v) = fixed_cost {
                t.fixed_cost = v;
            }
            if self.mark != o.mark {
                t.row_mark = self.mark;
            }
            if self.bar_color != o.bar_color {
                t.bar_color = self.bar_color;
            }
        }
        if self.constraint != o.constraint || self.constraint_date != o.constraint_date {
            let cdate = opt_date(p, &self.constraint_date, !self.constraint.is_start_constraint())?;
            if self.constraint.needs_date() && cdate.is_none() {
                return Err(format!("'{}' needs a constraint date.", self.constraint.label()));
            }
            let t = p.task_mut(id).ok_or("task was deleted")?;
            t.constraint = Constraint { kind: self.constraint, date: if self.constraint.needs_date() { cdate } else { None } };
        }
        if self.deadline != o.deadline {
            let deadline = opt_date(p, &self.deadline, true)?;
            p.task_mut(id).ok_or("task was deleted")?.deadline = deadline;
        }
        let bounds_changed = self.start_earliest != o.start_earliest
            || self.start_latest != o.start_latest
            || self.start_latest_fixed != o.start_latest_fixed
            || self.finish_earliest != o.finish_earliest
            || self.finish_latest != o.finish_latest
            || self.finish_latest_fixed != o.finish_latest_fixed
            || self.fills_group != o.fills_group
            || self.group_percent != o.group_percent;
        if bounds_changed {
            let percent = if self.group_percent != o.group_percent {
                Some(if self.group_percent.trim().is_empty() {
                    None
                } else {
                    Some(self.group_percent.trim().parse::<f64>().map_err(|_| "Percent of group must be a number")?)
                })
            } else {
                None
            };
            let t = p.task_mut(id).ok_or("task was deleted")?;
            if self.start_earliest != o.start_earliest {
                t.bounds.start_earliest = self.start_earliest;
            }
            if self.start_latest != o.start_latest {
                t.bounds.start_latest = self.start_latest;
            }
            if self.start_latest_fixed != o.start_latest_fixed {
                t.bounds.start_latest_fixed = self.start_latest_fixed;
            }
            if self.finish_earliest != o.finish_earliest {
                t.bounds.finish_earliest = self.finish_earliest;
            }
            if self.finish_latest != o.finish_latest {
                t.bounds.finish_latest = self.finish_latest;
            }
            if self.finish_latest_fixed != o.finish_latest_fixed {
                t.bounds.finish_latest_fixed = self.finish_latest_fixed;
            }
            // Size in group: fill clears percent; percent clears fill; Remaining space clears dates.
            if self.fills_group != o.fills_group {
                t.fills_group = self.fills_group;
                if self.fills_group {
                    t.duration_percent = None;
                    t.clear_date_constraints();
                }
            }
            if let Some(pct) = percent {
                t.duration_percent = pct;
                if pct.is_some() {
                    t.fills_group = false;
                }
            }
        }
        if self.mode == ScheduleMode::Manual && (self.start != o.start || self.finish != o.finish) {
            let t = p.task_mut(id).ok_or("task was deleted")?;
            if self.start != o.start {
                t.manual_start = self.start;
            }
            if self.finish != o.finish {
                t.manual_finish = self.finish;
            }
        }
        let estimate_changed = self.est_kind != o.est_kind
            || self.points != o.points
            || self.optimistic != o.optimistic
            || self.most_likely != o.most_likely
            || self.pessimistic != o.pessimistic
            || self.est_work != o.est_work;
        if estimate_changed {
            let estimate = match self.est_kind {
                EstKind::None => None,
                EstKind::Complexity => {
                    Some(Estimate::Complexity { points: self.points.trim().parse().map_err(|_| "Complexity points must be a number")? })
                }
                EstKind::ThreePoint => {
                    let w = |s: &str| parse_work(s, &p.settings.time).ok_or_else(|| format!("'{s}' is not a valid amount of work"));
                    Some(Estimate::ThreePoint {
                        optimistic: w(&self.optimistic)?,
                        most_likely: w(&self.most_likely)?,
                        pessimistic: w(&self.pessimistic)?,
                    })
                }
                EstKind::Work => {
                    Some(Estimate::Work { minutes: parse_work(&self.est_work, &p.settings.time).ok_or("Invalid work estimate")? })
                }
            };
            p.task_mut(id).ok_or("task was deleted")?.estimate = estimate;
        }
        if !self.summary {
            if self.milestone != o.milestone {
                p.set_milestone(id, self.milestone).map_err(err)?;
            }
            // Duration mode before a typed duration: typing opts out of Auto via set_duration.
            if !self.milestone && self.duration_mode != o.duration_mode {
                p.set_duration_mode(id, self.duration_mode).map_err(err)?;
            }
            if !self.milestone && (self.duration != o.duration || self.estimated != o.estimated) {
                let typed = p.parse_task_duration(&self.duration).ok_or_else(|| format!("'{}' is not a valid duration", self.duration))?;
                // The duration text shows "?" for an estimated task: unless it was edited, the box
                // decides (unticking it would otherwise never take).
                let estimated = if self.duration != o.duration { typed.estimated || self.estimated } else { self.estimated };
                let typed = TypedDuration { estimated, ..typed };
                p.set_typed_duration(id, typed).map_err(err)?;
            }
        }
        if self.preds != o.preds {
            let mut deps = Vec::new();
            for r in &self.preds {
                let lag = Lag::parse(&r.lag, &p.settings.time).ok_or_else(|| format!("'{}' is not a valid lag", r.lag))?;
                deps.push(Dependency { predecessor: r.pred, kind: r.kind, lag });
            }
            p.set_predecessors(id, deps).map_err(err)?;
        }
        if self.milestone && self.members != o.members {
            if let Some(t) = p.task_mut(id) {
                t.members.clear();
            }
            p.add_milestone_members(id, &self.members).map_err(err)?;
        }
        if !self.summary && self.resources != o.resources {
            self.apply_resources(p, &o.resources)?;
        }
        // Custom fields: only the ones edited here, so values changed meanwhile are kept.
        for row in self.custom.iter().filter(|r| r.text.trim() != r.original.trim()) {
            let listed = p.custom_field(row.field).is_some_and(|f| f.choices.iter().any(|c| c == crate::fields::NO_VALUE));
            let text = if row.text == crate::fields::NO_VALUE && !listed { "" } else { row.text.as_str() };
            let value = p.parse_custom_value(row.field, text).map_err(err)?;
            p.set_custom_value(id, row.field, value).map_err(err)?;
        }
        // Progress last so actual dates use the new schedule.
        plancraft_core::schedule::recalculate(p);
        if self.percent.trim() != o.percent.trim() {
            let pct: u8 =
                self.percent.trim().trim_end_matches('%').parse::<f64>().map_err(|_| "% complete must be a number")?.clamp(0.0, 100.0)
                    as u8;
            p.set_percent_complete(id, pct).map_err(err)?;
        }
        // An emptied (or "NA") field clears the date, with the same rules as the table's Actual
        // Start and Actual Finish columns (`columns::set_actual_start` / `set_actual_finish`).
        // The finish first: clearing both clears the finish before the start is checked.
        if self.actual_finish != o.actual_finish {
            let d = opt_date(p, &self.actual_finish, true)?;
            crate::columns::set_actual_finish(p, id, d)?;
        }
        if self.actual_start != o.actual_start {
            let d = opt_date(p, &self.actual_start, false)?;
            crate::columns::set_actual_start(p, id, d)?;
        }
        if self.resume.trim() != self.resume_orig.trim() {
            let resume = opt_date(p, &self.resume, false)?;
            p.set_resume(id, resume).map_err(err)?;
        }
        Ok(())
    }

    /// Writes the Resources tab's changes since the dialog opened (`orig`): resources removed
    /// there are unassigned, added ones assigned, and edited units or work set. Assignments an
    /// agent made meanwhile are kept.
    fn apply_resources(&self, p: &mut Project, orig: &[ResRow]) -> Result<(), String> {
        let id = self.id;
        let err = |e: PlanError| e.to_string();
        for gone in orig.iter().filter(|o| !self.resources.iter().any(|r| r.resource == o.resource)) {
            let found: Vec<AssignmentId> = p.assignments_for_task(id).filter(|a| a.resource == gone.resource).map(|a| a.id).collect();
            for a in found {
                p.unassign(a).map_err(err)?;
            }
        }
        for r in &self.resources {
            let before = orig.iter().find(|o| o.resource == r.resource);
            if before == Some(r) {
                continue;
            }
            let units = format::parse_units(&r.units).ok_or_else(|| format!("'{}' is not valid units", r.units))?;
            let work = if r.work.trim().is_empty() {
                None
            } else {
                Some(parse_work(&r.work, &p.settings.time).ok_or_else(|| format!("'{}' is not valid work", r.work))?)
            };
            let existing = p.assignments.iter().find(|a| a.task == id && a.resource == r.resource).map(|a| a.id);
            match (existing, before) {
                (Some(aid), Some(b)) => {
                    if r.units.trim() != b.units.trim() {
                        p.set_assignment_units(aid, units).map_err(err)?;
                    }
                    if let Some(w) = work.filter(|_| r.work.trim() != b.work.trim()) {
                        p.set_assignment_work(aid, w).map_err(err)?;
                    }
                }
                // Already assigned meanwhile (by an agent): keep that assignment.
                (Some(_), None) => {}
                (None, _) => {
                    p.assign(id, r.resource, Some(units), work).map_err(err)?;
                }
            }
        }
        Ok(())
    }

    /// True when any field differs from how the dialog opened (pending edits).
    fn changed(&self) -> bool {
        let Some(o) = self.orig.as_deref() else { return false };
        let mut now = self.clone();
        now.orig = None;
        now.nav = None;
        now.error = None;
        now.tab = o.tab;
        let mut was = o.clone();
        was.orig = None;
        was.nav = None;
        was.error = None;
        now != was
    }

    /// Previous/Next: saves pending edits through the normal apply path, then reloads the dialog
    /// for the adjacent task in outline order. A failed save keeps the dialog where it is.
    fn navigate(&mut self, doc: &mut Document, dir: i64) {
        let Some(idx) = doc.project.task_index(self.id) else { return };
        let next = if dir < 0 { idx.checked_sub(1) } else { idx.checked_add(1) };
        let Some(target) = next.and_then(|i| doc.project.tasks.get(i)).map(|t| t.id) else { return };
        if self.changed() {
            let label = format!("Task Information: {}", self.name);
            if let Err(e) = doc.edit_msg(&label, |p| self.apply_all(p)) {
                self.error = Some(e);
                return;
            }
        }
        let tab = self.tab;
        let Some(mut fresh) = TaskInfo::new(&doc.project, target, false) else { return };
        fresh.tab = if tab == Tab::Milestone && !fresh.milestone { Tab::General } else { tab };
        fresh.nav = Some(target);
        *self = fresh;
    }

    /// Returns false when the dialog should close.
    pub fn show(&mut self, ctx: &Context, doc: &mut Document) -> bool {
        let mut close = false;
        let multi = format!("Multiple Task Information ({} tasks)", self.others.len().saturating_add(1));
        let title = if self.is_multi() {
            multi.as_str()
        } else if self.milestone {
            "Milestone Information"
        } else if self.summary {
            "Summary Task Information"
        } else {
            "Task Information"
        };
        // Cmd+Up/Down and PageUp/Down walk the outline without closing the dialog.
        let mut nav = 0i64;
        if !self.is_multi() {
            let (up, down) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::ArrowUp) || i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp),
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::ArrowDown)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown),
                )
            });
            nav = i64::from(down) - i64::from(up);
        }
        egui::Window::new(title)
            .id(egui::Id::new("task_info_dialog"))
            .pivot(egui::Align2::CENTER_TOP)
            .default_pos(ctx.content_rect().center_top() + egui::vec2(0.0, 120.0))
            .title_bar(false)
            .resizable(true)
            .default_width(620.0)
            .min_height(420.0)
            .show(ctx, |ui| {
                let keep = crate::widgets::craft::dialog_header_with(ui, title, |ui| {
                    if !self.is_multi() {
                        if crate::icons::button(ui, "chevron-down", 18.0, crate::lang::text("ti.next")).clicked() {
                            nav = 1;
                        }
                        if crate::icons::button(ui, "chevron-up", 18.0, crate::lang::text("ti.prev")).clicked() {
                            nav = -1;
                        }
                    }
                });
                if !keep {
                    close = true;
                }
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(320.0));
                    ui.label(crate::lang::text("prop.duration_mode"));
                    let mode_editable = !self.summary && !self.milestone;
                    ui.add_enabled_ui(mode_editable, |ui| {
                        let label = match self.duration_mode {
                            DurationMode::Fixed => crate::lang::text("prop.duration_fixed"),
                            DurationMode::Auto => crate::lang::text("prop.duration_auto"),
                        };
                        egui::ComboBox::from_id_salt("ti_dur_mode").selected_text(label).show_ui(ui, |ui| {
                            for mode in DurationMode::ALL {
                                let text = match mode {
                                    DurationMode::Fixed => crate::lang::text("prop.duration_fixed"),
                                    DurationMode::Auto => crate::lang::text("prop.duration_auto"),
                                };
                                ui.selectable_value(&mut self.duration_mode, mode, text);
                            }
                        });
                    });
                    ui.label("Duration:");
                    // Several tasks: each milestone or summary among them keeps its own duration.
                    // Auto duration is calculated and read-only until Fixed is selected again.
                    let duration_editable = if self.is_multi() {
                        !self.summary && self.duration_mode == DurationMode::Fixed
                    } else {
                        !self.milestone && !self.summary && self.duration_mode == DurationMode::Fixed
                    };
                    ui.add_enabled(duration_editable, egui::TextEdit::singleline(&mut self.duration).desired_width(80.0)).on_hover_text(
                        if self.duration_mode == DurationMode::Auto { crate::lang::text("prop.duration_auto_hint") } else { "" },
                    );
                    ui.add_enabled(duration_editable, egui::Checkbox::new(&mut self.estimated, "Estimated"));
                });
                ui.add_space(4.0);
                if self.is_multi() {
                    ui.label(
                        RichText::new("Empty fields differ between the tasks. Only the fields you change are applied, to every task.")
                            .color(theme::pal().TEXT_WEAK),
                    );
                }
                ui.horizontal(|ui| {
                    let mut tabs = vec![(Tab::General, "General")];
                    // Links, assignments and milestone members belong to one task.
                    if !self.is_multi() {
                        tabs.extend([(Tab::Predecessors, "Predecessors"), (Tab::Resources, "Resources")]);
                    }
                    tabs.extend([(Tab::Advanced, "Advanced"), (Tab::Estimate, "Estimate")]);
                    if self.milestone && !self.is_multi() {
                        tabs.push((Tab::Milestone, "Milestone"));
                    }
                    if !self.custom.is_empty() {
                        tabs.push((Tab::Custom, "Custom Fields"));
                    }
                    tabs.push((Tab::Notes, "Notes"));
                    for (t, l) in tabs {
                        ui.selectable_value(&mut self.tab, t, l);
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| match self.tab {
                    Tab::General => self.general(ui, &doc.project),
                    Tab::Predecessors => self.predecessors(ui, &doc.project),
                    Tab::Resources => self.resources_tab(ui, &doc.project),
                    Tab::Advanced => self.advanced(ui, &doc.project),
                    Tab::Estimate => self.estimate(ui, &doc.project),
                    Tab::Milestone => self.milestone_tab(ui, &doc.project),
                    Tab::Custom => self.custom_tab(ui, &doc.project),
                    Tab::Notes => {
                        ui.add(egui::TextEdit::multiline(&mut self.notes).desired_rows(14).desired_width(f32::INFINITY));
                    }
                });
                if let Some(e) = &self.error {
                    ui.colored_label(theme::pal().ERROR, e);
                }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.add(egui::Button::new(RichText::new("  OK  ").color(egui::Color32::WHITE)).fill(theme::pal().ACCENT)).clicked() {
                        let label = if self.is_multi() {
                            "Multiple Task Information".to_string()
                        } else {
                            format!("Task Information: {}", self.name)
                        };
                        match doc.edit_msg(&label, |p| self.apply_all(p)) {
                            Ok(()) => close = true,
                            Err(e) => self.error = Some(e),
                        }
                    }
                });
            });
        if nav != 0 && !close {
            self.navigate(doc, nav);
        }
        !close
    }

    fn share_fields(&self, p: &Project) -> bool {
        // Size in group: any selected row with a parent that is not a leaf milestone.
        std::iter::once(self.id).chain(self.others.iter().copied()).all(|id| {
            let Some(idx) = p.task_index(id) else { return false };
            let Some(task) = p.task(id) else { return false };
            p.parent_index(idx).is_some() && (p.has_children(idx) || !task.is_milestone())
        })
    }

    fn size_choice(&self, p: &Project) -> SizeChoice {
        if self.group_percent.trim().parse::<f64>().ok().is_some_and(|pct| pct.is_finite() && pct > 0.0)
            || (!self.group_percent.trim().is_empty() && self.group_percent.trim().parse::<f64>().is_ok())
        {
            return SizeChoice::Percent;
        }
        let unrestricted = DateMode::of(self.side_window(p, false).0, self.side_window(p, false).1) == DateMode::Unrestricted
            && DateMode::of(self.side_window(p, true).0, self.side_window(p, true).1) == DateMode::Unrestricted;
        let wants_fill = self.duration_mode == DurationMode::Auto || self.fills_group;
        if wants_fill && unrestricted && self.group_percent.trim().is_empty() {
            SizeChoice::Fill
        } else {
            SizeChoice::Own
        }
    }

    fn set_size_choice(&mut self, p: &Project, choice: SizeChoice) {
        match choice {
            SizeChoice::Own => {
                self.group_percent.clear();
                self.fills_group = false;
                self.duration_mode = DurationMode::Fixed;
            }
            SizeChoice::Percent => {
                if self.group_percent.trim().is_empty() {
                    let base = p
                        .task_index(self.id)
                        .and_then(|i| p.parent_index(i))
                        .and_then(|pi| p.tasks.get(pi))
                        .map(|g| g.sched.duration)
                        .unwrap_or(0);
                    let span = p.task(self.id).map(|t| t.sched.duration).unwrap_or(0);
                    let value = if base > 0 {
                        ((span as f64 / base as f64) * 100.0).min(1000.0)
                    } else {
                        100.0
                    };
                    self.group_percent = format!("{}", (value * 100.0).round() / 100.0);
                }
                self.fills_group = false;
                self.duration_mode = DurationMode::Fixed;
            }
            SizeChoice::Fill => {
                self.group_percent.clear();
                self.fills_group = true;
                self.start_earliest = None;
                self.start_latest = None;
                self.start_latest_fixed = false;
                self.finish_earliest = None;
                self.finish_latest = None;
                self.finish_latest_fixed = false;
                if self.constraint.needs_date() {
                    self.constraint = ConstraintKind::Asap;
                    self.constraint_date.clear();
                }
                self.mode = ScheduleMode::Auto;
            }
        }
    }

    /// One side's minimum and maximum as edited here: the bound fields plus the legacy
    /// single-constraint pin, with a manual task's typed date reading as an On pin
    /// (the dialog twin of `Task::start_window`).
    fn side_window(&self, p: &Project, finish: bool) -> (Option<DateTime>, Option<DateTime>) {
        if self.mode == ScheduleMode::Manual {
            let d = if finish { self.finish } else { self.start };
            if let Some(d) = d {
                return (Some(d), Some(d));
            }
        }
        let (mut min, mut max) = if finish { (self.finish_earliest, self.finish_latest) } else { (self.start_earliest, self.start_latest) };
        let pin = p.parse_datetime(self.constraint_date.trim(), finish);
        match (finish, self.constraint) {
            (false, ConstraintKind::Snet) => min = min.max(pin),
            (false, ConstraintKind::Snlt) => max = max.min(pin).or(max).or(pin),
            (false, ConstraintKind::Mso) | (true, ConstraintKind::Mfo) => {
                min = min.or(pin);
                max = max.or(pin);
            }
            (true, ConstraintKind::Fnet) => min = min.max(pin),
            (true, ConstraintKind::Fnlt) => max = max.min(pin).or(max).or(pin),
            _ => {}
        }
        (min, max)
    }

    /// Switches one side's date rule, the Project Planner way: the mode's window lands in the
    /// bound fields, the legacy constraint pin on that side is absorbed, and Unrestricted drops
    /// the fixed flag. A manual task returns to automatic scheduling, its typed date on the
    /// other side kept as an On rule. All of it flows through the dialog's normal diff-apply.
    fn set_side_mode(&mut self, p: &Project, finish: bool, mode: DateMode) {
        let (min0, max0) = self.side_window(p, finish);
        let (other_min, other_max) = self.side_window(p, !finish);
        if self.mode == ScheduleMode::Manual {
            self.mode = ScheduleMode::Auto;
            if finish {
                self.start_earliest = other_min;
                self.start_latest = other_max;
            } else {
                self.finish_earliest = other_min;
                self.finish_latest = other_max;
            }
        }
        let anchor = if finish { self.finish.or(self.start) } else { self.start.or(self.finish) };
        let (min, max) = mode.window(min0, max0, anchor.unwrap_or(p.start));
        let side_pin = if finish {
            matches!(self.constraint, ConstraintKind::Fnet | ConstraintKind::Fnlt | ConstraintKind::Mfo)
        } else {
            matches!(self.constraint, ConstraintKind::Snet | ConstraintKind::Snlt | ConstraintKind::Mso)
        };
        if side_pin {
            self.constraint = ConstraintKind::Asap;
            self.constraint_date.clear();
        }
        if finish {
            self.finish_earliest = min;
            self.finish_latest = max;
            if max.is_none() {
                self.finish_latest_fixed = false;
            }
        } else {
            self.start_earliest = min;
            self.start_latest = max;
            if max.is_none() {
                self.start_latest_fixed = false;
            }
        }
    }

    /// One side of the single-task date rules: mode combo plus the date editors it needs.
    /// A manual task's pinned date is edited by the Start/Finish fields above, so On shows
    /// no duplicate editor then.
    fn date_rule_row(&mut self, ui: &mut Ui, p: &Project, finish: bool, minute: u32) {
        ui.label(if finish { "Finish:" } else { "Start:" });
        let (min, max) = self.side_window(p, finish);
        let mode = DateMode::of(min, max);
        let mut pick = None;
        egui::ComboBox::from_id_salt(("ti_date_mode", finish)).width(160.0).selected_text(crate::lang::date_mode_name(mode)).show_ui(
            ui,
            |ui| {
                for m in DateMode::ALL {
                    if ui.selectable_label(mode == m, crate::lang::date_mode_name(m)).clicked() {
                        pick = Some(m);
                    }
                }
            },
        );
        if let Some(m) = pick
            && m != mode
        {
            self.set_side_mode(p, finish, m);
            ui.end_row();
            return;
        }
        let manual = self.mode == ScheduleMode::Manual;
        let (lo_salt, hi_salt) = if finish { ("ti_rule_fmin", "ti_rule_fmax") } else { ("ti_rule_smin", "ti_rule_smax") };
        let set = |this: &mut Self, lo: Option<DateTime>, hi: Option<DateTime>| {
            if finish {
                this.finish_earliest = lo;
                this.finish_latest = hi;
            } else {
                this.start_earliest = lo;
                this.start_latest = hi;
            }
        };
        match mode {
            DateMode::Unrestricted => {
                ui.label(RichText::new(crate::lang::text("dm.calc_deps")).color(theme::pal().TEXT_WEAK));
            }
            DateMode::On if manual => {
                ui.label(RichText::new("Pinned by the typed dates above").color(theme::pal().TEXT_WEAK));
            }
            DateMode::On => {
                if let Some(Some(d)) = pick_date(ui, lo_salt, min, false, true, minute, "") {
                    set(self, Some(d), Some(d));
                }
            }
            DateMode::NoEarlier => {
                if let Some(Some(d)) = pick_date(ui, lo_salt, min, false, true, minute, "") {
                    set(self, Some(d), max);
                }
            }
            DateMode::NoLater => {
                if let Some(Some(d)) = pick_date(ui, hi_salt, max, false, true, minute, "") {
                    set(self, min, Some(d));
                }
            }
            DateMode::Between => {
                ui.horizontal(|ui| {
                    if let Some(Some(d)) = pick_date(ui, lo_salt, min, false, true, minute, "") {
                        if finish {
                            self.finish_earliest = Some(d);
                        } else {
                            self.start_earliest = Some(d);
                        }
                    }
                    if let Some(Some(d)) = pick_date(ui, hi_salt, max, false, true, minute, "") {
                        if finish {
                            self.finish_latest = Some(d);
                        } else {
                            self.start_latest = Some(d);
                        }
                    }
                });
            }
        }
        // The "moves the plan" tick belongs to the side's maximum, so it hides with it.
        if matches!(mode, DateMode::On | DateMode::NoLater | DateMode::Between) && !manual {
            let fixed = if finish { &mut self.finish_latest_fixed } else { &mut self.start_latest_fixed };
            ui.checkbox(fixed, "Moves the plan");
        }
        ui.end_row();
    }

    /// Multiple Task Information keeps the plain four-field bounds grid: a combined mode
    /// cannot say "differs", the empty-means-differs date fields can.
    fn bounds_grid(&mut self, ui: &mut Ui, p: &Project) {
        ui.label(RichText::new("No earlier than and no later than").strong());
        egui::Grid::new("ti_bounds").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
            let start_minute = p.settings.time.default_start_time;
            let finish_minute = p.settings.time.default_finish_time;
            let empty = if self.others.is_empty() { "geen" } else { "meerdere" };
            ui.label("Start no earlier than:");
            if let Some(next) = pick_date(ui, "ti_snet", self.start_earliest, true, true, start_minute, empty) {
                self.start_earliest = next;
            }
            ui.label("Start no later than:");
            if let Some(next) = pick_date(ui, "ti_snlt", self.start_latest, true, true, start_minute, empty) {
                self.start_latest = next;
            }
            ui.end_row();
            // The "moves the plan" ticks only mean something once their date is set.
            ui.label("");
            ui.add_enabled(
                self.start_latest.is_some(),
                egui::Checkbox::new(&mut self.start_latest_fixed, "Start no later than moves the plan"),
            );
            ui.label("");
            ui.add_enabled(
                self.finish_latest.is_some(),
                egui::Checkbox::new(&mut self.finish_latest_fixed, "Finish no later than moves the plan"),
            );
            ui.end_row();
            ui.label("Finish no earlier than:");
            if let Some(next) = pick_date(ui, "ti_fnet", self.finish_earliest, true, true, finish_minute, empty) {
                self.finish_earliest = next;
            }
            ui.label("Finish no later than:");
            if let Some(next) = pick_date(ui, "ti_fnlt", self.finish_latest, true, true, finish_minute, empty) {
                self.finish_latest = next;
            }
            ui.end_row();
        });
    }

    fn general(&mut self, ui: &mut Ui, p: &Project) {
        egui::Grid::new("ti_general").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
            // A group's progress rolls up from its subtasks: visible but not editable here.
            ui.label("Percent complete:");
            ui.add_enabled(!self.summary, egui::TextEdit::singleline(&mut self.percent).desired_width(60.0))
                .on_disabled_hover_text(crate::lang::text("prop.from_children"));
            ui.label("Status:");
            ui.add_enabled_ui(!self.summary, |ui| {
                let status_label = self.status.map(crate::lang::status_name).unwrap_or_else(|| crate::lang::text("common.multiple_lc"));
                egui::ComboBox::from_id_salt("ti_status").width(120.0).selected_text(status_label).show_ui(ui, |ui| {
                    for status in TaskStatus::ALL {
                        if ui.selectable_label(self.status == Some(status), crate::lang::status_name(status)).clicked() {
                            self.status = Some(status);
                        }
                    }
                });
            });
            ui.end_row();
            ui.label("Priority:");
            ui.add(egui::TextEdit::singleline(&mut self.priority).desired_width(60.0));
            ui.end_row();
            ui.label("Schedule mode:");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.mode, ScheduleMode::Manual, "Manually Scheduled");
                ui.radio_value(&mut self.mode, ScheduleMode::Auto, "Auto Scheduled");
            });
            ui.checkbox(&mut self.active, "Active");
            ui.checkbox(&mut self.on_timeline, "Display on Timeline");
            ui.end_row();
            let manual = self.mode == ScheduleMode::Manual;
            let start_minute = p.settings.time.default_start_time;
            let finish_minute = p.settings.time.default_finish_time;
            let empty = if self.others.is_empty() { "geen" } else { "meerdere" };
            // A greyed date on an unrestricted side explains itself on hover.
            let unrestricted = |this: &Self, finish: bool| !this.is_multi() && matches!(this.side_window(p, finish), (None, None));
            if self.milestone && !self.is_multi() {
                // A milestone is one moment, not a start-finish span.
                ui.label("Date:");
                let r = ui.scope(|ui| pick_date(ui, "ti_start", self.start, false, manual, start_minute, empty));
                if let Some(next) = r.inner {
                    self.start = next;
                }
                if !manual && unrestricted(self, true) {
                    r.response.on_hover_text(crate::lang::text("dm.calc_deps"));
                }
            } else {
                // A summary's dates come from its subtasks: shown greyed, not editable.
                ui.label("Start:");
                let r = ui.scope(|ui| pick_date(ui, "ti_start", self.start, false, manual && !self.summary, start_minute, empty));
                if let Some(next) = r.inner {
                    self.start = next;
                }
                if !manual && unrestricted(self, false) {
                    r.response.on_hover_text(crate::lang::text("dm.calc_deps"));
                }
                ui.label("Finish:");
                let r = ui.scope(|ui| pick_date(ui, "ti_finish", self.finish, false, manual && !self.summary, finish_minute, empty));
                if let Some(next) = r.inner {
                    self.finish = next;
                }
                if !manual && unrestricted(self, true) {
                    r.response.on_hover_text(crate::lang::text("dm.calc_deps"));
                }
            }
            ui.end_row();
            ui.label("Actual start:");
            pick_date_iso(ui, p, "ti_astart", &mut self.actual_start, false, !self.summary, "NA");
            ui.label("Actual finish:");
            pick_date_iso(ui, p, "ti_afinish", &mut self.actual_finish, true, !self.summary, "NA");
            ui.end_row();
            if !self.summary && !self.milestone {
                let stop = p.task(self.id).and_then(|t| t.sched.stop);
                ui.label("Stop:");
                ui.label(RichText::new(if stop.is_some() { format::iso(stop) } else { "NA".into() }).color(theme::pal().TEXT_WEAK))
                    .on_hover_text("End of the work done so far");
                ui.label("Resume:");
                ui.scope(|ui| {
                    pick_date_iso(ui, p, "ti_resume", &mut self.resume, false, stop.is_some(), "right after Stop");
                })
                .response
                .on_hover_text("When the remaining work restarts. A later date splits the task.");
                ui.end_row();
            }
            ui.label("");
            ui.add_enabled(!self.summary, egui::Checkbox::new(&mut self.milestone, "Mark task as milestone"));
            ui.end_row();
            // Row mark and (for a group) the bar colour, in the pane's language.
            if !self.is_multi() {
                ui.label("Row mark:");
                ui.horizontal(|ui| {
                    if let Some(mark) = self.mark {
                        ui.add(crate::icons::image("flag", 14.0, theme::row_mark_color(mark))).on_hover_text(crate::lang::mark_name(mark));
                    }
                    let label = self.mark.map(crate::lang::mark_name).unwrap_or(crate::lang::text("common.none"));
                    egui::ComboBox::from_id_salt("ti_mark").selected_text(label).show_ui(ui, |ui| {
                        for choice in [None].into_iter().chain(RowMark::ALL.into_iter().map(Some)) {
                            let text = choice.map(crate::lang::mark_name).unwrap_or(crate::lang::text("common.none"));
                            ui.selectable_value(&mut self.mark, choice, text);
                        }
                    });
                });
                if self.summary {
                    ui.label("Group color:");
                    ui.horizontal(|ui| {
                        for rgb in theme::GROUP_SWATCHES {
                            let on = self.bar_color == Some(rgb);
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                            ui.painter().rect_filled(rect, 2.0, theme::paint_rgb(rgb));
                            if on {
                                ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, theme::pal().TEXT), egui::StrokeKind::Outside);
                            }
                            if resp.clicked() {
                                self.bar_color = Some(rgb);
                            }
                        }
                        if ui.button(crate::lang::text("common.clear")).clicked() {
                            self.bar_color = None;
                        }
                    });
                }
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        if !self.is_multi() {
            // One rule per side, the Project Planner way: Unrestricted, On, no earlier than,
            // no later than or Between, with only the date editors the mode needs.
            ui.label(RichText::new("Date rules").strong());
            egui::Grid::new("ti_modes").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
                let start_minute = p.settings.time.default_start_time;
                let finish_minute = p.settings.time.default_finish_time;
                if self.milestone {
                    // A milestone is one moment: one rule, on the finish side.
                    self.date_rule_row(ui, p, true, start_minute);
                } else {
                    self.date_rule_row(ui, p, false, start_minute);
                    self.date_rule_row(ui, p, true, finish_minute);
                }
            });
        } else {
            self.bounds_grid(ui, p);
        }
        let share = self.share_fields(p);
        if share {
            ui.add_space(6.0);
            let current = self.size_choice(p);
            let own_label = if self.summary {
                crate::lang::text("prop.size_spans")
            } else {
                crate::lang::text("prop.size_own")
            };
            let shown = match current {
                SizeChoice::Own => own_label,
                SizeChoice::Percent => crate::lang::text("prop.size_percent"),
                SizeChoice::Fill => crate::lang::text("prop.size_fill"),
            };
            ui.horizontal(|ui| {
                ui.label(crate::lang::text("prop.size_in_group"));
                egui::ComboBox::from_id_salt("ti_size_group").selected_text(shown).show_ui(ui, |ui| {
                    for (choice, text) in [
                        (SizeChoice::Own, own_label),
                        (SizeChoice::Percent, crate::lang::text("prop.size_percent")),
                        (SizeChoice::Fill, crate::lang::text("prop.size_fill")),
                    ] {
                        if ui.selectable_label(current == choice, text).clicked() && current != choice {
                            self.set_size_choice(p, choice);
                        }
                    }
                });
            });
            if self.summary {
                ui.horizontal(|ui| {
                    ui.label(crate::lang::text("prop.placement"));
                    let placement = if self.constraint == ConstraintKind::Alap {
                        ConstraintKind::Alap
                    } else {
                        ConstraintKind::Asap
                    };
                    let shown = if placement == ConstraintKind::Alap {
                        crate::lang::text("prop.placement_alap")
                    } else {
                        crate::lang::text("prop.placement_asap")
                    };
                    egui::ComboBox::from_id_salt("ti_group_placement").selected_text(shown).show_ui(ui, |ui| {
                        for kind in [ConstraintKind::Asap, ConstraintKind::Alap] {
                            let text = if kind == ConstraintKind::Alap {
                                crate::lang::text("prop.placement_alap")
                            } else {
                                crate::lang::text("prop.placement_asap")
                            };
                            if ui.selectable_label(placement == kind, text).clicked() && placement != kind {
                                self.constraint = kind;
                                self.constraint_date.clear();
                            }
                        }
                    });
                });
            }
            if matches!(current, SizeChoice::Percent) || !self.group_percent.trim().is_empty() {
                ui.horizontal(|ui| {
                    ui.label(crate::lang::text("prop.size_percent_field"));
                    if ui.add(egui::TextEdit::singleline(&mut self.group_percent).desired_width(60.0).hint_text("100")).changed()
                        && !self.group_percent.trim().is_empty()
                    {
                        self.fills_group = false;
                        self.duration_mode = DurationMode::Fixed;
                    }
                    ui.label(RichText::new(crate::lang::text("prop.size_percent_suffix")).color(theme::pal().TEXT_WEAK));
                });
            }
        }
        if self.mode == ScheduleMode::Auto {
            let hint = if self.is_multi() {
                "Start and finish of auto-scheduled tasks are calculated from links, constraints and calendars. Set a constraint on the Advanced tab to pin a date."
            } else {
                "Start and finish of auto-scheduled tasks are calculated from links, date rules and calendars. Set a date rule to On to pin a date."
            };
            ui.label(RichText::new(hint).small().color(theme::pal().TEXT_WEAK));
        }
        if let Some(t) = p.task(self.id) {
            ui.add_space(6.0);
            egui::Grid::new("ti_computed").num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Scheduled:").color(theme::pal().TEXT_WEAK));
                if self.milestone {
                    // One moment; work does not apply to a milestone.
                    ui.label(format::date_long(t.sched.start));
                } else {
                    ui.label(format!("{} – {}", format::date_long(t.sched.start), format::date_long(t.sched.finish)));
                }
                ui.end_row();
                if !self.milestone {
                    ui.label(RichText::new("Work:").color(theme::pal().TEXT_WEAK));
                    ui.label(format!("{} planned, {} actual", format_work(t.sched.work), format_work(t.sched.actual_work)));
                    ui.end_row();
                }
                ui.label(RichText::new("Slack:").color(theme::pal().TEXT_WEAK));
                ui.label(format!(
                    "total {}, free {}{}",
                    p.format_duration(t.sched.total_slack, false),
                    p.format_duration(t.sched.free_slack, false),
                    if t.sched.critical { " · critical" } else { "" }
                ));
                ui.end_row();
                ui.label(RichText::new("Cost:").color(theme::pal().TEXT_WEAK));
                ui.label(format::money(p, t.sched.cost));
                ui.end_row();
                for w in &t.sched.warnings {
                    ui.add(crate::icons::image("triangle-alert", 13.0, theme::pal().ERROR));
                    ui.label(RichText::new(w).color(theme::pal().ERROR));
                    ui.end_row();
                }
                if let Some(hint) = crate::columns::warning_hint(&t.sched.warnings) {
                    ui.label("");
                    ui.label(RichText::new(hint).small().color(theme::pal().TEXT_WEAK));
                    ui.end_row();
                }
            });
        }
    }

    fn predecessors(&mut self, ui: &mut Ui, p: &Project) {
        let mut remove = None;
        egui::Grid::new("ti_preds").striped(true).num_columns(5).spacing([10.0, 6.0]).show(ui, |ui| {
            for h in ["ID", "Task Name", "Type", "Lag", ""] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (i, row) in self.preds.iter_mut().enumerate() {
                let id_text = match row.pred {
                    TaskRef::Local(id) => p.row_number(id).map(|r| r.to_string()).unwrap_or_default(),
                    TaskRef::External { task, .. } => format!("ext {}", task.0),
                };
                ui.label(id_text);
                ui.label(p.task_ref_name(&row.pred));
                egui::ComboBox::from_id_salt(("pred_type", i)).selected_text(row.kind.label()).width(170.0).show_ui(ui, |ui| {
                    for k in LinkType::ALL {
                        ui.selectable_value(&mut row.kind, k, k.label());
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut row.lag).desired_width(80.0).hint_text("0d"));
                if crate::icons::button(ui, "x", 18.0, "Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.preds.remove(i);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Add predecessor:");
            egui::ComboBox::from_id_salt("pred_add").selected_text("Choose a task…").width(320.0).show_ui(ui, |ui| {
                for (i, t) in p.tasks.iter().enumerate() {
                    if t.id == self.id || self.preds.iter().any(|r| r.pred == TaskRef::Local(t.id)) {
                        continue;
                    }
                    let label = format!("{}  {}{}", i + 1, "  ".repeat(t.outline_level as usize - 1), t.name);
                    if ui.selectable_label(false, label).clicked() {
                        self.preds.push(PredRow { pred: TaskRef::Local(t.id), kind: LinkType::FS, lag: String::new() });
                    }
                }
                for e in &p.external_projects {
                    for s in &e.tasks {
                        let r = TaskRef::External { project: e.id, task: s.id };
                        if self.preds.iter().any(|x| x.pred == r) {
                            continue;
                        }
                        if ui
                            .add(egui::Button::image_and_text(
                                crate::icons::image("link", 12.0, theme::pal().TEXT),
                                format!("{}\\{}", e.name, s.name),
                            ))
                            .clicked()
                        {
                            self.preds.push(PredRow { pred: r, kind: LinkType::FS, lag: String::new() });
                        }
                    }
                }
            });
        });
        // Typed entry besides the picker: row numbers with optional type and lag ("12", "12FS+2d").
        ui.horizontal(|ui| {
            let id = ui.id().with("pred_typed");
            let mut typed = ui.ctx().data(|d| d.get_temp::<String>(id)).unwrap_or_default();
            let resp =
                ui.add(egui::TextEdit::singleline(&mut typed).desired_width(200.0).hint_text(crate::lang::text("ti.pred_hint")).id(id));
            let add = ui.button("+").on_hover_text(crate::lang::text("ti.pred_hint")).clicked();
            if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || add) && !typed.trim().is_empty() {
                match p.parse_predecessors(&typed) {
                    Ok(deps) => {
                        for d in deps {
                            if d.predecessor != TaskRef::Local(self.id) && !self.preds.iter().any(|r| r.pred == d.predecessor) {
                                self.preds.push(PredRow { pred: d.predecessor, kind: d.kind, lag: d.lag.format(&p.settings.time) });
                            }
                        }
                        self.error = None;
                        typed.clear();
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            ui.ctx().data_mut(|d| d.insert_temp(id, typed));
        });
        ui.label(
            RichText::new(
                "Lag examples: 2d, -1d (lead), 50%, 3ed (elapsed days). Link to tasks in other files via Project > Links Between Projects.",
            )
            .small()
            .color(theme::pal().TEXT_WEAK),
        );
    }

    fn resources_tab(&mut self, ui: &mut Ui, p: &Project) {
        if self.summary {
            let names = p.format_resource_names_only(self.id);
            ui.label("A group's resources are the resources of its tasks.");
            ui.label(if names.is_empty() { "geen" } else { names.as_str() });
            return;
        }
        let mut remove = None;
        egui::Grid::new("ti_res").striped(true).num_columns(5).spacing([10.0, 6.0]).show(ui, |ui| {
            for h in ["Resource Name", "Type", "Units", "Work", ""] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (i, row) in self.resources.iter_mut().enumerate() {
                let r = p.resource(row.resource);
                ui.label(r.map(|r| r.name.clone()).unwrap_or_default());
                ui.label(r.map(|r| r.category.label()).unwrap_or_default());
                ui.add(egui::TextEdit::singleline(&mut row.units).desired_width(60.0));
                ui.add(egui::TextEdit::singleline(&mut row.work).desired_width(70.0).hint_text("auto"));
                if crate::icons::button(ui, "x", 18.0, "Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.resources.remove(i);
        }
        // The same checkbox multi-select as the properties pane: tick to assign, untick to remove.
        ui.horizontal(|ui| {
            ui.label("Add resource:");
            let button = ui.button(crate::lang::text("ti.choose_resources"));
            egui::Popup::from_toggle_button_response(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(220.0);
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for r in &p.resources {
                        let mut on = self.resources.iter().any(|x| x.resource == r.id);
                        if ui.checkbox(&mut on, format!("{} ({})", r.name, r.category.label())).changed() {
                            if on {
                                self.resources.push(ResRow { resource: r.id, units: "100%".into(), work: String::new() });
                            } else {
                                self.resources.retain(|x| x.resource != r.id);
                            }
                        }
                    }
                });
            });
        });
        ui.label(
            RichText::new("Leave Work empty to derive it from the duration (or the estimate for a first assignment).")
                .small()
                .color(theme::pal().TEXT_WEAK),
        );
    }

    fn advanced(&mut self, ui: &mut Ui, p: &Project) {
        egui::Grid::new("ti_adv").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            // Same order as the Planning properties tab: constraint (start-side bound) before deadline.
            ui.label("Constraint type:");
            egui::ComboBox::from_id_salt("ti_ctype").selected_text(self.constraint.label()).width(200.0).show_ui(ui, |ui| {
                for k in ConstraintKind::ALL {
                    ui.selectable_value(&mut self.constraint, k, k.label());
                }
            });
            ui.end_row();
            // The date only means something for a constraint type that takes one.
            ui.label("Constraint date:");
            pick_date_iso(
                ui,
                p,
                "ti_cdate",
                &mut self.constraint_date,
                !self.constraint.is_start_constraint(),
                self.constraint.needs_date(),
                "geen",
            );
            ui.end_row();
            ui.label("Deadline:");
            pick_date_iso(ui, p, "ti_deadline", &mut self.deadline, true, true, "NA");
            ui.end_row();
            ui.label("Task type:");
            egui::ComboBox::from_id_salt("ti_ttype")
                .selected_text(match self.task_type {
                    TaskType::FixedUnits => "Fixed Units",
                    TaskType::FixedDuration => "Fixed Duration",
                    TaskType::FixedWork => "Fixed Work",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.task_type, TaskType::FixedUnits, "Fixed Units");
                    ui.selectable_value(&mut self.task_type, TaskType::FixedDuration, "Fixed Duration");
                    ui.selectable_value(&mut self.task_type, TaskType::FixedWork, "Fixed Work");
                });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut self.effort_driven, "Effort driven");
            ui.end_row();
            ui.label("Calendar:");
            let cal_name = self.calendar.and_then(|c| p.calendar(c)).map(|c| c.name.clone()).unwrap_or_else(|| "None".into());
            egui::ComboBox::from_id_salt("ti_cal").selected_text(cal_name).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.calendar, None, "None");
                for c in &p.calendars {
                    ui.selectable_value(&mut self.calendar, Some(c.id), &c.name);
                }
            });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut self.ignore_res_cal, "Scheduling ignores resource calendars");
            ui.end_row();
            ui.label("Fixed cost:");
            ui.add(egui::TextEdit::singleline(&mut self.fixed_cost).desired_width(100.0));
            ui.end_row();
            ui.label("WBS code:");
            ui.label(p.task_index(self.id).map(|i| p.wbs(i)).unwrap_or_default());
            ui.end_row();
            ui.label("Unique ID:");
            ui.label(self.id.0.to_string());
            ui.end_row();
        });
    }

    fn estimate(&mut self, ui: &mut Ui, p: &Project) {
        ui.label("Estimate the effort before (or instead of) assigning resources. Estimates drive workload and, for the first assignment, the task's work and duration.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.est_kind, EstKind::None, "None");
            ui.radio_value(&mut self.est_kind, EstKind::Complexity, "Complexity points");
            ui.radio_value(&mut self.est_kind, EstKind::ThreePoint, "Three-point (PERT)");
            ui.radio_value(&mut self.est_kind, EstKind::Work, "Work");
        });
        ui.add_space(6.0);
        egui::Grid::new("ti_est").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| match self.est_kind {
            EstKind::None => {}
            EstKind::Complexity => {
                ui.label("Points:");
                ui.add(egui::TextEdit::singleline(&mut self.points).desired_width(80.0));
                ui.end_row();
                ui.label("Conversion:");
                ui.label(format!("1 point = {} (Project > Information)", format_work(p.settings.minutes_per_point)));
                ui.end_row();
                if let Ok(v) = self.points.trim().parse::<f64>() {
                    ui.label("Expected work:");
                    ui.label(format_work(Estimate::Complexity { points: v }.expected_work(&p.settings)));
                    ui.end_row();
                }
            }
            EstKind::ThreePoint => {
                ui.label("Optimistic:");
                ui.add(egui::TextEdit::singleline(&mut self.optimistic).desired_width(80.0).hint_text("e.g. 16h"));
                ui.end_row();
                ui.label("Most likely:");
                ui.add(egui::TextEdit::singleline(&mut self.most_likely).desired_width(80.0));
                ui.end_row();
                ui.label("Pessimistic:");
                ui.add(egui::TextEdit::singleline(&mut self.pessimistic).desired_width(80.0));
                ui.end_row();
                let w = |s: &str| parse_work(s, &p.settings.time);
                if let (Some(o), Some(m), Some(pe)) = (w(&self.optimistic), w(&self.most_likely), w(&self.pessimistic)) {
                    let e = Estimate::ThreePoint { optimistic: o, most_likely: m, pessimistic: pe };
                    ui.label("Expected (PERT):");
                    ui.label(format!("{} ± {}", format_work(e.expected_work(&p.settings)), format_work(e.std_dev())));
                    ui.end_row();
                }
            }
            EstKind::Work => {
                ui.label("Work:");
                ui.add(egui::TextEdit::singleline(&mut self.est_work).desired_width(80.0).hint_text("e.g. 24h"));
                ui.end_row();
            }
        });
    }

    fn custom_tab(&mut self, ui: &mut Ui, p: &Project) {
        egui::Grid::new("ti_custom").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            for row in &mut self.custom {
                let Some(f) = p.custom_field(row.field) else { continue };
                ui.label(&f.name);
                let options: Vec<String> = match f.kind {
                    CustomFieldKind::Flag => vec!["Yes".into(), "No".into(), crate::fields::NO_VALUE.into()],
                    CustomFieldKind::Text if !f.choices.is_empty() => {
                        f.choices.iter().cloned().chain(std::iter::once(crate::fields::NO_VALUE.to_string())).collect()
                    }
                    _ => Vec::new(),
                };
                if f.kind == CustomFieldKind::Date {
                    let salt = format!("ti_custom_date{}", row.field.0);
                    pick_date_iso(ui, p, &salt, &mut row.text, false, true, "geen");
                } else if options.is_empty() {
                    let hint = match f.kind {
                        CustomFieldKind::Duration => "e.g. 3d",
                        CustomFieldKind::Number | CustomFieldKind::Cost => "number",
                        _ => "",
                    };
                    ui.add(egui::TextEdit::singleline(&mut row.text).hint_text(hint).desired_width(220.0));
                } else {
                    let shown = if row.text.is_empty() { crate::fields::NO_VALUE.to_string() } else { row.text.clone() };
                    egui::ComboBox::from_id_salt(("ti_custom", row.field.0)).width(220.0).selected_text(shown).show_ui(ui, |ui| {
                        for o in options {
                            if ui.selectable_label(row.text == o, &o).clicked() {
                                row.text = if o == crate::fields::NO_VALUE { String::new() } else { o };
                            }
                        }
                    });
                }
                ui.end_row();
            }
        });
    }

    fn milestone_tab(&mut self, ui: &mut Ui, p: &Project) {
        ui.label("Deliverable (the output achieved at this milestone):");
        ui.add(egui::TextEdit::singleline(&mut self.deliverable).desired_width(f32::INFINITY));
        ui.add_space(8.0);
        ui.label(RichText::new("Tasks that make up this milestone").strong());
        ui.label(
            RichText::new("The milestone is reached when all of these tasks finish (they act as finish-to-start predecessors).")
                .small()
                .color(theme::pal().TEXT_WEAK),
        );
        let mut remove = None;
        for (i, m) in self.members.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(p.task_ref_name(m));
                if crate::icons::button(ui, "x", 18.0, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.members.remove(i);
        }
        egui::ComboBox::from_id_salt("ms_member_add").selected_text("Add task…").width(320.0).show_ui(ui, |ui| {
            for (i, t) in p.tasks.iter().enumerate() {
                if t.id == self.id || t.is_milestone() || self.members.contains(&TaskRef::Local(t.id)) {
                    continue;
                }
                if ui.selectable_label(false, format!("{}  {}", i + 1, t.name)).clicked() {
                    self.members.push(TaskRef::Local(t.id));
                }
            }
            for e in &p.external_projects {
                for s in &e.tasks {
                    let r = TaskRef::External { project: e.id, task: s.id };
                    if !self.members.contains(&r)
                        && ui
                            .add(egui::Button::image_and_text(
                                crate::icons::image("link", 12.0, theme::pal().TEXT),
                                format!("{}\\{}", e.name, s.name),
                            ))
                            .clicked()
                    {
                        self.members.push(r);
                    }
                }
            }
        });
    }
}

fn pick_date(
    ui: &mut egui::Ui,
    salt: &str,
    current: Option<DateTime>,
    clearable: bool,
    enabled: bool,
    minute: u32,
    empty: &str,
) -> Option<Option<DateTime>> {
    crate::widgets::date_picker::button(ui, crate::widgets::date_picker::DateField { salt, current, clearable, enabled, minute, empty })
}

/// A date picker editing an ISO-text field in place, so the diff-apply keeps comparing text.
/// Clearing writes the empty string (the apply path reads that as NA).
fn pick_date_iso(ui: &mut egui::Ui, p: &Project, salt: &str, text: &mut String, finish: bool, enabled: bool, empty: &str) {
    let minute = if finish { p.settings.time.default_finish_time } else { p.settings.time.default_start_time };
    let trimmed = text.trim();
    let current = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("na") { None } else { p.parse_datetime(trimmed, finish) };
    if let Some(next) = pick_date(ui, salt, current, true, enabled, minute, empty) {
        *text = format::iso(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plancraft_core::dates::{DateExt, ymd};

    const DAY: Minutes = 480;

    fn demo() -> Project {
        plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap())
    }

    #[derive(Clone, Copy)]
    enum LegacyDurationKind {
        Stretch,
        Percent,
    }

    fn legacy_duration_project(kind: LegacyDurationKind) -> (Project, TaskId) {
        let mut p = Project::new("Legacy", ymd(2026, 12, 7).unwrap());
        p.settings.rules.stretch = matches!(kind, LegacyDurationKind::Stretch);
        p.settings.rules.span = matches!(kind, LegacyDurationKind::Percent);
        let _g = p.add_task("Group", None);
        let pred = p.add_task("Predecessor", None);
        let prep = p.add_task("Preparation", None);
        let send = p.add_task("Send", None);
        for id in [pred, prep, send] {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.fills_group = false;
        }
        {
            let t = p.task_mut(pred).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            t.estimated = false;
            t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(ymd(2026, 12, 8).unwrap().and_hms_opt(17, 0, 0).unwrap()) };
        }
        {
            let t = p.task_mut(prep).unwrap();
            t.duration = 5 * DAY;
            t.estimated = false;
            t.duration_mode = DurationMode::Fixed;
            match kind {
                LegacyDurationKind::Stretch => {
                    t.fills_group = true;
                }
                LegacyDurationKind::Percent => {
                    t.duration_percent = Some(50.0);
                }
            }
        }
        {
            let t = p.task_mut(send).unwrap();
            t.duration = DAY;
            t.estimated = false;
            t.bounds.finish_latest = Some(ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap());
            t.bounds.finish_latest_fixed = true;
        }
        p.link(pred.into(), prep, LinkType::FS, Lag::None).unwrap();
        p.link(prep.into(), send, LinkType::FS, Lag::None).unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        (p, prep)
    }

    /// Regression: OK wrote every field back as the dialog opened, undoing what an agent changed
    /// while it was open.
    #[test]
    fn ok_keeps_what_an_agent_changed_while_the_dialog_was_open() {
        let mut p = demo();
        let t = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone() && !t.predecessors.is_empty()).unwrap();
        let (id, pred) = (t.id, t.predecessors[0].predecessor);
        let mut info = TaskInfo::new(&p, id, false).unwrap();

        // Meanwhile an agent renames it, lengthens it, marks progress, unlinks it and adds a note.
        p.task_mut(id).unwrap().name = "Renamed by the agent".into();
        p.set_duration(id, 9 * 480, false).unwrap();
        p.set_percent_complete(id, 30).unwrap();
        p.set_predecessors(id, Vec::new()).unwrap();
        p.task_mut(id).unwrap().priority = 900;
        plancraft_core::schedule::recalculate(&mut p);

        // The user changes only the notes and the deadline.
        info.notes = "Checked with the vendor".into();
        info.deadline = "2027-03-01".into();
        info.apply(&mut p).unwrap();

        let t = p.task(id).unwrap();
        assert_eq!(t.notes, "Checked with the vendor");
        assert!(t.deadline.is_some());
        assert_eq!(t.name, "Renamed by the agent");
        assert_eq!(t.duration, 9 * 480);
        assert_eq!(t.percent_complete, 30);
        assert!(t.predecessors.is_empty(), "the agent's unlink stays, not {pred:?}");
        assert_eq!(t.priority, 900);
    }

    #[test]
    fn task_info_opens_legacy_duration_tasks_as_auto() {
        for kind in [LegacyDurationKind::Stretch, LegacyDurationKind::Percent] {
            let (p, id) = legacy_duration_project(kind);
            let info = TaskInfo::new(&p, id, false).expect("task info opens");
            assert_eq!(info.duration_mode, DurationMode::Auto);
            let task = p.task(id).expect("task exists");
            match kind {
                LegacyDurationKind::Stretch => assert!(task.fills_group),
                LegacyDurationKind::Percent => assert!(task.duration_percent.is_some()),
            }
        }
    }

    #[test]
    fn task_info_apply_preserves_legacy_duration_flags_until_fixed_is_selected() {
        for kind in [LegacyDurationKind::Stretch, LegacyDurationKind::Percent] {
            let (mut p, id) = legacy_duration_project(kind);
            let mut info = TaskInfo::new(&p, id, false).expect("task info opens");
            info.notes.push_str(" changed");
            info.apply(&mut p).expect("noop save applies");
            let task = p.task(id).expect("task exists");
            assert_eq!(task.duration_mode, DurationMode::Fixed);
            match kind {
                LegacyDurationKind::Stretch => assert!(task.fills_group),
                LegacyDurationKind::Percent => assert!(task.duration_percent.is_some()),
            }

            let (mut p, id) = legacy_duration_project(kind);
            let mut info = TaskInfo::new(&p, id, false).expect("task info opens");
            info.duration_mode = DurationMode::Fixed;
            info.apply(&mut p).expect("fixed selection applies");
            let task = p.task(id).expect("task exists");
            assert_eq!(task.duration_mode, DurationMode::Fixed);
            match kind {
                LegacyDurationKind::Stretch => assert!(!task.fills_group),
                LegacyDurationKind::Percent => assert!(task.duration_percent.is_none()),
            }
            assert_eq!(p.settings.rules.stretch, matches!(kind, LegacyDurationKind::Stretch));
            assert_eq!(p.settings.rules.span, matches!(kind, LegacyDurationKind::Percent));
        }
    }

    /// Multiple Task Information: fields that differ start empty, and only the fields changed are
    /// written, to every selected task, which keep everything else of their own.
    #[test]
    fn multiple_task_information_changes_only_what_was_changed_in_every_task() {
        let mut p = demo();
        let picked: Vec<&Task> = p.tasks.iter().filter(|t| !t.sched.is_summary && !t.is_milestone()).take(3).collect();
        let ids: Vec<TaskId> = picked.iter().map(|t| t.id).collect();
        let before: Vec<(String, Minutes, Vec<Dependency>)> =
            picked.iter().map(|t| (t.name.clone(), t.duration, t.predecessors.clone())).collect();
        let milestone = p.tasks.iter().find(|t| t.is_milestone()).unwrap().id;
        let summary = p.tasks.iter().find(|t| t.sched.is_summary).unwrap().id;
        let mut all = ids.clone();
        all.extend([milestone, summary]);
        p.task_mut(ids[0]).unwrap().priority = 700;

        let mut info = TaskInfo::new_multi(&p, &all).unwrap();
        assert!(info.is_multi());
        assert_eq!(info.name, "", "the names differ");
        assert_eq!(info.priority, "", "one priority differs");
        info.priority = "800".into();
        info.notes = "Same note for all".into();
        info.duration = "4d".into();
        info.apply_all(&mut p).unwrap();

        for (id, (name, _, preds)) in ids.iter().zip(&before) {
            let t = p.task(*id).unwrap();
            assert_eq!((t.priority, t.notes.as_str(), &t.name, &t.predecessors), (800, "Same note for all", name, preds));
            assert_eq!(t.duration, 4 * 480);
        }
        let m = p.task(milestone).unwrap();
        assert!(m.is_milestone() && m.notes == "Same note for all", "a milestone keeps no duration");
        assert!(p.task(summary).unwrap().sched.is_summary);

        // Only "Estimated" ticked, with durations that differ: each keeps its own, now estimated.
        assert_eq!(TaskInfo::new_multi(&p, &ids).unwrap().duration, "4 days", "all 4 days now");
        p.set_duration(ids[1], 2 * 480, false).unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        let mut info = TaskInfo::new_multi(&p, &ids).unwrap();
        assert_eq!(info.duration, "");
        info.estimated = true;
        info.apply_all(&mut p).unwrap();
        assert_eq!(p.task(ids[1]).unwrap().duration, 2 * 480);
        assert!(ids.iter().all(|id| p.task(*id).unwrap().estimated));

        // One task: the plain dialog.
        assert!(!TaskInfo::new_multi(&p, &ids[..1]).unwrap().is_multi());
        assert!(TaskInfo::new_multi(&p, &[]).is_none());
    }

    /// Regression: choices and boxes showed the first task's value and were written to every task
    /// when the text field beside them was edited: typing a constraint date turned task B's
    /// Finish No Later Than into A's Start No Earlier Than; a duration made them all estimated.
    #[test]
    fn multiple_task_information_keeps_each_tasks_own_choices() {
        let mut p = demo();
        let ids: Vec<TaskId> = p.tasks.iter().filter(|t| !t.sched.is_summary && !t.is_milestone()).take(2).map(|t| t.id).collect();
        let (a, b) = (ids[0], ids[1]);
        let day = |d| plancraft_core::dates::ymd(2026, 11, d).unwrap().at(8, 0, 0, 0);
        p.task_mut(a).unwrap().constraint = Constraint { kind: ConstraintKind::Snet, date: Some(day(2)) };
        p.task_mut(b).unwrap().constraint = Constraint { kind: ConstraintKind::Fnlt, date: Some(day(20)) };
        p.task_mut(a).unwrap().estimated = true;
        p.task_mut(b).unwrap().estimated = false;
        plancraft_core::schedule::recalculate(&mut p);
        let mut info = TaskInfo::new_multi(&p, &ids).unwrap();
        assert_eq!(info.constraint_date, "", "the dates differ");
        info.constraint_date = "2026-11-25".into();
        info.duration = "4d".into();
        info.apply_all(&mut p).unwrap();
        assert_eq!(p.task(a).unwrap().constraint.kind, ConstraintKind::Snet);
        assert_eq!(p.task(b).unwrap().constraint.kind, ConstraintKind::Fnlt, "B keeps its own constraint type");
        assert!(p.task(a).unwrap().estimated && !p.task(b).unwrap().estimated, "each keeps its own Estimated");
        assert!(ids.iter().all(|id| p.task(*id).unwrap().duration == 4 * 480));

        // Regression: a value typed where a task's own choice can't hold it was dropped: a date
        // for a task As Soon As Possible, points for a task without an estimate.
        p.task_mut(b).unwrap().constraint = Constraint::default();
        p.task_mut(a).unwrap().estimate = Some(Estimate::Complexity { points: 5.0 });
        p.task_mut(b).unwrap().estimate = None;
        plancraft_core::schedule::recalculate(&mut p);
        let mut info = TaskInfo::new_multi(&p, &ids).unwrap();
        info.constraint_date = "2026-11-26".into();
        info.points = "8".into();
        info.apply_all(&mut p).unwrap();
        let cb = p.task(b).unwrap().constraint;
        assert!(cb.kind.needs_date() && cb.date.is_some(), "the date is kept: {cb:?}");
        assert!(ids.iter().all(|id| matches!(p.task(*id).unwrap().estimate, Some(Estimate::Complexity { points }) if points == 8.0)));
    }

    /// Regression: unticking Estimated did nothing: the unchanged duration text ("3 days?") was
    /// read as estimated again.
    #[test]
    fn unticking_estimated_takes() {
        let mut p = demo();
        let ids: Vec<TaskId> = p.tasks.iter().filter(|t| !t.sched.is_summary && !t.is_milestone()).take(2).map(|t| t.id).collect();
        for id in &ids {
            p.task_mut(*id).unwrap().estimated = true;
        }
        plancraft_core::schedule::recalculate(&mut p);
        let mut info = TaskInfo::new(&p, ids[0], false).unwrap();
        assert!(info.duration.ends_with('?'), "{}", info.duration);
        info.estimated = false;
        info.apply(&mut p).unwrap();
        assert!(!p.task(ids[0]).unwrap().estimated);
        let mut info = TaskInfo::new_multi(&p, &ids).unwrap();
        info.estimated = true;
        info.apply_all(&mut p).unwrap();
        let mut info = TaskInfo::new_multi(&p, &ids).unwrap();
        info.estimated = false;
        info.apply_all(&mut p).unwrap();
        assert!(ids.iter().all(|id| !p.task(*id).unwrap().estimated));
    }

    #[test]
    fn ok_writes_the_fields_changed_in_the_dialog() {
        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone()).unwrap().id;
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        info.name = "  New name ".into();
        info.duration = "3d".into();
        info.percent = "50".into();
        info.priority = "700".into();
        info.apply(&mut p).unwrap();
        let t = p.task(id).unwrap();
        assert_eq!((t.name.as_str(), t.duration, t.percent_complete, t.priority), ("New name", 3 * 480, 50, 700));
    }

    /// Resources removed or added in the dialog change only those assignments.
    #[test]
    fn resources_tab_keeps_assignments_an_agent_added() {
        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary && p.assignments_for_task(t.id).count() == 2).unwrap().id;
        let first = p.assignments_for_task(id).next().unwrap().resource;
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        let extra = p.add_resource("Agent's pick");
        p.assign(id, extra, None, None).unwrap();
        info.resources.retain(|r| r.resource != first);
        info.apply(&mut p).unwrap();
        let now: Vec<ResourceId> = p.assignments_for_task(id).map(|a| a.resource).collect();
        assert!(!now.contains(&first), "removed in the dialog");
        assert!(now.contains(&extra), "the agent's assignment stays: {now:?}");
        assert_eq!(now.len(), 2);
    }

    /// Regression: emptying Actual Start or Actual Finish did nothing (an empty field parsed to
    /// "no date", which was skipped).
    #[test]
    fn emptied_actual_dates_are_cleared() {
        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone() && t.percent_complete == 0).unwrap().id;
        p.set_percent_complete(id, 100).unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        let t = p.task(id).unwrap();
        assert!(t.actual_start.is_some() && t.actual_finish.is_some());

        // Back to 0% with both dates emptied (" " and "NA" count as empty): both are cleared.
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        info.percent = "0".into();
        info.actual_start = "NA".into();
        info.actual_finish = "  ".into();
        info.apply(&mut p).unwrap();
        let t = p.task(id).unwrap();
        assert_eq!((t.actual_start, t.actual_finish), (None, None));
    }

    /// Regression (review): clearing Actual Start left a task "half done, never started". A task
    /// that shows progress keeps its start (with a message saying why); entering an Actual Finish
    /// on a task that never started starts it, as in the table.
    #[test]
    fn a_task_with_progress_keeps_its_actual_start() {
        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone() && t.percent_complete == 0).unwrap().id;
        p.set_percent_complete(id, 50).unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        info.actual_start = String::new();
        let e = info.apply(&mut p).unwrap_err();
        assert!(e.contains("% complete to 0"), "{e}");

        // Regression (second review): clearing Actual Finish left a task "100% complete, not
        // finished"; it is refused while the task is 100% complete.
        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone() && t.percent_complete == 0).unwrap().id;
        p.set_percent_complete(id, 100).unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        info.actual_finish = String::new();
        let e = info.apply(&mut p).unwrap_err();
        assert!(e.contains("lower % complete"), "{e}");

        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone() && t.percent_complete == 0).unwrap().id;
        let scheduled = p.task(id).unwrap().sched.start;
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        info.actual_finish = "2026-12-01".into();
        info.apply(&mut p).unwrap();
        let t = p.task(id).unwrap();
        assert_eq!((t.actual_start, t.percent_complete), (scheduled, 100));
    }

    /// An actual date that was already empty when the dialog opened is left alone, so one an
    /// agent entered meanwhile is kept.
    #[test]
    fn untouched_empty_actual_dates_keep_what_an_agent_entered() {
        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone() && t.percent_complete == 0).unwrap().id;
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        assert!(info.actual_start.is_empty() && info.actual_finish.is_empty());
        p.set_percent_complete(id, 100).unwrap();
        info.notes = "Done early".into();
        info.apply(&mut p).unwrap();
        let t = p.task(id).unwrap();
        assert!(t.actual_start.is_some() && t.actual_finish.is_some(), "{:?} {:?}", t.actual_start, t.actual_finish);
    }

    #[test]
    fn an_invalid_fixed_cost_is_refused() {
        let mut p = demo();
        let id = p.tasks.iter().find(|t| !t.sched.is_summary).unwrap().id;
        let mut info = TaskInfo::new(&p, id, false).unwrap();
        info.fixed_cost = "lots".into();
        assert!(info.apply(&mut p).is_err());
    }
}
