#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use plancraft_core::dates::prelude::*;
use plancraft_core::prelude::*;

const DAY: Minutes = 480;

fn d(y: i32, m: u32, day: u32) -> Date {
    plancraft_core::dates::ymd(y, m, day).unwrap()
}

fn dt(y: i32, m: u32, day: u32, h: u32) -> DateTime {
    d(y, m, day).and_hms_opt(h, 0, 0).unwrap()
}

fn start(p: &Project, id: TaskId) -> DateTime {
    p.task(id).unwrap().sched.start.unwrap()
}

fn finish(p: &Project, id: TaskId) -> DateTime {
    p.task(id).unwrap().sched.finish.unwrap()
}

/// Live plan shape for Jaarverslag (2.2.1): dynamic summary edges with a nested Auto leaf that
/// pins Start On + Finish NoLater (moves the plan). Clearing the summary finish to "from
/// subtasks" must keep that child pin as the restricting finish.
#[test]
fn clearing_summary_finish_keeps_child_hard_finish_no_later() {
    let mut p = Project::new("Jaarverslag", d(2026, 11, 1));
    let s = p.add_task("2.2.1", None);
    let mid = p.add_task("2.2.1.1", None);
    let leaf = p.add_task("2.2.1.1.1", None);
    let sib = p.add_task("2.2.1.2", None);
    while p.task(s).unwrap().outline_level > 1 {
        p.outdent(s).unwrap();
    }
    p.indent(mid).unwrap();
    p.indent(leaf).unwrap();
    p.indent(leaf).unwrap();
    p.indent(sib).unwrap();

    p.set_duration(sib, DAY, false).unwrap();
    p.set_date_mode(sib, false, DateMode::On).unwrap();
    p.set_date_window(sib, false, Some(dt(2026, 12, 9, 8)), Some(dt(2026, 12, 9, 8))).unwrap();

    // Leaf: Auto, Start On 26-11, Finish NLT fixed 13-01 (matches inspector).
    p.task_mut(leaf).unwrap().duration_mode = DurationMode::Auto;
    p.set_duration(leaf, 35 * DAY, false).unwrap();
    p.set_date_mode(leaf, false, DateMode::On).unwrap();
    p.set_date_window(leaf, false, Some(dt(2026, 11, 26, 8)), Some(dt(2026, 11, 26, 8))).unwrap();
    p.set_date_mode(leaf, true, DateMode::NoLater).unwrap();
    p.set_date_window(leaf, true, None, Some(dt(2027, 1, 13, 17))).unwrap();
    p.task_mut(leaf).unwrap().bounds.finish_latest_fixed = true;

    // Summary like the first screenshot: start from children, fixed finish 13-01.
    p.set_date_mode(s, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(s, true, DateMode::On).unwrap();
    p.set_date_window(s, true, Some(dt(2027, 1, 13, 17)), Some(dt(2027, 1, 13, 17))).unwrap();
    p.task_mut(s).unwrap().bounds.finish_latest_fixed = true;

    p.set_date_mode(mid, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(mid, true, DateMode::Unrestricted).unwrap();

    recalculate(&mut p);
    assert_eq!(start(&p, leaf), dt(2026, 11, 26, 8), "leaf keeps Start On under summary finish cap");
    assert_eq!(finish(&p, leaf), dt(2027, 1, 13, 17), "leaf meets fixed NLT under summary finish cap");
    assert_eq!(start(&p, s), dt(2026, 11, 26, 8));
    assert_eq!(finish(&p, s), dt(2027, 1, 13, 17));

    // Switch summary finish to "Uit subtaken".
    p.set_date_mode(s, true, DateMode::Unrestricted).unwrap();
    recalculate(&mut p);

    assert_eq!(
        start(&p, leaf),
        dt(2026, 11, 26, 8),
        "child Start On must still hold after summary finish is from subtasks"
    );
    assert_eq!(
        finish(&p, leaf),
        dt(2027, 1, 13, 17),
        "child hard Finish NLT must still hold after summary finish is from subtasks"
    );
    assert_eq!(start(&p, s), dt(2026, 11, 26, 8), "summary start still rolls up from earliest child");
    assert_eq!(
        finish(&p, s),
        dt(2027, 1, 13, 17),
        "summary finish from subtasks must stay capped by the child hard finish"
    );
}

#[test]
fn clearing_summary_finish_keeps_child_finish_on() {
    let mut p = Project::new("On pin", d(2026, 11, 1));
    let s = p.add_task("Group", None);
    let leaf = p.add_task("Leaf", None);
    while p.task(s).unwrap().outline_level > 1 {
        p.outdent(s).unwrap();
    }
    p.indent(leaf).unwrap();

    p.task_mut(leaf).unwrap().duration_mode = DurationMode::Auto;
    p.set_duration(leaf, 35 * DAY, false).unwrap();
    p.set_date_mode(leaf, false, DateMode::On).unwrap();
    p.set_date_window(leaf, false, Some(dt(2026, 11, 26, 8)), Some(dt(2026, 11, 26, 8))).unwrap();
    p.set_date_mode(leaf, true, DateMode::On).unwrap();
    p.set_date_window(leaf, true, Some(dt(2027, 1, 13, 17)), Some(dt(2027, 1, 13, 17))).unwrap();
    p.task_mut(leaf).unwrap().bounds.finish_latest_fixed = true;

    p.set_date_mode(s, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(s, true, DateMode::On).unwrap();
    p.set_date_window(s, true, Some(dt(2027, 1, 13, 17)), Some(dt(2027, 1, 13, 17))).unwrap();
    p.task_mut(s).unwrap().bounds.finish_latest_fixed = true;
    recalculate(&mut p);

    p.set_date_mode(s, true, DateMode::Unrestricted).unwrap();
    recalculate(&mut p);

    assert_eq!(finish(&p, leaf), dt(2027, 1, 13, 17));
    assert_eq!(finish(&p, s), dt(2027, 1, 13, 17));
    assert_eq!(start(&p, s), dt(2026, 11, 26, 8));
}


#[test]
fn summary_predecessor_must_not_outrun_child_hard_finish_when_summary_finish_is_dynamic() {
    // Live shape: FS into 2.2.1 from a milestone that finishes 08-12, child Start On 26-11 +
    // Finish NLT fixed 13-01. Clearing summary finish must keep the child pin (26-11…13-01),
    // not slide the group to the predecessor (09-12…26-01).
    let mut p = Project::new("Pred+pin", d(2026, 11, 1));
    let pred = p.add_task("Pred", None);
    let s = p.add_task("2.2.1", None);
    let leaf = p.add_task("2.2.1.1.1", None);
    while p.task(pred).unwrap().outline_level > 1 {
        p.outdent(pred).unwrap();
    }
    while p.task(s).unwrap().outline_level > 1 {
        p.outdent(s).unwrap();
    }
    p.indent(leaf).unwrap();

    p.set_duration(pred, 0, false).unwrap();
    p.set_date_mode(pred, false, DateMode::On).unwrap();
    p.set_date_window(pred, false, Some(dt(2026, 12, 8, 8)), Some(dt(2026, 12, 8, 8))).unwrap();
    p.set_date_mode(pred, true, DateMode::On).unwrap();
    p.set_date_window(pred, true, Some(dt(2026, 12, 8, 17)), Some(dt(2026, 12, 8, 17))).unwrap();
    p.link(pred.into(), s, LinkType::FS, Lag::None).unwrap();

    p.task_mut(leaf).unwrap().duration_mode = DurationMode::Auto;
    p.set_duration(leaf, 35 * DAY, false).unwrap();
    p.set_date_mode(leaf, false, DateMode::On).unwrap();
    p.set_date_window(leaf, false, Some(dt(2026, 11, 26, 8)), Some(dt(2026, 11, 26, 8))).unwrap();
    p.set_date_mode(leaf, true, DateMode::NoLater).unwrap();
    p.set_date_window(leaf, true, None, Some(dt(2027, 1, 13, 17))).unwrap();
    p.task_mut(leaf).unwrap().bounds.finish_latest_fixed = true;

    p.task_mut(s).unwrap().fills_group = false;
    p.set_date_mode(s, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(s, true, DateMode::On).unwrap();
    p.set_date_window(s, true, Some(dt(2027, 1, 13, 17)), Some(dt(2027, 1, 13, 17))).unwrap();
    p.task_mut(s).unwrap().bounds.finish_latest_fixed = true;

    recalculate(&mut p);
    assert_eq!(finish(&p, pred), dt(2026, 12, 8, 17));
    assert_eq!(start(&p, leaf), dt(2026, 11, 26, 8), "under summary finish cap: child Start On");
    assert_eq!(finish(&p, leaf), dt(2027, 1, 13, 17));
    assert_eq!(start(&p, s), dt(2026, 11, 26, 8));
    assert_eq!(finish(&p, s), dt(2027, 1, 13, 17));

    p.set_date_mode(s, true, DateMode::Unrestricted).unwrap();
    recalculate(&mut p);

    assert_eq!(start(&p, leaf), dt(2026, 11, 26, 8), "child Start On must win over summary FS pred");
    assert_eq!(finish(&p, leaf), dt(2027, 1, 13, 17), "child hard NLT must still hold");
    assert_eq!(start(&p, s), dt(2026, 11, 26, 8), "summary start from subtasks follows child, not only FS pred");
    assert_eq!(finish(&p, s), dt(2027, 1, 13, 17), "summary finish from subtasks follows child hard finish");
}


#[test]
fn nested_share_floor_must_not_outrun_child_hard_finish_no_later() {
    // An ancestor Fill positions a start floor after the child's Start On. Own finish_latest_fixed
    // must still win (Merlin: fixed deadline overrides driven start). Previously share floors were
    // applied *after* apply_date_bounds, so clearing a summary finish cap let the floor stick and
    // the bar slid to floor…floor+duration (09-12…26-01 on Jaarverslag).
    let mut p = Project::new("Share floor vs hard NLT", d(2026, 11, 1));
    let outer = p.add_task("Outer", None);
    let stretch = p.add_task("Stretch", None);
    let mid = p.add_task("Mid", None);
    let leaf = p.add_task("Leaf", None);
    while p.task(outer).unwrap().outline_level > 1 {
        p.outdent(outer).unwrap();
    }
    p.indent(stretch).unwrap();
    p.indent(mid).unwrap();
    p.indent(mid).unwrap();
    p.indent(leaf).unwrap();
    p.indent(leaf).unwrap();
    p.indent(leaf).unwrap();

    // Outer is a late fixed window; Stretch fills it and floors descendants at 09-12.
    p.set_date_mode(outer, false, DateMode::On).unwrap();
    p.set_date_window(outer, false, Some(dt(2026, 12, 9, 8)), Some(dt(2026, 12, 9, 8))).unwrap();
    p.set_date_mode(outer, true, DateMode::On).unwrap();
    p.set_date_window(outer, true, Some(dt(2027, 2, 15, 17)), Some(dt(2027, 2, 15, 17))).unwrap();
    p.task_mut(outer).unwrap().bounds.finish_latest_fixed = true;

    p.task_mut(stretch).unwrap().fills_group = true;
    p.set_date_mode(stretch, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(stretch, true, DateMode::Unrestricted).unwrap();

    // Mid spans its activities (like 2.2.1 "Omspant") — finish comes from the child pin.
    p.task_mut(mid).unwrap().fills_group = false;
    p.set_date_mode(mid, false, DateMode::Unrestricted).unwrap();
    p.set_date_mode(mid, true, DateMode::Unrestricted).unwrap();

    p.task_mut(leaf).unwrap().duration_mode = DurationMode::Auto;
    p.set_duration(leaf, 35 * DAY, false).unwrap();
    p.set_date_mode(leaf, false, DateMode::On).unwrap();
    p.set_date_window(leaf, false, Some(dt(2026, 11, 26, 8)), Some(dt(2026, 11, 26, 8))).unwrap();
    p.set_date_mode(leaf, true, DateMode::NoLater).unwrap();
    p.set_date_window(leaf, true, None, Some(dt(2027, 1, 13, 17))).unwrap();
    p.task_mut(leaf).unwrap().bounds.finish_latest_fixed = true;
    p.task_mut(leaf).unwrap().fills_group = false;

    recalculate(&mut p);
    assert_eq!(finish(&p, leaf), dt(2027, 1, 13, 17), "hard NLT must hold against nested share floor");
    assert_eq!(start(&p, leaf), dt(2026, 11, 26, 8), "Start On + hard NLT window must hold");
    assert_eq!(start(&p, mid), dt(2026, 11, 26, 8), "Omspant mid start follows the pinned child");
    assert_eq!(finish(&p, mid), dt(2027, 1, 13, 17), "Omspant mid finish follows the hard-pinned child");
}
