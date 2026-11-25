//! Send the open plan to Planner Premium through the mppfill sidecar.
//!
//! PlanCraft writes the Planner-profile XML. mppfill fills the template and
//! imports it. The template and the sidecar are found beside the program or in
//! the usual places on this computer, then the template is kept in the app's
//! own folder. The person does not pick either file. The sign-in token stays
//! in mppfill's cache and is never read here.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use plancraft_core::model::Project;

const TEMPLATE_NAME: &str = "PLANNER-cyclus.mpp";
const STORED_TEMPLATE: &str = "planner-template.mpp";
const MIN_TEMPLATE_BYTES: u64 = 4096;
const MAX_LINE: usize = 2_000;
const MAX_STDERR: usize = 8_000;

/// Why the sender could not start or finish. The dialog turns this into a sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendFail {
    NoTool,
    NoTemplate,
    Write,
    Start,
    /// The sidecar's own message, already short and without a token.
    Rejected(String),
}

/// What the running sidecar is doing, as a language key the dialog knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendStep {
    SignIn,
    FileReady,
    Created,
    Scheduling,
    TasksOpen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendEvent {
    Step(SendStep),
    Project(String),
    Link(String),
    Schedule(String),
    Task(String),
    Warning(String),
    Done(Result<(), SendFail>),
}

/// Where to look. Tests pass a temp tree; the app fills this from the machine.
struct Hints {
    roots: Vec<PathBuf>,
    home: Option<PathBuf>,
    config: Option<PathBuf>,
    extra_templates: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct Resolved {
    program: PathBuf,
    /// Run `python -m mppfill` instead of the `mppfill` program.
    module: bool,
    work_dir: Option<PathBuf>,
    template: PathBuf,
}

/// Start sending `project` under `name`. `org` is used only when it is a Dynamics URL.
pub fn start(project: Project, name: String, org: String) -> Receiver<SendEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || send(project, &name, &org, &tx));
    rx
}

fn send(project: Project, name: &str, org: &str, tx: &Sender<SendEvent>) {
    let resolved = match resolve(&machine_hints()) {
        Ok(resolved) => resolved,
        Err(fail) => {
            let _ = tx.send(SendEvent::Done(Err(fail)));
            return;
        }
    };
    let name = clean_name(name);
    let dir = std::env::temp_dir().join("plancraft-planner");
    if std::fs::create_dir_all(&dir).is_err() {
        let _ = tx.send(SendEvent::Done(Err(SendFail::Write)));
        return;
    }
    let stem = file_stem(&name);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let xml = dir.join(format!("{stem}-{stamp}.xml"));
    let mpp = dir.join(format!("{stem}-{stamp}.mpp"));
    let (text, _notes) = plancraft_core::mspdi::to_xml_for_planner(&project);
    if plancraft_core::io::write_atomic(&xml, text.as_bytes(), plancraft_core::io::Privacy::KeepExisting).is_err() {
        let _ = tx.send(SendEvent::Done(Err(SendFail::Write)));
        return;
    }
    push(&resolved, &xml, &mpp, &name, org, tx);
    let _ = std::fs::remove_file(&xml);
    let _ = std::fs::remove_file(&mpp);
}

fn push(resolved: &Resolved, xml: &Path, mpp: &Path, name: &str, org: &str, tx: &Sender<SendEvent>) {
    let mut command = Command::new(&resolved.program);
    if resolved.module {
        command.arg("-m").arg("mppfill");
    }
    command.arg("push").arg(xml).arg("--out").arg(mpp).arg("--template").arg(&resolved.template).arg("--force").arg("--name").arg(name);
    if let Some(org) = dynamics_org(org) {
        command.arg("--org").arg(org);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    command.env("PYTHONUNBUFFERED", "1");
    command.env("PYTHONIOENCODING", "utf-8");
    if let Some(dir) = &resolved.work_dir {
        command.current_dir(dir);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = tx.send(SendEvent::Done(Err(SendFail::Start)));
            return;
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let err_thread = thread::spawn(move || stderr_tail(stderr));
    if let Some(pipe) = stdout {
        let mut reader = BufReader::new(pipe);
        while let Some(line) = read_line(&mut reader) {
            if !forward_line(&line, tx) {
                while read_line(&mut reader).is_some() {}
                break;
            }
        }
    }
    let status = child.wait();
    let err_text = err_thread.join().unwrap_or_default();
    let ok = status.as_ref().is_ok_and(|s| s.success());
    if ok {
        let _ = tx.send(SendEvent::Done(Ok(())));
        return;
    }
    let _ = tx.send(SendEvent::Done(Err(SendFail::Rejected(sidecar_message(&err_text)))));
}

/// Send one sidecar line to the dialog. Returns false when the dialog is gone.
fn forward_line(line: &str, tx: &Sender<SendEvent>) -> bool {
    let line = line.trim();
    if line.is_empty() || line.len() > MAX_LINE || secret_line(line) {
        return true;
    }
    let event = if line.starts_with("A browser window will open") {
        SendEvent::Step(SendStep::SignIn)
    } else if line.starts_with("Wrote ") {
        SendEvent::Step(SendStep::FileReady)
    } else if let Some(id) = line.strip_prefix("Project ")
        && is_guid(id.trim())
    {
        SendEvent::Project(id.trim().to_string())
    } else if line.starts_with("Opened the Tasks tab") || line.starts_with("Open the link") {
        SendEvent::Step(SendStep::TasksOpen)
    } else if line.starts_with("https://") && line.contains("dynamics.com") && !line.chars().any(char::is_whitespace) {
        SendEvent::Link(line.to_string())
    } else if let Some(rest) = line.strip_prefix("schedule ") {
        SendEvent::Schedule(rest.trim().to_string())
    } else if line.contains(" – ") && line.contains('T') {
        SendEvent::Task(line.to_string())
    } else if let Some(rest) = line.strip_prefix("- ") {
        SendEvent::Warning(rest.trim().chars().take(MAX_LINE).collect())
    } else {
        return true;
    };
    tx.send(event).is_ok()
}

fn stderr_tail(stderr: Option<std::process::ChildStderr>) -> String {
    let Some(mut pipe) = stderr else { return String::new() };
    let mut kept = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if let Some(piece) = chunk.get(..read) {
            kept.extend_from_slice(piece);
        }
        if kept.len() > MAX_STDERR {
            let drop_n = kept.len() - MAX_STDERR;
            kept.drain(0..drop_n);
        }
    }
    String::from_utf8_lossy(&kept).trim().to_string()
}

fn sidecar_message(stderr: &str) -> String {
    let line = stderr.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    let line = line.strip_prefix("mppfill:").unwrap_or(line).trim();
    let text: String = line.chars().filter(|c| !c.is_control()).take(300).collect();
    if text.is_empty() { "Planner did not accept the plan.".into() } else { text }
}

fn secret_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("access_token") || lower.contains("bearer ") || lower.contains("refresh_token")
}

fn machine_hints() -> Hints {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        push_chain(&mut roots, &cwd);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        push_chain(&mut roots, dir);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let mut extra = Vec::new();
    extra.push(PathBuf::from(r"D:\Drenthe").join(TEMPLATE_NAME));
    Hints { roots, home, config: crate::dialogs::misc::config_dir(), extra_templates: extra }
}

fn push_chain(roots: &mut Vec<PathBuf>, start: &Path) {
    let mut current = Some(start);
    for _ in 0..8 {
        let Some(path) = current else { break };
        if !roots.iter().any(|have| have == path) {
            roots.push(path.to_path_buf());
        }
        current = path.parent();
    }
}

fn resolve(hints: &Hints) -> Result<Resolved, SendFail> {
    let (program, module, work_dir) = find_tool(&hints.roots).ok_or(SendFail::NoTool)?;
    let template = find_template(hints).ok_or(SendFail::NoTemplate)?;
    Ok(Resolved { program, module, work_dir, template })
}

fn find_tool(roots: &[PathBuf]) -> Option<(PathBuf, bool, Option<PathBuf>)> {
    for root in roots {
        for base in [root.join("mppfill"), root.clone()] {
            if let Some(found) = venv_tool(&base) {
                return Some(found);
            }
        }
    }
    None
}

/// `(program, run as python -m mppfill, working directory)`.
fn venv_tool(mppfill_dir: &Path) -> Option<(PathBuf, bool, Option<PathBuf>)> {
    let venv = mppfill_dir.join(".venv");
    let windows = venv.join("Scripts");
    let unix = venv.join("bin");
    for scripts in [windows, unix] {
        let exe_name = if scripts.ends_with("Scripts") { "mppfill.exe" } else { "mppfill" };
        let py_name = if scripts.ends_with("Scripts") { "python.exe" } else { "python" };
        let work = Some(mppfill_dir.to_path_buf());
        let exe = scripts.join(exe_name);
        if exe.is_file() {
            return Some((exe, false, work));
        }
        let python = scripts.join(py_name);
        if python.is_file() {
            return Some((python, true, work));
        }
    }
    None
}

fn find_template(hints: &Hints) -> Option<PathBuf> {
    let stored = hints.config.as_ref().map(|dir| dir.join(STORED_TEMPLATE));
    if let Some(path) = stored.as_ref().filter(|path| is_template(path)) {
        return Some(path.clone());
    }
    let mut candidates = Vec::new();
    if let Some(home) = &hints.home {
        candidates.push(home.join("Downloads").join(TEMPLATE_NAME));
    }
    candidates.extend(hints.extra_templates.iter().cloned());
    for root in &hints.roots {
        candidates.push(root.join(TEMPLATE_NAME));
        candidates.push(root.join("mppfill").join(TEMPLATE_NAME));
    }
    let found = candidates.into_iter().find(|path| is_template(path))?;
    if let Some(dest) = stored
        && hints.config.as_ref().is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
        && std::fs::copy(&found, &dest).is_ok()
        && is_template(&dest)
    {
        return Some(dest);
    }
    Some(found)
}

fn is_template(path: &Path) -> bool {
    path.is_file() && path.metadata().is_ok_and(|info| info.len() >= MIN_TEMPLATE_BYTES)
}

/// A project name safe to hand to the sidecar as one argument.
pub fn clean_name(name: &str) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for c in name.chars() {
        if c.is_control() {
            continue;
        }
        out.push(c);
        count += 1;
        if count >= 200 {
            break;
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() { "Plan".to_string() } else { out }
}

fn file_stem(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c.is_whitespace() && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "plan".to_string() } else { out }
}

/// The Dynamics address when `org` is one. Anything else leaves the sidecar's own default.
fn dynamics_org(org: &str) -> Option<String> {
    let org = org.trim().trim_end_matches('/').to_string();
    if org.starts_with("https://")
        && org.ends_with(".dynamics.com")
        && !org.chars().any(|c| c.is_whitespace() || c.is_control())
        && org.len() <= 300
    {
        Some(org)
    } else {
        None
    }
}

fn is_guid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let widths = [8usize, 4, 4, 4, 12];
    parts.len() == widths.len()
        && parts.iter().zip(widths).all(|(part, width)| part.len() == width && part.bytes().all(|c| c.is_ascii_hexdigit()))
}

fn read_line(reader: &mut impl BufRead) -> Option<String> {
    let mut buf = Vec::new();
    match reader.read_until(b'\n', &mut buf) {
        Ok(0) => None,
        Ok(_) => {
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            Some(String::from_utf8_lossy(&buf).to_string())
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_loses_control_characters_and_stays_short() {
        let name = clean_name("  Plan\nCyclus\u{0007}  ");
        assert_eq!(name, "PlanCyclus");
        assert_eq!(clean_name("   "), "Plan");
        assert!(clean_name(&"a".repeat(500)).chars().count() <= 200);
    }

    #[test]
    fn only_a_dynamics_address_is_passed_on() {
        assert_eq!(dynamics_org(" https://orgc1fde974.crm4.dynamics.com/ "), Some("https://orgc1fde974.crm4.dynamics.com".into()));
        assert_eq!(dynamics_org(""), None);
        assert_eq!(dynamics_org("https://example.com"), None);
    }

    #[test]
    fn sidecar_lines_become_steps_and_a_token_line_is_dropped() {
        let (tx, rx) = mpsc::channel();
        assert!(forward_line("A browser window will open so you can sign in.", &tx));
        assert!(forward_line("Project c2763bc7-6a81-4200-a50c-f4e170a051ef", &tx));
        assert!(forward_line(
            "https://orgc1fde974.crm4.dynamics.com/main.aspx?appid=d5cbeb07-c54e-ed11-bba1-000d3abda444&pagetype=entityrecord&etn=msdyn_project&id=c2763bc7-6a81-4200-a50c-f4e170a051ef",
            &tx,
        ));
        assert!(forward_line("schedule 2026-10-15T08:00:00Z – 2027-12-15T17:00:00Z", &tx));
        assert!(forward_line("Afsluiting van de administratie: 2027-01-04T09:00:00Z – 2027-01-13T17:00:00Z", &tx));
        assert!(forward_line("- W_IMPORT_RESOURCE_NOTSUPPORTED: 79", &tx));
        assert!(forward_line("access_token=secret", &tx));
        drop(tx);
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            events,
            vec![
                SendEvent::Step(SendStep::SignIn),
                SendEvent::Project("c2763bc7-6a81-4200-a50c-f4e170a051ef".into()),
                SendEvent::Link(
                    "https://orgc1fde974.crm4.dynamics.com/main.aspx?appid=d5cbeb07-c54e-ed11-bba1-000d3abda444&pagetype=entityrecord&etn=msdyn_project&id=c2763bc7-6a81-4200-a50c-f4e170a051ef"
                        .into(),
                ),
                SendEvent::Schedule("2026-10-15T08:00:00Z – 2027-12-15T17:00:00Z".into()),
                SendEvent::Task("Afsluiting van de administratie: 2027-01-04T09:00:00Z – 2027-01-13T17:00:00Z".into()),
                SendEvent::Warning("W_IMPORT_RESOURCE_NOTSUPPORTED: 79".into()),
            ]
        );
    }

    #[test]
    fn the_tool_and_template_are_found_and_the_template_is_kept() {
        let root = std::env::temp_dir().join(format!("plancraft-send-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let scripts = root.join("mppfill").join(".venv").join("Scripts");
        let config = root.join("config");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(scripts.join("python.exe"), b"python").unwrap();
        let downloads = root.join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(downloads.join(TEMPLATE_NAME), vec![0u8; MIN_TEMPLATE_BYTES as usize]).unwrap();
        let hints =
            Hints { roots: vec![root.clone()], home: Some(root.clone()), config: Some(config.clone()), extra_templates: Vec::new() };
        let resolved = resolve(&hints).expect("found");
        assert!(resolved.module);
        assert_eq!(resolved.program, scripts.join("python.exe"));
        assert_eq!(resolved.template, config.join(STORED_TEMPLATE));
        assert!(is_template(&resolved.template));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_sidecar_is_reported_on_its_own() {
        let root = std::env::temp_dir().join(format!("plancraft-send-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let err = resolve(&Hints { roots: vec![root.clone()], home: None, config: None, extra_templates: Vec::new() });
        assert_eq!(err.err(), Some(SendFail::NoTool));
        let _ = std::fs::remove_dir_all(&root);
    }
}
