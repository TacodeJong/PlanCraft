//! Lucide icons (ISC license, `assets/icons/LICENSE-lucide.txt`): white-stroke SVGs embedded in
//! the binary, rasterized by `egui_extras` at the exact pixel size they are shown at and tinted
//! per use (idle icons in the weak ink tier, full ink on hover, status colors where they mean
//! something). PlanCraft's painted shapes stay painted: the milestone diamond, the row-mark dot
//! and the summary bracket are geometry, not icons.

use egui::{Color32, Rect, Ui, Vec2};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        /// Every bundled icon, by its Lucide name.
        static ICONS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../assets/icons/", $name, ".svg")) as &[u8])),*
        ];
    };
}

icons![
    "archive",
    "arrow-down",
    "arrow-right",
    "arrow-up",
    "book-open",
    "calendar",
    "calendar-days",
    "chart-column",
    "chart-gantt",
    "chart-line",
    "check",
    "chevron-left",
    "chevron-right",
    "circle",
    "circle-dot",
    "clipboard-list",
    "clipboard-paste",
    "copy",
    "crosshair",
    "diamond",
    "file-down",
    "file-output",
    "file-text",
    "flag",
    "fold-vertical",
    "folder",
    "folder-open",
    "gauge",
    "hash",
    "history",
    "hourglass",
    "indent-decrease",
    "indent-increase",
    "info",
    "kanban",
    "link",
    "list-tree",
    "locate",
    "mail",
    "maximize",
    "panel-right",
    "pause",
    "pen-line",
    "pin",
    "play",
    "plus",
    "printer",
    "redo-2",
    "repeat",
    "replace",
    "rotate-ccw",
    "rotate-cw",
    "route",
    "rows-3",
    "ruler",
    "save",
    "scale",
    "scissors",
    "search",
    "signal",
    "skip-forward",
    "split",
    "table",
    "tag",
    "timer",
    "trash-2",
    "triangle-alert",
    "undo-2",
    "unfold-vertical",
    "unlink",
    "user-round",
    "users-round",
    "workflow",
    "x",
    "zap",
    "zoom-in",
    "zoom-out",
];

const FALLBACK: &[u8] = include_bytes!("../assets/icons/circle.svg");

/// The icon's SVG as an image source; an unknown name falls back to a circle.
pub fn source(name: &str) -> egui::ImageSource<'static> {
    let (name, bytes) = ICONS.iter().find(|(n, _)| *n == name).copied().unwrap_or(("circle", FALLBACK));
    egui::ImageSource::Bytes { uri: std::borrow::Cow::Owned(format!("bytes://icons/{name}.svg")), bytes: egui::load::Bytes::Static(bytes) }
}

/// `name` at `size` px, tinted (the SVGs are white, so the tint is the color it shows in).
pub fn image(name: &str, size: f32, tint: Color32) -> egui::Image<'static> {
    egui::Image::new(source(name)).fit_to_exact_size(Vec2::splat(size)).tint(tint)
}

/// Paints `name` centered in `rect`.
pub fn paint(ui: &Ui, rect: Rect, name: &str, size: f32, tint: Color32) {
    paint_at(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(size)), name, tint);
}

/// Painter-level variant for code that has no `Ui` (grid cells, chart painters). The SVG is
/// rasterized at the rect's size; the first frame it may still be loading, then it just appears.
pub fn paint_at(p: &egui::Painter, rect: Rect, name: &str, tint: Color32) {
    let img = egui::Image::new(source(name));
    if let Ok(egui::load::TexturePoll::Ready { texture }) = img.load_for_size(p.ctx(), rect.size()) {
        p.image(texture.id, rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
    }
}

/// A small transparent icon button: ghost treatment (wash on hover, ink rises), 3 px corners.
/// `tip` is also its accessible label.
pub fn button(ui: &mut Ui, name: &str, box_size: f32, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(box_size), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, crate::theme::pal().WASH_HOVER);
    }
    let tint = if resp.hovered() { crate::theme::pal().TEXT } else { crate::theme::pal().TEXT_WEAK };
    paint(ui, rect, name, (box_size - 8.0).max(10.0), tint);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    /// Every embedded SVG parses and rasterizes (so a bad copy fails here, not on screen),
    /// and the strokes were rewritten to white so tinting works.
    #[test]
    fn every_icon_rasterizes_and_is_white() {
        for (name, bytes) in super::ICONS {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(!text.contains("currentColor"), "{name}: stroke must be rewritten to white");
            assert!(text.contains("stroke=\"#ffffff\""), "{name}: white stroke missing");
            let opts = resvg::usvg::Options::default();
            let tree = resvg::usvg::Tree::from_data(bytes, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut pixmap = resvg::tiny_skia::Pixmap::new(24, 24).unwrap();
            resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
            assert!(pixmap.data().iter().any(|&b| b != 0), "{name}: rendered empty");
        }
    }
}
