//! Graph v1.0 basic Planner board push (no dependencies).

use std::collections::HashMap;

use serde_json::{Value, json};

use plancraft_core::prelude::ResourceId;

use super::auth::{self, AuthConfig, TokenSet};
use super::http::Transport;
use super::map::{ExportPlan, ExportTask, first_order_hint, utc_datetime};

#[derive(Clone, Debug)]
pub struct GraphExportResult {
    pub plan_id: String,
    pub plan_url: String,
    pub tasks_created: usize,
    pub warnings: Vec<String>,
}

pub struct GraphOptions {
    /// M365 group that owns the new plan. Required.
    pub group_id: String,
}

pub fn list_member_groups(http: &mut dyn Transport, tokens: &TokenSet) -> Result<Vec<(String, String)>, String> {
    let value = auth::graph_get(http, tokens, "/v1.0/me/memberOf/microsoft.graph.group?$select=id,displayName")?;
    let mut out = Vec::new();
    if let Some(list) = value.get("value").and_then(|v| v.as_array()) {
        for item in list {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = item.get("displayName").and_then(|v| v.as_str()).unwrap_or("");
            if !id.is_empty() {
                out.push((id.to_string(), name.to_string()));
            }
        }
    }
    Ok(out)
}

pub fn export_basic(
    http: &mut dyn Transport,
    cfg: &AuthConfig,
    plan: &ExportPlan,
    options: &GraphOptions,
) -> Result<GraphExportResult, String> {
    let tokens = auth::graph_token(http, cfg)?;
    export_basic_with_tokens(http, &tokens, plan, options)
}

pub fn export_basic_with_tokens(
    http: &mut dyn Transport,
    tokens: &TokenSet,
    plan: &ExportPlan,
    options: &GraphOptions,
) -> Result<GraphExportResult, String> {
    let group_id = options.group_id.trim();
    if group_id.is_empty() {
        return Err(crate::lang::text("planner.need_group").into());
    }

    let mut warnings = plan.notes.clone();
    let created = auth::graph_post(http, tokens, "/v1.0/planner/plans", &json!({ "owner": group_id, "title": plan.title }))?;
    let plan_id = created.get("id").and_then(|v| v.as_str()).ok_or_else(|| "plan create returned no id".to_string())?.to_string();

    let mut bucket_ids = HashMap::new();
    for bucket in &plan.buckets {
        let body = json!({
            "name": bucket.name,
            "planId": plan_id,
            "orderHint": first_order_hint()
        });
        let resp = auth::graph_post(http, tokens, "/v1.0/planner/buckets", &body)?;
        if let Some(id) = resp.get("id").and_then(|v| v.as_str()) {
            bucket_ids.insert(bucket.key.clone(), id.to_string());
        }
    }
    let default_bucket = bucket_ids.values().next().cloned().ok_or_else(|| "no bucket was created".to_string())?;

    let entra = resolve_assignees(http, tokens, plan)?;

    let mut tasks_created = 0usize;
    for task in &plan.tasks {
        let bucket_id = bucket_ids.get(&task.bucket_key).cloned().unwrap_or_else(|| default_bucket.clone());
        match create_task(http, tokens, &plan_id, &bucket_id, task, &entra) {
            Ok(()) => tasks_created = tasks_created.saturating_add(1),
            Err(err) => warnings.push(format!("{}: {err}", task.title)),
        }
    }

    Ok(GraphExportResult {
        plan_id: plan_id.clone(),
        plan_url: format!("https://planner.cloud.microsoft/web/plan/{plan_id}"),
        tasks_created,
        warnings,
    })
}

fn create_task(
    http: &mut dyn Transport,
    tokens: &TokenSet,
    plan_id: &str,
    bucket_id: &str,
    task: &ExportTask,
    entra: &HashMap<ResourceId, String>,
) -> Result<(), String> {
    let mut assignments = serde_json::Map::new();
    for rid in &task.assignees {
        if let Some(user_id) = entra.get(rid) {
            assignments.insert(
                user_id.clone(),
                json!({
                    "@odata.type": "#microsoft.graph.plannerAssignment",
                    "orderHint": first_order_hint()
                }),
            );
        }
    }
    let mut body = json!({
        "planId": plan_id,
        "bucketId": bucket_id,
        "title": task.title,
        "percentComplete": task.percent_complete,
        "priority": 5
    });
    if let Some(obj) = body.as_object_mut() {
        if let Some(start) = task.start {
            obj.insert("startDateTime".into(), json!(utc_datetime(start)));
        }
        if let Some(finish) = task.finish {
            obj.insert("dueDateTime".into(), json!(utc_datetime(finish)));
        }
        if !assignments.is_empty() {
            obj.insert("assignments".into(), Value::Object(assignments));
        }
    }
    let created = auth::graph_post(http, tokens, "/v1.0/planner/tasks", &body)?;
    let task_id = created.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if !task.notes.trim().is_empty() && !task_id.is_empty() {
        let details = auth::graph_get(http, tokens, &format!("/v1.0/planner/tasks/{task_id}/details"))?;
        let etag = details.get("@odata.etag").and_then(|v| v.as_str()).unwrap_or("*");
        let _ =
            auth::graph_patch(http, tokens, &format!("/v1.0/planner/tasks/{task_id}/details"), etag, &json!({ "description": task.notes }));
    }
    Ok(())
}

fn resolve_assignees(http: &mut dyn Transport, tokens: &TokenSet, plan: &ExportPlan) -> Result<HashMap<ResourceId, String>, String> {
    let mut map = HashMap::new();
    for person in &plan.people {
        let email = person.email.trim();
        if email.is_empty() {
            continue;
        }
        let filter = format!("mail eq '{}' or userPrincipalName eq '{}'", escape_odata(email), escape_odata(email));
        let path = format!("/v1.0/users?$select=id,mail,userPrincipalName&$filter={filter}");
        match auth::graph_get(http, tokens, &path) {
            Ok(value) => {
                if let Some(id) =
                    value.get("value").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|u| u.get("id")).and_then(|v| v.as_str())
                {
                    map.insert(person.resource_id, id.to_string());
                }
            }
            Err(_) => {
                // Fall back to exact UPN path when $filter is refused.
                let encoded = email; // Graph path segment; emails are typically ASCII.
                if let Ok(user) = auth::graph_get(http, tokens, &format!("/v1.0/users/{encoded}?$select=id"))
                    && let Some(id) = user.get("id").and_then(|v| v.as_str())
                {
                    map.insert(person.resource_id, id.to_string());
                }
            }
        }
    }
    Ok(map)
}

fn escape_odata(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_export::http::HttpResponse;
    use crate::planner_export::http::mock::Scripted;
    use crate::planner_export::map::{ExportAssignee, ExportBucket, ExportTarget, ExportTask};
    use plancraft_core::ids::{ResourceId, TaskId};
    use uuid::Uuid;

    #[test]
    fn escape_odata_doubles_quotes() {
        assert_eq!(escape_odata("o'brien@x.com"), "o''brien@x.com");
    }

    #[test]
    fn export_basic_creates_plan_bucket_and_task() {
        let mut http = Scripted::new(vec![
            Ok(HttpResponse { status: 201, body: r#"{"id":"plan1"}"#.into() }),
            Ok(HttpResponse { status: 201, body: r#"{"id":"bucket1"}"#.into() }),
            Ok(HttpResponse { status: 201, body: r#"{"id":"task1"}"#.into() }),
            Ok(HttpResponse { status: 200, body: r#"{"@odata.etag":"W/\"1\"","description":""}"#.into() }),
            Ok(HttpResponse { status: 204, body: String::new() }),
        ]);
        let plan = ExportPlan {
            title: "Demo".into(),
            target: ExportTarget::BasicBoard,
            buckets: vec![ExportBucket { key: "status:Not started".into(), name: "Not started".into(), order: 0 }],
            tasks: vec![ExportTask {
                task_id: TaskId(1),
                guid: Uuid::nil(),
                title: "Design".into(),
                bucket_key: "status:Not started".into(),
                outline_level: 1,
                parent_task_id: None,
                start: None,
                finish: None,
                percent_complete: 0,
                effort_hours: 8.0,
                duration_days: 1.0,
                milestone: false,
                summary: false,
                notes: "hello".into(),
                assignees: vec![],
            }],
            dependencies: vec![],
            people: vec![ExportAssignee { resource_id: ResourceId(1), name: "Ana".into(), email: String::new() }],
            notes: vec![],
        };
        let tokens = TokenSet { access_token: "atok".into(), refresh_token: None, expires_in: 3600 };
        let result = export_basic_with_tokens(&mut http, &tokens, &plan, &GraphOptions { group_id: "group1".into() }).expect("export");
        assert_eq!(result.plan_id, "plan1");
        assert_eq!(result.tasks_created, 1);
    }
}
