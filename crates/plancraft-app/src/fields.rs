//! Table fields: the built-in task columns and the project's custom fields behind one type, with
//! stable keys (saved in the plan's table layouts), display, editing and sort keys.

use std::cmp::Ordering;

use plancraft_core::custom::{self, CustomFieldKind, CustomValue};
use plancraft_core::dates::prelude::*;
use plancraft_core::prelude::*;

use crate::columns::{self, BlField, CellCtx, TaskCol, TaskTable};
use crate::format;
use crate::widgets::grid::{Cell, ColumnDef};
use plancraft_core::baseline::{MAX_BASELINE, MAX_INTERIM};

/// The pick-list entry that clears a custom value.
pub const NO_VALUE: &str = "(none)";

/// A task table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Builtin(TaskCol),
    Custom(CustomFieldId),
}

/// Built-in columns and their keys (as saved in table layouts). Keys never change.
const BUILTIN: [(TaskCol, &str); 59] = [
    (TaskCol::Indicators, "indicators"),
    (TaskCol::Mode, "mode"),
    (TaskCol::Wbs, "wbs"),
    (TaskCol::Name, "name"),
    (TaskCol::Duration, "duration"),
    (TaskCol::LevelingDelay, "leveling_delay"),
    (TaskCol::Start, "start"),
    (TaskCol::Finish, "finish"),
    (TaskCol::Predecessors, "predecessors"),
    (TaskCol::Successors, "successors"),
    (TaskCol::Resources, "resources"),
    (TaskCol::Units, "units"),
    (TaskCol::PercentComplete, "percent_complete"),
    (TaskCol::Status, "status"),
    (TaskCol::RowMark, "row_mark"),
    (TaskCol::Work, "work"),
    (TaskCol::ActualWork, "actual_work"),
    (TaskCol::RemainingWork, "remaining_work"),
    (TaskCol::Cost, "cost"),
    (TaskCol::FixedCost, "fixed_cost"),
    (TaskCol::Baseline(0, BlField::Start), "baseline_start"),
    (TaskCol::Baseline(0, BlField::Finish), "baseline_finish"),
    (TaskCol::Baseline(0, BlField::Duration), "baseline_duration"),
    (TaskCol::Baseline(0, BlField::Work), "baseline_work"),
    (TaskCol::Baseline(0, BlField::Cost), "baseline_cost"),
    (TaskCol::StartVariance, "start_variance"),
    (TaskCol::FinishVariance, "finish_variance"),
    (TaskCol::WorkVariance, "work_variance"),
    (TaskCol::CostVariance, "cost_variance"),
    (TaskCol::ActualStart, "actual_start"),
    (TaskCol::ActualFinish, "actual_finish"),
    (TaskCol::ActualDuration, "actual_duration"),
    (TaskCol::RemainingDuration, "remaining_duration"),
    (TaskCol::Stop, "stop"),
    (TaskCol::Resume, "resume"),
    (TaskCol::EarlyStart, "early_start"),
    (TaskCol::EarlyFinish, "early_finish"),
    (TaskCol::LateStart, "late_start"),
    (TaskCol::LateFinish, "late_finish"),
    (TaskCol::FreeSlack, "free_slack"),
    (TaskCol::TotalSlack, "total_slack"),
    (TaskCol::ConstraintType, "constraint_type"),
    (TaskCol::ConstraintDate, "constraint_date"),
    (TaskCol::Deadline, "deadline"),
    (TaskCol::Priority, "priority"),
    (TaskCol::TaskType, "task_type"),
    (TaskCol::EstimateMethod, "estimate_method"),
    (TaskCol::Points, "points"),
    (TaskCol::Optimistic, "optimistic"),
    (TaskCol::MostLikely, "most_likely"),
    (TaskCol::Pessimistic, "pessimistic"),
    (TaskCol::ExpectedWork, "expected_work"),
    (TaskCol::Deliverable, "deliverable"),
    (TaskCol::Milestones, "milestones"),
    (TaskCol::Notes, "notes"),
    (TaskCol::UniqueId, "unique_id"),
    (TaskCol::Recurring, "recurring"),
    (TaskCol::BoardStatus, "board_status"),
    (TaskCol::ShowOnBoard, "show_on_board"),
];

/// A value to sort or group by. Empty values sort after everything else.
#[derive(Clone, Debug, PartialEq)]
pub enum SortKey {
    Num(f64),
    Time(DateTime),
    Text(String),
    Missing,
}

impl SortKey {
    fn rank(&self) -> u8 {
        match self {
            SortKey::Num(_) => 0,
            SortKey::Time(_) => 1,
            SortKey::Text(_) => 2,
            SortKey::Missing => 3,
        }
    }

    /// Total order: numbers, then dates, then text (case-insensitive), then empty values.
    pub fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (SortKey::Num(a), SortKey::Num(b)) => a.total_cmp(b),
            (SortKey::Time(a), SortKey::Time(b)) => a.cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl Field {
    pub fn key(&self) -> String {
        match self {
            Field::Builtin(c) => match BUILTIN.iter().find(|(b, _)| b == c) {
                Some((_, k)) => k.to_string(),
                None => match c {
                    TaskCol::Baseline(n, f) => format!("baseline{n}_{}", f.key()),
                    TaskCol::Interim(n, false) => format!("start{n}"),
                    TaskCol::Interim(n, true) => format!("finish{n}"),
                    _ => String::new(),
                },
            },
            Field::Custom(id) => custom::custom_column_key(*id),
        }
    }

    /// The field a key names in this project (`None` for unknown keys or deleted custom fields).
    pub fn from_key(p: &Project, key: &str) -> Option<Field> {
        if let Some(id) = custom::custom_column_field(key) {
            return p.custom_field(id).map(|_| Field::Custom(id));
        }
        if let Some((c, _)) = BUILTIN.iter().find(|(_, k)| *k == key) {
            return Some(Field::Builtin(*c));
        }
        generated_from_key(key).map(Field::Builtin)
    }

    pub fn title(&self, p: &Project) -> String {
        match self {
            Field::Builtin(c) => c.def(p).title,
            Field::Custom(id) => p.custom_field(*id).map_or_else(|| "?".into(), |f| f.name.clone()),
        }
    }

    pub fn def(&self, p: &Project) -> ColumnDef {
        match self {
            Field::Builtin(c) => c.def(p),
            Field::Custom(id) => {
                let Some(f) = p.custom_field(*id) else { return ColumnDef::new("?", 80.0).read_only() };
                let def = ColumnDef::new(f.name.clone(), 110.0);
                match f.kind {
                    CustomFieldKind::Text if !f.choices.is_empty() => {
                        let mut options: Vec<&str> = f.choices.iter().map(String::as_str).collect();
                        options.push(NO_VALUE);
                        def.choice(&options)
                    }
                    CustomFieldKind::Text => def,
                    CustomFieldKind::Number | CustomFieldKind::Cost | CustomFieldKind::Duration => def.right(),
                    CustomFieldKind::Date => def,
                    CustomFieldKind::Flag => def.center().choice(&["Yes", "No"]),
                }
            }
        }
    }

    pub fn cell(&self, p: &Project, idx: usize, ctx: &CellCtx) -> Cell {
        match self {
            Field::Builtin(c) => columns::cell(*c, p, idx, ctx),
            Field::Custom(id) => {
                let Some(t) = p.tasks.get(idx) else { return Cell::default() };
                let mut cell = match t.custom.get(id) {
                    None => Cell::default(),
                    Some(v) => {
                        let text = custom_display(p, *id, v);
                        let edit_text = match v {
                            CustomValue::Date(d) => Some(format::iso_date(Some(*d))),
                            CustomValue::Number(n) => Some(format!("{n}")),
                            _ => None,
                        };
                        Cell { text, edit_text, ..Default::default() }
                    }
                };
                if t.sched.is_summary {
                    cell.bold = true;
                }
                if !t.active {
                    cell.color = Some(crate::theme::pal().TEXT_WEAK);
                }
                cell
            }
        }
    }

    /// Applies an edited value. Runs inside an undoable edit.
    pub fn commit(&self, p: &mut Project, id: TaskId, text: &str) -> Result<(), String> {
        match self {
            Field::Builtin(c) => columns::commit(*c, p, id, text),
            Field::Custom(field) => {
                let listed = p.custom_field(*field).is_some_and(|f| f.choices.iter().any(|c| c == NO_VALUE));
                let text = if text == NO_VALUE && !listed { "" } else { text };
                let value = p.parse_custom_value(*field, text).map_err(|e| e.to_string())?;
                p.set_custom_value(id, *field, value).map_err(|e| e.to_string())
            }
        }
    }

    /// The value tasks are sorted and grouped by.
    pub fn sort_key(&self, p: &Project, idx: usize) -> SortKey {
        let Some(t) = p.tasks.get(idx) else { return SortKey::Missing };
        let s = &t.sched;
        let num = |v: f64| SortKey::Num(v);
        let mins = |m: Minutes| SortKey::Num(m as f64);
        let time = |d: Option<DateTime>| d.map_or(SortKey::Missing, SortKey::Time);
        let Field::Builtin(col) = self else {
            let Field::Custom(id) = self else { return SortKey::Missing };
            return match t.custom.get(id) {
                None => SortKey::Missing,
                Some(CustomValue::Text(v)) => SortKey::Text(v.to_lowercase()),
                Some(CustomValue::Number(n)) => num(*n),
                Some(CustomValue::Date(d)) => SortKey::Time(*d),
                Some(CustomValue::Duration(m)) => mins(*m),
                Some(CustomValue::Flag(b)) => num(if *b { 0.0 } else { 1.0 }),
            };
        };
        use TaskCol::*;
        match col {
            Wbs => num(idx as f64),
            UniqueId => num(f64::from(t.id.0)),
            Duration => mins(s.duration),
            LevelingDelay => mins(t.leveling_delay.max(0)),
            Start | EarlyStart => time(s.start),
            Finish | EarlyFinish => time(s.finish),
            LateStart => time(s.late_start),
            LateFinish => time(s.late_finish),
            ActualStart => time(t.actual_start),
            ActualFinish => time(t.actual_finish),
            ActualDuration => mins(t.duration_progress().0),
            RemainingDuration => mins(t.duration_progress().1),
            Stop => time(s.stop),
            Resume => time(s.resume),
            Baseline(n, BlField::Start) => time(t.baseline_in(*n).map(|b| b.start)),
            Baseline(n, BlField::Finish) => time(t.baseline_in(*n).map(|b| b.finish)),
            Baseline(n, BlField::Duration) => t.baseline_in(*n).map_or(SortKey::Missing, |b| mins(b.duration)),
            Baseline(n, BlField::Work) => t.baseline_in(*n).map_or(SortKey::Missing, |b| mins(b.work)),
            Baseline(n, BlField::Cost) => t.baseline_in(*n).map_or(SortKey::Missing, |b| num(b.cost)),
            Interim(n, finish) => time(t.interim_in(*n).map(|i| if *finish { i.finish } else { i.start })),
            ConstraintDate => time(t.constraint.date),
            Deadline => time(t.deadline),
            PercentComplete => num(f64::from(s.percent_complete)),
            Work => mins(s.work),
            ActualWork => mins(s.actual_work),
            RemainingWork => mins((s.work - s.actual_work).max(0)),
            Cost => num(s.cost),
            FixedCost => num(t.fixed_cost),
            WorkVariance => t.baseline.map_or(SortKey::Missing, |b| mins(s.work - b.work)),
            CostVariance => t.baseline.map_or(SortKey::Missing, |b| num(s.cost - b.cost)),
            FreeSlack => mins(s.free_slack),
            TotalSlack => mins(s.total_slack),
            Priority => num(f64::from(t.priority)),
            // In board order, not by name.
            BoardStatus if !s.is_summary => p
                .board_column_of(idx)
                .and_then(|c| p.board_columns.iter().position(|b| b.id == c))
                .map_or(SortKey::Missing, |k| num(k as f64)),
            ExpectedWork => t.estimate.map_or(SortKey::Missing, |e| mins(e.expected_work(&p.settings))),
            _ => {
                let none = |_: TaskId| false;
                let text = columns::cell(*col, p, idx, &CellCtx { overallocated: &none, others_on: None }).text;
                if text.trim().is_empty() { SortKey::Missing } else { SortKey::Text(text.to_lowercase()) }
            }
        }
    }

    /// The label tasks are grouped under (empty values share the "No value" group).
    pub fn group_label(&self, p: &Project, idx: usize) -> String {
        let text = match self {
            Field::Custom(id) => p.tasks.get(idx).and_then(|t| t.custom.get(id)).map(|v| custom_display(p, *id, v)).unwrap_or_default(),
            Field::Builtin(col) => {
                let none = |_: TaskId| false;
                columns::cell(*col, p, idx, &CellCtx { overallocated: &none, others_on: None }).text
            }
        };
        if text.trim().is_empty() { "No value".to_string() } else { text }
    }
}

/// A custom value as shown in the table (dates and money in the app's formats).
pub fn custom_display(p: &Project, id: CustomFieldId, v: &CustomValue) -> String {
    let cost = p.custom_field(id).is_some_and(|f| f.kind == CustomFieldKind::Cost);
    match v {
        CustomValue::Number(n) if cost => format::money(p, *n),
        CustomValue::Date(d) => format::date(Some(*d)),
        other => p.custom_value_text(id, other),
    }
}

/// The columns a task table shows: the plan's saved layout, or the table's defaults (also when
/// none of the saved columns is known here, so a table never ends up without columns).
pub fn table_fields(p: &Project, table: TaskTable) -> Vec<Field> {
    let saved: Vec<Field> = p
        .table_layout(table.label())
        .map(|layout| layout.columns.iter().filter_map(|k| Field::from_key(p, k)).collect())
        .unwrap_or_default();
    if saved.is_empty() { table.columns().into_iter().map(Field::Builtin).collect() } else { saved }
}

/// Baseline1–10 values and the interim plans (Start1–10, Finish1–10), in that order.
fn generated() -> impl Iterator<Item = TaskCol> {
    let baselines = (1..=MAX_BASELINE).flat_map(|n| BlField::ALL.into_iter().map(move |f| TaskCol::Baseline(n, f)));
    let interim = (1..=MAX_INTERIM).flat_map(|n| [TaskCol::Interim(n, false), TaskCol::Interim(n, true)]);
    baselines.chain(interim)
}

/// The Baseline1–10 or interim plan column a key names ("baseline3_cost", "start4", "finish4").
fn generated_from_key(key: &str) -> Option<TaskCol> {
    let num = |s: &str| -> Option<u8> { s.bytes().all(|b| b.is_ascii_digit()).then(|| s.parse().ok()).flatten() };
    let col = if let Some(rest) = key.strip_prefix("baseline") {
        let (n, field) = rest.split_once('_')?;
        TaskCol::Baseline(num(n)?, BlField::ALL.into_iter().find(|f| f.key() == field)?)
    } else if let Some(n) = key.strip_prefix("start") {
        TaskCol::Interim(num(n)?, false)
    } else {
        TaskCol::Interim(num(key.strip_prefix("finish")?)?, true)
    };
    // Only the generated range (Baseline1–10 and interim plans 1–10), so keys stay unique.
    (generated().any(|c| c == col) && Field::Builtin(col).key() == key).then_some(col)
}

/// Columns the chooser offers beyond a table's own layout: cheap, commonly wanted singles
/// (the Swift app's inspector offers these too).
const CHOOSER_EXTRAS: [TaskCol; 11] = [
    TaskCol::Wbs,
    TaskCol::Status,
    TaskCol::RowMark,
    TaskCol::PercentComplete,
    TaskCol::LevelingDelay,
    TaskCol::Resources,
    TaskCol::Units,
    TaskCol::Work,
    TaskCol::Cost,
    TaskCol::Deadline,
    TaskCol::Notes,
];

/// The columns the table shows for this user: the layout minus the keys switched off in the app
/// settings, plus the ones switched on that the layout lacks. The name column always shows, and
/// an empty result falls back to the full layout (a damaged settings file never empties a table).
pub fn visible_fields(p: &Project, table: TaskTable, choice: Option<&crate::settings::ColumnChoice>) -> Vec<Field> {
    let layout = table_fields(p, table);
    let Some(choice) = choice else { return layout };
    let mut list: Vec<Field> =
        layout.iter().copied().filter(|f| *f == Field::Builtin(TaskCol::Name) || !choice.hidden.iter().any(|k| *k == f.key())).collect();
    for key in &choice.extra {
        if let Some(f) = Field::from_key(p, key)
            && !list.contains(&f)
        {
            list.push(f);
        }
    }
    if list.is_empty() { layout } else { list }
}

/// What the Columns menu lists for `table`: the layout's columns, then the extras and the
/// project's custom fields. Each entry is (key, title, on, required); only the name is required.
pub fn column_choices(p: &Project, table: TaskTable, choice: Option<&crate::settings::ColumnChoice>) -> Vec<(String, String, bool, bool)> {
    let visible = visible_fields(p, table, choice);
    let mut all = table_fields(p, table);
    for f in CHOOSER_EXTRAS.into_iter().map(Field::Builtin).chain(p.custom_fields.iter().map(|f| Field::Custom(f.id))) {
        if !all.contains(&f) {
            all.push(f);
        }
    }
    all.into_iter().map(|f| (f.key(), f.title(p), visible.contains(&f), f == Field::Builtin(TaskCol::Name))).collect()
}

/// Flips one column on or off in this user's choice for `table`. The name column stays on.
pub fn toggle_column(p: &Project, table: TaskTable, choice: &mut crate::settings::ColumnChoice, key: &str) {
    if key == "name" || Field::from_key(p, key).is_none() {
        return;
    }
    if table_fields(p, table).iter().any(|f| f.key() == key) {
        match choice.hidden.iter().position(|k| k == key) {
            Some(i) => {
                choice.hidden.remove(i);
            }
            None => choice.hidden.push(key.to_string()),
        }
    } else {
        match choice.extra.iter().position(|k| k == key) {
            Some(i) => {
                choice.extra.remove(i);
            }
            None => choice.extra.push(key.to_string()),
        }
    }
}

/// Every field a column can show: the built-in columns, the numbered baselines and interim plans,
/// then the custom fields.
pub fn all_fields(p: &Project) -> Vec<Field> {
    BUILTIN
        .iter()
        .map(|(c, _)| *c)
        .chain(generated())
        .map(Field::Builtin)
        .chain(p.custom_fields.iter().map(|f| Field::Custom(f.id)))
        .collect()
}

/// Saves a changed column list for `table` (keys of `fields`).
pub fn save_columns(p: &mut Project, table: TaskTable, fields: &[Field]) -> Result<(), String> {
    let keys: Vec<String> = fields.iter().map(Field::key).collect();
    p.set_table_columns(table.label(), &keys).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_are_unique() {
        let p = plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let fields = all_fields(&p);
        let keys: std::collections::HashSet<String> = fields.iter().map(Field::key).collect();
        assert_eq!(keys.len(), fields.len(), "every field has its own key");
        for f in &fields {
            assert_eq!(Field::from_key(&p, &f.key()), Some(*f));
        }
        assert!(Field::from_key(&p, "custom:999").is_none());
        for bad in [
            "baseline01_cost",
            "baseline0_cost",
            "baseline11_start",
            "baseline3_",
            "baseline3_size",
            "start0",
            "start11",
            "finish00",
            "start+1",
        ] {
            assert!(Field::from_key(&p, bad).is_none(), "{bad}");
        }
        assert_eq!(Field::from_key(&p, "finish10"), Some(Field::Builtin(TaskCol::Interim(10, true))));
        assert!(Field::from_key(&p, "nonsense").is_none());
    }

    #[test]
    fn every_default_table_column_has_a_key() {
        for table in TaskTable::ALL {
            for col in table.columns() {
                assert!(!Field::Builtin(col).key().is_empty(), "{col:?} in {table:?} has no key");
            }
        }
    }

    #[test]
    fn leveling_delay_key_round_trips_and_defaults_include_it() {
        let p = plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let delay = Field::from_key(&p, "leveling_delay").expect("built-in key should resolve");
        assert_eq!(delay.key(), "leveling_delay");

        for table in [TaskTable::Entry, TaskTable::Schedule, TaskTable::Details] {
            let keys: Vec<String> = table_fields(&p, table).into_iter().map(|f| f.key()).collect();
            assert!(keys.iter().any(|k| k == "leveling_delay"), "{table:?} defaults should expose leveling_delay");
        }
    }

    /// Regression: a saved layout naming only unknown columns (a crafted or newer file) gave the
    /// table no columns at all, and the grid's cursor maths panicked on it.
    #[test]
    fn a_saved_layout_without_known_columns_falls_back_to_the_defaults() {
        let mut p = plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let defaults: Vec<Field> = TaskTable::Entry.columns().into_iter().map(Field::Builtin).collect();
        p.set_table_columns(TaskTable::Entry.label(), &["nonsense".to_string(), "from_the_future".to_string()]).unwrap();
        assert_eq!(table_fields(&p, TaskTable::Entry), defaults);

        // A layout with some known columns keeps just those.
        p.set_table_columns(TaskTable::Entry.label(), &["nonsense".to_string(), "name".to_string()]).unwrap();
        assert_eq!(table_fields(&p, TaskTable::Entry), vec![Field::from_key(&p, "name").unwrap()]);
    }

    /// The column chooser: hiding removes a column (never the name), extras append, and the
    /// choice is plain data that a settings round-trip keeps.
    #[test]
    fn column_choice_hides_and_adds_columns_but_never_the_name() {
        let p = plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let table = TaskTable::Entry;
        let mut choice = crate::settings::ColumnChoice::default();
        let default: Vec<String> = visible_fields(&p, table, None).iter().map(Field::key).collect();
        assert!(default.contains(&"duration".to_string()));

        toggle_column(&p, table, &mut choice, "duration");
        let keys: Vec<String> = visible_fields(&p, table, Some(&choice)).iter().map(Field::key).collect();
        assert!(!keys.contains(&"duration".to_string()), "{keys:?}");
        assert_eq!(keys.len(), default.len() - 1);

        // The status column is not in the Entry layout: switching it on appends it.
        toggle_column(&p, table, &mut choice, "status");
        let keys: Vec<String> = visible_fields(&p, table, Some(&choice)).iter().map(Field::key).collect();
        assert_eq!(keys.last().map(String::as_str), Some("status"));
        let listed = column_choices(&p, table, Some(&choice));
        assert!(listed.iter().any(|(k, _, on, _)| k == "status" && *on));
        assert!(listed.iter().any(|(k, _, on, _)| k == "duration" && !*on));
        assert!(listed.iter().any(|(k, _, _, required)| k == "name" && *required));

        // The name cannot be hidden; junk keys change nothing.
        toggle_column(&p, table, &mut choice, "name");
        toggle_column(&p, table, &mut choice, "from_the_future");
        assert!(visible_fields(&p, table, Some(&choice)).contains(&Field::Builtin(TaskCol::Name)));
        assert!(choice.hidden.iter().all(|k| k != "name" && k != "from_the_future"), "{choice:?}");

        // Toggling back restores the defaults exactly.
        toggle_column(&p, table, &mut choice, "duration");
        toggle_column(&p, table, &mut choice, "status");
        assert!(choice.hidden.is_empty() && choice.extra.is_empty());
        let keys: Vec<String> = visible_fields(&p, table, Some(&choice)).iter().map(Field::key).collect();
        assert_eq!(keys, default);

        // A hostile choice hiding everything never empties the table.
        let all_hidden =
            crate::settings::ColumnChoice { hidden: table_fields(&p, table).iter().map(Field::key).collect(), extra: Vec::new() };
        assert!(!visible_fields(&p, table, Some(&all_hidden)).is_empty());
    }

    #[test]
    fn sort_keys_order_numbers_dates_text_then_blanks() {
        let mut keys = [SortKey::Missing, SortKey::Text("b".into()), SortKey::Num(2.0), SortKey::Text("a".into()), SortKey::Num(-1.0)];
        keys.sort_by(SortKey::cmp);
        assert_eq!(keys, [SortKey::Num(-1.0), SortKey::Num(2.0), SortKey::Text("a".into()), SortKey::Text("b".into()), SortKey::Missing]);
    }

    #[test]
    fn leveling_delay_sort_key_is_numeric_minutes() {
        let mut p = Project::new("Sort", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let a = p.add_task("A", None);
        let b = p.add_task("B", None);
        let c = p.add_task("C", None);
        p.task_mut(a).expect("task a").leveling_delay = 12 * 480;
        p.task_mut(b).expect("task b").leveling_delay = 0;
        p.task_mut(c).expect("task c").leveling_delay = 2 * 480;

        let f = Field::from_key(&p, "leveling_delay").expect("delay field");
        let mut rows = [(0usize, f.sort_key(&p, 0)), (1usize, f.sort_key(&p, 1)), (2usize, f.sort_key(&p, 2))];
        rows.sort_by(|(_, a), (_, b)| a.cmp(b));

        assert_eq!(rows[0].0, p.task_index(b).expect("b index"));
        assert_eq!(rows[1].0, p.task_index(c).expect("c index"));
        assert_eq!(rows[2].0, p.task_index(a).expect("a index"));
    }
}
