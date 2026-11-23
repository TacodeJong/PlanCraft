//! Import a Swift Project Planner document (camelCase JSON) into a PlanCraft project.
//!
//! Task ids are new. The file on disk is only read. Scheduling uses PlanCraft's own engine,
//! with the group ruleset turned on and both date bounds kept.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::dates::{self, prelude::*};
use crate::duration::{Lag, Minutes};
use crate::ids::{ResourceId, TaskId};
use crate::io::{FileError, FileResult};
use crate::limits::{self, MAX_FILE_BYTES, MAX_MINUTES, MAX_NAME_CHARS, MAX_NOTES_CHARS, MAX_OUTLINE_LEVEL, MAX_UNITS};
use crate::model::*;

const ID_NOTE: &str = "Imported task ids are new. The original Project Planner file was not overwritten.";

fn one() -> f64 {
    1.0
}
fn yes() -> bool {
    true
}
fn hundred() -> f64 {
    100.0
}
fn eight() -> f64 {
    8.0
}
fn forty() -> f64 {
    40.0
}
fn prio() -> i64 {
    500
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    title: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default = "eight", rename = "hoursPerDay")]
    hours_per_day: f64,
    #[serde(default = "forty", rename = "hoursPerWeek")]
    hours_per_week: f64,
    #[serde(default)]
    tasks: Vec<RawTask>,
    #[serde(default)]
    resources: BTreeMap<String, RawResource>,
    #[serde(default)]
    holidays: Vec<RawHoliday>,
}

#[derive(Deserialize)]
struct RawTask {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    depth: i64,
    #[serde(default = "one")]
    duration: f64,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    progress: f64,
    #[serde(default = "prio")]
    priority: i64,
    #[serde(default = "hundred")]
    units: f64,
    #[serde(default)]
    resource: String,
    #[serde(default)]
    milestone: bool,
    #[serde(default, rename = "milestoneType")]
    milestone_type: String,
    #[serde(default)]
    deliverable: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    mark: String,
    #[serde(default, rename = "markNote")]
    mark_note: String,
    #[serde(default, rename = "durationPercent")]
    duration_percent: Option<f64>,
    #[serde(default = "yes", rename = "fillsGroup")]
    fills_group: bool,
    #[serde(default = "yes", rename = "hasEarliestStart")]
    has_earliest_start: bool,
    #[serde(default)]
    start: f64,
    #[serde(default, rename = "latestStart")]
    latest_start: Option<f64>,
    #[serde(default, rename = "earliestFinish")]
    earliest_finish: Option<f64>,
    #[serde(default, rename = "latestFinish")]
    latest_finish: Option<f64>,
    #[serde(default, rename = "startFixed")]
    start_fixed: bool,
    #[serde(default, rename = "endFixed")]
    end_fixed: bool,
    #[serde(default, rename = "givenAlignment")]
    given_alignment: String,
    #[serde(default)]
    dependencies: Vec<RawDep>,
}

#[derive(Deserialize)]
struct RawDep {
    #[serde(default, rename = "predecessorID")]
    predecessor_id: String,
    #[serde(default, rename = "type")]
    kind: i64,
    #[serde(default)]
    lag: f64,
}

#[derive(Deserialize)]
struct RawResource {
    #[serde(default = "hundred")]
    available: f64,
    #[serde(default)]
    email: String,
    #[serde(default)]
    initials: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    kind: String,
    #[serde(default, rename = "contactName")]
    contact_name: String,
    #[serde(default, rename = "contactEmail")]
    contact_email: String,
}

#[derive(Deserialize)]
struct RawHoliday {
    #[serde(default)]
    date: String,
    #[serde(default)]
    name: String,
}

/// True when `text` is a Project Planner task list, not a PlanCraft document.
pub fn looks_like(text: &str) -> bool {
    let text = text.trim_start_matches('\u{feff}').trim();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    if value.get("format").and_then(|v| v.as_str()) == Some(FILE_FORMAT) {
        return false;
    }
    value
        .get("tasks")
        .and_then(|v| v.as_array())
        .and_then(|tasks| tasks.first())
        .and_then(|task| task.as_object())
        .is_some_and(|task| task.contains_key("title") && task.contains_key("depth"))
}

/// True when `path` is a `.json` file whose contents look like a Project Planner document.
pub fn is_planner_path(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !ext.eq_ignore_ascii_case("json") {
        return false;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() > MAX_FILE_BYTES {
        return false;
    }
    std::fs::read_to_string(path).is_ok_and(|text| looks_like(&text))
}

/// Reads a Project Planner JSON file. Does not recalculate and does not write the file.
pub fn load(path: &Path) -> FileResult<(Project, Vec<String>)> {
    let file = crate::io::open_regular(path, false, crate::io::Links::Follow)
        .map_err(|source| FileError::Io { path: path.to_path_buf(), source })?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    if size > MAX_FILE_BYTES {
        return Err(FileError::TooLarge { path: path.to_path_buf(), size });
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_string(&mut text)
        .map_err(|source| FileError::Io { path: path.to_path_buf(), source })?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(FileError::TooLarge { path: path.to_path_buf(), size: text.len() as u64 });
    }
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported plan".to_string());
    import_text(&text, &name).map_err(|message| FileError::Import { path: path.to_path_buf(), message })
}

/// Builds a project from Project Planner JSON. Does not recalculate.
pub fn import_text(text: &str, name: &str) -> Result<(Project, Vec<String>), String> {
    let text = text.trim_start_matches('\u{feff}');
    let raw: RawFile = serde_json::from_str(text).map_err(|e| format!("not a Project Planner document ({e})"))?;
    if raw.format.as_deref() == Some(FILE_FORMAT) {
        return Err("this is a PlanCraft document, not a Project Planner file".into());
    }
    if raw.tasks.len() > limits::MAX_TASKS {
        return Err(format!("a project can have at most {} tasks", limits::MAX_TASKS));
    }
    if raw.resources.len() > limits::MAX_RESOURCES {
        return Err(format!("a project can have at most {} resources", limits::MAX_RESOURCES));
    }
    let title = if raw.title.trim().is_empty() { name } else { raw.title.trim() };
    let start_day = dates::ymd(2025, 12, 1).ok_or_else(|| "the import calendar origin is not a valid date".to_string())?;
    let mut project = Project::new(clip(title, MAX_NAME_CHARS), start_day);
    let minutes_per_day = hours_to_minutes(raw.hours_per_day, 8 * 60);
    let minutes_per_week = hours_to_minutes(raw.hours_per_week, 40 * 60);
    project.settings.time.minutes_per_day = minutes_per_day;
    project.settings.time.minutes_per_week = minutes_per_week;
    project.settings.rules = ScheduleRules::on();

    let mut notes = vec![ID_NOTE.to_string()];
    import_holidays(&mut project, &raw.holidays, &mut notes);
    let resources = import_resources(&mut project, &raw.resources);
    let summary = summary_flags(&raw.tasks);
    let mut uuids: BTreeMap<String, TaskId> = BTreeMap::new();
    let mut dropped_links = false;
    let mut prev_level: u16 = 0;

    for (i, task) in raw.tasks.iter().enumerate() {
        let is_summary = summary.get(i).copied().unwrap_or(false);
        let requested = u16::try_from(task.depth.max(0).saturating_add(1)).unwrap_or(MAX_OUTLINE_LEVEL);
        let level = requested.clamp(1, MAX_OUTLINE_LEVEL).min(prev_level.saturating_add(1).max(1));
        prev_level = level;
        let id = project.add_task(clip(task_name(task), MAX_NAME_CHARS), None);
        let key = task.id.trim().to_ascii_lowercase();
        if !key.is_empty() {
            uuids.entry(key).or_insert(id);
        }
        if let Some(row) = project.task_mut(id) {
            fill_task(row, task, level, is_summary, minutes_per_day, raw.hours_per_day);
        }
        if !is_summary {
            assign_resources(&mut project, id, task, &resources, &mut dropped_links);
        }
    }

    for task in &raw.tasks {
        let succ_key = task.id.trim().to_ascii_lowercase();
        let Some(succ) = uuids.get(&succ_key).copied() else { continue };
        for dep in &task.dependencies {
            let pred_key = dep.predecessor_id.trim().to_ascii_lowercase();
            let Some(pred) = uuids.get(&pred_key).copied() else {
                dropped_links = true;
                continue;
            };
            let lag = lag_of(dep.lag, minutes_per_day);
            if project.link(pred.into(), succ, link_kind(dep.kind), lag).is_err() {
                dropped_links = true;
            }
        }
    }
    if dropped_links {
        notes.push("Some links were left out (unknown predecessor, a cycle, or a link a summary cannot have).".into());
    }
    let _ = limits::sanitize(&mut project);
    Ok((project, notes))
}

fn summary_flags(tasks: &[RawTask]) -> Vec<bool> {
    tasks.iter().enumerate().map(|(i, task)| tasks.get(i.saturating_add(1)).is_some_and(|next| next.depth > task.depth)).collect()
}

fn fill_task(row: &mut Task, task: &RawTask, level: u16, is_summary: bool, minutes_per_day: Minutes, hours_per_day: f64) {
    row.outline_level = level;
    row.notes = clip(&task.notes, MAX_NOTES_CHARS);
    row.priority = u16::try_from(task.priority.clamp(0, i64::from(limits::MAX_PRIORITY))).unwrap_or(limits::MAX_PRIORITY);
    row.percent_complete = percent_complete(task.progress);
    row.estimated = false;
    row.task_type = TaskType::FixedDuration;
    row.fills_group = task.fills_group;
    if let Some(pct) = task.duration_percent
        && pct.is_finite()
    {
        row.duration_percent = Some(pct.clamp(0.0, 1000.0));
    }
    if let Some(color) = parse_hex(&task.color) {
        row.bar_color = Some(color);
    }
    if let Some(mark) = RowMark::parse(&task.mark) {
        row.row_mark = Some(mark);
        row.mark_note = clip(&task.mark_note, MAX_NOTES_CHARS);
    }
    let deliverable = if task.deliverable.trim().is_empty() { task.milestone_type.trim() } else { task.deliverable.trim() };
    row.deliverable = clip(deliverable, MAX_NAME_CHARS);
    apply_dates(row, task, is_summary, minutes_per_day, hours_per_day);
}

fn apply_dates(row: &mut Task, task: &RawTask, is_summary: bool, minutes_per_day: Minutes, hours_per_day: f64) {
    let start_on = task.has_earliest_start.then_some(task.start);
    let latest_start = finite(task.latest_start);
    let earliest_finish = finite(task.earliest_finish);
    let latest_finish = finite(task.latest_finish);
    let start_is_on = match (start_on, latest_start) {
        (Some(a), Some(b)) => (a - b).abs() <= 1e-6,
        _ => false,
    };
    let finish_is_on = match (earliest_finish, latest_finish) {
        (Some(a), Some(b)) => (a - b).abs() <= 1e-6,
        _ => false,
    };
    if task.has_earliest_start {
        row.bounds.start_earliest = clock(task.start, false, hours_per_day);
    }
    if let Some(day) = latest_start {
        row.bounds.start_latest = clock(day, false, hours_per_day);
        row.bounds.start_latest_fixed = task.start_fixed;
    }
    if let Some(day) = earliest_finish {
        row.bounds.finish_earliest = clock(day, true, hours_per_day);
    }
    if let Some(day) = latest_finish {
        row.bounds.finish_latest = clock(day, true, hours_per_day);
        row.bounds.finish_latest_fixed = task.end_fixed;
    }

    let both_fixed_on = start_is_on && finish_is_on && task.start_fixed && task.end_fixed && !task.milestone && !is_summary;
    if both_fixed_on {
        let days = latest_finish.unwrap_or(task.start) - task.start;
        row.duration = day_minutes(days, minutes_per_day);
        if let Some(date) = row.bounds.start_earliest {
            row.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(date) };
        }
        row.deadline = row.bounds.finish_latest;
        return;
    }
    row.duration = if task.milestone || is_summary { 0 } else { day_minutes(task.duration, minutes_per_day) };
    if task.milestone {
        row.kind = TaskKind::Milestone;
        row.duration = 0;
    }
    if task.milestone && finish_is_on && task.end_fixed {
        if let Some(date) = row.bounds.finish_latest.or(row.bounds.finish_earliest) {
            row.constraint = Constraint { kind: ConstraintKind::Mfo, date: Some(date) };
        }
        return;
    }
    if start_is_on && task.start_fixed {
        if let Some(date) = row.bounds.start_earliest.or(row.bounds.start_latest) {
            row.constraint = Constraint { kind: ConstraintKind::Mso, date: Some(date) };
        }
    } else if task.has_earliest_start {
        if let Some(date) = row.bounds.start_earliest {
            row.constraint = Constraint { kind: ConstraintKind::Snet, date: Some(date) };
        }
    } else if task.start_fixed {
        if let Some(date) = row.bounds.start_latest {
            row.constraint = Constraint { kind: ConstraintKind::Snlt, date: Some(date) };
        }
    } else if task.given_alignment.trim().eq_ignore_ascii_case("alap") {
        row.constraint = Constraint { kind: ConstraintKind::Alap, date: None };
    }
    if latest_finish.is_some() && !task.end_fixed && row.constraint.kind != ConstraintKind::Mfo {
        row.deadline = row.bounds.finish_latest;
    }
}

fn assign_resources(project: &mut Project, id: TaskId, task: &RawTask, resources: &BTreeMap<String, ResourceId>, dropped: &mut bool) {
    let units = (task.units / 100.0).clamp(0.0, MAX_UNITS);
    for name in task.resource.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(rid) = resources.get(name) else { continue };
        if project.assign(id, *rid, Some(units), None).is_err() {
            *dropped = true;
        }
    }
}

fn import_resources(project: &mut Project, resources: &BTreeMap<String, RawResource>) -> BTreeMap<String, ResourceId> {
    let mut ids = BTreeMap::new();
    for (name, info) in resources {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        let id = project.add_resource(clip(trimmed, MAX_NAME_CHARS));
        if let Some(row) = project.resource_mut(id) {
            let (kind, category) = resource_kind(&info.kind);
            row.kind = kind;
            row.category = category;
            row.email = clip(&info.email, MAX_NAME_CHARS);
            row.group = clip(&info.role, MAX_NAME_CHARS);
            if !info.initials.trim().is_empty() {
                row.initials = clip(info.initials.trim(), MAX_NAME_CHARS);
            }
            if info.available.is_finite() {
                row.max_units = (info.available / 100.0).clamp(0.0, MAX_UNITS);
            }
            row.notes = resource_notes(info);
        }
        ids.insert(trimmed.to_string(), id);
    }
    ids
}

fn resource_notes(info: &RawResource) -> String {
    let mut notes = String::new();
    if !info.phone.trim().is_empty() {
        notes.push_str("Phone: ");
        notes.push_str(info.phone.trim());
    }
    if !info.contact_name.trim().is_empty() || !info.contact_email.trim().is_empty() {
        if !notes.is_empty() {
            notes.push('\n');
        }
        notes.push_str("Contact: ");
        notes.push_str(info.contact_name.trim());
        if !info.contact_email.trim().is_empty() {
            if !info.contact_name.trim().is_empty() {
                notes.push(' ');
            }
            notes.push_str(info.contact_email.trim());
        }
    }
    clip(&notes, MAX_NOTES_CHARS)
}

fn import_holidays(project: &mut Project, holidays: &[RawHoliday], notes: &mut Vec<String>) {
    let cal_id = project.calendar;
    let Some(cal) = project.calendars.iter_mut().find(|c| c.id == cal_id) else { return };
    for holiday in holidays {
        if cal.exceptions.len() >= limits::MAX_LIST {
            notes.push("Some holidays were left out because the calendar is full.".into());
            break;
        }
        let Some(date) = parse_iso_date(&holiday.date) else { continue };
        let name = if holiday.name.trim().is_empty() { "Holiday" } else { holiday.name.trim() };
        cal.exceptions.push(crate::calendar::CalendarException::non_working(clip(name, MAX_NAME_CHARS), date, date));
    }
}

fn resource_kind(kind: &str) -> (ResourceType, ResourceCategory) {
    match kind.trim().to_ascii_lowercase().as_str() {
        "team" => (ResourceType::Work, ResourceCategory::Team),
        "equipment" => (ResourceType::Work, ResourceCategory::Equipment),
        "company" => (ResourceType::Work, ResourceCategory::PartnerCompany),
        "material" => (ResourceType::Material, ResourceCategory::Other),
        _ => (ResourceType::Work, ResourceCategory::TeamMember),
    }
}

fn link_kind(kind: i64) -> LinkType {
    match kind {
        1 => LinkType::SS,
        2 => LinkType::FF,
        3 => LinkType::SF,
        _ => LinkType::FS,
    }
}

fn lag_of(days: f64, minutes_per_day: Minutes) -> Lag {
    let minutes = day_minutes(days, minutes_per_day);
    if minutes == 0 { Lag::None } else { Lag::Working(minutes) }
}

fn percent_complete(progress: f64) -> u8 {
    if !progress.is_finite() {
        return 0;
    }
    let pct = progress.clamp(0.0, 100.0).round();
    u8::try_from(pct as i64).unwrap_or(0).min(100)
}

fn day_minutes(days: f64, minutes_per_day: Minutes) -> Minutes {
    if !days.is_finite() || days <= 0.0 || minutes_per_day <= 0 {
        return 0;
    }
    let minutes = days * minutes_per_day as f64;
    if !minutes.is_finite() {
        return MAX_MINUTES;
    }
    limits::clamp_minutes(minutes.round() as Minutes)
}

fn hours_to_minutes(hours: f64, fallback: Minutes) -> Minutes {
    if !hours.is_finite() || hours <= 0.0 {
        return fallback;
    }
    let minutes = (hours * 60.0).round();
    if !minutes.is_finite() {
        return fallback;
    }
    let minutes = limits::clamp_minutes(minutes as Minutes);
    if minutes <= 0 { fallback } else { minutes }
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|n| n.is_finite())
}

fn task_name(task: &RawTask) -> &str {
    if task.title.trim().is_empty() { "Untitled" } else { task.title.trim() }
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => text.get(..i).unwrap_or(text).to_string(),
        None => text.to_string(),
    }
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let text = text.trim().trim_start_matches('#');
    if text.chars().count() != 6 {
        return None;
    }
    let mut chars = text.chars();
    let mut byte = || {
        let hi = chars.next()?.to_digit(16)?;
        let lo = chars.next()?.to_digit(16)?;
        u8::try_from(hi * 16 + lo).ok()
    };
    Some([byte()?, byte()?, byte()?])
}

fn parse_iso_date(text: &str) -> Option<Date> {
    let mut parts = text.trim().split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    dates::ymd(y, m, d)
}

/// Workday 0 is Monday 1 December 2025 at 08:00. Weekends have no slot.
pub(crate) fn clock(workday: f64, finish: bool, hours_per_day: f64) -> Option<DateTime> {
    let whole = floor_i64(workday)?;
    let weeks = whole.div_euclid(5);
    let day_in_week = whole.saturating_sub(weeks.saturating_mul(5));
    let calendar_days = weeks.saturating_mul(7).saturating_add(day_in_week);
    let origin = dates::ymd(2025, 12, 1)?;
    let mut day = dates::add_days(origin, calendar_days)?;
    let hours = if hours_per_day.is_finite() && hours_per_day > 0.0 { hours_per_day } else { 8.0 };
    let frac = workday - whole as f64;
    let mut seconds = (frac * hours * 3600.0).round();
    if !seconds.is_finite() {
        seconds = 0.0;
    }
    let mut seconds = seconds as i64;
    let full = (hours * 3600.0).round() as i64;
    if finish && seconds == 0 {
        day = dates::add_days(day, -1)?;
        seconds = full;
    }
    if (hours - 8.0).abs() < 1e-6 && (seconds > 4 * 3600 || (seconds == 4 * 3600 && !finish)) {
        seconds = seconds.saturating_add(3600);
    }
    let minute_of_day = (8_i64 * 60).saturating_add(seconds.div_euclid(60));
    let extra_days = minute_of_day.div_euclid(24 * 60);
    let minute = minute_of_day.rem_euclid(24 * 60);
    if extra_days != 0 {
        day = dates::add_days(day, extra_days)?;
    }
    let hour = u32::try_from(minute / 60).ok()?;
    let min = u32::try_from(minute % 60).ok()?;
    dates::ymd_hm(day.year32(), day.month32(), day.day32(), hour, min)
}

fn floor_i64(v: f64) -> Option<i64> {
    if !v.is_finite() {
        return None;
    }
    let truncated = v.trunc();
    let floored = if v < truncated { truncated - 1.0 } else { truncated };
    if floored < i64::MIN as f64 || floored > i64::MAX as f64 {
        return None;
    }
    Some(floored as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::recalculate;

    #[test]
    fn looks_like_rejects_plancraft_and_accepts_planner_json() {
        let plan = Project::new("Own", dates::ymd(2026, 1, 5).unwrap());
        let own = crate::io::to_json(&plan).unwrap();
        assert!(!looks_like(&own), "a PlanCraft document is not a Project Planner file");
        assert!(looks_like(r#"{"title":"P","tasks":[{"title":"A","depth":0}]}"#));
        assert!(!looks_like(r#"{"tasks":[]}"#));
    }

    #[test]
    fn workday_zero_and_five_are_the_december_mondays() {
        let json = r#"{
            "title": "Days",
            "tasks": [
                {"title": "Zero", "depth": 0, "duration": 1, "hasEarliestStart": true, "start": 0, "id": "a"},
                {"title": "Five", "depth": 0, "duration": 1, "hasEarliestStart": true, "start": 5, "id": "b"}
            ]
        }"#;
        let (p, notes) = import_text(json, "Days").unwrap();
        assert!(notes.iter().any(|n| n.contains("not overwritten")));
        assert!(p.settings.rules.enabled());
        let zero = p.tasks.iter().find(|t| t.name == "Zero").unwrap().bounds.start_earliest.unwrap();
        let five = p.tasks.iter().find(|t| t.name == "Five").unwrap().bounds.start_earliest.unwrap();
        assert_eq!(zero, dates::ymd_hm(2025, 12, 1, 8, 0).unwrap());
        assert_eq!(five, dates::ymd_hm(2025, 12, 8, 8, 0).unwrap());
    }

    #[test]
    fn a_zero_duration_task_is_a_milestone_only_when_the_flag_says_so() {
        // PLANNER-cyclus.json stores "Sjablonen teamjaarplan aanleveren" with duration 0 and
        // milestone false. The Swift XML marks that same name as a milestone. The flag is the
        // source of truth: duration 0 on its own is not a milestone (other zero-duration leaves
        // in that file stay ordinary tasks).
        let json = r#"{
            "title": "Flags",
            "tasks": [
                {"title": "Sjabloon", "depth": 0, "duration": 0, "milestone": false, "id": "a"},
                {"title": "Besluit", "depth": 0, "duration": 1, "milestone": true, "milestoneType": "Besluit", "id": "b"}
            ]
        }"#;
        let (p, _) = import_text(json, "Flags").unwrap();
        let plain = p.tasks.iter().find(|t| t.name == "Sjabloon").unwrap();
        assert!(!plain.is_milestone());
        assert_eq!(plain.duration, 0);
        let marked = p.tasks.iter().find(|t| t.name == "Besluit").unwrap();
        assert!(marked.is_milestone());
        assert_eq!(marked.duration, 0);
    }

    #[test]
    fn a_finish_to_start_successor_is_not_scheduled_before_its_predecessor() {
        let json = r#"{
            "title": "Chain",
            "tasks": [
                {"title": "First", "depth": 0, "duration": 1, "hasEarliestStart": true, "start": 0, "id": "a"},
                {"title": "Second", "depth": 0, "duration": 1, "hasEarliestStart": false, "id": "b",
                 "dependencies": [{"predecessorID": "a", "type": 0}]}
            ]
        }"#;
        let (mut p, _) = import_text(json, "Chain").unwrap();
        recalculate(&mut p);
        let first = p.tasks.iter().find(|t| t.name == "First").unwrap().sched.finish.unwrap();
        let second = p.tasks.iter().find(|t| t.name == "Second").unwrap().sched.start.unwrap();
        assert!(second >= first, "second starts {second}, first finishes {first}");
    }
}
