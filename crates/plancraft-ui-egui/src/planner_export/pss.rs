//! Premium Plan 1+ push via Dataverse Project Scheduling Service (dependencies included).

use std::collections::HashMap;

use serde_json::{Value, json};
use uuid::Uuid;

use plancraft_core::prelude::{ResourceId, TaskId};

use super::auth::{self, AuthConfig, TokenSet};
use super::http::{Transport, bearer, expect_ok, json_body, parse_json};
use super::map::{ExportPlan, ExportTask, link_type_from_options, utc_datetime};

const MAX_OPS: usize = 200;

#[derive(Clone, Debug)]
pub struct PssExportResult {
    pub project_id: String,
    pub tasks_created: usize,
    pub dependencies_created: usize,
    pub assignments_created: usize,
    pub warnings: Vec<String>,
}

pub struct PssOptions {
    /// Dataverse org URL, e.g. `https://org.crm4.dynamics.com`.
    pub org_url: String,
}

pub fn export_premium(
    http: &mut dyn Transport,
    cfg: &AuthConfig,
    plan: &ExportPlan,
    options: &PssOptions,
) -> Result<PssExportResult, String> {
    let org = options.org_url.trim().trim_end_matches('/');
    if org.is_empty() {
        return Err(crate::lang::text("planner.need_org").into());
    }
    let tokens = auth::dataverse_token(http, cfg, org)?;
    export_premium_with_tokens(http, &tokens, plan, org)
}

pub fn export_premium_with_tokens(
    http: &mut dyn Transport,
    tokens: &TokenSet,
    plan: &ExportPlan,
    org: &str,
) -> Result<PssExportResult, String> {
    let mut warnings = plan.notes.clone();
    let api = format!("{}/api/data/v9.2", org.trim_end_matches('/'));

    let project_id = create_project(http, tokens, &api, &plan.title)?;
    let team = create_team_members(http, tokens, &api, &project_id, plan, &mut warnings)?;

    let link_options = read_link_type_options(http, tokens, &api).unwrap_or_default();
    let link_column = if link_options.column.is_empty() { "msdyn_projecttaskdependencylinktype".into() } else { link_options.column };

    let mut task_guids: HashMap<TaskId, Uuid> = HashMap::new();
    for task in &plan.tasks {
        task_guids.insert(task.task_id, task.guid);
    }

    let mut bucket_ids: HashMap<String, Uuid> = HashMap::new();
    let mut ops: Vec<Value> = Vec::new();

    for bucket in &plan.buckets {
        let id = Uuid::new_v4();
        bucket_ids.insert(bucket.key.clone(), id);
        ops.push(json!({
            "@odata.type": "Microsoft.Dynamics.CRM.msdyn_projectbucket",
            "msdyn_projectbucketid": id.to_string(),
            "msdyn_name": bucket.name,
            "msdyn_project@odata.bind": format!("/msdyn_projects({project_id})")
        }));
    }
    let default_bucket = bucket_ids.values().next().copied().unwrap_or_else(Uuid::new_v4);

    for task in &plan.tasks {
        ops.push(task_entity(task, &project_id, &bucket_ids, default_bucket, &task_guids));
    }

    let mut dependencies_created = 0usize;
    for dep in &plan.dependencies {
        let Some(pred) = task_guids.get(&dep.predecessor) else { continue };
        let Some(succ) = task_guids.get(&dep.successor) else { continue };
        let link_value = link_type_from_options(dep.kind, &link_options.options);
        let mut entity = json!({
            "@odata.type": "Microsoft.Dynamics.CRM.msdyn_projecttaskdependency",
            "msdyn_projecttaskdependencyid": Uuid::new_v4().to_string(),
            "msdyn_project@odata.bind": format!("/msdyn_projects({project_id})"),
            "msdyn_predecessortask@odata.bind": format!("/msdyn_projecttasks({pred})"),
            "msdyn_successortask@odata.bind": format!("/msdyn_projecttasks({succ})")
        });
        if let Some(obj) = entity.as_object_mut() {
            obj.insert(link_column.clone(), json!(link_value));
        }
        ops.push(entity);
        dependencies_created = dependencies_created.saturating_add(1);
    }

    // Tasks have to exist before an assignment can bind to them. A single set that mixes both
    // is rejected, and the people never land on the tasks.
    flush_operation_sets(http, tokens, &api, &project_id, &ops, &mut warnings)?;

    let mut assignments = Vec::new();
    for task in &plan.tasks {
        if task.summary {
            if !task.assignees.is_empty() {
                warnings.push(format!("{}: people are assigned on its tasks, not on the summary", task.title));
            }
            continue;
        }
        for rid in &task.assignees {
            let Some(member) = team.get(rid) else {
                warnings.push(format!("{}: no team member for an assigned resource", task.title));
                continue;
            };
            let Some(task_guid) = task_guids.get(&task.task_id) else { continue };
            assignments.push(json!({
                "@odata.type": "Microsoft.Dynamics.CRM.msdyn_resourceassignment",
                "msdyn_resourceassignmentid": Uuid::new_v4().to_string(),
                "msdyn_name": format!("{} / {}", task.title, member.name),
                "msdyn_projectid@odata.bind": format!("/msdyn_projects({project_id})"),
                "msdyn_projectteamid@odata.bind": format!("/msdyn_projectteams({})", member.team_id),
                "msdyn_taskid@odata.bind": format!("/msdyn_projecttasks({task_guid})")
            }));
        }
    }
    let assignments_created = assignments.len();
    if !assignments.is_empty() {
        flush_operation_sets(http, tokens, &api, &project_id, &assignments, &mut warnings)?;
    }

    Ok(PssExportResult { project_id, tasks_created: plan.tasks.len(), dependencies_created, assignments_created, warnings })
}

struct TeamMember {
    team_id: Uuid,
    name: String,
}

#[derive(Default)]
struct LinkTypeMeta {
    column: String,
    options: Vec<(i32, String)>,
}

/// The bookable resource for a user's email, when Dataverse has one.
fn bookable_resource(http: &mut dyn Transport, tokens: &TokenSet, api: &str, email: &str) -> Option<String> {
    let email = email.trim();
    if email.is_empty() {
        return None;
    }
    let escaped = email.replace('\'', "''");
    let users =
        dataverse_get(http, tokens, &format!("{api}/systemusers?$select=systemuserid&$filter=internalemailaddress eq '{escaped}'&$top=1"))
            .ok()?;
    let user = users.pointer("/value/0/systemuserid")?.as_str()?;
    let resources =
        dataverse_get(http, tokens, &format!("{api}/bookableresources?$select=bookableresourceid&$filter=_userid_value eq {user}&$top=1"))
            .ok()?;
    resources.pointer("/value/0/bookableresourceid")?.as_str().map(str::to_string)
}

/// Fixed duration. Matches the schedule mode Project for the web keeps on an MPP import ("Vaste duur").
const SCHEDULE_FIXED_DURATION: i32 = 192350001;

fn create_project(http: &mut dyn Transport, tokens: &TokenSet, api: &str, title: &str) -> Result<String, String> {
    // msdyn_CreateProjectV1 takes one parameter, `Project` (an msdyn_project). A flat
    // MSDyn_Subject field is rejected as an unknown action parameter.
    let body = json!({
        "Project": {
            "msdyn_subject": title,
            "msdyn_schedulemode": SCHEDULE_FIXED_DURATION
        }
    });
    let resp = dataverse_post(http, tokens, &format!("{api}/msdyn_CreateProjectV1"), &body)?;
    if let Some(id) = resp
        .get("ProjectId")
        .or_else(|| resp.get("projectid"))
        .or_else(|| resp.get("msdyn_projectid"))
        .or_else(|| resp.get("MSDyn_ProjectId"))
        .and_then(|v| v.as_str())
    {
        return Ok(id.to_string());
    }
    // Fallback: create the project row directly.
    let id = Uuid::new_v4();
    let entity = json!({
        "msdyn_projectid": id.to_string(),
        "msdyn_subject": title
    });
    dataverse_post(http, tokens, &format!("{api}/msdyn_projects"), &entity)?;
    Ok(id.to_string())
}

fn create_team_members(
    http: &mut dyn Transport,
    tokens: &TokenSet,
    api: &str,
    project_id: &str,
    plan: &ExportPlan,
    warnings: &mut Vec<String>,
) -> Result<HashMap<ResourceId, TeamMember>, String> {
    let mut map = HashMap::new();
    for person in &plan.people {
        let team_id = Uuid::new_v4();
        let mut member = json!({
            "@odata.type": "Microsoft.Dynamics.CRM.msdyn_projectteam",
            "msdyn_projectteamid": team_id.to_string(),
            "msdyn_name": person.name,
            "msdyn_project@odata.bind": format!("/msdyn_projects({project_id})")
        });
        // An assignment needs a project team member. Linking the bookable resource makes Planner
        // show that Microsoft 365 user; without a match the person is still assigned by name.
        if let Some(resource_id) = bookable_resource(http, tokens, api, &person.email) {
            if let Some(obj) = member.as_object_mut() {
                obj.insert("msdyn_bookableresourceid@odata.bind".into(), json!(format!("/bookableresources({resource_id})")));
            }
        } else if person.email.is_empty() {
            warnings.push(format!("{} has no email, so Planner cannot match a Microsoft 365 user", person.name));
        } else {
            warnings.push(format!("{} ({}) is not a bookable resource in this environment; assigned by name", person.name, person.email));
        }
        let body = json!({ "TeamMember": member });
        match dataverse_post(http, tokens, &format!("{api}/msdyn_CreateTeamMemberV1"), &body) {
            Ok(resp) => {
                let id = resp
                    .get("MSDyn_ProjectTeamId")
                    .or_else(|| resp.get("msdyn_projectteamid"))
                    .and_then(|v| v.as_str())
                    .map(|s| Uuid::parse_str(s).unwrap_or(team_id))
                    .unwrap_or(team_id);
                map.insert(person.resource_id, TeamMember { team_id: id, name: person.name.clone() });
            }
            Err(err) => warnings.push(format!("{}: {err}", person.name)),
        }
    }
    Ok(map)
}

fn task_entity(
    task: &ExportTask,
    project_id: &str,
    buckets: &HashMap<String, Uuid>,
    default_bucket: Uuid,
    guids: &HashMap<TaskId, Uuid>,
) -> Value {
    let bucket = buckets.get(&task.bucket_key).copied().unwrap_or(default_bucket);
    let mut entity = json!({
        "@odata.type": "Microsoft.Dynamics.CRM.msdyn_projecttask",
        "msdyn_projecttaskid": task.guid.to_string(),
        "msdyn_subject": task.title,
        "msdyn_outlinelevel": task.outline_level,
        "msdyn_effort": if task.milestone { 0.0 } else { task.effort_hours },
        "msdyn_duration": task.duration_days,
        "msdyn_ismilestone": task.milestone,
        "msdyn_project@odata.bind": format!("/msdyn_projects({project_id})"),
        "msdyn_projectbucket@odata.bind": format!("/msdyn_projectbuckets({bucket})")
    });
    if let Some(obj) = entity.as_object_mut() {
        if let Some(start) = task.start {
            let s = utc_datetime(start);
            obj.insert("msdyn_start".into(), json!(s));
            obj.insert("msdyn_scheduledstart".into(), json!(s));
            if task.milestone {
                obj.insert("msdyn_scheduledend".into(), json!(s));
            }
        }
        if !task.milestone
            && let Some(finish) = task.finish
        {
            obj.insert("msdyn_scheduledend".into(), json!(utc_datetime(finish)));
        }
        if let Some(parent) = task.parent_task_id.and_then(|id| guids.get(&id)) {
            obj.insert("msdyn_parenttask@odata.bind".into(), json!(format!("/msdyn_projecttasks({parent})")));
        }
    }
    entity
}

fn flush_operation_sets(
    http: &mut dyn Transport,
    tokens: &TokenSet,
    api: &str,
    project_id: &str,
    ops: &[Value],
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    for chunk in ops.chunks(MAX_OPS) {
        let set_id = create_operation_set(http, tokens, api, project_id)?;
        // Prefer V2 bulk create when the chunk is homogeneous-ish; fall back to V1 per entity.
        if chunk.len() > 1 {
            let body = json!({
                "OperationSetId": set_id,
                "Entities": chunk
            });
            if let Err(err) = dataverse_post(http, tokens, &format!("{api}/msdyn_PssCreateV2"), &body) {
                warnings.push(format!("bulk create failed ({err}); retrying one-by-one"));
                for entity in chunk {
                    let one = json!({ "OperationSetId": set_id, "Entity": entity });
                    if let Err(e) = dataverse_post(http, tokens, &format!("{api}/msdyn_PssCreateV1"), &one) {
                        warnings.push(e);
                    }
                }
            }
        } else if let Some(entity) = chunk.first() {
            let one = json!({ "OperationSetId": set_id, "Entity": entity });
            dataverse_post(http, tokens, &format!("{api}/msdyn_PssCreateV1"), &one)?;
        }
        let exec = json!({ "OperationSetId": set_id });
        dataverse_post(http, tokens, &format!("{api}/msdyn_ExecuteOperationSetV1"), &exec)?;
    }
    Ok(())
}

fn create_operation_set(http: &mut dyn Transport, tokens: &TokenSet, api: &str, project_id: &str) -> Result<String, String> {
    // Parameter names are ProjectId and Description. A navigation property is rejected.
    let body = json!({
        "ProjectId": project_id,
        "Description": "PlanCraft export"
    });
    let resp = dataverse_post(http, tokens, &format!("{api}/msdyn_CreateOperationSetV1"), &body)?;
    resp.get("OperationSetId")
        .or_else(|| resp.get("MSDyn_OperationSetId"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "operation set returned no id".into())
}

fn read_link_type_options(http: &mut dyn Transport, tokens: &TokenSet, api: &str) -> Result<LinkTypeMeta, String> {
    let path = format!(
        "{api}/EntityDefinitions(LogicalName='msdyn_projecttaskdependency')/Attributes(LogicalName='msdyn_projecttaskdependencylinktype')/Microsoft.Dynamics.CRM.PicklistAttributeMetadata?$select=LogicalName&$expand=OptionSet"
    );
    match dataverse_get(http, tokens, &path) {
        Ok(value) => Ok(LinkTypeMeta { column: "msdyn_projecttaskdependencylinktype".into(), options: option_pairs(&value) }),
        Err(_) => {
            let path = format!(
                "{api}/EntityDefinitions(LogicalName='msdyn_projecttaskdependency')/Attributes(LogicalName='msdyn_linktype')/Microsoft.Dynamics.CRM.PicklistAttributeMetadata?$select=LogicalName&$expand=OptionSet"
            );
            let value = dataverse_get(http, tokens, &path)?;
            Ok(LinkTypeMeta { column: "msdyn_linktype".into(), options: option_pairs(&value) })
        }
    }
}

fn option_pairs(value: &Value) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let options = value.pointer("/OptionSet/Options").or_else(|| value.pointer("/OptionSet/options")).and_then(|v| v.as_array());
    let Some(options) = options else { return out };
    for opt in options {
        let value = opt.get("Value").or_else(|| opt.get("value")).and_then(|v| v.as_i64()).and_then(|n| i32::try_from(n).ok());
        let label = opt
            .pointer("/Label/UserLocalizedLabel/Label")
            .or_else(|| opt.pointer("/label/userLocalizedLabel/label"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(value) = value {
            out.push((value, label));
        }
    }
    out
}

fn dataverse_get(http: &mut dyn Transport, tokens: &TokenSet, url: &str) -> Result<Value, String> {
    let auth = bearer(&tokens.access_token);
    let resp = http.request(
        "GET",
        url,
        &[("Authorization", &auth), ("Accept", "application/json"), ("OData-MaxVersion", "4.0"), ("OData-Version", "4.0")],
        None,
    )?;
    expect_ok(&resp, &[tokens.access_token.as_str()], "Dataverse")?;
    parse_json(&resp.body)
}

fn dataverse_post(http: &mut dyn Transport, tokens: &TokenSet, url: &str, body: &Value) -> Result<Value, String> {
    let auth = bearer(&tokens.access_token);
    let payload = json_body(body)?;
    let resp = http.request(
        "POST",
        url,
        &[
            ("Authorization", &auth),
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
            ("OData-MaxVersion", "4.0"),
            ("OData-Version", "4.0"),
        ],
        Some(&payload),
    )?;
    expect_ok(&resp, &[tokens.access_token.as_str()], "Dataverse")?;
    if resp.body.trim().is_empty() {
        return Ok(json!({}));
    }
    parse_json(&resp.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_export::http::HttpResponse;
    use crate::planner_export::http::mock::Scripted;
    use crate::planner_export::map::{ExportBucket, ExportDependency, ExportTarget, ExportTask};
    use plancraft_core::ids::TaskId;
    use plancraft_core::prelude::LinkType;

    #[test]
    fn premium_export_queues_deps_and_executes() {
        let mut http = Scripted::new(vec![
            // CreateProjectV1
            Ok(HttpResponse { status: 200, body: r#"{"MSDyn_ProjectId":"11111111-1111-1111-1111-111111111111"}"#.into() }),
            // link type metadata (fail → fallback fail → empty options)
            Ok(HttpResponse { status: 404, body: r#"{"error":{"message":"missing"}}"#.into() }),
            Ok(HttpResponse { status: 404, body: r#"{"error":{"message":"missing"}}"#.into() }),
            // CreateOperationSetV1
            Ok(HttpResponse { status: 200, body: r#"{"OperationSetId":"op1"}"#.into() }),
            // PssCreateV2
            Ok(HttpResponse { status: 204, body: String::new() }),
            // ExecuteOperationSetV1
            Ok(HttpResponse { status: 204, body: String::new() }),
        ]);
        let a = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let b = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let plan = ExportPlan {
            title: "Cycle".into(),
            target: ExportTarget::PremiumPss,
            buckets: vec![ExportBucket { key: "default".into(), name: "Tasks".into(), order: 0 }],
            tasks: vec![
                ExportTask {
                    task_id: TaskId(1),
                    guid: a,
                    title: "Design".into(),
                    bucket_key: "default".into(),
                    outline_level: 1,
                    parent_task_id: None,
                    start: None,
                    finish: None,
                    percent_complete: 0,
                    effort_hours: 8.0,
                    duration_days: 1.0,
                    milestone: false,
                    summary: false,
                    notes: String::new(),
                    assignees: vec![],
                },
                ExportTask {
                    task_id: TaskId(2),
                    guid: b,
                    title: "Build".into(),
                    bucket_key: "default".into(),
                    outline_level: 1,
                    parent_task_id: None,
                    start: None,
                    finish: None,
                    percent_complete: 0,
                    effort_hours: 8.0,
                    duration_days: 1.0,
                    milestone: false,
                    summary: false,
                    notes: String::new(),
                    assignees: vec![],
                },
            ],
            dependencies: vec![ExportDependency { predecessor: TaskId(1), successor: TaskId(2), kind: LinkType::FS }],
            people: vec![],
            notes: vec![],
        };
        let tokens = TokenSet { access_token: "atok".into(), refresh_token: None, expires_in: 3600 };
        let result = export_premium_with_tokens(&mut http, &tokens, &plan, "https://org.crm4.dynamics.com").unwrap();
        assert_eq!(result.tasks_created, 2);
        assert_eq!(result.dependencies_created, 1);
        assert!(http.calls.iter().any(|c| c.1.contains("msdyn_PssCreateV2") || c.1.contains("msdyn_PssCreateV1")));
        assert!(http.calls.iter().any(|c| c.1.contains("msdyn_ExecuteOperationSetV1")));
        let create = http.calls.iter().find(|c| c.1.contains("msdyn_CreateProjectV1")).expect("create project");
        let payload = create.2.as_deref().unwrap_or("");
        assert!(payload.contains("\"Project\""), "{payload}");
        assert!(payload.contains("msdyn_subject"), "{payload}");
        assert!(!payload.contains("MSDyn_Subject"), "{payload}");
        let opset = http.calls.iter().find(|c| c.1.contains("msdyn_CreateOperationSetV1")).expect("operation set");
        let payload = opset.2.as_deref().unwrap_or("");
        assert!(payload.contains("ProjectId"), "{payload}");
        assert!(!payload.contains("MSDyn_Project"), "{payload}");
    }
}
