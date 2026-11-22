#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::{NodeT, Queryable};

use super::*;

const DAY: Minutes = 480;

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
fn planning_leveling_delay_edits_reset_and_undo() {
    crate::lang::set_current(crate::lang::Lang::En);
    let mut p = Project::new("Planning delay", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
    let a = p.add_task("A", None);
    let b = p.add_task("B", None);
    p.task_mut(a).unwrap().leveling_delay = DAY;
    p.task_mut(b).unwrap().leveling_delay = 2 * DAY;
    plancraft_core::schedule::recalculate(&mut p);

    let mut ws = Workspace::default();
    ws.add(p, None);
    let mut h = super::harness_with(ws);
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = crate::lang::Lang::En;
    h.state_mut().st.properties_visible = true;
    h.state_mut().st.properties_tab = crate::state::PropTab::Planning;
    h.state_mut().st.sel.tasks.clear();
    h.state_mut().st.sel.tasks.insert(a);
    h.state_mut().st.sel.tasks.insert(b);
    h.state_mut().st.sel.anchor = Some(a);
    h.run_steps(3);

    let label_y = h.get_by_label(crate::lang::text("prop.leveling_delay")).rect().center().y;
    let input = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .filter(|n| !n.accesskit_node().is_disabled())
        .min_by(|a, b| {
            let da = (a.rect().center().y - label_y).abs();
            let db = (b.rect().center().y - label_y).abs();
            da.partial_cmp(&db).unwrap()
        })
        .expect("leveling delay text input");
    input.focus();
    input.type_text("3d");
    h.key_press(Key::Enter);
    h.run_steps(2);
    {
        let ws = lock(&h.state().ws);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.project.task(a).unwrap().leveling_delay, 3 * DAY);
        assert_eq!(doc.project.task(b).unwrap().leveling_delay, 3 * DAY);
    }

    h.get_by_label(crate::lang::text("prop.leveling_delay_reset")).click_accesskit();
    h.run_steps(2);
    {
        let ws = lock(&h.state().ws);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.project.task(a).unwrap().leveling_delay, 0);
        assert_eq!(doc.project.task(b).unwrap().leveling_delay, 0);
    }

    let ctx = h.ctx.clone();
    h.state_mut().exec(&ctx, Command::Undo);
    h.run_steps(2);
    {
        let ws = lock(&h.state().ws);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.project.task(a).unwrap().leveling_delay, 3 * DAY);
        assert_eq!(doc.project.task(b).unwrap().leveling_delay, 3 * DAY);
    }

    let focused = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .filter(|n| !n.accesskit_node().is_disabled())
        .min_by(|a, b| {
            let da = (a.rect().center().y - label_y).abs();
            let db = (b.rect().center().y - label_y).abs();
            da.partial_cmp(&db).unwrap()
        })
        .expect("leveling delay input with value");
    focused.focus();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("3ed".into()));
    h.key_press(Key::Enter);
    h.run_steps(2);
    let ws = lock(&h.state().ws);
    let doc = ws.active_doc().unwrap();
    assert_eq!(doc.project.task(a).unwrap().leveling_delay, 3 * DAY, "invalid elapsed input must not change data");
    assert_eq!(doc.project.task(b).unwrap().leveling_delay, 3 * DAY, "invalid elapsed input must not change data");
}

#[test]
fn planning_reset_discards_pending_focused_delay() {
    crate::lang::set_current(crate::lang::Lang::En);
    let mut p = Project::new("Pending delay", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
    let task = p.add_task("Task", None);
    p.task_mut(task).unwrap().leveling_delay = DAY;
    plancraft_core::schedule::recalculate(&mut p);
    let mut ws = Workspace::default();
    ws.add(p, None);
    let mut h = super::harness_with(ws);
    super::assert_harness_ready(&h);
    h.state_mut().st.properties_visible = true;
    h.state_mut().st.properties_tab = crate::state::PropTab::Planning;
    h.state_mut().st.sel.select_task(task);
    h.run_steps(3);
    let input = h.query_all_by_value("1 day").find(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput).expect("delay input");
    input.focus();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("3d".into()));
    h.run_steps(2);
    h.get_by_label(crate::lang::text("prop.leveling_delay_reset")).click_accesskit();
    h.run_steps(3);
    h.key_press(Key::Tab);
    h.run_steps(3);
    let ws = lock(&h.state().ws);
    assert_eq!(
        ws.active_doc().unwrap().project.task(task).unwrap().leveling_delay,
        0,
        "pending input must not overwrite an explicit reset"
    );
}

#[test]
fn planning_constraint_before_deadline_for_summary_and_leaf() {
    let lang = crate::lang::Lang::En;
    crate::lang::set_current(lang);
    let mut p = Project::new("Planning order", plancraft_core::dates::ymd(2026, 10, 1).unwrap());
    let group = p.add_task("Group", None);
    let leaf = p.add_task("Leaf", None);
    p.indent(leaf).unwrap();
    let snet = plancraft_core::dates::ymd(2026, 10, 15).unwrap().and_hms_opt(8, 0, 0).unwrap();
    let deadline = plancraft_core::dates::ymd(2026, 12, 31).unwrap().and_hms_opt(17, 0, 0).unwrap();
    for id in [group, leaf] {
        let t = p.task_mut(id).unwrap();
        t.mode = ScheduleMode::Manual;
        t.constraint = Constraint { kind: ConstraintKind::Snet, date: Some(snet) };
        t.deadline = Some(deadline);
    }
    plancraft_core::schedule::recalculate(&mut p);

    let mut ws = Workspace::default();
    ws.add(p, None);
    let mut h = super::harness_with(ws);
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = lang;
    h.state_mut().st.properties_visible = true;
    h.state_mut().st.properties_tab = crate::state::PropTab::Planning;

    let mode = crate::lang::tr(lang, "prop.mode");
    let constraint = crate::lang::tr(lang, "prop.constraint");
    let constraint_date = crate::lang::tr(lang, "prop.constraint_date");
    let deadline_label = crate::lang::tr(lang, "prop.deadline");

    for (id, kind) in [(group, "summary"), (leaf, "leaf")] {
        h.state_mut().st.sel.select_task(id);
        h.run_steps(3);
        let mode_y = h.get_by_label(mode).rect().center().y;
        let constraint_y = h.get_by_label(constraint).rect().center().y;
        let constraint_date_y = h.get_by_label(constraint_date).rect().center().y;
        let deadline_y = h.get_by_label(deadline_label).rect().center().y;
        assert!(
            mode_y < constraint_y && constraint_y < constraint_date_y && constraint_date_y < deadline_y,
            "{kind}: expected Schedule mode → Constraint → Constraint date → Deadline, got y={mode_y}/{constraint_y}/{constraint_date_y}/{deadline_y}"
        );
    }
}

#[test]
#[ignore = "renders on the GPU; run explicitly for visual review"]
fn planning_properties_preview_images() {
    use std::path::Path;

    let mut p = Project::new("Planning preview", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
    let task = p.add_task("Task", None);
    p.task_mut(task).unwrap().leveling_delay = DAY;
    plancraft_core::schedule::recalculate(&mut p);
    let base = p;
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target-leveling/previews");
    std::fs::create_dir_all(&out).unwrap();

    for (width, lang, theme) in
        [(1040.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark), (960.0, crate::lang::Lang::Nl, crate::theme::ThemeChoice::Light)]
    {
        crate::lang::set_current(lang);
        let mut ws = Workspace::default();
        ws.add(base.clone(), None);
        let mut h = super::harness_with(ws);
        h.set_size(vec2(width, 900.0));
        super::assert_harness_ready(&h);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetTheme(theme));
        h.state_mut().st.lang = lang;
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::Planning;
        h.state_mut().st.sel.select_task(task);
        h.run_steps(4);
        assert!(h.state().crash.is_none(), "planning preview crashed: {:?}", h.state().crash);
        let pane =
            h.ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::unique("properties_test_visible_rect"))).expect("properties pane rect");
        assert!(pane.width() > 0.0 && pane.height() > 0.0, "pane must have area: {pane:?}");
        let label = h.get_by_label(crate::lang::text("prop.leveling_delay"));
        let label_y = label.rect().center().y;
        let input = h
            .query_all_by_role(egui::accesskit::Role::TextInput)
            .filter(|n| !n.accesskit_node().is_disabled())
            .min_by(|a, b| {
                let da = (a.rect().center().y - label_y).abs();
                let db = (b.rect().center().y - label_y).abs();
                da.partial_cmp(&db).expect("comparable y distances")
            })
            .expect("leveling delay input");
        assert_rect_inside(pane, label.rect(), "leveling delay label");
        assert_rect_inside(pane, input.rect(), "leveling delay input");
        let reset = h.get_by_label(crate::lang::text("prop.leveling_delay_reset"));
        assert!(reset.rect().width() > 0.0 && reset.rect().height() > 0.0, "reset button should be visible");
        assert_rect_inside(pane, reset.rect(), "leveling delay reset");
        let img = h.render().expect("render planning preview");
        let lang_tag = if lang == crate::lang::Lang::En { "en" } else { "nl" };
        let theme_tag = if theme == crate::theme::ThemeChoice::Dark { "dark" } else { "light" };
        img.save(out.join(format!("planning-{theme_tag}-{lang_tag}-{width:.0}.png"))).expect("save planning preview");
    }
}
