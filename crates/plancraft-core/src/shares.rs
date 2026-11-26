//! Group stretch, percent-of-group, and per-task Auto duration, resolved into working minutes
//! before the CPM passes.
//!
//! Size-in-group (`fills_group` / `duration_percent`) and [`DurationMode::Auto`] drive share
//! resolution without requiring project-wide [`ScheduleRules`](crate::model::ScheduleRules).
//! Global stretch/span flags still force a resolve pass for older plans. The stored duration is
//! left as the fallback; the scheduler reads the resolved minutes for that pass.

use crate::dates::prelude::*;
use crate::duration::Minutes;
use crate::ids::TaskId;
use crate::model::*;
use crate::schedule::{self, project_calendar};

/// Resolved lengths for one pass. `None` means "use the stored duration".
#[derive(Clone, Debug, Default)]
pub struct SharePlan {
    pub durations: Vec<Option<Minutes>>,
    pub group_min: Vec<Option<Minutes>>,
    /// Parent window `[Ws, We]` for nested share groups: ASAP places `L` from `Ws`, ALAP to `We`.
    pub group_start: Vec<Option<DateTime>>,
    pub group_end: Vec<Option<DateTime>>,
    pub notes: Vec<String>,
    pub task_notes: Vec<(TaskId, String)>,
}

#[derive(Clone, Copy)]
enum Share {
    Percent(f64),
    Fill,
    /// Auto with Finish **On**: length is early start → pinned finish, not an equal group fill.
    FinishOn,
}

/// Whether share resolution should run: global stretch/span rules, per-task Auto, or any
/// Size-in-group fill / percent (`fills_group` / positive `duration_percent`) so Remaining space
/// and % of group work without flipping project-wide stretch/span flags.
pub fn needed(p: &Project) -> bool {
    p.settings.rules.enabled()
        || p.tasks.iter().any(|t| {
            t.duration_mode == DurationMode::Auto || t.fills_group || t.duration_percent.is_some_and(|pct| pct.is_finite() && pct > 0.0)
        })
}

/// Turns stretch, span and Auto-duration rows into concrete durations. The project's own durations are not written.
pub fn resolve(p: &Project) -> SharePlan {
    let n = p.tasks.len();
    let mut plan = SharePlan {
        durations: vec![None; n],
        group_min: vec![None; n],
        group_start: vec![None; n],
        group_end: vec![None; n],
        notes: Vec::new(),
        task_notes: Vec::new(),
    };
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
    // Nested recalculate must not re-enter share resolution: clear every flag `needed()` watches.
    base.settings.rules = ScheduleRules::default();
    for t in &mut base.tasks {
        t.duration_mode = DurationMode::Fixed;
        t.fills_group = false;
        t.duration_percent = None;
    }
    for (i, share) in shares.iter().enumerate() {
        // Zero Fill / Finish-On / percent rows so the probe exposes a true early start.
        // Finish-On also drops its finish pin for the probe — otherwise a zero-length bar is
        // pulled onto the On date and start≈pin, which made length fall back to the typed duration.
        if matches!(share, Some(Share::Fill) | Some(Share::FinishOn) | Some(Share::Percent(_)))
            && !summary.get(i).copied().unwrap_or(false)
            && let Some(t) = base.tasks.get_mut(i)
        {
            t.duration = 0;
            if matches!(share, Some(Share::FinishOn)) {
                t.bounds.finish_earliest = None;
                t.bounds.finish_latest = None;
                t.bounds.finish_latest_fixed = false;
            }
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
        // Own deadline/FNLT/finish_latest, else the earliest ancestor finish bound (trickles through
        // unrestricted intermediate summaries so nested Auto/fill can still see a window).
        let finish = bound_finish(group).or_else(|| ancestor_bound_finish(p, &parent, g));
        let windowed = finish.is_some() || allotted.get(g).copied().flatten().is_some();
        let length = group_length(p, &base, g, group, allotted.get(g).copied().flatten(), finish, &parent, &shares, rows, &cal);
        let window_start = bound_start(group)
            .or_else(|| ancestor_bound_start(p, &parent, g))
            .or_else(|| base.tasks.get(g).and_then(|t| t.sched.start))
            .unwrap_or(p.start);
        let window_end =
            finish.or_else(|| base.tasks.get(g).and_then(|t| t.sched.finish)).unwrap_or_else(|| cal.add(window_start, length.max(0)));
        let mut remaining = length;
        let mut fills: Vec<usize> = Vec::new();
        let mut finish_ons: Vec<usize> = Vec::new();
        let mut fixed: Vec<usize> = Vec::new();
        for &i in rows {
            match shares.get(i).copied().flatten() {
                Some(Share::Percent(pct)) => {
                    let value = percent_minutes(length, pct);
                    assign(&mut plan, &summary, i, value, &mut allotted, Some(window_start), Some(window_end));
                    remaining = remaining.saturating_sub(value);
                }
                Some(Share::Fill) => fills.push(i),
                Some(Share::FinishOn) => finish_ons.push(i),
                None => fixed.push(i),
            }
        }
        // Finish-On Auto: duration is early start → pinned finish (the date rule wins over typed
        // length and over equal group fill). Cap at the group window end when the pin is later.
        for &i in &finish_ons {
            let Some(t) = p.tasks.get(i) else { continue };
            let Some(finish_pin) = t.finish_window().1 else { continue };
            let start = base.tasks.get(i).and_then(|r| r.sched.start).unwrap_or(window_start);
            let pin = finish_pin.min(window_end);
            let value = if pin > start { cal.between(start, pin).max(0) } else { 0 };
            assign(&mut plan, &summary, i, value, &mut allotted, Some(start), Some(pin));
            // Serial Finish-On rows consume exclusive time before later fills in the chain.
            if !fills.is_empty() && fixed_precedes_any_fill(p, i, &fills, rows) {
                remaining = remaining.saturating_sub(value);
            }
        }
        // Own-duration siblings only shrink fill budgets when they sit *before* a fill in the
        // dependency order (serial). Parallel own-duration rows overlap in time — subtracting
        // them left Auto tasks with scraps (e.g. 0.2d) after a Fixed 10d neighbour.
        for &i in &fixed {
            if !fills.is_empty() && !fixed_precedes_any_fill(p, i, &fills, rows) {
                continue;
            }
            let used = own_minutes(p, &base, i, summary.get(i).copied().unwrap_or(false), &cal);
            remaining = remaining.saturating_sub(used);
        }
        if fills.is_empty() {
            continue;
        }
        if remaining > 0 {
            // Parallel fill branches each get the whole remaining window: they overlap in time,
            // so dividing by the number of rows would incorrectly make three parallel children
            // look like 33.33% each. Serially linked fills share the window.
            let parallel = fills_are_independent(p, &fills);
            let count = if parallel { 1 } else { fills.len() as Minutes };
            let each = if count > 0 { remaining / count } else { 0 };
            let mut extra = if count > 0 { remaining % count } else { 0 };
            for i in fills {
                let mut value = each;
                if !parallel && extra > 0 {
                    value = value.saturating_add(1);
                    extra -= 1;
                }
                assign(&mut plan, &summary, i, value, &mut allotted, Some(window_start), Some(window_end));
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
    // Nested share groups whose children already need more than the allotment grow to fit (Swift §6.3).
    for (i, share) in shares.iter().enumerate() {
        if share.is_none() || !summary.get(i).copied().unwrap_or(false) {
            continue;
        }
        let Some(allotted_len) = allotted.get(i).copied().flatten().or_else(|| plan.group_min.get(i).copied().flatten()) else {
            continue;
        };
        let base_span = own_minutes(p, &base, i, true, &cal);
        if base_span > allotted_len
            && let Some(name) = p.tasks.get(i).map(|t| t.name.as_str())
        {
            plan.notes.push(format!("Group {name} needs more than its share of its group; it grows to fit its activities"));
        }
    }
    note_unresolved_auto(p, &parent, &summary, &shares, &mut plan);
    cap_for_successors(p, &mut plan);
    plan
}

/// True when none of these sibling fill rows depends directly on another. They are separate
/// parallel branches and can each occupy the same remaining time window.
fn fills_are_independent(p: &Project, fills: &[usize]) -> bool {
    let ids: Vec<TaskId> = fills.iter().filter_map(|&i| p.tasks.get(i).map(|t| t.id)).collect();
    !fills
        .iter()
        .filter_map(|&i| p.tasks.get(i))
        .any(|t| t.predecessors.iter().any(|d| matches!(d.predecessor, TaskRef::Local(id) if ids.contains(&id))))
}

/// Whether a fixed-duration sibling is an ancestor (via local predecessor links among `siblings`)
/// of any fill row. Only then does it consume exclusive time before the fill.
fn fixed_precedes_any_fill(p: &Project, fixed: usize, fills: &[usize], siblings: &[usize]) -> bool {
    let sibling: std::collections::HashSet<usize> = siblings.iter().copied().collect();
    let fill_set: std::collections::HashSet<usize> = fills.iter().copied().collect();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![fixed];
    while let Some(i) = stack.pop() {
        if !seen.insert(i) {
            continue;
        }
        if fill_set.contains(&i) {
            return true;
        }
        let Some(id) = p.tasks.get(i).map(|t| t.id) else { continue };
        for (si, t) in p.tasks.iter().enumerate() {
            if !sibling.contains(&si) {
                continue;
            }
            if t.predecessors.iter().any(|d| d.predecessor.local() == Some(id)) {
                stack.push(si);
            }
        }
    }
    false
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
        if parent.get(i).copied().flatten().is_none() {
            plan.notes.push(format!("'{}' is set to Auto duration but has no parent group; it keeps its own duration", t.name));
        }
    }
}

fn assign(
    plan: &mut SharePlan,
    summary: &[bool],
    i: usize,
    value: Minutes,
    allotted: &mut [Option<Minutes>],
    window_start: Option<DateTime>,
    window_end: Option<DateTime>,
) {
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
    if let Some(slot) = plan.group_start.get_mut(i) {
        *slot = window_start;
    }
    if let Some(slot) = plan.group_end.get_mut(i) {
        *slot = window_end;
    }
}

/// Project Planner `groupShare`: a percentage when set; otherwise fill while `fills_group` or
/// explicit [`DurationMode::Auto`]. Nested summaries take part too (Fill allotment via `group_min`).
/// Global stretch/span flags are no longer required — Size-in-group sets the per-task flags directly.
///
/// Explicit Auto fills the group window unless a Finish **On** pin drives length from early start
/// to that date. Soft finish-no-later alone must not leave Auto on the typed duration. Legacy
/// `fills_group` still needs unrestricted sides. Paired date-driven leaves
/// (`date_duration_bounds`) are never shares — length comes from the window.
fn share_of(p: &Project, i: usize, parent: Option<usize>, is_summary: bool) -> Option<Share> {
    parent?;
    let t = p.tasks.get(i)?;
    if t.is_milestone() && !is_summary {
        return None;
    }
    if !is_summary && t.date_duration_bounds().is_some() {
        return None;
    }
    // Percent wins even when global span is off (Swift `durationPercent`).
    if let Some(pct) = t.duration_percent
        && pct.is_finite()
        && pct > 0.0
    {
        return Some(Share::Percent(pct.min(1000.0)));
    }
    if t.duration_mode == DurationMode::Auto {
        // Finish On is a date rule: duration runs to that pin, not an equal sibling fill.
        if t.finish_mode() == DateMode::On {
            return Some(Share::FinishOn);
        }
        return Some(Share::Fill);
    }
    // Legacy Remaining space (`fills_group`) still requires unrestricted sides.
    if t.fills_group && t.start_mode() == DateMode::Unrestricted && t.finish_mode() == DateMode::Unrestricted {
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

/// Earliest finish bound on an ancestor (deadline / FNLT / MFO / finish_latest). Walks parents only.
fn ancestor_bound_finish(p: &Project, parent: &[Option<usize>], mut i: usize) -> Option<DateTime> {
    let mut best = None;
    while let Some(pi) = parent.get(i).copied().flatten() {
        if let Some(fin) = p.tasks.get(pi).and_then(bound_finish) {
            best = Some(best.map_or(fin, |b: DateTime| b.min(fin)));
        }
        i = pi;
    }
    best
}

/// Latest start bound on an ancestor (SNET / MSO / start_earliest). Walks parents only.
fn ancestor_bound_start(p: &Project, parent: &[Option<usize>], mut i: usize) -> Option<DateTime> {
    let mut best = None;
    while let Some(pi) = parent.get(i).copied().flatten() {
        if let Some(s) = p.tasks.get(pi).and_then(bound_start) {
            best = Some(best.map_or(s, |b: DateTime| b.max(s)));
        }
        i = pi;
    }
    best
}

fn group_length(
    p: &Project,
    base: &Project,
    g: usize,
    group: &Task,
    allotted: Option<Minutes>,
    finish: Option<DateTime>,
    parent: &[Option<usize>],
    shares: &[Option<Share>],
    rows: &[usize],
    cal: &crate::calendar::WorkCal<'_>,
) -> Minutes {
    if let Some(n) = allotted {
        return n.max(0);
    }
    let base_start = base.tasks.get(g).and_then(|t| t.sched.start);
    let start = bound_start(group).or_else(|| ancestor_bound_start(p, parent, g)).or(base_start).unwrap_or(p.start);
    if let Some(fin) = finish {
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
/// successor date is still met. Bars must not finish after a pinned successor. When the window
/// is empty (cap ≤ start), keep the stored duration — a zero bar would hide an impossible
/// dependency (Swift keeps the fallback).
///
/// Rows that were classified as fill/percent but kept their stored duration (no group window)
/// are included too — otherwise a FNLT successor leaves the stretch bar crossing the pin.
fn cap_for_successors(p: &Project, plan: &mut SharePlan) {
    let n = p.tasks.len();
    let parent = schedule::parents(p);
    let summary: Vec<bool> = (0..n).map(|i| p.has_children(i)).collect();
    // (index, original allotment, honour soft successor pins)
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
    let cal = project_calendar(p);
    let hard_finish = successor_hard_finishes(p, &cal, false);
    let soft_finish = budgets.iter().any(|(_, _, include_soft)| *include_soft).then(|| successor_hard_finishes(p, &cal, true));

    // Shrink until stable. Pass 0 zeros share budgets (early-start probe) so inflated fills do not
    // invent a late "no room" start. Later passes re-probe with the capped lengths — soft Finish-On
    // predecessors can still slip under those lengths (cyclus #99/#101 → #102 SS< #129), so a single
    // early probe over-allows duration and the Opv successor never sees the predecessor on its date.
    let mut current: Vec<Minutes> = budgets.iter().map(|(_, m, _)| *m).collect();
    let mut final_cap: Vec<Option<DateTime>> = vec![None; budgets.len()];
    let mut impossible: Vec<bool> = vec![false; budgets.len()];
    const MAX_PASSES: usize = 8;
    for pass in 0..MAX_PASSES {
        let mut probe = p.clone();
        probe.settings.rules = ScheduleRules::default();
        for t in &mut probe.tasks {
            t.duration_mode = DurationMode::Fixed;
            t.fills_group = false;
            t.duration_percent = None;
        }
        apply_plan_durations(&mut probe, plan);
        for (bi, &(i, _, _)) in budgets.iter().enumerate() {
            if let Some(t) = probe.tasks.get_mut(i)
                && !t.is_milestone()
            {
                // Pass 0: earliest ASAP starts. Later: starts under the lengths we just chose.
                t.duration = if pass == 0 { 0 } else { current[bi] };
            }
        }
        schedule::recalculate(&mut probe);

        let mut changed = false;
        for (bi, &(i, _, include_soft)) in budgets.iter().enumerate() {
            let Some(start) = probe.tasks.get(i).and_then(|t| t.sched.start) else { continue };
            // Auto may honour soft successor pins, but a soft cap at or before the early start is not
            // a real window (e.g. soft Start-no-later on a later milestone). Ignore those; only a hard
            // pin may force the typed-duration fallback.
            let hard_cap = hard_finish.get(i).copied().flatten();
            let soft_cap = soft_finish.as_ref().and_then(|v| v.get(i).copied().flatten());
            let cap = if include_soft {
                match (soft_cap.filter(|s| *s > start), hard_cap) {
                    (Some(s), Some(h)) => Some(s.min(h)),
                    (Some(s), None) => Some(s),
                    (None, h) => h,
                }
            } else {
                hard_cap
            };
            let Some(cap) = cap else { continue };
            final_cap[bi] = Some(cap);
            // Impossible window: cap at/before start, or cap after start with no working minutes
            // between (lunch/weekend gap). Keep stored duration — never collapse the bar to zero.
            let max_dur = if cap <= start { 0 } else { cal.between(start, cap).max(0) };
            if max_dur == 0 {
                let kept = p.tasks.get(i).map(|t| t.duration.max(0)).unwrap_or(current[bi]);
                if current[bi] != kept {
                    current[bi] = kept;
                    changed = true;
                }
                impossible[bi] = true;
                continue;
            }
            impossible[bi] = false;
            // Target is min(original fill, room to cap). Must grow back when a later pass moves
            // the start earlier (predecessor Autos also shortened) — otherwise a mid-pass shrink
            // sticks (cyclus #101: 2.4d frozen from a Wed start, then start moved to Tue → ended
            // Thursday instead of Friday before the Monday Opv successor).
            let original = budgets[bi].1;
            let target = max_dur.min(original);
            if current[bi] != target {
                current[bi] = target;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    for (bi, &(i, original, _)) in budgets.iter().enumerate() {
        let Some(task) = p.tasks.get(i) else { continue };
        let capped = current[bi];
        set_duration(plan, i, capped);
        let Some(cap) = final_cap[bi] else { continue };
        if impossible[bi] {
            let policy = if task.duration_mode == DurationMode::Auto {
                "Auto duration"
            } else if task.duration_percent.is_some() {
                "percent share"
            } else {
                "legacy stretch"
            };
            plan.task_notes.push((
                task.id,
                format!(
                    "WBS {}: {policy} has no room left because successor cap is {}; keeping {}",
                    p.wbs(i),
                    cap.format("%Y-%m-%d %H:%M"),
                    p.format_duration(capped, false),
                ),
            ));
        } else if capped < original {
            note_cap(plan, task, p, i, original, capped, cap);
        }
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
///
/// Uses each task's **stored** duration when converting a finish pin into a start bound. The share
/// probe's fill allotments are often far larger than the typed length; subtracting those from a
/// successor MFO/FNLT invented impossible July caps on August Auto bars (cyclus #93–#99 via the
/// ALAP Autofill on the calendar chain after #102).
fn successor_hard_finishes(p: &Project, cal: &crate::calendar::WorkCal<'_>, include_soft: bool) -> Vec<Option<DateTime>> {
    let n = p.tasks.len();
    let mut hard = vec![None; n];
    for (i, t) in p.tasks.iter().enumerate() {
        if let Some(cap) = own_finish_cap(t, t.duration.max(0), cal, include_soft)
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
            let succ_start_hard = hard_start_of(succ, cal, succ_hard, include_soft);
            for dep in &succ.predecessors {
                let Some(pred_id) = dep.predecessor.local() else { continue };
                let Some(pred_i) = p.tasks.iter().position(|t| t.id == pred_id) else { continue };
                let pred_dur = p.tasks.get(pred_i).map(|t| t.duration.max(0)).unwrap_or(0);
                let Some(at) = link_pred_finish_cap(cal, dep, succ_start_hard, succ_hard, pred_dur) else {
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
    pred_duration: Minutes,
) -> Option<DateTime> {
    // FS/SS bound the predecessor's finish by the successor's start; FF/SF by its finish.
    // Lag (working, elapsed, percent) matches CPM via `schedule::unapply_lag`.
    let hard = match dep.kind {
        LinkType::FS | LinkType::SS => succ_start_hard?,
        LinkType::FF | LinkType::SF => succ_finish_hard?,
    };
    Some(schedule::unapply_lag(cal, hard, dep.lag, pred_duration))
}

fn hard_start_of(t: &Task, cal: &crate::calendar::WorkCal<'_>, finish_hard: Option<DateTime>, include_soft: bool) -> Option<DateTime> {
    if let Some(d) = hard_pinned_start(t) {
        return Some(d);
    }
    let dur = t.duration.max(0);
    let finish = finish_hard.or_else(|| own_finish_cap(t, dur, cal, include_soft))?;
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
            let pred_dur = p.task(pred).map(|t| t.duration.max(0)).unwrap_or(0);
            let at = schedule::unapply_lag(cal, hard, dep.lag, pred_dur);
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
    use crate::duration::Lag;
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

    /// Parallel Fixed + Auto: the Fixed neighbour must not eat the Auto budget. Both start from
    /// the same predecessor and overlap — Auto fills the full group window (not window − Fixed).
    #[test]
    fn parallel_fixed_sibling_does_not_starve_auto_duration() {
        let (mut p, g) = group_with_window(10);
        let pred = p.add_task("Pred", None);
        let auto = p.add_task("Auto talks", None);
        let fixed = p.add_task("Fixed writeup", None);
        for id in [pred, auto, fixed] {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.fills_group = false;
            t.estimated = false;
        }
        p.task_mut(pred).unwrap().duration = 480;
        {
            let t = p.task_mut(auto).unwrap();
            t.duration = 96; // 0.2d leftover people used to see when Fixed ate the window
            t.duration_mode = DurationMode::Auto;
        }
        p.task_mut(fixed).unwrap().duration = 10 * 480;
        p.link(pred.into(), auto, LinkType::FS, Lag::None).unwrap();
        p.link(pred.into(), fixed, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        // Pred (1d) is serial before Auto; Fixed writeup is parallel and must not eat the budget.
        let expect = window - 480;
        let auto_span = span(&p, auto);
        assert!(
            (auto_span - expect).abs() <= 1,
            "Auto should get window minus serial pred ({expect}), got {auto_span}; Fixed sibling is parallel"
        );
        assert!(auto_span > 96, "must not leave the old 0.2d scrap");
        assert_eq!(p.task(fixed).unwrap().sched.duration, 10 * 480);
    }

    #[test]
    fn serial_fixed_predecessor_still_shrinks_auto_remaining() {
        let (mut p, g) = group_with_window(10);
        let fixed = p.add_task("Fixed first", None);
        let auto = p.add_task("Auto after", None);
        for id in [fixed, auto] {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.fills_group = false;
            t.estimated = false;
        }
        p.task_mut(fixed).unwrap().duration = 4 * 480;
        {
            let t = p.task_mut(auto).unwrap();
            t.duration = 480;
            t.duration_mode = DurationMode::Auto;
        }
        p.link(fixed.into(), auto, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        let expect = window - 4 * 480;
        let got = span(&p, auto);
        assert!((got - expect).abs() <= 1, "serial Fixed should leave {expect}, got {got}");
    }

    #[test]
    fn parallel_stretch_children_each_fill_the_fixed_window() {
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
        assert!((sa - window).abs() <= 1, "parallel A {sa} vs full window {window}");
        assert!((sb - window).abs() <= 1, "parallel B {sb} vs full window {window}");
        assert_eq!(p.task(a).unwrap().duration, 480, "the stored duration stays the fallback");
    }

    #[test]
    fn serial_stretch_children_split_a_fixed_window() {
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
        p.link(a.into(), b, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        let (sa, sb) = (span(&p, a), span(&p, b));
        assert!((sa - window / 2).abs() <= 1, "serial A {sa} vs half {window}");
        assert!((sb - window / 2).abs() <= 1, "serial B {sb} vs half {window}");
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

    /// Parent deadline → unrestricted intermediate summary → Auto leaf: the ancestor finish bound
    /// trickles down so the nested Auto fills the window (no “no window” note).
    #[test]
    fn ancestor_deadline_windows_auto_under_unrestricted_summary() {
        let mut p = Project::new("Trickle", crate::dates::ymd(2026, 1, 5).unwrap());
        assert!(!p.settings.rules.enabled(), "Auto alone; no project-wide stretch");
        let root = p.add_task("1", None);
        let nest = p.add_task("1.3.1", None);
        let leaf = p.add_task("Leaf", None);
        p.task_mut(nest).unwrap().outline_level = 2;
        // Own size on the nest so only the ancestor deadline supplies P (not a Fill allotment).
        p.task_mut(nest).unwrap().fills_group = false;
        {
            let t = p.task_mut(leaf).unwrap();
            t.outline_level = 3;
            t.duration = 480;
            t.estimated = false;
            t.fills_group = false;
            t.duration_mode = DurationMode::Auto;
        }
        let deadline = day(&p, 10);
        p.task_mut(root).unwrap().deadline = Some(deadline);
        assert_eq!(p.task(nest).unwrap().start_mode(), DateMode::Unrestricted);
        assert_eq!(p.task(nest).unwrap().finish_mode(), DateMode::Unrestricted);
        assert!(!p.task(nest).unwrap().fills_group);

        let report = recalculate(&mut p);
        assert!(!report.warnings.iter().any(|w| w.contains("no window")), "ancestor deadline must supply a window: {:?}", report.warnings);
        let window = project_calendar(&p).between(p.start, deadline);
        assert!(window > 480, "deadline opens a multi-day window");
        assert!((span(&p, leaf) - window).abs() <= 1, "Auto leaf {} vs window {window}", span(&p, leaf));
        assert_eq!(p.task(leaf).unwrap().duration, 480, "stored duration stays the fallback");
    }

    /// Auto + Finish On: the pin drives length from early start (not equal group fill).
    #[test]
    fn auto_with_finish_on_uses_pin_not_equal_fill() {
        let (mut p, g) = group_with_window(10);
        let pred = p.add_task("Pred", None);
        let pinned = p.add_task("Finish on", None);
        let other = p.add_task("Other auto", None);
        for id in [pred, pinned, other] {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.fills_group = false;
            t.estimated = false;
        }
        p.task_mut(pred).unwrap().duration = 0; // milestone-like early start at group start
        let on = day(&p, 3);
        {
            let t = p.task_mut(pinned).unwrap();
            t.duration = 10 * 480;
            t.duration_mode = DurationMode::Auto;
            t.bounds.finish_earliest = Some(on);
            t.bounds.finish_latest = Some(on);
        }
        {
            let t = p.task_mut(other).unwrap();
            t.duration = 480;
            t.duration_mode = DurationMode::Auto;
        }
        p.link(pred.into(), pinned, LinkType::FS, Lag::None).unwrap();
        p.link(pred.into(), other, LinkType::FS, Lag::None).unwrap();
        assert!(
            share_of(&p, p.task_index(pinned).unwrap(), Some(p.task_index(g).unwrap()), false)
                .is_some_and(|s| matches!(s, Share::FinishOn)),
            "Finish On must select pin-driven Auto, not equal Fill"
        );
        recalculate(&mut p);
        let start = p.task(pinned).unwrap().sched.start.unwrap();
        let want = project_calendar(&p).between(start, on);
        assert!(
            (span(&p, pinned) - want).abs() <= 1,
            "Auto+Finish On span {} vs start→pin {want} (not equal-fill scraps)",
            span(&p, pinned)
        );
        assert_eq!(p.task(pinned).unwrap().sched.finish, Some(on));
    }

    /// Soft finish-no-later on Auto must not leave the typed duration in place (PLANNER 91/92).
    /// Rules win: Auto still classifies as Fill; the soft No-later then caps length.
    #[test]
    fn auto_with_soft_finish_no_later_still_fills() {
        let (mut p, g) = group_with_window(10);
        let a = p.add_task("Short scrap", None);
        let b = p.add_task("Soft pin", None);
        let stored_long = 10 * 480;
        for (id, stored) in [(a, 480), (b, stored_long)] {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.duration = stored;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
            t.fills_group = false;
        }
        let soft_cap = day(&p, 3);
        {
            let t = p.task_mut(b).unwrap();
            t.bounds.finish_latest = Some(soft_cap);
            t.bounds.finish_latest_fixed = false;
        }
        assert_eq!(p.task(b).unwrap().finish_mode(), DateMode::NoLater);
        let bi = p.task_index(b).unwrap();
        let gi = p.task_index(g).unwrap();
        assert!(
            share_of(&p, bi, Some(gi), false).is_some_and(|s| matches!(s, Share::Fill)),
            "soft No-later must not block Auto fill classification"
        );
        recalculate(&mut p);
        let window = {
            let t = p.task(g).unwrap();
            project_calendar(&p).between(t.bounds.start_earliest.unwrap(), t.bounds.finish_latest.unwrap())
        };
        let soft_span = project_calendar(&p).between(p.start, soft_cap);
        assert!((span(&p, b) - soft_span).abs() <= 1, "soft No-later overrules typed 10d: got {} soft {soft_span}", span(&p, b));
        assert!(span(&p, b) < stored_long, "typed duration must not win over the soft rule");
        // Parallel Auto siblings each take the full remaining window (not a half-split).
        assert!((span(&p, a) - window).abs() <= 1, "sibling Auto fills the group window: got {} window {window}", span(&p, a));
    }

    /// Nested unrestricted summary with fills_group under a parent deadline: the nest is Fill,
    /// children (Auto/fill) resolve inside the allotted window — no “no window” note.
    #[test]
    fn nested_fill_summary_allots_window_without_global_rules() {
        let mut p = Project::new("Nested fill", crate::dates::ymd(2026, 1, 5).unwrap());
        assert!(!p.settings.rules.enabled());
        let root = p.add_task("1", None);
        let nest = p.add_task("1.3.1", None);
        let leaf = p.add_task("Leaf", None);
        p.task_mut(root).unwrap().fills_group = false;
        {
            let t = p.task_mut(nest).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
        }
        {
            let t = p.task_mut(leaf).unwrap();
            t.outline_level = 3;
            t.duration = 480;
            t.estimated = false;
            t.fills_group = true;
            t.duration_mode = DurationMode::Fixed;
        }
        let deadline = day(&p, 10);
        p.task_mut(root).unwrap().deadline = Some(deadline);
        assert!(share_of(&p, 1, Some(0), true).is_some_and(|s| matches!(s, Share::Fill)));

        let report = recalculate(&mut p);
        assert!(
            !report.warnings.iter().any(|w| w.contains("no window")),
            "fill nest must receive an allotted window: {:?}",
            report.warnings
        );
        let window = project_calendar(&p).between(p.start, deadline);
        assert!((span(&p, leaf) - window).abs() <= 1, "fill leaf {} vs window {window}", span(&p, leaf));
    }

    /// Size-in-group Own on a nested summary: no fill allotment from the parent.
    #[test]
    fn nested_own_summary_gets_no_fill_allotment() {
        let mut p = Project::new("Own nest", crate::dates::ymd(2026, 1, 5).unwrap());
        assert!(!p.settings.rules.enabled());
        let root = p.add_task("1", None);
        let nest = p.add_task("1.3.1", None);
        let leaf = p.add_task("Leaf", None);
        p.task_mut(root).unwrap().fills_group = false;
        {
            let t = p.task_mut(nest).unwrap();
            t.outline_level = 2;
            t.fills_group = false;
        }
        {
            let t = p.task_mut(leaf).unwrap();
            t.outline_level = 3;
            t.duration = 480;
            t.estimated = false;
            t.fills_group = false;
            t.duration_mode = DurationMode::Fixed;
        }
        p.task_mut(root).unwrap().deadline = Some(day(&p, 10));
        assert!(share_of(&p, 1, Some(0), true).is_none(), "Own summary is not a share");
        recalculate(&mut p);
        assert_eq!(span(&p, leaf), 480, "own leaf keeps stored duration");
        // Nest rolls up from the leaf; it does not expand to the parent deadline window.
        assert_eq!(span(&p, nest), 480, "own nest spans its activities only");
        let window = project_calendar(&p).between(p.start, day(&p, 10));
        assert!(window > 480);
        assert!(span(&p, nest) < window, "own nest must not fill the parent window");
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
    fn nested_share_overflow_warns_and_grows() {
        let mut p = Project::new("Overflow", crate::dates::ymd(2026, 1, 5).unwrap());
        let root = p.add_task("Parent", None);
        let nest = p.add_task("Nest", None);
        let long = p.add_task("Long", None);
        let short_deadline = day(&p, 5);
        {
            let t = p.task_mut(root).unwrap();
            t.fills_group = false;
            t.deadline = Some(short_deadline); // short parent window
        }
        {
            let t = p.task_mut(nest).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
        }
        {
            let t = p.task_mut(long).unwrap();
            t.outline_level = 3;
            t.fills_group = false;
            t.duration = 8 * 480; // longer than parent window
            t.estimated = false;
        }
        let report = recalculate(&mut p);
        assert!(report.warnings.iter().any(|w| w.contains("needs more than its share")), "overflow note: {:?}", report.warnings);
        assert!(span(&p, nest) >= 8 * 480 - 1, "nest grows to fit long child");
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
        assert!(warning.contains("no room") || warning.contains("keeping"), "warning: {warning}");
    }

    #[test]
    fn impossible_successor_cap_keeps_nonzero_stored_duration() {
        let mut p = Project::new("Impossible", crate::dates::ymd(2026, 1, 5).unwrap());
        p.settings.rules = ScheduleRules::on();
        let _g = p.add_task("Group", None);
        let row = p.add_task("Row", None);
        let gate = p.add_task("Gate", None);
        let start = p.start;
        {
            let t = p.task_mut(row).unwrap();
            t.outline_level = 2;
            t.fills_group = true;
            t.duration = 3 * 480;
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
        assert_eq!(p.task(row).unwrap().sched.duration, 3 * 480, "impossible cap must keep the stored duration, not collapse to zero");
        assert!(
            p.task(row).unwrap().sched.warnings.iter().any(|w| w.contains("no room left")),
            "impossible window is still warned: {:?}",
            p.task(row).unwrap().sched.warnings
        );
    }

    /// Soft start-no-later on a successor must not invent an Auto predecessor cap before the
    /// real schedule (cyclus #99/#100: soft start_latest 18 Aug → bogus "17 Aug" cap).
    #[test]
    fn soft_successor_start_no_later_does_not_cap_auto_before_its_start() {
        let mut p = Project::new("SoftSL", crate::dates::ymd(2027, 8, 2).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let auto = p.add_task("Write", None);
        let mile = p.add_task("Gate", None);
        let g_end = day(&p, 40);
        let g_start = p.start;
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [auto, mile] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        {
            let t = p.task_mut(auto).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
        }
        let on = day(&p, 26);
        let soft_sl = day(&p, 12); // ~18 Aug region
        {
            let t = p.task_mut(mile).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            // Finish On + soft Start no-later (cyclus #100 pattern).
            t.bounds.finish_earliest = Some(on);
            t.bounds.finish_latest = Some(on);
            t.bounds.start_latest = Some(soft_sl);
            t.bounds.start_latest_fixed = false;
        }
        p.link(auto.into(), mile, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let t = p.task(auto).unwrap();
        assert!(
            !t.sched.warnings.iter().any(|w| w.contains("no room left")),
            "soft successor start_latest must not invent an impossible Auto cap: {:?}",
            t.sched.warnings
        );
        assert!(t.sched.duration > 480, "Auto should still fill toward the Finish On gate, got {}", t.sched.duration);
    }

    /// Soft finish-no-later on a successor that lands before Auto's probe start must be ignored
    /// (cyclus soft Start-no-later on #100), not treated as "no room left".
    #[test]
    fn soft_successor_finish_before_auto_start_is_ignored() {
        let mut p = Project::new("SoftJuly", crate::dates::ymd(2027, 7, 5).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let early = p.add_task("Early", None);
        let auto = p.add_task("Late Auto", None);
        let g_start = p.start;
        let g_end = day(&p, 50);
        let soft_july = day(&p, 10);
        let auto_snet = day(&p, 25); // August region
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [early, auto] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        {
            let t = p.task_mut(early).unwrap();
            t.duration = 5 * 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Fixed;
            // Soft FNLT in July — earlier than Auto's August start after pred chain.
            t.bounds.finish_latest = Some(soft_july);
            t.bounds.finish_latest_fixed = false;
        }
        {
            let t = p.task_mut(auto).unwrap();
            t.duration = 2 * 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
            t.bounds.start_earliest = Some(auto_snet);
        }
        // Soft FNLT on Early is Auto's soft successor cap (before Auto's SNET start).
        p.link(auto.into(), early, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let t = p.task(auto).unwrap();
        assert!(
            !t.sched.warnings.iter().any(|w| w.contains("no room left")),
            "soft July successor finish must not invent no-room: {:?}",
            t.sched.warnings
        );
        assert!(t.sched.duration > 0, "Auto must keep a positive duration");
    }

    /// Successor MFO must use stored duration when backing into a start cap — not the probe's
    /// inflated Auto fill (cyclus #129 ALAP Auto + #130 MFO pulled July caps onto #93–#99).
    #[test]
    fn successor_cap_uses_stored_duration_not_fill_allotment() {
        let mut p = Project::new("FillCap", crate::dates::ymd(2027, 8, 2).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let write = p.add_task("Write", None);
        let gate = p.add_task("Gate", None);
        let send = p.add_task("Send", None); // Auto in a wide window → huge fill allotment
        let board = p.add_task("Board", None); // MFO
        let g_start = p.start;
        let g_end = day(&p, 45);
        let mfo = day(&p, 40);
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [write, gate, send, board] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        {
            let t = p.task_mut(write).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
        }
        {
            let t = p.task_mut(gate).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
        }
        {
            let t = p.task_mut(send).unwrap();
            t.duration = 480; // stored 1d; fill would want many weeks
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
            t.constraint = Constraint { kind: ConstraintKind::Alap, date: None };
        }
        {
            let t = p.task_mut(board).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            t.constraint = Constraint { kind: ConstraintKind::Mfo, date: Some(mfo) };
        }
        p.link(write.into(), gate, LinkType::FS, Lag::None).unwrap();
        p.link(gate.into(), send, LinkType::SS, Lag::None).unwrap();
        p.link(send.into(), board, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let t = p.task(write).unwrap();
        assert!(
            !t.sched.warnings.iter().any(|w| w.contains("no room left")),
            "MFO minus fill allotment must not invent no-room: {:?}",
            t.sched.warnings
        );
        assert!(t.sched.duration > 0);
    }

    #[test]
    fn on_on_leaf_gets_date_driven_duration() {
        let mut p = Project::new("OnOn", crate::dates::ymd(2026, 1, 5).unwrap());
        let a = p.add_task("Paired", None);
        let start = p.start;
        let finish = day(&p, 4);
        {
            let t = p.task_mut(a).unwrap();
            t.fills_group = false;
            t.duration = 480; // stored fallback; scheduled length comes from On/On
            t.estimated = false;
            t.bounds.start_earliest = Some(start);
            t.bounds.start_latest = Some(start);
            t.bounds.start_latest_fixed = true;
            t.bounds.finish_earliest = Some(finish);
            t.bounds.finish_latest = Some(finish);
            t.bounds.finish_latest_fixed = true;
        }
        assert!(p.task(a).unwrap().date_duration_bounds().is_some());
        recalculate(&mut p);
        let window = project_calendar(&p).between(start, finish);
        assert!((span(&p, a) - window).abs() <= 1, "On/On span {} vs window {window}", span(&p, a));
        assert_eq!(p.task(a).unwrap().duration, 480, "stored duration stays the fallback");
    }

    fn fill_before_mfo(lag: Lag) -> (Project, crate::ids::TaskId) {
        let mut p = Project::new("LagCap", crate::dates::ymd(2026, 1, 5).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let fill = p.add_task("Fill", None);
        let gate = p.add_task("Gate", None);
        let g_start = p.start;
        let g_end = day(&p, 10);
        let mfo = day(&p, 5);
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [fill, gate] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        {
            let t = p.task_mut(fill).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
        }
        {
            let t = p.task_mut(gate).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            t.constraint = Constraint { kind: ConstraintKind::Mfo, date: Some(mfo) };
        }
        p.link(fill.into(), gate, LinkType::FS, lag).unwrap();
        recalculate(&mut p);
        (p, fill)
    }

    #[test]
    fn successor_cap_elapsed_lag_shortens_like_cpm() {
        let (none, fill_n) = fill_before_mfo(Lag::None);
        let (elapsed, fill_e) = fill_before_mfo(Lag::Elapsed(3 * 24 * 60)); // 3ed
        let (working, fill_w) = fill_before_mfo(Lag::Working(3 * 480)); // 3 working days
        let d_none = none.task(fill_n).unwrap().sched.duration;
        let d_elapsed = elapsed.task(fill_e).unwrap().sched.duration;
        let d_working = working.task(fill_w).unwrap().sched.duration;
        assert!(d_elapsed < d_none, "elapsed lag must pull the Auto cap earlier than none: elapsed={d_elapsed} none={d_none}");
        assert!(d_working < d_none, "working lag should shorten more than none: working={d_working} none={d_none}");
    }

    #[test]
    fn successor_cap_percent_lag_shortens_like_cpm() {
        let (none, fill_n) = fill_before_mfo(Lag::None);
        let (pct, fill_p) = fill_before_mfo(Lag::Percent(50));
        let d_none = none.task(fill_n).unwrap().sched.duration;
        let d_pct = pct.task(fill_p).unwrap().sched.duration;
        assert!(d_pct < d_none, "percent lag must pull the Auto cap earlier than none: pct={d_pct} none={d_none}");
    }

    /// Re-cap must grow back when a later pass opens the window (predecessor Auto shortened and
    /// the start moved earlier). Otherwise a mid-pass shrink sticks and the bar ends early.
    #[test]
    fn auto_recap_grows_back_when_start_moves_earlier() {
        let mut p = Project::new("GrowBack", crate::dates::ymd(2027, 8, 30).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let write = p.add_task("Write", None); // Auto — soft-capped by gate On
        let gate = p.add_task("Gate", None); // soft Finish On
        let fix = p.add_task("Fix", None); // Auto — soft-capped by board On via milestone
        let send = p.add_task("Send", None);
        let board = p.add_task("Board", None);
        let g_start = p.start;
        let g_end = day(&p, 40);
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [write, gate, fix, send, board] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        for id in [write, fix] {
            let t = p.task_mut(id).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
        }
        {
            let t = p.task_mut(gate).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
        }
        {
            let t = p.task_mut(send).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
        }
        {
            let t = p.task_mut(board).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.constraint = Constraint { kind: ConstraintKind::Alap, date: None };
        }
        // Soft On gate ~1 week; soft On board ~2 weeks — same shape as cyclus #100 / #129.
        let gate_on = day(&p, 5);
        let board_start = day(&p, 10);
        let board_finish = project_calendar(&p).add(board_start, 480);
        p.set_date_mode(gate, true, crate::model::DateMode::On).unwrap();
        p.set_date_window(gate, true, Some(gate_on), Some(gate_on)).unwrap();
        p.set_date_mode(board, false, crate::model::DateMode::On).unwrap();
        p.set_date_window(board, false, Some(board_start), Some(board_start)).unwrap();
        p.set_date_mode(board, true, crate::model::DateMode::On).unwrap();
        p.set_date_window(board, true, Some(board_finish), Some(board_finish)).unwrap();
        p.link(write.into(), gate, LinkType::FS, Lag::None).unwrap();
        p.link(gate.into(), fix, LinkType::FS, Lag::None).unwrap();
        p.link(fix.into(), send, LinkType::FS, Lag::None).unwrap();
        p.link_drive(send.into(), board, LinkType::SS, Lag::None, Some(crate::model::LinkDrive::FromSuccessor)).unwrap();
        recalculate(&mut p);
        let fix_t = p.task(fix).unwrap();
        let board_s = p.task(board).unwrap().sched.start.unwrap();
        let send_s = p.task(send).unwrap().sched.start.unwrap();
        assert_eq!(send_s, board_s, "Opv SS aligns send to board");
        // Fix should use the full Tue→cap window (end of last workday before/at the Monday pin),
        // not a mid-pass 2.4d leftover from when write still pushed the start later.
        let start = fix_t.sched.start.unwrap();
        let finish = fix_t.sched.finish.unwrap();
        let room = project_calendar(&p).between(start, board_s);
        assert!(
            (fix_t.sched.duration - room).abs() <= 1,
            "Fix must refill opened window: dur={} room={} start={} finish={} board={}",
            fix_t.sched.duration,
            room,
            start,
            finish,
            board_s
        );
    }

    /// Soft On successor via SS FromSuccessor (Opv): Auto before the milestone must re-cap after
    /// the early probe so the milestone can sit on the successor's start (cyclus #101 → #102 SS< #129).
    #[test]
    fn auto_recap_lets_opv_ss_milestone_meet_soft_on_successor() {
        let mut p = Project::new("OpvSS", crate::dates::ymd(2027, 8, 30).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let write = p.add_task("Write", None); // Auto — equal fill would be huge
        let send = p.add_task("Send milestone", None);
        let board = p.add_task("Board docs", None); // soft On + ALAP, SS< from send
        let g_start = p.start;
        let g_end = day(&p, 40);
        let board_start_pin = day(&p, 10);
        let board_finish_pin = project_calendar(&p).add(board_start_pin, 480);
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [write, send, board] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        {
            let t = p.task_mut(write).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
        }
        {
            let t = p.task_mut(send).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
        }
        {
            let t = p.task_mut(board).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.constraint = Constraint { kind: ConstraintKind::Alap, date: None };
        }
        // Soft On (not fixed) — same shape as cyclus 6.6.4.
        p.set_date_mode(board, false, crate::model::DateMode::On).unwrap();
        p.set_date_window(board, false, Some(board_start_pin), Some(board_start_pin)).unwrap();
        p.set_date_mode(board, true, crate::model::DateMode::On).unwrap();
        p.set_date_window(board, true, Some(board_finish_pin), Some(board_finish_pin)).unwrap();
        p.link(write.into(), send, LinkType::FS, Lag::None).unwrap();
        p.link_drive(send.into(), board, LinkType::SS, Lag::None, Some(crate::model::LinkDrive::FromSuccessor)).unwrap();
        recalculate(&mut p);
        let board_start = p.task(board).unwrap().sched.start.unwrap();
        let send_start = p.task(send).unwrap().sched.start.unwrap();
        assert_eq!(
            send_start, board_start,
            "Opv SS must pull the milestone onto the soft-On successor start; send={send_start} board={board_start}"
        );
        let write_t = p.task(write).unwrap();
        assert!(
            write_t.sched.finish.unwrap() <= board_start,
            "Auto must finish by the Opv successor start after re-cap; finish={} board={}",
            write_t.sched.finish.unwrap(),
            board_start
        );
    }

    /// Inflated Auto fill must not use its own late probe start for capping (cyclus #101: 19d
    /// fill slipped a soft Finish-On pred and made the hard successor look like "no room").
    #[test]
    fn successor_cap_uses_early_start_not_inflated_fill_start() {
        let mut p = Project::new("EarlyCap", crate::dates::ymd(2027, 8, 2).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let write = p.add_task("Write", None);
        let gate = p.add_task("Gate", None); // soft Finish On
        let fix = p.add_task("Fix", None); // Auto after gate
        let board = p.add_task("Board", None); // hard MFO
        let g_start = p.start;
        let g_end = day(&p, 60);
        let on = day(&p, 20);
        let soft_fl = day(&p, 24);
        let mfo = day(&p, 35);
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [write, gate, fix, board] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        {
            let t = p.task_mut(write).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
        }
        {
            let t = p.task_mut(gate).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            t.bounds.finish_earliest = Some(on);
            t.bounds.finish_latest = Some(on); // On, but not fixed — can slip under pressure
        }
        {
            let t = p.task_mut(fix).unwrap();
            t.duration = 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
            t.bounds.finish_latest = Some(soft_fl);
            t.bounds.finish_latest_fixed = false;
        }
        {
            let t = p.task_mut(board).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            t.constraint = Constraint { kind: ConstraintKind::Mfo, date: Some(mfo) };
        }
        p.link(write.into(), gate, LinkType::FS, Lag::None).unwrap();
        p.link(gate.into(), fix, LinkType::FS, Lag::None).unwrap();
        p.link(fix.into(), board, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let t = p.task(fix).unwrap();
        assert!(
            !t.sched.warnings.iter().any(|w| w.contains("no room left") && w.contains("keeping")),
            "must not keep stored from a bogus late fill start: {:?}",
            t.sched.warnings
        );
        assert!(
            t.sched.duration > t.duration,
            "Auto should fill toward soft FNLT/hard MFO, got sched={} stored={}",
            t.sched.duration,
            t.duration
        );
    }

    /// Zero working window (lunch gap): probe may snap onto the pin; either way keep stored.
    #[test]
    fn zero_working_window_keeps_stored_duration() {
        let mut p = Project::new("LunchCap", crate::dates::ymd(2026, 1, 5).unwrap()); // Monday
        p.settings.rules = ScheduleRules::on();
        let g = p.add_task("Group", None);
        let fill = p.add_task("Fill", None);
        let gate = p.add_task("Gate", None);
        let g_start = p.start;
        let g_end = day(&p, 20);
        let morning_end = crate::dates::ymd_hm(2026, 1, 5, 12, 0).unwrap();
        let afternoon = crate::dates::ymd_hm(2026, 1, 5, 13, 0).unwrap();
        assert_eq!(project_calendar(&p).between(morning_end, afternoon), 0);
        {
            let t = p.task_mut(g).unwrap();
            t.bounds.start_earliest = Some(g_start);
            t.bounds.finish_latest = Some(g_end);
            t.bounds.finish_latest_fixed = true;
        }
        for id in [fill, gate] {
            p.task_mut(id).unwrap().outline_level = 2;
        }
        {
            let t = p.task_mut(fill).unwrap();
            t.duration = 5 * 480;
            t.estimated = false;
            t.duration_mode = DurationMode::Auto;
            t.bounds.start_earliest = Some(morning_end);
        }
        {
            let t = p.task_mut(gate).unwrap();
            t.kind = TaskKind::Milestone;
            t.duration = 0;
            t.constraint = Constraint { kind: ConstraintKind::Mfo, date: Some(afternoon) };
        }
        p.link(fill.into(), gate, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let t = p.task(fill).unwrap();
        assert_eq!(t.sched.duration, 5 * 480, "impossible window keeps stored, never collapses to 0");
        assert!(
            t.sched.warnings.iter().any(|w| w.contains("no room left") && w.contains("keeping")),
            "expect keep-stored note: {:?}",
            t.sched.warnings
        );
    }
}
