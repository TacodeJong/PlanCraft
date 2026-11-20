//! The pane on the right of the active view. Values are read from the project each frame.

use egui::{Rect, RichText, Sense, Stroke, Ui, vec2};
use plancraft_core::prelude::*;
use plancraft_core::workspace::Document;

use crate::edit::EditExt;
use crate::format;
use crate::state::{ToastKind, UiState, ViewKind};
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
        ui.heading("Properties");
        ui.add_space(6.0);
        if resource_view(st.view) {
            match st.sel.single_resource().or_else(|| (st.view == ViewKind::Timesheet).then_some(st.timesheet_resource).flatten()) {
                Some(id) => resource_pane(ui, doc, st, id),
                None if st.sel.resources.len() > 1 => {
                    ui.label(RichText::new("Several resources are selected.").color(theme::pal().TEXT_WEAK));
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
    ui.label(RichText::new("No selection. Select an activity or a resource.").color(theme::pal().TEXT_WEAK));
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
    label(ui, "Name");
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
    label(ui, "Kind");
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
    label(ui, "Type");
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
    label(ui, "Email");
    if let Some(text) = line(ui, ("res_email", id), &email, "") {
        edit(doc, st, "Resource email", move |p| {
            if let Some(r) = p.resource_mut(id) {
                r.email = text.trim().to_string();
            }
            Ok(())
        });
    }
    label(ui, "Group");
    if let Some(text) = line(ui, ("res_group", id), &group, "") {
        edit(doc, st, "Resource group", move |p| {
            if let Some(r) = p.resource_mut(id) {
                r.group = text.trim().to_string();
            }
            Ok(())
        });
    }
    label(ui, "Max units");
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
        ui.label(RichText::new(format!("{} activities", ids.len())).color(theme::pal().TEXT_WEAK));
    }
    let snap = snapshot(&doc.project, &ids);
    let share = doc.project.settings.rules.enabled() && ids.iter().all(|id| can_share(&doc.project, *id));
    let any_summary = ids.iter().any(|id| doc.project.task_index(*id).is_some_and(|i| doc.project.has_children(i)));

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
        let ids = ids.clone();
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
        label(ui, "Outline");
        ui.label(format!("{}   level {}", doc.project.wbs(idx), doc.project.tasks.get(idx).map(|t| t.outline_level).unwrap_or(1)));
        if doc.project.has_children(idx) {
            group_color(ui, doc, st, first);
        }
    }
    if !any_summary {
        if snap.milestone.is_none() {
            ui.label(RichText::new("Milestone differs between the selection.").size(11.0).color(theme::pal().TEXT_WEAK));
        }
        let mut flag = snap.milestone.unwrap_or(false);
        if ui.checkbox(&mut flag, "Mark as milestone").changed() {
            let ids = ids.clone();
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
    }
    if !any_summary {
        label(ui, st.t("prop.duration"));
        if let Some(text) = line(ui, ("task_dur", first, multi), snap.duration.as_deref().unwrap_or(""), "") {
            let ids = ids.clone();
            edit(doc, st, "Duration", move |p| {
                let (minutes, estimated) = p.parse_duration(&text).ok_or_else(|| format!("'{text}' is not a valid duration"))?;
                for id in &ids {
                    p.set_duration(*id, minutes, estimated).map_err(|e| e.to_string())?;
                }
                Ok(())
            });
        }
    }
    let start_minute = doc.project.settings.time.default_start_time;
    let finish_minute = doc.project.settings.time.default_finish_time;
    label(ui, st.t("prop.start"));
    if let Some(Some(date)) = show_date(ui, &format!("task_start{first}"), snap.start, false, start_minute) {
        let ids = ids.clone();
        edit(doc, st, "Start", move |p| set_picked_date(p, &ids, date, false));
    }
    label(ui, st.t("prop.finish"));
    if let Some(Some(date)) = show_date(ui, &format!("task_finish{first}"), snap.finish, false, finish_minute) {
        let ids = ids.clone();
        edit(doc, st, "Finish", move |p| set_picked_date(p, &ids, date, true));
    }
    date_bound(ui, doc, st, &ids, first, snap.start_earliest, Bound::StartEarliest, start_minute);
    date_bound(ui, doc, st, &ids, first, snap.start_latest, Bound::StartLatest, start_minute);
    fixed_box(ui, doc, st, &ids, "Start no later than moves the plan", snap.start_fixed, true);
    date_bound(ui, doc, st, &ids, first, snap.finish_earliest, Bound::FinishEarliest, finish_minute);
    date_bound(ui, doc, st, &ids, first, snap.finish_latest, Bound::FinishLatest, finish_minute);
    fixed_box(ui, doc, st, &ids, "Finish no later than moves the plan", snap.finish_fixed, false);
    if share {
        let mut stretch = snap.stretch.unwrap_or(false);
        if ui.checkbox(&mut stretch, "Stretch to fill the group").changed() {
            let ids = ids.clone();
            edit(doc, st, "Stretch", move |p| {
                for id in &ids {
                    if let Some(t) = p.task_mut(*id) {
                        t.fills_group = stretch;
                    }
                }
                Ok(())
            });
        }
        label(ui, "Percent of group");
        if let Some(text) = line(ui, ("task_pct_group", first, multi), snap.percent_group.as_deref().unwrap_or(""), "none") {
            let ids = ids.clone();
            edit(doc, st, "Percent of group", move |p| {
                let pct = if text.trim().is_empty() {
                    None
                } else {
                    Some(text.trim().parse::<f64>().map_err(|_| "Percent of group must be a number")?)
                };
                for id in &ids {
                    if let Some(t) = p.task_mut(*id) {
                        t.duration_percent = pct;
                    }
                }
                Ok(())
            });
        }
    }
    label(ui, st.t("prop.predecessors"));
    predecessors(ui, doc, st, &ids, multi);
    label(ui, st.t("prop.resources"));
    if any_summary {
        let text = match snap.resources.as_deref() {
            None => "meerdere",
            Some(s) if s.trim().is_empty() => "geen",
            Some(s) => s,
        };
        ui.label(text).on_hover_text("A group's resources are the resources of its tasks");
    } else {
        resources(ui, doc, st, &ids, snap.resources.as_deref());
    }
    label(ui, st.t("prop.status"));
    let status_label =
        snap.status.map(crate::lang::status_name).unwrap_or(if multi { st.t("common.multiple") } else { st.t("status.not_started") });
    egui::ComboBox::from_id_salt(("task_status", first, multi)).selected_text(status_label).show_ui(ui, |ui| {
        for status in TaskStatus::ALL {
            if ui.selectable_label(snap.status == Some(status), crate::lang::status_name(status)).clicked() && snap.status != Some(status) {
                let ids = ids.clone();
                edit(doc, st, "Status", move |p| {
                    for id in &ids {
                        p.set_task_status(*id, status).map_err(|e| e.to_string())?;
                    }
                    Ok(())
                });
            }
        }
    });
    label(ui, st.t("prop.percent"));
    if let Some(text) = line(ui, ("task_pc", first, multi), snap.percent.as_deref().unwrap_or(""), "0") {
        let ids = ids.clone();
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
    label(ui, st.t("prop.mark"));
    let mark_label = snap.mark.map(|m| m.map(crate::lang::mark_name).unwrap_or(st.t("common.none"))).unwrap_or(st.t("common.multiple"));
    egui::ComboBox::from_id_salt(("task_mark", first, multi)).selected_text(mark_label).show_ui(ui, |ui| {
        let choices = [None].into_iter().chain(RowMark::ALL.into_iter().map(Some));
        for choice in choices {
            let text = choice.map(crate::lang::mark_name).unwrap_or(st.t("common.none"));
            if ui.selectable_label(snap.mark == Some(choice), text).clicked() && snap.mark != Some(choice) {
                let ids = ids.clone();
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
    label(ui, st.t("prop.notes"));
    if let Some(text) = notes(ui, ("task_notes", first, multi), snap.notes.as_deref().unwrap_or("")) {
        let ids = ids.clone();
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

#[derive(Clone, Copy)]
enum Bound {
    StartEarliest,
    StartLatest,
    FinishEarliest,
    FinishLatest,
}

impl Bound {
    fn label(self) -> &'static str {
        crate::lang::text(match self {
            Bound::StartEarliest => "prop.start_earliest",
            Bound::StartLatest => "prop.start_latest",
            Bound::FinishEarliest => "prop.finish_earliest",
            Bound::FinishLatest => "prop.finish_latest",
        })
    }
}

struct Snap {
    name: Option<String>,
    milestone: Option<bool>,
    duration: Option<String>,
    start: Option<Option<DateTime>>,
    finish: Option<Option<DateTime>>,
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
        start_earliest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.bounds.start_earliest)),
        start_latest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.bounds.start_latest)),
        finish_earliest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.bounds.finish_earliest)),
        finish_latest: common_dt(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.bounds.finish_latest)),
        start_fixed: flag(&|t| t.bounds.start_latest_fixed),
        finish_fixed: flag(&|t| t.bounds.finish_latest_fixed),
        stretch: flag(&|t| t.fills_group),
        percent_group: text(&|t| t.duration_percent.map(|n| n.to_string()).unwrap_or_default()),
        resources: text(&|t| p.format_resource_names(t.id)),
        percent: text(&|t| t.percent_complete.to_string()),
        status: common_status(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.status)),
        mark: common_mark(ids.iter().filter_map(|id| p.task(*id)).map(|t| t.row_mark)),
        notes: text(&|t| t.notes.clone()),
    }
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

fn set_picked_date(p: &mut Project, ids: &[TaskId], date: DateTime, finish: bool) -> Result<(), String> {
    for id in ids {
        let Some(idx) = p.task_index(*id) else { continue };
        let summary = p.has_children(idx);
        let auto = p.task(*id).is_some_and(|t| t.mode == ScheduleMode::Auto);
        if auto && !summary {
            p.set_schedule_mode(&[*id], ScheduleMode::Manual).map_err(|e| e.to_string())?;
        }
        p.set_manual_date(*id, date, finish).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn show_date(ui: &mut Ui, salt: &str, value: Option<Option<DateTime>>, clearable: bool, minute: u32) -> Option<Option<DateTime>> {
    let (current, empty) = match value {
        None => (None, "meerdere"),
        Some(None) => (None, "geen"),
        Some(Some(dt)) => (Some(dt), "geen"),
    };
    crate::widgets::date_picker::button(
        ui,
        crate::widgets::date_picker::DateField { salt, current, clearable, enabled: true, minute, empty },
    )
}

fn date_bound(
    ui: &mut Ui,
    doc: &mut Document,
    st: &mut UiState,
    ids: &[TaskId],
    first: TaskId,
    current: Option<Option<DateTime>>,
    bound: Bound,
    minute: u32,
) {
    label(ui, bound.label());
    let Some(next) = show_date(ui, &format!("{}{first}", bound.label()), current, true, minute) else { return };
    let ids = ids.to_vec();
    let title = bound.label();
    edit(doc, st, title, move |p| {
        for id in &ids {
            if let Some(t) = p.task_mut(*id) {
                match bound {
                    Bound::StartEarliest => t.bounds.start_earliest = next,
                    Bound::StartLatest => t.bounds.start_latest = next,
                    Bound::FinishEarliest => t.bounds.finish_earliest = next,
                    Bound::FinishLatest => t.bounds.finish_latest = next,
                }
            }
        }
        Ok(())
    });
}

fn fixed_box(ui: &mut Ui, doc: &mut Document, st: &mut UiState, ids: &[TaskId], title: &str, current: Option<bool>, start: bool) {
    let mut on = current.unwrap_or(false);
    if ui.checkbox(&mut on, title).changed() {
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
        ui.label(RichText::new("Differs between the selection. Ticking sets them all.").size(11.0).color(theme::pal().TEXT_WEAK));
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
                if ui.button("×").on_hover_text("Remove this predecessor").clicked() {
                    edit(doc, st, "Remove predecessor", move |p| {
                        p.unlink(pred, first).map_err(|e| e.to_string())?;
                        Ok(())
                    });
                }
            });
        }
    } else {
        ui.label(RichText::new("Adds a finish-to-start link on each selected activity.").size(11.0).color(theme::pal().TEXT_WEAK));
    }
    let choices: Vec<(TaskId, String)> = doc
        .project
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| ids.iter().all(|id| can_link(&doc.project, *id, t.id) && (multi || !already(&doc.project, *id, t.id))))
        .map(|(i, t)| (t.id, format!("{}  {}", i.saturating_add(1), t.name)))
        .collect();
    egui::ComboBox::from_id_salt(("pred_add", first)).selected_text("Add predecessor").show_ui(ui, |ui| {
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
        None => "meerdere",
        Some(text) if text.trim().is_empty() => "geen",
        Some(text) => text,
    };
    let listed: Vec<(ResourceId, String)> = doc.project.resources.iter().map(|r| (r.id, r.name.clone())).collect();
    let button = ui.button(label).on_hover_text("Assigned resources");
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
        let resp = ui.add(egui::TextEdit::singleline(&mut typed).hint_text("New resource"));
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
    label(ui, "Group color");
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
        if ui.button("Clear").clicked() {
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
