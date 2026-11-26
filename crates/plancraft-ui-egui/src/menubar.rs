//! The title band and the two-row command bar.
//!
//! Layout:
//!
//! ```text
//! ┌──────────┬───────────────────────────────────────────────────────────┐
//! │  view    │ Task  Resource  Project  Report  View  Format             │  tabs
//! │ selector ├───────────────────────────────────────────────────────────┤
//! │    v     │ [+ Task] [Milestone] │ … │ [More ▾]   single toolbar row  │  toolbar
//! └──────────┴───────────────────────────────────────────────────────────┘
//! ```
//!
//! The view selector spans both rows. Tabs are plain tabs, and the toolbar is
//! one row of compact ArtCraft controls. When the row cannot fit, overflow goes
//! into a discoverable More menu (never a hidden scrollbar).

use plancraft_core::baseline::PlanSlot;
use std::cell::RefCell;

use egui::{Align, Align2, FontId, Layout, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use plancraft_core::model::ScheduleMode;

use crate::columns::TaskTable;
use crate::commands::Command;
use crate::logo;
use crate::menu::{host_is_mac, platform_shortcut_label, shortcut_label, tip_with_shortcut};
use crate::settings::AppSettings;
use crate::state::{MenuTab, TaskFilter, UiState, VIEW_MENU_GROUPS, ViewKind};
use crate::theme;
use crate::widgets::craft::{self, CONTROL_H, CORNER, ITEM_GAP};

pub struct MenuInfo {
    pub has_doc: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub task_selected: bool,
    /// How many tasks are selected. Link needs ≥2; Unlink may use one.
    pub selected_task_count: usize,
    pub resource_selected: bool,
    pub mcp_running: bool,
    pub mcp_url: Option<String>,
    /// Whether the Undo History window is open (the History button shows as pressed).
    pub history_open: bool,
    /// Document title shown centred in the unified title bar (PhotoCraft `Title` slot).
    pub doc_title: String,
    /// Fields offered for sorting and grouping: (key, title), the table's columns first.
    pub fields: Vec<(String, String)>,
    /// The Columns menu: (key, title, on, required). Only the name column is required.
    pub columns: Vec<(String, String, bool, bool)>,
}

const TAB_ROW_H: f32 = 28.0;
const TOOL_ROW_H: f32 = CONTROL_H;
const SELECTOR_W: f32 = 104.0;
#[allow(dead_code)] // height comes from craft-ui TitleBar / SlotSize::TitleBar now
const TITLE_H: f32 = 40.0;
/// Fallback when a Ui is not available (unit tests). Prefer [`more_reserve_width`].
#[cfg(test)]
const MORE_RESERVE_FALLBACK: f32 = 72.0;
/// Chip strip width: same paint metrics as `timescale_chips` (28px chips, 1px gaps).
const TIMESCALE_W: f32 = {
    let n = crate::state::Timescale::ALL.len();
    28.0 * n as f32 + 1.0 * n.saturating_sub(1) as f32
};
const BASELINE_COMBO_W: f32 = 88.0;

// ---------------------------------------------------------------------------
// Overflow planning (pure)
// ---------------------------------------------------------------------------

/// One toolbar chip for width planning. `primary` chips stay visible when space is tight.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolChip {
    pub id: &'static str,
    pub width: f32,
    pub primary: bool,
}

/// Measured width of the More control plus the gap before it.
pub fn more_reserve_width(ui: &Ui) -> f32 {
    craft::tool_width(ui, Some(crate::lang::text("tb.more"))) + ITEM_GAP
}

/// Split chips into those that stay on the row and those that move into More.
/// When everything fits, overflow is empty and no More reserve is taken.
/// `more_w` must be the measured More control width plus its leading gap.
pub fn partition_chips(chips: &[ToolChip], available: f32, more_w: f32) -> (Vec<usize>, Vec<usize>) {
    if chips.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let gap = ITEM_GAP;
    let total = chips.iter().map(|c| c.width).sum::<f32>() + gap * chips.len().saturating_sub(1) as f32;
    if total <= available {
        return ((0..chips.len()).collect(), Vec::new());
    }
    let budget = (available - more_w.max(0.0)).max(0.0);
    let mut visible: Vec<usize> = Vec::new();
    let mut used = 0.0;
    // Keep primary chips first (in order), then fill with the rest in order.
    for (i, chip) in chips.iter().enumerate() {
        if !chip.primary {
            continue;
        }
        let next = if visible.is_empty() { chip.width } else { used + gap + chip.width };
        if next <= budget {
            visible.push(i);
            used = next;
        }
    }
    for (i, chip) in chips.iter().enumerate() {
        if chip.primary {
            continue;
        }
        let next = if visible.is_empty() { chip.width } else { used + gap + chip.width };
        if next <= budget {
            visible.push(i);
            used = next;
        }
    }
    visible.sort_unstable();
    let overflow: Vec<usize> = (0..chips.len()).filter(|i| !visible.contains(i)).collect();
    (visible, overflow)
}

// ---------------------------------------------------------------------------
// Title band
// ---------------------------------------------------------------------------

/// Unified PhotoCraft-family title bar via real `craft_ui_shell::shell::TitleBar`
/// (`show_titled`: leading brand/QA, centred document [`Title`], trailing MCP).
/// Window frame is `craft_window::policy::Frame::Inset` on macOS (`craft_shell`).
pub fn title_bar(ui: &mut Ui, info: &MenuInfo, out: &mut Vec<Command>) {
    let mac = host_is_mac();
    let doc = info.doc_title.as_str();
    // Leading + trailing both emit commands; share one buffer (TitleBar holds both closures).
    let pending = std::cell::RefCell::new(Vec::<Command>::new());
    // Integrated caption Close does not close the window. Quit uses the same unsaved-changes path as the menu.
    let bar = crate::craft_shell::show_title_bar(
        ui,
        doc,
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (mark, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
            logo::paint(ui.painter(), mark);
            ui.add_space(8.0);
            let brand_font = FontId::new(theme::TITLE_BAND_LABEL_SIZE, theme::display());
            let brand_galley = ui.painter().layout_no_wrap("PlanCraft".to_owned(), brand_font, theme::pal().TEXT);
            let brand_w = brand_galley.size().x;
            let brand_h = brand_galley.size().y.max(theme::TITLE_BAND_QA_SIZE);
            let (brand_rect, _) = ui.allocate_exact_size(vec2(brand_w, brand_h), Sense::click());
            ui.painter().galley(
                pos2(brand_rect.left(), brand_rect.center().y - brand_galley.size().y / 2.0),
                brand_galley,
                theme::pal().TEXT,
            );
            ui.add_space(10.0);
            let (sep, _) = ui.allocate_exact_size(vec2(11.0, theme::TITLE_BAND_QA_SIZE), Sense::hover());
            ui.painter().vline(sep.center().x, (sep.center().y - 8.0)..=(sep.center().y + 8.0), theme::hairline());
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = ITEM_GAP;
            if craft::title_qa_button(ui, "save", &tip_with_shortcut(&Command::Save, "Save", mac), info.has_doc).clicked() {
                pending.borrow_mut().push(Command::Save);
            }
            let undo_base = match &info.undo_label {
                Some(l) => format!("Undo {l}"),
                None => "Undo".into(),
            };
            if craft::title_qa_button(ui, "undo-2", &tip_with_shortcut(&Command::Undo, &undo_base, mac), info.can_undo).clicked() {
                pending.borrow_mut().push(Command::Undo);
            }
            if craft::title_qa_button(ui, "redo-2", &tip_with_shortcut(&Command::Redo, "Redo", mac), info.can_redo).clicked() {
                pending.borrow_mut().push(Command::Redo);
            }
        },
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (icon, txt, color) =
                if info.mcp_running { ("zap", "MCP LIVE", theme::pal().MCP_LIVE) } else { ("circle", "MCP OFF", theme::pal().MCP_OFF) };
            let tip = match &info.mcp_url {
                Some(u) => format!("AI agents can control this window via MCP at {u}\nClick to manage."),
                None => "Start the built-in MCP server so AI agents (e.g. Claude Code) can work with your projects.".into(),
            };
            let label = craft::hud_job(txt, color);
            let resp = ui
                .add(egui::Button::image_and_text(crate::icons::image(icon, theme::TITLE_BAND_STATUS_SIZE, color), label).frame(false))
                .on_hover_text(tip)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                pending.borrow_mut().push(Command::McpPanel);
            }
        },
    );
    if bar.close_requested {
        pending.borrow_mut().push(Command::Quit);
    }
    out.append(&mut pending.into_inner());
}

// ---------------------------------------------------------------------------
// Toolbar widgets
// ---------------------------------------------------------------------------

fn button(
    ui: &mut Ui,
    icon: &str,
    label: Option<&str>,
    tip: &str,
    enabled: bool,
    active: bool,
    _priority: bool,
    toggle: bool,
) -> egui::Response {
    // Priority keeps commands visible in overflow planning; it does not change toolbar styling.
    craft::tool_button(ui, icon, label, tip, enabled, active, false, toggle)
}

fn popup_button(ui: &mut Ui, icon: &str, label: &'static str, tip: &'static str, enabled: bool, open: bool) -> egui::Response {
    craft::tool_button_on(ui, icon, Some(label), tip, enabled, false, false, false, open, theme::pal().PANEL)
}

/// Thin vertical divider between groups of related buttons.
fn sep(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(11.0, CONTROL_H), Sense::hover());
    ui.painter().vline(rect.center().x, (rect.top() + 6.0)..=(rect.bottom() - 6.0), theme::hairline());
}

/// A mini pie button for "x% complete".
fn pie(ui: &mut Ui, pct: u8, enabled: bool) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let (r, resp) = ui.allocate_exact_size(vec2(28.0, CONTROL_H), if enabled { Sense::click() } else { Sense::hover() });
    let chrome = theme::control_chrome(
        theme::pal().PANEL,
        theme::ControlRole::Ghost,
        theme::ControlState { enabled, selected: false, hovered: resp.hovered(), pressed: resp.is_pointer_button_down_on(), open: false },
    );
    if let Some(fill) = chrome.fill {
        ui.painter().rect_filled(r, CORNER, fill);
    }
    if let Some(stroke) = chrome.stroke {
        ui.painter().rect_stroke(r, CORNER, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    }
    craft::paint_focus_ring(ui, r, enabled && resp.has_focus());
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
    let tip = format!("Mark {pct}% complete");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip.clone()));
    resp.on_hover_text(tip)
}

fn combo_label(ui: &mut Ui, text: &str) {
    craft::hud(ui, text, theme::pal().TEXT_WEAK);
}

fn tip(cmd: Command, description: &str) -> String {
    tip_with_shortcut(&cmd, description, host_is_mac())
}

// ---------------------------------------------------------------------------
// View selector (spans both rows)
// ---------------------------------------------------------------------------

fn view_selector(ui: &mut Ui, rect: Rect, st: &UiState, enabled: bool, push: &dyn Fn(Command)) {
    let enabled = enabled && ui.is_enabled();
    let resp = ui.interact(rect, egui::Id::unique("view_selector"), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, st.view.label()));
    let popup_id = egui::Popup::default_response_id(&resp);
    if !enabled && egui::Popup::is_id_open(ui.ctx(), popup_id) {
        egui::Popup::close_id(ui.ctx(), popup_id);
    }
    let popup_open = enabled && egui::Popup::is_id_open(ui.ctx(), popup_id);
    let p = ui.painter();
    p.rect_filled(rect, 0.0, theme::pal().PANEL_BG);
    let chrome = theme::control_chrome(
        theme::pal().PANEL_BG,
        theme::ControlRole::Ghost,
        theme::ControlState {
            enabled,
            selected: false,
            hovered: resp.hovered(),
            pressed: resp.is_pointer_button_down_on(),
            open: popup_open || resp.clicked(),
        },
    );
    if let Some(fill) = chrome.fill {
        p.rect_filled(rect, 0.0, fill);
    }
    if let Some(stroke) = chrome.stroke {
        p.rect_stroke(rect, CORNER, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    }
    craft::paint_focus_ring(ui, rect, enabled && resp.has_focus());
    p.vline(rect.right() - 0.5, rect.y_range(), theme::hairline());
    let color = if enabled { theme::pal().TEXT } else { theme::pal().LINE_STRONG };
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 18.0), vec2(20.0, 20.0));
    crate::icons::paint(ui, icon_rect, st.view.icon(), 18.0, color);
    let galley = p.layout(st.view.label().to_string(), FontId::proportional(theme::VIEW_SELECTOR_LABEL_SIZE), color, rect.width() - 12.0);
    let gh = galley.size().y;
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 32.0), galley, color);
    let cy = rect.top() + 32.0 + gh + 6.0;
    let c = pos2(rect.center().x, cy.min(rect.bottom() - 5.0));
    let arrow = if enabled { theme::pal().TEXT_WEAK } else { theme::pal().LINE_STRONG };
    p.add(Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 2.5)], arrow, Stroke::NONE));
    let resp = resp.on_hover_text(crate::lang::text("view.change"));
    egui::Popup::menu(&resp).id(popup_id).show(|ui| {
        ui.set_min_width(190.0);
        for (i, group) in VIEW_MENU_GROUPS.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            craft::hud(ui, crate::lang::text(group.caption_key), theme::pal().TEXT_WEAK);
            for &v in group.views {
                let selected = st.view == v;
                let ink = if selected { theme::pal().INVERT_FG } else { theme::pal().TEXT };
                let img = crate::icons::image(v.icon(), 14.0, ink);
                let mut button = egui::Button::image_and_text(img, egui::RichText::new(v.label()).color(ink)).selected(selected);
                if selected {
                    button = button.fill(theme::pal().INVERT_BG);
                }
                if ui.add(button).clicked() {
                    push(Command::SetView(v));
                }
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
    let caps = |text: &str, color| craft::caps(&p, text, theme::HUD_SIZE, theme::mono(), theme::HUD_SPACING, color);
    let _ = info;

    for tab in MenuTab::ALL {
        let w = caps(tab.label(), theme::pal().TEXT).size().x + 22.0;
        let r = tab_rect(x, w);
        let resp = ui.interact(r, egui::Id::unique(("tab", tab.label())), Sense::click());
        let active = st.menu_tab == tab;
        let color = if active {
            p.rect_filled(r, 0.0, theme::pal().INVERT_BG);
            theme::pal().INVERT_FG
        } else {
            let chrome = theme::control_chrome(
                theme::pal().PANEL_BG,
                theme::ControlRole::Ghost,
                theme::ControlState {
                    enabled: true,
                    selected: false,
                    hovered: resp.hovered(),
                    pressed: resp.is_pointer_button_down_on(),
                    open: false,
                },
            );
            if let Some(fill) = chrome.fill {
                p.rect_filled(r, CORNER, fill);
            }
            if let Some(stroke) = chrome.stroke {
                p.rect_stroke(r, CORNER, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
            }
            if resp.hovered() { theme::pal().TEXT } else { theme::pal().TEXT_WEAK }
        };
        craft::paint_focus_ring(ui, r, resp.has_focus());
        let g = caps(tab.label(), color);
        p.galley(pos2(r.center().x - g.size().x / 2.0 + theme::HUD_SPACING / 2.0, r.center().y - g.size().y / 2.0), g, color);
        if resp.clicked() {
            st.menu_tab = tab;
        }
        x += w + 4.0;
    }

    let r = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(22.0, 20.0));
    let resp = ui.interact(r, egui::Id::unique("tab_help"), Sense::click());
    let chrome = theme::control_chrome(
        theme::pal().PANEL_BG,
        theme::ControlRole::Ghost,
        theme::ControlState {
            enabled: true,
            selected: false,
            hovered: resp.hovered(),
            pressed: resp.is_pointer_button_down_on(),
            open: false,
        },
    );
    if let Some(fill) = chrome.fill {
        p.rect_filled(r, CORNER, fill);
    }
    if let Some(stroke) = chrome.stroke {
        p.rect_stroke(r, CORNER, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    }
    craft::paint_focus_ring(ui, r, resp.has_focus());
    p.text(r.center(), Align2::CENTER_CENTER, "?", FontId::proportional(13.0), theme::pal().TEXT_WEAK);
    if resp.on_hover_text("About PlanCraft").clicked() {
        push(Command::About);
    }
}

// ---------------------------------------------------------------------------
// Toolbar contents per tab
// ---------------------------------------------------------------------------

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
            let color = if enabled { theme::pal().TEXT } else { theme::pal().TEXT_WEAK };
            let mut chip = egui::Button::new(craft::hud_job(ts.letter(), color)).min_size(vec2(28.0, CONTROL_H - 6.0));
            if on {
                chip = chip.fill(theme::pal().WASH_SELECTED).stroke(Stroke::new(1.0, theme::pal().LINE_STRONG));
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

/// A paint-able toolbar entry (command chip or separator / pie).
enum Entry {
    Cmd {
        icon: &'static str,
        /// HUD label on the row; `None` means icon-only.
        label: Option<&'static str>,
        /// Short localized caption for accessibility and the More menu (always set for commands).
        caption: &'static str,
        tip: String,
        enabled: bool,
        active: bool,
        /// When true, More shows a checkbox reflecting `active`.
        toggle: bool,
        primary: bool,
        cmd: Command,
    },
    Sep,
    Pie {
        pct: u8,
        enabled: bool,
    },
}

impl Entry {
    fn chip(&self, ui: &Ui) -> Option<ToolChip> {
        match self {
            Entry::Cmd { label, caption, primary, .. } => {
                Some(ToolChip { id: caption, width: craft::tool_width(ui, *label), primary: *primary })
            }
            Entry::Sep => Some(ToolChip { id: "sep", width: 11.0, primary: false }),
            Entry::Pie { .. } => Some(ToolChip { id: "pie", width: 28.0, primary: false }),
        }
    }

    fn paint(&self, ui: &mut Ui, push: &dyn Fn(Command)) {
        match self {
            Entry::Cmd { icon, label, caption, tip, enabled, active, toggle, primary, cmd } => {
                // Icon-only: hover/a11y use the localized caption (plus shortcut), not an English tip.
                let owned;
                let tip_ref: &str = if label.is_none() {
                    owned = match shortcut_label(cmd) {
                        Some(p) => format!("{caption} ({})", platform_shortcut_label(&p, host_is_mac())),
                        None => (*caption).to_string(),
                    };
                    owned.as_str()
                } else {
                    tip.as_str()
                };
                // `primary` still drives overflow keep-on-row; invert chrome is suppressed for toggles.
                if button(ui, icon, *label, tip_ref, *enabled, *active, *primary, *toggle).clicked() {
                    push(cmd.clone());
                }
            }
            Entry::Sep => sep(ui),
            Entry::Pie { pct, enabled } => {
                if pie(ui, *pct, *enabled).clicked() {
                    push(Command::SetPercent(*pct));
                }
            }
        }
    }

    fn overflow_label(&self) -> Option<&'static str> {
        match self {
            Entry::Cmd { caption, .. } => Some(*caption),
            Entry::Pie { pct, .. } => match *pct {
                0 => Some("0%"),
                25 => Some("25%"),
                50 => Some("50%"),
                75 => Some("75%"),
                100 => Some("100%"),
                _ => None,
            },
            Entry::Sep => None,
        }
    }

    fn overflow_enabled(&self) -> bool {
        match self {
            Entry::Cmd { enabled, .. } | Entry::Pie { enabled, .. } => *enabled,
            Entry::Sep => false,
        }
    }

    fn overflow_run(&self, push: &dyn Fn(Command)) {
        match self {
            Entry::Cmd { cmd, enabled, .. } if *enabled => push(cmd.clone()),
            Entry::Pie { pct, enabled } if *enabled => push(Command::SetPercent(*pct)),
            _ => {}
        }
    }

    fn paint_overflow_row(&self, ui: &mut Ui, push: &dyn Fn(Command)) {
        let Some(caption) = self.overflow_label() else { return };
        let enabled = self.overflow_enabled();
        match self {
            Entry::Cmd { tip, active, toggle, cmd, .. } => {
                let shortcut = shortcut_label(cmd).map(|p| platform_shortcut_label(&p, host_is_mac())).unwrap_or_default();
                if *toggle {
                    let mut on = *active;
                    let resp = ui.add_enabled(enabled, egui::Checkbox::new(&mut on, caption)).on_hover_text(tip);
                    if enabled && (resp.changed() || resp.clicked()) {
                        self.overflow_run(push);
                        ui.close();
                    }
                } else {
                    let mut btn = egui::Button::new(caption);
                    if !shortcut.is_empty() {
                        btn = btn.shortcut_text(shortcut);
                    }
                    if ui.add_enabled(enabled, btn).on_hover_text(tip).clicked() {
                        self.overflow_run(push);
                        ui.close();
                    }
                }
            }
            Entry::Pie { .. } => {
                if ui.add_enabled(enabled, egui::Button::new(caption)).clicked() {
                    self.overflow_run(push);
                    ui.close();
                }
            }
            Entry::Sep => {}
        }
    }
}

fn cmd(icon: &'static str, label: Option<&'static str>, tip: String, enabled: bool, primary: bool, command: Command) -> Entry {
    let caption = label.unwrap_or("");
    Entry::Cmd { icon, label, caption, tip, enabled, active: false, toggle: false, primary, cmd: command }
}

fn icon_cmd(icon: &'static str, caption_key: &'static str, tip: String, enabled: bool, primary: bool, command: Command) -> Entry {
    let caption = crate::lang::text(caption_key);
    Entry::Cmd { icon, label: None, caption, tip, enabled, active: false, toggle: false, primary, cmd: command }
}

fn toggle_cmd(
    icon: &'static str,
    label: Option<&'static str>,
    tip: String,
    enabled: bool,
    active: bool,
    primary: bool,
    command: Command,
) -> Entry {
    let caption = label.unwrap_or("");
    Entry::Cmd { icon, label, caption, tip, enabled, active, toggle: true, primary, cmd: command }
}

fn task_entries(st: &UiState, info: &MenuInfo) -> Vec<Entry> {
    let doc = info.has_doc;
    let sel = info.task_selected && doc;
    let link = doc && info.selected_task_count >= 2;
    vec![
        cmd(
            "plus",
            Some(crate::lang::text("tb.task")),
            tip(Command::InsertTask, "Insert a task beneath the selection"),
            doc,
            true,
            Command::InsertTask,
        ),
        cmd("diamond", Some(crate::lang::text("tb.milestone")), "Insert a milestone".into(), doc, true, Command::InsertMilestone),
        cmd(
            "rows-3",
            Some(crate::lang::text("tb.summary")),
            "Group the selected tasks under a new summary task".into(),
            doc,
            false,
            Command::InsertSummary,
        ),
        cmd(
            "repeat",
            Some(crate::lang::text("tb.recurring")),
            "Insert a task that repeats daily, weekly, monthly or yearly".into(),
            doc,
            false,
            Command::RecurringTask,
        ),
        icon_cmd("trash-2", "tb.delete", tip(Command::DeleteSelection, "Delete the selected tasks"), sel, false, Command::DeleteSelection),
        Entry::Sep,
        icon_cmd("indent-decrease", "tb.outdent", tip(Command::Outdent, "Outdent"), sel, false, Command::Outdent),
        icon_cmd("indent-increase", "tb.indent", tip(Command::Indent, "Indent"), sel, false, Command::Indent),
        icon_cmd("arrow-up", "tb.move_up", tip(Command::MoveUp, "Move up"), sel, false, Command::MoveUp),
        icon_cmd("arrow-down", "tb.move_down", tip(Command::MoveDown, "Move down"), sel, false, Command::MoveDown),
        Entry::Sep,
        cmd(
            "link",
            Some(crate::lang::text("tb.link")),
            tip(Command::LinkSelected, "Link the selected tasks finish-to-start"),
            link,
            true,
            Command::LinkSelected,
        ),
        cmd(
            "unlink",
            Some(crate::lang::text("tb.unlink")),
            tip(Command::UnlinkSelected, "Remove links between the selected tasks"),
            sel,
            false,
            Command::UnlinkSelected,
        ),
        toggle_cmd(
            "split",
            Some(crate::lang::text("tb.split")),
            "Split Task: click a bar where its work should pause; drag a part to move it, or back to rejoin".into(),
            doc,
            st.gantt.split_mode,
            false,
            Command::SplitTask,
        ),
        Entry::Sep,
        Entry::Pie { pct: 0, enabled: sel },
        Entry::Pie { pct: 25, enabled: sel },
        Entry::Pie { pct: 50, enabled: sel },
        Entry::Pie { pct: 75, enabled: sel },
        Entry::Pie { pct: 100, enabled: sel },
        cmd(
            "check",
            Some(crate::lang::text("tb.on_track")),
            "Set progress to where the schedule says it should be at the status date (today if none is set)".into(),
            sel,
            false,
            Command::MarkOnTrack,
        ),
        Entry::Sep,
        icon_cmd(
            "rotate-cw",
            "tb.auto_schedule",
            "Auto schedule the selected tasks".into(),
            sel,
            false,
            Command::SetMode(ScheduleMode::Auto),
        ),
        icon_cmd(
            "pin",
            "tb.manual_schedule",
            "Manually schedule the selected tasks".into(),
            sel,
            false,
            Command::SetMode(ScheduleMode::Manual),
        ),
        cmd(
            "eraser",
            Some(crate::lang::text("tb.clear_dates")),
            tip(Command::ClearDates, "Clear date limits; Size of group fill/% stay eligible"),
            sel,
            false,
            Command::ClearDates,
        ),
        icon_cmd("pause", "tb.toggle_active", "Inactivate / activate".into(), sel, false, Command::ToggleActive),
        Entry::Sep,
        cmd("info", Some(crate::lang::text("tb.information")), tip(Command::TaskInfo, "Task information"), sel, false, Command::TaskInfo),
        icon_cmd("pen-line", "tb.notes", "Task notes".into(), sel, false, Command::TaskNotes),
        icon_cmd("calendar", "tb.show_on_timeline", "Show on the timeline".into(), sel, false, Command::ToggleTimeline),
        icon_cmd("locate", "tb.scroll_to", "Scroll the chart to the task".into(), sel, false, Command::ScrollToTask),
        Entry::Sep,
        icon_cmd("scissors", "tb.cut", tip(Command::Cut, "Cut"), sel, false, Command::Cut),
        icon_cmd("copy", "tb.copy", tip(Command::Copy, "Copy"), sel, false, Command::Copy),
        icon_cmd("clipboard-paste", "tb.paste", tip(Command::Paste, "Paste"), doc && !st.clipboard.is_empty(), false, Command::Paste),
    ]
}

fn paint_entries(ui: &mut Ui, entries: &[Entry], push: &dyn Fn(Command)) {
    paint_entries_reserving(ui, entries, push, 0.0);
}

/// Partition and paint `entries`, keeping `reserve` px free at the trailing edge for a follow-on
/// control (e.g. the Format baseline combo).
fn paint_entries_reserving(ui: &mut Ui, entries: &[Entry], push: &dyn Fn(Command), reserve: f32) -> Vec<usize> {
    let chips: Vec<ToolChip> = entries.iter().filter_map(|e| e.chip(ui)).collect();
    let more_w = more_reserve_width(ui);
    let available = (ui.available_width() - reserve).max(0.0);
    let (visible, overflow) = partition_chips(&chips, available, more_w);
    ui.spacing_mut().item_spacing.x = ITEM_GAP;
    for &i in &visible {
        if let Some(e) = entries.get(i) {
            e.paint(ui, push);
        }
    }
    if !overflow.is_empty() {
        more_menu(ui, |ui| {
            for &i in &overflow {
                let Some(e) = entries.get(i) else { continue };
                e.paint_overflow_row(ui, push);
            }
        });
    }
    overflow
}

fn more_menu(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    let popup_id = ui.make_persistent_id("tb_more_popup");
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let more = popup_button(ui, "list-tree", crate::lang::text("tb.more"), crate::lang::text("tb.more_hint"), true, open);
    let max_height = (ui.ctx().content_rect().bottom() - more.rect.bottom() - theme::POPUP_SCREEN_MARGIN).max(40.0);
    egui::Popup::menu(&more).id(popup_id).show(|ui| {
        ui.set_min_width(220.0);
        egui::ScrollArea::vertical().max_height(max_height).show(ui, add_contents);
        #[cfg(test)]
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::unique("toolbar_more_popup_rect"), ui.min_rect()));
    });
}

/// Appearance / language / startup controls that permanently live in the View tab More menu.
fn view_settings_more(ui: &mut Ui, st: &UiState, settings: &AppSettings, push: &dyn Fn(Command)) {
    ui.label(egui::RichText::new(crate::lang::text("tb.theme")).small().color(theme::pal().TEXT_WEAK));
    for t in crate::theme::ThemeChoice::ALL {
        if ui.selectable_label(settings.theme == t, crate::lang::theme_name(t)).clicked() {
            push(Command::SetTheme(t));
            ui.close();
        }
    }
    ui.separator();
    ui.label(egui::RichText::new(crate::lang::text("tb.startup")).small().color(theme::pal().TEXT_WEAK));
    for mode in crate::settings::Startup::ALL {
        if ui.selectable_label(settings.startup == mode, mode.label()).clicked() {
            push(Command::SetStartup(mode));
            ui.close();
        }
    }
    let mut open_overview = settings.open_to_overview;
    if ui.checkbox(&mut open_overview, crate::lang::text("startup.overview")).changed() {
        push(Command::SetOpenOverview(open_overview));
    }
    let mut wrap = st.word_wrap;
    if ui.checkbox(&mut wrap, crate::lang::text("tb.wrap")).changed() {
        push(Command::ToggleWordWrap);
    }
    ui.separator();
    ui.label(egui::RichText::new(crate::lang::text("tb.language")).small().color(theme::pal().TEXT_WEAK));
    for lang in crate::lang::Lang::ALL {
        if ui.selectable_label(st.lang == lang, lang.label()).clicked() {
            push(Command::SetLang(lang));
            ui.close();
        }
    }
    ui.separator();
    ui.label(egui::RichText::new(crate::lang::text("tb.style")).small().color(theme::pal().TEXT_WEAK));
    for style in plancraft_core::model::ChartStyle::ALL {
        if ui.selectable_label(st.chart_style == style, crate::lang::style_name(style)).clicked() {
            push(Command::SetChartStyle(style));
            ui.close();
        }
    }
}

fn toolbar(ui: &mut Ui, st: &mut UiState, info: &MenuInfo, settings: &AppSettings, push: &dyn Fn(Command)) {
    let doc = info.has_doc;
    let sel = info.task_selected && doc;
    let res = info.resource_selected && doc;
    match st.menu_tab {
        MenuTab::Task => {
            let entries = task_entries(st, info);
            paint_entries(ui, &entries, push);
        }
        MenuTab::Resource => {
            let entries = vec![
                cmd(
                    "users-round",
                    Some(crate::lang::text("tb.assign")),
                    "Assign resources to the selected tasks".into(),
                    sel,
                    true,
                    Command::AssignResources,
                ),
                Entry::Sep,
                cmd("plus", Some(crate::lang::text("tb.add_resource")), "Add a resource".into(), doc, true, Command::InsertResource),
                cmd(
                    "info",
                    Some(crate::lang::text("tb.information")),
                    "Resource information, time off and availability".into(),
                    res,
                    false,
                    Command::ResourceInfo,
                ),
                cmd(
                    "trash-2",
                    Some(crate::lang::text("tb.delete_resource")),
                    "Delete the selected resources (replace assignments when they are used)".into(),
                    res,
                    false,
                    Command::DeleteResources,
                ),
                Entry::Sep,
                cmd(
                    "scale",
                    Some(crate::lang::text("tb.level")),
                    "Delay tasks to resolve resource overallocations".into(),
                    doc,
                    false,
                    Command::LevelAll,
                ),
                cmd(
                    "rotate-ccw",
                    Some(crate::lang::text("tb.clear_level")),
                    "Remove all leveling delays".into(),
                    doc,
                    false,
                    Command::ClearLeveling,
                ),
                Entry::Sep,
                cmd(
                    "timer",
                    Some(crate::lang::text("tb.timesheet")),
                    "Log hours per resource and task".into(),
                    doc,
                    false,
                    Command::SetView(ViewKind::Timesheet),
                ),
                cmd(
                    "rotate-cw",
                    Some(crate::lang::text("tb.update_progress")),
                    "Set % complete from tracked time".into(),
                    doc,
                    false,
                    Command::UpdateProgressFromTime,
                ),
            ];
            paint_entries(ui, &entries, push);
        }
        MenuTab::Project => {
            let entries = vec![
                cmd(
                    "archive",
                    Some(crate::lang::text("tb.project_info")),
                    "Name, dates, calendar and scheduling options".into(),
                    doc,
                    true,
                    Command::ProjectInfo,
                ),
                cmd(
                    "calendar-days",
                    Some(crate::lang::text("tb.working_time")),
                    "Calendars, work weeks and holidays".into(),
                    doc,
                    true,
                    Command::ChangeWorkingTime,
                ),
                Entry::Sep,
                cmd(
                    "link",
                    Some(crate::lang::text("tb.links")),
                    "Depend on tasks in other project files".into(),
                    doc,
                    false,
                    Command::ExternalLinks,
                ),
                icon_cmd("rotate-cw", "tb.refresh_links", "Refresh links to other projects".into(), doc, false, Command::RefreshExternal),
                Entry::Sep,
                cmd(
                    "tag",
                    Some(crate::lang::text("tb.custom")),
                    "Define your own task fields: text with a pick-list, number, cost, date, duration, flag".into(),
                    doc,
                    false,
                    Command::CustomFields,
                ),
                Entry::Sep,
                cmd(
                    "skip-forward",
                    Some(crate::lang::text("tb.update")),
                    "Update progress through a date, or reschedule work not done yet to start after it".into(),
                    doc,
                    false,
                    Command::UpdateProject,
                ),
                Entry::Sep,
                cmd(
                    "ruler",
                    Some(crate::lang::text("tb.baseline")),
                    "Save the schedule as Baseline or Baseline1–10, or set an interim plan".into(),
                    doc,
                    false,
                    Command::SetBaseline,
                ),
                icon_cmd("x", "tb.clear_baseline", "Clear a baseline or an interim plan".into(), doc, false, Command::ClearBaseline),
                Entry::Sep,
                toggle_cmd(
                    "zap",
                    Some(crate::lang::text("tb.agents")),
                    "Built-in MCP server for AI agents".into(),
                    true,
                    info.mcp_running,
                    false,
                    Command::McpPanel,
                ),
            ];
            paint_entries(ui, &entries, push);
        }
        MenuTab::Report => {
            let entries = vec![
                cmd(
                    "chart-column",
                    Some(crate::lang::text("tb.statistics")),
                    "Project statistics, variance and earned value".into(),
                    doc,
                    true,
                    Command::Statistics,
                ),
                cmd(
                    "diamond",
                    Some(crate::lang::text("tb.milestones")),
                    "Milestone status report".into(),
                    doc,
                    false,
                    Command::SetView(ViewKind::Milestones),
                ),
                cmd(
                    "signal",
                    Some(crate::lang::text("tb.workload")),
                    "Resource utilization and resource graph".into(),
                    doc,
                    false,
                    Command::SetView(ViewKind::Workload),
                ),
                cmd(
                    "chart-line",
                    Some(crate::lang::text("tb.tracking")),
                    "Current schedule against the baseline".into(),
                    doc,
                    false,
                    Command::SetView(ViewKind::TrackingGantt),
                ),
                cmd(
                    "book-open",
                    Some(crate::lang::text("tb.baselines")),
                    "Compare Baseline, Baseline1 and Baseline2 bars".into(),
                    doc,
                    false,
                    Command::SetView(ViewKind::MultipleBaselines),
                ),
                Entry::Sep,
                cmd(
                    "printer",
                    Some(crate::lang::text("tb.print")),
                    tip(Command::Print, "Print preview, page setup and printing"),
                    doc,
                    false,
                    Command::Print,
                ),
                cmd(
                    "file-text",
                    Some(crate::lang::text("tb.export_pdf")),
                    "Save the current view as a PDF".into(),
                    doc,
                    false,
                    Command::ExportPdf,
                ),
                cmd(
                    "file-output",
                    Some(crate::lang::text("tb.export_csv")),
                    "Export the task list as CSV".into(),
                    doc,
                    false,
                    Command::ExportCsv(plancraft_core::tabular::CsvTable::Tasks),
                ),
                cmd(
                    "file-down",
                    Some(crate::lang::text("tb.export_xml")),
                    "Save the plan for Planner Premium, with each task's calculated start pinned".into(),
                    doc,
                    false,
                    Command::ExportMspdi,
                ),
                cmd(
                    "chart-gantt",
                    Some(crate::lang::text("tb.export_gantt")),
                    "Coloured Gantt cells for the tasks this filter shows".into(),
                    doc,
                    false,
                    Command::ExportGantt,
                ),
                cmd(
                    "table",
                    Some(crate::lang::text("tb.export_usage")),
                    "Tasks, assignments and time-phased work as an Excel sheet (from the Task Usage view)".into(),
                    doc && st.view == ViewKind::TaskUsage,
                    false,
                    Command::ExportUsage,
                ),
                cmd(
                    "kanban",
                    Some(crate::lang::text("tb.send_planner")),
                    crate::lang::text("send.explain").into(),
                    doc,
                    false,
                    Command::SendPlanner,
                ),
            ];
            paint_entries(ui, &entries, push);
        }
        MenuTab::View => {
            // Keep filter / table / columns inline. Sort, group, search, icons, timescale and
            // panes share one overflow plan with appearance settings in the same More menu.
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
            let columns_enabled = !info.columns.is_empty();
            let columns_popup = ui.make_persistent_id("tb_columns_popup");
            let columns_open = egui::Popup::is_id_open(ui.ctx(), columns_popup);
            if !columns_enabled && columns_open {
                egui::Popup::close_id(ui.ctx(), columns_popup);
            }
            let columns_btn = ui
                .add_enabled(
                    columns_enabled,
                    egui::Button::new(crate::lang::text("tb.columns")).selected(columns_open).min_size(vec2(0.0, CONTROL_H - 6.0)),
                )
                .on_hover_text(crate::lang::text("tb.columns_hint"));
            egui::Popup::from_toggle_button_response(&columns_btn)
                .id(columns_popup)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_min_width(200.0);
                    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                        for (key, title, on, required) in &info.columns {
                            let mut on = *on;
                            if ui.add_enabled(!required, egui::Checkbox::new(&mut on, title)).changed() {
                                push(Command::ToggleColumn(key.clone()));
                            }
                        }
                    });
                });

            let title_of = |key: &Option<String>, none: &str| {
                key.as_ref().and_then(|k| info.fields.iter().find(|(fk, _)| fk == k)).map_or_else(|| none.to_string(), |(_, t)| t.clone())
            };
            let sort_key = st.sort.as_ref().map(|(k, _)| k.clone());
            let sort_text = match &st.sort {
                Some((_, desc)) => {
                    format!("{} ({})", title_of(&sort_key, st.t("tb.outline")), if *desc { st.t("tb.desc") } else { st.t("tb.asc") })
                }
                None => st.t("tb.outline").to_string(),
            };
            let group_text = title_of(&st.group, st.t("tb.no_group"));

            let sort_width = craft::caps(
                ui.painter(),
                crate::lang::text("tb.sort"),
                theme::HUD_SIZE,
                theme::mono(),
                theme::HUD_SPACING,
                theme::pal().TEXT_WEAK,
            )
            .size()
            .x + ITEM_GAP
                + 120.0;
            let group_width = craft::caps(
                ui.painter(),
                crate::lang::text("tb.group"),
                theme::HUD_SIZE,
                theme::mono(),
                theme::HUD_SPACING,
                theme::pal().TEXT_WEAK,
            )
            .size()
            .x + ITEM_GAP
                + 110.0;
            const SEARCH_W: f32 = 130.0;
            let trailing = vec![
                icon_cmd(
                    "replace",
                    "tb.find",
                    tip(Command::Find(false), "Find and replace in the task table"),
                    doc,
                    false,
                    Command::Find(false),
                ),
                icon_cmd("arrow-right", "tb.goto", tip(Command::GoTo, "Go to a task ID or a date"), doc, false, Command::GoTo),
                Entry::Sep,
                icon_cmd("fold-vertical", "tb.collapse_all", "Collapse all summaries".into(), doc, false, Command::CollapseAll),
                icon_cmd("unfold-vertical", "tb.expand_all", "Expand all summaries".into(), doc, false, Command::ExpandAll),
                Entry::Sep,
                icon_cmd("zoom-in", "tb.zoom_in", tip(Command::ZoomIn, "Zoom in"), doc, false, Command::ZoomIn),
                icon_cmd("zoom-out", "tb.zoom_out", tip(Command::ZoomOut, "Zoom out"), doc, false, Command::ZoomOut),
                cmd(
                    "maximize",
                    Some(crate::lang::text("tb.entire")),
                    "Fit the whole project in the chart".into(),
                    doc,
                    false,
                    Command::ZoomEntire,
                ),
                cmd(
                    "crosshair",
                    Some(crate::lang::text("tb.selected")),
                    "Fit the selected tasks".into(),
                    sel,
                    false,
                    Command::ZoomSelected,
                ),
                Entry::Sep,
                toggle_cmd(
                    "calendar",
                    Some(crate::lang::text("tb.timeline")),
                    "Show the timeline strip".into(),
                    doc,
                    st.timeline_visible,
                    false,
                    Command::ToggleTimelinePane,
                ),
                toggle_cmd(
                    "panel-right",
                    Some(crate::lang::text("tb.properties")),
                    "Show the properties pane".into(),
                    doc,
                    st.properties_visible,
                    false,
                    Command::TogglePropertiesPane,
                ),
                toggle_cmd(
                    "file-text",
                    Some(crate::lang::text("tb.wrap")),
                    "Wrap long text in table cells".into(),
                    true,
                    st.word_wrap,
                    false,
                    Command::ToggleWordWrap,
                ),
                toggle_cmd(
                    "history",
                    Some(crate::lang::text("tb.history")),
                    "Undo history".into(),
                    doc,
                    info.history_open,
                    false,
                    Command::History,
                ),
            ];
            let mut chips: Vec<ToolChip> = vec![
                ToolChip { id: "sort", width: sort_width, primary: true },
                ToolChip { id: "group", width: group_width, primary: true },
                ToolChip { id: "search", width: SEARCH_W, primary: false },
            ];
            let entry_base = chips.len();
            chips.extend(trailing.iter().filter_map(|e| e.chip(ui)));
            let timescale_idx = chips.len();
            chips.push(ToolChip { id: "timescale", width: TIMESCALE_W, primary: true });
            let more_w = more_reserve_width(ui);
            let row_budget = (ui.available_width() - more_w).max(0.0);
            let (visible, overflow) = partition_chips(&chips, row_budget, 0.0);
            ui.spacing_mut().item_spacing.x = ITEM_GAP;

            let paint_sort = |ui: &mut Ui, st: &UiState, info: &MenuInfo, push: &dyn Fn(Command), sort_text: &str| {
                combo_label(ui, crate::lang::text("tb.sort"));
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
            };
            let paint_group = |ui: &mut Ui, st: &UiState, info: &MenuInfo, push: &dyn Fn(Command), group_text: &str| {
                combo_label(ui, crate::lang::text("tb.group"));
                egui::ComboBox::from_id_salt("tb_group").width(110.0).selected_text(group_text).show_ui(ui, |ui| {
                    if ui.selectable_label(st.group.is_none(), st.t("tb.no_group")).clicked() {
                        push(Command::GroupBy(None));
                    }
                    for (key, title) in &info.fields {
                        if ui.selectable_label(st.group.as_ref() == Some(key), title).clicked() {
                            push(Command::GroupBy(Some(key.clone())));
                        }
                    }
                });
            };

            for &i in &visible {
                if i == 0 {
                    paint_sort(ui, st, info, push, &sort_text);
                } else if i == 1 {
                    paint_group(ui, st, info, push, &group_text);
                } else if i == 2 {
                    ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(130.0).hint_text(crate::lang::text("tb.search")));
                } else if i == timescale_idx {
                    timescale_chips(ui, st, info, push);
                } else if i >= entry_base
                    && i < timescale_idx
                    && let Some(e) = trailing.get(i - entry_base)
                {
                    e.paint(ui, push);
                }
            }
            more_menu(ui, |ui| {
                for &i in &overflow {
                    if i == 0 {
                        paint_sort(ui, st, info, push, &sort_text);
                    } else if i == 1 {
                        paint_group(ui, st, info, push, &group_text);
                    } else if i == 2 {
                        ui.label(egui::RichText::new(crate::lang::text("tb.search")).small().color(theme::pal().TEXT_WEAK));
                        ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(180.0).hint_text(crate::lang::text("tb.search")));
                    } else if i == timescale_idx {
                        ui.label(egui::RichText::new(crate::lang::text("tb.timescale")).small().color(theme::pal().TEXT_WEAK));
                        timescale_chips(ui, st, info, push);
                    } else if i >= entry_base
                        && i < timescale_idx
                        && let Some(e) = trailing.get(i - entry_base)
                    {
                        e.paint_overflow_row(ui, push);
                    }
                }
                if !overflow.is_empty() {
                    ui.separator();
                }
                view_settings_more(ui, st, settings, push);
            });
        }
        MenuTab::Format => {
            let toggle = |ui: &mut Ui, icon: &str, label: &str, tip: &str, on: bool, cmd: Command| {
                if button(ui, icon, Some(label), tip, doc, on, false, true).clicked() {
                    push(cmd);
                }
            };
            if st.view == ViewKind::NetworkDiagram {
                let n = &st.network;
                let entries = [
                    toggle_cmd(
                        "triangle-alert",
                        Some(crate::lang::text("fmt.critical_first")),
                        "Lay out the critical path on the top row".into(),
                        doc,
                        n.opts.critical_first,
                        true,
                        Command::ToggleNetworkCriticalFirst,
                    ),
                    toggle_cmd(
                        "rows-3",
                        Some(crate::lang::text("fmt.summaries")),
                        "Show summary tasks, left of their subtasks".into(),
                        doc,
                        n.show_summaries,
                        false,
                        Command::ToggleNetworkSummaries,
                    ),
                    Entry::Sep,
                    toggle_cmd(
                        "link",
                        Some(crate::lang::text("fmt.link_labels")),
                        "Show the link type and lag on each line".into(),
                        doc,
                        n.link_labels,
                        false,
                        Command::ToggleNetworkLinkLabels,
                    ),
                    toggle_cmd(
                        "gauge",
                        Some(crate::lang::text("fmt.marks")),
                        "One diagonal through tasks in progress, an X through complete tasks".into(),
                        doc,
                        n.progress_marks,
                        false,
                        Command::ToggleNetworkProgressMarks,
                    ),
                    toggle_cmd(
                        "hash",
                        Some(crate::lang::text("fmt.ids")),
                        "Small boxes that show only the task ID".into(),
                        doc,
                        n.opts.ids_only,
                        false,
                        Command::ToggleNetworkIdsOnly,
                    ),
                    Entry::Sep,
                    cmd(
                        "maximize",
                        Some(crate::lang::text("tb.fit")),
                        "Zoom to show the whole diagram".into(),
                        doc,
                        false,
                        Command::ZoomEntire,
                    ),
                ];
                // Fit is a local flag on the network view; keep the old click behaviour.
                let chips: Vec<ToolChip> = entries.iter().filter_map(|e| e.chip(ui)).collect();
                let (visible, overflow) = partition_chips(&chips, ui.available_width(), more_reserve_width(ui));
                ui.spacing_mut().item_spacing.x = ITEM_GAP;
                for &i in &visible {
                    if i + 1 == entries.len() {
                        if button(
                            ui,
                            "maximize",
                            Some(crate::lang::text("tb.fit")),
                            "Zoom to show the whole diagram",
                            doc,
                            false,
                            false,
                            false,
                        )
                        .clicked()
                        {
                            st.network.fit_pending = true;
                        }
                    } else if let Some(e) = entries.get(i) {
                        e.paint(ui, push);
                    }
                }
                if !overflow.is_empty() {
                    more_menu(ui, |ui| {
                        for &i in &overflow {
                            if i + 1 == entries.len() {
                                if ui.button(crate::lang::text("tb.fit")).clicked() {
                                    st.network.fit_pending = true;
                                    ui.close();
                                }
                                continue;
                            }
                            let Some(e) = entries.get(i) else { continue };
                            e.paint_overflow_row(ui, push);
                        }
                    });
                }
                return;
            }
            let g = &st.gantt;
            if st.view == ViewKind::MultipleBaselines {
                combo_label(ui, crate::lang::text("fmt.bars_of"));
                let shown: Vec<String> =
                    crate::views::gantt_chart::baseline_slots(g.multi_baselines).map(|n| PlanSlot::Baseline(n).name()).collect();
                let title = if shown.is_empty() { "no baseline".to_string() } else { shown.join(", ") };
                let popup_id = ui.make_persistent_id("tb_multi_baselines_popup");
                let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                if !doc && open {
                    egui::Popup::close_id(ui.ctx(), popup_id);
                }
                let btn = ui
                    .add_enabled(doc, egui::Button::new(title).selected(open).min_size(vec2(0.0, CONTROL_H - 6.0)))
                    .on_hover_text("Which baselines to compare, one bar each");
                egui::Popup::from_toggle_button_response(&btn)
                    .id(popup_id)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        for n in 0..=plancraft_core::baseline::MAX_BASELINE {
                            let bit = 1u16 << n;
                            let mut on = g.multi_baselines & bit != 0;
                            if ui.checkbox(&mut on, PlanSlot::Baseline(n).name()).changed() {
                                push(Command::ToggleMultiBaseline(n));
                            }
                        }
                    });
                sep(ui);
                toggle(
                    ui,
                    "rows-3",
                    crate::lang::text("fmt.summary"),
                    "Show the project summary bar",
                    g.show_project_summary,
                    Command::ToggleShowProjectSummary,
                );
                return;
            }
            // One plan for every Format toggle; reserve the baseline slot combo after the strip.
            let entries = vec![
                toggle_cmd(
                    "triangle-alert",
                    Some(crate::lang::text("fmt.critical")),
                    "Highlight critical tasks".into(),
                    doc,
                    g.show_critical,
                    true,
                    Command::ToggleShowCritical,
                ),
                toggle_cmd(
                    "route",
                    Some(crate::lang::text("fmt.paths")),
                    tip(Command::ToggleHighlightPaths, "Highlight the selected task's predecessors and successors"),
                    doc,
                    g.show_paths,
                    true,
                    Command::ToggleHighlightPaths,
                ),
                toggle_cmd(
                    "hourglass",
                    Some(crate::lang::text("fmt.slack")),
                    "Show total slack after each task".into(),
                    doc,
                    g.show_slack,
                    false,
                    Command::ToggleShowSlack,
                ),
                toggle_cmd(
                    "ruler",
                    Some(crate::lang::text("fmt.baseline")),
                    "Show baseline bars (the Tracking Gantt always does)".into(),
                    doc,
                    g.show_baseline,
                    false,
                    Command::ToggleShowBaseline,
                ),
                Entry::Sep,
                toggle_cmd(
                    "gauge",
                    Some(crate::lang::text("fmt.progress")),
                    "Show progress inside bars".into(),
                    doc,
                    g.show_progress,
                    false,
                    Command::ToggleShowProgress,
                ),
                toggle_cmd(
                    "link",
                    Some(crate::lang::text("fmt.links")),
                    "Show dependency arrows".into(),
                    doc,
                    g.show_links,
                    false,
                    Command::ToggleShowLinks,
                ),
                toggle_cmd(
                    "user-round",
                    Some(crate::lang::text("fmt.names")),
                    "Show resource names beside bars".into(),
                    doc,
                    g.show_resource_names,
                    false,
                    Command::ToggleShowResourceNames,
                ),
                toggle_cmd(
                    "rows-3",
                    Some(crate::lang::text("fmt.summary")),
                    "Show the project summary bar".into(),
                    doc,
                    g.show_project_summary,
                    false,
                    Command::ToggleShowProjectSummary,
                ),
            ];
            let combo_reserve = BASELINE_COMBO_W + ITEM_GAP;
            paint_entries_reserving(ui, &entries, push, combo_reserve);
            egui::ComboBox::from_id_salt("tb_baseline_slot")
                .width(BASELINE_COMBO_W)
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
    child.set_height(TOOL_ROW_H);
    child.spacing_mut().item_spacing.x = ITEM_GAP;
    child.with_layout(Layout::left_to_right(Align::Center), |ui| toolbar(ui, st, info, settings, &push));

    view_selector(ui, selector, st, info.has_doc, &push);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    out.extend(cmds.into_inner());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_priority_keeps_task_and_milestone_neutral() {
        for choice in [theme::ThemeChoice::Light, theme::ThemeChoice::Dark] {
            let ctx = egui::Context::default();
            theme::install(&ctx, choice);
            let mut out = ctx.run_ui(Default::default(), |ui| {
                ui.horizontal(|ui| {
                    button(ui, "plus", Some("Task"), "Task", true, false, true, false);
                    button(ui, "diamond", Some("Milestone"), "Milestone", true, false, true, false);
                    button(ui, "folder", Some("Summary"), "Summary", true, false, false, false);
                });
            });
            let inverted =
                out.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == theme::pal().INVERT_BG));
            out.textures_delta.clear();
            assert!(!inverted, "toolbar overflow priority must not give Task/Milestone a special inverted background in {choice:?}");
        }
    }

    #[test]
    fn artcraft_toolbar_metrics() {
        assert_eq!(CONTROL_H, 34.0);
        assert_eq!(CORNER, 3.0);
        assert_eq!(ITEM_GAP, 4.0);
        assert_eq!(TITLE_H, 40.0);
        assert_eq!(TOOL_ROW_H, 34.0);
        assert_eq!(theme::TITLE_BAND_LABEL_SIZE, 16.0);
        assert_eq!(theme::TITLE_BAND_QA_SIZE, 28.0);
    }

    #[test]
    fn timescale_width_follows_chip_count() {
        let n = crate::state::Timescale::ALL.len();
        assert_eq!(n, 4, "Q M W D");
        assert_eq!(TIMESCALE_W, 28.0 * n as f32 + 1.0 * (n.saturating_sub(1)) as f32);
    }

    #[test]
    fn partition_keeps_every_chip_visible_or_in_more() {
        let chips = vec![
            ToolChip { id: "a", width: 80.0, primary: true },
            ToolChip { id: "b", width: 80.0, primary: true },
            ToolChip { id: "c", width: 200.0, primary: false },
            ToolChip { id: "d", width: 200.0, primary: false },
            ToolChip { id: "e", width: 200.0, primary: false },
        ];
        let (vis, more) = partition_chips(&chips, 300.0, MORE_RESERVE_FALLBACK);
        assert!(!more.is_empty());
        assert!(vis.iter().any(|&i| chips[i].id == "a"));
        let mut all = vis;
        all.extend(more);
        all.sort_unstable();
        assert_eq!(all, (0..chips.len()).collect::<Vec<_>>());
    }

    #[test]
    fn partition_skips_more_reserve_when_everything_fits() {
        let chips = vec![ToolChip { id: "a", width: 40.0, primary: true }, ToolChip { id: "b", width: 40.0, primary: false }];
        let (vis, more) = partition_chips(&chips, 200.0, MORE_RESERVE_FALLBACK);
        assert!(more.is_empty());
        assert_eq!(vis.len(), 2);
    }

    #[test]
    fn more_en_nl_and_overflow_caption_keys_exist() {
        for key in ["tb.more", "tb.more_hint", "tb.paste", "tb.cut", "tb.copy", "tb.delete", "tb.find", "tb.timescale", "tb.properties"] {
            assert_ne!(crate::lang::tr(crate::lang::Lang::En, key), key, "{key} en");
            assert_ne!(crate::lang::tr(crate::lang::Lang::Nl, key), key, "{key} nl");
        }
        assert_eq!(crate::lang::tr(crate::lang::Lang::En, "tb.more"), "More");
        assert_eq!(crate::lang::tr(crate::lang::Lang::Nl, "tb.more"), "Meer");
        assert_eq!(crate::lang::tr(crate::lang::Lang::Nl, "tb.paste"), "Plakken");
    }
}
