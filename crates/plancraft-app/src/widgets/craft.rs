//! ArtCraft-style building blocks: upper-case, letter-spaced mono labels (the HUD texture).
//!
//! The caps conversion and the letter-spacing happen here, at render time — the EN/NL strings
//! in `lang.rs` stay normal-case.

use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Galley, Response, Sense, Ui, pos2, vec2};

use crate::theme;

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

/// A section heading: 11 px Geist Mono 650, upper case, tracked a little tighter.
pub fn section_label(ui: &mut Ui, text: &str) -> Response {
    let color = theme::pal().TEXT_WEAK;
    let galley = caps(ui.painter(), text, theme::HUD_SIZE, theme::mono_bold(), theme::LABEL_SPACING, color);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    resp
}
