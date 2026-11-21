//! Group stretch, percent-of-group, and per-task Auto duration, resolved into working minutes
//! before the CPM passes.
//!
//! Legacy stretch/span run only while [`ScheduleRules`](crate::model::ScheduleRules) is on.
//! [`DurationMode::Auto`] is a per-task opt-in that reuses the same fill path without enabling
//! project-wide rules. The stored duration is left as the fallback; the scheduler reads the
//! resolved minutes for that pass.

use crate::dates::prelude::*;
use crate::duration::{Lag, Minutes};
use crate::ids::TaskId;
use crate::model::*;
use crate::schedule::{self, project_calendar};

/// Resolved lengths for one pass. `None` means "use the stored duration".
#[derive(Clone, Debug, Default)]
pub struct SharePlan {
    pub durations: Vec<Option<Minutes>>,
    pub group_min: Vec<Option<Minutes>>,
    pub notes: Vec<String>,
    pub task_notes: Vec<(TaskId, String)>,
}

#[derive(Clone, Copy)]
enum Share {
    Percent(f64),
    Fill,
}

/// Whether share resolution should run: global stretch/span rules, or any explicit Auto duration.
pub fn needed(p: &Project) -> bool {
    p.settings.rules.enabled() || p.tasks.iter().any(|t| t.duration_mode == DurationMode::Auto)
}

/// Turns stretch, span and Auto-duration rows into concrete durations. The project's own durations are not written.
pub fn resolve(p: &Project) -> SharePlan {
    let n = p.tasks.len();
    let mut plan = SharePlan { durations: vec![None; n], group_min: vec![None; n], notes: Vec::new(), task_notes: Vec::new() };
    if !needed(p) || n == 0 {
        return plan;
    }
    let parent = schedule::parents(p);
    let summary: Vec<bool> = (0..n).map(|i| p.has_children(i)).collect();
    let shares: Vec<Option<Share>> =
        (0..n).map(|i| share_of(p, i, parent.get(i).copied().flatten(), summary.get(i).copied().unwrap_or(false))).collect();
    if shares.iter().all(Option::is_none) {
        // Auto tasks that are unsupported (no parent, own date rules) must still warn honestly.
        note_unresolved_auto(p, &parent, &summary, &shares, &mut plan);
        return plan;
    }

    let mut base = p.clone();
    // Nested recalculate must not re-enter share resolution (legacy clears rules; Auto must too).
    base.settings.rules = ScheduleRules::default();
    for t in &mut base.tasks {
        t.duration_mode = DurationMode::Fixed;
    }
    for (i, share) in shares.iter().enumerate() {
        if share.is_some()
            && !summary.get(i).copied().unwrap_or(false)
            && let Some(t) = base.tasks.get_mut(i)
        {
            t.duration = 0;
        }
    }
    schedule::recalculate(&mut base);

    let cal = project_calendar(p);
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, par) in parent.iter().enumerate() {
        if let Some(list) = par.and_then(|pi| children.get_mut(pi)) {
            list.push(i);
        }
    }
    let mut allotted: Vec<Option<Minutes>> = vec![None; n];
    for g in 0..n {
        let Some(rows) = children.get(g).filter(|r| !r.is_empty()) else { continue };
        let Some(group) = p.tasks.get(g) else { continue };
        let windowed = bound_finish(group).is_some() || allotted.get(g).copied().flatten().is_some();
        let length = group_length(p, &base, g, group, allotted.get(g).copied().flatten(), &shares, rows, &cal);
        let mut remaining = length;
        let mut fills: Vec<usize> = Vec::new();
        for &i in rows {
            match shares.get(i).copied().flatten() {
                Some(Share::Percent(pct)) => {
                    let value = percent_minutes(length, pct);
                    assign(&mut plan, &summary, i, value, &mut allotted);
                    remaining = remaining.saturating_sub(value);
                }
                Some(Share::Fill) => fills.push(i),
                None => {
                    let used = own_minutes(p, &base, i, summary.get(i).copied().unwrap_or(false), &cal);
                    remaining = remaining.saturating_sub(used);
                }
            }
        }
        if fills.is_empty() {
            continue;
        }
        if remaining > 0 {
            let count = fills.len() as Minutes;
            let each = if count > 0 { remaining / count } else { 0 };
            let mut extra = if count > 0 { remaining % count } else { 0 };
            for i in fills {
                let mut value = each;
                if extra > 0 {
                    value = value.saturating_add(1);
                    extra -= 1;
                }
                assign(&mut plan, &summary, i, value, &mut allotted);
            }
        } else {
            // Keep the stored duration as the budget so successor capping can still shrink it
            // (Project Planner leaves it out of shareBudgets; we must not draw past a pinned successor).
            for &i in &fills {
                if summary.get(i).copied().unwrap_or(false) {
                    continue;
                }
                if plan.durations.get(i).copied().flatten().is_none()
                    && let Some(t) = p.tasks.get(i)
                    && let Some(slot) = plan.durations.get_mut(i)
                {
                    *slot = Some(t.duration.max(0));
                }
            }
            let name = group.name.as_str();
            if windowed || length > 0 {
                plan.notes.push(format!("Group {name} has no room left for its unrestricted activities; they keep their own duration"));
            } else {
                plan.notes
                    .push(format!("Group {name} has no window for its stretch or auto-duration activities; they keep their own duration"));
            }
        }
    }
    // Percent / fill rows that never received a group allotment still need a budget for capping.
    for (i, share) in shares.iter().enumerate() {
        if share.is_none() || summary.get(i).copied().unwrap_or(false) {
            continue;
        }
        if plan.durations.get(i).copied().flatten().is_none()
            && let Some(t) = p.tasks.get(i)
            && let Some(slot) = plan.durations.get_mut(i)
        {
            *slot = Some(t.duration.max(0));
        }
    }
    note_unresolved_auto(p, &parent, &summary, &shares, &mut plan);
    cap_for_successors(p, &mut plan);
    plan
}

/// Auto tasks that could not fill (no parent, own dates, or no window) must not look calculated.
fn note_unresolved_auto(p: &Project, parent: &[Option<usize>], summary: &[bool], shares: &[Option<Share>], plan: &mut SharePlan) {
    for (i, t) in p.tasks.iter().enumerate() {
        if t.duration_mode != DurationMode::Auto || summary.get(i).copied().unwrap_or(false) || t.is_milestone() {
            continue;
        }
        if shares.get(i).and_then(|s| *s).is_some() {
            // Got a fill classification; a missing allotment already produced a group note.
            continue;
        }
        let name = t.name.as_str();
        if parent.get(i).copied().flatten().is_none() {
            plan.notes.push(format!("'{name}' is set to Auto duration but has no parent group; it keeps its own duration"));
        } else if t.start_mode() != DateMode::Unrestricted || t.finish_mode() != DateMode::Unrestricted {
            plan.notes.push(format!("'{name}' is set to Auto duration but has its own start or finish rule; it keeps its own duration"));
        }
    }
}

fn assign(plan: &mut SharePlan, summary: &[bool], i: usize, value: Minutes, allotted: &mut [Option<Minutes>]) {
    let value = value.max(0);
    if !summary.get(i).copied().unwrap_or(false) {
        if let Some(slot) = plan.durations.get_mut(i) {
            *slot = Some(value);
        }
        return;
    }
    if let Some(slot) = plan.group_min.get_mut(i) {
        *slot = Some(value);
    }
    if let Some(slot) = allotted.get_mut(i) {
        *slot = Some(value);
    }
}

/// Project Planner `groupShare`: a percentage when set; otherwise fill while legacy `fillsGroup`
/// (with global stretch) or explicit [`DurationMode::Auto`], and both date sides are unrestricted.
/// Any own date on either side keeps the stored duration.
fn share_of(p: &Project, i: usize, parent: Option<usize>, is_summary: bool) -> Option<Share> {
    parent?;
    let t = p.tasks.get(i)?;
    if t.is_milestone() && !is_summary {
        return None;
    }
    if p.settings.rules.span
        && let Some(pct) = t.duration_percent
        && pct.is_finite()
        && pct > 0.0
    {
        return Some(Share::Percent(pct.min(1000.0)));
    }
    let wants_fill = t.duration_mode == DurationMode::Auto || (p.settings.rules.stretch && t.fills_group);
    if wants_fill && t.start_mode() == DateMode::Unrestricted && t.finish_mode() == DateMode::Unrestricted {
        return Some(Share::Fill);
    }
    None
}

fn bound_start(t: &Task) -> Option<DateTime> {
    t.bounds
        .start_earliest
        .or_else(|| t.constraint.date.filter(|_| matches!(t.constraint.kind, ConstraintKind::Snet | ConstraintKind::Mso)))
}

fn bound_finish(t: &Task) -> Option<DateTime> {
    t.bounds
        .finish_latest
        .or(t.deadline)
        .or_else(|| t.constraint.date.filter(|_| matches!(t.constraint.kind, ConstraintKind::Fnlt | ConstraintKind::Mfo)))
}

fn group_length(
    p: &Project,
    base: &Project,
    g: usize,
    group: &Task,
    allotted: Option<Minutes>,
    shares: &[Option<Share>],
    rows: &[usize],
    cal: &crate::calendar::WorkCal<'_>,
) -> Minutes {
    if let Some(n) = allotted {
        return n.max(0);
    }
    let base_start = base.tasks.get(g).and_then(|t| t.sched.start);
    let start = bound_start(group).or(base_start).unwrap_or(p.start);
    if let Some(fin) = bound_finish(group) {
        return cal.between(start, fin).max(0);
    }
    let mut own_start: Option<DateTime> = None;
    let mut own_finish: Option<DateTime> = None;
    for &i in rows {
        if shares.get(i).and_then(|s| *s).is_some() {
            continue;
        }
        let Some(t) = base.tasks.get(i) else { continue };
        if let Some(s) = t.sched.start {
            own_start = Some(own_start.map_or(s, |o| o.min(s)));
        }
        if let Some(f) = t.sched.finish {
            own_finish = Some(own_finish.map_or(f, |o| o.max(f)));
        }
    }
    match (own_start, own_finish) {
        (Some(s), Some(f)) => {
            let from = start.min(s);
            cal.between(from, f).max(0)
        }
        _ => link_span(p, base, g, rows, shares, cal),
    }
}

/// A window implied by a hard successor date when the group has no explicit finish and no
/// sibling span. The stretch child fills up to that date.
fn link_span(
    p: &Project,
    base: &Project,
    g: usize,
    rows: &[usize],
    shares: &[Option<Share>],
    cal: &crate::calendar::WorkCal<'_>,
) -> Minutes {
    let mut best = 0;
    let mut consider = |idx: usize| {
        let Some(task) = p.tasks.get(idx) else { return };
        let Some(start) = base.tasks.get(idx).and_then(|t| t.sched.start) else { return };
        let include_soft = task.duration_mode == DurationMode::Auto;
        let Some(cap) = earliest_successor_cap(p, cal, task.id, include_soft) else { return };
        if cap > start {
            best = best.max(cal.between(start, cap).max(0));
        }
    };
    consider(g);
    for &i in rows {
        if matches!(shares.get(i).copied().flatten(), Some(Share::Fill) | Some(Share::Percent(_))) {
            consider(i);
        }
    }
    best
}

fn own_minutes(p: &Project, base: &Project, i: usize, is_summary: bool, cal: &crate::calendar::WorkCal<'_>) -> Minutes {
    if is_summary {
        let Some(t) = base.tasks.get(i) else { return 0 };
        return match (t.sched.start, t.sched.finish) {
            (Some(s), Some(f)) => cal.between(s, f).max(0),
            _ => 0,
        };
    }
    let Some(t) = p.tasks.get(i) else { return 0 };
    if t.is_milestone() { 0 } else { t.duration.max(0) }
}

fn percent_minutes(length: Minutes, pct: f64) -> Minutes {
    if !pct.is_finite() || pct <= 0.0 || length <= 0 {
        return 0;
    }
    let v = (length as f64) * (pct / 100.0);
    if !v.is_finite() { 0 } else { crate::limits::clamp_minutes(v.round() as Minutes) }
}

/// Project Planner `limitingSuccessorShares`: shrink stretch/percent durations so a fixed
/// successor date is still met. Bars must not finish after a pinned successor; when the window
/// is empty the share collapses to zero and a note is kept (impossible dependency stays visible).
///
/// Rows that were classified as fill/percent but kept their stored duration (no group window)
/// are included too — otherwise a FNLT successor leaves the stretch bar crossing the pin.
fn cap_for_successors(p: &Project, plan: &mut SharePlan) {
    let n = p.tasks.len();
    let parent = schedule::parents(p);
    let summary: Vec<bool> = (0..n).map(|i| p.has_children(i)).collect();
    let mut budgets: Vec<(usize, Minutes, bool)> = Vec::new();
    for i in 0..n {
        let include_soft = p.tasks.get(i).is_some_and(|t| t.duration_mode == DurationMode::Auto);
        if let Some(m) = plan.durations.get(i).copied().flatten() {
            budgets.push((i, m, include_soft));
            continue;
        }
        // No allotment written: still a share row that fell back to its own duration.
        if share_of(p, i, parent.get(i).copied().flatten(), summary.get(i).copied().unwrap_or(false)).is_none() {
            continue;
        }
        let stored = p.tasks.get(i).map(|t| t.duration.max(0)).unwrap_or(0);
        budgets.push((i, stored, include_soft));
    }
    if budgets.is_empty() {
        return;
    }
    let mut probe = p.clone();
    probe.settings.rules = ScheduleRules::default();
    for t in &mut probe.tasks {
        t.duration_mode = DurationMode::Fixed;
    }
    apply_plan_durations(&mut probe, plan);
    schedule::recalculate(&mut probe);
    let cal = project_calendar(p);
    let hard_finish = successor_hard_finishes(p, &probe, &cal, false);
    let soft_finish = budgets.iter().any(|(_, _, include_soft)| *include_soft).then(|| successor_hard_finishes(p, &probe, &cal, true));
    for &(i, current, include_soft) in &budgets {
        let Some(start) = probe.tasks.get(i).and_then(|t| t.sched.start) else { continue };
        let Some(task) = p.tasks.get(i) else { continue };
        let caps = if include_soft { soft_finish.as_ref().unwrap_or(&hard_finish) } else { &hard_finish };
        let Some(cap) = caps.get(i).copied().flatten() else { continue };
        if cap <= start {
            set_duration(plan, i, 0);
            note_cap(plan, task, p, i, current, 0, cap);
            continue;
        }
        let max_dur = cal.between(start, cap).max(0);
        if max_dur >= current {
            continue;
        }
        set_duration(plan, i, max_dur);
        note_cap(plan, task, p, i, current, max_dur, cap);
    }
}

fn set_duration(plan: &mut SharePlan, i: usize, value: Minutes) {
    if let Some(slot) = plan.durations.get_mut(i) {
        *slot = Some(value);
    }
}

fn note_cap(plan: &mut SharePlan, task: &Task, p: &Project, i: usize, current: Minutes, capped: Minutes, cap: DateTime) {
    let policy = if task.duration_mode == DurationMode::Auto {
        "Auto duration"
    } else if task.duration_percent.is_some() {
        "percent share"
    } else {
        "legacy stretch"
    };
    let note = if current == 0 && capped == 0 {
        format!("WBS {}: {policy} has no room left because successor cap is {}", p.wbs(i), cap.format("%Y-%m-%d %H:%M"))
    } else {
        let verb = if capped == 0 { "collapsed" } else { "shortened" };
        format!(
            "WBS {}: {policy} {verb} from {} to {} because successor cap is {}",
            p.wbs(i),
            p.format_duration(current, false),
            p.format_duration(capped, false),
            cap.format("%Y-%m-%d %H:%M"),
        )
    };
    plan.task_notes.push((task.id, note));
}

fn apply_plan_durations(probe: &mut Project, plan: &SharePlan) {
    for (i, d) in plan.durations.iter().enumerate() {
        if let Some(m) = *d
            && let Some(t) = probe.tasks.get_mut(i)
            && !t.is_milestone()
        {
            t.duration = m;
        }
    }
}

/// Latest finish each task may keep so every fixed successor date still holds (Swift `successorHard`
/// on F). Own fixed finishes seed the cap; dependency edges carry it backward with lag.
fn successor_hard_finishes(p: &Project, probe: &Project, cal: &crate::calendar::WorkCal<'_>, include_soft: bool) -> Vec<Option<DateTime>> {
    let n = p.tasks.len();
    let mut hard = vec![None; n];
    for (i, t) in p.tasks.iter().enumerate() {
        if let Some(cap) = own_finish_cap(t, probe.tasks.get(i).map(|r| r.sched.duration).unwrap_or(t.duration), cal, include_soft)
            && let Some(slot) = hard.get_mut(i)
        {
            *slot = Some(cap);
        }
    }
    // Propagate backward in reverse outline order (successors usually sit below predecessors).
    for _ in 0..n.saturating_add(1).min(64) {
        let mut changed = false;
        for succ_i in (0..n).rev() {
            let Some(succ) = p.tasks.get(succ_i) else { continue };
            let succ_hard = hard.get(succ_i).copied().flatten();
            let succ_start_hard = hard_start_of(p, probe, cal, succ_i, succ_hard, include_soft);
            for dep in &succ.predecessors {
                let Some(pred_id) = dep.predecessor.local() else { continue };
                let Some(pred_i) = p.tasks.iter().position(|t| t.id == pred_id) else { continue };
                let Some(at) = link_pred_finish_cap(cal, dep, succ_start_hard, succ_hard) else {
                    continue;
                };
                let slot = match hard.get_mut(pred_i) {
                    Some(s) => s,
                    None => continue,
                };
                let next = match *slot {
                    Some(c) if c <= at => c,
                    _ => {
                        changed = true;
                        at
                    }
                };
                *slot = Some(next);
            }
        }
        if !changed {
            break;
        }
    }
    hard
}

fn link_pred_finish_cap(
    cal: &crate::calendar::WorkCal<'_>,
    dep: &Dependency,
    succ_start_hard: Option<DateTime>,
    succ_finish_hard: Option<DateTime>,
) -> Option<DateTime> {
    let lag = dep.lag;
    match dep.kind {
        LinkType::FS => {
            let hard = succ_start_hard?;
            Some(match lag {
                Lag::None | Lag::Working(0) => hard,
                Lag::Working(m) => cal.sub(hard, m),
                _ => hard,
            })
        }
        LinkType::SS => {
            // A start-to-start hard date still bounds how late the predecessor may run when we
            // only have a finish-side budget: treat it like FS for duration limiting.
            let hard = succ_start_hard?;
            Some(match lag {
                Lag::None | Lag::Working(0) => hard,
                Lag::Working(m) => cal.sub(hard, m),
                _ => hard,
            })
        }
        LinkType::FF => {
            let hard = succ_finish_hard?;
            Some(match lag {
                Lag::None | Lag::Working(0) => hard,
                Lag::Working(m) => cal.sub(hard, m),
                _ => hard,
            })
        }
        LinkType::SF => {
            let hard = succ_finish_hard?;
            Some(match lag {
                Lag::None | Lag::Working(0) => hard,
                Lag::Working(m) => cal.sub(hard, m),
                _ => hard,
            })
        }
    }
}

fn hard_start_of(
    p: &Project,
    probe: &Project,
    cal: &crate::calendar::WorkCal<'_>,
    i: usize,
    finish_hard: Option<DateTime>,
    include_soft: bool,
) -> Option<DateTime> {
    let t = p.tasks.get(i)?;
    if let Some(d) = hard_pinned_start(t) {
        return Some(d);
    }
    let finish =
        finish_hard.or_else(|| own_finish_cap(t, probe.tasks.get(i).map(|r| r.sched.duration).unwrap_or(t.duration), cal, include_soft))?;
    let dur = probe.tasks.get(i).map(|r| r.sched.duration).unwrap_or(t.duration).max(0);
    if dur > 0 { Some(cal.sub(finish, dur)) } else { Some(finish) }
}

fn own_finish_cap(t: &Task, duration: Minutes, cal: &crate::calendar::WorkCal<'_>, include_soft: bool) -> Option<DateTime> {
    if let Some(d) = hard_pinned_start(t) {
        return Some(if duration > 0 { cal.add(d, duration) } else { d });
    }
    if t.bounds.finish_latest_fixed {
        return t.bounds.finish_latest;
    }
    if include_soft && t.bounds.finish_latest.is_some() {
        return t.bounds.finish_latest;
    }
    t.constraint.date.filter(|_| matches!(t.constraint.kind, ConstraintKind::Mfo | ConstraintKind::Fnlt))
}

fn hard_pinned_start(t: &Task) -> Option<DateTime> {
    if t.bounds.start_latest_fixed {
        return t.bounds.start_latest;
    }
    t.constraint.date.filter(|_| matches!(t.constraint.kind, ConstraintKind::Mso | ConstraintKind::Snlt))
}

/// Direct FS/FF hard date of a successor, for sizing a fill window before the CPM probe.
fn earliest_successor_cap(
    p: &Project,
    cal: &crate::calendar::WorkCal<'_>,
    pred: crate::ids::TaskId,
    include_soft: bool,
) -> Option<DateTime> {
    let mut cap: Option<DateTime> = None;
    for succ in &p.tasks {
        for dep in &succ.predecessors {
            if dep.predecessor.local() != Some(pred) {
                continue;
            }
            let hard = match dep.kind {
                LinkType::FS | LinkType::SS => hard_start_for_link(cal, succ, include_soft),
                LinkType::FF | LinkType::SF => own_finish_cap(succ, succ.duration.max(0), cal, include_soft),
            };
            let Some(hard) = hard else { continue };
            let at = match dep.lag {
                Lag::None | Lag::Working(0) => hard,
                Lag::Working(m) => cal.sub(hard, m),
                _ => hard,
            };
            cap = Some(cap.map_or(at, |c| c.min(at)));
        }
    }
    cap
}

/// Pinned start for an FS/SS edge: fixed start, or finish − duration for a fixed finish / FNLT / MFO.
fn hard_start_for_link(cal: &crate::calendar::WorkCal<'_>, t: &Task, include_soft: bool) -> Option<DateTime> {
    if let Some(d) = hard_pinned_start(t) {
        return Some(d);
    }
    let finish = own_finish_cap(t, t.duration.max(0), cal, include_soft)?;
    let dur = t.duration.max(0);
    if dur > 0 { Some(cal.sub(finish, dur)) } else { Some(finish) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::recalculate;

    fn day(p: &Project, n: i64) -> DateTime {
        project_calendar(p).add(p.start, n * p.settings.time.minutes_per_day)
    }

    fn span(p: &Project, id: crate::ids::TaskId) -> Minutes {
        let t = p.task(id).unwrap();
        match (t.sched.start, t.sched.finish) {
            (Some(s), Some(f)) => project_calendar(p).between(s, f),
            _ => t.sched.duration,
        }
    }

    fn group_with_window(days: i64) -> (Project, crate::ids::TaskId) {
        let mut p = Project::new("Shares", crate::dates::ymd(2026, 1, 5).unwrap());
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let end = day(&p, days);
        let start = p.start;
        let t = p.task_mut(g).unwrap();
        t.bounds.start_earliest = Some(start);
        t.bounds.finish_latest = Some(end);
        (p, g)
    }

    #[test]
    fn ruleset_off_keeps_an_fs_chain() {
        let mut p = Project::new("Chain", crate::dates::ymd(2026, 1, 5).unwrap());
        assert!(!p.settings.rules.enabled(), "existing plans leave the ruleset off");
        let a = p.add_task("A", None);
        let b = p.add_task("B", None);
        p.task_mut(a).unwrap().duration = 2 * 480;
        p.task_mut(b).unwrap().duration = 480;
        p.link(a.into(), b, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let af = p.task(a).unwrap().sched.finish.unwrap();
        let bs = p.task(b).unwrap().sched.start.unwrap();
        assert!(bs >= af, "the successor still starts when the predecessor finishes");
        assert_eq!(p.task(a).unwrap().duration, 2 * 480);
        assert_eq!(p.task(b).unwrap().sched.duration, 480);
    }

    #[test]
    fn two_stretch_children_split_a_fixed_window() {
        let (mut p, g) = group_with_window(10);
        let a = p.add_task("A", None);
        let b = p.add_task("B", None);
        for id in [a, b] {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 480;
            t.estimated = false;
        }
        recalculate(&mut p);
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        let (sa, sb) = (span(&p, a), span(&p, b));
        assert!((sa - window / 2).abs() <= 1, "A {sa} vs half {window}");
        assert!((sb - window / 2).abs() <= 1, "B {sb} vs half {window}");
        assert_eq!(p.task(a).unwrap().duration, 480, "the stored duration stays the fallback");
    }

    #[test]
    fn percent_and_stretch_share_twenty_five_and_seventy_five() {
        let (mut p, g) = group_with_window(10);
        let a = p.add_task("Quarter", None);
        let b = p.add_task("Rest", None);
        {
            let t = p.task_mut(a).unwrap();
            t.outline_level = 2;
            t.duration_percent = Some(25.0);
            t.fills_group = false;
            t.duration = 480;
            t.estimated = false;
        }
        {
            let t = p.task_mut(b).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 480;
            t.estimated = false;
        }
        recalculate(&mut p);
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        let (sa, sb) = (span(&p, a), span(&p, b));
        assert!((sa - window / 4).abs() <= 1, "25% is {sa}, window {window}");
        assert!((sb - window * 3 / 4).abs() <= 1, "75% is {sb}, window {window}");
    }

    #[test]
    fn a_lone_stretch_child_matches_the_group() {
        let (mut p, g) = group_with_window(8);
        let a = p.add_task("Only", None);
        let t = p.task_mut(a).unwrap();
        t.outline_level = 2;
        t.fills_group = true;
        t.duration = 480;
        t.estimated = false;
        recalculate(&mut p);
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        assert!((span(&p, a) - window).abs() <= 1, "child {} group {window}", span(&p, a));
    }

    #[test]
    fn start_no_earlier_and_finish_no_later_both_apply() {
        let mut p = Project::new("Bounds", crate::dates::ymd(2026, 1, 5).unwrap());
        p.settings.rules = ScheduleRules::on();
        let a = p.add_task("A", None);
        let earliest = day(&p, 2);
        let no_later = day(&p, 3);
        {
            let t = p.task_mut(a).unwrap();
            t.duration = 5 * 480;
            t.estimated = false;
            t.bounds.start_earliest = Some(earliest);
            t.bounds.finish_latest = Some(no_later);
            t.bounds.finish_latest_fixed = false;
            t.constraint = Constraint::default();
        }
        recalculate(&mut p);
        let t = p.task(a).unwrap();
        assert!(t.sched.start.unwrap() >= earliest, "it does not start before the earliest date");
        assert!(
            t.sched.warnings.iter().any(|w| {
                let w = w.to_lowercase();
                w.contains("no later") || w.contains("no-later") || w.contains("deadline")
            }),
            "a missed finish no-later is reported: {:?}",
            t.sched.warnings
        );
        assert_eq!(t.bounds.start_earliest, Some(earliest));
        assert_eq!(t.bounds.finish_latest, Some(no_later));
        assert_eq!(t.constraint.kind, ConstraintKind::Asap, "the second bound is not folded into the single constraint");
    }

    #[test]
    fn a_lone_stretch_child_fills_the_window_a_successor_sets() {
        let mut p = Project::new("Link", crate::dates::ymd(2026, 1, 5).unwrap());
        p.settings.rules = ScheduleRules::on();
        let _g = p.add_task("Group", None);
        let a = p.add_task("Only", None);
        let later = p.add_task("Later", None);
        {
            let t = p.task_mut(a).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 480;
            t.estimated = false;
        }
        let cap = day(&p, 6);
        p.task_mut(later).unwrap().constraint = Constraint { kind: ConstraintKind::Mso, date: Some(cap) };
        p.link(a.into(), later, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let start = p.task(a).unwrap().sched.start.unwrap();
        let window = project_calendar(&p).between(start, cap);
        assert!(window > 480, "the successor opens a window longer than the stored day");
        assert!((span(&p, a) - window).abs() <= 1, "child {} window {window}", span(&p, a));
    }

    #[test]
    fn a_lone_stretch_child_without_a_window_keeps_its_duration_and_says_so() {
        let mut p = Project::new("Open", crate::dates::ymd(2026, 1, 5).unwrap());
        p.settings.rules = ScheduleRules::on();
        let _g = p.add_task("Group", None);
        let a = p.add_task("Only", None);
        {
            let t = p.task_mut(a).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 3 * 480;
            t.estimated = false;
        }
        let report = recalculate(&mut p);
        assert_eq!(p.task(a).unwrap().sched.duration, 3 * 480, "an unconstrained stretch keeps its own duration");
        assert!(
            report.warnings.iter().any(|w| w.contains("no window") && w.contains("Group")),
            "the missing window is reported: {:?}",
            report.warnings
        );
    }

    #[test]
    fn a_start_bound_cancels_stretching_like_project_planner() {
        let (mut p, g) = group_with_window(8);
        let a = p.add_task("Bound", None);
        let earliest = p.start;
        {
            let t = p.task_mut(a).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 480;
            t.estimated = false;
            t.bounds.start_earliest = Some(earliest);
            t.constraint = Constraint { kind: ConstraintKind::Snet, date: Some(earliest) };
        }
        recalculate(&mut p);
        assert_eq!(p.task(a).unwrap().start_mode(), DateMode::NoEarlier);
        assert_eq!(span(&p, a), 480, "an own start date keeps the stored duration (groupShare is nil)");
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        assert!(window > 480, "the group window is wider than the kept duration");
        assert_eq!(p.task(a).unwrap().duration, 480, "the stored duration stays the fallback");
    }

    #[test]
    fn a_fixed_fnlt_successor_caps_a_stretch_predecessor() {
        let mut p = Project::new("FNLT", crate::dates::ymd(2026, 1, 5).unwrap());
        p.settings.rules = ScheduleRules::on();
        let _g = p.add_task("Group", None);
        let draft = p.add_task("Draft", None);
        let send = p.add_task("Send", None);
        {
            let t = p.task_mut(draft).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 5 * 480;
            t.estimated = false;
        }
        // Finish no later than the end of working day 3 (start + 3 days of work).
        let pin = project_calendar(&p).add(p.start, 3 * 480);
        {
            let t = p.task_mut(send).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 480;
            t.estimated = false;
            t.bounds.finish_latest = Some(pin);
            t.bounds.finish_latest_fixed = true;
        }
        p.link(draft.into(), send, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let df = p.task(draft).unwrap().sched.finish.unwrap();
        let ss = p.task(send).unwrap().sched.start.unwrap();
        assert!(df <= ss, "draft finishes {df} after send starts {ss}");
        assert!(span(&p, draft) < 5 * 480, "the stretch draft was shortened, got {}", span(&p, draft));
        assert_eq!(p.task(send).unwrap().sched.duration, 480, "a finish-bound send does not stretch");
    }

    #[test]
    fn a_zero_budget_share_still_gets_a_no_room_warning() {
        let mut p = Project::new("Zero budget", crate::dates::ymd(2026, 1, 5).unwrap());
        p.settings.rules = ScheduleRules::on();
        let _g = p.add_task("Group", None);
        let row = p.add_task("Row", None);
        let gate = p.add_task("Gate", None);
        let start = p.start;
        {
            let t = p.task_mut(row).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 0;
            t.estimated = false;
        }
        {
            let t = p.task_mut(gate).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            t.estimated = false;
            t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(start) };
        }
        p.link(row.into(), gate, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);

        let warning = p.task(row).unwrap().sched.warnings.first().cloned().unwrap_or_default();
        assert_eq!(p.task(row).unwrap().sched.duration, 0, "the stored duration still stays at zero");
        assert_eq!(p.task(row).unwrap().sched.warnings.len(), 1, "the no-room warning is still attached to the row");
        assert!(warning.contains("has no room left"), "warning: {warning}");
    }
}
