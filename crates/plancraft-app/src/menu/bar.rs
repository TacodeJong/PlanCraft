//! In-window conventional menu row (Windows/Linux, and macOS fallback).

use egui::{Align, Layout, Sense, Ui, vec2};

use crate::commands::Command;
use crate::theme;

use super::model::{ItemKind, MenuBar, Node};

const ROW_H: f32 = 24.0;

/// Draw File/Edit/View/Format/Project/Help as an egui menu row. Appends chosen commands to `out`.
pub fn draw_menu_row(ui: &mut Ui, bar: &MenuBar, out: &mut Vec<Command>) {
    let rect = ui.available_rect_before_wrap();
    let rect = egui::Rect::from_min_size(rect.min, vec2(rect.width(), ROW_H));
    ui.painter().rect_filled(rect, 0.0, theme::pal().PANEL_BG);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
    child.spacing_mut().item_spacing.x = 2.0;
    for menu in &bar.menus {
        if matches!(menu.role, super::model::MenuRole::App) {
            continue;
        }
        // Window document list stays in the native bar; on desktop show it here too.
        child.menu_button(&menu.title, |ui| {
            ui.set_min_width(200.0);
            draw_nodes(ui, &menu.children, out);
        });
    }
    ui.allocate_rect(rect, Sense::hover());
}

fn draw_nodes(ui: &mut Ui, nodes: &[Node], out: &mut Vec<Command>) {
    for n in nodes {
        match n {
            Node::Separator => {
                ui.separator();
            }
            Node::Submenu { label, children, .. } => {
                ui.menu_button(label, |ui| draw_nodes(ui, children, out));
            }
            Node::Standard(_) => {
                // AppKit-only rows (Services, Zoom, …) have no egui equivalent.
            }
            Node::Item(it) => {
                if it.id == super::model::HIDE {
                    continue;
                }
                let shortcut = it.shortcut.clone().unwrap_or_default();
                let resp = match it.kind {
                    ItemKind::Plain => ui.add_enabled(it.enabled, egui::Button::new(&it.label).shortcut_text(shortcut)),
                    ItemKind::Check(on) | ItemKind::Radio(on) => {
                        let mut on = on;
                        ui.add_enabled_ui(it.enabled, |ui| {
                            let r = ui.checkbox(&mut on, &it.label);
                            if !shortcut.is_empty() {
                                let _ = r.clone().on_hover_text(shortcut);
                            }
                            r
                        })
                        .inner
                    }
                };
                if resp.clicked() {
                    out.push(it.command.clone());
                    ui.close();
                }
            }
        }
    }
}
