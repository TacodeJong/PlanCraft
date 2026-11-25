//! Deleting a resource that still has assignments: replace them, or drop them.

use egui::{Context, RichText};
use plancraft_core::prelude::*;
use plancraft_core::workspace::Workspace;

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
    /// One undo step (via Session).
    pub fn replace(&self, ws: &mut Workspace, handle: u64) -> Result<(), String> {
        let name = self.new_name.trim().to_string();
        let known = {
            let doc = ws.index_of(handle).and_then(|i| ws.docs.get(i)).ok_or_else(|| "document was closed".to_string())?;
            self.target(&doc.project)?
        };
        let ids = self.ids.clone();
        super::edit_bound(ws, handle, "Replace Resource", |p| {
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

    pub fn remove_assignments(&self, ws: &mut Workspace, handle: u64) -> Result<(), String> {
        let ids = self.ids.clone();
        let prev = ws.active;
        if let Some(i) = ws.index_of(handle) {
            ws.active = Some(i);
        }
        let result = crate::engine_bridge::execute(
            ws,
            "resource.delete",
            &serde_json::json!({ "resources": ids.iter().map(|id| id.0).collect::<Vec<_>>() }),
        );
        if prev.is_some_and(|j| j < ws.docs.len()) {
            ws.active = prev;
        }
        result.map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        ws: &mut Workspace,
        handle: u64,
        sel: &mut Selection,
        toasts: &mut Vec<(ToastKind, String)>,
    ) -> bool {
        let Some(doc) = ws.index_of(handle).and_then(|i| ws.docs.get(i)) else {
            return false;
        };
        let n = self.assigned(&doc.project);
        let names: Vec<String> = self.ids.iter().filter_map(|id| doc.project.resource(*id).map(|r| r.name.clone())).collect();
        let body = match names.as_slice() {
            [one] => format!("'{one}' is assigned to {n} activities."),
            _ => format!("{} resources are assigned to {n} activities.", names.len()),
        };
        let choices: Vec<(ResourceId, String)> =
            doc.project.resources.iter().filter(|r| !self.ids.contains(&r.id)).map(|r| (r.id, r.name.clone())).collect();
        let can_replace = self.target(&doc.project).is_ok();
        let pick_label = self
            .replacement
            .and_then(|id| choices.iter().find(|(cid, _)| *cid == id).map(|(_, n)| n.clone()))
            .unwrap_or_else(|| "Choose…".into());

        let mut open = true;
        let mut replace = false;
        let mut remove = false;
        // Destructive confirm: block the shell (same family as Save changes?), not a floating Window.
        let modal = crate::widgets::craft::blocking_modal(ctx, egui::Id::unique("resource_delete"), |ui| {
            ui.set_width(420.0);
            crate::widgets::craft::confirm_title(ui, crate::lang::text("dlg.delete_resource"));
            crate::widgets::craft::confirm_body(ui, body);
            crate::widgets::craft::confirm_note(ui, "Choose a replacement, or remove the assignments.");
            ui.add_space(6.0);
            ui.label(RichText::new("Replace with").color(crate::theme::pal().TEXT_WEAK));
            egui::ComboBox::from_id_salt("resource_delete_pick").selected_text(pick_label).show_ui(ui, |ui| {
                for (id, name) in &choices {
                    ui.selectable_value(&mut self.replacement, Some(*id), name);
                }
            });
            ui.horizontal(|ui| {
                ui.label("New resource");
                ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("Name").desired_width(180.0));
            });
            crate::widgets::craft::confirm_actions_rule(ui);
            ui.horizontal(|ui| {
                if crate::widgets::craft::secondary_button(ui, crate::lang::text("common.cancel")).clicked() {
                    open = false;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::widgets::craft::primary_button_enabled(ui, "Replace & Delete", can_replace).clicked() {
                        replace = true;
                    }
                    if crate::widgets::craft::secondary_button(ui, "Remove Assignments & Delete").clicked() {
                        remove = true;
                    }
                });
            });
        });
        if modal.should_close() {
            open = false;
        }

        let mut done = false;
        if replace {
            match self.replace(ws, handle) {
                Ok(()) => done = true,
                Err(e) => toasts.push((ToastKind::Warning, e)),
            }
        } else if remove {
            match self.remove_assignments(ws, handle) {
                Ok(()) => done = true,
                Err(e) => toasts.push((ToastKind::Warning, e)),
            }
        }
        if done {
            let gone = self.ids.clone();
            sel.resources.retain(|id| !gone.contains(id));
        }
        open && !done
    }
}
