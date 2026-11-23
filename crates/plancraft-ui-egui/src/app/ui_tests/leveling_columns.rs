#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use egui_kittest::kittest::Queryable;

use super::*;

const DAY: Minutes = 480;

fn visible_task_names(h: &Harness<'static, PlanCraftApp>) -> Vec<String> {
    let app = h.state();
    let ws = lock(&app.ws);
    let p = &ws.active_doc().unwrap().project;
    crate::views::visible_rows(p, &app.st)
        .into_iter()
        .filter_map(|r| r.task())
        .filter_map(|i| p.tasks.get(i))
        .map(|t| t.name.clone())
        .collect()
}

fn current_delays(h: &Harness<'static, PlanCraftApp>) -> Vec<(String, Minutes)> {
    let ws = lock(&h.state().ws);
    let p = &ws.active_doc().unwrap().project;
    p.tasks.iter().filter(|t| !t.sched.is_summary).map(|t| (t.name.clone(), t.leveling_delay)).collect()
}

#[test]
fn view_toolbar_supports_delay_column_sort_and_filters_without_mutating_data() {
    crate::lang::set_current(crate::lang::Lang::En);
    let mut p = Project::new("Leveling columns", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
    let twelve = p.add_task("Twelve day delay", None);
    let zero = p.add_task("Zero delay", None);
    let two = p.add_task("Two day delay", None);
    p.task_mut(twelve).expect("twelve").leveling_delay = 12 * DAY;
    p.task_mut(zero).expect("zero").leveling_delay = 0;
    p.task_mut(two).expect("two").leveling_delay = 2 * DAY;
    plancraft_core::schedule::recalculate(&mut p);

    let mut ws = Workspace::default();
    ws.add(p, None);
    let mut h = super::harness_with(ws);
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = crate::lang::Lang::En;
    h.state_mut().st.menu_tab = crate::state::MenuTab::View;
    h.run_steps(3);

    let before = current_delays(&h);

    // Details table exposes leveling delay.
    h.state_mut().st.table = crate::columns::TaskTable::Details;
    h.run_steps(2);
    {
        let app = h.state();
        let ws = lock(&app.ws);
        let p = &ws.active_doc().unwrap().project;
        let choice = app.st.columns.get(app.st.table.label());
        let keys: Vec<String> = crate::fields::visible_fields(p, app.st.table, choice).into_iter().map(|f| f.key()).collect();
        assert!(keys.iter().any(|k| k == "leveling_delay"), "details table should expose leveling_delay: {keys:?}");
    }

    // Filter / sort via the same state the View toolbar writes (combo popups are flaky under
    // kittest when the sheet paints wrapped cells).
    h.state_mut().st.filter = crate::state::TaskFilter::WithLevelingDelay;
    h.run_steps(2);
    assert_eq!(visible_task_names(&h), vec!["Twelve day delay".to_string(), "Two day delay".to_string()]);

    h.state_mut().st.sort = Some(("leveling_delay".into(), false));
    h.run_steps(2);
    assert_eq!(visible_task_names(&h), vec!["Two day delay".to_string(), "Twelve day delay".to_string()]);

    h.state_mut().st.filter = crate::state::TaskFilter::WithoutLevelingDelay;
    h.run_steps(2);
    assert_eq!(visible_task_names(&h), vec!["Zero delay".to_string()]);

    let after = current_delays(&h);
    assert_eq!(before, after, "column/filter/sort interactions must not mutate task delays");
}

#[test]
#[ignore = "renders on the GPU; run explicitly for visual review"]
fn delay_column_filter_preview_images() {
    use egui::vec2;
    use std::path::Path;
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target-leveling/previews");
    std::fs::create_dir_all(&out).unwrap();
    for (width, lang, theme) in
        [(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark), (960.0, crate::lang::Lang::Nl, crate::theme::ThemeChoice::Light)]
    {
        crate::lang::set_current(lang);
        let mut p = Project::new("Delay controls", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
        let zero = p.add_task("No delay", None);
        let two = p.add_task("Two day delay", None);
        let twelve = p.add_task("Twelve day delay", None);
        p.set_leveling_delay(two, 2 * DAY).unwrap();
        p.set_leveling_delay(twelve, 12 * DAY).unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        let before: Vec<_> = p.tasks.iter().map(|t| (t.id, t.leveling_delay)).collect();
        let mut ws = Workspace::default();
        ws.add(p, None);
        let mut h = super::harness_with(ws);
        h.set_size(vec2(width, 900.0));
        super::assert_harness_ready(&h);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetTheme(theme));
        h.state_mut().st.lang = lang;
        h.state_mut().st.menu_tab = crate::state::MenuTab::View;
        h.state_mut().st.properties_visible = false;
        h.state_mut().st.view = crate::state::ViewKind::Gantt;
        h.state_mut().st.table_width = 750.0;
        h.state_mut().st.filter = crate::state::TaskFilter::WithLevelingDelay;
        h.run_steps(4);
        assert!(h.state().crash.is_none(), "delay preview frame crashed");
        let header = {
            let ws = lock(&h.state().ws);
            crate::columns::TaskCol::LevelingDelay.def(&ws.active_doc().unwrap().project)
        };
        let font = egui::FontId::proportional(14.0);
        let label_width = h.ctx.fonts_mut(|fonts| fonts.layout_no_wrap(header.title.to_owned(), font, crate::theme::pal().TEXT).size().x);
        assert!(header.width >= label_width + 12.0, "localized delay header needs room: width={} text={label_width}", header.width);
        assert_eq!(visible_task_names(&h), vec!["Two day delay", "Twelve day delay"]);
        assert!(h.query_all_by_label("No delay").next().is_none(), "zero-delay task must be filtered out");
        let ws = lock(&h.state().ws);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.project.task(zero).unwrap().leveling_delay, 0);
        assert_eq!(before, doc.project.tasks.iter().map(|t| (t.id, t.leveling_delay)).collect::<Vec<_>>());
        drop(ws);
        // Font-budget measurement may grow the atlas; rebuild meshes before GPU capture.
        h.run_steps(2);
        let image = h.render().expect("real GPU delay column render");
        image
            .save(out.join(format!("delay-column-{}-{width:.0}.png", if lang == crate::lang::Lang::En { "en" } else { "nl" })))
            .expect("save delay column preview");
    }
}
