//! Quick Entry: type or paste one task per line, preview the tree, insert it in one undo step.

use egui::{Context, RichText, Ui};

use plancraft_core::ids::TaskId;
use plancraft_core::workspace::Document;

use crate::edit::EditExt;
use crate::quick_entry::{self, Issue, LinkKind, Sheet};
use crate::theme;

#[derive(Default)]
pub struct QuickEntry {
    text: String,
    error: Option<String>,
}

impl QuickEntry {
    pub fn show(&mut self, ctx: &Context, doc: &mut Document, at: Option<usize>, selected: &mut Option<TaskId>) -> bool {
        let mut open = true;
        let mut close = false;
        let time = doc.project.settings.time.clone();
        egui::Window::new(t("qe.title")).id(egui::Id::new("quick_entry")).title_bar(false).default_width(720.0).min_width(640.0).show(
            ctx,
            |ui| {
                ui.set_min_width(600.0);
                open = crate::widgets::craft::dialog_header(ui, t("qe.title"));
                ui.label(t("qe.hint"));
                ui.add(egui::TextEdit::multiline(&mut self.text).desired_rows(8).desired_width(f32::INFINITY).hint_text(t("qe.sample")));
                let sheet = quick_entry::parse(&self.text, &time);
                ui.add_space(4.0);
                ui.label(RichText::new(t("qe.shorthand")).small().color(theme::pal().TEXT_WEAK));
                ui.label(RichText::new(t("qe.keys")).small().color(theme::pal().TEXT_WEAK));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(260.0).id_salt("quick_entry_preview").show(ui, |ui| {
                    preview(ui, &sheet);
                });
                ui.add_space(6.0);
                let n = sheet.tickets.len();
                let groups = sheet.tickets.iter().filter(|ticket| ticket.group).count();
                let links = sheet.tickets.iter().flat_map(|ticket| &ticket.links).filter(|link| link.ok).count();
                ui.label(format!("{n} {} · {groups} {} · {links} {}", t("qe.tickets"), t("qe.groups"), t("qe.links")));
                if sheet.looped {
                    ui.label(RichText::new(t("qe.loop")).color(theme::pal().ERROR));
                }
                if let Some(err) = &self.error {
                    ui.label(RichText::new(err).color(theme::pal().ERROR));
                }
                let ready = n > 0 && !sheet.looped && sheet.tickets.iter().all(|ticket| ticket.issue.is_none());
                ui.horizontal(|ui| {
                    if ui.button(t("qe.cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                    let label = format!("{} ({n})", t("qe.insert"));
                    let go = ui.add_enabled(ready, egui::Button::new(label)).clicked();
                    let key = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                    if ready && (go || key) {
                        match doc.edit_msg(t("qe.undo"), |p| quick_entry::apply(p, &sheet, at)) {
                            Ok(ids) => {
                                *selected = ids.first().copied();
                                close = true;
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                });
            },
        );
        open && !close
    }
}

fn preview(ui: &mut Ui, sheet: &Sheet) {
    if sheet.tickets.is_empty() {
        ui.label(RichText::new(t("qe.empty")).color(theme::pal().TEXT_WEAK));
        return;
    }
    for ticket in &sheet.tickets {
        ui.horizontal(|ui| {
            ui.add_space(f32::from(ticket.level.saturating_sub(1)) * 16.0);
            ui.label(&ticket.name);
            let mark = if ticket.group {
                t("qe.group").to_string()
            } else if ticket.milestone {
                t("qe.milestone").to_string()
            } else if ticket.estimated {
                format!("{}?", ticket.duration_label)
            } else {
                ticket.duration_label.clone()
            };
            if !mark.is_empty() {
                ui.label(RichText::new(mark).color(theme::pal().TEXT_WEAK));
            }
            if !ticket.resources.is_empty() {
                ui.label(RichText::new(format!("@{}", ticket.resources.join(", "))).small().color(theme::pal().TEXT_WEAK));
            }
            for link in &ticket.links {
                let target = match &link.kind {
                    LinkKind::Above => t("qe.above").to_string(),
                    LinkKind::Number(n) => n.to_string(),
                    LinkKind::Wbs(code) => code.clone(),
                };
                let mut text = format!("{} {target}", t("qe.after"));
                if !link.ok {
                    text.push(' ');
                    text.push_str(t("qe.unavailable"));
                }
                let color = if link.ok { theme::pal().TEXT_WEAK } else { theme::pal().ERROR };
                ui.label(RichText::new(text).small().color(color));
            }
        });
        if let Some(issue) = ticket.issue {
            ui.horizontal(|ui| {
                ui.add_space(f32::from(ticket.level.saturating_sub(1)) * 16.0 + 16.0);
                ui.label(RichText::new(issue_text(issue)).small().color(theme::pal().ERROR));
            });
        }
    }
}

fn issue_text(issue: Issue) -> &'static str {
    match issue {
        Issue::NeedsName => t("qe.need_name"),
        Issue::BadToken => t("qe.bad_token"),
        Issue::SkippedLevel => t("qe.skip"),
        Issue::GroupMilestone => t("qe.group_milestone"),
        Issue::GroupResource => t("qe.group_resource"),
        Issue::Unavailable => t("qe.unavailable"),
        Issue::Loop => t("qe.loop"),
    }
}

fn t(key: &'static str) -> &'static str {
    crate::lang::text(key)
}
