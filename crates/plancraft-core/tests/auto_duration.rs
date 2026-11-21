#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
//! Explicit per-task Fixed vs Auto duration (opt-in stretch without global rules).

use plancraft_core::dates::prelude::*;
use plancraft_core::dates::ymd;
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

const DAY: Minutes = 480;

fn dt(y: i32, m: u32, d: u32, h: u32) -> DateTime {
    ymd(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
}

fn day(p: &Project, n: i64) -> DateTime {
    plancraft_core::schedule::project_calendar(p).add(p.start, n * p.settings.time.minutes_per_day)
}

/// Synthetic case from DURATION-TASK: group, pinned predecessor, preparation, send FNLT, successor.
fn synthetic_plan() -> (Project, TaskId, TaskId, TaskId, TaskId, TaskId) {
    let mut p = Project::new("Auto duration", ymd(2026, 12, 7).unwrap()); // Monday
    assert!(!p.settings.rules.enabled(), "rules stay off; Auto is per-task");
    let g = p.add_task("Group", None);
    let pred = p.add_task("Predecessor", None);
    let prep = p.add_task("Preparation", None);
    let send = p.add_task("Send", None);
    let succ = p.add_task("Successor", None);
    for id in [pred, prep, send, succ] {
        let t = p.task_mut(id).unwrap();
        t.outline_level = 2;
        // Opt out of the Swift fills_group default so these tests isolate DurationMode::Auto.
        t.fills_group = false;
    }
    {
        let t = p.task_mut(pred).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(dt(2026, 12, 8, 17)) };
    }
    {
        let t = p.task_mut(prep).unwrap();
        t.duration = 5 * DAY;
        t.estimated = false;
        // Unrestricted dates alone must not convert Fixed → Auto.
        assert_eq!(t.duration_mode, DurationMode::Fixed);
        assert!(!t.fills_group);
        assert_eq!(t.start_mode(), DateMode::Unrestricted);
        assert_eq!(t.finish_mode(), DateMode::Unrestricted);
    }
    {
        let t = p.task_mut(send).unwrap();
        t.duration = DAY;
        t.estimated = false;
        t.bounds.finish_latest = Some(dt(2026, 12, 11, 17));
        t.bounds.finish_latest_fixed = true;
    }
    {
        let t = p.task_mut(succ).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(dt(2026, 12, 16, 17)) };
    }
    p.link(pred.into(), prep, LinkType::FS, Lag::None).unwrap();
    p.link(prep.into(), send, LinkType::FS, Lag::None).unwrap();
    p.link(send.into(), succ, LinkType::FS, Lag::None).unwrap();
    (p, g, pred, prep, send, succ)
}

#[test]
fn explicitly_selecting_fixed_stops_legacy_stretch() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    p.settings.rules.stretch = true;
    p.task_mut(prep).unwrap().fills_group = true;
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().sched.duration, 2 * DAY, "legacy calculation is active before switching");
    p.set_duration_mode(prep, DurationMode::Fixed).unwrap();
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().sched.duration, 5 * DAY, "explicit Fixed must use its stored duration");
    assert!(p.settings.rules.stretch, "do not disable other tasks' project-wide rules");
}

#[test]
fn explicitly_selecting_fixed_stops_legacy_percent() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    p.settings.rules.span = true;
    p.task_mut(prep).unwrap().duration_percent = Some(40.0);
    recalculate(&mut p);
    assert_ne!(p.task(prep).unwrap().sched.duration, 5 * DAY, "legacy percent calculation is active before switching");
    p.set_duration_mode(prep, DurationMode::Fixed).unwrap();
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().sched.duration, 5 * DAY, "explicit Fixed must restore the stored duration");
    assert!(p.settings.rules.span, "do not disable project-wide span rules");
}

#[test]
fn fixed_preparation_keeps_five_days_without_global_rules() {
    let (mut p, _g, _pred, prep, send, _succ) = synthetic_plan();
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().sched.duration, 5 * DAY);
    assert_eq!(p.task(prep).unwrap().duration, 5 * DAY);
    assert_eq!(p.task(send).unwrap().sched.duration, DAY);
    assert!(p.settings.rules.is_off());
}

#[test]
fn explicit_auto_preparation_calculates_two_working_days() {
    let (mut p, _g, _pred, prep, send, _succ) = synthetic_plan();
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    assert!(p.settings.rules.is_off(), "Auto must not turn on project-wide stretch/span");
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().duration, 5 * DAY, "stored input stays the fallback");
    assert_eq!(p.task(prep).unwrap().sched.duration, 2 * DAY, "9–10 Dec working window before send on the 11th");
    assert_eq!(p.task(send).unwrap().sched.duration, DAY);
    let send_start = p.task(send).unwrap().sched.start.unwrap();
    assert_eq!(send_start.date(), ymd(2026, 12, 11).unwrap());
}

#[test]
fn explicit_auto_uses_successor_no_later_window_even_when_not_fixed() {
    let (mut p, _g, _pred, prep, send, _succ) = synthetic_plan();
    {
        let t = p.task_mut(send).unwrap();
        t.bounds.finish_latest_fixed = false;
        t.constraint = Constraint::default();
    }
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    recalculate(&mut p);
    assert_eq!(
        p.task(prep).unwrap().sched.duration,
        2 * DAY,
        "Auto duration should still size to successor's explicit finish-latest window"
    );
}

#[test]
fn legacy_stretch_ignores_soft_successor_no_later_cap() {
    let (mut p, _g, _pred, prep, send, _succ) = synthetic_plan();
    p.settings.rules.stretch = true;
    p.task_mut(prep).unwrap().fills_group = true;
    {
        let t = p.task_mut(send).unwrap();
        t.bounds.finish_latest_fixed = false;
        t.constraint = Constraint::default();
    }
    let report = recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().sched.duration, 5 * DAY, "legacy stretch must not be shortened by a soft successor bound");
    assert_eq!(p.task(send).unwrap().sched.start.unwrap().date(), ymd(2026, 12, 16).unwrap());
    assert!(
        report.warnings.iter().any(|w| {
            let lw = w.to_lowercase();
            lw.contains("no later") || lw.contains("no-later") || lw.contains("deadline")
        }),
        "soft no-later overruns should stay visible: {:?}",
        report.warnings
    );
}

#[test]
fn legacy_percent_ignores_soft_successor_no_later_cap() {
    let mut p = Project::new("Legacy percent", ymd(2026, 1, 5).unwrap());
    p.settings.rules.span = true;
    let g = p.add_task("Group", None);
    let prep = p.add_task("Prep", None);
    let send = p.add_task("Send", None);
    let start = p.start;
    let group_end = day(&p, 10);
    let send_soft = day(&p, 3);
    for id in [prep, send] {
        p.task_mut(id).unwrap().outline_level = 2;
    }
    {
        let t = p.task_mut(g).unwrap();
        t.bounds.start_earliest = Some(start);
        t.bounds.finish_latest = Some(group_end);
    }
    {
        let t = p.task_mut(prep).unwrap();
        t.duration = DAY;
        t.estimated = false;
        t.duration_percent = Some(60.0);
    }
    {
        let t = p.task_mut(send).unwrap();
        t.duration = DAY;
        t.estimated = false;
        t.bounds.finish_latest = Some(send_soft);
        t.bounds.finish_latest_fixed = false;
    }
    p.link(prep.into(), send, LinkType::FS, Lag::None).unwrap();
    let report = recalculate(&mut p);
    assert_eq!(
        p.task(prep).unwrap().sched.duration,
        6 * DAY,
        "legacy percent must keep the group share when only a soft successor bound is missed"
    );
    assert!(
        report.warnings.iter().any(|w| {
            let lw = w.to_lowercase();
            lw.contains("no later") || lw.contains("no-later") || lw.contains("deadline")
        }),
        "soft no-later overruns should stay visible: {:?}",
        report.warnings
    );
}

#[test]
fn leveling_delay_on_send_is_not_silently_cleared() {
    let (mut p, _g, _pred, prep, send, _succ) = synthetic_plan();
    p.task_mut(send).unwrap().leveling_delay = 3 * DAY;
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    let report = recalculate(&mut p);
    assert_eq!(p.task(send).unwrap().leveling_delay, 3 * DAY, "never clear delays to make Auto fit");
    assert_eq!(p.task(prep).unwrap().predecessors.len(), 1);
    assert_eq!(p.task(prep).unwrap().sched.duration, 2 * DAY);
    assert_eq!(p.task(send).unwrap().sched.start.unwrap().date(), ymd(2026, 12, 16).unwrap());
    assert!(p.task(send).unwrap().bounds.finish_latest.is_some());
    assert!(
        report.warnings.iter().any(|w| {
            let lw = w.to_lowercase();
            lw.contains("no later") || lw.contains("no-later") || lw.contains("deadline")
        }),
        "the no-later overrun must remain visible after delaying send: {:?}",
        report.warnings
    );
}

#[test]
fn sibling_fills_group_with_finish_bound_stays_own_duration() {
    let (mut p, _g, _pred, prep, send, _succ) = synthetic_plan();
    // fills_group alone fills only while both sides are Unrestricted; send has FNLT so it stays
    // Own. Auto on prep must not require project-wide rules.
    p.task_mut(send).unwrap().fills_group = true;
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    recalculate(&mut p);
    assert!(p.settings.rules.is_off());
    assert_eq!(p.task(send).unwrap().sched.duration, DAY, "finish-bound fill keeps stored duration");
    assert!(p.task(send).unwrap().fills_group);
    assert_eq!(p.task(prep).unwrap().sched.duration, 2 * DAY);
}

#[test]
fn display_uses_calculated_duration_for_auto() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    recalculate(&mut p);
    let t = p.task(prep).unwrap();
    assert_eq!(p.format_task_duration(t), p.format_duration(2 * DAY, false));
    assert_ne!(p.format_task_minutes(t, t.duration, false), p.format_task_duration(t));
}

#[test]
fn json_round_trip_old_file_stays_fixed_and_new_auto_persists() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    let old = serde_json::to_string(&p).unwrap();
    assert!(!old.contains("duration_mode") && !old.contains("durationMode"), "Fixed default is omitted");
    let loaded: Project = serde_json::from_str(&old).unwrap();
    assert_eq!(loaded.task(prep).unwrap().duration_mode, DurationMode::Fixed);

    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    recalculate(&mut p);
    let new = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&new).unwrap();
    assert_eq!(back.task(prep).unwrap().duration_mode, DurationMode::Auto);
    assert_eq!(back.task(prep).unwrap().duration, 5 * DAY);
    recalculate(&mut back.clone());
    // After load, Auto still calculates.
    let mut back = back;
    recalculate(&mut back);
    assert_eq!(back.task(prep).unwrap().sched.duration, 2 * DAY);
}

#[test]
fn undo_redo_restores_duration_mode() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    recalculate(&mut p);
    let mut doc = Document::new(1, p, None);
    doc.edit("Auto duration", |proj| {
        proj.set_duration_mode(prep, DurationMode::Auto).unwrap();
    })
    .unwrap();
    assert_eq!(doc.project.task(prep).unwrap().duration_mode, DurationMode::Auto);
    assert_eq!(doc.project.task(prep).unwrap().sched.duration, 2 * DAY);
    assert!(doc.undo());
    assert_eq!(doc.project.task(prep).unwrap().duration_mode, DurationMode::Fixed);
    assert_eq!(doc.project.task(prep).unwrap().sched.duration, 5 * DAY);
    assert!(doc.redo());
    assert_eq!(doc.project.task(prep).unwrap().duration_mode, DurationMode::Auto);
    assert_eq!(doc.project.task(prep).unwrap().sched.duration, 2 * DAY);
}

#[test]
fn switching_fixed_to_auto_and_back_preserves_stored_input() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().sched.duration, 2 * DAY);
    p.set_duration_mode(prep, DurationMode::Fixed).unwrap();
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().duration, 5 * DAY);
    assert_eq!(p.task(prep).unwrap().sched.duration, 5 * DAY);
}

#[test]
fn set_duration_switches_auto_to_fixed() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    p.set_duration(prep, 3 * DAY, false).unwrap();
    assert_eq!(p.task(prep).unwrap().duration_mode, DurationMode::Fixed);
    assert_eq!(p.task(prep).unwrap().duration, 3 * DAY);
}

#[test]
fn stale_manual_dates_on_auto_scheduled_task_are_ignored() {
    let (mut p, _g, _pred, prep, _send, _succ) = synthetic_plan();
    {
        let t = p.task_mut(prep).unwrap();
        t.mode = ScheduleMode::Auto;
        t.manual_start = Some(dt(2026, 12, 1, 8));
        t.manual_finish = Some(dt(2026, 12, 20, 17));
    }
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    recalculate(&mut p);
    assert_eq!(p.task(prep).unwrap().sched.duration, 2 * DAY);
    let start = p.task(prep).unwrap().sched.start.unwrap();
    assert_eq!(start.date(), ymd(2026, 12, 9).unwrap());
}

#[test]
fn root_task_auto_is_rejected() {
    let mut p = Project::new("Root", ymd(2026, 12, 7).unwrap());
    let a = p.add_task("Alone", None);
    let err = p.set_duration_mode(a, DurationMode::Auto).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("group") || err.to_string().to_lowercase().contains("parent"), "{err}");
    assert_eq!(p.task(a).unwrap().duration_mode, DurationMode::Fixed);
}

#[test]
fn milestone_and_manual_reject_auto() {
    let mut p = Project::new("Guards", ymd(2026, 12, 7).unwrap());
    let g = p.add_task("Group", None);
    let m = p.add_task("Mile", None);
    let leaf = p.add_task("Leaf", None);
    p.task_mut(m).unwrap().outline_level = 2;
    p.task_mut(leaf).unwrap().outline_level = 2;
    let _ = g;
    p.task_mut(m).unwrap().kind = TaskKind::Milestone;
    p.task_mut(m).unwrap().duration = 0;
    assert!(p.set_duration_mode(m, DurationMode::Auto).is_err());
    p.task_mut(leaf).unwrap().mode = ScheduleMode::Manual;
    assert!(p.set_duration_mode(leaf, DurationMode::Auto).is_err());
}

#[test]
fn auto_without_window_keeps_stored_and_reports() {
    let mut p = Project::new("Open", ymd(2026, 1, 5).unwrap());
    let _g = p.add_task("Group", None);
    let a = p.add_task("Only", None);
    {
        let t = p.task_mut(a).unwrap();
        t.outline_level = 2;
        t.duration = 3 * DAY;
        t.estimated = false;
    }
    p.set_duration_mode(a, DurationMode::Auto).unwrap();
    let report = recalculate(&mut p);
    assert_eq!(p.task(a).unwrap().sched.duration, 3 * DAY);
    assert!(
        report.warnings.iter().any(|w| w.to_lowercase().contains("window") || w.to_lowercase().contains("auto")),
        "missing window must be visible: {:?}",
        report.warnings
    );
}

#[test]
fn auto_with_own_start_rule_warns_when_it_falls_back() {
    let mut p = Project::new("Own rule", ymd(2026, 1, 5).unwrap());
    let _g = p.add_task("Group", None);
    let a = p.add_task("Only", None);
    {
        let t = p.task_mut(a).unwrap();
        t.outline_level = 2;
        t.duration = 3 * DAY;
        t.estimated = false;
        t.bounds.start_earliest = Some(dt(2026, 1, 6, 8));
        t.constraint = Constraint { kind: ConstraintKind::Snet, date: Some(dt(2026, 1, 6, 8)) };
    }
    p.set_duration_mode(a, DurationMode::Auto).unwrap();
    let report = recalculate(&mut p);
    assert_eq!(p.task(a).unwrap().sched.duration, 3 * DAY);
    assert!(
        report.warnings.iter().any(|w| w.contains("Auto duration") && w.contains("own start or finish rule")),
        "fallback warning must be visible: {:?}",
        report.warnings
    );
}

#[test]
fn fills_group_and_percent_work_without_global_rules() {
    let mut p = Project::new("Size in group", ymd(2026, 1, 5).unwrap());
    let g = p.add_task("Group", None);
    let a = p.add_task("A", None);
    p.task_mut(a).unwrap().outline_level = 2;
    p.task_mut(a).unwrap().fills_group = true;
    p.task_mut(a).unwrap().duration = DAY;
    p.task_mut(a).unwrap().estimated = false;
    let end = day(&p, 8);
    let start = p.start;
    {
        let t = p.task_mut(g).unwrap();
        t.bounds.start_earliest = Some(start);
        t.bounds.finish_latest = Some(end);
    }
    assert!(!p.settings.rules.enabled());
    recalculate(&mut p);
    let window = plancraft_core::schedule::project_calendar(&p).between(p.start, end);
    assert!((p.task(a).unwrap().sched.duration - window).abs() <= 1, "fills_group fills without rules");

    p.task_mut(a).unwrap().fills_group = false;
    p.task_mut(a).unwrap().duration_percent = Some(25.0);
    recalculate(&mut p);
    assert!((p.task(a).unwrap().sched.duration - window / 4).abs() <= 1, "percent spans without rules");
}
