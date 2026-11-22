//! Shared JSON param helpers for engine commands.

use plancraft_core::duration::Lag;
use plancraft_core::ids::{AssignmentId, ResourceId, TaskId};
use plancraft_core::model::{LinkType, Project, ScheduleMode};
use serde_json::Value;

pub fn opt_str<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("param '{key}' must be a string")),
    }
}

pub fn opt_u64(p: &Value, key: &str) -> Result<Option<u64>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_i64().and_then(|i| u64::try_from(i).ok()))
            .map(Some)
            .ok_or_else(|| format!("param '{key}' must be a number")),
    }
}

pub fn opt_usize(p: &Value, key: &str) -> Result<Option<usize>, String> {
    Ok(opt_u64(p, key)?.and_then(|n| usize::try_from(n).ok()))
}

pub fn opt_f64(p: &Value, key: &str) -> Result<Option<f64>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .or_else(|| v.as_i64().map(|i| i as f64))
            .map(Some)
            .ok_or_else(|| format!("param '{key}' must be a number")),
    }
}

pub fn opt_bool(p: &Value, key: &str) -> Result<Option<bool>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("param '{key}' must be a boolean")),
    }
}

pub fn req_u8(p: &Value, key: &str) -> Result<u8, String> {
    let n = opt_u64(p, key)?.ok_or_else(|| format!("missing number param '{key}'"))?;
    u8::try_from(n).map_err(|_| format!("param '{key}' out of range"))
}

fn as_id(v: &Value, what: &str) -> Result<u32, String> {
    let n = v
        .as_u64()
        .or_else(|| v.as_i64().and_then(|i| u64::try_from(i).ok()))
        .ok_or_else(|| format!("{what} id must be a number"))?;
    u32::try_from(n).map_err(|_| format!("{what} id out of range"))
}

pub fn req_task_id(p: &Value, key: &str) -> Result<TaskId, String> {
    let v = p.get(key).ok_or_else(|| format!("missing number param '{key}'"))?;
    as_id(v, "task").map(TaskId)
}

pub fn req_resource_id(p: &Value, key: &str) -> Result<ResourceId, String> {
    let v = p.get(key).ok_or_else(|| format!("missing number param '{key}'"))?;
    as_id(v, "resource").map(ResourceId)
}

pub fn opt_assignment_id(p: &Value, key: &str) -> Result<Option<AssignmentId>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => as_id(v, "assignment").map(|n| Some(AssignmentId(n))),
    }
}

pub fn task_ids(p: &Value) -> Result<Vec<TaskId>, String> {
    let arr = p
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing array param 'tasks'".to_string())?;
    arr.iter().map(|v| as_id(v, "task").map(TaskId)).collect()
}

pub fn resource_ids(p: &Value) -> Result<Vec<ResourceId>, String> {
    let arr = p
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing array param 'resources'".to_string())?;
    arr.iter().map(|v| as_id(v, "resource").map(ResourceId)).collect()
}

pub fn schedule_mode(p: &Value) -> Result<ScheduleMode, String> {
    match opt_str(p, "mode")?.unwrap_or("auto") {
        "auto" => Ok(ScheduleMode::Auto),
        "manual" => Ok(ScheduleMode::Manual),
        other => Err(format!("unknown schedule mode '{other}' (use auto|manual)")),
    }
}

pub fn link_type(p: &Value) -> Result<LinkType, String> {
    let s = opt_str(p, "type")?.unwrap_or("FS");
    LinkType::parse(s).ok_or_else(|| format!("unknown link type '{s}' (FS|SS|FF|SF)"))
}

/// Lag as a Project-relative string (`2d`, `-1d`, `50%`) or working minutes number.
pub fn lag_param(project: &Project, p: &Value) -> Result<Lag, String> {
    match p.get("lag") {
        None | Some(Value::Null) => Ok(Lag::None),
        Some(Value::Number(n)) => {
            let mins = n
                .as_i64()
                .or_else(|| n.as_f64().map(|f| f.round() as i64))
                .ok_or_else(|| "param 'lag' must be a number or string".to_string())?;
            Ok(if mins == 0 { Lag::None } else { Lag::Working(mins) })
        }
        Some(Value::String(s)) => Lag::parse(s, &project.settings.time)
            .ok_or_else(|| format!("invalid lag '{s}'")),
        Some(_) => Err("param 'lag' must be a number or string".into()),
    }
}

/// Selected tasks whose ancestor is also selected follow the ancestor; order by row.
pub fn top_level_tasks(project: &Project, ids: &[TaskId]) -> Vec<TaskId> {
    let set: std::collections::HashSet<TaskId> = ids.iter().copied().collect();
    let mut out: Vec<TaskId> = ids
        .iter()
        .copied()
        .filter(|&id| {
            let mut cur = project.task_index(id).and_then(|i| project.parent_index(i));
            while let Some(c) = cur {
                if project.tasks.get(c).is_some_and(|t| set.contains(&t.id)) {
                    return false;
                }
                cur = project.parent_index(c);
            }
            true
        })
        .collect();
    out.sort_by_key(|id| project.task_index(*id));
    out
}
