//! Task mutation commands (`task.*`).

use plancraft_core::ids::TaskId;
use plancraft_core::model::{LinkType, ScheduleMode, TaskRef};
use serde_json::{Value, json};

use super::params::{lag_param, link_type, opt_str, opt_usize, req_task_id, req_u8, schedule_mode, task_ids, top_level_tasks};
use crate::session::{Session, ok_empty};

pub fn run_insert(s: &mut Session, p: &Value) -> Result<Value, String> {
    let name = opt_str(p, "name")?.unwrap_or("<New Task>").to_string();
    let at = opt_usize(p, "at")?;
    let id = s.edit_active("Insert Task", |proj| proj.add_task(name, at))?;
    Ok(json!({ "task": id.0 }))
}

pub fn run_insert_milestone(s: &mut Session, p: &Value) -> Result<Value, String> {
    let name = opt_str(p, "name")?.unwrap_or("<New Milestone>").to_string();
    let at = opt_usize(p, "at")?;
    let id = s.edit_active("Insert Milestone", |proj| proj.add_milestone(name, at))?;
    Ok(json!({ "task": id.0 }))
}

pub fn run_insert_summary(s: &mut Session, p: &Value) -> Result<Value, String> {
    let name = opt_str(p, "name")?.unwrap_or("<New Summary Task>").to_string();
    let ids = match p.get("tasks") {
        Some(Value::Array(a)) if !a.is_empty() => task_ids(p)?,
        _ => Vec::new(),
    };
    let id = if ids.is_empty() {
        s.edit_active("Insert Summary Task", |proj| proj.add_task(name, None))?
    } else {
        s.try_edit_active("Insert Summary Task", |proj| proj.add_summary_over(name, &ids))?
    };
    Ok(json!({ "task": id.0 }))
}

pub fn run_delete(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.is_empty() {
        return Err("'tasks' must not be empty".into());
    }
    let n = ids.len();
    let label = if n == 1 { "Delete Task" } else { "Delete Tasks" };
    let deleted = s.try_edit_active(label, |proj| proj.delete_tasks(&ids))?;
    Ok(json!({ "deleted": deleted.iter().map(|id| id.0).collect::<Vec<_>>() }))
}

pub fn run_indent(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    let tops = {
        let doc = s.active_doc().ok_or_else(|| "no open document".to_string())?;
        top_level_tasks(&doc.project, &ids)
    };
    s.try_edit_active("Indent", |proj| {
        for id in &tops {
            proj.indent(*id)?;
        }
        Ok(())
    })?;
    Ok(json!({ "tasks": tops.iter().map(|id| id.0).collect::<Vec<_>>() }))
}

pub fn run_outdent(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    let mut tops = {
        let doc = s.active_doc().ok_or_else(|| "no open document".to_string())?;
        top_level_tasks(&doc.project, &ids)
    };
    // Bottom-up so earlier promotions do not capture later siblings.
    tops.reverse();
    s.try_edit_active("Outdent", |proj| {
        for id in &tops {
            proj.outdent(*id)?;
        }
        Ok(())
    })?;
    tops.reverse();
    Ok(json!({ "tasks": tops.iter().map(|id| id.0).collect::<Vec<_>>() }))
}

pub fn run_move_up(s: &mut Session, p: &Value) -> Result<Value, String> {
    shift(s, p, true)
}

pub fn run_move_down(s: &mut Session, p: &Value) -> Result<Value, String> {
    shift(s, p, false)
}

fn shift(s: &mut Session, p: &Value, up: bool) -> Result<Value, String> {
    let ids = task_ids(p)?;
    let mut tops = {
        let doc = s.active_doc().ok_or_else(|| "no open document".to_string())?;
        top_level_tasks(&doc.project, &ids)
    };
    if !up {
        tops.reverse();
    }
    if tops.is_empty() {
        return Ok(ok_empty());
    }
    s.try_edit_active(if up { "Move Up" } else { "Move Down" }, |proj| {
        for id in &tops {
            proj.shift_task(*id, up)?;
        }
        Ok(())
    })?;
    Ok(json!({ "tasks": tops.iter().map(|id| id.0).collect::<Vec<_>>() }))
}

pub fn run_link(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.len() < 2 {
        return Err("select two or more tasks to link them in order".into());
    }
    let kind = link_type(p)?;
    let lag = {
        let doc = s.active_doc().ok_or_else(|| "no open document".to_string())?;
        lag_param(&doc.project, p)?
    };
    if kind == LinkType::FS && lag.is_zero() {
        s.try_edit_active("Link Tasks", |proj| proj.link_chain(&ids))?;
    } else {
        s.try_edit_active("Link Tasks", |proj| {
            for pair in ids.windows(2) {
                if let [pred, succ] = pair {
                    proj.link(TaskRef::Local(*pred), *succ, kind, lag)?;
                }
            }
            Ok(())
        })?;
    }
    Ok(json!({
        "links": ids.len() - 1,
        "tasks": ids.iter().map(|id| id.0).collect::<Vec<_>>(),
        "type": kind.code(),
    }))
}

/// One predecessor → successor edge (typed, with optional lag).
pub fn run_link_pair(s: &mut Session, p: &Value) -> Result<Value, String> {
    let pred = req_task_id(p, "predecessor")?;
    let succ = req_task_id(p, "successor")?;
    let kind = link_type(p)?;
    let lag = {
        let doc = s.active_doc().ok_or_else(|| "no open document".to_string())?;
        lag_param(&doc.project, p)?
    };
    s.try_edit_active("Link Tasks", |proj| proj.link(TaskRef::Local(pred), succ, kind, lag))?;
    Ok(json!({
        "linked": true,
        "predecessor": pred.0,
        "successor": succ.0,
        "type": kind.code(),
    }))
}

pub fn run_unlink(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.is_empty() {
        return Err("'tasks' must not be empty".into());
    }
    s.edit_active("Unlink Tasks", |proj| {
        proj.unlink_tasks(&ids);
    })?;
    Ok(ok_empty())
}

/// Remove one predecessor → successor edge.
pub fn run_unlink_pair(s: &mut Session, p: &Value) -> Result<Value, String> {
    let pred = req_task_id(p, "predecessor")?;
    let succ = req_task_id(p, "successor")?;
    let removed = s.try_edit_active("Unlink Tasks", |proj| {
        let ok = proj.unlink(TaskRef::Local(pred), succ)?;
        if !ok {
            return Err(plancraft_core::ops::PlanError::Invalid("these tasks are not linked".into()));
        }
        Ok(1usize)
    })?;
    Ok(json!({ "removed_links": removed }))
}

pub fn run_set_percent(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.is_empty() {
        return Err("'tasks' must not be empty".into());
    }
    let pct = req_u8(p, "percent")?;
    let label = format!("{pct}% Complete");
    s.try_edit_active(&label, |proj| {
        for id in &ids {
            proj.set_percent_complete(*id, pct)?;
        }
        Ok(())
    })?;
    Ok(json!({ "percent": pct, "tasks": ids.iter().map(|id| id.0).collect::<Vec<_>>() }))
}

pub fn run_set_mode(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.is_empty() {
        return Err("'tasks' must not be empty".into());
    }
    let mode = schedule_mode(p)?;
    let label = if mode == ScheduleMode::Manual { "Manually Schedule" } else { "Auto Schedule" };
    s.try_edit_active(label, |proj| proj.set_schedule_mode(&ids, mode))?;
    Ok(json!({ "mode": if mode == ScheduleMode::Manual { "manual" } else { "auto" } }))
}

pub fn run_toggle_active(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.is_empty() {
        return Err("'tasks' must not be empty".into());
    }
    let active = s.edit_active("Inactivate Task", |proj| {
        let new = !ids.first().and_then(|id| proj.task(*id)).map(|t| t.active).unwrap_or(true);
        for id in &ids {
            if let Some(t) = proj.task_mut(*id) {
                t.active = new;
            }
        }
        new
    })?;
    Ok(json!({ "active": active }))
}

pub fn run_clear_dates(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.is_empty() {
        return Err("'tasks' must not be empty".into());
    }
    s.try_edit_active("Clear Dates", |proj| proj.clear_dates(&ids))?;
    Ok(ok_empty())
}

pub fn run_mark_on_track(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    let report = s.try_edit_active("Mark on Track", |proj| proj.mark_on_track(&ids))?;
    Ok(json!({
        "updated": report.updated.iter().map(|id| id.0).collect::<Vec<_>>(),
        "split": report.split.iter().map(|id| id.0).collect::<Vec<_>>(),
        "constrained": report.constrained.iter().map(|id| id.0).collect::<Vec<_>>(),
        "skipped": report.skipped.iter().map(|(id, why)| json!({ "id": id.0, "reason": why })).collect::<Vec<_>>(),
    }))
}

pub fn run_toggle_timeline(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    if ids.is_empty() {
        return Err("'tasks' must not be empty".into());
    }
    let show = s.edit_active("Add to Timeline", |proj| {
        let new = !ids.first().and_then(|id| proj.task(*id)).map(|t| t.show_on_timeline).unwrap_or(false);
        for id in &ids {
            if let Some(t) = proj.task_mut(*id) {
                t.show_on_timeline = new;
            }
        }
        new
    })?;
    Ok(json!({ "show_on_timeline": show }))
}

pub fn run_rejoin(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = task_ids(p)?;
    let n = s.edit_active("Rejoin Split Task", |proj| {
        let mut n = 0usize;
        for id in &ids {
            n += proj.remove_splits(*id).unwrap_or(0);
        }
        n
    })?;
    Ok(json!({ "removed_splits": n }))
}

/// Resolve a single task id from an insert result.
#[allow(dead_code)]
pub fn task_id_result(v: &Value) -> Option<TaskId> {
    v.get("task").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).map(TaskId)
}
