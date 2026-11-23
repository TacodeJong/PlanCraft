//! Quick Entry sheet: one line per task, shorthand for indent, duration, milestone and links.
//!
//! The outline's Enter / Tab naming is a different flow and is not handled here.

use plancraft_core::duration::{self, DurationUnit, TimeSettings};
use plancraft_core::limits::MAX_OUTLINE_LEVEL;
use plancraft_core::prelude::*;

/// Why a line cannot be inserted. The line stays in the preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Issue {
    NeedsName,
    BadToken,
    SkippedLevel,
    GroupMilestone,
    GroupResource,
    Unavailable,
    Loop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Above,
    Number(usize),
    Wbs(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkMark {
    pub kind: LinkKind,
    pub ok: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ticket {
    pub name: String,
    pub level: u16,
    pub minutes: Option<i64>,
    pub estimated: bool,
    pub unit: DurationUnit,
    pub duration_label: String,
    pub milestone: bool,
    pub group: bool,
    pub resources: Vec<String>,
    pub links: Vec<LinkMark>,
    /// Sheet indexes of the predecessors that can be linked.
    pub preds: Vec<usize>,
    pub issue: Option<Issue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub tickets: Vec<Ticket>,
    pub looped: bool,
}

struct Draft {
    name: String,
    raw: u16,
    minutes: Option<i64>,
    estimated: bool,
    unit: DurationUnit,
    duration_label: String,
    milestone: bool,
    resources: Vec<String>,
    link_kinds: Vec<LinkKind>,
    issue: Option<Issue>,
}

/// Parses the sheet. Blank lines are separators. Every other line is kept, and a bad one is flagged.
pub fn parse(text: &str, settings: &TimeSettings) -> Sheet {
    let mut drafts = Vec::new();
    for line in text.lines() {
        if let Some(draft) = draft_line(line, settings) {
            drafts.push(draft);
        }
    }
    let min = drafts.iter().map(|d| d.raw).min().unwrap_or(0);
    let mut prev = 0u16;
    let mut levels = Vec::new();
    for draft in &mut drafts {
        if draft.raw > prev.saturating_add(1) {
            note(draft, Issue::SkippedLevel);
        }
        prev = draft.raw;
        let level = draft.raw.saturating_sub(min).saturating_add(1).min(MAX_OUTLINE_LEVEL);
        levels.push(level);
    }
    let wbs = wbs_numbers(&levels);
    let mut tickets = Vec::new();
    for (i, draft) in drafts.iter().enumerate() {
        let level = levels.get(i).copied().unwrap_or(1);
        let group = levels.get(i.saturating_add(1)).is_some_and(|next| *next > level);
        let mut issue = draft.issue;
        if group && draft.milestone {
            issue = issue.or(Some(Issue::GroupMilestone));
        }
        if group && !draft.resources.is_empty() {
            issue = issue.or(Some(Issue::GroupResource));
        }
        let (minutes, label, estimated, unit) = if group || draft.milestone {
            (None, String::new(), false, DurationUnit::Days)
        } else if let Some(mins) = draft.minutes {
            (Some(mins), draft.duration_label.clone(), draft.estimated, draft.unit)
        } else {
            (Some(settings.minutes_per_day.max(1)), "1d".to_string(), false, DurationUnit::Days)
        };
        tickets.push(Ticket {
            name: draft.name.clone(),
            level,
            minutes,
            estimated,
            unit,
            duration_label: label,
            milestone: draft.milestone && !group,
            group,
            resources: draft.resources.clone(),
            links: Vec::new(),
            preds: Vec::new(),
            issue,
        });
    }
    let mut looped = false;
    let mut succs = vec![Vec::new(); tickets.len()];
    for (i, draft) in drafts.iter().enumerate() {
        let Some(ticket) = tickets.get_mut(i) else { continue };
        for kind in &draft.link_kinds {
            let target = match kind {
                LinkKind::Above => i.checked_sub(1),
                LinkKind::Number(n) => resolve_number(*n, &wbs),
                LinkKind::Wbs(code) => wbs.iter().position(|w| w == code),
            };
            let (ok, cycled) = match target {
                Some(pred) if outline_related(&levels, pred, i) => (false, false),
                Some(pred) if reaches(&succs, i, pred) => (false, true),
                Some(_) => (true, false),
                None => (false, false),
            };
            if cycled {
                looped = true;
            }
            if ok {
                if let Some(pred) = target {
                    if let Some(list) = succs.get_mut(pred) {
                        list.push(i);
                    }
                    if !ticket.preds.contains(&pred) {
                        ticket.preds.push(pred);
                    }
                }
            } else if ticket.issue.is_none() {
                ticket.issue = Some(if cycled { Issue::Loop } else { Issue::Unavailable });
            }
            ticket.links.push(LinkMark { kind: kind.clone(), ok });
        }
    }
    Sheet { tickets, looped }
}

fn note(draft: &mut Draft, issue: Issue) {
    if draft.issue.is_none() {
        draft.issue = Some(issue);
    }
}

fn draft_line(line: &str, settings: &TimeSettings) -> Option<Draft> {
    if line.trim().is_empty() {
        return None;
    }
    let (raw, rest) = split_indent(line);
    let mut name_words = Vec::new();
    let mut minutes = None;
    let mut estimated = false;
    let mut unit = DurationUnit::Days;
    let mut duration_label = String::new();
    let mut milestone = false;
    let mut resources = Vec::new();
    let mut link_kinds = Vec::new();
    let mut issue = None;
    for word in rest.split_whitespace() {
        if word == "!" {
            milestone = true;
            continue;
        }
        if let Some(spec) = word.strip_prefix('^') {
            if spec.is_empty() {
                link_kinds.push(LinkKind::Above);
            } else if let Some(kind) = link_kind(spec) {
                link_kinds.push(kind);
            } else if issue.is_none() {
                issue = Some(Issue::BadToken);
            }
            continue;
        }
        if let Some(spec) = word.strip_prefix('@') {
            let added = push_resources(&mut resources, spec);
            if !added && issue.is_none() {
                issue = Some(Issue::BadToken);
            }
            continue;
        }
        if let Some((mins, est, u, label)) = duration_word(word, settings) {
            minutes = Some(mins);
            estimated = est;
            unit = u;
            duration_label = label;
            continue;
        }
        name_words.push(word);
    }
    let name = name_words.join(" ");
    if name.is_empty() && issue.is_none() {
        issue = Some(Issue::NeedsName);
    }
    Some(Draft { name, raw, minutes, estimated, unit, duration_label, milestone, resources, link_kinds, issue })
}

fn push_resources(out: &mut Vec<String>, spec: &str) -> bool {
    let before = out.len();
    for part in spec.split([',', ';']) {
        let name = part.trim();
        if !name.is_empty() {
            out.push(name.to_string());
        }
    }
    out.len() > before
}

fn link_kind(spec: &str) -> Option<LinkKind> {
    let chars: Vec<char> = spec.chars().collect();
    let digits = chars.iter().any(|c| c.is_ascii_digit());
    let symbols = chars.iter().all(|c| c.is_ascii_digit() || *c == '.');
    let ends = chars.first().is_some_and(|c| *c != '.') && chars.last().is_some_and(|c| *c != '.');
    if !digits || !symbols || !ends || spec.contains("..") {
        return None;
    }
    if spec.contains('.') { Some(LinkKind::Wbs(spec.to_string())) } else { spec.parse().ok().map(LinkKind::Number) }
}

fn duration_word(word: &str, settings: &TimeSettings) -> Option<(i64, bool, DurationUnit, String)> {
    if !word.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let (mins, est, unit) = duration::parse_duration(word, DurationUnit::Days, settings)?;
    if mins <= 0 {
        return None;
    }
    let label = word.trim().trim_end_matches('?').trim().to_string();
    Some((mins, est, unit, label))
}

/// Leading tabs, each two spaces, a run of dots, and `>` each add one indent level.
fn split_indent(line: &str) -> (u16, &str) {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut n = 0usize;
    let mut level = 0u16;
    while let Some((_, c)) = chars.get(n).copied() {
        if c == '\t' {
            level = level.saturating_add(1);
            n = n.saturating_add(1);
            continue;
        }
        if c == ' ' {
            let mut k = n;
            let mut spaces = 0u16;
            while matches!(chars.get(k), Some((_, ' '))) {
                spaces = spaces.saturating_add(1);
                k = k.saturating_add(1);
            }
            level = level.saturating_add(spaces / 2);
            let next = chars.get(k).map(|(_, ch)| *ch);
            n = k;
            if spaces % 2 == 1 && next.is_some_and(|ch| ch == '.' || ch == '>') {
                continue;
            }
            if next.is_none() || next.is_some_and(|ch| ch != '\t' && ch != '.' && ch != '>') {
                break;
            }
            continue;
        }
        if c == '.' {
            let mut k = n;
            let mut dots = 0u16;
            while matches!(chars.get(k), Some((_, '.'))) {
                dots = dots.saturating_add(1);
                k = k.saturating_add(1);
            }
            let next = chars.get(k).map(|(_, ch)| *ch);
            if !next.is_none_or(|ch| ch.is_whitespace() || ch == '>') {
                break;
            }
            level = level.saturating_add(dots);
            n = k;
            continue;
        }
        if c == '>' {
            level = level.saturating_add(1);
            n = n.saturating_add(1);
            continue;
        }
        break;
    }
    let start = chars.get(n).map(|(i, _)| *i).unwrap_or(line.len());
    (level, line.get(start..).unwrap_or(""))
}

fn wbs_numbers(levels: &[u16]) -> Vec<String> {
    let mut out = Vec::new();
    let mut counters: Vec<usize> = Vec::new();
    for level in levels {
        let lvl = usize::from((*level).max(1));
        counters.resize(lvl, 0);
        if let Some(c) = counters.last_mut() {
            *c = c.saturating_add(1);
        }
        out.push(counters.iter().map(usize::to_string).collect::<Vec<_>>().join("."));
    }
    out
}

fn resolve_number(n: usize, wbs: &[String]) -> Option<usize> {
    n.checked_sub(1).filter(|i| wbs.get(*i).is_some()).or_else(|| wbs.iter().position(|w| w == &n.to_string()))
}

fn subtree_end(levels: &[u16], i: usize) -> usize {
    let Some(level) = levels.get(i).copied() else { return i };
    let mut j = i.saturating_add(1);
    while levels.get(j).is_some_and(|l| *l > level) {
        j = j.saturating_add(1);
    }
    j
}

fn outline_related(levels: &[u16], a: usize, b: usize) -> bool {
    if a == b {
        return true;
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    hi < subtree_end(levels, lo)
}

fn reaches(succs: &[Vec<usize>], from: usize, target: usize) -> bool {
    let mut stack = vec![from];
    let mut seen = vec![false; succs.len()];
    while let Some(n) = stack.pop() {
        if seen.get(n).copied().unwrap_or(true) {
            continue;
        }
        if let Some(flag) = seen.get_mut(n) {
            *flag = true;
        }
        let Some(nexts) = succs.get(n) else { continue };
        for &step in nexts {
            if step == target {
                return true;
            }
            stack.push(step);
        }
    }
    false
}

/// Inserts every ticket at `at` (or at the end). One call, so the caller's edit is one undo step.
/// A flagged line refuses the whole insert.
pub fn apply(p: &mut Project, sheet: &Sheet, at: Option<usize>) -> Result<Vec<TaskId>, String> {
    if sheet.tickets.is_empty() || sheet.looped || sheet.tickets.iter().any(|t| t.issue.is_some()) {
        return Err(crate::lang::text("qe.blocked").into());
    }
    let index0 = at.unwrap_or(p.tasks.len()).min(p.tasks.len());
    let inherited = match index0.checked_sub(1).and_then(|i| p.tasks.get(i).map(|t| (i, t.outline_level))) {
        Some((above, level)) if p.has_children(above) => level.saturating_add(1),
        Some((_, level)) => level,
        None => 1,
    };
    let mut ids = Vec::new();
    for (offset, ticket) in sheet.tickets.iter().enumerate() {
        let index = index0.saturating_add(offset);
        let id = if ticket.milestone { p.add_milestone(&ticket.name, Some(index)) } else { p.add_task(&ticket.name, Some(index)) };
        let absolute = inherited.saturating_add(ticket.level.saturating_sub(1)).clamp(1, MAX_OUTLINE_LEVEL);
        if let Some(row) = p.task_mut(id) {
            row.outline_level = absolute;
        }
        ids.push(id);
    }
    for (ticket, id) in sheet.tickets.iter().zip(&ids) {
        if ticket.group {
            if let Some(row) = p.task_mut(*id) {
                row.kind = TaskKind::Task;
            }
        } else if !ticket.milestone
            && let Some(minutes) = ticket.minutes
        {
            p.set_typed_duration(
                *id,
                TypedDuration { minutes, estimated: ticket.estimated, elapsed: false, unit: ticket.unit },
            )
            .map_err(|e| e.to_string())?;
        }
        if !ticket.group && !ticket.resources.is_empty() {
            p.apply_resource_names(*id, &ticket.resources.join(", ")).map_err(|e| e.to_string())?;
        }
    }
    for (ticket, succ) in sheet.tickets.iter().zip(&ids) {
        for pred_i in &ticket.preds {
            let Some(pred) = ids.get(*pred_i).copied() else { continue };
            p.link(pred.into(), *succ, LinkType::FS, Lag::None).map_err(|e| e.to_string())?;
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::EditExt;
    use plancraft_core::workspace::{ChangeSource, Workspace};

    const PASTE: &str = "\
Kick off meeting 1d

Phase
  Design 3d
	. Check 4h @Ana
	Build 2w ^2.1
Ship ! ^2.2
";

    fn time() -> TimeSettings {
        TimeSettings::default()
    }

    fn task<'a>(p: &'a Project, name: &str) -> &'a Task {
        p.tasks.iter().find(|t| t.name == name).unwrap()
    }

    #[test]
    fn a_paste_with_indents_durations_and_links_becomes_the_tree() {
        let sheet = parse(PASTE, &time());
        assert!(sheet.tickets.iter().all(|t| t.issue.is_none()), "{:?}", sheet.tickets.iter().map(|t| &t.issue).collect::<Vec<_>>());
        assert!(!sheet.looped);
        let mut p = Project::new("P", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        p.add_task("A", None);
        p.add_task("B", None);
        let ids = apply(&mut p, &sheet, Some(1)).unwrap();
        assert_eq!(ids.len(), 6);
        assert_eq!(
            p.tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["A", "Kick off meeting", "Phase", "Design", "Check", "Build", "Ship", "B"]
        );
        let day = p.settings.time.minutes_per_day;
        assert_eq!(task(&p, "Kick off meeting").duration, day);
        assert_eq!(task(&p, "Kick off meeting").outline_level, 1);
        let phase = p.tasks.iter().position(|t| t.name == "Phase").unwrap();
        assert!(p.has_children(phase));
        assert_eq!(task(&p, "Design").outline_level, 2);
        assert_eq!(p.format_resource_names(task(&p, "Check").id), "Ana");
        assert!(p.has_children(p.tasks.iter().position(|t| t.name == "Design").unwrap()));
        assert_eq!(task(&p, "Check").outline_level, 3);
        assert_eq!(task(&p, "Check").duration, 4 * 60);
        assert_eq!(task(&p, "Build").duration, 2 * p.settings.time.minutes_per_week);
        assert!(task(&p, "Build").predecessors.iter().any(|d| d.predecessor.local() == Some(task(&p, "Design").id)));
        assert!(task(&p, "Ship").is_milestone());
        assert!(task(&p, "Ship").predecessors.iter().any(|d| d.predecessor.local() == Some(task(&p, "Build").id)));

        let chevron = parse("Phase\n> Design\n>> Check\n", &time());
        let tabs = parse("Phase\n\tDesign\n\t\tCheck\n", &time());
        let dots = parse("Phase\n. Design\n.. Check\n", &time());
        let spaces = parse("Phase\n  Design\n    Check\n", &time());
        for other in [&chevron, &tabs, &dots, &spaces] {
            assert!(other.tickets.iter().all(|t| t.issue.is_none()));
            assert_eq!(other.tickets.iter().map(|t| t.level).collect::<Vec<_>>(), vec![1, 2, 3]);
        }
    }

    #[test]
    fn an_invalid_line_is_flagged_and_kept() {
        let skipped = parse("Phase\n\t\t\tSkipped 3d\n", &time());
        assert_eq!(skipped.tickets.len(), 2, "the bad line stays in the preview");
        assert_eq!(skipped.tickets[1].name, "Skipped");
        assert_eq!(skipped.tickets[1].issue, Some(Issue::SkippedLevel));
        let missing = parse("Ghost ^9\n", &time());
        assert_eq!(missing.tickets.len(), 1);
        assert_eq!(missing.tickets[0].issue, Some(Issue::Unavailable));
        assert!(missing.tickets[0].links.iter().any(|l| !l.ok));
        let grouped = parse("Phase @Ana\n  Child\n", &time());
        assert_eq!(grouped.tickets[0].issue, Some(Issue::GroupResource));
        assert_eq!(grouped.tickets.len(), 2);
        let mut p = Project::new("P", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        assert!(apply(&mut p, &skipped, None).is_err());
        assert!(p.tasks.is_empty(), "a flagged line inserts nothing");
    }

    #[test]
    fn insert_undoes_in_one_step() {
        let sheet = parse(PASTE, &time());
        let mut ws = Workspace::default();
        let mut plan = Project::new("P", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        plan.add_task("A", None);
        plan.add_task("B", None);
        ws.add(plan, None);
        let doc = ws.active_doc_mut().unwrap();
        doc.edit_msg("Quick Entry", |p| apply(p, &sheet, Some(1)).map(|_| ())).unwrap();
        assert_eq!(doc.project.tasks.len(), 8);
        assert_eq!(doc.history().len(), 1);
        assert_eq!(doc.undo_label(), Some("Quick Entry"));
        assert!(doc.try_undo_as(ChangeSource::User).unwrap());
        assert_eq!(doc.project.tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert!(!doc.can_undo());
    }
}
