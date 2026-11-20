//! Deleting a resource that still has assignments: replace them, or drop them.

use egui::{Context, RichText};
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

use crate::edit::EditExt;
use crate::state::{Selection, ToastKind};

pub struct ResourceDelete {
    pub ids: Vec<ResourceId>,
    pub replacement: Option<ResourceId>,
    pub new_name: String,
}

impl ResourceDelete {
    pub fn new(p: &Project, ids: Vec<ResourceId>) -> Self {
        let replacement = p.resources.iter().find(|r| !ids.contains(&r.id)).map(|r| r.id);
        Self { ids, replacement, new_name: String::new() }
    }

    fn assigned(&self, p: &Project) -> usize {
        self.ids.iter().map(|id| p.assignments_for_resource(*id).count()).sum()
    }

    /// `Ok(None)` means the typed name is a new resource. `Ok(Some)` is an existing one.
    fn target(&self, p: &Project) -> Result<Option<ResourceId>, String> {
        let name = self.new_name.trim();
        if !name.is_empty() {
            if let Some(existing) = p.resource_by_name(name) {
                if self.ids.contains(&existing.id) {
                    return Err("That resource is one of the ones being deleted.".into());
                }
                return Ok(Some(existing.id));
            }
            return Ok(None);
        }
        match self.replacement {
            Some(id) if !self.ids.contains(&id) && p.resource(id).is_some() => Ok(Some(id)),
            _ => Err("Choose a replacement.".into()),
        }
    }

    /// Moves every assignment and time entry onto the replacement, then deletes these resources.
    /// One undo step.
    pub fn replace(&self, doc: &mut Document) -> Result<(), String> {
        let name = self.new_name.trim().to_string();
        let known = self.target(&doc.project)?;
        let ids = self.ids.clone();
        doc.edit_msg("Replace Resource", |p| {
            let to = match known {
                Some(id) => id,
                None => p.resource_by_name(&name).map(|r| r.id).unwrap_or_else(|| p.add_resource(name.clone())),
            };
            if ids.contains(&to) {
                return Err("Choose a different resource.".into());
            }
            for id in &ids {
                if p.resource(*id).is_some() {
                    p.replace_resource(*id, to).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        })
    }

    pub fn remove_assignments(&self, doc: &mut Document) -> Result<(), String> {
        let ids = self.ids.clone();
        doc.edit_msg("Delete Resource", |p| p.delete_resources(&ids).map_err(|e| e.to_string()))
    }

    pub fn show(&mut self, ctx: &Context, doc: &mut Document, sel: &mut Selection, toasts: &mut Vec<(ToastKind, String)>) -> bool {
        let mut open = true;
        let mut done = false;
        let n = self.assigned(&doc.project);
        let names: Vec<String> = self.ids.iter().filter_map(|id| doc.project.resource(*id).map(|r| r.name.clone())).collect();
        let title = match names.as_slice() {
            [one] => format!("'{one}' is assigned to {n} activities."),
            _ => format!("{} resources are assigned to {n} activities.", names.len()),
        };
        let can_replace = self.target(&doc.project).is_ok();
        egui::Window::new("Delete Resource")
            .id(egui::Id::new("resource_delete"))
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .title_bar(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                open = crate::widgets::craft::dialog_header(ui, "Delete Resource");
                ui.label(RichText::new(title).strong());
                ui.label("Choose a replacement, or remove the assignments.");
                ui.add_space(6.0);
                ui.label("Replace with");
                let label =
                    self.replacement.and_then(|id| doc.project.resource(id)).map(|r| r.name.clone()).unwrap_or_else(|| "Choose…".into());
                egui::ComboBox::from_id_salt("resource_delete_pick").selected_text(label).show_ui(ui, |ui| {
                    for r in &doc.project.resources {
                        if self.ids.contains(&r.id) {
                            continue;
                        }
                        ui.selectable_value(&mut self.replacement, Some(r.id), &r.name);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("New resource");
                    ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("Name").desired_width(180.0));
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Remove Assignments & Delete").clicked() {
                        match self.remove_assignments(doc) {
                            Ok(()) => done = true,
                            Err(e) => toasts.push((ToastKind::Warning, e)),
                        }
                    }
                    if ui.add_enabled(can_replace, egui::Button::new("Replace & Delete")).clicked() {
                        match self.replace(doc) {
                            Ok(()) => done = true,
                            Err(e) => toasts.push((ToastKind::Warning, e)),
                        }
                    }
                });
            });
        if done {
            let gone = self.ids.clone();
            sel.resources.retain(|id| !gone.contains(id));
        }
        open && !done
    }
}
