//! Registered engine commands (`file.*`, `edit.*`, `task.*`, `resource.*`, `project.*`, …).

mod params;
mod project;
mod resource;
mod task;

use serde_json::{Value, json};

use crate::command::CommandSpec;
use crate::event::Event;
use crate::session::{Session, ok_doc, ok_empty};

macro_rules! cmd {
    ($id:literal, $label:literal, $menu:expr, $shortcut:expr, $params:literal, $enabled:expr, $run:expr $(,)?) => {
        CommandSpec {
            id: $id,
            label: $label,
            menu: $menu,
            shortcut: $shortcut,
            params: $params,
            enabled: $enabled,
            run: $run,
            journal: true,
        }
    };
}

macro_rules! query {
    ($id:literal, $label:literal, $params:literal, $enabled:expr, $run:expr $(,)?) => {
        CommandSpec { id: $id, label: $label, menu: &[], shortcut: None, params: $params, enabled: $enabled, run: $run, journal: false }
    };
}

fn always_ok(_: &Session) -> Result<(), String> {
    Ok(())
}

fn needs_doc(s: &Session) -> Result<(), String> {
    s.require_active()
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value, String> {
    let today = plancraft_core::dates::today();
    let name = params::opt_str(p, "name")?.unwrap_or("New Project").to_string();
    let project = plancraft_core::model::Project::new(name, today);
    let handle = s.add_project(project, None);
    Ok(ok_doc(handle))
}

fn run_new_demo(s: &mut Session, _p: &Value) -> Result<Value, String> {
    let today = plancraft_core::dates::today();
    let back = i64::from(plancraft_core::dates::weekday_index(today.weekday())) + 14;
    let monday = plancraft_core::dates::add_days(today, -back).unwrap_or(today);
    let handle = s.add_project(plancraft_core::sample::demo_project(monday), None);
    Ok(ok_doc(handle))
}

fn run_open(s: &mut Session, p: &Value) -> Result<Value, String> {
    let path = p.get("path").and_then(|v| v.as_str()).ok_or_else(|| "missing string param 'path'".to_string())?;
    let handle = s.open_path(path)?;
    Ok(ok_doc(handle))
}

fn run_save(s: &mut Session, _p: &Value) -> Result<Value, String> {
    let doc = s.active_doc_mut().ok_or_else(|| "no open document".to_string())?;
    if doc.path.is_none() {
        return Err("document has no path; use file.saveAs".into());
    }
    doc.save().map_err(|e| e.to_string())?;
    s.push_event(Event::ProjectChanged);
    Ok(ok_empty())
}

fn run_save_as(s: &mut Session, p: &Value) -> Result<Value, String> {
    let path = p.get("path").and_then(|v| v.as_str()).ok_or_else(|| "missing string param 'path'".to_string())?;
    let doc = s.active_doc_mut().ok_or_else(|| "no open document".to_string())?;
    doc.save_as(path).map_err(|e| e.to_string())?;
    s.push_event(Event::ProjectChanged);
    Ok(ok_empty())
}

fn run_close(s: &mut Session, _p: &Value) -> Result<Value, String> {
    let Some(i) = s.workspace.active else {
        return Err("no open document".into());
    };
    if i < s.workspace.docs.len() {
        s.workspace.docs.remove(i);
    }
    s.workspace.active = if s.workspace.docs.is_empty() { None } else { Some(i.min(s.workspace.docs.len().saturating_sub(1))) };
    s.push_event(Event::ProjectChanged);
    Ok(ok_empty())
}

fn run_undo(s: &mut Session, _p: &Value) -> Result<Value, String> {
    let source = s.source;
    let doc = s.active_doc_mut().ok_or_else(|| "no open document".to_string())?;
    match doc.try_undo_as(source) {
        Ok(true) => {
            s.push_event(Event::ProjectChanged);
            Ok(ok_empty())
        }
        Ok(false) => Err("nothing to undo".into()),
        Err(why) => Err(why),
    }
}

fn run_redo(s: &mut Session, _p: &Value) -> Result<Value, String> {
    let source = s.source;
    let doc = s.active_doc_mut().ok_or_else(|| "no open document".to_string())?;
    match doc.try_redo_as(source) {
        Ok(true) => {
            s.push_event(Event::ProjectChanged);
            Ok(ok_empty())
        }
        Ok(false) => Err("nothing to redo".into()),
        Err(why) => Err(why),
    }
}

fn run_inspect(s: &mut Session, _p: &Value) -> Result<Value, String> {
    let doc = s.active_doc().ok_or_else(|| "no open document".to_string())?;
    Ok(json!({
        "document": doc.handle,
        "name": doc.project.name,
        "path": doc.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "dirty": doc.dirty,
        "tasks": doc.project.tasks.len(),
        "resources": doc.project.resources.len(),
        "undo": doc.undo_label(),
        "redo": doc.redo_label(),
    }))
}

fn run_list_docs(s: &mut Session, _p: &Value) -> Result<Value, String> {
    let docs: Vec<Value> = s
        .workspace
        .docs
        .iter()
        .map(|d| {
            json!({
                "document": d.handle,
                "name": d.project.name,
                "path": d.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                "dirty": d.dirty,
                "active": s.workspace.active_doc().is_some_and(|a| a.handle == d.handle),
            })
        })
        .collect();
    Ok(json!({ "documents": docs }))
}

fn run_commands_list(_s: &mut Session, _p: &Value) -> Result<Value, String> {
    let list: Vec<Value> = crate::command::registry()
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "label": c.label,
                "menu": c.menu,
                "shortcut": c.shortcut,
                "params": c.params,
                "journal": c.journal,
            })
        })
        .collect();
    Ok(json!({ "commands": list }))
}

/// Full engine command table (shared by UI adapter, CLI and MCP `run_command`).
pub static ALL: &[CommandSpec] = &[
    // —— File ——
    cmd!("file.new", "New Project", &["File"], Some("Cmd+N"), r#"{"name":"string?"}"#, always_ok, run_new),
    cmd!("file.newDemo", "New Demo Project", &["File"], None, r#"{}"#, always_ok, run_new_demo),
    cmd!("file.open", "Open…", &["File"], Some("Cmd+O"), r#"{"path":"string"}"#, always_ok, run_open),
    cmd!("file.save", "Save", &["File"], Some("Cmd+S"), r#"{}"#, needs_doc, run_save),
    cmd!("file.saveAs", "Save As…", &["File"], Some("Cmd+Shift+S"), r#"{"path":"string"}"#, needs_doc, run_save_as,),
    cmd!("file.close", "Close", &["File"], None, r#"{}"#, needs_doc, run_close),
    // —— Edit ——
    cmd!("edit.undo", "Undo", &["Edit"], Some("Cmd+Z"), r#"{}"#, needs_doc, run_undo),
    cmd!("edit.redo", "Redo", &["Edit"], Some("Cmd+Shift+Z"), r#"{}"#, needs_doc, run_redo),
    // —— Task ——
    cmd!("task.insert", "Insert Task", &["Task"], None, r#"{"name":"string?","at":"number?"}"#, needs_doc, task::run_insert,),
    cmd!(
        "task.insertMilestone",
        "Insert Milestone",
        &["Task"],
        None,
        r#"{"name":"string?","at":"number?"}"#,
        needs_doc,
        task::run_insert_milestone,
    ),
    cmd!(
        "task.insertSummary",
        "Insert Summary Task",
        &["Task"],
        None,
        r#"{"name":"string?","tasks":"number[]?"}"#,
        needs_doc,
        task::run_insert_summary,
    ),
    cmd!("task.delete", "Delete Tasks", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_delete,),
    cmd!("task.indent", "Indent", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_indent),
    cmd!("task.outdent", "Outdent", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_outdent),
    cmd!("task.moveUp", "Move Up", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_move_up),
    cmd!("task.moveDown", "Move Down", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_move_down),
    cmd!(
        "task.link",
        "Link Tasks",
        &["Task"],
        None,
        r#"{"tasks":"number[]","type":"FS|SS|FF|SF?","lag":"string|number?"}"#,
        needs_doc,
        task::run_link,
    ),
    cmd!(
        "task.linkPair",
        "Link Task Pair",
        &["Task"],
        None,
        r#"{"predecessor":"number","successor":"number","type":"FS|SS|FF|SF?","lag":"string|number?"}"#,
        needs_doc,
        task::run_link_pair,
    ),
    cmd!("task.unlink", "Unlink Tasks", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_unlink),
    cmd!(
        "task.unlinkPair",
        "Unlink Task Pair",
        &["Task"],
        None,
        r#"{"predecessor":"number","successor":"number"}"#,
        needs_doc,
        task::run_unlink_pair,
    ),
    cmd!(
        "task.setPercent",
        "Set % Complete",
        &["Task"],
        None,
        r#"{"tasks":"number[]","percent":"number"}"#,
        needs_doc,
        task::run_set_percent,
    ),
    cmd!(
        "task.setMode",
        "Set Schedule Mode",
        &["Task"],
        None,
        r#"{"tasks":"number[]","mode":"auto|manual"}"#,
        needs_doc,
        task::run_set_mode,
    ),
    cmd!("task.toggleActive", "Toggle Active", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_toggle_active,),
    cmd!("task.clearDates", "Clear Dates", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_clear_dates,),
    cmd!("task.markOnTrack", "Mark on Track", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_mark_on_track,),
    cmd!("task.toggleTimeline", "Add to Timeline", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_toggle_timeline,),
    cmd!("task.rejoinSplits", "Rejoin Split Task", &["Task"], None, r#"{"tasks":"number[]"}"#, needs_doc, task::run_rejoin,),
    // —— Resource ——
    cmd!("resource.insert", "Add Resource", &["Resource"], None, r#"{"name":"string?"}"#, needs_doc, resource::run_insert,),
    cmd!("resource.delete", "Delete Resources", &["Resource"], None, r#"{"resources":"number[]"}"#, needs_doc, resource::run_delete,),
    cmd!(
        "resource.levelAll",
        "Level Resources",
        &["Resource"],
        None,
        r#"{"clear_existing":"bool?","max_delay_days":"number?","resources":"number[]?"}"#,
        needs_doc,
        resource::run_level_all,
    ),
    cmd!("resource.clearLeveling", "Clear Leveling", &["Resource"], None, r#"{}"#, needs_doc, resource::run_clear_leveling,),
    cmd!(
        "resource.assign",
        "Assign Resource",
        &["Resource"],
        None,
        r#"{"task":"number","resource":"number","units":"number?","work_minutes":"number?"}"#,
        needs_doc,
        resource::run_assign,
    ),
    cmd!(
        "resource.unassign",
        "Remove Assignment",
        &["Resource"],
        None,
        r#"{"assignment":"number?","task":"number?","resource":"number?"}"#,
        needs_doc,
        resource::run_unassign,
    ),
    // —— Project ——
    cmd!(
        "project.updateProgressFromTime",
        "Update Progress from Time",
        &["Project"],
        None,
        r#"{}"#,
        needs_doc,
        project::run_update_progress_from_time,
    ),
    cmd!(
        "project.setChartStyle",
        "Set Chart Style",
        &["Project"],
        None,
        r#"{"style":"blue|graphite|green|orange|high_contrast"}"#,
        needs_doc,
        project::run_set_chart_style,
    ),
    // —— Queries ——
    query!("project.inspect", "Inspect Project", r#"{}"#, needs_doc, run_inspect),
    query!("workspace.list", "List Documents", r#"{}"#, always_ok, run_list_docs),
    query!("commands.list", "List Commands", r#"{}"#, always_ok, run_commands_list),
];

#[cfg(test)]
mod tests {
    use plancraft_core::dates::today;
    use plancraft_core::model::Project;
    use plancraft_core::workspace::Workspace;
    use serde_json::json;

    use crate::Session;

    #[test]
    fn task_insert_indent_link_round_trip() {
        let mut session = Session::new(Workspace::default());
        session.add_project(Project::new("T", today()), None);
        let a = session.execute("task.insert", &json!({"name": "A"})).unwrap();
        let b = session.execute("task.insert", &json!({"name": "B"})).unwrap();
        let c = session.execute("task.insert", &json!({"name": "C"})).unwrap();
        let id_a = a["task"].as_u64().unwrap();
        let id_b = b["task"].as_u64().unwrap();
        let id_c = c["task"].as_u64().unwrap();
        session.execute("task.link", &json!({"tasks": [id_a, id_b]})).unwrap();
        session.execute("task.indent", &json!({"tasks": [id_c]})).unwrap();
        let doc = session.active_doc().unwrap();
        assert_eq!(doc.project.tasks.len(), 3);
        assert!(doc.project.tasks[2].outline_level > doc.project.tasks[1].outline_level);
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(session.active_doc().unwrap().project.tasks[2].outline_level, 1);
    }

    #[test]
    fn resource_insert_and_clear_leveling() {
        let mut session = Session::new(Workspace::default());
        session.add_project(Project::new("T", today()), None);
        let r = session.execute("resource.insert", &json!({"name": "Ana"})).unwrap();
        assert!(r["resource"].as_u64().is_some());
        session.execute("resource.clearLeveling", &json!({})).unwrap();
    }

    #[test]
    fn execute_mut_uses_shared_workspace() {
        let mut ws = Workspace::default();
        ws.add(Project::new("T", today()), None);
        Session::execute_mut(&mut ws, plancraft_core::workspace::ChangeSource::User, "task.insert", &json!({"name": "X"})).unwrap();
        assert_eq!(ws.active_doc().unwrap().project.tasks.len(), 1);
    }
}
