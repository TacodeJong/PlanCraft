//! The title band and the two-row command bar.
//!
//! Layout:
//!
//! ```text
//! ┌──────────┬───────────────────────────────────────────────────────────┐
//! │  view    │ File v  Task  Resource  Project  Report  View  Format      │  tabs
//! │ selector ├───────────────────────────────────────────────────────────┤
//! │    v     │ [+ Task] [Milestone] │ in/outdent │ [Link] … single toolbar │  toolbar
//! └──────────┴───────────────────────────────────────────────────────────┘
//! ```
//!
//! The view selector spans both rows. Tabs are plain tabs, and the toolbar is
//! one row of compact inline buttons separated by thin dividers.

use plancraft_core::baseline::PlanSlot;
use std::cell::RefCell;

use egui::{Align, Align2, FontId, Layout, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use plancraft_core::model::ScheduleMode;

use crate::columns::TaskTable;
use crate::commands::Command;
use crate::logo;
use crate::settings::AppSettings;
use crate::state::{MenuTab, TaskFilter, UiState, ViewKind};
use crate::theme;

pub struct MenuInfo {
    pub has_doc: bool,
    pub title: String,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub task_selected: bool,
    pub resource_selected: bool,
    pub mcp_running: bool,
    pub mcp_url: Option<String>,
    /// Whether the Undo History window is open (the History button shows as pressed).
    pub history_open: bool,
    /// Fields offered for sorting and grouping: (key, title), the table's columns first.
    pub fields: Vec<(String, String)>,
    /// The Columns menu: (key, title, on, required). Only the name column is required.
    pub columns: Vec<(String, String, bool, bool)>,
}

const TAB_ROW_H: f32 = 28.0;
const TOOL_ROW_H: f32 = 34.0;
const SELECTOR_W: f32 = 104.0;
const BUTTON_H: f32 = 26.0;

// ---------------------------------------------------------------------------
// Title band
// ---------------------------------------------------------------------------

/// The deep-blue band at the top: logo, quick access buttons, document title, MCP status.
pub fn title_bar(ui: &mut Ui, info: &MenuInfo, out: &mut Vec<Command>) {
    let rect = ui.available_rect_before_wrap();
    let rect = Rect::from_min_size(rect.min, vec2(rect.width(), 34.0));
    ui.painter().rect_filled(rect, 0.0, theme::pal().TITLE_BAR);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
    let ui2 = &mut child;
    logo::paint(ui2.painter(), Rect::from_center_size(pos2(ui2.cursor().left() + 11.0, rect.center().y), vec2(22.0, 22.0)));
    ui2.add_space(28.0);
    ui2.label(egui::RichText::new("PlanCraft").color(theme::pal().TEXT).size(14.0).strong());
    ui2.add_space(12.0);
    let qa = |ui: &mut Ui, icon: &str, tip: &str, enabled: bool| -> bool {
        let (r, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && resp.hovered() {
            ui.painter().rect_filled(r, 3.0, theme::pal().WASH_HOVER);
        }
        let color = if enabled { theme::pal().TEXT } else { theme::pal().LINE_STRONG };
        crate::icons::paint(ui, r, icon, 14.0, color);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
        resp.on_hover_text(tip).clicked()
    };
    if qa(ui2, "save", "Save (Ctrl+S)", info.has_doc) {
        out.push(Command::Save);
    }
    let undo_tip = match &info.undo_label {
        Some(l) => format!("Undo {l} (Ctrl+Z)"),
        None => "Undo (Ctrl+Z)".into(),
    };
    if qa(ui2, "undo-2", &undo_tip, info.can_undo) {
        out.push(Command::Undo);
    }
    if qa(ui2, "redo-2", "Redo (Ctrl+Y)", info.can_redo) {
        out.push(Command::Redo);
    }
    let title = if info.has_doc { format!("{} - PlanCraft", info.title) } else { "PlanCraft".into() };
    ui2.painter().text(rect.center(), Align2::CENTER_CENTER, title, FontId::proportional(13.0), theme::pal().TEXT_WEAK);
    ui2.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let (icon, txt, color) =
            if info.mcp_running { ("zap", "MCP live", theme::pal().MCP_LIVE) } else { ("circle", "MCP off", theme::pal().MCP_OFF) };
        let resp = ui.add(
            egui::Button::image_and_text(crate::icons::image(icon, 12.0, color), egui::RichText::new(txt).color(color).size(12.0))
                .frame(false),
        );
        let tip = match &info.mcp_url {
            Some(u) => format!("AI agents can control this window via MCP at {u}\nClick to manage."),
            None => "Start the built-in MCP server so AI agents (e.g. Claude Code) can work with your projects.".into(),
        };
        if resp.on_hover_text(tip).clicked() {
            out.push(Command::McpPanel);
        }
    });
    ui.allocate_rect(rect, Sense::hover());
}

// ---------------------------------------------------------------------------
// Toolbar widgets
// ---------------------------------------------------------------------------

/// Inline toolbar button: icon, optional label, tooltip.
fn button(ui: &mut Ui, icon: &str, label: Option<&str>, tip: &str, enabled: bool, active: bool) -> egui::Response {
    let font = FontId::proportional(12.5);
    let label_w = label.map(|l| ui.painter().layout_no_wrap(l.to_string(), font.clone(), theme::pal().TEXT).size().x + 6.0).unwrap_or(0.0);
    let w = 26.0 + label_w + if label.is_some() { 4.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, BUTTON_H), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    // Ghost buttons: transparent at rest; hover is a wash with the border raised — never the accent.
    if active {
        p.rect_filled(rect, 3.0, theme::pal().WASH_SELECTED);
        p.rect_stroke(rect, 3.0, Stroke::new(1.0, theme::pal().LINE_STRONG), StrokeKind::Inside);
    } else if enabled && resp.hovered() {
        p.rect_filled(rect, 3.0, theme::pal().WASH_HOVER);
        p.rect_stroke(rect, 3.0, Stroke::new(1.0, theme::pal().LINE), StrokeKind::Inside);
    }
    let icon_color = if !enabled {
        theme::pal().LINE_STRONG
    } else if active || resp.hovered() {
        theme::pal().TEXT
    } else {
        theme::pal().TEXT_WEAK
    };
    let center = pos2(rect.left() + 13.0, rect.center().y);
    crate::icons::paint(ui, Rect::from_center_size(center, vec2(14.0, 14.0)), icon, 14.0, icon_color);
    if let Some(l) = label {
        p.text(
            pos2(rect.left() + 26.0, rect.center().y),
            Align2::LEFT_CENTER,
            l,
            font,
            if enabled { theme::pal().TEXT } else { theme::pal().TEXT_WEAK },
        );
    }
    let name = label.map_or_else(|| tip.to_string(), |l| l.to_string());
    resp.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Thin vertical divider between groups of related buttons.
fn sep(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(11.0, BUTTON_H), Sense::hover());
    ui.painter().vline(rect.center().x, (rect.top() + 4.0)..=(rect.bottom() - 4.0), theme::hairline());
}

/// A mini pie button for "x% complete".
fn pie(ui: &mut Ui, pct: u8, enabled: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, BUTTON_H), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(r, 3.0, theme::pal().WASH_HOVER);
    }
    let color = if !enabled {
        theme::pal().LINE_STRONG
    } else if resp.hovered() {
        theme::pal().TEXT
    } else {
        theme::pal().TEXT_WEAK
    };
    let c = r.center();
    ui.painter().circle_stroke(c, 6.5, Stroke::new(1.0, color));
    if pct >= 100 {
        ui.painter().circle_filled(c, 6.5, color);
    } else if pct > 0 {
        let n = 16;
        let mut pts = vec![c];
        for i in 0..=n {
            let a = -std::f32::consts::FRAC_PI_2 + (pct as f32 / 100.0) * std::f32::consts::TAU * i as f32 / n as f32;
            pts.push(c + vec2(a.cos(), a.sin()) * 6.5);
        }
        ui.painter().add(Shape::convex_polygon(pts, color, Stroke::NONE));
    }
    resp.on_hover_text(format!("Mark {pct}% complete"))
}

fn combo_label(ui: &mut Ui, text: &str) {
    crate::widgets::craft::hud(ui, text, theme::pal().TEXT_WEAK);
}

// ---------------------------------------------------------------------------
// View selector (spans both rows)
// ---------------------------------------------------------------------------

fn view_selector(ui: &mut Ui, rect: Rect, st: &UiState, enabled: bool, push: &dyn Fn(Command)) {
    let resp = ui.interact(rect, egui::Id::new("view_selector"), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    p.rect_filled(rect, 0.0, theme::pal().PANEL_BG);
    if resp.hovered() && enabled {
        p.rect_filled(rect, 0.0, theme::pal().WASH);
    }
    p.vline(rect.right() - 0.5, rect.y_range(), theme::hairline());
    let color = if enabled { theme::pal().TEXT } else { theme::pal().LINE_STRONG };
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 18.0), vec2(20.0, 20.0));
    crate::icons::paint(ui, icon_rect, st.view.icon(), 18.0, color);
    let galley = p.layout(st.view.label().to_string(), FontId::proportional(12.0), theme::pal().TEXT, rect.width() - 12.0);
    let gh = galley.size().y;
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 32.0), galley, theme::pal().TEXT);
    // Dropdown chevron.
    let cy = rect.top() + 32.0 + gh + 6.0;
    let c = pos2(rect.center().x, cy.min(rect.bottom() - 5.0));
    p.add(Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 2.5)], theme::pal().TEXT_WEAK, Stroke::NONE));
    let resp = resp.on_hover_text("Change view");
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(190.0);
        ui.label(egui::RichText::new(crate::lang::text("view.task_views")).small().color(theme::pal().TEXT_WEAK));
        for v in ViewKind::ALL {
            if v == ViewKind::ResourceSheet {
                ui.separator();
                ui.label(egui::RichText::new(crate::lang::text("view.resource_views")).small().color(theme::pal().TEXT_WEAK));
            }
            let img = crate::icons::image(v.icon(), 14.0, theme::pal().TEXT);
            if ui.add(egui::Button::image_and_text(img, v.label()).selected(st.view == v)).clicked() {
                push(Command::SetView(v));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

fn tab_strip(ui: &mut Ui, rect: Rect, st: &mut UiState, info: &MenuInfo, _settings: &AppSettings, push: &dyn Fn(Command)) {
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, theme::pal().PANEL_BG);
    p.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    let mut x = rect.left() + 6.0;
    let tab_rect = |x: f32, w: f32| Rect::from_min_max(pos2(x, rect.top() + 2.0), pos2(x + w, rect.bottom() - 2.0));
    // Mono-caps HUD tabs; the active one is an inverted chip (the breadcrumb-leaf treatment).
    let caps = |text: &str, color| crate::widgets::craft::caps(&p, text, theme::HUD_SIZE, theme::mono(), theme::HUD_SPACING, color);

    // File lives in the shared application menu (native on macOS, in-window row elsewhere).
    let _ = info;

    for tab in MenuTab::ALL {
        let w = caps(tab.label(), theme::pal().TEXT).size().x + 22.0;
        let r = tab_rect(x, w);
        let resp = ui.interact(r, egui::Id::new(("tab", tab.label())), Sense::click());
        let active = st.menu_tab == tab;
        let color = if active {
            // A square ink block with page-colored mono caps.
            p.rect_filled(r, 0.0, theme::pal().INVERT_BG);
            theme::pal().INVERT_FG
        } else if resp.hovered() {
            p.rect_filled(r, 3.0, theme::pal().WASH_HOVER);
            theme::pal().TEXT
        } else {
            theme::pal().TEXT_WEAK
        };
        let g = caps(tab.label(), color);
        p.galley(pos2(r.center().x - g.size().x / 2.0 + theme::HUD_SPACING / 2.0, r.center().y - g.size().y / 2.0), g, color);
        if resp.clicked() {
            st.menu_tab = tab;
        }
        x += w + 4.0;
    }

    // Right side: help/about.
    let r = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(22.0, 20.0));
    let resp = ui.interact(r, egui::Id::new("tab_help"), Sense::click());
    if resp.hovered() {
        p.rect_filled(r, 3.0, theme::pal().WASH_HOVER);
    }
    p.text(r.center(), Align2::CENTER_CENTER, "?", FontId::proportional(13.0), theme::pal().TEXT_WEAK);
    if resp.on_hover_text("About PlanCraft").clicked() {
        push(Command::About);
    }
}

// ---------------------------------------------------------------------------
// Toolbar contents per tab
// ---------------------------------------------------------------------------

/// The Q / M / D chips: timescale presets for the chart zoom and the usage buckets.
/// The active chip follows what the current view actually shows (free zoom can sit
/// between presets); views without a time axis keep the chips, disabled, with a tooltip.
fn timescale_chips(ui: &mut Ui, st: &UiState, info: &MenuInfo, push: &dyn Fn(Command)) {
    use crate::state::Timescale;
    let active = match st.view {
        ViewKind::Gantt | ViewKind::TrackingGantt | ViewKind::MultipleBaselines | ViewKind::TeamPlanner => {
            Timescale::of_px(st.gantt.px_per_day)
        }
        ViewKind::TaskUsage | ViewKind::ResourceUsage | ViewKind::Workload => Timescale::of_granularity(st.granularity),
        _ => None,
    };
    let time_view = matches!(
        st.view,
        ViewKind::Gantt
            | ViewKind::TrackingGantt
            | ViewKind::MultipleBaselines
            | ViewKind::TeamPlanner
            | ViewKind::TaskUsage
            | ViewKind::ResourceUsage
            | ViewKind::Workload
    );
    let enabled = info.has_doc && time_view;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for ts in Timescale::ALL {
            let on = enabled && active == Some(ts);
            let color = if on { theme::pal().BG } else { theme::pal().TEXT };
            let mut chip = egui::Button::new(crate::widgets::craft::hud_job(ts.letter(), color)).min_size(vec2(24.0, 22.0));
            if on {
                chip = chip.fill(theme::pal().TEXT);
            }
            let r = ui
                .add_enabled(enabled, chip)
                .on_hover_text(crate::lang::text(ts.name_key()))
                .on_disabled_hover_text(crate::lang::text("ts.no_axis"));
            if r.clicked() {
                push(Command::SetTimescale(ts));
            }
        }
    });
}

fn toolbar(ui: &mut Ui, st: &mut UiState, info: &MenuInfo, settings: &AppSettings, push: &dyn Fn(Command)) {
    let doc = info.has_doc;
    let sel = info.task_selected && doc;
    let res = info.resource_selected && doc;
    let b = |ui: &mut Ui, icon: &str, label: Option<&str>, tip: &str, enabled: bool, cmd: Command| {
        if button(ui, icon, label, tip, enabled, false).clicked() {
            push(cmd);
        }
    };
    match st.menu_tab {
        MenuTab::Task => {
            b(ui, "plus", Some(crate::lang::text("tb.task")), "Insert a task above the selection (Insert)", doc, Command::InsertTask);
            b(ui, "diamond", Some(crate::lang::text("tb.milestone")), "Insert a milestone", doc, Command::InsertMilestone);
            b(
                ui,
                "rows-3",
                Some(crate::lang::text("tb.summary")),
                "Group the selected tasks under a new summary task",
                doc,
                Command::InsertSummary,
            );
            b(
                ui,
                "repeat",
                Some(crate::lang::text("tb.recurring")),
                "Insert a task that repeats daily, weekly, monthly or yearly",
                doc,
                Command::RecurringTask,
            );
            b(ui, "trash-2", None, "Delete the selected tasks (Delete)", sel, Command::DeleteSelection);
            sep(ui);
            b(ui, "indent-decrease", None, "Outdent (Alt+Shift+Left)", sel, Command::Outdent);
            b(ui, "indent-increase", None, "Indent (Alt+Shift+Right)", sel, Command::Indent);
            b(ui, "arrow-up", None, "Move up (Alt+Shift+Up)", sel, Command::MoveUp);
            b(ui, "arrow-down", None, "Move down (Alt+Shift+Down)", sel, Command::MoveDown);
            sep(ui);
            b(
                ui,
                "link",
                Some(crate::lang::text("tb.link")),
                "Link the selected tasks finish-to-start (Ctrl+F2)",
                sel,
                Command::LinkSelected,
            );
            b(
                ui,
                "unlink",
                Some(crate::lang::text("tb.unlink")),
                "Remove links between the selected tasks (Ctrl+Shift+F2)",
                sel,
                Command::UnlinkSelected,
            );
            if button(
                ui,
                "split",
                Some(crate::lang::text("tb.split")),
                "Split Task: click a bar where its work should pause; drag a part to move it, or back to rejoin",
                doc,
                st.gantt.split_mode,
            )
            .clicked()
            {
                push(Command::SplitTask);
            }
            sep(ui);
            for pct in [0u8, 25, 50, 75, 100] {
                if pie(ui, pct, sel).clicked() {
                    push(Command::SetPercent(pct));
                }
            }
            b(
                ui,
                "check",
                Some(crate::lang::text("tb.on_track")),
                "Set progress to where the schedule says it should be at the status date (today if none is set)",
                sel,
                Command::MarkOnTrack,
            );
            sep(ui);
            b(ui, "rotate-cw", None, "Auto schedule the selected tasks", sel, Command::SetMode(ScheduleMode::Auto));
            b(ui, "pin", None, "Manually schedule the selected tasks", sel, Command::SetMode(ScheduleMode::Manual));
            b(ui, "pause", None, "Inactivate / activate", sel, Command::ToggleActive);
            sep(ui);
            b(ui, "info", Some(crate::lang::text("tb.information")), "Task information (Shift+F2)", sel, Command::TaskInfo);
            b(ui, "pen-line", None, "Task notes", sel, Command::TaskNotes);
            b(ui, "calendar", None, "Show on the timeline", sel, Command::ToggleTimeline);
            b(ui, "locate", None, "Scroll the chart to the task", sel, Command::ScrollToTask);
            sep(ui);
            b(ui, "scissors", None, "Cut (Ctrl+X)", sel, Command::Cut);
            b(ui, "copy", None, "Copy (Ctrl+C)", sel, Command::Copy);
            b(ui, "clipboard-paste", None, "Paste (Ctrl+V)", doc && !st.clipboard.is_empty(), Command::Paste);
        }
        MenuTab::Resource => {
            b(
                ui,
                "users-round",
                Some(crate::lang::text("tb.assign")),
                "Assign resources to the selected tasks",
                sel,
                Command::AssignResources,
            );
            sep(ui);
            b(ui, "plus", Some(crate::lang::text("tb.add_resource")), "Add a resource", doc, Command::InsertResource);
            b(
                ui,
                "info",
                Some(crate::lang::text("tb.information")),
                "Resource information, time off and availability",
                res,
                Command::ResourceInfo,
            );
            b(
                ui,
                "trash-2",
                Some(crate::lang::text("tb.delete_resource")),
                "Delete the selected resources (replace assignments when they are used)",
                res,
                Command::DeleteResources,
            );
            sep(ui);
            b(ui, "scale", Some(crate::lang::text("tb.level")), "Delay tasks to resolve resource overallocations", doc, Command::LevelAll);
            b(ui, "rotate-ccw", Some(crate::lang::text("tb.clear_level")), "Remove all leveling delays", doc, Command::ClearLeveling);
            sep(ui);
            b(
                ui,
                "timer",
                Some(crate::lang::text("tb.timesheet")),
                "Log hours per resource and task",
                doc,
                Command::SetView(ViewKind::Timesheet),
            );
            b(
                ui,
                "rotate-cw",
                Some(crate::lang::text("tb.update_progress")),
                "Set % complete from tracked time",
                doc,
                Command::UpdateProgressFromTime,
            );
        }
        MenuTab::Project => {
            b(
                ui,
                "archive",
                Some(crate::lang::text("tb.project_info")),
                "Name, dates, calendar and scheduling options",
                doc,
                Command::ProjectInfo,
            );
            b(
                ui,
                "calendar-days",
                Some(crate::lang::text("tb.working_time")),
                "Calendars, work weeks and holidays",
                doc,
                Command::ChangeWorkingTime,
            );
            sep(ui);
            b(ui, "link", Some(crate::lang::text("tb.links")), "Depend on tasks in other project files", doc, Command::ExternalLinks);
            b(ui, "rotate-cw", None, "Refresh links to other projects", doc, Command::RefreshExternal);
            sep(ui);
            b(
                ui,
                "tag",
                Some(crate::lang::text("tb.custom")),
                "Define your own task fields: text with a pick-list, number, cost, date, duration, flag",
                doc,
                Command::CustomFields,
            );
            sep(ui);
            b(
                ui,
                "skip-forward",
                Some(crate::lang::text("tb.update")),
                "Update progress through a date, or reschedule work not done yet to start after it",
                doc,
                Command::UpdateProject,
            );
            sep(ui);
            b(
                ui,
                "ruler",
                Some(crate::lang::text("tb.baseline")),
                "Save the schedule as Baseline or Baseline1–10, or set an interim plan",
                doc,
                Command::SetBaseline,
            );
            b(ui, "x", None, "Clear a baseline or an interim plan", doc, Command::ClearBaseline);
            sep(ui);
            if button(ui, "zap", Some(crate::lang::text("tb.agents")), "Built-in MCP server for AI agents", true, info.mcp_running)
                .clicked()
            {
                push(Command::McpPanel);
            }
        }
        MenuTab::Report => {
            b(
                ui,
                "chart-column",
                Some(crate::lang::text("tb.statistics")),
                "Project statistics, variance and earned value",
                doc,
                Command::Statistics,
            );
            b(
                ui,
                "diamond",
                Some(crate::lang::text("tb.milestones")),
                "Milestone status report",
                doc,
                Command::SetView(ViewKind::Milestones),
            );
            b(
                ui,
                "signal",
                Some(crate::lang::text("tb.workload")),
                "Resource utilization and resource graph",
                doc,
                Command::SetView(ViewKind::Workload),
            );
            b(
                ui,
                "chart-line",
                Some(crate::lang::text("tb.tracking")),
                "Current schedule against the baseline",
                doc,
                Command::SetView(ViewKind::TrackingGantt),
            );
            b(
                ui,
                "book-open",
                Some(crate::lang::text("tb.baselines")),
                "Compare Baseline, Baseline1 and Baseline2 bars",
                doc,
                Command::SetView(ViewKind::MultipleBaselines),
            );
            sep(ui);
            b(ui, "printer", Some(crate::lang::text("tb.print")), "Print preview, page setup and printing (Ctrl+P)", doc, Command::Print);
            b(ui, "file-text", Some(crate::lang::text("tb.export_pdf")), "Save the current view as a PDF", doc, Command::ExportPdf);
            b(
                ui,
                "file-output",
                Some(crate::lang::text("tb.export_csv")),
                "Export the task list as CSV",
                doc,
                Command::ExportCsv(plancraft_core::tabular::CsvTable::Tasks),
            );
            b(
                ui,
                "file-down",
                Some(crate::lang::text("tb.export_xml")),
                "Save the plan as a Microsoft Project XML file",
                doc,
                Command::ExportMspdi,
            );
            b(
                ui,
                "chart-gantt",
                Some(crate::lang::text("tb.export_gantt")),
                "Coloured Gantt cells for the tasks this filter shows",
                doc,
                Command::ExportGantt,
            );
            b(
                ui,
                "table",
                Some(crate::lang::text("tb.export_usage")),
                "Tasks, assignments and time-phased work as an Excel sheet (from the Task Usage view)",
                doc && st.view == ViewKind::TaskUsage,
                Command::ExportUsage,
            );
            b(
                ui,
                "kanban",
                Some(crate::lang::text("tb.export_planner")),
                "Push this plan to Microsoft Planner",
                doc,
                Command::ExportPlanner,
            );
        }
        MenuTab::View => {
            combo_label(ui, crate::lang::text("tb.filter"));
            egui::ComboBox::from_id_salt("tb_filter").width(130.0).selected_text(st.filter.label()).show_ui(ui, |ui| {
                for f in TaskFilter::ALL {
                    if ui.selectable_label(st.filter == f, f.label()).clicked() {
                        push(Command::Filter(f));
                    }
                }
            });
            combo_label(ui, crate::lang::text("tb.table"));
            egui::ComboBox::from_id_salt("tb_table").width(100.0).selected_text(st.table.title()).show_ui(ui, |ui| {
                for t in TaskTable::ALL {
                    if ui.selectable_label(st.table == t, t.title()).clicked() {
                        push(Command::Table(t));
                    }
                }
            });
            // Which columns the task table shows: a per-user choice, saved like the theme.
            let columns_btn = ui
                .add_enabled(!info.columns.is_empty(), egui::Button::new(crate::lang::text("tb.columns")))
                .on_hover_text(crate::lang::text("tb.columns_hint"));
            egui::Popup::from_toggle_button_response(&columns_btn).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
                |ui| {
                    ui.set_min_width(200.0);
                    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                        for (key, title, on, required) in &info.columns {
                            let mut on = *on;
                            if ui.add_enabled(!required, egui::Checkbox::new(&mut on, title)).changed() {
                                push(Command::ToggleColumn(key.clone()));
                            }
                        }
                    });
                },
            );
            let title_of = |key: &Option<String>, none: &str| {
                key.as_ref().and_then(|k| info.fields.iter().find(|(fk, _)| fk == k)).map_or_else(|| none.to_string(), |(_, t)| t.clone())
            };
            combo_label(ui, crate::lang::text("tb.sort"));
            let sort_key = st.sort.as_ref().map(|(k, _)| k.clone());
            let sort_text = match &st.sort {
                Some((_, desc)) => {
                    format!("{} ({})", title_of(&sort_key, st.t("tb.outline")), if *desc { st.t("tb.desc") } else { st.t("tb.asc") })
                }
                None => st.t("tb.outline").to_string(),
            };
            egui::ComboBox::from_id_salt("tb_sort").width(120.0).selected_text(sort_text).show_ui(ui, |ui| {
                if ui.selectable_label(st.sort.is_none(), st.t("tb.no_sort")).clicked() {
                    push(Command::SortBy(None));
                }
                for (key, title) in &info.fields {
                    let current = st.sort.as_ref().filter(|(k, _)| k == key).map(|(_, d)| *d);
                    if ui.selectable_label(current == Some(false), format!("{title}, {}", st.t("tb.asc"))).clicked() {
                        push(Command::SortBy(Some((key.clone(), false))));
                    }
                    if ui.selectable_label(current == Some(true), format!("{title}, {}", st.t("tb.desc"))).clicked() {
                        push(Command::SortBy(Some((key.clone(), true))));
                    }
                }
            });
            combo_label(ui, crate::lang::text("tb.group"));
            egui::ComboBox::from_id_salt("tb_group").width(110.0).selected_text(title_of(&st.group, st.t("tb.no_group"))).show_ui(
                ui,
                |ui| {
                    if ui.selectable_label(st.group.is_none(), st.t("tb.no_group")).clicked() {
                        push(Command::GroupBy(None));
                    }
                    for (key, title) in &info.fields {
                        if ui.selectable_label(st.group.as_ref() == Some(key), title).clicked() {
                            push(Command::GroupBy(Some(key.clone())));
                        }
                    }
                },
            );
            ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(130.0).hint_text(crate::lang::text("tb.search")));
            b(
                ui,
                "replace",
                None,
                "Find and replace in the task table (Ctrl+F; replace: Ctrl+H, or Cmd+Option+F on a Mac)",
                doc,
                Command::Find(false),
            );
            b(ui, "arrow-right", None, "Go to a task ID or a date (F5)", doc, Command::GoTo);
            sep(ui);
            b(ui, "fold-vertical", None, "Collapse all summaries", doc, Command::CollapseAll);
            b(ui, "unfold-vertical", None, "Expand all summaries", doc, Command::ExpandAll);
            sep(ui);
            b(ui, "zoom-in", None, "Zoom in (Ctrl++)", doc, Command::ZoomIn);
            b(ui, "zoom-out", None, "Zoom out (Ctrl+-)", doc, Command::ZoomOut);
            b(ui, "maximize", Some(crate::lang::text("tb.entire")), "Fit the whole project in the chart", doc, Command::ZoomEntire);
            b(ui, "crosshair", Some(crate::lang::text("tb.selected")), "Fit the selected tasks", sel, Command::ZoomSelected);
            timescale_chips(ui, st, info, push);
            sep(ui);
            if button(ui, "calendar", Some(crate::lang::text("tb.timeline")), "Show the timeline strip", doc, st.timeline_visible).clicked()
            {
                push(Command::ToggleTimelinePane);
            }
            if button(ui, "panel-right", Some(crate::lang::text("tb.properties")), "Show the properties pane", doc, st.properties_visible)
                .clicked()
            {
                push(Command::TogglePropertiesPane);
            }
            if button(ui, "history", Some(crate::lang::text("tb.history")), "Undo history", doc, info.history_open).clicked() {
                push(Command::History);
            }
            sep(ui);
            combo_label(ui, crate::lang::text("tb.theme"));
            egui::ComboBox::from_id_salt("tb_theme").width(80.0).selected_text(crate::lang::theme_name(settings.theme)).show_ui(ui, |ui| {
                for t in crate::theme::ThemeChoice::ALL {
                    if ui.selectable_label(settings.theme == t, crate::lang::theme_name(t)).clicked() {
                        push(Command::SetTheme(t));
                    }
                }
            });
            combo_label(ui, crate::lang::text("tb.startup"));
            egui::ComboBox::from_id_salt("tb_startup").width(140.0).selected_text(settings.startup.label()).show_ui(ui, |ui| {
                for mode in crate::settings::Startup::ALL {
                    if ui.selectable_label(settings.startup == mode, mode.label()).clicked() {
                        push(Command::SetStartup(mode));
                    }
                }
            });
            let mut open_overview = settings.open_to_overview;
            if ui.checkbox(&mut open_overview, crate::lang::text("startup.overview")).changed() {
                push(Command::SetOpenOverview(open_overview));
            }
            combo_label(ui, crate::lang::text("tb.language"));
            egui::ComboBox::from_id_salt("tb_lang").width(100.0).selected_text(st.lang.label()).show_ui(ui, |ui| {
                for lang in crate::lang::Lang::ALL {
                    if ui.selectable_label(st.lang == lang, lang.label()).clicked() {
                        push(Command::SetLang(lang));
                    }
                }
            });
            combo_label(ui, crate::lang::text("tb.style"));
            egui::ComboBox::from_id_salt("tb_style").width(110.0).selected_text(crate::lang::style_name(st.chart_style)).show_ui(
                ui,
                |ui| {
                    for style in plancraft_core::model::ChartStyle::ALL {
                        if ui.selectable_label(st.chart_style == style, crate::lang::style_name(style)).clicked() {
                            push(Command::SetChartStyle(style));
                        }
                    }
                },
            );
        }
        MenuTab::Format => {
            let toggle = |ui: &mut Ui, icon: &str, label: &str, tip: &str, on: bool, cmd: Command| {
                if button(ui, icon, Some(label), tip, doc, on).clicked() {
                    push(cmd);
                }
            };
            if st.view == ViewKind::NetworkDiagram {
                let n = &st.network;
                toggle(
                    ui,
                    "triangle-alert",
                    "Critical First",
                    "Lay out the critical path on the top row",
                    n.opts.critical_first,
                    Command::ToggleNetworkCriticalFirst,
                );
                toggle(
                    ui,
                    "rows-3",
                    "Summary Tasks",
                    "Show summary tasks, left of their subtasks",
                    n.show_summaries,
                    Command::ToggleNetworkSummaries,
                );
                sep(ui);
                toggle(
                    ui,
                    "link",
                    "Link Labels",
                    "Show the link type and lag on each line",
                    n.link_labels,
                    Command::ToggleNetworkLinkLabels,
                );
                toggle(
                    ui,
                    "gauge",
                    "Progress Marks",
                    "One diagonal through tasks in progress, an X through complete tasks",
                    n.progress_marks,
                    Command::ToggleNetworkProgressMarks,
                );
                toggle(ui, "hash", "IDs Only", "Small boxes that show only the task ID", n.opts.ids_only, Command::ToggleNetworkIdsOnly);
                sep(ui);
                if button(ui, "maximize", Some(crate::lang::text("tb.fit")), "Zoom to show the whole diagram", doc, false).clicked() {
                    st.network.fit_pending = true;
                }
                return;
            }
            let g = &st.gantt;
            if st.view == ViewKind::MultipleBaselines {
                combo_label(ui, "Bars of");
                let shown: Vec<String> =
                    crate::views::gantt_chart::baseline_slots(g.multi_baselines).map(|n| PlanSlot::Baseline(n).name()).collect();
                let title = if shown.is_empty() { "no baseline".to_string() } else { shown.join(", ") };
                ui.add_enabled_ui(doc, |ui| {
                    ui.menu_button(title, |ui| {
                        for n in 0..=plancraft_core::baseline::MAX_BASELINE {
                            let bit = 1u16 << n;
                            let mut on = g.multi_baselines & bit != 0;
                            if ui.checkbox(&mut on, PlanSlot::Baseline(n).name()).changed() {
                                push(Command::ToggleMultiBaseline(n));
                            }
                        }
                    })
                    .response
                    .on_hover_text("Which baselines to compare, one bar each")
                });
                sep(ui);
                toggle(
                    ui,
                    "rows-3",
                    "Project Summary",
                    "Show the project summary bar",
                    g.show_project_summary,
                    Command::ToggleShowProjectSummary,
                );
                return;
            }
            toggle(
                ui,
                "triangle-alert",
                crate::lang::text("fmt.critical"),
                "Highlight critical tasks",
                g.show_critical,
                Command::ToggleShowCritical,
            );
            toggle(
                ui,
                "route",
                crate::lang::text("fmt.paths"),
                "Highlight the selected task's predecessors and successors (Alt+Ctrl+L)",
                g.show_paths,
                Command::ToggleHighlightPaths,
            );
            toggle(ui, "hourglass", "Slack", "Show total slack after each task", g.show_slack, Command::ToggleShowSlack);
            toggle(
                ui,
                "ruler",
                "Baseline",
                "Show baseline bars (the Tracking Gantt always does)",
                g.show_baseline,
                Command::ToggleShowBaseline,
            );
            egui::ComboBox::from_id_salt("tb_baseline_slot")
                .width(88.0)
                .selected_text(PlanSlot::Baseline(g.baseline_slot).name())
                .show_ui(ui, |ui| {
                    for n in 0..=plancraft_core::baseline::MAX_BASELINE {
                        if ui.selectable_label(g.baseline_slot == n, PlanSlot::Baseline(n).name()).clicked() {
                            push(Command::SetBaselineSlot(n));
                        }
                    }
                })
                .response
                .on_hover_text("Which baseline the baseline bars show");
            sep(ui);
            toggle(ui, "gauge", "Progress", "Show progress inside bars", g.show_progress, Command::ToggleShowProgress);
            toggle(ui, "link", "Links", "Show dependency arrows", g.show_links, Command::ToggleShowLinks);
            toggle(
                ui,
                "user-round",
                "Resource Names",
                "Show resource names beside bars",
                g.show_resource_names,
                Command::ToggleShowResourceNames,
            );
            toggle(
                ui,
                "rows-3",
                "Project Summary",
                "Show the project summary bar",
                g.show_project_summary,
                Command::ToggleShowProjectSummary,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The whole command bar
// ---------------------------------------------------------------------------

/// Draws the view selector, tab row and toolbar row. Commands are appended to `out`.
pub fn command_bar(ui: &mut Ui, st: &mut UiState, info: &MenuInfo, settings: &AppSettings, out: &mut Vec<Command>) {
    let avail = ui.available_rect_before_wrap();
    let rect = Rect::from_min_size(avail.min, vec2(avail.width(), TAB_ROW_H + TOOL_ROW_H));
    ui.allocate_rect(rect, Sense::hover());
    let cmds = RefCell::new(Vec::new());
    let push = |c: Command| cmds.borrow_mut().push(c);

    let selector = Rect::from_min_size(rect.min, vec2(SELECTOR_W, rect.height()));
    let tabs = Rect::from_min_max(pos2(selector.right(), rect.top()), pos2(rect.right(), rect.top() + TAB_ROW_H));
    let tools = Rect::from_min_max(pos2(selector.right(), tabs.bottom()), rect.max);

    tab_strip(ui, tabs, st, info, settings, &push);

    ui.painter().rect_filled(tools, 0.0, theme::pal().PANEL);
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(tools.shrink2(vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
    egui::ScrollArea::horizontal()
        .id_salt("toolbar_scroll")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut child, |ui| {
            ui.set_height(TOOL_ROW_H);
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| toolbar(ui, st, info, settings, &push));
        });

    view_selector(ui, selector, st, info.has_doc, &push);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    out.extend(cmds.into_inner());
}
