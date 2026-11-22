//! Resource mutation commands (`resource.*`).

use plancraft_core::dates::FormatExt;
use plancraft_core::ids::AssignmentId;
use plancraft_core::leveling::{LevelingOptions, clear_leveling, level_resources};
use serde_json::{Value, json};

use super::params::{
    opt_assignment_id, opt_bool, opt_f64, opt_str, opt_u64, req_resource_id, req_task_id, resource_ids,
};
use crate::session::{Session, ok_empty};

pub fn run_insert(s: &mut Session, p: &Value) -> Result<Value, String> {
    let name = match opt_str(p, "name")? {
        Some(n) => n.to_string(),
        None => {
            let n = s.active_doc().map(|d| d.project.resources.len() + 1).unwrap_or(1);
            format!("Resource {n}")
        }
    };
    if let Some(doc) = s.active_doc()
        && doc.project.resources.iter().any(|r| r.name.eq_ignore_ascii_case(&name))
    {
        return Err(format!("a resource named '{name}' already exists"));
    }
    let id = s.edit_active("Add Resource", |proj| proj.add_resource(name))?;
    Ok(json!({ "resource": id.0 }))
}

pub fn run_delete(s: &mut Session, p: &Value) -> Result<Value, String> {
    let ids = resource_ids(p)?;
    if ids.is_empty() {
        return Err("'resources' must not be empty".into());
    }
    s.try_edit_active("Delete Resource", |proj| proj.delete_resources(&ids))?;
    Ok(json!({ "deleted": ids.iter().map(|id| id.0).collect::<Vec<_>>() }))
}

pub fn run_level_all(s: &mut Session, p: &Value) -> Result<Value, String> {
    let mut opts = LevelingOptions::default();
    if let Some(b) = opt_bool(p, "clear_existing")? {
        opts.clear_existing = b;
    }
    if let Some(d) = opt_u64(p, "max_delay_days")? {
        opts.max_delay_days = u32::try_from(d.min(5000)).unwrap_or(5000);
    }
    if p.get("resources").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
        opts.resources = resource_ids(p)?;
    }
    let report = s.edit_active("Level Resources", |proj| level_resources(proj, &opts))?;
    let fmt = |d: Option<plancraft_core::dates::prelude::DateTime>| {
        d.map(|d| d.format("%Y-%m-%d %H:%M").to_string())
    };
    Ok(json!({
        "delayed": report.delayed.iter().map(|(id, mins)| json!({ "task": id.0, "delay_minutes": mins })).collect::<Vec<_>>(),
        "unresolved": report.unresolved,
        "delayed_count": report.delayed.len(),
        "finish_before": fmt(report.finish_before),
        "finish_after": fmt(report.finish_after),
    }))
}

pub fn run_clear_leveling(s: &mut Session, _p: &Value) -> Result<Value, String> {
    s.edit_active("Clear Leveling", clear_leveling)?;
    Ok(ok_empty())
}

pub fn run_assign(s: &mut Session, p: &Value) -> Result<Value, String> {
    let task = req_task_id(p, "task")?;
    let resource = req_resource_id(p, "resource")?;
    let units = opt_f64(p, "units")?;
    let work = opt_u64(p, "work_minutes")?.and_then(|n| i64::try_from(n).ok());
    let id = s.try_edit_active("Assign Resource", |proj| proj.assign(task, resource, units, work))?;
    Ok(json!({ "assignment": id.0, "task": task.0, "resource": resource.0 }))
}

pub fn run_unassign(s: &mut Session, p: &Value) -> Result<Value, String> {
    let id = if let Some(id) = opt_assignment_id(p, "assignment")? {
        id
    } else {
        let task = req_task_id(p, "task")?;
        let resource = req_resource_id(p, "resource")?;
        let doc = s.active_doc().ok_or_else(|| "no open document".to_string())?;
        doc.project
            .assignments
            .iter()
            .find(|a| a.task == task && a.resource == resource)
            .map(|a| a.id)
            .ok_or_else(|| format!("resource {} is not assigned to task {}", resource.0, task.0))?
    };
    let task = s.try_edit_active("Remove Assignment", |proj| {
        let task = proj.assignment(id).map(|a| a.task).ok_or(plancraft_core::ops::PlanError::AssignmentNotFound(id))?;
        proj.unassign(id)?;
        Ok(task)
    })?;
    Ok(json!({ "removed": true, "assignment": id.0, "task": task.0 }))
}

/// Resolve assignment id from result (for callers / tests).
#[allow(dead_code)]
pub fn assignment_id_result(v: &Value) -> Option<AssignmentId> {
    v.get("assignment").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).map(AssignmentId)
}
