//! Who is waiting on a task, derived from links and the resources on the receiving work.

use egui::{Rect, RichText, Ui};
use plancraft_core::deliveries::{self, DeliveryStatus};
use plancraft_core::workspace::Document;

use crate::commands::Command;
use crate::format;
use crate::state::{UiState, ViewKind};
use crate::theme;

pub fn show(ui: &mut Ui, rect: Rect, doc: &mut Document, st: &mut UiState, out: &mut Vec<Command>) {
    let all = deliveries::recipients(&doc.project);
    let total = all.len();
    let q = st.search.trim().to_lowercase();
    let people: Vec<_> = all.into_iter().filter(|person| q.is_empty() || person.name.to_lowercase().contains(&q)).collect();
    let rect = super::filter_bar::deliveries(ui, rect, st, people.len(), total);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(12.0)));
    let ui = &mut child;
    crate::widgets::craft::display_heading(ui, "Deliveries");
    ui.label(RichText::new("What each person is waiting to receive.").color(theme::pal().TEXT_WEAK));
    ui.add_space(8.0);
    if people.is_empty() {
        ui.label("No one is waiting on a delivery.");
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for person in people {
            egui::Frame::new()
                .fill(theme::pal().BG)
                .stroke(theme::hairline())
                .corner_radius(egui::CornerRadius::ZERO)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&person.name).size(16.0).strong());
                    for delivery in &person.deliveries {
                        let name = doc.project.task(delivery.predecessor).map(|t| t.name.clone()).unwrap_or_else(|| "Task".into());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).strong());
                            chip(ui, delivery.status);
                            ui.label(RichText::new(delivery.link.code()).color(theme::pal().TEXT_WEAK));
                            if let Some(via) = delivery.via_group {
                                let via_name = doc.project.task(via).map(|t| t.name.clone()).unwrap_or_default();
                                if !via_name.is_empty() {
                                    ui.label(RichText::new(format!("via {via_name}")).color(theme::pal().TEXT_WEAK));
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let id = delivery.predecessor;
                                if ui.button("Open in Gantt").clicked() {
                                    st.sel.select_task(id);
                                    out.push(Command::SetView(ViewKind::Gantt));
                                    out.push(Command::ScrollToTask);
                                }
                            });
                        });
                        let teams = if delivery.teams.is_empty() { "Unassigned".to_string() } else { delivery.teams.join(", ") };
                        let due = format::date(delivery.due);
                        ui.label(
                            RichText::new(format!("{teams} · due {due} · {progress}%", progress = delivery.progress.round() as i64))
                                .color(theme::pal().TEXT_WEAK),
                        );
                    }
                });
            ui.add_space(8.0);
        }
    });
}

fn chip(ui: &mut Ui, status: DeliveryStatus) {
    let (fg, bg) = match status {
        DeliveryStatus::Received => (theme::pal().CHIP_TEXT, theme::pal().CHIP_OK),
        DeliveryStatus::InProgress => (theme::pal().CHIP_TEXT, theme::pal().ACCENT),
        DeliveryStatus::Late => (theme::pal().CHIP_TEXT, theme::pal().CHIP_ERROR),
        DeliveryStatus::Waiting => (theme::pal().TEXT, theme::pal().ACCENT_PALE),
    };
    egui::Frame::new().fill(bg).corner_radius(10.0).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(status.label()).color(fg).size(11.5).strong());
    });
}
