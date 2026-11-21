#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use plancraft_core::dates::prelude::*;
use plancraft_core::dates::ymd;
use plancraft_core::prelude::*;

const DAY: Minutes = 480;

fn day(p: &Project, n: i64) -> DateTime {
    plancraft_core::schedule::project_calendar(p).add(p.start, n * p.settings.time.minutes_per_day)
}

struct Fixture {
    p: Project,
    auto: TaskId,
    auto_gate: TaskId,
    stretch: TaskId,
    stretch_gate: TaskId,
    percent: TaskId,
    percent_gate: TaskId,
    collapse: TaskId,
    collapse_gate: TaskId,
    fixed: TaskId,
}

fn fixture() -> Fixture {
    let mut p = Project::new("Warning attribution", ymd(2026, 1, 5).unwrap());
    p.settings.rules = ScheduleRules::on();

    let auto_group = p.add_task("Auto group", None);
    let auto = p.add_task("Auto row", None);
    let auto_gate = p.add_task("Auto gate", None);

    let stretch_group = p.add_task("Stretch group", None);
    let stretch = p.add_task("Stretch row", None);
    let stretch_gate = p.add_task("Stretch gate", None);

    let percent_group = p.add_task("Percent group", None);
    let percent = p.add_task("Percent row", None);
    let percent_gate = p.add_task("Percent gate", None);

    let collapse_group = p.add_task("Collapse group", None);
    let collapse = p.add_task("Collapse row", None);
    let collapse_gate = p.add_task("Collapse gate", None);

    let fixed = p.add_task("Fixed row", None);

    for id in [auto, auto_gate, stretch, stretch_gate, percent, percent_gate, collapse, collapse_gate] {
        p.task_mut(id).unwrap().outline_level = 2;
    }

    let start = p.start;
    let end = day(&p, 8);
    let auto_gate_date = day(&p, 2);
    let stretch_gate_date = day(&p, 3);
    let percent_gate_date = day(&p, 3);
    for group in [auto_group, stretch_group, percent_group, collapse_group] {
        let t = p.task_mut(group).unwrap();
        t.bounds.start_earliest = Some(start);
        t.bounds.finish_latest = Some(end);
    }

    {
        let t = p.task_mut(auto).unwrap();
        t.duration = 5 * DAY;
        t.duration_mode = DurationMode::Auto;
        t.estimated = false;
    }
    {
        let t = p.task_mut(auto_gate).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.bounds.finish_latest = Some(auto_gate_date);
        t.bounds.finish_latest_fixed = false;
    }
    p.link(auto.into(), auto_gate, LinkType::FS, Lag::None).unwrap();

    {
        let t = p.task_mut(stretch).unwrap();
        t.duration = 5 * DAY;
        t.estimated = false;
        t.fills_group = true;
    }
    {
        let t = p.task_mut(stretch_gate).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(stretch_gate_date) };
    }
    p.link(stretch.into(), stretch_gate, LinkType::FS, Lag::None).unwrap();

    {
        let t = p.task_mut(percent).unwrap();
        t.duration = 5 * DAY;
        t.estimated = false;
        t.duration_percent = Some(50.0);
    }
    {
        let t = p.task_mut(percent_gate).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(percent_gate_date) };
    }
    p.link(percent.into(), percent_gate, LinkType::FS, Lag::None).unwrap();

    {
        let t = p.task_mut(collapse).unwrap();
        t.duration = 5 * DAY;
        t.estimated = false;
        t.fills_group = true;
    }
    {
        let t = p.task_mut(collapse_gate).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(start) };
    }
    p.link(collapse.into(), collapse_gate, LinkType::FS, Lag::None).unwrap();

    {
        let t = p.task_mut(fixed).unwrap();
        t.duration = 2 * DAY;
        t.estimated = false;
        t.leveling_delay = 240;
    }

    Fixture { p, auto, auto_gate, stretch, stretch_gate, percent, percent_gate, collapse, collapse_gate, fixed }
}

fn warning_for(p: &Project, id: TaskId) -> &str {
    let task = p.task(id).unwrap();
    task.sched.warnings.first().map(String::as_str).unwrap_or("")
}

#[test]
fn capped_rows_report_their_own_warning_location() {
    let mut fx = fixture();
    let report = plancraft_core::schedule::recalculate(&mut fx.p);

    let auto_idx = fx.p.task_index(fx.auto).unwrap();
    let stretch_idx = fx.p.task_index(fx.stretch).unwrap();
    let percent_idx = fx.p.task_index(fx.percent).unwrap();
    let collapse_idx = fx.p.task_index(fx.collapse).unwrap();
    let auto_gate_cap = fx.p.task(fx.auto_gate).unwrap().sched.start.unwrap();
    let stretch_gate_cap = fx.p.task(fx.stretch_gate).unwrap().sched.start.unwrap();
    let percent_gate_cap = fx.p.task(fx.percent_gate).unwrap().sched.start.unwrap();
    let collapse_gate_cap = fx.p.task(fx.collapse_gate).unwrap().sched.start.unwrap();

    let auto_w = warning_for(&fx.p, fx.auto);
    assert!(fx.p.task(fx.auto).unwrap().sched.warnings.len() == 1);
    assert!(auto_w.contains(&fx.p.wbs(auto_idx)));
    assert!(auto_w.contains("Auto"));
    assert!(auto_w.contains(&fx.p.format_duration(8 * DAY, false)));
    assert!(auto_w.contains(&fx.p.format_duration(2 * DAY, false)));
    assert!(auto_w.contains(&auto_gate_cap.format("%Y-%m-%d %H:%M")));

    let stretch_w = warning_for(&fx.p, fx.stretch);
    assert!(fx.p.task(fx.stretch).unwrap().sched.warnings.len() == 1);
    assert!(stretch_w.contains(&fx.p.wbs(stretch_idx)));
    assert!(stretch_w.contains("legacy stretch"));
    assert!(stretch_w.contains(&fx.p.format_duration(8 * DAY, false)));
    assert!(stretch_w.contains(&fx.p.format_duration(3 * DAY, false)));
    assert!(stretch_w.contains(&stretch_gate_cap.format("%Y-%m-%d %H:%M")));

    let percent_w = warning_for(&fx.p, fx.percent);
    assert!(fx.p.task(fx.percent).unwrap().sched.warnings.len() == 1);
    assert!(percent_w.contains(&fx.p.wbs(percent_idx)));
    assert!(percent_w.contains("percent"));
    assert!(percent_w.contains(&fx.p.format_duration(4 * DAY, false)));
    assert!(percent_w.contains(&fx.p.format_duration(3 * DAY, false)));
    assert!(percent_w.contains(&percent_gate_cap.format("%Y-%m-%d %H:%M")));

    let collapse_notes: Vec<String> = fx.p.task(fx.collapse).unwrap().sched.warnings.clone();
    assert!(
        collapse_notes.iter().any(|w| w.contains(&fx.p.wbs(collapse_idx)) && w.contains("no room left")),
        "collapse share warning: {collapse_notes:?}"
    );
    assert!(
        collapse_notes.iter().any(|w| w.contains(&collapse_gate_cap.format("%Y-%m-%d %H:%M"))),
        "collapse cap date: {collapse_notes:?}"
    );
    assert_eq!(
        fx.p.task(fx.collapse).unwrap().sched.duration,
        5 * DAY,
        "impossible window keeps stored duration, not a zero bar"
    );

    assert!(fx.p.task(fx.fixed).unwrap().sched.warnings.is_empty());
    assert!(
        !report.warnings.iter().any(|w| (w.contains("Auto row:") || w.contains("Stretch row:") || w.contains("Percent row:")) && w.contains("fixed date")),
        "soft limits should not be mislabeled: {report:?}"
    );
    assert!(!report.warnings.iter().any(|w| w.contains("was shortened so a successor's fixed date")));
    assert!(!report.warnings.iter().any(|w| w.contains("has no room left for its unrestricted activities")));

    assert!(report.warnings.iter().any(|w| w.starts_with("Auto row: ") && w.contains(&fx.p.wbs(auto_idx))));
    assert!(report.warnings.iter().any(|w| w.starts_with("Stretch row: ") && w.contains(&fx.p.wbs(stretch_idx))));
    assert!(report.warnings.iter().any(|w| w.starts_with("Percent row: ") && w.contains(&fx.p.wbs(percent_idx))));
    assert!(report.warnings.iter().any(|w| w.starts_with("Collapse row: ") && w.contains(&fx.p.wbs(collapse_idx))));
}

#[test]
fn capped_summary_rows_keep_their_own_warning_location() {
    let mut p = Project::new("Summary warning", ymd(2026, 1, 5).unwrap());
    p.settings.rules = ScheduleRules::on();

    let outer = p.add_task("Outer group", None);
    let inner = p.add_task("Inner summary", None);
    let child = p.add_task("Inner child", None);
    let gate = p.add_task("Gate", None);

    p.task_mut(inner).unwrap().outline_level = 2;
    p.task_mut(child).unwrap().outline_level = 3;
    p.task_mut(gate).unwrap().outline_level = 2;

    let start = p.start;
    let end = day(&p, 8);
    let cap = day(&p, 2);
    {
        let t = p.task_mut(outer).unwrap();
        t.bounds.start_earliest = Some(start);
        t.bounds.finish_latest = Some(end);
    }
    {
        let t = p.task_mut(inner).unwrap();
        t.duration = 5 * DAY;
        t.duration_mode = DurationMode::Auto;
        t.estimated = false;
    }
    {
        let t = p.task_mut(child).unwrap();
        t.duration = DAY;
        t.estimated = false;
    }
    {
        let t = p.task_mut(gate).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.bounds.finish_latest = Some(cap);
        t.bounds.finish_latest_fixed = false;
    }
    p.link(inner.into(), gate, LinkType::FS, Lag::None).unwrap();

    let first_report = plancraft_core::schedule::recalculate(&mut p);
    let report = plancraft_core::schedule::recalculate(&mut p);

    let inner_idx = p.task_index(inner).unwrap();
    let warning = warning_for(&p, inner);
    assert_eq!(p.task(inner).unwrap().sched.warnings.len(), 1);
    assert!(warning.contains(&p.wbs(inner_idx)));
    assert!(warning.contains("Auto"));
    assert!(warning.contains("successor cap is"));
    assert_eq!(first_report.warnings.iter().filter(|w| w.starts_with("Inner summary: ")).count(), 1);
    assert_eq!(report.warnings.iter().filter(|w| w.starts_with("Inner summary: ")).count(), 1);
}

#[test]
fn stored_inputs_survive_serialization_unchanged() {
    let mut fx = fixture();
    plancraft_core::schedule::recalculate(&mut fx.p);

    let text = serde_json::to_string(&fx.p).unwrap();
    let round: Project = serde_json::from_str(&text).unwrap();

    let auto = round.task(fx.auto).unwrap();
    assert_eq!(auto.duration_mode, DurationMode::Auto);
    assert_eq!(auto.duration, 5 * DAY);
    assert!(!auto.estimated);
    assert!(auto.sched.duration < auto.duration);

    let fixed = round.task(fx.fixed).unwrap();
    assert_eq!(fixed.duration, 2 * DAY);
    assert_eq!(fixed.leveling_delay, 240);
    assert!(!fixed.estimated);
}
