//! Thin adapter: UI [`Command`] → `plancraft_engine::Session::execute`.
//!
//! Document-mutating menu/toolbar actions go through the shared engine registry.
//! Dialog *open/close* and chrome stay local; dialog *apply* and paste go through
//! [`edit_doc`] (Session-attributed undo) when the caller holds the workspace.

use plancraft_core::ids::{ResourceId, TaskId};
use plancraft_core::model::{ChartStyle, Project, ScheduleMode};
use plancraft_core::workspace::{ChangeSource, Workspace};
use plancraft_engine::{CommandError, Session};
use serde_json::{Value, json};

use crate::commands::Command;

/// Run an engine command against the locked workspace (User attribution).
pub fn execute(ws: &mut Workspace, id: &str, params: &Value) -> Result<Value, CommandError> {
    Session::execute_mut(ws, ChangeSource::User, id, params)
}

/// Undoable labeled edit on a document (dialog OK / paste), via [`Session`].
pub fn edit_doc<R>(
    ws: &mut Workspace,
    handle: u64,
    label: &str,
    f: impl FnOnce(&mut Project) -> Result<R, String>,
) -> Result<R, String> {
    Session::edit_doc_mut(ws, ChangeSource::User, handle, label, f)
}

/// Undoable labeled edit on the active document.
pub fn edit_active<R>(
    ws: &mut Workspace,
    label: &str,
    f: impl FnOnce(&mut Project) -> Result<R, String>,
) -> Result<R, String> {
    let handle = ws
        .active_doc()
        .map(|d| d.handle)
        .ok_or_else(|| "no open document".to_string())?;
    edit_doc(ws, handle, label, f)
}

fn task_params(ids: &[TaskId]) -> Value {
    json!({ "tasks": ids.iter().map(|id| id.0).collect::<Vec<_>>() })
}

fn resource_params(ids: &[ResourceId]) -> Value {
    json!({ "resources": ids.iter().map(|id| id.0).collect::<Vec<_>>() })
}

fn mode_str(mode: ScheduleMode) -> &'static str {
    match mode {
        ScheduleMode::Auto => "auto",
        ScheduleMode::Manual => "manual",
    }
}

fn chart_style_str(style: ChartStyle) -> &'static str {
    match style {
        ChartStyle::Blue => "blue",
        ChartStyle::Graphite => "graphite",
        ChartStyle::Green => "green",
        ChartStyle::Orange => "orange",
        ChartStyle::HighContrast => "high_contrast",
    }
}

/// Map a document-mutating UI command onto an engine id + JSON params.
///
/// Returns `None` for UI-only commands (dialogs, view chrome) or when required
/// selection context is missing (caller should no-op or toast).
pub fn map_doc_command(
    cmd: &Command,
    task_ids: &[TaskId],
    resource_ids: &[ResourceId],
    // Outline index for Insert Task/Milestone (beneath the selection).
    insert_at: Option<usize>,
) -> Option<(&'static str, Value)> {
    match cmd {
        Command::Undo => Some(("edit.undo", json!({}))),
        Command::Redo => Some(("edit.redo", json!({}))),
        Command::InsertTask => Some((
            "task.insert",
            json!({ "name": "<New Task>", "at": insert_at }),
        )),
        Command::InsertMilestone => Some((
            "task.insertMilestone",
            json!({ "name": "<New Milestone>", "at": insert_at }),
        )),
        Command::InsertSummary => {
            if task_ids.is_empty() {
                Some(("task.insertSummary", json!({ "name": "<New Summary Task>" })))
            } else {
                Some((
                    "task.insertSummary",
                    json!({ "name": "<New Summary Task>", "tasks": task_ids.iter().map(|id| id.0).collect::<Vec<_>>() }),
                ))
            }
        }
        Command::DeleteSelection if !task_ids.is_empty() => Some(("task.delete", task_params(task_ids))),
        Command::Indent if !task_ids.is_empty() => Some(("task.indent", task_params(task_ids))),
        Command::Outdent if !task_ids.is_empty() => Some(("task.outdent", task_params(task_ids))),
        Command::MoveUp if !task_ids.is_empty() => Some(("task.moveUp", task_params(task_ids))),
        Command::MoveDown if !task_ids.is_empty() => Some(("task.moveDown", task_params(task_ids))),
        Command::LinkSelected if task_ids.len() >= 2 => Some(("task.link", task_params(task_ids))),
        Command::UnlinkSelected if !task_ids.is_empty() => Some(("task.unlink", task_params(task_ids))),
        Command::SetPercent(pct) if !task_ids.is_empty() => {
            Some(("task.setPercent", json!({ "tasks": task_ids.iter().map(|id| id.0).collect::<Vec<_>>(), "percent": pct })))
        }
        Command::MarkOnTrack => Some(("task.markOnTrack", task_params(task_ids))),
        Command::ToggleActive if !task_ids.is_empty() => Some(("task.toggleActive", task_params(task_ids))),
        Command::SetMode(mode) if !task_ids.is_empty() => Some((
            "task.setMode",
            json!({ "tasks": task_ids.iter().map(|id| id.0).collect::<Vec<_>>(), "mode": mode_str(*mode) }),
        )),
        Command::ClearDates if !task_ids.is_empty() => Some(("task.clearDates", task_params(task_ids))),
        Command::ToggleTimeline if !task_ids.is_empty() => Some(("task.toggleTimeline", task_params(task_ids))),
        Command::RejoinTasks if !task_ids.is_empty() => Some(("task.rejoinSplits", task_params(task_ids))),
        Command::InsertResource => Some(("resource.insert", json!({}))),
        Command::DeleteResources if !resource_ids.is_empty() => Some(("resource.delete", resource_params(resource_ids))),
        Command::LevelAll => Some(("resource.levelAll", json!({}))),
        Command::ClearLeveling => Some(("resource.clearLeveling", json!({}))),
        Command::UpdateProgressFromTime => Some(("project.updateProgressFromTime", json!({}))),
        Command::SetChartStyle(style) => Some(("project.setChartStyle", json!({ "style": chart_style_str(*style) }))),
        _ => None,
    }
}

/// Parse a new task id from an insert-style engine result.
pub fn result_task_id(v: &Value) -> Option<TaskId> {
    v.get("task").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).map(TaskId)
}

/// Parse a new resource id from `resource.insert`.
pub fn result_resource_id(v: &Value) -> Option<ResourceId> {
    v.get("resource").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).map(ResourceId)
}
