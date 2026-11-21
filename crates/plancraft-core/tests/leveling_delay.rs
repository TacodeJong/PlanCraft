#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use plancraft_core::dates::{DateExt, ymd};
use plancraft_core::limits::MAX_MINUTES;
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

const DAY: Minutes = 480;

fn schedule_fixture() -> (Project, TaskId, TaskId, TaskId) {
    let mut p = Project::new("Leveling", ymd(2026, 12, 7).unwrap());
    let _group = p.add_task("Group", None);
    let kickoff = p.add_task("Kickoff", None);
    let prep = p.add_task("Preparation", None);
    let send = p.add_task("Send", None);
    let other = p.add_task("Other delayed", None);
    for id in [kickoff, prep, send, other] {
        p.task_mut(id).unwrap().outline_level = 2;
    }
    {
        let t = p.task_mut(kickoff).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(ymd(2026, 12, 8).unwrap().and_hms_opt(17, 0, 0).unwrap()) };
    }
    {
        let t = p.task_mut(prep).unwrap();
        t.duration = 2 * DAY;
        t.duration_mode = DurationMode::Auto;
        t.leveling_delay = 3 * DAY;
    }
    {
        let t = p.task_mut(send).unwrap();
        t.duration = DAY;
        t.bounds.finish_latest = Some(ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap());
        t.bounds.finish_latest_fixed = true;
    }
    p.task_mut(other).unwrap().leveling_delay = 2 * DAY;
    p.link(kickoff.into(), prep, LinkType::FS, Lag::None).unwrap();
    p.link(prep.into(), send, LinkType::FS, Lag::None).unwrap();
    recalculate(&mut p);
    (p, prep, send, other)
}

#[test]
fn set_leveling_delay_changes_only_the_target_task() {
    let mut p = Project::new("Single", ymd(2026, 12, 7).unwrap());
    let a = p.add_task("A", None);
    let b = p.add_task("B", None);
    let before_a = p.task(a).unwrap().clone();
    let before_b = p.task(b).unwrap().clone();

    p.set_leveling_delay(a, 2 * DAY).unwrap();

    let after_a = p.task(a).unwrap();
    let after_b = p.task(b).unwrap();
    assert_eq!(after_a.leveling_delay, 2 * DAY);
    assert_eq!(after_a.mode, before_a.mode);
    assert_eq!(after_a.duration, before_a.duration);
    assert_eq!(after_a.duration_mode, before_a.duration_mode);
    assert_eq!(after_a.constraint, before_a.constraint);
    assert_eq!(after_a.bounds, before_a.bounds);
    assert_eq!(after_a.manual_start, before_a.manual_start);
    assert_eq!(after_a.manual_finish, before_a.manual_finish);
    assert_eq!(after_a.actual_start, before_a.actual_start);
    assert_eq!(after_a.actual_finish, before_a.actual_finish);
    assert_eq!(after_a.predecessors, before_a.predecessors);
    assert_eq!(after_a.members, before_a.members);
    assert_eq!(after_a.calendar, before_a.calendar);
    assert_eq!(after_a.ignore_resource_calendars, before_a.ignore_resource_calendars);
    assert_eq!(after_b, &before_b, "unselected task must stay unchanged");
}

#[test]
fn set_leveling_delay_rejects_out_of_range_values_atomically() {
    let mut p = Project::new("Reject", ymd(2026, 12, 7).unwrap());
    let t = p.add_task("Task", None);
    let before = p.clone();

    let err = p.set_leveling_delay(t, -1).unwrap_err();
    assert!(err.to_string().contains("between 0 and 5 years"), "{err}");
    assert_eq!(p, before, "negative input must not mutate the project");

    let err = p.set_leveling_delay(t, MAX_MINUTES.saturating_add(1)).unwrap_err();
    assert!(err.to_string().contains("between 0 and 5 years"), "{err}");
    assert_eq!(p, before, "oversized input must not mutate the project");
}

#[test]
fn set_leveling_delay_is_undoable_and_persists_roundtrip() {
    let mut p = Project::new("Undo", ymd(2026, 12, 7).unwrap());
    let t = p.add_task("Task", None);
    let mut doc = Document::new(1, p, None);

    let _ = doc.edit("Set delay", |proj| proj.set_leveling_delay(t, 3 * DAY)).unwrap();
    assert_eq!(doc.project.task(t).unwrap().leveling_delay, 3 * DAY);

    assert!(doc.undo());
    assert_eq!(doc.project.task(t).unwrap().leveling_delay, 0);
    assert!(doc.redo());
    assert_eq!(doc.project.task(t).unwrap().leveling_delay, 3 * DAY);

    let text = serde_json::to_string(&doc.project).unwrap();
    let round: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(round.task(t).unwrap().leveling_delay, 3 * DAY);
}

#[test]
fn clearing_only_one_delay_keeps_other_delays_and_restores_finish_limit() {
    let (mut p, prep, send, other) = schedule_fixture();
    let delayed_warnings = p.task(send).unwrap().sched.warnings.clone();
    assert!(
        delayed_warnings.iter().any(|w| w.to_ascii_lowercase().contains("finish") || w.to_ascii_lowercase().contains("late")),
        "fixture should be warning before clearing delay: {delayed_warnings:?}"
    );

    p.set_leveling_delay(prep, 0).unwrap();
    recalculate(&mut p);

    let cleared_finish = p.task(send).unwrap().sched.finish.unwrap();
    assert!(cleared_finish <= ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap());
    assert!(p.task(send).unwrap().sched.warnings.is_empty(), "send should no longer warn after clearing only prep delay");
    assert_eq!(p.task(other).unwrap().leveling_delay, 2 * DAY, "other task delay must stay unchanged");
    assert_eq!(cleared_finish, ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap());
}
