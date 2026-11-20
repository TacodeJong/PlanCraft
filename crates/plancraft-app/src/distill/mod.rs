//! Distill Plan: a brief becomes a task tree the user reviews, then one undoable edit.

mod http;
mod keys;

use serde_json::Value;

use plancraft_core::limits::MAX_OUTLINE_LEVEL;
use plancraft_core::prelude::*;

use crate::settings::AiSettings;

pub use http::{Chat, ChatRequest, UreqChat};
pub use keys::{delete as delete_key, load as load_key, store as store_key, tail};

const MAX_SOURCE: usize = 80_000;

/// One row of the review. `level` is 1 for a top group.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftTask {
    pub name: String,
    pub level: u16,
    pub days: u32,
    pub estimated: bool,
    pub milestone: bool,
    pub resources: Vec<String>,
    pub after: Vec<String>,
    pub included: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub summary: String,
    pub tasks: Vec<DraftTask>,
}

const SYSTEM: &str = "\
You turn a project brief into a plan. Reply with one JSON object and no other text.
Shape: {\"summary\":\"one sentence\",\"tasks\":[{\"name\":\"...\",\"level\":1,\"days\":3,\"milestone\":false,\"resources\":[\"Name\"],\"after\":[\"Earlier task\"]}]}
Rules: level 1 is a top group and 2 is a child of it; never skip a level. A row with children is a group: days 0, milestone false. days is working days. A milestone has milestone true and days 0. after names tasks that must finish first; never a parent or a child. resources are people or teams the text names, or []. Use the language of the source. Include only work the text supports.";

/// Sends the brief, and retries once when the reply is not a plan.
pub fn distill(chat: &mut dyn Chat, ai: &AiSettings, api_key: &str, source: &str) -> Result<Draft, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err(crate::lang::text("distill.need_text").into());
    }
    if api_key.trim().is_empty() {
        return Err(crate::lang::text("distill.need_key").into());
    }
    if ai.model.trim().is_empty() {
        return Err(crate::lang::text("distill.need_model").into());
    }
    let request = ChatRequest {
        provider: ai.provider,
        model: ai.model.trim().to_string(),
        base_url: ai.base_url.clone(),
        api_key: api_key.trim().to_string(),
        system: SYSTEM.into(),
        user: clip(source),
    };
    let first = chat.complete(&request)?;
    match parse_draft(&first) {
        Ok(draft) => Ok(draft),
        Err(err) => {
            let mut again = request;
            again.user = format!(
                "The previous reply was not usable JSON ({err}). Reply again with only the JSON object, no markdown.\n\n{}",
                clip(&first)
            );
            let second = chat.complete(&again)?;
            parse_draft(&second)
        }
    }
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_SOURCE).collect()
}

/// Reads a model reply. Malformed input is an error, never a panic.
pub fn parse_draft(raw: &str) -> Result<Draft, String> {
    let slice = json_slice(raw);
    let value: Value = serde_json::from_str(slice).map_err(|_| "the reply is not a JSON object".to_string())?;
    let list = value
        .get("tasks")
        .or_else(|| value.get("items"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| "the reply has no tasks array".to_string())?;
    let mut tasks = Vec::new();
    for node in list {
        flatten(node, 1, &mut tasks);
    }
    if tasks.is_empty() {
        return Err("the reply has no named tasks".into());
    }
    let summary = value.get("summary").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    Ok(Draft { summary, tasks })
}

fn json_slice(raw: &str) -> &str {
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    let Some(start) = trimmed.find('{') else { return trimmed };
    let Some(end) = trimmed.rfind('}') else { return trimmed };
    if end <= start {
        return trimmed;
    }
    trimmed.get(start..=end).unwrap_or(trimmed)
}

fn flatten(node: &Value, level: u16, out: &mut Vec<DraftTask>) {
    let children = node.get("children").and_then(|c| c.as_array()).filter(|c| !c.is_empty());
    if let Some(task) = task_from(node, level, children.is_some()) {
        out.push(task);
    }
    if let Some(list) = children {
        let next = level.saturating_add(1).min(MAX_OUTLINE_LEVEL);
        for child in list {
            flatten(child, next, out);
        }
    }
}

fn task_from(node: &Value, level: u16, group: bool) -> Option<DraftTask> {
    let name = text_field(node, &["name", "title"])?;
    if name.is_empty() {
        return None;
    }
    let stated = node.get("level").and_then(|v| v.as_u64()).and_then(|n| u16::try_from(n).ok());
    let level = stated.unwrap_or(level).clamp(1, MAX_OUTLINE_LEVEL);
    let milestone = !group && bool_field(node, &["milestone", "is_milestone"]);
    let (days, estimated) = if group || milestone { (0, false) } else { days_of(node) };
    Some(DraftTask {
        name,
        level,
        days,
        estimated,
        milestone,
        resources: names_field(node, &["resources", "resource"]),
        after: names_field(node, &["after", "predecessors", "depends_on"]),
        included: node.get("included").and_then(|v| v.as_bool()).unwrap_or(true),
    })
}

fn text_field(node: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| node.get(*k).and_then(|v| v.as_str()).map(|s| s.trim().to_string())).filter(|s| !s.is_empty())
}

fn bool_field(node: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|k| node.get(*k).and_then(|v| v.as_bool()).unwrap_or(false))
}

fn days_of(node: &Value) -> (u32, bool) {
    for key in ["days", "duration", "duration_days"] {
        let Some(value) = node.get(key) else { continue };
        if let Some(n) = value.as_f64().filter(|n| n.is_finite() && *n >= 0.0) {
            return (n.round().clamp(0.0, 1000.0) as u32, false);
        }
        if let Some(text) = value.as_str() {
            let estimated = text.contains('?');
            let number: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
            if let Ok(n) = number.parse::<f64>()
                && n.is_finite()
                && n >= 0.0
            {
                return (n.round().clamp(0.0, 1000.0) as u32, estimated);
            }
        }
    }
    (1, false)
}

fn names_field(node: &Value, keys: &[&str]) -> Vec<String> {
    let Some(value) = keys.iter().find_map(|k| node.get(*k)) else { return Vec::new() };
    match value {
        Value::String(s) => split_names(s),
        Value::Array(items) => items.iter().filter_map(|v| v.as_str()).flat_map(split_names).collect(),
        _ => Vec::new(),
    }
}

fn split_names(text: &str) -> Vec<String> {
    text.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Adds the included rows. `replace` removes the current tasks first. One call, so the caller's
/// edit is one undo step. A dependency that would be illegal is skipped.
pub fn apply_draft(p: &mut Project, draft: &Draft, replace: bool, chain: bool) -> Result<usize, String> {
    if replace {
        let ids: Vec<TaskId> = p.tasks.iter().map(|t| t.id).collect();
        if !ids.is_empty() {
            p.delete_tasks(&ids).map_err(|e| e.to_string())?;
        }
    }
    let rows: Vec<&DraftTask> = draft.tasks.iter().filter(|t| t.included && !t.name.trim().is_empty()).collect();
    if rows.is_empty() {
        return Err(crate::lang::text("distill.none").into());
    }
    let start = p.tasks.len();
    let mut ids = Vec::new();
    for (i, task) in rows.iter().enumerate() {
        let deeper = rows.get(i.saturating_add(1)).is_some_and(|next| next.level > task.level);
        let id = if task.milestone && !deeper { p.add_milestone(task.name.trim(), None) } else { p.add_task(task.name.trim(), None) };
        if let Some(row) = p.task_mut(id) {
            row.outline_level = task.level.clamp(1, MAX_OUTLINE_LEVEL);
        }
        ids.push(id);
    }
    let mut prev = start.checked_sub(1).and_then(|i| p.tasks.get(i)).map(|t| t.outline_level).unwrap_or(0);
    if let Some(added) = p.tasks.get_mut(start..) {
        for task in added {
            task.outline_level = task.outline_level.clamp(1, prev.saturating_add(1).min(MAX_OUTLINE_LEVEL));
            prev = task.outline_level;
        }
    }
    let levels: Vec<u16> = p.tasks.get(start..).map(|rows| rows.iter().map(|t| t.outline_level).collect()).unwrap_or_default();
    let per = p.settings.time.minutes_per_day.max(1);
    for (i, task) in rows.iter().enumerate() {
        let Some(id) = ids.get(i).copied() else { continue };
        let group = levels.get(i).zip(levels.get(i.saturating_add(1))).is_some_and(|(a, b)| b > a);
        if group {
            if let Some(row) = p.task_mut(id) {
                row.kind = TaskKind::Task;
            }
            continue;
        }
        if task.milestone {
            continue;
        }
        let days = if task.days == 0 { 1 } else { task.days };
        let minutes = i64::from(days).saturating_mul(per);
        p.set_duration(id, minutes, task.estimated).map_err(|e| e.to_string())?;
        if !task.resources.is_empty() {
            p.apply_resource_names(id, &task.resources.join(", ")).map_err(|e| e.to_string())?;
        }
    }
    let mut by_name: Vec<(String, TaskId)> = Vec::new();
    for (i, task) in rows.iter().enumerate() {
        let group = levels.get(i).zip(levels.get(i.saturating_add(1))).is_some_and(|(a, b)| b > a);
        if group {
            continue;
        }
        if let Some(id) = ids.get(i).copied() {
            by_name.push((task.name.trim().to_lowercase(), id));
        }
    }
    for (i, task) in rows.iter().enumerate() {
        let Some(id) = ids.get(i).copied() else { continue };
        let group = levels.get(i).zip(levels.get(i.saturating_add(1))).is_some_and(|(a, b)| b > a);
        if group {
            continue;
        }
        for pred_name in &task.after {
            link_named(p, &by_name, pred_name, id);
        }
        if chain && task.after.is_empty() {
            let level = levels.get(i).copied().unwrap_or(1);
            if let Some(pred) = previous_leaf(&rows, &levels, &ids, i, level) {
                link_named(p, &by_name, &pred, id);
            }
        }
    }
    Ok(rows.len())
}

fn previous_leaf(rows: &[&DraftTask], levels: &[u16], ids: &[TaskId], i: usize, level: u16) -> Option<String> {
    rows.get(..i)?.iter().enumerate().rev().find_map(|(j, task)| {
        let same = levels.get(j).is_some_and(|n| *n == level);
        let group = levels.get(j).zip(levels.get(j.saturating_add(1))).is_some_and(|(a, b)| b > a);
        if same && !group && ids.get(j).is_some() { Some(task.name.clone()) } else { None }
    })
}

fn link_named(p: &mut Project, by_name: &[(String, TaskId)], pred_name: &str, succ: TaskId) {
    let key = pred_name.trim().to_lowercase();
    let Some((_, pred)) = by_name.iter().find(|(name, _)| name == &key) else { return };
    if *pred == succ {
        return;
    }
    let _ = p.link((*pred).into(), succ, LinkType::FS, Lag::None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::EditExt;
    use plancraft_core::workspace::{ChangeSource, Workspace};

    const CANNED: &str = r#"{"summary":"A small build","tasks":[
        {"name":"Phase","level":1,"days":0,"milestone":false,"resources":[],"after":[]},
        {"name":"Design","level":2,"days":3,"milestone":false,"resources":["Ana"],"after":[]},
        {"name":"Build","level":2,"days":2,"resources":[],"after":["Design"]},
        {"name":"Done","level":2,"milestone":true,"days":0,"after":["Build"]}
    ]}"#;

    struct Script {
        replies: Vec<Result<String, String>>,
        calls: usize,
    }

    impl Chat for Script {
        fn complete(&mut self, request: &ChatRequest) -> Result<String, String> {
            assert!(!format!("{request:?}").contains(&request.api_key));
            let i = self.calls;
            self.calls = self.calls.saturating_add(1);
            self.replies.get(i).cloned().unwrap_or_else(|| Err("no reply".into()))
        }
    }

    #[test]
    fn a_canned_reply_becomes_the_task_tree() {
        let draft = parse_draft(CANNED).unwrap();
        assert_eq!(draft.summary, "A small build");
        assert_eq!(draft.tasks.len(), 4);
        let mut p = Project::new("P", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let n = apply_draft(&mut p, &draft, false, false).unwrap();
        assert_eq!(n, 4);
        assert!(p.has_children(0), "Phase is the group");
        assert_eq!(p.tasks[1].name, "Design");
        assert_eq!(p.tasks[1].outline_level, 2);
        assert_eq!(p.tasks[1].duration, 3 * p.settings.time.minutes_per_day);
        assert_eq!(p.format_resource_names(p.tasks[1].id), "Ana");
        let build = p.tasks[2].id;
        assert!(p.tasks[2].predecessors.iter().any(|d| d.predecessor.local() == Some(p.tasks[1].id)));
        assert!(p.task(p.tasks[3].id).unwrap().is_milestone());
        assert!(p.tasks[3].predecessors.iter().any(|d| d.predecessor.local() == Some(build)));
    }

    #[test]
    fn a_nested_reply_and_a_fence_parse() {
        let raw = "```json\n{\"summary\":\"s\",\"tasks\":[{\"name\":\"Phase\",\"children\":[{\"name\":\"Design\",\"days\":2}]}]}\n```";
        let draft = parse_draft(raw).unwrap();
        assert_eq!(draft.tasks.len(), 2);
        assert_eq!(draft.tasks[1].level, 2);
        assert_eq!(draft.tasks[1].days, 2);
    }

    #[test]
    fn a_malformed_reply_is_an_error_and_retried_once() {
        assert!(parse_draft("not json").is_err());
        assert!(parse_draft("{\"tasks\":\"nope\"}").is_err());
        assert!(parse_draft("{\"tasks\":[]}").is_err());
        let ai = AiSettings { provider: crate::settings::AiProvider::OpenAi, model: "gpt-4o-mini".into(), base_url: String::new() };
        let mut bad = Script { replies: vec![Ok("nope".into()), Ok("still nope".into())], calls: 0 };
        let err = distill(&mut bad, &ai, "sk-test", "Build a shed").unwrap_err();
        assert_eq!(bad.calls, 2, "{err}");
        let mut fixed = Script { replies: vec![Ok("{".into()), Ok(CANNED.into())], calls: 0 };
        let draft = distill(&mut fixed, &ai, "sk-test", "Build a shed").unwrap();
        assert_eq!(draft.tasks[1].name, "Design");
        assert_eq!(fixed.calls, 2);
        let mut down = Script { replies: vec![Err("offline".into())], calls: 0 };
        assert!(distill(&mut down, &ai, "sk-test", "Build a shed").is_err());
        assert_eq!(down.calls, 1, "a transport failure is not retried");
    }

    #[test]
    fn accept_is_one_undo_step() {
        let draft = parse_draft(CANNED).unwrap();
        let mut ws = Workspace::default();
        ws.add(Project::new("P", plancraft_core::dates::ymd(2026, 10, 19).unwrap()), None);
        let doc = ws.active_doc_mut().unwrap();
        doc.edit_msg("Distill Plan", |p| apply_draft(p, &draft, false, false).map(|_| ())).unwrap();
        assert_eq!(doc.project.tasks.len(), 4);
        assert_eq!(doc.history().len(), 1);
        assert_eq!(doc.undo_label(), Some("Distill Plan"));
        assert!(doc.try_undo_as(ChangeSource::User).unwrap());
        assert!(doc.project.tasks.is_empty(), "one undo removes the whole draft");
        assert!(!doc.can_undo());
    }

    #[test]
    fn scrub_removes_the_key() {
        assert_eq!(keys::scrub("token sk-secret rejected", "sk-secret"), "token •••• rejected");
        assert_eq!(tail("sk-secret-1234"), "1234");
    }
}
