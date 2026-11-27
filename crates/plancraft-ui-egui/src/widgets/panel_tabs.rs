//! PhotoCraft-style **panel** tab strip chrome for side inspectors.
//!
//! Structure and fit rules follow craft-ui `TabKind::Panel` (storytold/craft-libs @ 77c7c623):
//! - `crates/ui-widgets/src/tabs/rules.rs` — shrink before overflow, keep active width,
//!   overflow menu lists **hidden** tabs only, overflow sits after the last shown tab.
//!   The active tab carries a small close cross so the inspector can be dismissed.
//! - `crates/ui-widgets/src/tabs/tab.rs` — selected tab fills panel surface; idle tabs are
//!   muted labels on the strip (no document-style dividers).
//! - `crates/ui-panels/src/docking/group.rs` — dock groups use
//!   `TabStrip::kind(TabKind::Panel)` when tabs are not closable.
//!
//! Live `TabStrip` lives behind craft-ui’s component runtime (`StyleCx` / `ShowCollection`).
//! PlanCraft pins egui 0.37 + `craft-window` / `craft-ui-shell` for the title bar; panel
//! docking/`TabStrip` are not wired as deps yet, so this module mirrors `TabKind::Panel`
//! structure and tokens for side inspectors.

use egui::{Align, FontFamily, FontId, Layout, PopupCloseBehavior, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme;

/// Height of a panel tab strip (craft `size.panel_header` ≈ 28 pt compact).
pub const PANEL_TAB_H: f32 = 28.0;
/// Minimum width of a shrunk panel tab (first letters; craft `MIN_LETTERS` room).
const MIN_TAB_W: f32 = 36.0;
const OVERFLOW_W: f32 = 28.0;
const TAB_PAD_X: f32 = 10.0;
/// Room for the little close cross on the active tab.
const CLOSE_W: f32 = 16.0;

/// One panel tab: stable id + label.
#[derive(Clone, Debug)]
pub struct PanelTab {
    pub id: &'static str,
    pub label: String,
}

/// Result of drawing a panel tab strip.
#[derive(Clone, Debug, Default)]
pub struct PanelTabOut {
    /// Tab id clicked this pass (including from the overflow menu).
    pub picked: Option<&'static str>,
    /// The close cross on the active tab was clicked.
    pub close: bool,
}

/// Draws a PhotoCraft-family panel tab strip: active tab on a lighter panel fill, idle muted,
/// `…` after shown tabs listing only the tabs that did not fit (`OverflowMenu::HiddenOnly`).
pub fn show(ui: &mut Ui, tabs: &[PanelTab], active_id: &str) -> PanelTabOut {
    let mut out = PanelTabOut::default();
    if tabs.is_empty() {
        return out;
    }
    let available = ui.available_width();
    let row = ui.available_rect_before_wrap();
    let strip = Rect::from_min_size(row.min, vec2(available.max(0.0), PANEL_TAB_H));
    ui.painter().rect_filled(strip, 0.0, theme::pal().BG);
    ui.painter().hline(strip.x_range(), strip.bottom() - 0.5, theme::hairline());

    let font = FontId::new(12.0, FontFamily::Proportional);
    let naturals: Vec<f32> = tabs
        .iter()
        .map(|t| {
            let g = ui.painter().layout_no_wrap(t.label.clone(), font.clone(), theme::pal().TEXT);
            let close = if t.id == active_id { CLOSE_W } else { 0.0 };
            (g.size().x + 2.0 * TAB_PAD_X + close).max(MIN_TAB_W)
        })
        .collect();
    let selected = tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
    let (shown, hidden) = fit_panel_tabs(&naturals, selected, available, OVERFLOW_W);

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip).layout(Layout::left_to_right(Align::Center)));
    child.spacing_mut().item_spacing.x = 0.0;

    for &i in &shown {
        let Some(tab) = tabs.get(i) else { continue };
        let active = tab.id == active_id;
        let w = *naturals.get(i).unwrap_or(&MIN_TAB_W);
        let (rect, resp) = child.allocate_exact_size(vec2(w, PANEL_TAB_H), Sense::click());
        if active {
            child.painter().rect_filled(rect, 0.0, theme::pal().PANEL);
        } else if resp.hovered() {
            child.painter().rect_filled(rect, 0.0, theme::pal().WASH_HOVER);
        }
        let color = if active { theme::pal().TEXT } else { theme::pal().TEXT_WEAK };
        let galley = child.painter().layout_no_wrap(tab.label.clone(), font.clone(), color);
        let label_right = if active { rect.right() - CLOSE_W } else { rect.right() - 2.0 };
        // Panel tabs clip labels at the edge (`TabKind::clips_label`), not ellipsis.
        let clip = Rect::from_min_max(egui::pos2(rect.left() + 2.0, rect.top()), egui::pos2(label_right, rect.bottom()));
        child.painter().with_clip_rect(clip).galley(
            egui::pos2(rect.left() + TAB_PAD_X, rect.center().y - galley.size().y * 0.5),
            galley,
            color,
        );
        let closed = if active { close_cross(&mut child, rect, tab.id) } else { false };
        if closed {
            out.close = true;
        } else if resp.clicked() {
            out.picked = Some(tab.id);
        }
        resp.on_hover_text(&tab.label);
    }

    if !hidden.is_empty() {
        let more = child.add_sized(vec2(OVERFLOW_W, PANEL_TAB_H), egui::Button::new("…").frame(false));
        egui::Popup::from_toggle_button_response(&more).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(120.0);
            // OverflowMenu::HiddenOnly — only tabs that did not fit.
            for &i in &hidden {
                let Some(tab) = tabs.get(i) else { continue };
                if ui.selectable_label(false, &tab.label).clicked() {
                    out.picked = Some(tab.id);
                    ui.close();
                }
            }
        });
    }

    ui.allocate_rect(strip, Sense::hover());
    out
}

/// A small cross at the right of the active tab. Returns whether it was clicked.
fn close_cross(ui: &mut Ui, tab: Rect, tab_id: &str) -> bool {
    let cross = Rect::from_center_size(pos2(tab.right() - CLOSE_W * 0.5 - 1.0, tab.center().y), vec2(CLOSE_W - 2.0, CLOSE_W - 2.0));
    let resp = ui.interact(cross, ui.scope_id().with(("panel_tab_close", tab_id)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(cross, 2.0, theme::pal().WASH_HOVER);
    }
    let color = if resp.hovered() { theme::pal().TEXT } else { theme::pal().TEXT_WEAK };
    let mark = cross.shrink(3.5);
    let stroke = Stroke::new(1.15, color);
    ui.painter().line_segment([mark.left_top(), mark.right_bottom()], stroke);
    ui.painter().line_segment([mark.right_top(), mark.left_bottom()], stroke);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, crate::lang::text("dlg.close")));
    resp.on_hover_text(crate::lang::text("dlg.close")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Same fit idea as craft `FitMode::Shrink { keep_selected: true }`: keep the active tab's
/// natural width, shrink others toward `MIN_TAB_W`, then overflow trailing tabs.
fn fit_panel_tabs(natural: &[f32], selected: usize, available: f32, overflow_w: f32) -> (Vec<usize>, Vec<usize>) {
    let n = natural.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    if natural.iter().sum::<f32>() <= available {
        return ((0..n).collect(), Vec::new());
    }
    let budget = (available - overflow_w).max(0.0);
    let mut widths = natural.to_vec();
    let mut used: f32 = widths.iter().sum();
    while used > budget {
        let mut shrunk = false;
        for (i, w) in widths.iter_mut().enumerate() {
            if i == selected {
                continue;
            }
            if *w > MIN_TAB_W {
                let step = (*w - MIN_TAB_W).min(used - budget).min(8.0);
                *w -= step;
                used -= step;
                shrunk = true;
                if used <= budget {
                    break;
                }
            }
        }
        if !shrunk {
            break;
        }
    }
    let mut shown: Vec<usize> = (0..n).collect();
    let mut hidden = Vec::new();
    while shown.len() > 1 {
        let used_now: f32 = shown.iter().filter_map(|&i| widths.get(i).copied()).sum();
        if used_now <= budget {
            break;
        }
        let drop_at = shown.iter().rposition(|&i| i != selected).unwrap_or(shown.len() - 1);
        if let Some(i) = shown.get(drop_at).copied() {
            shown.remove(drop_at);
            hidden.insert(0, i);
        } else {
            break;
        }
    }
    (shown, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_fit_when_roomy() {
        let (shown, hidden) = fit_panel_tabs(&[40.0, 40.0, 40.0], 0, 200.0, OVERFLOW_W);
        assert_eq!(shown, vec![0, 1, 2]);
        assert!(hidden.is_empty());
    }

    #[test]
    fn keeps_selected_when_overflowing() {
        let (shown, hidden) = fit_panel_tabs(&[80.0, 80.0, 80.0, 80.0, 80.0], 2, 120.0, OVERFLOW_W);
        assert!(shown.contains(&2), "active stays: {shown:?}");
        assert!(!hidden.is_empty());
        assert!(!hidden.contains(&2));
    }
}
