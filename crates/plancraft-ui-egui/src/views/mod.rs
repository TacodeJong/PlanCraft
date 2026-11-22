//! Main views.

pub mod board;
pub mod calendar;
pub mod deliveries;
pub mod filter_bar;
pub mod gantt;
pub mod gantt_chart;
pub mod gantt_xlsx;
pub mod milestones;
pub mod network;
pub mod overview;
pub mod properties;
pub mod report;
pub mod resource_sheet;
pub mod resource_usage;
pub mod task_usage;
pub mod team_planner;
pub mod timeline;
pub mod timesheet;
pub mod workload;

use std::collections::{HashMap, HashSet};

use plancraft_core::prelude::*;

use crate::columns;
use crate::fields::{Field, SortKey};
use crate::state::{DateWindow, FocusScope, PageGroup, Selection, TaskFilter, TaskKindFilter, UiState};

/// One row of a task view: a task, the header of a group, or the notes of an expanded task.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Task(usize),
    Group(GroupRow),
    /// Notes of the task at this index. Read-only; groups never get one.
    Notes(usize),
}

/// A group header: the shared value and the tasks under it (task indices, in display order).
#[derive(Clone, Debug, PartialEq)]
pub struct GroupRow {
    pub label: String,
    pub tasks: Vec<usize>,
    pub collapsed: bool,
}

impl Row {
    /// The task index of a task row.
    pub fn task(&self) -> Option<usize> {
        match self {
            Row::Task(i) => Some(*i),
            Row::Group(_) | Row::Notes(_) => None,
        }
    }
}

/// The task shown on display row `row` (`None` for group headers and blank rows).
pub fn task_at<'a>(p: &'a Project, rows: &[Row], row: usize) -> Option<&'a Task> {
    rows.get(row).and_then(Row::task).and_then(|i| p.tasks.get(i))
}

/// The display row of the task at index `idx`, if it is shown.
pub fn row_of_task(rows: &[Row], idx: usize) -> Option<usize> {
    rows.iter().position(|r| r.task() == Some(idx))
}

fn matches_filter(p: &Project, t: &Task, f: TaskFilter) -> bool {
    match f {
        TaskFilter::All => true,
        TaskFilter::Critical => t.sched.critical,
        TaskFilter::WithLevelingDelay => t.leveling_delay > 0,
        TaskFilter::WithoutLevelingDelay => t.leveling_delay == 0,
        TaskFilter::Incomplete => !t.is_complete(),
        TaskFilter::Completed => t.is_complete(),
        TaskFilter::Milestones => t.is_milestone(),
        TaskFilter::Late => columns::is_late(p, t),
        TaskFilter::Summary => t.sched.is_summary,
        TaskFilter::Unassigned => !t.sched.is_summary && !t.is_milestone() && p.assignments_for_task(t.id).next().is_none(),
    }
}

/// Whether a task passes the view's filter, the activity focus and the other strip criteria.
/// The text matches the task name or an assigned resource name.
pub fn task_matches(p: &Project, st: &UiState, t: &Task) -> bool {
    matches_filter(p, t, st.filter)
        && text_hit(p, t, &st.search.trim().to_lowercase())
        && focus_hit(p, st, t)
        && status_hit(p, st, t)
        && kind_hit(p, st, t)
        && date_hit(p, st, t)
        && resource_hit(p, st, t)
}

/// True when task views are hiding something. Ancestors of a match stay visible as context.
pub fn criteria_active(st: &UiState) -> bool {
    st.filter != TaskFilter::All
        || !st.search.trim().is_empty()
        || st.focus.is_some()
        || st.status_filter.is_some()
        || st.kind_filter != TaskKindFilter::All
        || st.date_window != DateWindow::All
        || !st.filter_resources.is_empty()
}

fn focus_hit(p: &Project, st: &UiState, t: &Task) -> bool {
    let Some(focus) = st.focus else { return true };
    let Some(root) = p.task_index(focus.task) else { return false };
    let Some(idx) = p.task_index(t.id) else { return false };
    match focus.scope {
        FocusScope::Subtree => in_subtree(p, root, idx),
        FocusScope::Path => p.path_roles(&[focus.task]).contains_key(&t.id),
    }
}

fn in_subtree(p: &Project, root: usize, idx: usize) -> bool {
    let mut at = Some(idx);
    while let Some(i) = at {
        if i == root {
            return true;
        }
        at = p.parent_index(i);
    }
    false
}

fn status_hit(p: &Project, st: &UiState, t: &Task) -> bool {
    let Some(want) = st.status_filter else { return true };
    p.task_index(t.id).is_some_and(|i| p.displayed_status(i) == want)
}

fn kind_hit(p: &Project, st: &UiState, t: &Task) -> bool {
    let Some(idx) = p.task_index(t.id) else { return false };
    match st.kind_filter {
        TaskKindFilter::All => true,
        TaskKindFilter::Tasks => !t.is_milestone() && !p.has_children(idx),
        TaskKindFilter::Milestones => t.is_milestone(),
        TaskKindFilter::Groups => p.has_children(idx),
    }
}

fn date_hit(p: &Project, st: &UiState, t: &Task) -> bool {
    match st.date_window {
        DateWindow::All => true,
        DateWindow::Overdue => columns::is_late(p, t),
        DateWindow::ThisWeek => {
            let today = plancraft_core::dates::today();
            let back = i64::from(plancraft_core::dates::weekday_index(today.weekday()));
            let monday = plancraft_core::dates::shift_days(today, -back);
            overlaps(t, monday, plancraft_core::dates::shift_days(monday, 6))
        }
        DateWindow::NextTwoWeeks => {
            let today = plancraft_core::dates::today();
            let back = i64::from(plancraft_core::dates::weekday_index(today.weekday()));
            let monday = plancraft_core::dates::shift_days(today, -back);
            let start = plancraft_core::dates::shift_days(monday, 7);
            overlaps(t, start, plancraft_core::dates::shift_days(start, 13))
        }
        DateWindow::Range => {
            if st.date_from.is_none() && st.date_to.is_none() {
                return true;
            }
            let today = plancraft_core::dates::today();
            let from = st.date_from.unwrap_or_else(|| plancraft_core::dates::ymd(1900, 1, 1).unwrap_or(today));
            let to = st.date_to.unwrap_or_else(|| plancraft_core::dates::ymd(2300, 1, 1).unwrap_or(today));
            overlaps(t, from, to)
        }
    }
}

fn overlaps(t: &Task, from: plancraft_core::dates::Date, to: plancraft_core::dates::Date) -> bool {
    let start = t.sched.start.or(t.manual_start).map(|d| d.date());
    let finish = t.sched.finish.or(t.manual_finish).map(|d| d.date());
    match (start, finish) {
        (Some(s), Some(f)) => s <= to && f >= from,
        _ => false,
    }
}

fn resource_hit(p: &Project, st: &UiState, t: &Task) -> bool {
    if st.filter_resources.is_empty() {
        return true;
    }
    p.assignments_for_task(t.id).any(|a| st.filter_resources.contains(&a.resource))
}

fn text_hit(p: &Project, t: &Task, search: &str) -> bool {
    if search.is_empty() {
        return true;
    }
    t.name.to_lowercase().contains(search) || p.format_resource_names_only(t.id).to_lowercase().contains(search)
}

fn page_group_entries(group: PageGroup, p: &Project, idx: usize) -> Vec<(String, SortKey)> {
    let Some(t) = p.tasks.get(idx) else {
        return vec![("No value".into(), SortKey::Missing)];
    };
    let one = |label: String| {
        let key = SortKey::Text(label.to_lowercase());
        vec![(label, key)]
    };
    match group {
        PageGroup::None => Vec::new(),
        PageGroup::Milestones => one(if t.is_milestone() { "Milestones".into() } else { "Activities".into() }),
        PageGroup::Critical => one(if t.sched.critical { "On critical path".into() } else { "Not on critical path".into() }),
        PageGroup::Priority => one(format!("Priority {}", t.priority)),
        // One header per assignee — multi-assigned tasks belong in each matching group.
        PageGroup::Resources => resource_group_entries(p, t.id, "Unassigned"),
        PageGroup::Status => one(crate::lang::status_name(p.displayed_status(idx)).into()),
    }
}

/// Group labels for a field: Resources splits into one group per assignee; other fields stay single-valued.
fn field_group_entries(field: &Field, p: &Project, idx: usize) -> Vec<(String, SortKey)> {
    if matches!(field, Field::Builtin(crate::columns::TaskCol::Resources)) {
        let empty = "No value";
        let Some(t) = p.tasks.get(idx) else {
            return vec![(empty.into(), SortKey::Missing)];
        };
        return resource_group_entries(p, t.id, empty);
    }
    let label = field.group_label(p, idx);
    let key = field.sort_key(p, idx);
    vec![(label, key)]
}

/// Distinct resource names on a task, each as its own group; unassigned tasks share `empty`.
fn resource_group_entries(p: &Project, task: TaskId, empty: &str) -> Vec<(String, SortKey)> {
    let mut seen = Vec::new();
    let mut entries = Vec::new();
    for a in p.assignments_for_task(task) {
        if seen.contains(&a.resource) {
            continue;
        }
        seen.push(a.resource);
        if let Some(r) = p.resource(a.resource) {
            let label = r.name.clone();
            let key = SortKey::Text(label.to_lowercase());
            entries.push((label, key));
        }
    }
    if entries.is_empty() {
        vec![(empty.to_string(), SortKey::Missing)]
    } else {
        entries
    }
}

/// Orders task indices by the sort field (ties keep outline order; empty values stay last in
/// either direction).
fn sort_indices(p: &Project, items: &mut [usize], field: &Field, descending: bool) {
    let keys: HashMap<usize, SortKey> = items.iter().map(|&i| (i, field.sort_key(p, i))).collect();
    items.sort_by(|a, b| {
        let (ka, kb) = (keys.get(a).unwrap_or(&SortKey::Missing), keys.get(b).unwrap_or(&SortKey::Missing));
        let missing = (matches!(ka, SortKey::Missing), matches!(kb, SortKey::Missing));
        let ord = match missing {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ if descending => kb.cmp(ka),
            _ => ka.cmp(kb),
        };
        ord.then(a.cmp(b))
    });
}

/// Rows shown in task views, honoring the filter and search text, the sort (within the outline:
/// subtasks stay under their summary), grouping (subtasks under group headers) and collapsed
/// summaries and groups.
pub fn visible_rows(p: &Project, st: &UiState) -> Vec<Row> {
    let filtering = criteria_active(st);
    let matches = |t: &Task| task_matches(p, st, t);
    let sort = st.sort.as_ref().and_then(|(k, desc)| Field::from_key(p, k).map(|f| (f, *desc)));

    if st.page_group != PageGroup::None {
        let group = st.page_group;
        return grouped(p, st, filtering, &sort, &matches, &|i| page_group_entries(group, p, i));
    }
    if let Some(group) = st.group.as_ref().and_then(|k| Field::from_key(p, k)) {
        return grouped(p, st, filtering, &sort, &matches, &|i| field_group_entries(&group, p, i));
    }

    outline_rows(p, st)
}

/// Subtasks under one header per label. Summaries stay out; the filter and search still apply.
/// A task may appear under several labels (multi-assigned resources).
fn grouped(
    p: &Project,
    st: &UiState,
    filtering: bool,
    sort: &Option<(Field, bool)>,
    matches: &dyn Fn(&Task) -> bool,
    entries_of: &dyn Fn(usize) -> Vec<(String, SortKey)>,
) -> Vec<Row> {
    let mut tasks: Vec<usize> =
        p.tasks.iter().enumerate().filter(|(_, t)| !t.sched.is_summary && (!filtering || matches(t))).map(|(i, _)| i).collect();
    if let Some((field, desc)) = sort {
        sort_indices(p, &mut tasks, field, *desc);
    }
    let mut groups: Vec<(SortKey, GroupRow)> = Vec::new();
    let mut by_label: HashMap<String, usize> = HashMap::new();
    for i in tasks {
        for (label, key) in entries_of(i) {
            let slot = *by_label.entry(label.clone()).or_insert_with(|| {
                groups.push((
                    key.clone(),
                    GroupRow { label: label.clone(), tasks: Vec::new(), collapsed: st.collapsed_groups.contains(&label) },
                ));
                groups.len().saturating_sub(1)
            });
            if let Some((k, g)) = groups.get_mut(slot) {
                if key.cmp(k) == std::cmp::Ordering::Less {
                    *k = key;
                }
                g.tasks.push(i);
            }
        }
    }
    groups.sort_by(|(ka, a), (kb, b)| ka.cmp(kb).then_with(|| a.label.cmp(&b.label)));
    let mut rows = Vec::new();
    for (_, g) in groups {
        let shown = if g.collapsed { Vec::new() } else { g.tasks.clone() };
        rows.push(Row::Group(g));
        rows.extend(shown.into_iter().map(Row::Task));
    }
    rows
}

/// Rows of the outline (no grouping), honoring the filter and search text, the sort within the
/// outline and collapsed summaries.
pub fn outline_rows(p: &Project, st: &UiState) -> Vec<Row> {
    let filtering = criteria_active(st);
    let matches = |t: &Task| task_matches(p, st, t);
    let sort = st.sort.as_ref().and_then(|(k, desc)| Field::from_key(p, k).map(|f| (f, *desc)));
    let mut keep: Vec<bool> = vec![!filtering; p.tasks.len()];
    if filtering {
        for (i, t) in p.tasks.iter().enumerate() {
            if !matches(t) {
                continue;
            }
            // Keep summary rows above matches as context, like "show related summary rows".
            let mut a = Some(i);
            while let Some(ai) = a {
                if let Some(k) = keep.get_mut(ai) {
                    *k = true;
                }
                a = p.parent_index(ai);
            }
        }
    }
    // Children of each row (and the top level), from one pass over outline levels.
    let mut roots: Vec<usize> = Vec::new();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); p.tasks.len()];
    let mut stack: Vec<(usize, u16)> = Vec::new();
    for (i, t) in p.tasks.iter().enumerate() {
        while stack.last().is_some_and(|&(_, lvl)| lvl >= t.outline_level) {
            stack.pop();
        }
        match stack.last().and_then(|&(pi, _)| children.get_mut(pi)) {
            Some(list) => list.push(i),
            None => roots.push(i),
        }
        stack.push((i, t.outline_level));
    }
    if let Some((field, desc)) = &sort {
        sort_indices(p, &mut roots, field, *desc);
        for list in &mut children {
            sort_indices(p, list, field, *desc);
        }
    }
    let mut rows = Vec::with_capacity(p.tasks.len());
    let mut todo: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(i) = todo.pop() {
        if !keep.get(i).copied().unwrap_or(false) {
            continue;
        }
        rows.push(Row::Task(i));
        let collapsed = p.tasks.get(i).is_some_and(|t| st.collapsed.contains(&t.id));
        if !collapsed && let Some(kids) = children.get(i) {
            todo.extend(kids.iter().rev());
        }
    }
    rows
}

/// Applies a click on `row` to the task selection. A group header selects the tasks under it.
pub fn select_row(sel: &mut Selection, p: &Project, rows: &[Row], row: usize, shift: bool, ctrl: bool) {
    if let Some(Row::Group(g)) = rows.get(row) {
        if !ctrl {
            sel.tasks.clear();
        }
        sel.tasks.extend(g.tasks.iter().filter_map(|&i| p.tasks.get(i)).map(|t| t.id));
        sel.clear_resources();
        return;
    }
    let Some(id) = task_at(p, rows, row).map(|t| t.id) else { return };
    if shift && let Some(anchor) = sel.anchor {
        let a = rows.iter().position(|r| task_at(p, std::slice::from_ref(r), 0).is_some_and(|t| t.id == anchor)).unwrap_or(row);
        let (lo, hi) = if a <= row { (a, row) } else { (row, a) };
        if !ctrl {
            sel.tasks.clear();
        }
        for r in rows.iter().take(hi + 1).skip(lo) {
            if let Some(t) = r.task().and_then(|i| p.tasks.get(i)) {
                sel.tasks.insert(t.id);
            }
        }
        sel.clear_resources();
    } else if ctrl {
        if !sel.tasks.remove(&id) {
            sel.tasks.insert(id);
        }
        sel.anchor = Some(id);
        sel.clear_resources();
    } else {
        sel.select_task(id);
    }
}

/// Selected task ids in outline order.
pub fn selected_in_order(p: &Project, sel: &Selection) -> Vec<TaskId> {
    p.tasks.iter().filter(|t| sel.tasks.contains(&t.id)).map(|t| t.id).collect()
}

/// Task ids that have an assignment to an overallocated resource.
pub fn overallocated_tasks(p: &Project, over: &HashSet<ResourceId>) -> HashSet<TaskId> {
    if over.is_empty() {
        return HashSet::new();
    }
    p.assignments.iter().filter(|a| over.contains(&a.resource)).map(|a| a.task).collect()
}

/// Task indices in the order the task table shows them, as if every summary and group were
/// expanded (what Find searches): filtered-out tasks are left out. Multi-group duplicates
/// (a task under several resource headers) appear once, in first-seen order.
pub fn search_order(p: &Project, st: &mut UiState) -> Vec<usize> {
    let collapsed = std::mem::take(&mut st.collapsed);
    let groups = std::mem::take(&mut st.collapsed_groups);
    let rows = visible_rows(p, st);
    st.collapsed = collapsed;
    st.collapsed_groups = groups;
    let mut seen = HashSet::new();
    rows.iter().filter_map(Row::task).filter(|&i| seen.insert(i)).collect()
}

/// Shows task `id` in the task table: expands the summaries (and the group) hiding it, selects it,
/// moves the grid cursor to it (column `col`, or the cursor's column) and scrolls the table and
/// the chart to it. Returns false when the filter or search hides it.
pub fn reveal_task(p: &Project, st: &mut UiState, id: TaskId, col: Option<usize>) -> bool {
    let Some(idx) = p.task_index(id) else { return false };
    let mut a = p.parent_index(idx);
    while let Some(ai) = a {
        if let Some(t) = p.tasks.get(ai) {
            st.collapsed.remove(&t.id);
        }
        a = p.parent_index(ai);
    }
    if st.page_group != PageGroup::None {
        for (label, _) in page_group_entries(st.page_group, p, idx) {
            st.collapsed_groups.remove(&label);
        }
    } else if let Some(group) = st.group.as_ref().and_then(|k| Field::from_key(p, k)) {
        for (label, _) in field_group_entries(&group, p, idx) {
            st.collapsed_groups.remove(&label);
        }
    }
    let rows = visible_rows(p, st);
    let Some(row) = row_of_task(&rows, idx) else { return false };
    st.sel.select_task(id);
    let col = col.or(st.task_grid.cursor.map(|(_, c)| c)).unwrap_or(1);
    st.task_grid.cursor = Some((row, col));
    st.task_grid.whole_row = false;
    st.task_grid.scroll_to = Some(row);
    if let Some(start) = p.tasks.get(idx).and_then(|t| t.sched.start) {
        st.gantt.scroll_to(start);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::{TaskCol, commit};

    fn demo() -> Project {
        plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap())
    }

    #[test]
    fn collapse_and_filter_rows() {
        let p = demo();
        let mut st = UiState::default();
        assert_eq!(visible_rows(&p, &st).len(), p.tasks.len());
        let planning = p.tasks[0].id;
        st.collapsed.insert(planning);
        assert_eq!(visible_rows(&p, &st).len(), p.tasks.len() - 3);
        st.collapsed.clear();
        st.filter = TaskFilter::Milestones;
        let rows = visible_rows(&p, &st);
        // 4 milestones plus their 4 summary parents.
        assert_eq!(rows.len(), 8);
        assert!(rows.iter().filter_map(Row::task).all(|i| p.tasks[i].is_milestone() || p.tasks[i].sched.is_summary));
    }

    #[test]
    fn sorting_keeps_subtasks_under_their_summary() {
        let p = demo();
        let mut st = UiState { sort: Some(("duration".into(), true)), ..UiState::default() };
        let rows = visible_rows(&p, &st);
        assert_eq!(rows.len(), p.tasks.len(), "sorting hides nothing");
        // Each subtask still follows its own summary, and siblings run longest first.
        let order: Vec<usize> = rows.iter().filter_map(Row::task).collect();
        for (pos, &i) in order.iter().enumerate() {
            if let Some(parent) = p.parent_index(i) {
                let ppos = order.iter().position(|&x| x == parent).unwrap();
                assert!(ppos < pos, "{} shown before its summary", p.tasks[i].name);
                assert!(order[ppos..pos].iter().all(|&x| x == parent || p.parent_index(x).is_some()), "subtasks stay inside their summary");
            }
        }
        for parent in (0..p.tasks.len()).filter(|&i| p.has_children(i)) {
            let kids: Vec<usize> = order.iter().copied().filter(|&x| p.parent_index(x) == Some(parent)).collect();
            assert!(kids.windows(2).all(|w| p.tasks[w[0]].sched.duration >= p.tasks[w[1]].sched.duration), "{}", p.tasks[parent].name);
        }
        st.sort = Some(("duration".into(), false));
        let asc: Vec<usize> = visible_rows(&p, &st).iter().filter_map(Row::task).collect();
        assert_ne!(asc, order);
    }

    #[test]
    fn grouping_by_a_custom_field_lists_subtasks_under_headers() {
        let p = demo();
        let status = p.find_custom_field("Status").unwrap().id;
        let mut st = UiState { group: Some(plancraft_core::custom::custom_column_key(status)), ..UiState::default() };
        let rows = visible_rows(&p, &st);
        let groups: Vec<&GroupRow> = rows.iter().filter_map(|r| if let Row::Group(g) = r { Some(g) } else { None }).collect();
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["Done", "In progress", "To do", "No value"], "groups in value order, blanks last");
        let mut seen = 0;
        for g in &groups {
            for &i in &g.tasks {
                let t = &p.tasks[i];
                assert!(!t.sched.is_summary, "summaries are not grouped");
                let v = t.custom.get(&status).map(|v| p.custom_value_text(status, v));
                assert_eq!(v.as_deref().unwrap_or("No value"), g.label);
                seen += 1;
            }
        }
        assert_eq!(seen, p.tasks.iter().filter(|t| !t.sched.is_summary).count());
        // A collapsed group shows its header only.
        st.collapsed_groups.insert("To do".into());
        let rows = visible_rows(&p, &st);
        let todo = rows.iter().position(|r| matches!(r, Row::Group(g) if g.label == "To do")).unwrap();
        assert!(matches!(rows.get(todo + 1), Some(Row::Group(_))), "no task rows under a collapsed group");
    }

    #[test]
    fn grouping_by_resource_splits_multi_assignees_into_one_group_each() {
        let p = demo();
        let st = UiState { group: Some("resources".into()), ..UiState::default() };
        let rows = visible_rows(&p, &st);
        let groups: Vec<&GroupRow> = rows.iter().filter_map(|r| if let Row::Group(g) = r { Some(g) } else { None }).collect();
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert!(!labels.iter().any(|l| l.contains(", ")), "no combination keys like \"Ana, Ben\"");
        assert!(labels.contains(&"Ana Lima"));
        assert!(labels.contains(&"Ben Okafor"));
        assert!(labels.contains(&"No value"), "unassigned leaves share one empty group");

        let frontend = p.tasks.iter().position(|t| t.name == "Frontend application").unwrap();
        let under = |name: &str| groups.iter().find(|g| g.label == name).map(|g| g.tasks.as_slice()).unwrap_or(&[]);
        assert!(under("Chloe Park").contains(&frontend), "multi-assigned task under Chloe");
        assert!(under("Ben Okafor").contains(&frontend), "and also under Ben");

        // Page-group Resources uses the same split, with "Unassigned" for empty.
        let st = UiState { page_group: PageGroup::Resources, ..UiState::default() };
        let page_labels: Vec<String> = visible_rows(&p, &st)
            .iter()
            .filter_map(|r| if let Row::Group(g) = r { Some(g.label.clone()) } else { None })
            .collect();
        assert!(!page_labels.iter().any(|l| l.contains(", ")));
        assert!(page_labels.iter().any(|l| l == "Unassigned"));
        assert!(!page_labels.iter().any(|l| l == "No value"));
    }

    #[test]
    fn pick_list_cells_set_and_clear_values() {
        let mut p = demo();
        let risk = p.find_custom_field("Risk").unwrap().id;
        let id = p.tasks[1].id;
        let field = crate::fields::Field::Custom(risk);
        field.commit(&mut p, id, "High").unwrap();
        assert_eq!(p.task(id).unwrap().custom.get(&risk), Some(&CustomValue::Text("High".into())));
        assert!(field.commit(&mut p, id, "Severe").is_err());
        field.commit(&mut p, id, crate::fields::NO_VALUE).unwrap();
        assert!(!p.task(id).unwrap().custom.contains_key(&risk));
    }

    #[test]
    fn column_commits_follow_ms_project_rules() {
        let mut p = demo();
        let id = p.tasks[1].id;
        commit(TaskCol::Duration, &mut p, id, "4d").unwrap();
        assert_eq!(p.task(id).unwrap().duration, 4 * 480);
        // Typing a start date on an auto task sets a Start No Earlier Than constraint.
        commit(TaskCol::Start, &mut p, id, "2026-11-02").unwrap();
        assert_eq!(p.task(id).unwrap().constraint.kind, ConstraintKind::Snet);
        assert!(commit(TaskCol::Duration, &mut p, id, "abc").is_err());
        assert!(commit(TaskCol::Name, &mut p, id, "  ").is_err());
        // Row 3 already depends on this task, so linking back is rejected.
        assert!(commit(TaskCol::Predecessors, &mut p, id, "3").is_err());
        let event = p.tasks[16].id;
        commit(TaskCol::Predecessors, &mut p, event, "14FS+1d").unwrap();
        assert_eq!(p.format_predecessors(event), "14FS+1 day");
    }

    /// Regression: the Calendar and Milestones views ignored the filter and search (the status bar
    /// still said "Filter: …"), and the Calendar showed inactive tasks.
    #[test]
    fn calendar_and_milestones_follow_the_filter_and_search() {
        let mut p = demo();
        let mut st = UiState::default();
        let on_calendar = |p: &Project, st: &UiState| -> Vec<String> {
            p.tasks.iter().filter(|t| calendar::shows(p, st, t)).map(|t| t.name.clone()).collect()
        };
        let all = on_calendar(&p, &st);
        assert_eq!(all.len(), p.tasks.iter().filter(|t| !t.sched.is_summary).count());

        st.filter = TaskFilter::Milestones;
        assert_eq!(on_calendar(&p, &st).len(), 4);
        st.filter = TaskFilter::All;
        st.search = "beta".into();
        let found = on_calendar(&p, &st);
        assert!(!found.is_empty() && found.len() < all.len(), "{found:?}");
        assert!(found.iter().all(|n| n.to_lowercase().contains("beta")), "{found:?}");
        let at = plancraft_core::report::status_moment(&p);
        let (listed, total) = milestones::shown(&p, &st, at);
        assert_eq!(total, 4);
        assert_eq!(listed.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Beta ready"]);

        // An inactive task leaves the calendar.
        st.search.clear();
        let leaf = p.tasks.iter().find(|t| !t.sched.is_summary && !t.is_milestone()).unwrap().id;
        p.task_mut(leaf).unwrap().active = false;
        plancraft_core::schedule::recalculate(&mut p);
        assert_eq!(on_calendar(&p, &st).len(), all.len() - 1);
    }

    #[test]
    fn subtree_filter_shows_the_group_its_descendants_and_its_ancestors() {
        let mut p = Project::new("Tree", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let phase = p.add_task("Phase", None);
        let design = p.add_task("Design", None);
        let wire = p.add_task("Wireframe", None);
        let sibling = p.add_task("Sibling", None);
        p.indent(design).unwrap();
        p.indent(wire).unwrap();
        p.indent(wire).unwrap();
        let st = UiState {
            focus: Some(crate::state::ActivityFocus { task: design, scope: crate::state::FocusScope::Subtree }),
            ..UiState::default()
        };
        let names: Vec<&str> = outline_rows(&p, &st).iter().filter_map(Row::task).map(|i| p.tasks[i].name.as_str()).collect();
        assert_eq!(names, ["Phase", "Design", "Wireframe"]);
        assert!(!names.contains(&"Sibling"));
        let bytes = report::workbook(&p, &st).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Phase") && text.contains("Design") && text.contains("Wireframe"));
        assert!(!text.contains("Sibling"), "the export keeps the same subtree");
        let _ = (phase, sibling);
    }

    #[test]
    fn notes_rows_open_under_a_leaf_and_never_under_a_group() {
        let mut p = demo();
        let summary = p.tasks.iter().position(|t| t.sched.is_summary).unwrap();
        let leaf = p.tasks.iter().position(|t| !t.sched.is_summary).unwrap();
        p.tasks[summary].notes = "group notes stay in the pane".into();
        p.tasks[leaf].notes = "bring the brief".into();
        let mut open = std::collections::HashSet::new();
        open.insert(p.tasks[summary].id);
        open.insert(p.tasks[leaf].id);
        let rows = gantt::outline_with_notes(&visible_rows(&p, &UiState::default()), &p, &open);
        let notes: Vec<usize> = rows.iter().filter_map(|r| if let Row::Notes(i) = r { Some(*i) } else { None }).collect();
        assert_eq!(notes, vec![leaf]);
        assert!(rows.iter().any(|r| matches!(r, Row::Notes(i) if *i == leaf)));
        assert!(rows.iter().all(|r| !matches!(r, Row::Notes(i) if *i == summary)));
    }

    #[test]
    fn delayed_filters_are_shared_and_distinct_from_late() {
        let mut p = demo();
        let delayed = p
            .tasks
            .iter()
            .find(|t| !t.sched.is_summary && !t.is_milestone() && t.sched.finish.is_some())
            .map(|t| t.id)
            .expect("a leaf task");
        let late = p
            .tasks
            .iter()
            .find(|t| !t.sched.is_summary && !t.is_milestone() && t.id != delayed && t.sched.finish.is_some())
            .map(|t| t.id)
            .expect("a second leaf task");

        p.task_mut(delayed).expect("delayed task").leveling_delay = 2 * 480;
        p.task_mut(late).expect("late task").leveling_delay = 0;
        let finish = p.task(late).and_then(|t| t.sched.finish).expect("late task finish");
        p.task_mut(late).expect("late task mut").deadline = Some(plancraft_core::dates::add_minutes(finish, -2 * 1440));
        p.task_mut(late).expect("late task mut").percent_complete = 0;
        plancraft_core::schedule::recalculate(&mut p);

        let delayed_task = p.task(delayed).expect("delayed task");
        let late_task = p.task(late).expect("late task");
        assert!(delayed_task.leveling_delay > 0);
        assert!(!columns::is_late(&p, delayed_task), "delayed fixture should not be late");
        assert!(columns::is_late(&p, late_task), "late fixture should be late");

        let mut st = UiState { filter: TaskFilter::WithLevelingDelay, ..UiState::default() };
        assert!(task_matches(&p, &st, delayed_task));
        assert!(!task_matches(&p, &st, late_task));

        st.filter = TaskFilter::WithoutLevelingDelay;
        assert!(!task_matches(&p, &st, delayed_task));
        assert!(task_matches(&p, &st, late_task));

        st.filter = TaskFilter::WithLevelingDelay;
        st.search = delayed_task.name.clone();
        assert!(task_matches(&p, &st, delayed_task), "new filter combines with existing criteria");
        st.search = "no-such-task".into();
        assert!(!task_matches(&p, &st, delayed_task));
    }
}
