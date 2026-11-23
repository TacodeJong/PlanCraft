//! The filter strip on a page. It only narrows what that page draws.

use egui::{Rect, RichText, Ui, vec2};
use plancraft_core::prelude::*;

use crate::state::{ActivityFocus, DateWindow, FocusScope, MarkFilter, PageGroup, TaskKindFilter, UiState};
use crate::theme;
use crate::widgets::craft;
use crate::widgets::date_picker::{self, DateField};

const BAR_H: f32 = 32.0;
const TASK_BAR_H: f32 = 58.0;

/// Task pages: the activity focus, status, type, dates and resources, plus grouping.
pub fn task(ui: &mut Ui, rect: Rect, p: &Project, st: &mut UiState) -> Rect {
    let (bar, rest) = split_h(rect, TASK_BAR_H);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    child.painter().rect_filled(bar, 0.0, theme::pal().PANEL_BG);
    child.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(st.t("bar.filters")).strong().color(theme::pal().TEXT));
        let picked = p.tasks.iter().find(|t| st.sel.tasks.contains(&t.id)).map(|t| t.id);
        // One context-sensitive control: Show selection when idle, Clear filter when focused.
        if let Some(focus) = st.focus {
            let name = p.task(focus.task).map(|t| t.name.as_str()).unwrap_or("—");
            ui.label(RichText::new(name).color(theme::pal().TEXT_WEAK));
            egui::ComboBox::from_id_salt("focus_scope").width(150.0).selected_text(focus.scope.label()).show_ui(ui, |ui| {
                for scope in FocusScope::ALL {
                    if ui.selectable_label(focus.scope == scope, scope.label()).clicked() {
                        st.focus = Some(ActivityFocus { task: focus.task, scope });
                    }
                }
            });
            // Always usable while focused — even if the current selection is empty.
            if craft::secondary_button(ui, st.t("filter.clear_activity")).clicked() {
                st.focus = None;
            }
        } else if craft::secondary_button_enabled(ui, st.t("filter.show_selection"), picked.is_some()).clicked()
            && let Some(task) = picked
        {
            st.focus = Some(ActivityFocus { task, scope: FocusScope::default() });
        }
        let status = st.status_filter.map(crate::lang::status_name).unwrap_or_else(|| st.t("filter.status_all"));
        egui::ComboBox::from_id_salt("filter_status").width(120.0).selected_text(status).show_ui(ui, |ui| {
            if ui.selectable_label(st.status_filter.is_none(), st.t("filter.status_all")).clicked() {
                st.status_filter = None;
            }
            for s in plancraft_core::model::TaskStatus::ALL {
                if ui.selectable_label(st.status_filter == Some(s), crate::lang::status_name(s)).clicked() {
                    st.status_filter = Some(s);
                }
            }
        });
        egui::ComboBox::from_id_salt("filter_kind").width(110.0).selected_text(st.kind_filter.label()).show_ui(ui, |ui| {
            for kind in TaskKindFilter::ALL {
                if ui.selectable_label(st.kind_filter == kind, kind.label()).clicked() {
                    st.kind_filter = kind;
                }
            }
        });
        egui::ComboBox::from_id_salt("filter_mark").width(120.0).selected_text(st.mark_filter.label()).show_ui(ui, |ui| {
            for mark in MarkFilter::ALL {
                if ui.selectable_label(st.mark_filter == mark, mark.label()).clicked() {
                    st.mark_filter = mark;
                }
            }
        });
        egui::ComboBox::from_id_salt("filter_dates").width(120.0).selected_text(st.date_window.label()).show_ui(ui, |ui| {
            for window in DateWindow::ALL {
                if ui.selectable_label(st.date_window == window, window.label()).clicked() {
                    st.date_window = window;
                }
            }
        });
        if st.date_window == DateWindow::Range {
            let minute = p.settings.time.default_start_time;
            if let Some(next) = date_picker::button(
                ui,
                DateField {
                    salt: "filter_from",
                    current: st.date_from.map(plancraft_core::dates::midnight),
                    clearable: true,
                    enabled: true,
                    minute,
                    empty: st.t("filter.from"),
                },
            ) {
                st.date_from = next.map(|dt| dt.date());
            }
            if let Some(next) = date_picker::button(
                ui,
                DateField {
                    salt: "filter_to",
                    current: st.date_to.map(plancraft_core::dates::midnight),
                    clearable: true,
                    enabled: true,
                    minute,
                    empty: st.t("filter.to"),
                },
            ) {
                st.date_to = next.map(|dt| dt.date());
            }
        }
        let res_label = match st.filter_resources.as_slice() {
            [] => st.t("filter.everyone").to_string(),
            [one] => p.resource(*one).map(|r| r.name.clone()).unwrap_or_else(|| st.t("filter.everyone").to_string()),
            many => format!("{} {}", many.len(), st.t("filter.resources")),
        };
        ui.menu_button(res_label, |ui| {
            if ui.selectable_label(st.filter_resources.is_empty(), st.t("filter.everyone")).clicked() {
                st.filter_resources.clear();
            }
            for r in &p.resources {
                let mut on = st.filter_resources.contains(&r.id);
                if ui.checkbox(&mut on, &r.name).changed() {
                    if on {
                        if !st.filter_resources.contains(&r.id) {
                            st.filter_resources.push(r.id);
                        }
                    } else {
                        st.filter_resources.retain(|id| *id != r.id);
                    }
                }
            }
        });
        egui::ComboBox::from_id_salt("page_group").width(130.0).selected_text(st.page_group.label()).show_ui(ui, |ui| {
            for g in PageGroup::ALL {
                if ui.selectable_label(st.page_group == g, g.label()).clicked() {
                    st.page_group = g;
                }
            }
        });
        if st.filter != crate::state::TaskFilter::All {
            ui.label(RichText::new(st.filter.label()).color(theme::pal().TEXT_WEAK));
        }
        let q = st.search.trim();
        if !q.is_empty() {
            ui.label(RichText::new(format!("“{q}”")).color(theme::pal().TEXT_WEAK));
        }
        if st.focus.is_none() && super::criteria_active(st) && craft::secondary_button(ui, st.t("filter.reset")).clicked() {
            st.focus = None;
            st.status_filter = None;
            st.kind_filter = TaskKindFilter::All;
            st.mark_filter = MarkFilter::All;
            st.date_window = DateWindow::All;
            st.date_from = None;
            st.date_to = None;
            st.filter_resources.clear();
            st.page_group = PageGroup::None;
            st.filter = crate::state::TaskFilter::All;
        }
        let n = p.tasks.len();
        let x = p.tasks.iter().filter(|t| super::task_matches(p, st, t)).count();
        if x != n {
            ui.label(RichText::new(format!("{x} {} {n}", st.t("bar.of"))).color(theme::pal().ACCENT));
        }
    });
    rest
}

/// Resource pages: name (the toolbar search) and category.
pub fn resources(ui: &mut Ui, rect: Rect, st: &mut UiState, shown: usize, total: usize) -> Rect {
    let (bar, rest) = split(rect);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    child.painter().rect_filled(bar, 0.0, theme::pal().PANEL_BG);
    child.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(st.t("bar.filters")).strong().color(theme::pal().TEXT));
        let label = st.resource_category.map(crate::lang::category_name).unwrap_or_else(|| st.t("bar.all_categories"));
        egui::ComboBox::from_id_salt("res_cat").width(160.0).selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(st.resource_category.is_none(), st.t("bar.all_categories")).clicked() {
                st.resource_category = None;
            }
            for cat in ResourceCategory::ALL {
                if ui.selectable_label(st.resource_category == Some(cat), crate::lang::category_name(cat)).clicked() {
                    st.resource_category = Some(cat);
                }
            }
        });
        let q = st.search.trim();
        if !q.is_empty() {
            ui.label(RichText::new(format!("“{q}”")).color(theme::pal().TEXT_WEAK));
        }
        if shown != total {
            ui.label(RichText::new(format!("{shown} {} {total}", st.t("bar.of"))).color(theme::pal().ACCENT));
        }
    });
    rest
}

/// Deliveries: the toolbar search matches the recipient's name.
pub fn deliveries(ui: &mut Ui, rect: Rect, st: &UiState, shown: usize, total: usize) -> Rect {
    let (bar, rest) = split(rect);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    child.painter().rect_filled(bar, 0.0, theme::pal().PANEL_BG);
    child.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(st.t("bar.filters")).strong().color(theme::pal().TEXT));
        let q = st.search.trim();
        let text = if q.is_empty() { st.t("filter.all_recipients").to_string() } else { format!("{} “{q}”", st.t("filter.recipient")) };
        ui.label(RichText::new(text).color(theme::pal().TEXT_WEAK));
        if shown != total {
            ui.label(RichText::new(format!("{shown} {} {total}", st.t("bar.of"))).color(theme::pal().ACCENT));
        }
    });
    rest
}

/// Overview: one resource, or everyone.
pub fn overview(ui: &mut Ui, rect: Rect, p: &Project, st: &mut UiState, shown: usize, total: usize) -> Rect {
    let (bar, rest) = split(rect);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    child.painter().rect_filled(bar, 0.0, theme::pal().PANEL_BG);
    child.painter().hline(bar.x_range(), bar.bottom() - 0.5, theme::hairline());
    child.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(st.t("bar.filters")).strong().color(theme::pal().TEXT));
        let label = st.agenda_resource.clone().unwrap_or_else(|| st.t("filter.everyone").to_string());
        egui::ComboBox::from_id_salt("agenda_res").width(180.0).selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(st.agenda_resource.is_none(), st.t("filter.everyone")).clicked() {
                st.agenda_resource = None;
            }
            for r in &p.resources {
                let on = st.agenda_resource.as_deref() == Some(r.name.as_str());
                if ui.selectable_label(on, &r.name).clicked() {
                    st.agenda_resource = Some(r.name.clone());
                }
            }
        });
        if shown != total {
            ui.label(RichText::new(format!("{shown} {} {total}", st.t("bar.of"))).color(theme::pal().TEXT_WEAK));
        }
    });
    rest
}

pub fn resource_matches(st: &UiState, r: &Resource) -> bool {
    let q = st.search.trim().to_lowercase();
    let name_ok = q.is_empty() || r.name.to_lowercase().contains(&q);
    let cat_ok = st.resource_category.is_none_or(|c| r.category == c);
    name_ok && cat_ok
}

pub fn agenda_matches(p: &Project, st: &UiState, id: TaskId) -> bool {
    let Some(name) = st.agenda_resource.as_deref() else { return true };
    p.format_resource_names_only(id).split(',').any(|who| who.trim().eq_ignore_ascii_case(name))
}

fn split(rect: Rect) -> (Rect, Rect) {
    split_h(rect, BAR_H)
}

fn split_h(rect: Rect, height: f32) -> (Rect, Rect) {
    let bar = Rect::from_min_size(rect.min, vec2(rect.width(), height.min(rect.height())));
    let rest = Rect::from_min_max(egui::pos2(rect.left(), bar.bottom()), rect.max);
    (bar, rest)
}
