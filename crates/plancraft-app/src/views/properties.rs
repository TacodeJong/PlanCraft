//! The pane on the right of the active view. Values are read from the project each frame.

use egui::{Rect, RichText, Sense, Stroke, Ui, vec2};
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

use crate::edit::EditExt;
use crate::format;
use crate::state::{PropTab, ToastKind, UiState, ViewKind};
use crate::theme;

const MIN_W: f32 = 280.0;
const MAX_W: f32 = 520.0;

/// Draws the pane when it is open and returns the rect left for the active view.
pub fn dock(ui: &mut Ui, rect: Rect, doc: &mut Document, st: &mut UiState) -> Rect {
    let room = (rect.width() - 200.0).max(0.0);
    if !st.properties_visible || room < 1.0 {
        return rect;
    }
    let width = st.properties_width.clamp(MIN_W.min(room), MAX_W.min(room));
    let pane = Rect::from_min_max(pos2(rect.right() - width, rect.top()), rect.max);
    let handle = Rect::from_min_max(pos2(pane.left() - 3.0, pane.top()), pos2(pane.left() + 3.0, pane.bottom()));
    let drag = ui.interact(handle, ui.id().with("properties_drag"), Sense::drag());
    if drag.hovered() || drag.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if drag.dragged() {
        st.properties_width = (st.properties_width - drag.drag_delta().x).clamp(MIN_W, MAX_W);
    }
    ui.painter().rect_filled(pane, 0.0, theme::pal().BG);
    ui.painter().vline(pane.left(), pane.y_range(), Stroke::new(1.0, theme::pal().GRID_LINE_STRONG));
    show(ui, pane.shrink2(vec2(10.0, 8.0)), doc, st);
    Rect::from_min_max(rect.min, pos2(handle.left(), rect.bottom()))
}

fn show(ui: &mut Ui, rect: Rect, doc: &mut Document, st: &mut UiState) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    egui::ScrollArea::vertical().show(&mut child, |ui| {
        crate::widgets::craft::section_label(ui, st.t("tb.properties"));
        ui.add_space(2.0);
        if resource_view(st.view) {
            match st.sel.single_resource().or_else(|| (st.view == ViewKind::Timesheet).then_some(st.timesheet_resource).flatten()) {
                Some(id) => resource_pane(ui, doc, st, id),
                None if st.sel.resources.len() > 1 => {
                    ui.label(RichText::new(st.t("prop.several_resources")).color(theme::pal().TEXT_WEAK));
                }
                None if !st.sel.tasks.is_empty() => task_pane(ui, doc, st),
                None => empty(ui),
            }
        } else if st.sel.tasks.is_empty() {
            empty(ui);
        } else {
            task_pane(ui, doc, st);
        }
    });
}

fn empty(ui: &mut Ui) {
    ui.label(RichText::new(crate::lang::text("prop.no_selection")).color(theme::pal().TEXT_WEAK));
}

/// A greyed, uneditable field showing a derived value (kept visible rather than hidden).
fn derived_line(ui: &mut Ui, value: &str, hover: &str) {
    let mut text = value.to_string();
    ui.add_enabled(false, egui::TextEdit::singleline(&mut text).desired_width(ui.available_width())).on_disabled_hover_text(hover);
}

fn resource_view(view: ViewKind) -> bool {
    matches!(view, ViewKind::ResourceSheet | ViewKind::ResourceUsage | ViewKind::Workload | ViewKind::TeamPlanner | ViewKind::Timesheet)
}

fn resource_pane(ui: &mut Ui, doc: &mut Document, st: &mut UiState, id: ResourceId) {
    let Some(resource) = doc.project.resource(id) else {
        empty(ui);
        return;
    };
    let name = resource.name.clone();
    let kind = resource.kind;
    let category = resource.category;
    let email = resource.email.clone();
    let group = resource.group.clone();
    let units = format::units(resource.max_units);
    label(ui, st.t("prop.name"));
    if let Some(text) = line(ui, ("res_name", id), &name, "") {
        edit(doc, st, "Resource name", move |p| {
            if text.trim().is_empty() {
                return Err("The resource name cannot be empty.".into());
            }
            if let Some(r) = p.resource_mut(id) {
                r.name = text.trim().to_string();
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.kind"));
    egui::ComboBox::from_id_salt(("res_kind", id)).selected_text(crate::lang::category_name(category)).show_ui(ui, |ui| {
        for choice in ResourceCategory::ALL {
            if ui.selectable_label(category == choice, crate::lang::category_name(choice)).clicked() && category != choice {
                edit(doc, st, "Resource kind", move |p| {
                    if let Some(r) = p.resource_mut(id) {
                        r.category = choice;
                    }
                    Ok(())
                });
            }
        }
    });
    label(ui, st.t("prop.type"));
    egui::ComboBox::from_id_salt(("res_type", id)).selected_text(type_label(kind)).show_ui(ui, |ui| {
        for choice in [ResourceType::Work, ResourceType::Material, ResourceType::Cost] {
            if ui.selectable_label(kind == choice, type_label(choice)).clicked() && kind != choice {
                edit(doc, st, "Resource type", move |p| {
                    if let Some(r) = p.resource_mut(id) {
                        r.kind = choice;
                    }
                    Ok(())
                });
            }
        }
    });
    label(ui, st.t("prop.email"));
    if let Some(text) = line(ui, ("res_email", id), &email, "") {
        edit(doc, st, "Resource email", move |p| {
            if let Some(r) = p.resource_mut(id) {
                r.email = text.trim().to_string();
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.group"));
    if let Some(text) = line(ui, ("res_group", id), &group, "") {
        edit(doc, st, "Resource group", move |p| {
            if let Some(r) = p.resource_mut(id) {
                r.group = text.trim().to_string();
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.max_units"));
    if let Some(text) = line(ui, ("res_units", id), &units, "100%") {
        edit(doc, st, "Max units", move |p| {
            let units = format::parse_units(&text).ok_or_else(|| format!("'{text}' is not a valid amount of units"))?;
            if let Some(r) = p.resource_mut(id) {
                r.max_units = units;
            }
            Ok(())
        });
    }
}

fn task_pane(ui: &mut Ui, doc: &mut Document, st: &mut UiState) {
    let ids: Vec<TaskId> = doc.project.tasks.iter().filter(|t| st.sel.tasks.contains(&t.id)).map(|t| t.id).collect();
    let Some(first) = ids.first().copied() else {
        empty(ui);
        return;
    };
    let multi = ids.len() > 1;
    if multi {
        ui.label(RichText::new(format!("{} {}", ids.len(), st.t("prop.activities"))).color(theme::pal().TEXT_WEAK));
    }
    // Scheduling warnings of the selection stay above the tabs, visible whichever tab is open.
    let warned: Vec<String> = ids.iter().filter_map(|id| doc.project.task(*id)).flat_map(|t| t.sched.warnings.iter().cloned()).collect();
    if !warned.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.add(crate::icons::image("triangle-alert", 13.0, theme::pal().ERROR));
            ui.label(RichText::new(warned.join("\n")).color(theme::pal().ERROR));
        });
        if let Some(hint) = crate::columns::warning_hint(&warned) {
            ui.label(RichText::new(hint).small().color(theme::pal().TEXT_WEAK));
        }
        ui.add_space(4.0);
    }
    let snap = snapshot(&doc.project, &ids);
    let share = doc.project.settings.rules.enabled() && ids.iter().all(|id| can_share(&doc.project, *id));
    let any_summary = ids.iter().any(|id| doc.project.task_index(*id).is_some_and(|i| doc.project.has_children(i)));
    // Only milestones selected: one moment instead of separate start and finish fields.
    let milestone_only = !any_summary && snap.milestone == Some(true);

    // The pane's tabs, so it does not become one long scroll.
    ui.horizontal_wrapped(|ui| {
        for tab in PropTab::ALL {
            ui.selectable_value(&mut st.properties_tab, tab, tab.title());
        }
    });
    ui.separator();
    match st.properties_tab {
        PropTab::General => general_tab(ui, doc, st, &ids, &snap, first, multi, any_summary, milestone_only, share),
        PropTab::Planning => planning_tab(ui, doc, st, &ids, &snap, first, multi),
        PropTab::Resources => resources_tab(ui, doc, st, &ids, &snap, first, multi, any_summary),
        PropTab::Progress => progress_tab(ui, doc, st, &ids, &snap, first, multi, any_summary, milestone_only),
        PropTab::Notes => notes_block(ui, doc, st, &ids, &snap, first, multi),
    }
}

fn general_tab(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    snap: &Snap,
    first: TaskId,
    multi: bool,
    any_summary: bool,
    milestone_only: bool,
    share: bool,
) {
    ui.horizontal(|ui| {
        if !multi && snap.milestone == Some(true) {
            let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            let c = rect.center();
            ui.painter().add(egui::Shape::convex_polygon(
                vec![c + vec2(0.0, -5.0), c + vec2(5.0, 0.0), c + vec2(0.0, 5.0), c + vec2(-5.0, 0.0)],
                theme::pal().MILESTONE,
                egui::Stroke::NONE,
            ));
        }
        label(ui, st.t("prop.name"));
    });
    if let Some(text) = line(ui, ("task_name", first, multi), snap.name.as_deref().unwrap_or(""), if multi { "Multiple" } else { "" }) {
        let ids = ids.to_vec();
        edit(doc, st, "Name", move |p| {
            if text.trim().is_empty() {
                return Err("The task name cannot be empty.".into());
            }
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.name = text.trim().to_string();
                }
            }
            Ok(())
        });
    }
    if !multi && let Some(idx) = doc.project.task_index(first) {
        label(ui, st.t("prop.outline"));
        ui.label(format!(
            "{}   {} {}",
            doc.project.wbs(idx),
            st.t("prop.level"),
            doc.project.tasks.get(idx).map(|t| t.outline_level).unwrap_or(1)
        ));
        if doc.project.has_children(idx) {
            group_color(ui, doc, st, first);
        }
    }
    if snap.milestone.is_none() {
        ui.label(RichText::new(st.t("prop.milestone_differs")).size(11.0).color(theme::pal().TEXT_WEAK));
    }
    let mut flag = snap.milestone.unwrap_or(false);
    // A group cannot be a milestone: the box stays visible but greyed (GUI.md).
    if ui.add_enabled(!any_summary, egui::Checkbox::new(&mut flag, st.t("prop.milestone"))).changed() {
        let ids = ids.to_vec();
        edit(doc, st, "Milestone", move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.kind = if flag { TaskKind::Milestone } else { TaskKind::Task };
                    if flag {
                        t.duration = 0;
                    }
                }
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.mark"));
    let mark_label = snap.mark.map(|m| m.map(crate::lang::mark_name).unwrap_or(st.t("common.none"))).unwrap_or(st.t("common.multiple"));
    if let Some(Some(mark)) = snap.mark {
        ui.add(crate::icons::image("flag", 14.0, theme::row_mark_color(mark))).on_hover_text(crate::lang::mark_name(mark));
    }
    egui::ComboBox::from_id_salt(("task_mark", first, multi)).selected_text(mark_label).show_ui(ui, |ui| {
        let choices = [None].into_iter().chain(RowMark::ALL.into_iter().map(Some));
        for choice in choices {
            let text = choice.map(crate::lang::mark_name).unwrap_or(st.t("common.none"));
            if ui.selectable_label(snap.mark == Some(choice), text).clicked() && snap.mark != Some(choice) {
                let ids = ids.to_vec();
                edit(doc, st, "Row mark", move |p| {
                    for id in &ids {
                        if let Some(t) = p.task_mut(*id) {
                            t.row_mark = choice;
                        }
                    }
                    Ok(())
                });
            }
        }
    });
    // The everyday fields, like the Swift inspector: no tab-hopping for normal work.
    schedule_fields(ui, doc, st, ids, snap, first, multi, any_summary, milestone_only, share);
    label(ui, st.t("prop.predecessors"));
    predecessors(ui, doc, st, ids, multi);
    resources_tab(ui, doc, st, ids, snap, first, multi, any_summary);
    status_block(ui, doc, st, ids, snap, first, multi, any_summary);
    percent_block(ui, doc, st, ids, snap, first, multi, any_summary);
    if milestone_only {
        label(ui, st.t("prop.deliverable"));
        if let Some(text) = line(ui, ("task_deliverable", first, multi), snap.deliverable.as_deref().unwrap_or(""), "") {
            let ids = ids.to_vec();
            edit(doc, st, "Deliverable", move |p| {
                for id in &ids {
                    if let Some(t) = p.task_mut(*id) {
                        t.deliverable = text.clone();
                    }
                }
                Ok(())
            });
        }
    }
    notes_block(ui, doc, st, ids, snap, first, multi);
}

fn notes_block(ui: &mut Ui, doc: &mut Document, st: &mut UiState, ids: &[TaskId], snap: &Snap, first: TaskId, multi: bool) {
    label(ui, st.t("prop.notes"));
    if let Some(text) = notes(ui, ("task_notes", first, multi), snap.notes.as_deref().unwrap_or("")) {
        let ids = ids.to_vec();
        edit(doc, st, "Notes", move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.notes = text.clone();
                }
            }
            Ok(())
        });
    }
}

/// The project's custom fields, edited with the same parse-and-set path as the dialog and the
/// table (`parse_custom_value` + `set_custom_value`).
fn custom_fields(ui: &mut Ui, doc: &mut Document, st: &mut UiState, ids: &[TaskId], first: TaskId, multi: bool) {
    let fields: Vec<(CustomFieldId, String, plancraft_core::custom::CustomFieldKind, Vec<String>)> =
        doc.project.custom_fields.iter().map(|f| (f.id, f.name.clone(), f.kind, f.choices.clone())).collect();
    if fields.is_empty() {
        return;
    }
    use plancraft_core::custom::CustomFieldKind;
    label(ui, st.t("prop.custom_fields"));
    for (fid, name, kind, choices) in fields {
        label(ui, &name);
        let value = |t: &Task| match t.custom.get(&fid) {
            None => String::new(),
            Some(CustomValue::Date(d)) => format::iso_date(Some(*d)),
            Some(CustomValue::Number(n)) => format!("{n}"),
            Some(other) => doc.project.custom_value_text(fid, other),
        };
        let current = common(ids.iter().filter_map(|id| doc.project.task(*id)).map(&value));
        let apply = |doc: &mut Document, st: &mut UiState, text: String| {
            let ids = ids.to_vec();
            edit(doc, st, "Custom field", move |p| {
                let listed = p.custom_field(fid).is_some_and(|f| f.choices.iter().any(|c| c == crate::fields::NO_VALUE));
                let text = if text == crate::fields::NO_VALUE && !listed { "" } else { text.as_str() };
                let value = p.parse_custom_value(fid, text).map_err(|e| e.to_string())?;
                for id in &ids {
                    p.set_custom_value(*id, fid, value.clone()).map_err(|e| e.to_string())?;
                }
                Ok(())
            });
        };
        let options: Vec<String> = match kind {
            CustomFieldKind::Flag => vec!["Yes".into(), "No".into(), crate::fields::NO_VALUE.into()],
            CustomFieldKind::Text if !choices.is_empty() => {
                choices.iter().cloned().chain(std::iter::once(crate::fields::NO_VALUE.to_string())).collect()
            }
            _ => Vec::new(),
        };
        if options.is_empty() {
            let hint = match kind {
                CustomFieldKind::Date => "YYYY-MM-DD",
                CustomFieldKind::Duration => "e.g. 3d",
                CustomFieldKind::Number | CustomFieldKind::Cost => "number",
                _ => "",
            };
            if let Some(text) = line(ui, ("task_custom", fid.0, first, multi), current.as_deref().unwrap_or(""), hint) {
                apply(doc, st, text);
            }
        } else {
            let shown = match current.as_deref() {
                Some("") | None => crate::fields::NO_VALUE.to_string(),
                Some(v) => v.to_string(),
            };
            let mut pick = None;
            egui::ComboBox::from_id_salt(("task_custom", fid.0, first, multi)).selected_text(shown).show_ui(ui, |ui| {
                for o in &options {
                    if ui.selectable_label(current.as_deref() == Some(o), o).clicked() {
                        pick = Some(if o == crate::fields::NO_VALUE { String::new() } else { o.clone() });
                    }
                }
            });
            if let Some(text) = pick {
                apply(doc, st, text);
            }
        }
    }
}

/// Duration, dates, bounds and group-share fields: the scheduling heart of the General tab.
fn schedule_fields(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    snap: &Snap,
    first: TaskId,
    multi: bool,
    any_summary: bool,
    milestone_only: bool,
    share: bool,
) {
    let derived_duration = common(ids.iter().filter_map(|id| doc.project.task(*id)).map(|t| doc.project.format_task_duration(t)));
    label(ui, st.t("prop.duration"));
    if any_summary {
        // A group's span comes from its subtasks: visible but not editable.
        derived_line(ui, derived_duration.as_deref().unwrap_or_else(|| st.t("common.multiple_lc")), st.t("prop.from_children"));
    } else if milestone_only {
        // A milestone is always 0: greyed rather than hidden.
        derived_line(ui, snap.duration.as_deref().unwrap_or(""), st.t("prop.milestone_duration"));
    } else if let Some(text) = line(ui, ("task_dur", first, multi), snap.duration.as_deref().unwrap_or(""), "") {
        let ids = ids.to_vec();
        edit(doc, st, "Duration", move |p| {
            let (minutes, estimated) = p.parse_duration(&text).ok_or_else(|| format!("'{text}' is not a valid duration"))?;
            for id in &ids {
                p.set_duration(*id, minutes, estimated).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
    }
    let start_minute = doc.project.settings.time.default_start_time;
    let finish_minute = doc.project.settings.time.default_finish_time;
    if milestone_only {
        // A milestone is one moment, not a span: one rule, on the finish side (the Swift
        // app's canonical point for a leaf milestone).
        let side = DateSide {
            finish: true,
            minute: start_minute,
            scheduled: snap.start,
            mode: snap.finish_mode,
            min: snap.finish_earliest,
            max: snap.finish_latest,
            fixed: snap.finish_fixed,
        };
        date_mode_side(ui, doc, st, ids, first, multi, st.t("prop.moment"), side);
    } else {
        let side = DateSide {
            finish: false,
            minute: start_minute,
            scheduled: snap.start,
            mode: snap.start_mode,
            min: snap.start_earliest,
            max: snap.start_latest,
            fixed: snap.start_fixed,
        };
        date_mode_side(ui, doc, st, ids, first, multi, st.t("prop.start"), side);
        let side = DateSide {
            finish: true,
            minute: finish_minute,
            scheduled: snap.finish,
            mode: snap.finish_mode,
            min: snap.finish_earliest,
            max: snap.finish_latest,
            fixed: snap.finish_fixed,
        };
        date_mode_side(ui, doc, st, ids, first, multi, st.t("prop.finish"), side);
    }
    if share {
        // Stretch and percent-of-group both size a share: setting one clears the other.
        let mut stretch = snap.stretch.unwrap_or(false);
        if ui.checkbox(&mut stretch, st.t("prop.stretch")).changed() {
            let ids = ids.to_vec();
            edit(doc, st, "Stretch", move |p| {
                for id in &ids {
                    if let Some(t) = p.task_mut(*id) {
                        t.fills_group = stretch;
                        if stretch {
                            t.duration_percent = None;
                        }
                    }
                }
                Ok(())
            });
        }
        label(ui, st.t("prop.percent_group"));
        if let Some(text) = line(ui, ("task_pct_group", first, multi), snap.percent_group.as_deref().unwrap_or(""), st.t("common.none_lc"))
        {
            let ids = ids.to_vec();
            edit(doc, st, "Percent of group", move |p| {
                let pct = if text.trim().is_empty() {
                    None
                } else {
                    Some(text.trim().parse::<f64>().map_err(|_| "Percent of group must be a number")?)
                };
                for id in &ids {
                    if let Some(t) = p.task_mut(*id) {
                        t.duration_percent = pct;
                        if pct.is_some() {
                            t.fills_group = false;
                        }
                    }
                }
                Ok(())
            });
        }
    }
}

/// Advanced fields, in parity with Task Information's Advanced tab.
fn planning_tab(ui: &mut Ui, doc: &mut Document, st: &mut UiState, ids: &[TaskId], snap: &Snap, first: TaskId, multi: bool) {
    let mut active = snap.active.unwrap_or(true);
    if ui.checkbox(&mut active, st.t("prop.active")).changed() {
        let ids = ids.to_vec();
        edit(doc, st, "Active", move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.active = active;
                }
            }
            Ok(())
        });
    }
    let mut on_tl = snap.on_timeline.unwrap_or(false);
    if ui.checkbox(&mut on_tl, st.t("prop.on_timeline")).changed() {
        let ids = ids.to_vec();
        edit(doc, st, "Display on Timeline", move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.show_on_timeline = on_tl;
                }
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.priority"));
    if let Some(text) = line(ui, ("task_prio", first, multi), snap.priority.as_deref().unwrap_or(""), "500") {
        let ids = ids.to_vec();
        edit(doc, st, "Priority", move |p| {
            let v = text.trim().parse::<u16>().map_err(|_| "Priority must be a number from 0 to 1000")?.min(1000);
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.priority = v;
                }
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.mode"));
    let auto = st.t("prop.mode_auto");
    let manual = st.t("prop.mode_manual");
    let mode_label = snap.mode.map(|m| if m == ScheduleMode::Auto { auto } else { manual }).unwrap_or(st.t("common.multiple"));
    egui::ComboBox::from_id_salt(("task_mode", first, multi)).selected_text(mode_label).show_ui(ui, |ui| {
        for (mode, text) in [(ScheduleMode::Auto, auto), (ScheduleMode::Manual, manual)] {
            if ui.selectable_label(snap.mode == Some(mode), text).clicked() && snap.mode != Some(mode) {
                let ids = ids.to_vec();
                edit(doc, st, "Schedule mode", move |p| p.set_schedule_mode(&ids, mode).map_err(|e| e.to_string()));
            }
        }
    });
    label(ui, st.t("prop.deadline"));
    let deadline_minute = doc.project.settings.time.default_finish_time;
    if let Some(next) = show_date(ui, &format!("task_deadline{first}"), snap.deadline, true, true, deadline_minute) {
        let ids = ids.to_vec();
        edit(doc, st, "Deadline", move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.deadline = next;
                }
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.constraint"));
    let kind_label = snap.constraint.map(|k| k.label()).unwrap_or(st.t("common.multiple"));
    egui::ComboBox::from_id_salt(("task_constraint", first, multi)).selected_text(kind_label).show_ui(ui, |ui| {
        for kind in ConstraintKind::ALL {
            if ui.selectable_label(snap.constraint == Some(kind), kind.label()).clicked() && snap.constraint != Some(kind) {
                let ids = ids.to_vec();
                edit(doc, st, "Constraint", move |p| {
                    for id in &ids {
                        if let Some(t) = p.task_mut(*id) {
                            t.constraint.kind = kind;
                            if kind.needs_date() && t.constraint.date.is_none() {
                                t.constraint.date = if kind.is_start_constraint() { t.sched.start } else { t.sched.finish };
                            }
                            if !kind.needs_date() {
                                t.constraint.date = None;
                            }
                        }
                    }
                    Ok(())
                });
            }
        }
    });
    if snap.constraint.is_some_and(|k| k.needs_date()) {
        label(ui, st.t("prop.constraint_date"));
        let start_side = snap.constraint.is_some_and(|k| k.is_start_constraint());
        let minute = if start_side { doc.project.settings.time.default_start_time } else { doc.project.settings.time.default_finish_time };
        if let Some(Some(date)) = show_date(ui, &format!("task_cdate{first}"), snap.constraint_date, false, true, minute) {
            let ids = ids.to_vec();
            edit(doc, st, "Constraint date", move |p| {
                for id in &ids {
                    if let Some(t) = p.task_mut(*id) {
                        t.constraint.date = Some(date);
                    }
                }
                Ok(())
            });
        }
    }
    label(ui, st.t("prop.task_type"));
    let type_name = |t: TaskType| match t {
        TaskType::FixedUnits => "Fixed Units",
        TaskType::FixedDuration => "Fixed Duration",
        TaskType::FixedWork => "Fixed Work",
    };
    let tt_label = snap.task_type.map(type_name).unwrap_or(st.t("common.multiple"));
    egui::ComboBox::from_id_salt(("task_type", first, multi)).selected_text(tt_label).show_ui(ui, |ui| {
        for tt in [TaskType::FixedUnits, TaskType::FixedDuration, TaskType::FixedWork] {
            if ui.selectable_label(snap.task_type == Some(tt), type_name(tt)).clicked() && snap.task_type != Some(tt) {
                let ids = ids.to_vec();
                edit(doc, st, "Task type", move |p| {
                    for id in &ids {
                        if let Some(t) = p.task_mut(*id) {
                            t.task_type = tt;
                        }
                    }
                    Ok(())
                });
            }
        }
    });
    let mut effort = snap.effort.unwrap_or(false);
    if ui.checkbox(&mut effort, st.t("prop.effort_driven")).changed() {
        let ids = ids.to_vec();
        edit(doc, st, "Effort driven", move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.effort_driven = effort;
                }
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.calendar"));
    let calendars: Vec<(Option<CalendarId>, String)> = std::iter::once((None, st.t("common.none").to_string()))
        .chain(doc.project.calendars.iter().map(|c| (Some(c.id), c.name.clone())))
        .collect();
    let cal_label = match snap.calendar {
        None => st.t("common.multiple").to_string(),
        Some(current) => calendars.iter().find(|(id, _)| *id == current).map(|(_, n)| n.clone()).unwrap_or_default(),
    };
    egui::ComboBox::from_id_salt(("task_cal", first, multi)).selected_text(cal_label).show_ui(ui, |ui| {
        for (cal, name) in &calendars {
            if ui.selectable_label(snap.calendar == Some(*cal), name).clicked() && snap.calendar != Some(*cal) {
                let (cal, ids) = (*cal, ids.to_vec());
                edit(doc, st, "Task calendar", move |p| {
                    for id in &ids {
                        if let Some(t) = p.task_mut(*id) {
                            t.calendar = cal;
                        }
                    }
                    Ok(())
                });
            }
        }
    });
    let mut ignore = snap.ignore_res_cal.unwrap_or(false);
    if ui.checkbox(&mut ignore, st.t("prop.ignore_res_cal")).changed() {
        let ids = ids.to_vec();
        edit(doc, st, "Ignore resource calendars", move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.ignore_resource_calendars = ignore;
                }
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.fixed_cost"));
    if let Some(text) = line(ui, ("task_fixed_cost", first, multi), snap.fixed_cost.as_deref().unwrap_or(""), "0.00") {
        let ids = ids.to_vec();
        edit(doc, st, "Fixed cost", move |p| {
            let v = format::parse_money(&text).ok_or("invalid amount")?;
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    t.fixed_cost = v;
                }
            }
            Ok(())
        });
    }
    estimate_block(ui, doc, st, first, multi);
    custom_fields(ui, doc, st, ids, first, multi);
}

/// The estimate, through the same commit path as the table's Estimate columns.
fn estimate_block(ui: &mut Ui, doc: &mut Document, st: &mut UiState, first: TaskId, multi: bool) {
    label(ui, st.t("prop.estimate"));
    if multi {
        ui.label(RichText::new(st.t("common.multiple_lc")).color(theme::pal().TEXT_WEAK));
        return;
    }
    use crate::columns::TaskCol;
    let Some(idx) = doc.project.task_index(first) else { return };
    let none = |_: TaskId| false;
    let cctx = crate::columns::CellCtx { overallocated: &none, others_on: None };
    let kind = crate::columns::cell(TaskCol::EstimateMethod, &doc.project, idx, &cctx).text;
    egui::ComboBox::from_id_salt(("task_est", first)).selected_text(kind.clone()).show_ui(ui, |ui| {
        for choice in ["None", "Complexity", "Three-point", "Work"] {
            if ui.selectable_label(kind == choice, choice).clicked() && kind != choice {
                edit(doc, st, "Estimate", move |p| crate::columns::commit(TaskCol::EstimateMethod, p, first, choice));
            }
        }
    });
    let fields: &[(TaskCol, &str)] = match kind.as_str() {
        "Complexity" => &[(TaskCol::Points, "Points")],
        "Three-point" => {
            &[(TaskCol::Optimistic, "Optimistic"), (TaskCol::MostLikely, "Most likely"), (TaskCol::Pessimistic, "Pessimistic")]
        }
        "Work" => &[(TaskCol::Work, "Work")],
        _ => &[],
    };
    for (col, name) in fields {
        label(ui, name);
        let current = crate::columns::cell(*col, &doc.project, idx, &cctx).text;
        if let Some(text) = line(ui, ("task_est_field", first, name), &current, "") {
            let col = *col;
            edit(doc, st, "Estimate", move |p| crate::columns::commit(col, p, first, &text));
        }
    }
    if kind != "None" {
        let expected = crate::columns::cell(TaskCol::ExpectedWork, &doc.project, idx, &cctx).text;
        if !expected.is_empty() {
            label(ui, "Expected work");
            derived_line(ui, &expected, "");
        }
    }
}

fn resources_tab(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    snap: &Snap,
    first: TaskId,
    multi: bool,
    any_summary: bool,
) {
    label(ui, st.t("prop.resources"));
    if any_summary {
        let text = match snap.resources.as_deref() {
            None => st.t("common.multiple_lc"),
            Some(s) if s.trim().is_empty() => st.t("common.none_lc"),
            Some(s) => s,
        };
        ui.label(text).on_hover_text(st.t("prop.group_resources"));
        return;
    }
    resources(ui, doc, st, ids, snap.resources.as_deref());
    // One task: its assignments with editable units and work, like the dialog's Resources tab.
    if !multi {
        let rows: Vec<(AssignmentId, String, f64, Minutes)> = doc
            .project
            .assignments_for_task(first)
            .map(|a| {
                let name = doc.project.resource(a.resource).map(|r| r.name.clone()).unwrap_or_default();
                (a.id, name, a.units, a.work)
            })
            .collect();
        for (aid, name, units, work) in rows {
            label(ui, &name);
            ui.horizontal(|ui| {
                ui.label(RichText::new(st.t("prop.units")).size(11.0).color(theme::pal().TEXT_WEAK));
                if let Some(text) = line(ui, ("asg_units", aid.0), &format::units(units), "100%") {
                    edit(doc, st, "Assignment units", move |p| {
                        let u = format::parse_units(&text).ok_or_else(|| format!("'{text}' is not valid units"))?;
                        p.set_assignment_units(aid, u).map_err(|e| e.to_string())
                    });
                }
                ui.label(RichText::new(st.t("prop.work")).size(11.0).color(theme::pal().TEXT_WEAK));
                if let Some(text) = line(ui, ("asg_work", aid.0), &format_work(work), "") {
                    edit(doc, st, "Assignment work", move |p| {
                        let w = parse_work(&text, &p.settings.time).ok_or_else(|| format!("'{text}' is not valid work"))?;
                        p.set_assignment_work(aid, w).map_err(|e| e.to_string())
                    });
                }
            });
        }
    }
}

/// The task's status, as a combo on a leaf and read-only roll-up on a group (SPECS.md).
fn status_block(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    snap: &Snap,
    first: TaskId,
    multi: bool,
    any_summary: bool,
) {
    label(ui, st.t("prop.status"));
    if any_summary {
        // A group's status rolls up from its subtasks (SPECS.md): shown, not edited here.
        let derived_status = common(ids.iter().filter_map(|id| doc.project.task(*id)).map(|t| {
            let pct = t.sched.percent_complete;
            crate::lang::status_name(if pct >= 100.0 {
                TaskStatus::Done
            } else if pct <= 0.0 {
                TaskStatus::NotStarted
            } else {
                TaskStatus::Busy
            })
            .to_string()
        }));
        ui.label(derived_status.unwrap_or_else(|| st.t("common.multiple_lc").to_string())).on_hover_text(st.t("prop.from_children"));
    } else {
        let status_label =
            snap.status.map(crate::lang::status_name).unwrap_or(if multi { st.t("common.multiple") } else { st.t("status.not_started") });
        egui::ComboBox::from_id_salt(("task_status", first, multi)).selected_text(status_label).show_ui(ui, |ui| {
            for status in TaskStatus::ALL {
                if ui.selectable_label(snap.status == Some(status), crate::lang::status_name(status)).clicked()
                    && snap.status != Some(status)
                {
                    let ids = ids.to_vec();
                    edit(doc, st, "Status", move |p| {
                        for id in &ids {
                            p.set_task_status(*id, status).map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    });
                }
            }
        });
    }
}

fn percent_block(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    snap: &Snap,
    first: TaskId,
    multi: bool,
    any_summary: bool,
) {
    label(ui, st.t("prop.percent"));
    if any_summary {
        let derived = common(ids.iter().filter_map(|id| doc.project.task(*id)).map(|t| format::percent(t.sched.percent_complete)));
        derived_line(ui, derived.as_deref().unwrap_or_else(|| st.t("common.multiple_lc")), st.t("prop.from_children"));
    } else if let Some(text) = line(ui, ("task_pc", first, multi), snap.percent.as_deref().unwrap_or(""), "0") {
        let ids = ids.to_vec();
        edit(doc, st, "Percent complete", move |p| {
            let pct =
                text.trim().trim_end_matches('%').trim().parse::<u8>().map_err(|_| "Percent complete must be a number from 0 to 100")?;
            if pct > 100 {
                return Err("Percent complete must be a number from 0 to 100".into());
            }
            for id in &ids {
                p.set_percent_complete(*id, pct).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
    }
}

fn progress_tab(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    snap: &Snap,
    first: TaskId,
    multi: bool,
    any_summary: bool,
    milestone_only: bool,
) {
    status_block(ui, doc, st, ids, snap, first, multi, any_summary);
    percent_block(ui, doc, st, ids, snap, first, multi, any_summary);
    // Actual dates with the same rules as the table's columns (progress keeps its start, a
    // finish completes the task); the pickers' "geen" clears them.
    let start_minute = doc.project.settings.time.default_start_time;
    let finish_minute = doc.project.settings.time.default_finish_time;
    label(ui, st.t("prop.actual_start"));
    if let Some(next) = show_date(ui, &format!("task_astart{first}"), snap.actual_start, true, !any_summary, start_minute) {
        let ids = ids.to_vec();
        edit(doc, st, "Actual start", move |p| {
            for id in &ids {
                crate::columns::set_actual_start(p, *id, next)?;
            }
            Ok(())
        });
    }
    label(ui, st.t("prop.actual_finish"));
    if let Some(next) = show_date(ui, &format!("task_afinish{first}"), snap.actual_finish, true, !any_summary, finish_minute) {
        let ids = ids.to_vec();
        edit(doc, st, "Actual finish", move |p| {
            for id in &ids {
                crate::columns::set_actual_finish(p, *id, next)?;
            }
            Ok(())
        });
    }
    // Work rolls up from the assignments: shown, not edited here.
    let actual_work = common(ids.iter().filter_map(|id| doc.project.task(*id)).map(|t| format_work(t.sched.actual_work)));
    label(ui, st.t("prop.actual_work"));
    derived_line(ui, actual_work.as_deref().unwrap_or_else(|| st.t("common.multiple_lc")), st.t("prop.from_children"));
    let remaining =
        common(ids.iter().filter_map(|id| doc.project.task(*id)).map(|t| format_work((t.sched.work - t.sched.actual_work).max(0))));
    label(ui, st.t("prop.remaining_work"));
    derived_line(ui, remaining.as_deref().unwrap_or_else(|| st.t("common.multiple_lc")), st.t("prop.from_children"));
    if !any_summary && !milestone_only {
        label(ui, st.t("prop.stop"));
        let stop_text = match snap.stop {
            None => st.t("common.multiple_lc").to_string(),
            Some(None) => "NA".to_string(),
            Some(Some(d)) => crate::widgets::date_picker::format_dutch(d),
        };
        derived_line(ui, &stop_text, "End of the work done so far");
        label(ui, st.t("prop.resume"));
        let has_stop = matches!(snap.stop, Some(Some(_)));
        if let Some(next) = show_date(ui, &format!("task_resume{first}"), snap.resume, true, has_stop, start_minute) {
            let ids = ids.to_vec();
            edit(doc, st, "Resume", move |p| {
                for id in &ids {
                    p.set_resume(*id, next).map_err(|e| e.to_string())?;
                }
                Ok(())
            });
        }
    }
    // Baseline and variance, read-only, when a baseline was saved.
    if !multi
        && let Some(t) = doc.project.task(first)
        && let Some(b) = t.baseline
    {
        label(ui, st.t("prop.baseline"));
        derived_line(ui, &format!("{} – {}", format::date(Some(b.start)), format::date(Some(b.finish))), "");
        if let Some(f) = t.sched.finish {
            let wc = plancraft_core::schedule::project_calendar(&doc.project);
            let v = wc.between(b.finish, f);
            label(ui, st.t("prop.variance"));
            let text = if v == 0 {
                doc.project.format_duration(0, false)
            } else {
                format!("{}{}", if v > 0 { "+" } else { "-" }, doc.project.format_duration(v.abs(), false))
            };
            derived_line(ui, &text, "");
        }
    }
}

struct Snap {
    name: Option<String>,
    milestone: Option<bool>,
    duration: Option<String>,
    start: Option<Option<DateTime>>,
    finish: Option<Option<DateTime>>,
    start_mode: Option<DateMode>,
    finish_mode: Option<DateMode>,
    start_earliest: Option<Option<DateTime>>,
    start_latest: Option<Option<DateTime>>,
    finish_earliest: Option<Option<DateTime>>,
    finish_latest: Option<Option<DateTime>>,
    start_fixed: Option<bool>,
    finish_fixed: Option<bool>,
    stretch: Option<bool>,
    percent_group: Option<String>,
    resources: Option<String>,
    percent: Option<String>,
    status: Option<TaskStatus>,
    mark: Option<Option<RowMark>>,
    notes: Option<String>,
    active: Option<bool>,
    on_timeline: Option<bool>,
    priority: Option<String>,
    deliverable: Option<String>,
    mode: Option<ScheduleMode>,
    deadline: Option<Option<DateTime>>,
    constraint: Option<ConstraintKind>,
    constraint_date: Option<Option<DateTime>>,
    task_type: Option<TaskType>,
    effort: Option<bool>,
    calendar: Option<Option<CalendarId>>,
    ignore_res_cal: Option<bool>,
    fixed_cost: Option<String>,
    actual_start: Option<Option<DateTime>>,
    actual_finish: Option<Option<DateTime>>,
    stop: Option<Option<DateTime>>,
    resume: Option<Option<DateTime>>,
}

fn snapshot(p: &Project, ids: &[TaskId]) -> Snap {
    let text = |f: &dyn Fn(&Task) -> String| common(ids.iter().filter_map(|id| p.task(*id)).map(f));
    let flag = |f: &dyn Fn(&Task) -> bool| common_flag(ids.iter().filter_map(|id| p.task(*id)).map(f));
    Snap {
        name: text(&|t| t.name.clone()),
        milestone: flag(&|t| t.is_milestone()),
        duration: text(&|t| p.format_task_minutes(t, t.duration, t.estimated)),
        start: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.sched.start.or(t.manual_start))),
        finish: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.sched.finish.or(t.manual_finish))),
        start_mode: common_eq(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.start_mode())),
        finish_mode: common_eq(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.finish_mode())),
        // The windows, not the raw bounds: a legacy SNET-style pin or a manual date shows
        // as the side's minimum or maximum until an edit absorbs it into the bounds.
        start_earliest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.start_window().0)),
        start_latest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.start_window().1)),
        finish_earliest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.finish_window().0)),
        finish_latest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.finish_window().1)),
        start_fixed: flag(&|t| t.bounds.start_latest_fixed),
        finish_fixed: flag(&|t| t.bounds.finish_latest_fixed),
        stretch: flag(&|t| t.fills_group),
        percent_group: text(&|t| t.duration_percent.map(|n| n.to_string()).unwrap_or_default()),
        resources: text(&|t| p.format_resource_names_only(t.id)),
        percent: text(&|t| t.percent_complete.to_string()),
        status: common_status(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.status)),
        mark: common_mark(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.row_mark)),
        notes: text(&|t| t.notes.clone()),
        active: flag(&|t| t.active),
        on_timeline: flag(&|t| t.show_on_timeline),
        priority: text(&|t| t.priority.to_string()),
        deliverable: text(&|t| t.deliverable.clone()),
        mode: common_eq(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.mode)),
        deadline: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.deadline)),
        constraint: common_eq(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.constraint.kind)),
        constraint_date: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.constraint.date)),
        task_type: common_eq(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.task_type)),
        effort: flag(&|t| t.effort_driven),
        calendar: common_eq(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.calendar)),
        ignore_res_cal: flag(&|t| t.ignore_resource_calendars),
        fixed_cost: text(&|t| if t.fixed_cost == 0.0 { String::new() } else { format!("{:.2}", t.fixed_cost) }),
        actual_start: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.actual_start)),
        actual_finish: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.actual_finish)),
        stop: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.sched.stop)),
        resume: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.sched.resume)),
    }
}

fn common_eq<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn common(mut values: impl Iterator<Item = String>) -> Option<String> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn common_flag(mut values: impl Iterator<Item = bool>) -> Option<bool> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn common_dt(mut values: impl Iterator<Item = Option<DateTime>>) -> Option<Option<DateTime>> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn common_status(mut values: impl Iterator<Item = TaskStatus>) -> Option<TaskStatus> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn common_mark(mut values: impl Iterator<Item = Option<RowMark>>) -> Option<Option<RowMark>> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn can_share(p: &Project, id: TaskId) -> bool {
    let Some(idx) = p.task_index(id) else { return false };
    let Some(task) = p.task(id) else { return false };
    p.parent_index(idx).is_some() && !p.has_children(idx) && !task.is_milestone()
}

fn show_date(
    ui: &mut Ui,
    salt: &str,
    value: Option<Option<DateTime>>,
    clearable: bool,
    enabled: bool,
    minute: u32,
) -> Option<Option<DateTime>> {
    let (current, empty) = match value {
        None => (None, crate::lang::text("common.multiple_lc")),
        Some(None) => (None, crate::lang::text("common.none_lc")),
        Some(Some(dt)) => (Some(dt), crate::lang::text("common.none_lc")),
    };
    crate::widgets::date_picker::button(ui, crate::widgets::date_picker::DateField { salt, current, clearable, enabled, minute, empty })
}

/// One side (start or finish) of the date rules: everything [`date_mode_side`] needs.
struct DateSide {
    finish: bool,
    minute: u32,
    scheduled: Option<Option<DateTime>>,
    mode: Option<DateMode>,
    min: Option<Option<DateTime>>,
    max: Option<Option<DateTime>>,
    fixed: Option<bool>,
}

/// One side's date rule, the Project Planner way: a mode picker (Unrestricted, On, no earlier,
/// no later, Between), only the date editors that mode needs, and the calculated date greyed
/// when the mode doesn't pin it. An unrestricted side explains itself on hover.
fn date_mode_side(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    first: TaskId,
    multi: bool,
    title: &str,
    side: DateSide,
) {
    label(ui, title);
    let DateSide { finish, minute, scheduled, mode, min, max, fixed } = side;
    // The calculated date: greyed, because the mode's own editors below do the pinning.
    if mode != Some(DateMode::On) {
        let resp = ui.scope(|ui| show_date(ui, &format!("sched_{finish}{first}"), scheduled, false, false, minute)).response;
        if mode == Some(DateMode::Unrestricted) {
            resp.on_hover_text(st.t("dm.calc_deps"));
        }
    }
    let shown = mode.map(crate::lang::date_mode_name).unwrap_or_else(|| st.t("common.multiple_lc"));
    let mut pick = None;
    egui::ComboBox::from_id_salt(("date_mode", finish, first, multi)).width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
        for m in DateMode::ALL {
            if ui.selectable_label(mode == Some(m), crate::lang::date_mode_name(m)).clicked() {
                pick = Some(m);
            }
        }
    });
    if let Some(m) = pick
        && Some(m) != mode
    {
        let ids = ids.to_vec();
        edit(doc, st, if finish { "Finish rule" } else { "Start rule" }, move |p| {
            for id in &ids {
                p.set_date_mode(*id, finish, m).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        return; // the editors below would show the old mode's fields this frame
    }
    let write = |doc: &mut Document, st: &mut UiState, ids: &[TaskId], to_min: bool, date: DateTime| {
        let ids = ids.to_vec();
        edit(doc, st, if finish { "Finish rule" } else { "Start rule" }, move |p| {
            for id in &ids {
                let Some(t) = p.task(*id) else { continue };
                let (mn, mx) = if finish { t.finish_window() } else { t.start_window() };
                let (mn, mx) = if to_min { (Some(date), mx) } else { (mn, Some(date)) };
                p.set_date_window(*id, finish, mn, mx).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
    };
    match mode {
        Some(DateMode::On) => {
            // The single pinned date: picking one writes both the minimum and the maximum.
            if let Some(Some(date)) = show_date(ui, &format!("on_{finish}{first}"), min, false, true, minute) {
                let ids = ids.to_vec();
                edit(doc, st, if finish { "Finish rule" } else { "Start rule" }, move |p| {
                    for id in &ids {
                        p.set_date_window(*id, finish, Some(date), Some(date)).map_err(|e| e.to_string())?;
                    }
                    Ok(())
                });
            }
        }
        Some(DateMode::NoEarlier) => {
            if let Some(Some(date)) = show_date(ui, &format!("min_{finish}{first}"), min, false, true, minute) {
                write(doc, st, ids, true, date);
            }
        }
        Some(DateMode::NoLater) => {
            if let Some(Some(date)) = show_date(ui, &format!("max_{finish}{first}"), max, false, true, minute) {
                write(doc, st, ids, false, date);
            }
        }
        Some(DateMode::Between) => {
            if let Some(Some(date)) = show_date(ui, &format!("min_{finish}{first}"), min, false, true, minute) {
                write(doc, st, ids, true, date);
            }
            if let Some(Some(date)) = show_date(ui, &format!("max_{finish}{first}"), max, false, true, minute) {
                write(doc, st, ids, false, date);
            }
        }
        Some(DateMode::Unrestricted) | None => {}
    }
    // The "moves the plan" tick belongs to the side's maximum, so it hides with it.
    if matches!(mode, Some(DateMode::On) | Some(DateMode::NoLater) | Some(DateMode::Between)) {
        let key = if finish { "prop.finish_fixed" } else { "prop.start_fixed" };
        fixed_box(ui, doc, st, ids, st.t(key), fixed, !finish, true);
    }
}

/// The "moves the plan" tick under its "no later than" date. It only means something once that
/// date is set, so it is greyed (not hidden) without one, and indented to sit with its date.
fn fixed_box(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    title: &str,
    current: Option<bool>,
    start: bool,
    has_date: bool,
) {
    let mut on = current.unwrap_or(false);
    let changed = ui
        .horizontal(|ui| {
            ui.add_space(12.0);
            ui.add_enabled(has_date, egui::Checkbox::new(&mut on, title)).changed()
        })
        .inner;
    if changed {
        let ids = ids.to_vec();
        edit(doc, st, title, move |p| {
            for id in &ids {
                if let Some(t) = p.task_mut(*id) {
                    if start {
                        t.bounds.start_latest_fixed = on;
                    } else {
                        t.bounds.finish_latest_fixed = on;
                    }
                }
            }
            Ok(())
        });
    }
    if current.is_none() {
        ui.label(RichText::new(st.t("prop.differs")).size(11.0).color(theme::pal().TEXT_WEAK));
    }
}

fn label(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(11.0).color(theme::pal().TEXT_WEAK));
}

fn line(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, current: &str, hint: &str) -> Option<String> {
    let id = ui.id().with(salt);
    let mut text = ui.ctx().data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
    let resp = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(ui.available_width()).hint_text(hint));
    if resp.lost_focus() {
        ui.ctx().data_mut(|d| d.remove::<String>(id));
        if text.trim() != current.trim() {
            return Some(text);
        }
    } else if resp.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(id, text));
    }
    None
}

fn notes(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, current: &str) -> Option<String> {
    let id = ui.id().with(salt);
    let mut text = ui.ctx().data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
    let resp = ui.add(egui::TextEdit::multiline(&mut text).id(id).desired_width(ui.available_width()).desired_rows(4));
    if resp.lost_focus() {
        ui.ctx().data_mut(|d| d.remove::<String>(id));
        if text != current {
            return Some(text);
        }
    } else if resp.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(id, text));
    }
    None
}

fn edit(doc: &mut Document, st: &mut UiState, label: &str, f: impl FnOnce(&mut Project) -> Result<(), String>) {
    if let Err(e) = doc.edit_msg(label, f) {
        st.toast(ToastKind::Error, e);
    }
}

fn predecessors(ui: &mut Ui, doc: &mut Document, st: &mut UiState, ids: &[TaskId], multi: bool) {
    let Some(&first) = ids.first() else { return };
    if !multi {
        let links: Vec<(TaskRef, LinkType, Lag)> =
            doc.project.task(first).map(|t| t.predecessors.iter().map(|d| (d.predecessor, d.kind, d.lag)).collect()).unwrap_or_default();
        let mpd = doc.project.settings.time.minutes_per_day.max(1);
        for (pred, kind, lag) in links {
            ui.horizontal(|ui| {
                let row = pred.local().and_then(|id| doc.project.task_index(id)).map(|i| i.saturating_add(1));
                let name = doc.project.task_ref_name(&pred);
                let title = match row {
                    Some(n) => format!("{n}  {name}"),
                    None => name,
                };
                ui.label(RichText::new(title).color(theme::pal().TEXT));
                egui::ComboBox::from_id_salt(("pred_kind", first, pred)).width(52.0).selected_text(kind.code()).show_ui(ui, |ui| {
                    for next in LinkType::ALL {
                        if ui.selectable_label(kind == next, next.code()).clicked() {
                            edit(doc, st, "Predecessor", move |p| {
                                p.link(pred, first, next, lag).map_err(|e| e.to_string())?;
                                Ok(())
                            });
                        }
                    }
                });
                let mut days = lag_days(lag, mpd);
                if ui.add(egui::DragValue::new(&mut days).range(-365.0..=365.0).speed(1.0).suffix("d")).changed() {
                    let minutes = (days as i64).saturating_mul(mpd);
                    let next_lag = if minutes == 0 { Lag::None } else { Lag::Working(minutes) };
                    edit(doc, st, "Lag", move |p| {
                        p.link(pred, first, kind, next_lag).map_err(|e| e.to_string())?;
                        Ok(())
                    });
                }
                if ui.button("×").on_hover_text(crate::lang::text("prop.remove_predecessor")).clicked() {
                    edit(doc, st, "Remove predecessor", move |p| {
                        p.unlink(pred, first).map_err(|e| e.to_string())?;
                        Ok(())
                    });
                }
            });
        }
    } else {
        ui.label(RichText::new(crate::lang::text("prop.add_fs")).size(11.0).color(theme::pal().TEXT_WEAK));
    }
    let choices: Vec<(TaskId, String)> = doc
        .project
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| ids.iter().all(|id| can_link(&doc.project, *id, t.id) && (multi || !already(&doc.project, *id, t.id))))
        .map(|(i, t)| (t.id, format!("{}  {}", i.saturating_add(1), t.name)))
        .collect();
    egui::ComboBox::from_id_salt(("pred_add", first)).selected_text(crate::lang::text("prop.add_predecessor")).show_ui(ui, |ui| {
        for (pred, title) in &choices {
            if ui.selectable_label(false, title).clicked() {
                let pred = *pred;
                let ids = ids.to_vec();
                edit(doc, st, "Add predecessor", move |p| {
                    for id in &ids {
                        if can_link(p, *id, pred) {
                            p.link(TaskRef::Local(pred), *id, LinkType::FS, Lag::None).map_err(|e| e.to_string())?;
                        }
                    }
                    Ok(())
                });
            }
        }
    });
}

fn resources(ui: &mut Ui, doc: &mut Document, st: &mut UiState, ids: &[TaskId], summary: Option<&str>) {
    let Some(&first) = ids.first() else { return };
    let label = match summary {
        None => crate::lang::text("common.multiple_lc"),
        Some(text) if text.trim().is_empty() => crate::lang::text("common.none_lc"),
        Some(text) => text,
    };
    let listed: Vec<(ResourceId, String)> = doc.project.resources.iter().map(|r| (r.id, r.name.clone())).collect();
    let button = ui.button(label).on_hover_text(crate::lang::text("prop.assigned"));
    egui::Popup::from_toggle_button_response(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(200.0);
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            for (rid, name) in &listed {
                let on_all = ids.iter().all(|id| assigned(&doc.project, *id, *rid));
                let mut on = on_all;
                if ui.checkbox(&mut on, name).changed() {
                    let rid = *rid;
                    let ids = ids.to_vec();
                    edit(doc, st, "Resources", move |p| set_assigned(p, &ids, rid, on));
                }
            }
        });
        let typed_id = ui.id().with(("new_res", first));
        let mut typed = ui.ctx().data(|d| d.get_temp::<String>(typed_id)).unwrap_or_default();
        let resp = ui.add(egui::TextEdit::singleline(&mut typed).hint_text(crate::lang::text("prop.new_resource")));
        if resp.lost_focus() && !typed.trim().is_empty() {
            let name = typed.trim().to_string();
            ui.ctx().data_mut(|d| d.insert_temp(typed_id, String::new()));
            let ids = ids.to_vec();
            edit(doc, st, "Resources", move |p| {
                let rid = p.add_resource(name);
                set_assigned(p, &ids, rid, true)
            });
        } else if resp.has_focus() || !typed.is_empty() {
            ui.ctx().data_mut(|d| d.insert_temp(typed_id, typed));
        }
    });
}

fn group_color(ui: &mut Ui, doc: &mut Document, st: &mut UiState, id: TaskId) {
    label(ui, st.t("prop.group_color"));
    let current = doc.project.task(id).and_then(|t| t.bar_color);
    ui.horizontal(|ui| {
        for rgb in theme::GROUP_SWATCHES {
            let on = current == Some(rgb);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 2.0, theme::paint_rgb(rgb));
            if on {
                ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, theme::pal().TEXT), egui::StrokeKind::Outside);
            }
            if resp.clicked() {
                edit(doc, st, "Group color", move |p| {
                    if let Some(t) = p.task_mut(id) {
                        t.bar_color = Some(rgb);
                    }
                    Ok(())
                });
            }
        }
        if ui.button(st.t("common.clear")).clicked() {
            edit(doc, st, "Group color", move |p| {
                if let Some(t) = p.task_mut(id) {
                    t.bar_color = None;
                }
                Ok(())
            });
        }
    });
}

fn assigned(p: &Project, task: TaskId, resource: ResourceId) -> bool {
    p.assignments_for_task(task).any(|a| a.resource == resource)
}

fn set_assigned(p: &mut Project, ids: &[TaskId], resource: ResourceId, on: bool) -> Result<(), String> {
    for id in ids {
        let existing = p.assignments_for_task(*id).find(|a| a.resource == resource).map(|a| a.id);
        if on {
            if existing.is_none() {
                p.assign(*id, resource, None, None).map_err(|e| e.to_string())?;
            }
        } else if let Some(aid) = existing {
            p.unassign(aid).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn can_link(p: &Project, succ: TaskId, pred: TaskId) -> bool {
    if succ == pred {
        return false;
    }
    let (Some(si), Some(pi)) = (p.task_index(succ), p.task_index(pred)) else { return false };
    !p.descendants_range(pi).contains(&si) && !p.descendants_range(si).contains(&pi)
}

fn already(p: &Project, succ: TaskId, pred: TaskId) -> bool {
    p.task(succ).is_some_and(|t| t.predecessors.iter().any(|d| d.predecessor.local() == Some(pred)))
}

fn lag_days(lag: Lag, per_day: Minutes) -> f64 {
    let minutes = match lag {
        Lag::None => 0,
        Lag::Working(m) | Lag::Elapsed(m) => m,
        Lag::Percent(_) => 0,
    };
    minutes as f64 / per_day.max(1) as f64
}

fn type_label(kind: ResourceType) -> &'static str {
    match kind {
        ResourceType::Work => "Work",
        ResourceType::Material => "Material",
        ResourceType::Cost => "Cost",
    }
}

fn pos2(x: f32, y: f32) -> egui::Pos2 {
    egui::pos2(x, y)
}
