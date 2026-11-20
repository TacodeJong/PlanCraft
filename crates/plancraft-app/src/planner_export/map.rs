//! PlanCraft project → Planner export intent (no network).

use std::collections::HashMap;

use plancraft_core::dates::{DateTime, FormatExt};
use plancraft_core::prelude::*;
use uuid::Uuid;

/// Where the push lands. Premium is required when dependencies must be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportTarget {
    /// Graph v1.0 board: plan / buckets / tasks / assignees / dates / %. No dependencies.
    BasicBoard,
    /// Dataverse + Project Scheduling Service (Plan 1+). Dependencies included.
    PremiumPss,
}

impl ExportTarget {
    pub const ALL: [ExportTarget; 2] = [ExportTarget::PremiumPss, ExportTarget::BasicBoard];

    pub fn label(self) -> &'static str {
        crate::lang::text(match self {
            ExportTarget::PremiumPss => "planner.target.premium",
            ExportTarget::BasicBoard => "planner.target.basic",
        })
    }

    pub fn includes_dependencies(self) -> bool {
        matches!(self, ExportTarget::PremiumPss)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportBucket {
    pub key: String,
    pub name: String,
    pub order: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportAssignee {
    /// PlanCraft resource id (for the durable map).
    pub resource_id: ResourceId,
    pub name: String,
    /// Email used to resolve an Entra user; empty when unknown.
    pub email: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportTask {
    pub task_id: TaskId,
    /// Client-supplied GUID for premium binds in one operation set.
    pub guid: Uuid,
    pub title: String,
    pub bucket_key: String,
    pub outline_level: u16,
    pub parent_task_id: Option<TaskId>,
    pub start: Option<DateTime>,
    pub finish: Option<DateTime>,
    pub percent_complete: u8,
    pub effort_hours: f64,
    pub milestone: bool,
    pub notes: String,
    pub assignees: Vec<ResourceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportDependency {
    pub predecessor: TaskId,
    pub successor: TaskId,
    pub kind: LinkType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportPlan {
    pub title: String,
    pub target: ExportTarget,
    pub buckets: Vec<ExportBucket>,
    pub tasks: Vec<ExportTask>,
    pub dependencies: Vec<ExportDependency>,
    pub people: Vec<ExportAssignee>,
    pub notes: Vec<String>,
}

/// Builds the export intent from a scheduled project.
pub fn build_export(project: &Project, target: ExportTarget) -> ExportPlan {
    let title = if project.name.trim().is_empty() { "PlanCraft".into() } else { project.name.trim().to_string() };
    let mut notes = Vec::new();
    if matches!(target, ExportTarget::BasicBoard) {
        notes.push(crate::lang::text("planner.note.no_deps").into());
    }

    let people = people_of(project);
    let buckets = buckets_of(project, target);
    let default_bucket = buckets.first().map(|b| b.key.clone()).unwrap_or_else(|| "default".into());
    let bucket_for_row = bucket_keys_for_rows(project, &buckets, &default_bucket);

    let parents = parent_ids(project);
    let mut tasks = Vec::new();
    for (i, task) in project.tasks.iter().enumerate() {
        if matches!(target, ExportTarget::BasicBoard) && task.sched.is_summary {
            continue;
        }
        let start = task.sched.start;
        let mut finish = task.sched.finish;
        let milestone = task.is_milestone();
        if milestone && let Some(s) = start {
            finish = Some(s);
        }
        let effort_hours = {
            let minutes = if task.sched.work > 0 { task.sched.work } else { task.duration.max(0) };
            (minutes as f64) / 60.0
        };
        let assignees: Vec<ResourceId> = project.assignments_for_task(task.id).map(|a| a.resource).collect();
        let bucket_key = bucket_for_row.get(i).cloned().unwrap_or_else(|| default_bucket.clone());
        tasks.push(ExportTask {
            task_id: task.id,
            guid: Uuid::new_v4(),
            title: task.name.trim().to_string(),
            bucket_key,
            outline_level: task.outline_level.max(1),
            parent_task_id: parents.get(&task.id).copied().flatten(),
            start,
            finish,
            percent_complete: task.percent_complete.min(100),
            effort_hours,
            milestone,
            notes: task.notes.clone(),
            assignees,
        });
    }

    let task_set: std::collections::HashSet<TaskId> = tasks.iter().map(|t| t.task_id).collect();
    let mut dependencies = Vec::new();
    if target.includes_dependencies() {
        for task in &project.tasks {
            if !task_set.contains(&task.id) {
                continue;
            }
            for dep in &task.predecessors {
                let Some(pred) = dep.predecessor.local() else { continue };
                if !task_set.contains(&pred) {
                    continue;
                }
                dependencies.push(ExportDependency { predecessor: pred, successor: task.id, kind: dep.kind });
            }
        }
    }

    if tasks.iter().any(|t| t.start.is_none() || t.finish.is_none()) {
        notes.push(crate::lang::text("planner.note.undated").into());
    }

    ExportPlan { title, target, buckets, tasks, dependencies, people, notes }
}

fn people_of(project: &Project) -> Vec<ExportAssignee> {
    let mut used = std::collections::HashSet::new();
    for a in &project.assignments {
        used.insert(a.resource);
    }
    project
        .resources
        .iter()
        .filter(|r| used.contains(&r.id) || !r.email.trim().is_empty())
        .filter(|r| matches!(r.kind, ResourceType::Work) && matches!(r.category, ResourceCategory::TeamMember | ResourceCategory::Other))
        .map(|r| ExportAssignee { resource_id: r.id, name: r.name.clone(), email: r.email.trim().to_string() })
        .collect()
}

fn buckets_of(project: &Project, target: ExportTarget) -> Vec<ExportBucket> {
    match target {
        ExportTarget::BasicBoard => {
            // Board columns from workflow status.
            let mut seen = Vec::new();
            for task in &project.tasks {
                if task.sched.is_summary {
                    continue;
                }
                let name = status_bucket_name(task.status);
                if !seen.iter().any(|n: &String| n == name) {
                    seen.push(name.to_string());
                }
            }
            if seen.is_empty() {
                seen.push(status_bucket_name(TaskStatus::NotStarted).to_string());
            }
            seen.into_iter().enumerate().map(|(i, name)| ExportBucket { key: format!("status:{name}"), name, order: i as u32 }).collect()
        }
        ExportTarget::PremiumPss => {
            let mut buckets = Vec::new();
            for task in &project.tasks {
                if task.outline_level == 1 && task.sched.is_summary {
                    buckets.push(ExportBucket {
                        key: format!("group:{}", task.id.0),
                        name: task.name.trim().to_string(),
                        order: buckets.len() as u32,
                    });
                }
            }
            if buckets.is_empty() {
                buckets.push(ExportBucket { key: "default".into(), name: "Tasks".into(), order: 0 });
            }
            buckets
        }
    }
}

fn status_bucket_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::NotStarted => "Not started",
        TaskStatus::Started => "Started",
        TaskStatus::Busy => "In progress",
        TaskStatus::Done => "Done",
    }
}

fn bucket_keys_for_rows(project: &Project, buckets: &[ExportBucket], default_key: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(project.tasks.len());
    let mut current_group: Option<String> = None;
    for task in &project.tasks {
        if task.outline_level == 1 && task.sched.is_summary {
            let key = format!("group:{}", task.id.0);
            if buckets.iter().any(|b| b.key == key) {
                current_group = Some(key);
            }
        }
        let key = if buckets.iter().any(|b| b.key.starts_with("status:")) {
            let name = status_bucket_name(task.status);
            format!("status:{name}")
        } else {
            current_group.clone().unwrap_or_else(|| default_key.to_string())
        };
        out.push(key);
    }
    out
}

fn parent_ids(project: &Project) -> HashMap<TaskId, Option<TaskId>> {
    let mut stack: Vec<(u16, TaskId)> = Vec::new();
    let mut map = HashMap::new();
    for task in &project.tasks {
        while stack.last().is_some_and(|(level, _)| *level >= task.outline_level) {
            stack.pop();
        }
        let parent = stack.last().map(|(_, id)| *id);
        map.insert(task.id, parent);
        stack.push((task.outline_level, task.id));
    }
    map
}

/// UTC `DateTimeOffset` string Graph / Dataverse accept (`…Z`).
pub fn utc_datetime(dt: DateTime) -> String {
    format!("{}Z", dt.format("%Y-%m-%dT%H:%M:%S"))
}

/// Maps a PlanCraft link to the Graph beta dependency constraint name (documentation only;
/// production deps go through PSS option-set integers).
#[cfg(test)]
pub fn constraint_name(kind: LinkType) -> &'static str {
    match kind {
        LinkType::FS => "sourceFinishDrivesTargetStart",
        LinkType::SS => "sourceStartDrivesTargetStart",
        LinkType::FF => "sourceFinishDrivesTargetFinish",
        LinkType::SF => "targetStartDrivesSourceFinish",
    }
}

/// Project Operations sample integers for `msdyn_linktype`. Prefer live option-set values when
/// metadata is available; these are the documented fallbacks.
pub fn link_type_fallback(kind: LinkType) -> i32 {
    match kind {
        LinkType::FS => 192_350_000,
        LinkType::SS => 192_350_001,
        LinkType::FF => 192_350_002,
        LinkType::SF => 192_350_003,
    }
}

/// Resolves a link-type integer from option-set labels (case-insensitive contains).
pub fn link_type_from_options(kind: LinkType, options: &[(i32, String)]) -> i32 {
    let needles: &[&str] = match kind {
        LinkType::FS => &["finish to start", "finishtostart", "finish-to-start", "fs"],
        LinkType::SS => &["start to start", "starttostart", "start-to-start", "ss"],
        LinkType::FF => &["finish to finish", "finishtofinish", "finish-to-finish", "ff"],
        LinkType::SF => &["start to finish", "starttofinish", "start-to-finish", "sf"],
    };
    for needle in needles {
        for (value, label) in options {
            let lower = label.to_ascii_lowercase();
            if lower.contains(needle) {
                return *value;
            }
        }
    }
    link_type_fallback(kind)
}

/// Planner order hint that places an item first.
pub fn first_order_hint() -> &'static str {
    " !"
}

#[cfg(test)]
mod tests {
    use super::*;
    use plancraft_core::schedule::recalculate;

    fn sample() -> Project {
        let mut p = Project::new("Cycle", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let phase = p.add_task("Phase", None);
        let design = p.add_task("Design", None);
        let build = p.add_task("Build", None);
        let done = p.add_milestone("Done", None);
        if let Some(t) = p.task_mut(phase) {
            t.outline_level = 1;
        }
        if let Some(t) = p.task_mut(design) {
            t.outline_level = 2;
        }
        if let Some(t) = p.task_mut(build) {
            t.outline_level = 2;
        }
        if let Some(t) = p.task_mut(done) {
            t.outline_level = 2;
        }
        p.set_duration(design, 3 * p.settings.time.minutes_per_day, false).unwrap();
        p.set_duration(build, 2 * p.settings.time.minutes_per_day, false).unwrap();
        p.link(design.into(), build, LinkType::FS, Lag::None).unwrap();
        p.link(build.into(), done, LinkType::FS, Lag::None).unwrap();
        let ana = p.add_resource("Ana");
        if let Some(r) = p.resource_mut(ana) {
            r.email = "ana@example.com".into();
        }
        p.assign(design, ana, Some(1.0), None).unwrap();
        recalculate(&mut p);
        p
    }

    #[test]
    fn premium_export_keeps_hierarchy_and_dependencies() {
        let plan = build_export(&sample(), ExportTarget::PremiumPss);
        assert_eq!(plan.title, "Cycle");
        assert!(plan.buckets.iter().any(|b| b.name == "Phase"));
        assert_eq!(plan.tasks.len(), 4, "summaries stay for premium WBS");
        assert_eq!(plan.dependencies.len(), 2);
        assert!(plan.people.iter().any(|p| p.email == "ana@example.com"));
        let design = plan.tasks.iter().find(|t| t.title == "Design").unwrap();
        assert!(design.start.is_some() && design.finish.is_some());
        assert!(!design.assignees.is_empty());
        let done = plan.tasks.iter().find(|t| t.title == "Done").unwrap();
        assert!(done.milestone);
        assert_eq!(done.start, done.finish);
    }

    #[test]
    fn basic_export_skips_summaries_and_dependencies() {
        let plan = build_export(&sample(), ExportTarget::BasicBoard);
        assert!(plan.tasks.iter().all(|t| t.title != "Phase"));
        assert!(plan.dependencies.is_empty());
        assert!(!plan.notes.is_empty());
        assert!(plan.buckets.iter().any(|b| b.name == "Not started" || b.name == "In progress" || b.name == "Done"));
    }

    #[test]
    fn link_type_options_prefer_labels() {
        let options = vec![
            (1, "Finish-to-Start".into()),
            (2, "Start-to-Start".into()),
            (3, "Finish-to-Finish".into()),
            (4, "Start-to-Finish".into()),
        ];
        assert_eq!(link_type_from_options(LinkType::FS, &options), 1);
        assert_eq!(link_type_from_options(LinkType::SS, &options), 2);
        assert_eq!(link_type_from_options(LinkType::FF, &options), 3);
        assert_eq!(link_type_from_options(LinkType::SF, &options), 4);
        assert_eq!(link_type_from_options(LinkType::FS, &[]), 192_350_000);
    }

    #[test]
    fn constraint_names_match_graph_beta_docs() {
        assert_eq!(constraint_name(LinkType::FS), "sourceFinishDrivesTargetStart");
        assert_eq!(constraint_name(LinkType::SF), "targetStartDrivesSourceFinish");
    }

    #[test]
    fn utc_datetime_appends_z() {
        let dt = plancraft_core::dates::ymd_hm(2026, 12, 1, 8, 0).unwrap();
        assert_eq!(utc_datetime(dt), "2026-12-01T08:00:00Z");
    }
}
