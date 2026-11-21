#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::Path;

use egui::vec2;
use egui_kittest::kittest::{NodeT, Queryable};

use super::*;

fn assert_rect_inside(container: egui::Rect, rect: egui::Rect, what: &str) {
    assert!(
        rect.left() >= container.left() - 0.5
            && rect.right() <= container.right() + 0.5
            && rect.top() >= container.top() - 0.5
            && rect.bottom() <= container.bottom() + 0.5,
        "{what} must stay inside the Properties pane: rect={rect:?} pane={container:?}"
    );
}

#[test]
fn general_status_presets_use_board_columns_and_percent_stays_in_progress() {
    crate::lang::set_current(crate::lang::Lang::En);
    let mut p = Project::new("General status", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
    let id = p.add_task("Task", None);
    let review = p.add_board_column("Review", None, None).unwrap();
    plancraft_core::schedule::recalculate(&mut p);
    let mut ws = Workspace::default();
    ws.add(p, None);
    let mut h = super::harness_with(ws);
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = crate::lang::Lang::En;
    h.state_mut().st.sel.select_task(id);
    h.state_mut().st.properties_visible = true;
    h.state_mut().st.properties_tab = crate::state::PropTab::General;
    h.run_steps(3);

    assert!(h.query_all_by_label("Percent complete").next().is_none(), "General must not expose Percent complete; it belongs in Progress");
    h.get_by_label("Review").click_accesskit();
    h.run_steps(2);
    {
        let ws = lock(&h.state().ws);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.project.task(id).unwrap().board_status, Some(review));
    }

    h.state_mut().st.properties_tab = crate::state::PropTab::Progress;
    h.run_steps(2);
    assert!(h.query_all_by_label("Percent complete").next().is_some(), "Progress keeps Percent complete");
}

#[test]
fn general_placement_picker_round_trips_with_undo_without_changing_windows() {
    for lang in [crate::lang::Lang::En, crate::lang::Lang::Nl] {
        crate::lang::set_current(lang);
        let mut p = Project::new("Placement", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
        let _group = p.add_task("Group", None);
        let id = p.add_task("Prep", None);
        {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.fills_group = false;
            t.constraint = Constraint {
                kind: ConstraintKind::Fnlt,
                date: Some(plancraft_core::dates::ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap()),
            };
        }
        plancraft_core::schedule::recalculate(&mut p);
        let mut ws = Workspace::default();
        ws.add(p, None);
        let mut h = super::harness_with(ws);
        super::assert_harness_ready(&h);
        h.state_mut().st.lang = lang;
        h.state_mut().st.sel.select_task(id);
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;
        h.run_steps(3);

        let asap = crate::lang::tr(lang, "prop.placement_asap");
        let alap = crate::lang::tr(lang, "prop.placement_alap");
        h.get_by_value(asap).click_accesskit();
        h.run_steps(1);
        h.get_by_label(alap).click_accesskit();
        h.run_steps(1);
        {
            let ws = lock(&h.state().ws);
            let t = ws.active_doc().unwrap().project.task(id).unwrap();
            assert_eq!(t.constraint.kind, ConstraintKind::Alap);
            assert_eq!(t.bounds.finish_latest, Some(plancraft_core::dates::ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap()));
            assert_eq!(t.constraint.date, None);
        }
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::Undo);
        h.run_steps(2);
        let ws = lock(&h.state().ws);
        let t = ws.active_doc().unwrap().project.task(id).unwrap();
        assert_eq!(t.constraint.kind, ConstraintKind::Fnlt);
        assert_eq!(t.constraint.date, Some(plancraft_core::dates::ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap()));
    }
}

#[test]
fn general_date_labels_distinguish_scheduled_result_and_limit_in_en_nl() {
    for lang in [crate::lang::Lang::En, crate::lang::Lang::Nl] {
        crate::lang::set_current(lang);
        let mut p = Project::new("Date labels", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let free = p.add_task("Free", None);
        let limited = p.add_task("Limited", None);
        let fixed = p.add_task("Fixed", None);
        let hand = p.add_task("Hand deadline", None);
        {
            let t = p.task_mut(limited).unwrap();
            t.bounds.finish_latest = Some(plancraft_core::dates::ymd(2026, 11, 11).unwrap().and_hms_opt(17, 0, 0).unwrap());
        }
        {
            let day = plancraft_core::dates::ymd(2026, 10, 15).unwrap().and_hms_opt(8, 0, 0).unwrap();
            let t = p.task_mut(fixed).unwrap();
            t.bounds.start_earliest = Some(day);
            t.bounds.start_latest = Some(day);
            let end = plancraft_core::dates::ymd(2026, 12, 31).unwrap().and_hms_opt(17, 0, 0).unwrap();
            t.bounds.finish_earliest = Some(end);
            t.bounds.finish_latest = Some(end);
        }
        {
            // Hand-set finish limit: Task.deadline, independent of DateMode (side stays Unrestricted).
            let t = p.task_mut(hand).unwrap();
            t.deadline = Some(plancraft_core::dates::ymd(2026, 11, 20).unwrap().and_hms_opt(17, 0, 0).unwrap());
        }
        plancraft_core::schedule::recalculate(&mut p);
        let mut ws = Workspace::default();
        ws.add(p, None);
        let mut h = super::harness_with(ws);
        super::assert_harness_ready(&h);
        h.state_mut().st.lang = lang;
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;

        let start_limit = crate::lang::tr(lang, "prop.start_limit");
        let finish_limit = crate::lang::tr(lang, "prop.finish_limit");
        let deadline = crate::lang::tr(lang, "prop.deadline");
        let hand_deadline = crate::widgets::date_picker::format_dutch(
            plancraft_core::dates::ymd(2026, 11, 20).unwrap().and_hms_opt(17, 0, 0).unwrap(),
        );
        let nlt = crate::widgets::date_picker::format_dutch(
            plancraft_core::dates::ymd(2026, 11, 11).unwrap().and_hms_opt(17, 0, 0).unwrap(),
        );

        h.state_mut().st.sel.select_task(free);
        h.run_steps(3);
        assert!(h.query_all_by_label(finish_limit).next().is_none(), "{lang:?}: unrestricted task must not expose a fake finish limit");

        // Rule NoLater: edit the constraint in the mode-row date — no separate Finish limit row.
        h.state_mut().st.sel.select_task(limited);
        h.run_steps(2);
        assert!(
            h.query_all_by_label(finish_limit).next().is_none(),
            "{lang:?}: rule NoLater uses the mode-row date, not a Finish limit label"
        );
        assert!(!h.get_by_label(&nlt).accesskit_node().is_disabled(), "{lang:?}: rule NLT date must be editable in the mode row");

        // Rule Fixed (On): same — mode-row pin only.
        h.state_mut().st.sel.select_task(fixed);
        h.run_steps(2);
        assert!(h.query_all_by_label(start_limit).next().is_none(), "{lang:?}: Fixed start must not show a second Start limit field");
        assert!(h.query_all_by_label(finish_limit).next().is_none(), "{lang:?}: Fixed finish must not show a second Finish limit field");

        // Hand finish limit = Task.deadline on the Planning tab; finish DateMode stays Unrestricted.
        h.state_mut().st.sel.select_task(hand);
        h.state_mut().st.properties_tab = crate::state::PropTab::Planning;
        h.run_steps(2);
        assert_eq!(
            {
                let ws = lock(&h.state().ws);
                ws.active_doc().unwrap().project.task(hand).unwrap().finish_mode()
            },
            DateMode::Unrestricted,
            "{lang:?}: hand deadline must not flip the finish side into a DateMode rule"
        );
        assert!(h.query_all_by_label(deadline).next().is_some(), "{lang:?}: hand deadline control lives on Planning");
        assert!(!h.get_by_label(&hand_deadline).accesskit_node().is_disabled(), "{lang:?}: hand deadline must stay editable");
        h.state_mut().st.properties_tab = crate::state::PropTab::General;
        h.run_steps(2);
        assert!(h.query_all_by_label(finish_limit).next().is_none(), "{lang:?}: hand deadline must not invent a Finish limit row on General");
    }
}

#[test]
fn summary_edges_expose_dynamic_vs_fixed_per_side_and_keep_sides_independent() {
    for lang in [crate::lang::Lang::En, crate::lang::Lang::Nl] {
        crate::lang::set_current(lang);
        let mut p = Project::new("Summary edges", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let group = p.add_task("Group", None);
        let child = p.add_task("Child", None);
        {
            let t = p.task_mut(child).unwrap();
            t.duration = 2 * 480;
            t.fills_group = false;
        }
        p.indent(child).unwrap();
        p.set_placement(group, ConstraintKind::Alap).unwrap();
        p.set_date_mode(group, true, DateMode::Between).unwrap();
        p.set_date_window(
            group,
            true,
            Some(plancraft_core::dates::ymd(2026, 10, 23).unwrap().and_hms_opt(17, 0, 0).unwrap()),
            Some(plancraft_core::dates::ymd(2026, 10, 28).unwrap().and_hms_opt(17, 0, 0).unwrap()),
        )
        .unwrap();
        plancraft_core::schedule::recalculate(&mut p);

        let mut ws = Workspace::default();
        ws.add(p, None);
        let mut h = super::harness_with(ws);
        super::assert_harness_ready(&h);
        h.state_mut().st.lang = lang;
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;
        h.state_mut().st.sel.select_task(group);
        h.run_steps(3);

        let start_label = crate::lang::tr(lang, "prop.start");
        let finish_label = crate::lang::tr(lang, "prop.finish");
        let dynamic = crate::lang::tr(lang, "prop.group_edge_dynamic");
        let fixed = crate::lang::tr(lang, "prop.group_edge_fixed");
        let between = crate::lang::tr(lang, "dm.between");

        let start_row = h.get_by_label(start_label);
        let start_combo = h
            .query_all_by_value(dynamic)
            .min_by(|a, b| {
                let da = (a.rect().center().y - start_row.rect().center().y).abs();
                let db = (b.rect().center().y - start_row.rect().center().y).abs();
                da.partial_cmp(&db).expect("comparable y distances")
            })
            .expect("start edge combo");
        start_combo.click_accesskit();
        h.run_steps(1);
        h.get_by_label(fixed).click_accesskit();
        h.run_steps(2);

        {
            let ws = lock(&h.state().ws);
            let t = ws.active_doc().unwrap().project.task(group).unwrap();
            assert_eq!(t.start_mode(), DateMode::On, "{lang:?}: start side switches to fixed mode");
            assert_eq!(t.finish_mode(), DateMode::Between, "{lang:?}: finish side stays in Between mode");
        }

        let finish_row_y = h.get_by_label(finish_label).rect().center().y;
        let finish_combo = h
            .query_all_by_value(between)
            .min_by(|a, b| {
                let da = (a.rect().center().y - finish_row_y).abs();
                let db = (b.rect().center().y - finish_row_y).abs();
                da.partial_cmp(&db).expect("comparable y distances")
            })
            .expect("finish edge combo in Between mode");
        assert!(
            (finish_combo.rect().center().y - finish_row_y).abs() <= 2.0,
            "{lang:?}: finish edge keeps its Between selector row alignment"
        );

        let ws = lock(&h.state().ws);
        let t = ws.active_doc().unwrap().project.task(group).unwrap();
        assert_eq!(t.start_mode(), DateMode::On, "{lang:?}: start side remains fixed after editing finish");
        assert_eq!(t.finish_mode(), DateMode::Between, "{lang:?}: finish side switches to Between");
        let (mn, mx) = t.finish_window();
        assert!(mn.is_some() && mx.is_some() && mn != mx, "{lang:?}: finish Between keeps a real range");
        assert_eq!(t.constraint.kind, ConstraintKind::Alap, "{lang:?}: summary edge edits must not alter placement");
    }
}

#[test]
fn general_auto_tasks_hide_stale_manual_dates_but_manual_tasks_show_theirs_in_en_nl() {
    let auto_start = plancraft_core::dates::ymd(2026, 11, 3).unwrap().and_hms_opt(8, 0, 0).unwrap();
    let auto_finish = plancraft_core::dates::ymd(2026, 11, 4).unwrap().and_hms_opt(17, 0, 0).unwrap();
    let manual_start = plancraft_core::dates::ymd(2026, 11, 10).unwrap().and_hms_opt(8, 0, 0).unwrap();
    let manual_finish = plancraft_core::dates::ymd(2026, 11, 11).unwrap().and_hms_opt(17, 0, 0).unwrap();
    let auto_start_text = crate::widgets::date_picker::format_dutch(auto_start);
    let auto_finish_text = crate::widgets::date_picker::format_dutch(auto_finish);
    let manual_start_text = crate::widgets::date_picker::format_dutch(manual_start);
    let manual_finish_text = crate::widgets::date_picker::format_dutch(manual_finish);
    for lang in [crate::lang::Lang::En, crate::lang::Lang::Nl] {
        crate::lang::set_current(lang);
        let mut p = Project::new("Date labels", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let auto = p.add_task("Auto", None);
        let manual = p.add_task("Manual", None);
        {
            let t = p.task_mut(auto).unwrap();
            t.mode = ScheduleMode::Auto;
            t.manual_start = Some(auto_start);
            t.manual_finish = Some(auto_finish);
        }
        {
            let t = p.task_mut(manual).unwrap();
            t.mode = ScheduleMode::Manual;
            t.manual_start = Some(manual_start);
            t.manual_finish = Some(manual_finish);
        }
        let mut ws = Workspace::default();
        ws.add(p, None);
        let mut h = super::harness_with(ws);
        super::assert_harness_ready(&h);
        h.state_mut().st.lang = lang;
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;

        h.state_mut().st.sel.select_task(auto);
        h.run_steps(3);
        let limit = crate::lang::tr(lang, "prop.finish_limit");
        assert!(h.query_all_by_label(limit).next().is_none(), "{lang:?}: auto task must not expose a fake finish limit");
        assert!(h.query_all_by_label(&auto_start_text).next().is_none(), "{lang:?}: stale auto start must stay hidden");
        assert!(h.query_all_by_label(&auto_finish_text).next().is_none(), "{lang:?}: stale auto finish must stay hidden");

        // Manual pins show as Fixed (On) in the mode-row date slot — no second Finish limit row.
        h.state_mut().st.sel.select_task(manual);
        h.run_steps(2);
        assert!(h.query_all_by_label(limit).next().is_none(), "{lang:?}: Fixed manual finish must not show a second Finish limit field");
        assert!(h.query_all_by_label(&manual_start_text).next().is_some(), "{lang:?}: manual start must show for manual task");
        assert!(h.query_all_by_label(&manual_finish_text).next().is_some(), "{lang:?}: manual finish must show for manual task");
    }
}

#[test]
fn general_compact_rows_keep_start_mode_and_result_on_one_line_when_roomy() {
    crate::lang::set_current(crate::lang::Lang::En);
    let mut p = Project::new("Compact", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
    let prep = p.add_task("Prep", None);
    let send = p.add_task("Send", None);
    p.task_mut(prep).unwrap().duration = 2 * 480;
    p.task_mut(send).unwrap().duration = 480;
    p.task_mut(send).unwrap().bounds.finish_latest = Some(plancraft_core::dates::ymd(2026, 12, 15).unwrap().and_hms_opt(17, 0, 0).unwrap());
    p.link(prep.into(), send, LinkType::FS, Lag::None).unwrap();
    plancraft_core::schedule::recalculate(&mut p);
    let start_text = crate::widgets::date_picker::format_dutch(p.task(send).unwrap().sched.start.unwrap());

    let mut ws = Workspace::default();
    ws.add(p, None);
    let mut h = super::harness_with(ws);
    h.set_size(vec2(1040.0, 900.0));
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = crate::lang::Lang::En;
    h.state_mut().st.properties_visible = true;
    h.state_mut().st.properties_tab = crate::state::PropTab::General;
    h.state_mut().st.sel.select_task(send);
    h.run_steps(4);

    let start_label = h.get_by_label(crate::lang::text("prop.start"));
    let mode = h.get_by_value(crate::lang::text("dm.unrestricted"));
    let start_value = h.get_by_label(&start_text);
    assert!(
        (start_label.rect().center().y - mode.rect().center().y).abs() <= 0.5
            && (start_label.rect().center().y - start_value.rect().center().y).abs() <= 0.5,
        "start label, mode and result should align on one row"
    );
}

#[test]
fn size_in_group_picker_shows_for_nested_leaf_and_summary_en_nl() {
    for lang in [crate::lang::Lang::En, crate::lang::Lang::Nl] {
        crate::lang::set_current(lang);
        let mut p = Project::new("Size in group", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
        let root = p.add_task("Root", None);
        let nest = p.add_task("Nest", None);
        let leaf = p.add_task("Leaf", None);
        p.task_mut(nest).unwrap().outline_level = 2;
        p.task_mut(leaf).unwrap().outline_level = 3;
        p.task_mut(nest).unwrap().fills_group = false;
        p.task_mut(leaf).unwrap().fills_group = false;
        plancraft_core::schedule::recalculate(&mut p);

        let mut ws = Workspace::default();
        ws.add(p, None);
        let mut h = super::harness_with(ws);
        super::assert_harness_ready(&h);
        h.state_mut().st.lang = lang;
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;

        let size_label = crate::lang::tr(lang, "prop.size_in_group");
        let spans = crate::lang::tr(lang, "prop.size_spans");
        let own = crate::lang::tr(lang, "prop.size_own");
        let fill = crate::lang::tr(lang, "prop.size_fill");
        let placement_label = crate::lang::tr(lang, "prop.placement");
        let asap = crate::lang::tr(lang, "prop.placement_asap");
        let alap = crate::lang::tr(lang, "prop.placement_alap");

        h.state_mut().st.sel.select_task(root);
        h.run_steps(3);
        assert!(h.query_all_by_label(size_label).next().is_none(), "{lang:?}: top-level has no Size in group");

        h.state_mut().st.sel.select_task(nest);
        h.run_steps(2);
        assert!(h.query_all_by_label(size_label).next().is_some(), "{lang:?}: nested summary shows Size in group");
        assert!(h.query_all_by_value(spans).next().is_some(), "{lang:?}: summary own label is Spans its activities");
        assert!(h.query_all_by_label(placement_label).next().is_some(), "{lang:?}: nested summary shows Placement beside Size of group");
        h.get_by_value(asap).click_accesskit();
        h.run_steps(2);
        h.get_by_label(alap).click_accesskit();
        h.run_steps(3);
        {
            let ws = lock(&h.state().ws);
            assert_eq!(
                ws.active_doc().unwrap().project.task(nest).unwrap().constraint.kind,
                ConstraintKind::Alap,
                "{lang:?}: nested summary placement is editable"
            );
        }

        h.state_mut().st.sel.select_task(leaf);
        h.run_steps(2);
        assert!(h.query_all_by_label(size_label).next().is_some(), "{lang:?}: nested leaf shows Size in group");
        assert!(h.query_all_by_value(own).next().is_some(), "{lang:?}: leaf own label is Own duration");

        // Open the combo and pick Remaining space.
        h.get_by_value(own).click_accesskit();
        h.run_steps(2);
        h.get_by_label(fill).click_accesskit();
        h.run_steps(3);
        {
            let ws = lock(&h.state().ws);
            let t = ws.active_doc().unwrap().project.task(leaf).unwrap();
            assert!(t.fills_group, "{lang:?}: Remaining space sets fills_group");
            assert!(t.duration_percent.is_none());
            assert_eq!(t.start_mode(), DateMode::Unrestricted);
            assert_eq!(t.finish_mode(), DateMode::Unrestricted);
        }
        assert!(h.query_all_by_value(fill).next().is_some(), "{lang:?}: picker shows Remaining space");
    }
}

#[test]
fn general_preview_geometry_stays_inside_pane_en_nl() {
    let base = {
        let mut p = Project::new("General previews", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
        let _g = p.add_task("Group", None);
        let prep = p.add_task("Preparation", None);
        let send = p.add_task("Send", None);
        for id in [prep, send] {
            let t = p.task_mut(id).unwrap();
            t.outline_level = 2;
            t.fills_group = false;
        }
        p.task_mut(send).unwrap().bounds.finish_latest =
            Some(plancraft_core::dates::ymd(2026, 12, 15).unwrap().and_hms_opt(17, 0, 0).unwrap());
        p.link(prep.into(), send, LinkType::FS, Lag::None).unwrap();
        p.set_duration_mode(prep, DurationMode::Auto).unwrap();
        p.add_board_column("Review", None, None).unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        p
    };
    for (width, lang) in [(1040.0, crate::lang::Lang::En), (960.0, crate::lang::Lang::Nl)] {
        crate::lang::set_current(lang);
        let mut ws = Workspace::default();
        ws.add(base.clone(), None);
        let mut h = super::harness_with(ws);
        h.set_size(vec2(width, 900.0));
        super::assert_harness_ready(&h);
        h.state_mut().st.lang = lang;
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;
        let send = {
            let ws = lock(&h.state().ws);
            ws.active_doc().unwrap().project.tasks.iter().find(|t| t.name == "Send").map(|t| t.id).expect("send task")
        };
        h.state_mut().st.sel.select_task(send);
        h.run_steps(4);

        let pane = h
            .ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("properties_test_visible_rect")))
            .expect("actual allocated Properties pane rectangle");
        assert!(pane.width() > 0.0 && pane.height() > 0.0, "pane must have area: {pane:?}");
        let roomy_row = lang == crate::lang::Lang::En;
        let send_start = {
            let ws = lock(&h.state().ws);
            let doc = ws.active_doc().unwrap();
            crate::widgets::date_picker::format_dutch(
                doc.project.task(send).unwrap().sched.start.expect("send should have a scheduled start"),
            )
        };
        let send_limit = {
            let ws = lock(&h.state().ws);
            let doc = ws.active_doc().unwrap();
            crate::widgets::date_picker::format_dutch(
                doc.project.task(send).unwrap().bounds.finish_latest.expect("send should have a finish limit"),
            )
        };

        let start_label = h.get_by_label(crate::lang::text("prop.start"));
        let start_mode = h.get_by_value(crate::lang::text("dm.unrestricted"));
        let start_value = h.get_by_label(&send_start);
        if roomy_row {
            assert!((start_label.rect().center().y - start_mode.rect().center().y).abs() <= 0.5);
            assert!((start_label.rect().center().y - start_value.rect().center().y).abs() <= 0.5);
        }
        assert_rect_inside(pane, start_label.rect(), "start label");
        assert_rect_inside(pane, start_mode.rect(), "start mode");
        assert_rect_inside(pane, start_value.rect(), "start value");

        let finish_label = h.get_by_label(crate::lang::text("prop.finish"));
        let finish_mode = h.get_by_value(crate::lang::text("dm.no_later"));
        // NoLater: mode-row date is the constraint (NLT), not a separate scheduled-result field.
        let finish_value = h.get_by_label(&send_limit);
        assert_rect_inside(pane, finish_label.rect(), "finish label");
        assert_rect_inside(pane, finish_mode.rect(), "finish mode");
        assert_rect_inside(pane, finish_value.rect(), "finish constraint date");

        let duration_label = h.get_by_label(crate::lang::text("prop.duration"));
        let duration_mode = h.get_by_value(crate::lang::text("prop.duration_fixed"));
        let duration_value = h
            .query_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|n| !n.accesskit_node().is_disabled())
            .min_by(|a, b| {
                let da = (a.rect().center().y - duration_label.rect().center().y).abs();
                let db = (b.rect().center().y - duration_label.rect().center().y).abs();
                da.partial_cmp(&db).expect("comparable y distances")
            })
            .expect("duration input");
        assert_rect_inside(pane, duration_label.rect(), "duration label");
        assert_rect_inside(pane, duration_mode.rect(), "duration mode");
        assert_rect_inside(pane, duration_value.rect(), "duration value");
        assert!(
            (duration_label.rect().center().y - duration_mode.rect().center().y).abs() <= 0.5,
            "duration mode should share the label row"
        );
        assert!(
            (duration_label.rect().center().y - duration_value.rect().center().y).abs() <= 0.5,
            "duration value should fit beside its compact mode: label={:?} mode={:?} value={:?}",
            duration_label.rect(),
            duration_mode.rect(),
            duration_value.rect()
        );

        let placement_label = h.get_by_label(crate::lang::text("prop.placement"));
        let placement_value = h.get_by_value(crate::lang::text("prop.placement_asap"));
        assert_rect_inside(pane, placement_label.rect(), "placement label");
        assert_rect_inside(pane, placement_value.rect(), "placement value");

        assert_rect_inside(pane, h.get_by_label(&send_limit).rect(), "finish limit");
    }
}

#[test]
#[ignore = "renders on the GPU; run explicitly for visual review"]
fn general_properties_preview_images() {
    crate::lang::set_current(crate::lang::Lang::En);
    let mut p = Project::new("General previews", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
    let _g = p.add_task("Group", None);
    let prep = p.add_task("Preparation", None);
    let send = p.add_task("Send", None);
    for id in [prep, send] {
        let t = p.task_mut(id).unwrap();
        t.outline_level = 2;
        t.fills_group = false;
    }
    p.task_mut(send).unwrap().bounds.finish_latest = Some(plancraft_core::dates::ymd(2026, 12, 15).unwrap().and_hms_opt(17, 0, 0).unwrap());
    p.link(prep.into(), send, LinkType::FS, Lag::None).unwrap();
    p.set_duration_mode(prep, DurationMode::Auto).unwrap();
    p.add_board_column("Review", None, None).unwrap();
    plancraft_core::schedule::recalculate(&mut p);
    let start_text =
        crate::widgets::date_picker::format_dutch(p.task(send).unwrap().sched.start.expect("send should have a scheduled start"));
    let send_finish =
        crate::widgets::date_picker::format_dutch(p.task(send).unwrap().sched.finish.expect("send should have a scheduled finish"));
    let send_limit =
        crate::widgets::date_picker::format_dutch(p.task(send).unwrap().bounds.finish_latest.expect("send should have a finish limit"));

    let base = p;
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target-leveling/previews");
    std::fs::create_dir_all(&out).unwrap();

    fn wheel(h: &mut egui_kittest::Harness<'static, PlanCraftApp>, delta_y: f32) {
        let win = h.ctx.content_rect();
        h.event(egui::Event::PointerMoved(egui::pos2(win.right() - 24.0, win.center().y)));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, delta_y),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
    }

    for (filename, detail_filename, width, lang, theme) in [
        ("general-dark-en-1040.png", "general-dark-en-1040-detail.png", 1040.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark),
        ("general-light-nl-960.png", "general-light-nl-960-detail.png", 960.0, crate::lang::Lang::Nl, crate::theme::ThemeChoice::Light),
    ] {
        crate::lang::set_current(lang);
        let mut ws = Workspace::default();
        ws.add(base.clone(), None);
        let mut h = super::harness_with(ws);
        h.set_size(vec2(width, 900.0));
        super::assert_harness_ready(&h);
        h.state_mut().st.lang = lang;
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetTheme(theme));
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;
        h.state_mut().st.sel.select_task(send);
        h.run_steps(4);
        assert!(h.state().crash.is_none(), "preview frame crashed: {:?}", h.state().crash);
        // NoLater: constraint date lives in the mode-row slot — no separate Finish limit label / scheduled pair.
        let limit_label = crate::lang::tr(lang, "prop.finish_limit");
        assert!(h.query_all_by_label(limit_label).next().is_none(), "NoLater must not show a second Finish limit field");
        assert!(!h.get_by_label(&send_limit).accesskit_node().is_disabled(), "NLT constraint date must remain editable");
        assert!(
            h.query_all_by_label(crate::lang::tr(lang, "prop.leveling_delay")).next().is_none(),
            "General tab should not show leveling delay editor"
        );
        assert_ne!(send_finish, send_limit, "scheduled finish and NLT should differ for this fixture");
        let status = h.query_all_by_label(crate::lang::text("prop.status")).next().expect("status label");
        let name_field =
            h.query_all_by_value("Send").find(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput).expect("name text input");
        assert!(status.rect().top() < name_field.rect().top(), "status must appear before Name");
        let pane = h
            .ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("properties_test_visible_rect")))
            .expect("actual allocated Properties pane rectangle");
        assert!(pane.width() > 0.0 && pane.height() > 0.0, "properties pane should have area: {pane:?}");
        let start_label = h.get_by_label(crate::lang::text("prop.start"));
        let start_mode = h.get_by_value(crate::lang::text("dm.unrestricted"));
        let start_value = h.get_by_label(&start_text);
        assert_rect_inside(pane, start_label.rect(), "start label");
        assert_rect_inside(pane, start_mode.rect(), "start mode");
        assert_rect_inside(pane, start_value.rect(), "start value");
        let finish_mode = h.get_by_value(crate::lang::text("dm.no_later"));
        let limit_rect = h.get_by_label(&send_limit).rect();
        assert!(limit_rect.width() > 0.0 && limit_rect.height() > 0.0, "NLT constraint date should be painted");
        assert_rect_inside(pane, finish_mode.rect(), "finish mode");
        assert_rect_inside(pane, limit_rect, "finish NLT date");
        let img = h.render().expect("render general preview");
        img.save(out.join(filename)).expect("save general preview");
        wheel(&mut h, -240.0);
        h.run_steps(1);
        assert!(h.state().crash.is_none(), "detail scroll frame crashed: {:?}", h.state().crash);
        let detail = h.render().expect("render general preview detail");
        detail.save(out.join(detail_filename)).expect("save general preview detail");
    }
}
