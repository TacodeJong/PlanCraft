//! Agenda of tasks due around the status date, with the row mark beside each one.

use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, vec2};
use plancraft_core::agenda::{self, AgendaBucket};
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

use crate::commands::Command;
use crate::edit::EditExt;
use crate::format;
use crate::state::{ToastKind, UiState};
use crate::theme;

pub fn show(ui: &mut Ui, rect: Rect, doc: &mut Document, st: &mut UiState, _out: &mut Vec<Command>) {
    let items = agenda::items(&doc.project);
    let resource = st.agenda_resource.clone();
    let tiles = agenda::tiles(&doc.project, resource.as_deref());
    let today = plancraft_core::dates::today();
    let strip = agenda::strip(&doc.project, resource.as_deref(), today);
    let day = st.agenda_day;
    let shown: Vec<_> = items
        .iter()
        .filter(|item| super::filter_bar::agenda_matches(&doc.project, st, item.id))
        .filter(|item| day.is_none_or(|d| item.finish.date() == d))
        .cloned()
        .collect();
    let rect = super::filter_bar::overview(ui, rect, &doc.project, st, shown.len(), items.len());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(12.0)));
    let ui = &mut child;
    ui.heading(st.t("view.overview"));
    ui.label(RichText::new(st.t("ov.around")).color(theme::pal().TEXT_WEAK));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        tile(ui, tiles.overdue, st.t("ov.overdue"), if tiles.overdue > 0 { theme::pal().ERROR } else { theme::pal().TEXT });
        tile(ui, tiles.this_week, st.t("ov.this_week"), theme::pal().ACCENT);
        tile(ui, tiles.milestones, st.t("ov.milestones"), theme::pal().MILESTONE);
        tile(ui, tiles.waiting, st.t("ov.waiting"), if tiles.waiting > 0 { theme::pal().WARN } else { theme::pal().TEXT });
    });
    ui.add_space(8.0);
    egui::ScrollArea::horizontal().id_salt("overview_days").show(ui, |ui| {
        ui.horizontal(|ui| {
            for cell in &strip {
                let on = st.agenda_day == Some(cell.date);
                let weekend = plancraft_core::dates::weekday_index(cell.date.weekday()) >= 5;
                let color = if weekend { theme::pal().TEXT_WEAK } else { theme::pal().TEXT };
                let label =
                    format!("{}\n{}\n{} · {}", weekday_short(cell.date), cell.date.day(), cell.starting, cell.due + cell.milestones);
                let mut button = egui::Button::new(RichText::new(label).size(11.0).color(color)).selected(on);
                if cell.date == today && !on {
                    button = button.fill(theme::pal().SELECTION);
                }
                let tip = format!(
                    "{} {}  ·  {} {}  ·  {} {}",
                    cell.starting,
                    st.t("ov.starting"),
                    cell.due,
                    st.t("ov.due"),
                    cell.milestones,
                    st.t("ov.milestones")
                );
                if ui.add(button).on_hover_text(tip).clicked() {
                    st.agenda_day = if on { None } else { Some(cell.date) };
                }
            }
        });
    });
    ui.add_space(8.0);
    if let Some(day) = st.agenda_day {
        let rows = agenda::on_day(&doc.project, resource.as_deref(), day);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(RichText::new(format::day(day)).strong().color(theme::pal().ACCENT));
            if rows.is_empty() {
                ui.label(RichText::new(st.t("ov.none")).color(theme::pal().TEXT_WEAK));
            }
            for item in rows {
                day_row(ui, doc, st, &item);
            }
        });
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for bucket in [AgendaBucket::Overdue, AgendaBucket::Today, AgendaBucket::ThisWeek, AgendaBucket::Upcoming] {
            ui.add_space(6.0);
            ui.label(RichText::new(bucket_name(st, bucket)).strong().color(theme::pal().ACCENT));
            let rows: Vec<_> = shown.iter().filter(|item| item.bucket == bucket).cloned().collect();
            if rows.is_empty() {
                ui.label(RichText::new("None").color(theme::pal().TEXT_WEAK));
                continue;
            }
            for item in rows {
                let mark = item.row_mark;
                egui::Frame::new()
                    .fill(theme::pal().BG)
                    .stroke(egui::Stroke::new(1.0, theme::pal().GRID_LINE_STRONG))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let color = mark.map(mark_color).unwrap_or(theme::pal().GRID_LINE_STRONG);
                            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 4.0, color);
                            let resp = ui.add(egui::Label::new(RichText::new(&item.name).strong()).sense(Sense::click()));
                            if resp.clicked() {
                                st.sel.select_task(item.id);
                            }
                            ui.label(RichText::new(format::date(Some(item.finish))).color(theme::pal().TEXT_WEAK));
                            if item.late {
                                ui.label(RichText::new("Late").color(theme::pal().ERROR));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let current = mark.map(RowMark::label).unwrap_or("None");
                                egui::ComboBox::from_id_salt(("overview_mark", item.id)).width(90.0).selected_text(current).show_ui(
                                    ui,
                                    |ui| {
                                        let choices = [None].into_iter().chain(RowMark::ALL.into_iter().map(Some));
                                        for choice in choices {
                                            let label = choice.map(RowMark::label).unwrap_or("None");
                                            if ui.selectable_label(mark == choice, label).clicked() && mark != choice {
                                                let id = item.id;
                                                if let Err(e) = doc.edit_msg("Row mark", move |p| {
                                                    if let Some(t) = p.task_mut(id) {
                                                        t.row_mark = choice;
                                                    }
                                                    Ok(())
                                                }) {
                                                    st.toast(ToastKind::Error, e);
                                                }
                                            }
                                        }
                                    },
                                );
                            });
                        });
                    });
            }
        }
    });
}

fn weekday_short(date: plancraft_core::dates::Date) -> &'static str {
    plancraft_core::dates::weekday_short(date.weekday())
}

fn day_row(ui: &mut Ui, doc: &Document, st: &mut UiState, item: &agenda::DayItem) {
    let mark = doc.project.task(item.id).and_then(|t| t.row_mark);
    egui::Frame::new()
        .fill(theme::pal().BG)
        .stroke(Stroke::new(1.0, theme::pal().GRID_LINE_STRONG))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, mark.map(mark_color).unwrap_or(theme::pal().GRID_LINE_STRONG));
                if item.milestone {
                    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    let c = rect.center();
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![c + vec2(0.0, -5.0), c + vec2(5.0, 0.0), c + vec2(0.0, 5.0), c + vec2(-5.0, 0.0)],
                        theme::pal().MILESTONE,
                        Stroke::NONE,
                    ));
                }
                let resp = ui.add(egui::Label::new(RichText::new(&item.name).strong()).sense(Sense::click()));
                if resp.clicked() {
                    st.sel.select_task(item.id);
                }
                let kind = if item.milestone {
                    st.t("ov.milestones")
                } else if item.due {
                    st.t("ov.due")
                } else {
                    st.t("ov.starting")
                };
                ui.label(RichText::new(kind).color(theme::pal().TEXT_WEAK));
            });
        });
}

fn tile(ui: &mut Ui, n: usize, label: &str, color: Color32) {
    egui::Frame::new()
        .fill(theme::pal().PANEL_BG)
        .stroke(Stroke::new(1.0, theme::pal().GRID_LINE_STRONG))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(110.0);
            ui.label(RichText::new(n.to_string()).strong().size(18.0).color(color));
            ui.label(RichText::new(label).small().color(theme::pal().TEXT_WEAK));
        });
}

fn bucket_name(st: &UiState, bucket: AgendaBucket) -> &'static str {
    match bucket {
        AgendaBucket::Overdue => st.t("ov.overdue"),
        AgendaBucket::Today => st.t("ov.today"),
        AgendaBucket::ThisWeek => st.t("ov.this_week"),
        AgendaBucket::Upcoming => st.t("ov.upcoming"),
    }
}

fn mark_color(mark: RowMark) -> Color32 {
    match mark {
        RowMark::Amber => theme::pal().WARN,
        RowMark::Blue => theme::pal().ACCENT,
        RowMark::Pink => theme::pal().ERROR,
        RowMark::Green => theme::pal().CHIP_OK,
    }
}
