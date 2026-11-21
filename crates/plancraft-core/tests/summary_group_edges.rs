#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use plancraft_core::dates::prelude::*;
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

const DAY: Minutes = 480;

fn d(y: i32, m: u32, day: u32) -> Date {
    plancraft_core::dates::ymd(y, m, day).unwrap()
}

fn dt(y: i32, m: u32, day: u32, h: u32) -> DateTime {
    d(y, m, day).and_hms_opt(h, 0, 0).unwrap()
}

fn project() -> Project {
    Project::new("Summary edges", d(2026, 10, 19))
}

fn start(p: &Project, id: TaskId) -> DateTime {
    p.task(id).unwrap().sched.start.unwrap()
}

fn finish(p: &Project, id: TaskId) -> DateTime {
    p.task(id).unwrap().sched.finish.unwrap()
}

fn top_task(p: &mut Project, name: &str, days: i64) -> TaskId {
    let id = p.add_task(name, None);
    p.set_duration(id, days * DAY, false).unwrap();
    while p.task(id).unwrap().outline_level > 1 {
        p.outdent(id).unwrap();
    }
    id
}

fn set_on(p: &mut Project, id: TaskId, finish: bool, at: DateTime) {
    p.set_date_mode(id, finish, DateMode::On).unwrap();
    p.set_date_window(id, finish, Some(at), Some(at)).unwrap();
}

fn has_warning(p: &Project, id: TaskId, needle: &str) -> bool {
    p.task(id).unwrap().sched.warnings.iter().any(|w| w.contains(needle))
}

#[test]
fn summary_fixed_start_and_dynamic_finish_keeps_its_start_anchor() {
    let mut p = project();
    let s = top_task(&mut p, "Group", 1);
    let a = top_task(&mut p, "A", 2);
    let b = top_task(&mut p, "B", 3);
    p.indent(a).unwrap();
    p.indent(b).unwrap();
    p.link(a.into(), b, LinkType::FS, Lag::None).unwrap();
    p.set_date_mode(s, false, DateMode::On).unwrap();
    p.set_date_mode(s, true, DateMode::Unrestricted).unwrap();
    p.set_date_window(s, false, Some(dt(2026, 10, 22, 8)), Some(dt(2026, 10, 22, 8))).unwrap();
    recalculate(&mut p);

    assert_eq!(start(&p, s), dt(2026, 10, 22, 8), "the summary keeps its fixed start");
    assert_eq!(finish(&p, s), finish(&p, b), "the summary finish still rolls up from children");
}

#[test]
fn summary_dynamic_start_and_fixed_finish_keeps_its_finish_anchor() {
    let mut p = project();
    let s = top_task(&mut p, "Group", 1);
    let a = top_task(&mut p, "A", 2);
    p.indent(a).unwrap();
    p.set_date_mode(s, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(s, true, DateMode::On).unwrap();
    p.set_date_window(s, true, Some(dt(2026, 10, 30, 17)), Some(dt(2026, 10, 30, 17))).unwrap();
    recalculate(&mut p);

    assert_eq!(start(&p, s), start(&p, a), "the summary start still rolls up from children");
    assert_eq!(finish(&p, s), dt(2026, 10, 30, 17), "the summary keeps its fixed finish");
}

#[test]
fn summary_start_no_later_does_not_become_a_descendant_start_floor() {
    let mut p = project();
    let s = top_task(&mut p, "Group", 1);
    let a = top_task(&mut p, "A", 1);
    p.indent(a).unwrap();

    p.set_date_mode(s, false, DateMode::NoLater).unwrap();
    p.set_date_window(s, false, None, Some(dt(2026, 10, 22, 8))).unwrap();
    recalculate(&mut p);

    assert_eq!(start(&p, a), dt(2026, 10, 19, 8), "a summary Start No Later Than must not push children later");
    assert_eq!(start(&p, s), start(&p, a), "the summary start still rolls up from children");
}

#[test]
fn summary_soft_finish_no_later_keeps_link_driven_child_dates() {
    let mut p = project();
    let driver = top_task(&mut p, "Driver", 3);
    let s = top_task(&mut p, "Group", 1);
    let a = top_task(&mut p, "A", 2);
    p.indent(a).unwrap();
    p.link(driver.into(), a, LinkType::FS, Lag::None).unwrap();

    p.set_date_mode(s, true, DateMode::NoLater).unwrap();
    p.set_date_window(s, true, None, Some(dt(2026, 10, 22, 17))).unwrap();
    recalculate(&mut p);

    assert_eq!(finish(&p, driver), dt(2026, 10, 21, 17));
    assert_eq!(start(&p, a), dt(2026, 10, 22, 8), "soft summary finish limits late dates, not early link logic");
    assert_eq!(finish(&p, a), dt(2026, 10, 23, 17));
}

#[test]
fn summary_between_start_preserves_a_real_window_instead_of_flattening_to_one_anchor() {
    let mut p = project();
    let s = top_task(&mut p, "Group", 1);
    let a = top_task(&mut p, "A", 1);
    p.indent(a).unwrap();
    p.task_mut(a).unwrap().constraint = Constraint { kind: ConstraintKind::Snet, date: Some(dt(2026, 10, 22, 8)) };

    p.set_date_mode(s, false, DateMode::Between).unwrap();
    p.set_date_window(s, false, Some(dt(2026, 10, 20, 8)), Some(dt(2026, 10, 24, 8))).unwrap();
    recalculate(&mut p);

    assert_eq!(start(&p, a), dt(2026, 10, 22, 8));
    assert_eq!(start(&p, s), start(&p, a), "summary Between start should remain dynamic inside its range");
    let t = p.task(s).unwrap();
    assert_eq!(t.start_window(), (Some(dt(2026, 10, 20, 8)), Some(dt(2026, 10, 24, 8))));
}

#[test]
fn summary_edges_cover_all_fixed_dynamic_pairs_and_child_growth_shrink() {
    struct Case {
        name: &'static str,
        start_mode: DateMode,
        finish_mode: DateMode,
    }
    let cases = [
        Case { name: "dynamic/dynamic", start_mode: DateMode::Unrestricted, finish_mode: DateMode::Unrestricted },
        Case { name: "fixed/dynamic", start_mode: DateMode::On, finish_mode: DateMode::Unrestricted },
        Case { name: "dynamic/fixed", start_mode: DateMode::Unrestricted, finish_mode: DateMode::On },
        Case { name: "fixed/fixed", start_mode: DateMode::On, finish_mode: DateMode::On },
    ];
    for case in cases {
        let mut p = project();
        let s = top_task(&mut p, "Group", 1);
        let a = top_task(&mut p, "A", 2);
        p.indent(a).unwrap();
        p.set_date_mode(s, false, case.start_mode).unwrap();
        p.set_date_mode(s, true, case.finish_mode).unwrap();
        if case.start_mode == DateMode::On {
            p.set_date_window(s, false, Some(dt(2026, 10, 16, 8)), Some(dt(2026, 10, 16, 8))).unwrap();
        }
        if case.finish_mode == DateMode::On {
            p.set_date_window(s, true, Some(dt(2026, 10, 30, 17)), Some(dt(2026, 10, 30, 17))).unwrap();
        }

        recalculate(&mut p);
        let child_start = start(&p, a);
        let child_finish_2d = finish(&p, a);
        if case.start_mode == DateMode::Unrestricted {
            assert_eq!(start(&p, s), child_start, "{} base start follows child", case.name);
        } else {
            assert_eq!(start(&p, s), dt(2026, 10, 16, 8), "{} base start keeps anchor", case.name);
        }
        if case.finish_mode == DateMode::Unrestricted {
            assert_eq!(finish(&p, s), child_finish_2d, "{} base finish follows child", case.name);
        } else {
            assert_eq!(finish(&p, s), dt(2026, 10, 30, 17), "{} base finish keeps anchor", case.name);
        }

        p.set_duration(a, 4 * DAY, false).unwrap();
        recalculate(&mut p);
        let child_finish_4d = finish(&p, a);
        if case.finish_mode == DateMode::Unrestricted {
            assert_eq!(finish(&p, s), child_finish_4d, "{} grows with child", case.name);
        } else {
            assert_eq!(finish(&p, s), dt(2026, 10, 30, 17), "{} fixed finish does not grow", case.name);
        }

        p.set_duration(a, DAY, false).unwrap();
        recalculate(&mut p);
        let child_finish_1d = finish(&p, a);
        if case.finish_mode == DateMode::Unrestricted {
            assert_eq!(finish(&p, s), child_finish_1d, "{} shrinks with child", case.name);
        } else {
            assert_eq!(finish(&p, s), dt(2026, 10, 30, 17), "{} fixed finish does not shrink", case.name);
        }
    }
}

#[test]
fn nested_summary_boundaries_roll_up_dynamic_edges_but_keep_fixed_edges() {
    let mut p = project();
    let outer = top_task(&mut p, "Outer", 1);
    let middle = top_task(&mut p, "Middle", 1);
    let inner = top_task(&mut p, "Inner", 1);
    let driver = top_task(&mut p, "Driver", 4);
    let delayed = top_task(&mut p, "Delayed", 1);
    let fixed = top_task(&mut p, "Fixed", 2);
    p.indent(middle).unwrap();
    p.indent(inner).unwrap();
    p.indent(inner).unwrap();
    p.indent(driver).unwrap();
    p.indent(driver).unwrap();
    p.indent(driver).unwrap();
    p.indent(delayed).unwrap();
    p.indent(delayed).unwrap();
    p.indent(delayed).unwrap();
    p.indent(fixed).unwrap();
    p.indent(fixed).unwrap();
    p.indent(fixed).unwrap();
    p.link(driver.into(), delayed, LinkType::FS, Lag::None).unwrap();
    p.set_leveling_delay(delayed, 2 * DAY).unwrap();
    set_on(&mut p, fixed, false, dt(2026, 10, 27, 8));

    set_on(&mut p, outer, true, dt(2026, 10, 24, 17));

    p.set_date_mode(middle, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(middle, true, DateMode::Unrestricted).unwrap();

    set_on(&mut p, inner, false, dt(2026, 10, 21, 8));
    set_on(&mut p, inner, true, dt(2026, 10, 28, 17));

    recalculate(&mut p);

    let (outer_i, middle_i, inner_i, delayed_i, fixed_i) = (
        p.task_index(outer).unwrap(),
        p.task_index(middle).unwrap(),
        p.task_index(inner).unwrap(),
        p.task_index(delayed).unwrap(),
        p.task_index(fixed).unwrap(),
    );
    assert_eq!(p.task(outer).unwrap().outline_level, 1);
    assert_eq!(p.task(middle).unwrap().outline_level, 2);
    assert_eq!(p.task(inner).unwrap().outline_level, 3);
    assert_eq!(p.task(delayed).unwrap().outline_level, 4);
    assert_eq!(p.parent_index(outer_i), None);
    assert_eq!(p.parent_index(middle_i), Some(outer_i));
    assert_eq!(p.parent_index(inner_i), Some(middle_i));
    assert_eq!(p.parent_index(delayed_i), Some(inner_i));
    assert_eq!(p.wbs(outer_i), "1");
    assert_eq!(p.wbs(middle_i), "1.1");
    assert_eq!(p.wbs(inner_i), "1.1.1");
    assert_eq!(p.wbs(delayed_i), "1.1.1.2");
    assert_eq!(p.wbs(fixed_i), "1.1.1.3");

    assert_eq!(finish(&p, outer), dt(2026, 10, 24, 17));
    assert_eq!(start(&p, middle), start(&p, inner));
    assert_eq!(finish(&p, middle), finish(&p, inner));
    assert_eq!(start(&p, inner), dt(2026, 10, 21, 8));
    assert_eq!(finish(&p, inner), dt(2026, 10, 28, 17));

    assert_eq!(p.task(delayed).unwrap().leveling_delay, 2 * DAY, "delay input is preserved");
    assert_eq!(p.task(fixed).unwrap().duration, 2 * DAY, "fixed task duration is preserved");
    assert!(
        has_warning(&p, delayed, "ancestor summary") && has_warning(&p, delayed, "Outer") && has_warning(&p, delayed, "fixed finish date"),
        "delayed descendant overrun must warn with ancestor attribution: {:?}",
        p.task(delayed).unwrap().sched.warnings
    );
    assert!(
        has_warning(&p, fixed, "ancestor summary") && has_warning(&p, fixed, "Outer") && has_warning(&p, fixed, "fixed finish date"),
        "fixed inner edge overrun must warn with ancestor attribution: {:?}",
        p.task(fixed).unwrap().sched.warnings
    );
}

#[test]
fn summary_windows_and_placement_survive_document_undo_redo_and_roundtrip() {
    let mut p = project();
    let s = top_task(&mut p, "Group", 1);
    let a = top_task(&mut p, "A", 2);
    p.indent(a).unwrap();
    p.set_placement(s, ConstraintKind::Alap).unwrap();
    recalculate(&mut p);
    let mut doc = Document::new(1, p, None);

    let _ = doc
        .edit("Summary start no earlier", |proj| {
            proj.set_date_mode(s, false, DateMode::NoEarlier)?;
            proj.set_date_window(s, false, Some(dt(2026, 10, 20, 8)), None)
        })
        .unwrap();
    let _ = doc
        .edit("Summary finish between", |proj| {
            proj.set_date_mode(s, true, DateMode::Between)?;
            proj.set_date_window(s, true, Some(dt(2026, 10, 23, 17)), Some(dt(2026, 10, 28, 17)))
        })
        .unwrap();

    {
        let t = doc.project.task(s).unwrap();
        assert_eq!(t.constraint.kind, ConstraintKind::Alap, "summary edge edits must not erase placement policy");
        assert_eq!(t.start_window(), (Some(dt(2026, 10, 20, 8)), None));
        assert_eq!(t.finish_window(), (Some(dt(2026, 10, 23, 17)), Some(dt(2026, 10, 28, 17))));
    }

    assert!(doc.undo());
    {
        let t = doc.project.task(s).unwrap();
        assert_eq!(t.finish_mode(), DateMode::Unrestricted);
    }
    assert!(doc.undo());
    {
        let t = doc.project.task(s).unwrap();
        assert_eq!(t.start_mode(), DateMode::Unrestricted);
        assert_eq!(t.constraint.kind, ConstraintKind::Alap);
    }
    assert!(doc.redo());
    assert!(doc.redo());

    let text = serde_json::to_string(&doc.project).unwrap();
    let round: Project = serde_json::from_str(&text).unwrap();
    let t = round.task(s).unwrap();
    assert_eq!(t.constraint.kind, ConstraintKind::Alap);
    assert_eq!(t.start_mode(), DateMode::NoEarlier);
    assert_eq!(t.finish_mode(), DateMode::Between);
    assert_eq!(t.start_window(), (Some(dt(2026, 10, 20, 8)), None));
    assert_eq!(t.finish_window(), (Some(dt(2026, 10, 23, 17)), Some(dt(2026, 10, 28, 17))));
}

#[test]
fn parent_check_alap_late_dates_use_the_ancestor_window_without_false_conflicts() {
    let mut p = project();
    let group = top_task(&mut p, "Bounded", 1);
    let child = top_task(&mut p, "ALAP child", 2);
    p.indent(child).unwrap();
    let _outside = top_task(&mut p, "Outside long task", 20);
    set_on(&mut p, group, true, dt(2026, 10, 23, 17));
    p.set_placement(child, ConstraintKind::Alap).unwrap();
    recalculate(&mut p);
    let t = p.task(child).unwrap();
    assert!(
        t.sched.late_finish.unwrap() <= dt(2026, 10, 23, 17),
        "late dates must use the ancestor window, not the unrelated project finish"
    );
    assert_eq!(finish(&p, child), dt(2026, 10, 23, 17));
    assert!(!has_warning(&p, child, "ancestor summary"), "feasible ALAP placement must not report a false boundary conflict");
}

#[test]
fn parent_check_fixed_inner_summary_overrun_warns_even_when_its_child_fits() {
    let mut p = project();
    let outer = top_task(&mut p, "Outer fixed window", 1);
    let inner = top_task(&mut p, "Inner later edge", 1);
    let child = top_task(&mut p, "Short child", 1);
    p.indent(inner).unwrap();
    p.indent(child).unwrap();
    p.indent(child).unwrap();
    set_on(&mut p, outer, true, dt(2026, 10, 23, 17));
    set_on(&mut p, inner, true, dt(2026, 10, 30, 17));
    recalculate(&mut p);
    assert_eq!(p.parent_index(p.task_index(child).unwrap()), p.task_index(inner));
    assert_eq!(finish(&p, inner), dt(2026, 10, 30, 17));
    assert!(has_warning(&p, inner, "Outer fixed window"), "the incompatible fixed inner edge itself must be flagged");
}

#[test]
fn parent_check_ancestor_cap_does_not_move_a_childs_own_hard_finish_pin() {
    let mut p = project();
    let group = top_task(&mut p, "Earlier group finish", 1);
    let child = top_task(&mut p, "Hard pinned child", 1);
    p.indent(child).unwrap();
    set_on(&mut p, group, true, dt(2026, 10, 23, 17));
    set_on(&mut p, child, true, dt(2026, 10, 30, 17));
    p.task_mut(child).unwrap().bounds.finish_latest_fixed = true;
    recalculate(&mut p);
    assert_eq!(finish(&p, child), dt(2026, 10, 30, 17), "keep the child's explicit hard pin and warn about the impossible window");
    assert!(has_warning(&p, child, "Earlier group finish"));
    assert_eq!(p.task(child).unwrap().duration, DAY);
}

#[test]
fn parent_check_successor_of_a_fixed_finish_summary_follows_its_anchor() {
    let mut p = project();
    let group = top_task(&mut p, "Fixed group predecessor", 1);
    let child = top_task(&mut p, "Short child", 1);
    p.indent(child).unwrap();
    let successor = top_task(&mut p, "Successor", 1);
    set_on(&mut p, group, true, dt(2026, 10, 30, 17));
    p.link(group.into(), successor, LinkType::FS, Lag::None).unwrap();
    recalculate(&mut p);
    assert!(start(&p, successor) >= finish(&p, group), "successor must follow the fixed group edge, not the earlier finish of its child");
}

#[test]
fn parent_check_dynamic_outer_group_links_include_fixed_nested_group_edges() {
    for kind in [LinkType::FS, LinkType::SS] {
        let mut p = project();
        let outer = top_task(&mut p, "Dynamic outer", 1);
        let inner = top_task(&mut p, "Fixed inner", 1);
        let child = top_task(&mut p, "Delayed short child", 1);
        p.indent(inner).unwrap();
        p.indent(child).unwrap();
        p.indent(child).unwrap();
        let successor = top_task(&mut p, "Following task", 1);
        set_on(&mut p, inner, false, dt(2026, 10, 22, 8));
        set_on(&mut p, inner, true, dt(2026, 10, 30, 17));
        p.set_leveling_delay(child, DAY).unwrap();
        p.link(outer.into(), successor, kind, Lag::None).unwrap();
        recalculate(&mut p);
        let edge = if kind == LinkType::FS { finish(&p, outer) } else { start(&p, outer) };
        let expected = plancraft_core::schedule::project_calendar(&p).snap_forward(edge);
        assert_eq!(
            start(&p, successor),
            expected,
            "{kind:?}: the dynamic outer group must include fixed nested edges rather than using only raw leaf dates"
        );
        assert_eq!(p.task(child).unwrap().leveling_delay, DAY);
    }
}
