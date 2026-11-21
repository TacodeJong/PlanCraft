#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use plancraft_core::dates::prelude::*;
use plancraft_core::dates::ymd;
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

const DAY: Minutes = 480;

fn dt(y: i32, m: u32, d: u32, h: u32) -> DateTime {
    ymd(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
}

fn setup_linked_task(kind: ConstraintKind, date: DateTime) -> (Project, TaskId) {
    let mut p = Project::new("Placement", ymd(2026, 12, 7).unwrap());
    let _g = p.add_task("Group", None);
    let pred = p.add_task("Pred", None);
    let t = p.add_task("Task", None);
    for id in [pred, t] {
        p.task_mut(id).unwrap().outline_level = 2;
    }
    {
        let row = p.task_mut(pred).unwrap();
        row.kind = TaskKind::Milestone;
        row.duration = 0;
        row.estimated = false;
        row.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(dt(2026, 12, 15, 17)) };
    }
    {
        let row = p.task_mut(t).unwrap();
        row.duration = 2 * DAY;
        row.estimated = false;
        row.constraint = Constraint { kind, date: Some(date) };
    }
    p.link(pred.into(), t, LinkType::FS, Lag::None).unwrap();
    (p, t)
}

fn signature(p: &Project, id: TaskId) -> (Option<DateTime>, Option<DateTime>) {
    let row = p.task(id).unwrap();
    (row.sched.start, row.sched.finish)
}

#[test]
fn set_placement_preserves_effective_hard_fnlt_policy() {
    let pin = dt(2026, 12, 11, 17);
    let (mut p, t) = setup_linked_task(ConstraintKind::Fnlt, pin);
    recalculate(&mut p);
    let before = signature(&p, t);

    p.set_placement(t, ConstraintKind::Asap).unwrap();
    let row = p.task(t).unwrap();
    assert_eq!(row.constraint.kind, ConstraintKind::Asap);
    assert!(row.constraint.date.is_none());
    assert_eq!(row.bounds.finish_latest, Some(pin));
    assert!(row.bounds.finish_latest_fixed, "legacy FNLT must migrate as a fixed finish bound");

    recalculate(&mut p);
    assert_eq!(signature(&p, t), before, "migrating placement must preserve the scheduled result");
}

#[test]
fn set_placement_preserves_effective_hard_snlt_policy() {
    let pin = dt(2026, 12, 11, 8);
    let (mut p, t) = setup_linked_task(ConstraintKind::Snlt, pin);
    recalculate(&mut p);
    let before = signature(&p, t);

    p.set_placement(t, ConstraintKind::Asap).unwrap();
    let row = p.task(t).unwrap();
    assert_eq!(row.bounds.start_latest, Some(pin));
    assert!(row.bounds.start_latest_fixed, "legacy SNLT must migrate as a fixed start bound");

    recalculate(&mut p);
    assert_eq!(signature(&p, t), before, "migrating placement must preserve the scheduled result");
}

#[test]
fn set_placement_preserves_effective_hard_mso_policy() {
    let pin = dt(2026, 12, 11, 8);
    let (mut p, t) = setup_linked_task(ConstraintKind::Mso, pin);
    recalculate(&mut p);
    let before = signature(&p, t);

    p.set_placement(t, ConstraintKind::Asap).unwrap();
    let row = p.task(t).unwrap();
    assert_eq!(row.bounds.start_earliest, Some(pin));
    assert_eq!(row.bounds.start_latest, Some(pin));
    assert!(row.bounds.start_latest_fixed, "legacy MSO must keep its exact-start hard policy");

    recalculate(&mut p);
    assert_eq!(signature(&p, t), before, "migrating placement must preserve the scheduled result");
}

#[test]
fn set_placement_preserves_effective_hard_mfo_policy() {
    let pin = dt(2026, 12, 11, 17);
    let (mut p, t) = setup_linked_task(ConstraintKind::Mfo, pin);
    recalculate(&mut p);
    let before = signature(&p, t);

    p.set_placement(t, ConstraintKind::Asap).unwrap();
    let row = p.task(t).unwrap();
    assert_eq!(row.bounds.finish_earliest, Some(pin));
    assert_eq!(row.bounds.finish_latest, Some(pin));
    assert!(row.bounds.finish_latest_fixed, "legacy MFO must keep its exact-finish hard policy");

    recalculate(&mut p);
    assert_eq!(signature(&p, t), before, "migrating placement must preserve the scheduled result");
}

#[test]
fn set_placement_keeps_modern_soft_windows_soft() {
    let mut p = Project::new("Placement", ymd(2026, 12, 7).unwrap());
    let _g = p.add_task("Group", None);
    let t = p.add_task("Soft", None);
    {
        let row = p.task_mut(t).unwrap();
        row.outline_level = 2;
        row.bounds.finish_latest = Some(dt(2026, 12, 11, 17));
        row.bounds.finish_latest_fixed = false;
    }
    p.set_placement(t, ConstraintKind::Alap).unwrap();
    let row = p.task(t).unwrap();
    assert_eq!(row.constraint.kind, ConstraintKind::Alap);
    assert_eq!(row.bounds.finish_latest, Some(dt(2026, 12, 11, 17)));
    assert!(!row.bounds.finish_latest_fixed, "modern soft bounds must stay warning-only after placement change");
}

#[test]
fn set_placement_rejects_manual_tasks_without_mutation() {
    let mut p = Project::new("Placement", ymd(2026, 12, 7).unwrap());
    let _g = p.add_task("Group", None);
    let pred = p.add_task("Pred", None);
    let t = p.add_task("Manual", None);
    for id in [pred, t] {
        p.task_mut(id).unwrap().outline_level = 2;
    }
    {
        let row = p.task_mut(pred).unwrap();
        row.kind = TaskKind::Milestone;
        row.duration = 0;
        row.estimated = false;
        row.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(dt(2026, 12, 9, 8)) };
    }
    {
        let row = p.task_mut(t).unwrap();
        row.mode = ScheduleMode::Manual;
        row.manual_start = Some(dt(2026, 12, 10, 8));
        row.manual_finish = Some(dt(2026, 12, 12, 17));
        row.bounds.start_earliest = Some(dt(2026, 12, 8, 8));
        row.bounds.start_latest = Some(dt(2026, 12, 9, 8));
        row.bounds.start_latest_fixed = true;
        row.bounds.finish_earliest = Some(dt(2026, 12, 13, 17));
        row.bounds.finish_latest = Some(dt(2026, 12, 14, 17));
        row.bounds.finish_latest_fixed = false;
        row.constraint = Constraint { kind: ConstraintKind::Fnlt, date: Some(dt(2026, 12, 15, 17)) };
        row.duration_mode = DurationMode::Auto;
        row.leveling_delay = 3 * DAY;
    }
    p.link(pred.into(), t, LinkType::FS, Lag::None).unwrap();
    let before = p.clone();

    for kind in [ConstraintKind::Asap, ConstraintKind::Alap] {
        let err = p.set_placement(t, kind).unwrap_err();
        assert_eq!(
            err.to_string(),
            "placement cannot be changed on a manually scheduled task",
            "manual tasks should be rejected before any placement migration",
        );
        assert_eq!(p, before);
    }
}

#[test]
fn set_placement_rejects_unrepresentable_legacy_soft_overlap_without_mutation() {
    let mut p = Project::new("Placement", ymd(2026, 12, 7).unwrap());
    let _g = p.add_task("Group", None);
    let t = p.add_task("Task", None);
    {
        let row = p.task_mut(t).unwrap();
        row.outline_level = 2;
        row.constraint = Constraint { kind: ConstraintKind::Fnlt, date: Some(dt(2026, 12, 11, 17)) };
        row.bounds.finish_latest = Some(dt(2026, 12, 9, 17));
        row.bounds.finish_latest_fixed = false;
    }
    let before = p.task(t).unwrap().clone();
    let err = p.set_placement(t, ConstraintKind::Asap).unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(msg.contains("cannot") || msg.contains("overlap") || msg.contains("represent"), "{err}");
    let after = p.task(t).unwrap();
    assert_eq!(after.constraint, before.constraint);
    assert_eq!(after.bounds, before.bounds);
}

#[test]
fn set_placement_is_undoable_and_persists() {
    let mut p = Project::new("Placement", ymd(2026, 12, 7).unwrap());
    let _g = p.add_task("Group", None);
    let t = p.add_task("Prep", None);
    p.task_mut(t).unwrap().outline_level = 2;

    let mut doc = Document::new(1, p, None);
    let _ = doc.edit("Set placement", |proj| proj.set_placement(t, ConstraintKind::Alap)).unwrap();
    assert_eq!(doc.project.task(t).unwrap().constraint.kind, ConstraintKind::Alap);
    assert!(doc.undo());
    assert_eq!(doc.project.task(t).unwrap().constraint.kind, ConstraintKind::Asap);
    assert!(doc.redo());
    assert_eq!(doc.project.task(t).unwrap().constraint.kind, ConstraintKind::Alap);

    let text = serde_json::to_string(&doc.project).unwrap();
    let round: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(round.task(t).unwrap().constraint.kind, ConstraintKind::Alap);
}
