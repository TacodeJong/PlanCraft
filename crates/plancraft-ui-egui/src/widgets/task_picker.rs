//! Searchable task chooser (number / WBS / name), matching the Swift PredecessorPopover.

use egui::{FontFamily, FontId, Key, RichText, ScrollArea, Sense, Ui};
use plancraft_core::prelude::*;

use crate::theme;
use crate::widgets::craft;

/// One row in a predecessor (or similar) task picker.
#[derive(Clone)]
pub struct TaskChoice {
    pub id: TaskId,
    /// 1-based sheet row number.
    pub number: usize,
    pub wbs: String,
    pub name: String,
    pub depth: u16,
    pub enabled: bool,
}

impl TaskChoice {
    pub fn from_project(p: &Project, idx: usize, enabled: bool) -> Option<Self> {
        let t = p.tasks.get(idx)?;
        Some(Self {
            id: t.id,
            number: idx.saturating_add(1),
            wbs: p.wbs(idx),
            name: t.name.clone(),
            depth: t.outline_level.saturating_sub(1),
            enabled,
        })
    }

    pub fn label(&self) -> String {
        format!("{} · {} {}", self.number, self.wbs, self.name)
    }

    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim();
        if q.is_empty() {
            return true;
        }
        let q = q.to_ascii_lowercase();
        self.number.to_string().contains(&q) || self.wbs.to_ascii_lowercase().contains(&q) || self.name.to_ascii_lowercase().contains(&q)
    }
}

/// Combo that opens a searchable list of tasks. Returns the chosen id when the user picks one
/// (or presses Enter on the first enabled match).
pub fn show(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    button: &str,
    width: f32,
    choices: &[TaskChoice],
) -> Option<TaskId> {
    let filter_id = ui.make_persistent_id(("task_pick_filter", &id_salt));
    let focused_id = ui.make_persistent_id(("task_pick_focused", &id_salt));
    let mut picked = None;

    let mut filter = ui.ctx().data(|d| d.get_temp::<String>(filter_id)).unwrap_or_default();
    let response = egui::ComboBox::from_id_salt(("task_pick", &id_salt))
        .selected_text(button)
        .width(width)
        .height(320.0)
        // Keep open while typing/filtering; only a pick or outside click dismisses.
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.set_min_width(width.max(300.0));
            let search = ui.add(
                craft::text_edit(&mut filter)
                    .desired_width(ui.available_width())
                    .hint_text(crate::lang::text("prop.task_search"))
                    .id(filter_id.with("edit")),
            );
            let already = ui.ctx().data(|d| d.get_temp::<bool>(focused_id)).unwrap_or(false);
            if !already {
                search.request_focus();
                ui.ctx().data_mut(|d| d.insert_temp(focused_id, true));
            }

            let query = filter.clone();
            let shown: Vec<&TaskChoice> = choices.iter().filter(|c| c.matches(&query)).collect();
            if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                if let Some(c) = shown.iter().find(|c| c.enabled) {
                    picked = Some(c.id);
                    ui.close();
                }
            }

            ui.add_space(4.0);
            ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                if shown.is_empty() {
                    ui.label(RichText::new(crate::lang::text("prop.task_search_empty")).color(theme::pal().TEXT_WEAK));
                    return;
                }
                for c in shown {
                    let row = choice_row(ui, c);
                    if c.enabled && row.clicked() {
                        picked = Some(c.id);
                        ui.close();
                    }
                }
            });
        });

    if response.inner.is_none() {
        // Popup closed: drop filter so the next open starts clean and refocuses.
        ui.ctx().data_mut(|d| {
            d.remove_temp::<String>(filter_id);
            d.remove_temp::<bool>(focused_id);
        });
    } else {
        ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter));
    }

    picked
}

fn choice_row(ui: &mut Ui, c: &TaskChoice) -> egui::Response {
    let weak = theme::pal().TEXT_WEAK;
    let text = theme::pal().TEXT;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::click());
    if resp.hovered() && c.enabled {
        ui.painter().rect_filled(rect, 2.0, theme::pal().WASH_HOVER);
    }
    let mut x = rect.left() + 6.0;
    let y = rect.center().y;
    let mono = FontId::new(12.0, theme::mono());
    let body = FontId::new(12.0, FontFamily::Proportional);
    let painter = ui.painter();
    let num_galley = painter.layout_no_wrap(c.number.to_string(), mono.clone(), weak);
    let num_w = 28.0_f32;
    painter.galley(egui::pos2(x + num_w - num_galley.size().x, y - num_galley.size().y / 2.0), num_galley, weak);
    x += num_w + 6.0;
    x += f32::from(c.depth) * 12.0;
    let wbs_galley = painter.layout_no_wrap(c.wbs.clone(), mono, weak);
    let wbs_w = wbs_galley.size().x;
    painter.galley(egui::pos2(x, y - wbs_galley.size().y / 2.0), wbs_galley, weak);
    x += wbs_w + 6.0;
    let name_color = if c.enabled { text } else { weak };
    let name_galley = painter.layout_no_wrap(c.name.clone(), body, name_color);
    painter.galley(egui::pos2(x, y - name_galley.size().y / 2.0), name_galley, name_color);
    if !c.enabled { resp.on_hover_text(crate::lang::text("prop.task_search_blocked")) } else { resp.on_hover_text(c.label()) }
}
