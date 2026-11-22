#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Clone, Serialize, Deserialize)]
struct ManifestEntry {
    filename: String,
    view_state: String,
    language: String,
    theme: String,
    size: [u32; 2],
}

fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/docs-screenshots")
}

fn assert_clean(h: &Harness<'static, PlanCraftApp>, what: &str) {
    assert!(h.state().crash.is_none(), "{what}: {:?}", h.state().crash);
}

fn settle(h: &mut Harness<'static, PlanCraftApp>, frames: usize, what: &str) {
    for _ in 0..frames {
        h.run_steps(1);
        assert_clean(h, what);
    }
}

fn harness_demo(width: f32, lang: crate::lang::Lang, theme: crate::theme::ThemeChoice) -> Harness<'static, PlanCraftApp> {
    crate::lang::set_current(lang);
    let mut h = super::harness_at(width);
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = lang;
    let ctx = h.ctx.clone();
    h.state_mut().exec(&ctx, Command::SetTheme(theme));
    settle(&mut h, 3, "apply demo harness language/theme");
    h
}

fn harness_with_workspace(
    ws: Workspace,
    width: f32,
    lang: crate::lang::Lang,
    theme: crate::theme::ThemeChoice,
) -> Harness<'static, PlanCraftApp> {
    crate::lang::set_current(lang);
    let mut h = super::harness_with(ws);
    h.set_size(vec2(width, 900.0));
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = lang;
    let ctx = h.ctx.clone();
    h.state_mut().exec(&ctx, Command::SetTheme(theme));
    settle(&mut h, 3, "apply workspace harness language/theme");
    h
}

fn assert_only_print_dialog_open(app: &PlanCraftApp) {
    let d = &app.dialogs;
    assert!(d.print.is_some(), "print dialog must be open");
    assert!(d.task_info.is_none());
    assert!(d.find.is_none());
    assert!(d.goto.is_none());
    assert!(d.resource_info.is_none());
    assert!(d.project_info.is_none());
    assert!(d.working_time.is_none());
    assert!(d.custom_fields.is_none());
    assert!(d.update_project.is_none());
    assert!(d.set_baseline.is_none());
    assert!(d.clear_baseline.is_none());
    assert!(d.recurring.is_none());
    assert!(d.assign.is_none());
    assert!(d.resource_delete.is_none());
    assert!(d.external.is_none());
    assert!(d.import.is_empty());
    assert!(!d.stats);
    assert!(!d.about);
    assert!(!d.history);
    assert!(!d.mcp);
    assert!(d.distill.is_none());
    assert!(d.planner_export.is_none());
    assert!(d.quick.is_none());
    assert!(!d.ai_open);
}

// Several data views paint their table/canvas directly and do not expose individual
// resource/column names as accessibility labels. Check their real selector and data
// prerequisites here; the host additionally inspects the rendered PNG contents.
fn assert_document_view(h: &Harness<'static, PlanCraftApp>, view: ViewKind) {
    assert_eq!(h.state().st.view, view);
    assert!(h.query_all_by_label(view.label()).any(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox));
    let ws = lock(&h.state().ws);
    let project = &ws.active_doc().unwrap().project;
    assert!(!project.tasks.is_empty());
    if matches!(view, ViewKind::ResourceUsage | ViewKind::TeamPlanner | ViewKind::Workload) {
        assert!(!project.resources.is_empty());
        assert!(!project.assignments.is_empty());
    }
}

fn capture(
    entries: &mut Vec<ManifestEntry>,
    dir: &Path,
    started_before: SystemTime,
    filename: &str,
    view_state: &str,
    language: crate::lang::Lang,
    theme: crate::theme::ThemeChoice,
    h: &mut Harness<'static, PlanCraftApp>,
) {
    settle(h, 4, filename);
    let image = h.render().expect("render screenshot frame");
    let path = dir.join(filename);
    image.save(&path).expect("save screenshot PNG");
    let modified = fs::metadata(&path).expect("read saved PNG metadata").modified().expect("read PNG mtime");
    assert!(modified >= started_before - Duration::from_secs(2), "{filename}: file mtime must be fresh");
    entries.push(ManifestEntry {
        filename: filename.to_string(),
        view_state: view_state.to_string(),
        language: match language {
            crate::lang::Lang::En => "en".to_string(),
            crate::lang::Lang::Nl => "nl".to_string(),
        },
        theme: match theme {
            crate::theme::ThemeChoice::Dark => "dark".to_string(),
            crate::theme::ThemeChoice::Light => "light".to_string(),
            crate::theme::ThemeChoice::System => "system".to_string(),
        },
        size: [image.width(), image.height()],
    });
}

fn make_auto_duration_project() -> (Project, TaskId) {
    let mut p = Project::new("Auto Duration", plancraft_core::dates::ymd(2026, 12, 7).unwrap());
    let day: Minutes = 480;

    let _group = p.add_task("Summary", None);
    let milestone = p.add_task("Milestone", None);
    let preparation = p.add_task("Preparation", None);
    let send = p.add_task("Send", None);
    let review = p.add_milestone("Review", None);
    for id in [milestone, preparation, send, review] {
        let t = p.task_mut(id).unwrap();
        t.outline_level = 2;
        t.fills_group = false;
    }

    {
        let t = p.task_mut(milestone).unwrap();
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t.constraint = Constraint {
            kind: ConstraintKind::Mso,
            date: Some(plancraft_core::dates::ymd(2026, 12, 8).unwrap().and_hms_opt(17, 0, 0).unwrap()),
        };
    }
    {
        let t = p.task_mut(preparation).unwrap();
        t.duration = 5 * day;
        t.estimated = false;
    }
    {
        let t = p.task_mut(send).unwrap();
        t.duration = day;
        t.estimated = false;
        t.bounds.finish_latest = Some(plancraft_core::dates::ymd(2026, 12, 11).unwrap().and_hms_opt(17, 0, 0).unwrap());
        t.bounds.finish_latest_fixed = true;
    }

    p.link(milestone.into(), preparation, LinkType::FS, Lag::None).unwrap();
    p.link(preparation.into(), send, LinkType::FS, Lag::None).unwrap();
    p.task_mut(review).unwrap().constraint = Constraint {
        kind: ConstraintKind::Mso,
        date: Some(plancraft_core::dates::ymd(2026, 12, 16).unwrap().and_hms_opt(17, 0, 0).unwrap()),
    };
    p.link(send.into(), review, LinkType::FS, Lag::None).unwrap();
    p.set_duration_mode(preparation, DurationMode::Auto).unwrap();
    plancraft_core::schedule::recalculate(&mut p);

    let prep = p.task(preparation).unwrap();
    assert_eq!(prep.duration, 5 * day, "stored duration must stay old value");
    assert_eq!(prep.duration_mode, DurationMode::Auto, "demo uses Auto duration");
    assert!(prep.duration_percent.is_none(), "percent stays clear under Auto");
    let shown = p.format_task_duration(prep);
    assert_eq!(shown, p.format_duration(2 * day, false), "calculated duration must be 2 days");
    assert!(p.tasks.iter().all(|task| task.sched.warnings.is_empty()), "the Auto-duration demo must have no task constraint violations");
    // The engine may retain its informational notice that it shortened a shared span
    // to respect a successor date. Keep that real notice visible in the capture.

    (p, preparation)
}

#[test]
#[ignore = "renders on the GPU; host runs this explicitly to capture docs screenshots"]
fn docs_screenshot_images() {
    let started_before = SystemTime::now();
    let dir = out_dir();
    fs::create_dir_all(&dir).expect("create docs screenshot output directory");

    let mut entries = Vec::new();

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        let task_id = super::leaf(&h, 0);
        assert!(!lock(&h.state().ws).active_doc().unwrap().project.task(task_id).unwrap().sched.is_summary);
        h.state_mut().st.sel.select_task(task_id);
        h.state_mut().exec(&ctx, Command::TaskInfo);
        settle(&mut h, 3, "task info open");
        assert!(h.state().dialogs.task_info.is_some(), "Task Information dialog must be open");
        assert!(
            h.query_all_by_label("Task Information").any(|n| n.accesskit_node().role() == egui::accesskit::Role::Window),
            "real Task Information window must be rendered"
        );
        capture(
            &mut entries,
            &dir,
            started_before,
            "task-information.png",
            "Task Information dialog for selected leaf task",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::NetworkDiagram));
        h.state_mut().st.menu_tab = crate::state::MenuTab::Format;
        settle(&mut h, 3, "network view");
        h.get_by_label("Critical First");
        capture(
            &mut entries,
            &dir,
            started_before,
            "network.png",
            "Network Diagram view with Format controls",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::ResourceUsage));
        settle(&mut h, 3, "resource usage view");
        assert_document_view(&h, ViewKind::ResourceUsage);
        capture(
            &mut entries,
            &dir,
            started_before,
            "resource-usage.png",
            "Resource Usage view",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Board));
        settle(&mut h, 3, "board view");
        assert_document_view(&h, ViewKind::Board);
        capture(
            &mut entries,
            &dir,
            started_before,
            "board.png",
            "Task Board view",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Light);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Gantt));
        h.state_mut().st.menu_tab = crate::state::MenuTab::View;
        settle(&mut h, 3, "gantt light view");
        h.get_by_label("Q");
        capture(
            &mut entries,
            &dir,
            started_before,
            "gantt.png",
            "Gantt Chart view in light theme",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Light,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Calendar));
        settle(&mut h, 3, "calendar view");
        h.get_by_label("Previous month");
        h.get_by_label("Next month");
        capture(
            &mut entries,
            &dir,
            started_before,
            "calendar.png",
            "Calendar view",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_with_workspace(Workspace::default(), 1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        h.state_mut().st.start_page = true;
        settle(&mut h, 3, "start page");
        h.get_by_label("Open…");
        capture(
            &mut entries,
            &dir,
            started_before,
            "start-page.png",
            "Start Page with empty workspace",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::TrackingGantt));
        h.state_mut().st.menu_tab = crate::state::MenuTab::View;
        settle(&mut h, 3, "tracking gantt view");
        h.get_by_label("Q");
        capture(
            &mut entries,
            &dir,
            started_before,
            "tracking.png",
            "Tracking Gantt view",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Gantt));
        h.state_mut().exec(&ctx, Command::Print);
        settle(&mut h, 3, "print preview dialog");
        assert_only_print_dialog_open(h.state());
        assert!(
            h.query_all_by_label("Print").any(|n| n.accesskit_node().role() == egui::accesskit::Role::Window),
            "real Print preview window must be rendered"
        );
        capture(
            &mut entries,
            &dir,
            started_before,
            "print-preview.png",
            "Print preview dialog only",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::Nl, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Gantt));
        settle(&mut h, 3, "language screenshot");
        assert!(
            h.query_all_by_label("Gantt-diagram").any(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox),
            "Dutch localized view selector must be visible"
        );
        assert!(h.query_all_by_label("Taak").any(|n| n.accesskit_node().role() == egui::accesskit::Role::Button));
        capture(
            &mut entries,
            &dir,
            started_before,
            "languages.png",
            "Dutch UI in Gantt view",
            crate::lang::Lang::Nl,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Milestones));
        settle(&mut h, 3, "milestones view");
        h.get_by_label("Plan approved");
        capture(
            &mut entries,
            &dir,
            started_before,
            "milestones.png",
            "Milestones view",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Gantt));
        h.state_mut().st.menu_tab = crate::state::MenuTab::View;
        settle(&mut h, 3, "gantt dark view");
        h.get_by_label("Q");
        capture(
            &mut entries,
            &dir,
            started_before,
            "gantt-dark.png",
            "Gantt Chart view in dark theme",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::TeamPlanner));
        settle(&mut h, 3, "team planner view");
        assert_document_view(&h, ViewKind::TeamPlanner);
        capture(
            &mut entries,
            &dir,
            started_before,
            "team-planner.png",
            "Team Planner view",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Workload));
        settle(&mut h, 3, "workload view");
        assert_document_view(&h, ViewKind::Workload);
        capture(
            &mut entries,
            &dir,
            started_before,
            "workload.png",
            "Workload view",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Overview));
        settle(&mut h, 3, "overview dark");
        assert!(
            h.query_all_by_label("Overview").any(|n| n.accesskit_node().role() == egui::accesskit::Role::Label),
            "Overview heading label must be present"
        );
        h.get_by_label("Open Gantt");
        capture(
            &mut entries,
            &dir,
            started_before,
            "overview.png",
            "Overview view in dark theme",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Light);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Overview));
        settle(&mut h, 3, "overview light");
        h.get_by_label("Open Gantt");
        capture(
            &mut entries,
            &dir,
            started_before,
            "overview-light.png",
            "Overview view in light theme",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Light,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(960.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Overview));
        settle(&mut h, 3, "overview narrow");
        h.get_by_label("Open Gantt");
        capture(
            &mut entries,
            &dir,
            started_before,
            "overview-narrow.png",
            "Overview view at 960x900",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let (p, prep_id) = make_auto_duration_project();
        let mut ws = Workspace::default();
        ws.add(p, None);
        let mut h = harness_with_workspace(ws, 1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let two_days = {
            let ws = lock(&h.state().ws);
            let p = &ws.active_doc().unwrap().project;
            p.format_duration(2 * 480, false)
        };
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.properties_tab = crate::state::PropTab::General;
        h.state_mut().st.sel.select_task(prep_id);
        settle(&mut h, 4, "duration auto properties");
        assert!(
            h.query_all_by_value(&two_days)
                .any(|n| { n.accesskit_node().role() == egui::accesskit::Role::TextInput && n.accesskit_node().is_disabled() }),
            "rendered tree must show the calculated 2-day duration"
        );
        let auto_mode = h.get_by_value(crate::lang::text("prop.duration_auto"));
        assert_eq!(auto_mode.accesskit_node().role(), egui::accesskit::Role::ComboBox);
        capture(
            &mut entries,
            &dir,
            started_before,
            "duration-auto.png",
            "Properties General showing auto duration = 2 days, stored duration preserved",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let id = {
            let ws = lock(&h.state().ws);
            let p = &ws.active_doc().unwrap().project;
            p.tasks.iter().find(|t| t.name == "Market research").unwrap().id
        };
        {
            let ws = lock(&h.state().ws);
            let p = &ws.active_doc().unwrap().project;
            let units = p.assignments_for_task(id).next().map(|a| a.units).unwrap();
            assert!(units != 1.0, "screenshot task must show non-100% allocation");
        }
        h.state_mut().st.properties_visible = true;
        h.state_mut().st.sel.select_task(id);
        h.state_mut().st.properties_tab = crate::state::PropTab::General;
        settle(&mut h, 3, "properties general resources check");
        assert!(h.query_all_by_label(crate::lang::text("prop.units")).next().is_none(), "General tab must not expose allocation inputs");
        h.state_mut().st.properties_tab = crate::state::PropTab::Resources;
        settle(&mut h, 3, "properties resources tab");
        h.get_by_label(crate::lang::text("prop.units"));
        assert!(
            h.query_all_by_value("50%")
                .any(|n| { n.accesskit_node().role() == egui::accesskit::Role::TextInput && !n.accesskit_node().is_disabled() }),
            "Resources tab must retain the editable allocation field"
        );
        capture(
            &mut entries,
            &dir,
            started_before,
            "properties-resources.png",
            "Properties Resources tab with non-100% units",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        let ctx = h.ctx.clone();
        h.state_mut().exec(&ctx, Command::SetView(ViewKind::Gantt));
        h.state_mut().st.menu_tab = crate::state::MenuTab::Format;
        h.state_mut().st.gantt.show_critical = true;
        h.state_mut().st.gantt.show_paths = true;
        let selected = super::leaf(&h, 0);
        h.state_mut().st.sel.select_task(selected);
        settle(&mut h, 3, "format toggles screenshot");
        for label in ["Critical Path", "Highlight Paths"] {
            assert!(
                h.query_all_by_label(label).any(|n| {
                    n.accesskit_node().role() == egui::accesskit::Role::Button
                        && n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
                }),
                "{label} must expose selected state"
            );
        }
        capture(
            &mut entries,
            &dir,
            started_before,
            "format-toggles.png",
            "Format toolbar with Critical Path and Highlight Paths selected",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    {
        let mut h = harness_demo(1440.0, crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
        h.state_mut().st.view = ViewKind::Gantt;
        // Keep the selected fixture task visible until the selection-focus action runs.
        // The focus itself is the filter illustrated in this capture.
        h.state_mut().st.status_filter = None;
        let show = crate::lang::text("filter.show_selection");
        let clear = crate::lang::text("filter.clear_activity");
        let id = super::leaf(&h, 0);
        h.state_mut().st.sel.select_task(id);
        settle(&mut h, 3, "selection filter pre-click");
        h.get_by_label(show).click();
        settle(&mut h, 3, "selection filter focused");
        assert_eq!(h.state().st.focus.map(|f| f.task), Some(id));
        assert_eq!(h.query_all_by_label(clear).count(), 1, "focused state must show exactly one Clear filter button");
        assert!(h.query_all_by_label(show).next().is_none(), "Show selection must be absent while focused");
        assert!(
            h.query_all_by_label(crate::lang::text("filter.reset")).next().is_none(),
            "one contextual clear action, not a second Reset filters"
        );
        capture(
            &mut entries,
            &dir,
            started_before,
            "selection-filter.png",
            "Focused selection filter with a single Clear filter button",
            crate::lang::Lang::En,
            crate::theme::ThemeChoice::Dark,
            &mut h,
        );
    }

    let expected = [
        "task-information.png",
        "network.png",
        "resource-usage.png",
        "board.png",
        "gantt.png",
        "calendar.png",
        "start-page.png",
        "tracking.png",
        "print-preview.png",
        "languages.png",
        "milestones.png",
        "gantt-dark.png",
        "team-planner.png",
        "workload.png",
        "overview.png",
        "overview-light.png",
        "overview-narrow.png",
        "duration-auto.png",
        "properties-resources.png",
        "format-toggles.png",
        "selection-filter.png",
    ];
    let expected_set: BTreeSet<_> = expected.into_iter().map(str::to_string).collect();
    let got_set: BTreeSet<_> = entries.iter().map(|e| e.filename.clone()).collect();
    assert_eq!(entries.len(), 21, "manifest must contain exactly 21 entries");
    assert_eq!(got_set.len(), 21, "filenames must be unique");
    assert_eq!(got_set, expected_set, "captured files must match required list exactly");

    let manifest_path = dir.join("manifest.json");
    let json = serde_json::to_string_pretty(&entries).expect("serialize screenshot manifest");
    fs::write(&manifest_path, json).expect("write screenshot manifest");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let parsed: Vec<ManifestEntry> = serde_json::from_str(&manifest_text).expect("parse manifest");
    assert_eq!(parsed.len(), 21, "manifest must record all captures");
}
