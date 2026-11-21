//! ArtCraft-style building blocks: upper-case, letter-spaced mono labels (the HUD texture).
//!
//! The caps conversion and the letter-spacing happen here, at render time — the EN/NL strings
//! in `lang.rs` stay normal-case.

use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Galley, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::theme;

/// Shared height for toolbar / title-band controls (ArtCraft compact row).
pub const CONTROL_H: f32 = 34.0;
/// Corner radius for wash / inverted chips.
pub const CORNER: f32 = theme::CONTROL_RADIUS as f32;
/// Gap between adjacent toolbar controls.
pub const ITEM_GAP: f32 = 4.0;

fn tool_chrome(
    surface: Color32,
    enabled: bool,
    active: bool,
    invert: bool,
    pressed: bool,
    hovered: bool,
    open: bool,
) -> theme::ControlChrome {
    let role = if invert { theme::ControlRole::Primary } else { theme::ControlRole::Ghost };
    theme::control_chrome(surface, role, theme::ControlState { enabled, selected: active, hovered, pressed, open })
}

/// `text` laid out upper case in `family`, `spacing` points between letters.
pub fn caps(p: &egui::Painter, text: &str, size: f32, family: FontFamily, spacing: f32, color: Color32) -> Arc<Galley> {
    p.layout_job(caps_job(text, size, family, spacing, color))
}

fn caps_job(text: &str, size: f32, family: FontFamily, spacing: f32, color: Color32) -> egui::text::LayoutJob {
    let format = egui::TextFormat { font_id: FontId::new(size, family), color, extra_letter_spacing: spacing, ..Default::default() };
    egui::text::LayoutJob::single_section(text.to_uppercase(), format)
}

/// A HUD label as a layout job: 11 px Geist Mono, upper case, widely spaced. Fits anywhere
/// a `WidgetText` goes (labels, buttons), so a segment can also be clickable.
pub fn hud_job(text: &str, color: Color32) -> egui::text::LayoutJob {
    caps_job(text, theme::HUD_SIZE, theme::mono(), theme::HUD_SPACING, color)
}

/// A `.hud-label`: 11 px Geist Mono, upper case, widely spaced.
pub fn hud(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = caps(ui.painter(), text, theme::HUD_SIZE, theme::mono(), theme::HUD_SPACING, color);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    resp
}

/// Estimated width of a toolbar control (icon, optional HUD label).
pub fn tool_width(ui: &Ui, label: Option<&str>) -> f32 {
    let label_w = label
        .map(|l| ui.painter().layout_job(caps_job(l, theme::HUD_SIZE, theme::mono(), theme::HUD_SPACING, theme::pal().TEXT)).size().x + 8.0)
        .unwrap_or(0.0);
    28.0 + label_w + if label.is_some() { 6.0 } else { 0.0 }
}

pub fn paint_focus_ring(ui: &Ui, rect: Rect, focused: bool) {
    if focused {
        ui.painter().rect_stroke(rect.shrink(0.5), CORNER, Stroke::new(1.0, theme::pal().ACCENT), StrokeKind::Inside);
    }
}

/// Ghost toolbar button: wash hover/press, optional HUD label.
///
/// `primary` inverts (not accent blue) for one-shot actions. For `toggle` controls, `primary` is
/// overflow-priority only — invert would mask the persistent on/off wash — and `active` drives both
/// the selected chrome and the accessible toggled state.
pub fn tool_button(
    ui: &mut Ui,
    icon: &str,
    label: Option<&str>,
    tip: &str,
    enabled: bool,
    active: bool,
    primary: bool,
    toggle: bool,
) -> Response {
    tool_button_on(ui, icon, label, tip, enabled, active, primary, toggle, false, theme::pal().PANEL)
}

pub fn tool_button_on(
    ui: &mut Ui,
    icon: &str,
    label: Option<&str>,
    tip: &str,
    enabled: bool,
    active: bool,
    primary: bool,
    toggle: bool,
    open: bool,
    surface: Color32,
) -> Response {
    let enabled = enabled && ui.is_enabled();
    let w = tool_width(ui, label);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, CONTROL_H), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    // Toggles never use invert fill: callers may still mark them `primary` so overflow keeps them.
    let invert = primary && enabled && !toggle;
    let chrome = tool_chrome(surface, enabled, active, invert, resp.is_pointer_button_down_on(), resp.hovered(), open);
    if let Some(fill) = chrome.fill {
        p.rect_filled(rect, CORNER, fill);
    }
    if let Some(stroke) = chrome.stroke {
        p.rect_stroke(rect, CORNER, Stroke::new(1.0, stroke), StrokeKind::Inside);
    }
    paint_focus_ring(ui, rect, enabled && resp.has_focus());
    let icon_color = if !enabled {
        theme::pal().LINE_STRONG
    } else if invert {
        theme::pal().INVERT_FG
    } else if active || open || resp.hovered() {
        theme::pal().TEXT
    } else {
        theme::pal().TEXT_WEAK
    };
    let center = pos2(rect.left() + 14.0, rect.center().y);
    crate::icons::paint(ui, egui::Rect::from_center_size(center, vec2(14.0, 14.0)), icon, 14.0, icon_color);
    if let Some(l) = label {
        let color = if !enabled {
            theme::pal().TEXT_WEAK
        } else if invert {
            theme::pal().INVERT_FG
        } else {
            theme::pal().TEXT
        };
        let galley = caps(p, l, theme::HUD_SIZE, theme::mono(), theme::HUD_SPACING, color);
        p.galley(pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0), galley, color);
    }
    // Icon-only controls keep a short accessible name (drop a trailing " (Cmd+S)" hint).
    let name = label.map_or_else(
        || match tip.split_once(" (") {
            Some((head, _)) if !head.is_empty() => head.to_string(),
            _ => tip.to_string(),
        },
        |l| l.to_string(),
    );
    resp.widget_info(move || {
        if toggle {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, &name)
        } else {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name)
        }
    });
    let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// An ArtCraft dialog header for a window with `.title_bar(false)`: the title left-aligned in
/// caps mono, extra controls and a Lucide close button at the right edge, a hairline below.
/// Returns `false` when the close button was clicked. The window stays movable by its background.
pub fn dialog_header(ui: &mut Ui, title: &str) -> bool {
    dialog_header_with(ui, title, |_| {})
}

/// [`dialog_header`] with extra right-side controls (drawn right-to-left, after the close button).
pub fn dialog_header_with(ui: &mut Ui, title: &str, extra: impl FnOnce(&mut Ui)) -> bool {
    let mut keep = true;
    ui.horizontal(|ui| {
        let color = theme::pal().TEXT;
        let galley = caps(ui.painter(), title, theme::HUD_SIZE + 1.0, theme::mono_bold(), theme::LABEL_SPACING, color);
        // No widget_info: the window itself already exposes the title to accesskit.
        let (rect, _resp) = ui.allocate_exact_size(vec2(galley.size().x, 22.0), Sense::hover());
        ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, color);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::icons::button(ui, "x", 18.0, crate::lang::text("dlg.close")).clicked() {
                keep = false;
            }
            extra(ui);
        });
    });
    let y = ui.cursor().top() + 2.0;
    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, theme::pal().LINE));
    ui.add_space(6.0);
    keep
}

/// A section heading: 11 px Geist Mono 650, upper case, tracked a little tighter.
pub fn section_label(ui: &mut Ui, text: &str) -> Response {
    let color = theme::pal().TEXT_WEAK;
    let galley = caps(ui.painter(), text, theme::HUD_SIZE, theme::mono_bold(), theme::LABEL_SPACING, color);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    resp
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    #[test]
    fn artcraft_control_metrics() {
        assert_eq!(super::CONTROL_H, 34.0);
        assert_eq!(super::CORNER, 3.0);
        assert_eq!(super::ITEM_GAP, 4.0);
    }

    #[test]
    fn toolbar_chrome_precedence_contract() {
        assert_eq!(
            crate::theme::composite_over(egui::Color32::from_white_alpha(128), egui::Color32::BLACK),
            egui::Color32::from_gray(128),
            "premultiplied Color32 channels must not be multiplied by alpha twice",
        );
        let disabled_active = super::tool_chrome(crate::theme::pal().PANEL, false, true, false, false, false, false);
        assert!(disabled_active.fill.is_none());
        assert!(disabled_active.stroke.is_none());

        // Primary hover/press stay in the inverted role with enough contrast in both palettes.
        let luminance = |c: Color32| {
            let ch = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        };
        let contrast = |fg: Color32, bg: Color32| {
            let (x, y) = (luminance(fg), luminance(bg));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        for (name, p) in [("light", &crate::theme::LIGHT), ("dark", &crate::theme::DARK)] {
            let hover = crate::theme::control_chrome_with_palette(
                p,
                p.PANEL,
                crate::theme::ControlRole::Primary,
                crate::theme::ControlState { enabled: true, selected: false, hovered: true, pressed: false, open: false },
            );
            let press = crate::theme::control_chrome_with_palette(
                p,
                p.PANEL,
                crate::theme::ControlRole::Primary,
                crate::theme::ControlState { enabled: true, selected: false, hovered: true, pressed: true, open: false },
            );
            let hover_fill = hover.fill.expect("hover fill");
            let press_fill = press.fill.expect("press fill");
            assert!(contrast(p.INVERT_FG, hover_fill) >= 4.5, "{name}: primary hover label contrast");
            assert!(contrast(p.INVERT_FG, press_fill) >= 4.5, "{name}: primary press label contrast");
        }

        let selected_idle = super::tool_chrome(crate::theme::pal().PANEL, true, true, false, false, false, false);
        let selected_hover = super::tool_chrome(crate::theme::pal().PANEL, true, true, false, false, true, false);
        assert_ne!(selected_idle.fill, selected_hover.fill, "selected toggles keep a hover delta");
        assert_eq!(selected_hover.stroke, Some(crate::theme::pal().LINE_STRONG));

        // Shared resolver: custom and native hover/down tokens agree on the strong border.
        for p in [&crate::theme::LIGHT, &crate::theme::DARK] {
            let native_hover = crate::theme::control_chrome_with_palette(
                p,
                p.CONTROLS,
                crate::theme::ControlRole::Native,
                crate::theme::ControlState { enabled: true, selected: false, hovered: true, pressed: false, open: false },
            );
            let custom_hover = crate::theme::control_chrome_with_palette(
                p,
                p.PANEL,
                crate::theme::ControlRole::Ghost,
                crate::theme::ControlState { enabled: true, selected: false, hovered: true, pressed: false, open: false },
            );
            assert_eq!(native_hover.stroke, custom_hover.stroke);
        }

        // Global typography/margins are installed once and inherited by dialog windows.
        let ctx = egui::Context::default();
        crate::theme::install(&ctx, crate::theme::ThemeChoice::Dark);
        let style = ctx.style_of(egui::Theme::Dark);
        let body = style.text_styles.get(&egui::TextStyle::Body).cloned();
        assert_eq!(body, Some(egui::FontId::new(13.0, egui::FontFamily::Proportional)));
        assert_eq!(style.spacing.window_margin, egui::Margin::same(crate::theme::DIALOG_MARGIN));

        // Nested disabled scope must suppress an otherwise-enabled custom button.
        let mut enabled_in_scope = true;
        let mut clicked_in_scope = true;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.add_enabled_ui(false, |ui| {
                let resp = super::tool_button(ui, "plus", Some("Task"), "Task", true, false, false, false);
                enabled_in_scope = resp.enabled();
                clicked_in_scope = resp.clicked();
            });
        });
        out.textures_delta.clear();
        assert!(!enabled_in_scope);
        assert!(!clicked_in_scope);
    }
}
