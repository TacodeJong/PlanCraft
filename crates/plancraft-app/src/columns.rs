//! Task table columns: how each field is displayed and edited.

use plancraft_core::dates::prelude::*;
use plancraft_core::prelude::*;
use plancraft_core::report;

use crate::format;
use crate::theme;
use crate::widgets::grid::{Cell, ColumnDef, Glyph};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskCol {
    Indicators,
    Mode,
    Wbs,
    Name,
    Duration,
    Start,
    Finish,
    Predecessors,
    Successors,
    Resources,
    PercentComplete,
    /// The workflow status (not started, started, in progress, done).
    Status,
    /// The row's coloured mark.
    RowMark,
    Work,
    ActualWork,
    RemainingWork,
    Cost,
    FixedCost,
    /// A value of Baseline (0) or Baseline1–10.
    Baseline(u8, BlField),
    /// Interim plan 1–10: its start (`false`) or finish (`true`), the Start*n* and Finish*n* fields.
    Interim(u8, bool),
    StartVariance,
    FinishVariance,
    WorkVariance,
    CostVariance,
    ActualStart,
    ActualFinish,
    ActualDuration,
    RemainingDuration,
    Stop,
    Resume,
    EarlyStart,
    EarlyFinish,
    LateStart,
    LateFinish,
    FreeSlack,
    TotalSlack,
    ConstraintType,
    ConstraintDate,
    Deadline,
    Priority,
    TaskType,
    EstimateMethod,
    Points,
    Optimistic,
    MostLikely,
    Pessimistic,
    ExpectedWork,
    Deliverable,
    Milestones,
    Notes,
    UniqueId,
    /// Yes for a recurring task and its occurrences.
    Recurring,
    /// The Task Board column a task's card is in.
    BoardStatus,
    /// Whether the task has a card on the Task Board.
    ShowOnBoard,
}

/// The values a baseline holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlField {
    Start,
    Finish,
    Duration,
    Work,
    Cost,
}

impl BlField {
    pub const ALL: [BlField; 5] = [BlField::Start, BlField::Finish, BlField::Duration, BlField::Work, BlField::Cost];

    pub fn label(self) -> &'static str {
        match self {
            BlField::Start => "Start",
            BlField::Finish => "Finish",
            BlField::Duration => "Duration",
            BlField::Work => "Work",
            BlField::Cost => "Cost",
        }
    }

    /// The key suffix in table layouts ("start", "finish", ...).
    pub fn key(self) -> &'static str {
        match self {
            BlField::Start => "start",
            BlField::Finish => "finish",
            BlField::Duration => "duration",
            BlField::Work => "work",
            BlField::Cost => "cost",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskTable {
    Entry,
    Schedule,
    Tracking,
    Work,
    Cost,
    Estimates,
    Variance,
    Summary,
    Details,
}

impl TaskTable {
    pub const ALL: [TaskTable; 9] = [
        TaskTable::Entry,
        TaskTable::Schedule,
        TaskTable::Tracking,
        TaskTable::Work,
        TaskTable::Cost,
        TaskTable::Estimates,
        TaskTable::Variance,
        TaskTable::Summary,
        TaskTable::Details,
    ];

    /// English name stored with a custom column layout. Display uses [`Self::title`].
    pub fn label(self) -> &'static str {
        match self {
            TaskTable::Entry => "Entry",
            TaskTable::Schedule => "Schedule",
            TaskTable::Tracking => "Tracking",
            TaskTable::Work => "Work",
            TaskTable::Cost => "Cost",
            TaskTable::Estimates => "Estimates",
            TaskTable::Variance => "Variance",
            TaskTable::Summary => "Summary",
            TaskTable::Details => "Details",
        }
    }

    pub fn title(self) -> &'static str {
        crate::lang::text(match self {
            TaskTable::Entry => "table.entry",
            TaskTable::Schedule => "table.schedule",
            TaskTable::Tracking => "table.tracking",
            TaskTable::Work => "table.work",
            TaskTable::Cost => "table.cost",
            TaskTable::Estimates => "table.estimates",
            TaskTable::Variance => "table.variance",
            TaskTable::Summary => "table.summary",
            TaskTable::Details => "table.details",
        })
    }

    pub fn columns(self) -> Vec<TaskCol> {
        use TaskCol::*;
        match self {
            TaskTable::Entry => vec![Indicators, Mode, Name, Duration, Start, Finish, Predecessors, Resources],
            TaskTable::Schedule => vec![
                Indicators,
                Name,
                Start,
                Finish,
                LateStart,
                LateFinish,
                FreeSlack,
                TotalSlack,
                ConstraintType,
                ConstraintDate,
                Deadline,
            ],
            TaskTable::Tracking => {
                vec![
                    Indicators,
                    Name,
                    ActualStart,
                    ActualFinish,
                    PercentComplete,
                    ActualDuration,
                    RemainingDuration,
                    ActualWork,
                    RemainingWork,
                    Cost,
                ]
            }
            TaskTable::Work => {
                vec![Indicators, Name, Work, Baseline(0, BlField::Work), WorkVariance, ActualWork, RemainingWork, PercentComplete]
            }
            TaskTable::Cost => vec![Indicators, Name, FixedCost, Cost, Baseline(0, BlField::Cost), CostVariance],
            TaskTable::Estimates => {
                vec![Indicators, Name, EstimateMethod, Points, Optimistic, MostLikely, Pessimistic, ExpectedWork, Work, Duration, Resources]
            }
            TaskTable::Variance => vec![
                Indicators,
                Name,
                Start,
                Finish,
                Baseline(0, BlField::Start),
                Baseline(0, BlField::Finish),
                StartVariance,
                FinishVariance,
            ],
            TaskTable::Summary => vec![Indicators, Wbs, Name, Duration, Start, Finish, PercentComplete, Cost, Work, Milestones],
            TaskTable::Details => {
                vec![Indicators, UniqueId, Name, TaskType, Priority, EarlyStart, EarlyFinish, Predecessors, Successors, Deliverable, Notes]
            }
        }
    }
}

impl TaskCol {
    pub fn def(self, p: &Project) -> ColumnDef {
        use TaskCol::*;
        let _ = p;
        match self {
            Indicators => ColumnDef::new("ℹ", 34.0).read_only().center(),
            Mode => ColumnDef::new(ct("col.mode"), 46.0).center().choice(&["Auto Scheduled", "Manually Scheduled"]),
            Wbs => ColumnDef::new(ct("col.wbs"), 50.0).read_only(),
            Name => ColumnDef::new(ct("col.name"), 230.0),
            Duration => ColumnDef::new(ct("col.duration"), 76.0).right(),
            Start => ColumnDef::new(ct("col.start"), 96.0),
            Finish => ColumnDef::new(ct("col.finish"), 96.0),
            Predecessors => ColumnDef::new(ct("col.predecessors"), 96.0),
            Successors => ColumnDef::new(ct("col.successors"), 96.0).read_only(),
            Resources => ColumnDef::new(ct("col.resources"), 160.0),
            PercentComplete => ColumnDef::new(ct("col.percent"), 62.0).right(),
            Status => {
                let names = TaskStatus::ALL.map(crate::lang::status_name);
                ColumnDef::new(ct("col.status"), 96.0).choice(&names)
            }
            RowMark => {
                let mut names = vec![crate::lang::text("common.none")];
                names.extend(plancraft_core::model::RowMark::ALL.map(crate::lang::mark_name));
                ColumnDef::new(ct("col.row_mark"), 86.0).choice(&names)
            }
            Work => ColumnDef::new(ct("col.work"), 70.0).right(),
            ActualWork => ColumnDef::new(ct("col.actual_work"), 72.0).right().read_only(),
            RemainingWork => ColumnDef::new(ct("col.remaining_work"), 72.0).right().read_only(),
            Cost => ColumnDef::new(ct("col.cost"), 90.0).right().read_only(),
            FixedCost => ColumnDef::new(ct("col.fixed_cost"), 86.0).right(),
            Baseline(n, f) => {
                let title = format!("{} {}", PlanSlot::Baseline(n).name(), f.label());
                match f {
                    BlField::Start | BlField::Finish => ColumnDef::new(title, 106.0).read_only(),
                    _ => ColumnDef::new(title, 96.0).right().read_only(),
                }
            }
            Interim(n, finish) => ColumnDef::new(format!("{}{n}", if finish { "Finish" } else { "Start" }), 96.0).read_only(),
            StartVariance => ColumnDef::new(ct("col.start_var"), 76.0).right().read_only(),
            FinishVariance => ColumnDef::new(ct("col.finish_var"), 76.0).right().read_only(),
            WorkVariance => ColumnDef::new(ct("col.work_var"), 72.0).right().read_only(),
            CostVariance => ColumnDef::new(ct("col.cost_var"), 86.0).right().read_only(),
            ActualStart => ColumnDef::new(ct("col.actual_start"), 100.0),
            ActualFinish => ColumnDef::new(ct("col.actual_finish"), 100.0),
            ActualDuration => ColumnDef::new(ct("col.actual_duration"), 76.0).right(),
            RemainingDuration => ColumnDef::new(ct("col.remaining_duration"), 76.0).right(),
            Stop => ColumnDef::new(ct("col.stop"), 100.0).read_only(),
            Resume => ColumnDef::new(ct("col.resume"), 100.0),
            EarlyStart => ColumnDef::new(ct("col.early_start"), 100.0).read_only(),
            EarlyFinish => ColumnDef::new(ct("col.early_finish"), 100.0).read_only(),
            LateStart => ColumnDef::new(ct("col.late_start"), 100.0).read_only(),
            LateFinish => ColumnDef::new(ct("col.late_finish"), 100.0).read_only(),
            FreeSlack => ColumnDef::new(ct("col.free_slack"), 76.0).right().read_only(),
            TotalSlack => ColumnDef::new(ct("col.total_slack"), 76.0).right().read_only(),
            ConstraintType => ColumnDef::new(ct("col.constraint"), 150.0).choice(&ConstraintKind::ALL.map(|c| c.label())),
            ConstraintDate => ColumnDef::new(ct("col.constraint_date"), 104.0),
            Deadline => ColumnDef::new(ct("col.deadline"), 100.0),
            Priority => ColumnDef::new(ct("col.priority"), 60.0).right(),
            TaskType => ColumnDef::new(ct("col.type"), 100.0).choice(&["Fixed Units", "Fixed Duration", "Fixed Work"]),
            EstimateMethod => ColumnDef::new(ct("col.estimate"), 96.0).choice(&["None", "Complexity", "Three-point", "Work"]),
            Points => ColumnDef::new(ct("col.points"), 58.0).right(),
            Optimistic => ColumnDef::new(ct("col.optimistic"), 80.0).right(),
            MostLikely => ColumnDef::new(ct("col.likely"), 80.0).right(),
            Pessimistic => ColumnDef::new(ct("col.pessimistic"), 80.0).right(),
            ExpectedWork => ColumnDef::new(ct("col.expected"), 90.0).right().read_only(),
            Deliverable => ColumnDef::new(ct("col.deliverable"), 160.0),
            Milestones => ColumnDef::new(ct("col.milestones"), 140.0).read_only(),
            Notes => ColumnDef::new(ct("col.notes"), 200.0),
            UniqueId => ColumnDef::new(ct("col.unique"), 64.0).right().read_only(),
            Recurring => ColumnDef::new(ct("col.recurring"), 70.0).center().read_only(),
            BoardStatus => {
                let names: Vec<&str> = p.board_columns.iter().map(|c| c.name.as_str()).collect();
                ColumnDef::new(ct("col.board"), 110.0).choice(&names)
            }
            ShowOnBoard => ColumnDef::new(ct("col.show_board"), 92.0).center().choice(&["Yes", "No"]),
        }
    }
}

fn ct(key: &'static str) -> &'static str {
    crate::lang::text(key)
}

fn var_text(p: &Project, m: Minutes) -> String {
    if m == 0 { p.format_duration(0, false) } else { format!("{}{}", if m > 0 { "+" } else { "-" }, p.format_duration(m.abs(), false)) }
}

/// Indicator glyphs for a task.
/// `others`: who else is on it, in a plan opened from a server.
pub fn indicators(
    p: &Project,
    t: &Task,
    overallocated: &dyn Fn(TaskId) -> bool,
    others: Option<&str>,
) -> Vec<(Glyph, egui::Color32, String)> {
    let mut v = Vec::new();
    if let Some(names) = others {
        // A warning, not a lock: they may be changing it right now.
        v.push((Glyph::Icon("users-round"), theme::pal().ACCENT_MID, format!("Also on this task: {names}")));
    }
    if t.is_complete() {
        v.push((Glyph::Icon("check"), theme::pal().OK, "This task was completed".into()));
    }
    if !t.sched.warnings.is_empty() {
        v.push((Glyph::Icon("triangle-alert"), theme::pal().ERROR, t.sched.warnings.join("\n")));
    } else if t.deadline.is_some() {
        v.push((Glyph::Icon("flag"), theme::pal().DEADLINE, format!("Deadline: {}", format::date(t.deadline))));
    }
    if t.constraint.kind.needs_date() {
        v.push((
            Glyph::Icon("pin"),
            theme::pal().ACCENT_MID,
            format!("{} constraint on {}", t.constraint.kind.label(), format::date(t.constraint.date)),
        ));
    }
    if overallocated(t.id) {
        v.push((Glyph::Icon("user-round"), theme::pal().ERROR, "A resource assigned to this task is overallocated".into()));
    }
    if t.predecessors.iter().any(|d| matches!(d.predecessor, TaskRef::External { .. }))
        || t.members.iter().any(|m| matches!(m, TaskRef::External { .. }))
    {
        v.push((Glyph::Icon("link"), theme::pal().ACCENT_MID, "Linked to a task in another project".into()));
    }
    if !t.notes.is_empty() {
        v.push((Glyph::Note, theme::pal().TEXT_WEAK, t.notes.clone()));
    }
    if !t.active {
        v.push((Glyph::Icon("pause"), theme::pal().TEXT_WEAK, "Inactive task".into()));
    }
    if t.recurrence.is_some() {
        let n = p.task_index(t.id).map_or(0, |i| p.descendants_range(i).len());
        v.insert(0, (Glyph::Icon("repeat"), theme::pal().ACCENT_MID, format!("Recurring task with {n} occurrences")));
    }
    let _ = p;
    v.truncate(2);
    v
}

pub struct CellCtx<'a> {
    pub overallocated: &'a dyn Fn(TaskId) -> bool,
    /// In a plan opened from a server: who else is on each task (names, joined).
    pub others_on: Option<&'a std::collections::HashMap<TaskId, String>>,
}

/// Cell content of `col` for the task at `idx`.
pub fn cell(col: TaskCol, p: &Project, idx: usize, ctx: &CellCtx) -> Cell {
    use TaskCol::*;
    let Some(t) = p.tasks.get(idx) else { return Cell::default() };
    let s = &t.sched;
    let summary = s.is_summary;
    let mut c = match col {
        Indicators => {
            let others = ctx.others_on.and_then(|m| m.get(&t.id)).map(String::as_str);
            Cell { icons: indicators(p, t, ctx.overallocated, others), ..Default::default() }
        }
        Mode => {
            let (g, tip) = match t.mode {
                ScheduleMode::Auto => ("rotate-cw", "Auto Scheduled"),
                ScheduleMode::Manual => ("pin", "Manually Scheduled"),
            };
            Cell {
                icons: vec![(Glyph::Icon(g), theme::pal().ACCENT_MID, tip.into())],
                tooltip: Some(tip.into()),
                edit_text: Some(tip.into()),
                ..Default::default()
            }
        }
        Wbs => Cell::weak(p.wbs(idx)),
        Name => {
            let indent = (t.outline_level.saturating_sub(1)) as f32 * 16.0;
            let mut icons = Vec::new();
            if let Some(mark) = t.row_mark {
                icons.push((Glyph::Dot, theme::row_mark_color(mark), crate::lang::mark_name(mark).into()));
            }
            if t.is_milestone() {
                icons.push((Glyph::Diamond, theme::pal().MILESTONE, crate::lang::text("rpt.milestone").into()));
            }
            if summary && let Some(rgb) = t.bar_color {
                icons.push((Glyph::Swatch, theme::paint_rgb(rgb), crate::lang::text("prop.group_color").into()));
            }
            let mut cell = Cell {
                text: t.name.clone(),
                bold: summary,
                indent,
                expander: if summary { Some(true) } else { None },
                icons,
                ..Default::default()
            };
            if summary && let Some(rgb) = t.bar_color {
                cell.color = Some(theme::paint_rgb(rgb));
            }
            cell
        }
        Duration => Cell::text(p.format_task_duration(t)),
        Start => Cell { text: format::date(s.start), edit_text: Some(format::iso_date(s.start)), ..Default::default() },
        Finish => Cell { text: format::date(s.finish), edit_text: Some(format::iso_date(s.finish)), ..Default::default() },
        Predecessors => Cell::text(p.format_predecessors(t.id)),
        Successors => Cell::text(
            p.successors(t.id).iter().filter_map(|(sid, _)| p.row_number(*sid)).map(|r| r.to_string()).collect::<Vec<_>>().join(","),
        ),
        Resources => Cell::text(p.format_resource_names(t.id)),
        PercentComplete => Cell::text(format::percent(s.percent_complete)),
        Status => {
            // A group's status rolls up from its subtasks, like its percent.
            let status = if summary {
                if s.percent_complete >= 100.0 {
                    TaskStatus::Done
                } else if s.percent_complete <= 0.0 {
                    TaskStatus::NotStarted
                } else {
                    TaskStatus::Busy
                }
            } else {
                t.status
            };
            Cell::text(crate::lang::status_name(status))
        }
        RowMark => match t.row_mark {
            None => Cell::default(),
            Some(m) => Cell {
                text: crate::lang::mark_name(m).into(),
                icons: vec![(Glyph::Dot, theme::row_mark_color(m), crate::lang::mark_name(m).into())],
                ..Default::default()
            },
        },
        Work => Cell::text(format_work(s.work)),
        ActualWork => Cell::text(format_work(s.actual_work)),
        RemainingWork => Cell::text(format_work((s.work - s.actual_work).max(0))),
        Cost => Cell::text(format::money(p, s.cost)),
        FixedCost => Cell::text(format::money(p, t.fixed_cost)),
        Baseline(n, f) => Cell::text(match (t.baseline_in(n), f) {
            (Some(b), BlField::Start) => format::date(Some(b.start)),
            (Some(b), BlField::Finish) => format::date(Some(b.finish)),
            (Some(b), BlField::Duration) => p.format_task_minutes(t, b.duration, false),
            (Some(b), BlField::Work) => format_work(b.work),
            (Some(b), BlField::Cost) => format::money(p, b.cost),
            (None, BlField::Start | BlField::Finish) => "NA".into(),
            (None, _) => String::new(),
        }),
        Interim(n, finish) => {
            Cell::text(t.interim_in(n).map_or_else(|| "NA".into(), |i| format::date(Some(if finish { i.finish } else { i.start }))))
        }
        StartVariance | FinishVariance => {
            let wc = plancraft_core::schedule::project_calendar(p);
            let v = match (t.baseline, s.start, s.finish) {
                (Some(b), Some(st), Some(fi)) => {
                    Some(if col == StartVariance { wc.between(b.start, st) } else { wc.between(b.finish, fi) })
                }
                _ => None,
            };
            match v {
                Some(m) => Cell {
                    text: var_text(p, m),
                    color: Some(if m > 0 { theme::pal().ERROR } else { theme::pal().TEXT }),
                    ..Default::default()
                },
                None => Cell::default(),
            }
        }
        WorkVariance => t.baseline.map(|b| Cell::text(format_work(s.work - b.work))).unwrap_or_default(),
        CostVariance => t.baseline.map(|b| Cell::text(format::money(p, s.cost - b.cost))).unwrap_or_default(),
        ActualStart => Cell { text: format::date(t.actual_start), edit_text: Some(format::iso_date(t.actual_start)), ..Default::default() },
        ActualFinish => {
            Cell { text: format::date(t.actual_finish), edit_text: Some(format::iso_date(t.actual_finish)), ..Default::default() }
        }
        ActualDuration | RemainingDuration => {
            let (done, left) = t.duration_progress();
            Cell::text(p.format_task_minutes(t, if col == ActualDuration { done } else { left }, false))
        }
        Stop => Cell::text(format::date(s.stop)),
        Resume => Cell { text: format::date(s.resume), edit_text: Some(format::iso_date(s.resume)), ..Default::default() },
        EarlyStart => Cell::text(format::date(s.early_start)),
        EarlyFinish => Cell::text(format::date(s.early_finish)),
        LateStart => Cell::text(format::date(s.late_start)),
        LateFinish => Cell::text(format::date(s.late_finish)),
        FreeSlack => Cell::text(format::slack(p, s.free_slack)),
        TotalSlack => Cell {
            text: format::slack(p, s.total_slack),
            color: if s.total_slack < 0 { Some(theme::pal().ERROR) } else { None },
            ..Default::default()
        },
        ConstraintType => Cell::text(t.constraint.kind.label()),
        ConstraintDate => {
            Cell { text: format::date(t.constraint.date), edit_text: Some(format::iso_date(t.constraint.date)), ..Default::default() }
        }
        Deadline => Cell { text: format::date(t.deadline), edit_text: Some(format::iso_date(t.deadline)), ..Default::default() },
        Priority => Cell::text(t.priority.to_string()),
        TaskType => Cell::text(match t.task_type {
            plancraft_core::model::TaskType::FixedUnits => "Fixed Units",
            plancraft_core::model::TaskType::FixedDuration => "Fixed Duration",
            plancraft_core::model::TaskType::FixedWork => "Fixed Work",
        }),
        EstimateMethod => Cell::text(match t.estimate {
            None => "None",
            Some(Estimate::Complexity { .. }) => "Complexity",
            Some(Estimate::ThreePoint { .. }) => "Three-point",
            Some(Estimate::Work { .. }) => "Work",
        }),
        Points => match t.estimate {
            Some(Estimate::Complexity { points }) => Cell::text(format!("{points}")),
            _ => Cell::default(),
        },
        Optimistic | MostLikely | Pessimistic => match t.estimate {
            Some(Estimate::ThreePoint { optimistic, most_likely, pessimistic }) => Cell::text(format_work(match col {
                Optimistic => optimistic,
                MostLikely => most_likely,
                _ => pessimistic,
            })),
            _ => Cell::default(),
        },
        ExpectedWork => t.estimate.map(|e| Cell::text(format_work(e.expected_work(&p.settings)))).unwrap_or_default(),
        Deliverable => Cell::text(t.deliverable.clone()),
        Milestones => {
            Cell::weak(p.milestones_of(t.id).iter().filter_map(|m| p.task(*m)).map(|m| m.name.clone()).collect::<Vec<_>>().join(", "))
        }
        Notes => Cell::text(t.notes.lines().next().unwrap_or_default().to_string()),
        UniqueId => Cell::weak(t.id.0.to_string()),
        Recurring => Cell::text(if p.is_recurring(idx) { "Yes" } else { "No" }),
        // Summary tasks have no card; their subtasks do.
        BoardStatus if summary => Cell::default(),
        BoardStatus => Cell::text(p.board_column_of(idx).and_then(|c| p.board_column(c)).map(|c| c.name.clone()).unwrap_or_default()),
        ShowOnBoard => Cell::text(if t.show_on_board { "Yes" } else { "No" }),
    };
    if summary && col != Name && col != Indicators {
        c.bold = true;
    }
    if !t.active {
        c.color = Some(theme::pal().TEXT_WEAK);
        c.strike = col == Name;
    }
    if col == Name && t.sched.critical && t.mode == ScheduleMode::Auto && !summary {
        // Critical tasks are shown in the chart; keep table readable.
    }
    c
}

fn parse_date(p: &Project, text: &str, finish: bool) -> Result<Option<DateTime>, String> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("na") {
        return Ok(None);
    }
    p.parse_datetime(t, finish).map(Some).ok_or_else(|| format!("'{t}' is not a valid date (use YYYY-MM-DD)"))
}

fn parse_work_text(p: &Project, text: &str) -> Result<Minutes, String> {
    parse_work(text, &p.settings.time).ok_or_else(|| format!("'{text}' is not a valid amount of work"))
}

fn task_mut(p: &mut Project, id: TaskId) -> Result<&mut Task, String> {
    p.task_mut(id).ok_or_else(|| "task not found".to_string())
}

/// Applies an edited value to a task. Runs inside an undoable edit.
pub fn commit(col: TaskCol, p: &mut Project, id: TaskId, text: &str) -> Result<(), String> {
    use TaskCol::*;
    let idx = p.task_index(id).ok_or("task not found")?;
    let summary = p.has_children(idx);
    let err = |e: PlanError| e.to_string();
    match col {
        Name => {
            if text.trim().is_empty() {
                return Err("task name cannot be empty".into());
            }
            task_mut(p, id)?.name = text.trim().to_string();
        }
        Mode => {
            let mode = if text.starts_with('M') { ScheduleMode::Manual } else { ScheduleMode::Auto };
            p.set_schedule_mode(&[id], mode).map_err(err)?;
        }
        Duration => {
            let typed = p.parse_task_duration(text).ok_or_else(|| format!("'{text}' is not a valid duration"))?;
            p.set_typed_duration(id, typed).map_err(err)?;
        }
        Start | Finish => {
            let finish = col == Finish;
            let date = parse_date(p, text, finish)?.ok_or("a date is required")?;
            let t = task_mut(p, id)?;
            // A date typed on a summary makes it manually scheduled (top-down planning).
            if summary || t.mode == ScheduleMode::Manual {
                p.set_manual_date(id, date, finish).map_err(err)?;
            } else {
                t.constraint = Constraint { kind: if finish { ConstraintKind::Fnet } else { ConstraintKind::Snet }, date: Some(date) };
            }
        }
        Predecessors => {
            let deps = p.parse_predecessors(text).map_err(err)?;
            p.set_predecessors(id, deps).map_err(err)?;
        }
        BoardStatus => {
            if summary {
                return Err("summary tasks have no card on the board; their subtasks do".into());
            }
            if text.trim().is_empty() {
                // Cleared: the card goes back to the first column.
                task_mut(p, id)?.board_status = None;
                return Ok(());
            }
            let col = p.find_board_column(text).ok_or_else(|| format!("there is no board column named '{}'", text.trim()))?;
            p.set_board_status(&[id], col).map_err(err)?;
        }
        ShowOnBoard => {
            let yes = text.trim().to_lowercase();
            let show = match yes.as_str() {
                "yes" | "y" | "true" => true,
                "no" | "n" | "false" => false,
                _ => return Err("Show on Board is Yes or No".into()),
            };
            p.set_show_on_board(&[id], show).map_err(err)?;
        }
        Resources => p.apply_resource_names(id, text).map_err(err)?,
        Status => {
            let status = TaskStatus::ALL
                .into_iter()
                .find(|s| crate::lang::status_name(*s) == text || s.label() == text)
                .ok_or_else(|| format!("'{text}' is not a status"))?;
            p.set_task_status(id, status).map_err(err)?;
        }
        RowMark => {
            let mark = if text.trim().is_empty() || text == crate::lang::text("common.none") || text == "None" {
                None
            } else {
                Some(
                    plancraft_core::model::RowMark::ALL
                        .into_iter()
                        .find(|m| crate::lang::mark_name(*m) == text)
                        .ok_or_else(|| format!("'{text}' is not a row mark"))?,
                )
            };
            task_mut(p, id)?.row_mark = mark;
        }
        PercentComplete => {
            let v: f64 = text.trim().trim_end_matches('%').trim().parse().map_err(|_| format!("'{text}' is not a percentage"))?;
            p.set_percent_complete(id, v.clamp(0.0, 100.0).round() as u8).map_err(err)?;
        }
        Work => {
            if summary {
                return Err("summary work is calculated from subtasks".into());
            }
            let w = parse_work_text(p, text)?;
            let assignments: Vec<(AssignmentId, Minutes)> = p
                .assignments_for_task(id)
                .filter(|a| p.resource(a.resource).is_some_and(|r| r.kind == ResourceType::Work))
                .map(|a| (a.id, a.work))
                .collect();
            if assignments.is_empty() {
                task_mut(p, id)?.estimate = Some(Estimate::Work { minutes: w });
            } else {
                let total: Minutes = assignments.iter().map(|(_, w)| w).sum();
                for (aid, aw) in &assignments {
                    let share = if total > 0 { *aw as f64 / total as f64 } else { 1.0 / assignments.len() as f64 };
                    p.set_assignment_work(*aid, (w as f64 * share).round() as Minutes).map_err(err)?;
                }
            }
        }
        FixedCost => task_mut(p, id)?.fixed_cost = format::parse_money(text).ok_or("invalid amount")?,
        ActualStart => {
            let d = parse_date(p, text, false)?;
            set_actual_start(p, id, d)?;
        }
        ActualDuration | RemainingDuration => {
            let m = p.parse_task_minutes(id, text).map_err(err)?;
            if col == ActualDuration { p.set_actual_duration(id, m) } else { p.set_remaining_duration(id, m) }.map_err(err)?;
        }
        Resume => {
            let d = parse_date(p, text, false)?;
            p.set_resume(id, d).map_err(err)?;
        }
        ActualFinish => {
            let d = parse_date(p, text, true)?;
            set_actual_finish(p, id, d)?;
        }
        ConstraintType => {
            let kind = ConstraintKind::ALL.into_iter().find(|k| k.label() == text).unwrap_or_default();
            let t = task_mut(p, id)?;
            t.constraint.kind = kind;
            if kind.needs_date() && t.constraint.date.is_none() {
                t.constraint.date = if kind.is_start_constraint() { t.sched.start } else { t.sched.finish };
            }
            if !kind.needs_date() {
                t.constraint.date = None;
            }
        }
        ConstraintDate => {
            let finish = !task_mut(p, id)?.constraint.kind.is_start_constraint();
            let d = parse_date(p, text, finish)?;
            let t = task_mut(p, id)?;
            t.constraint.date = d;
            if d.is_some() && !t.constraint.kind.needs_date() {
                t.constraint.kind = ConstraintKind::Snet;
            }
            if d.is_none() {
                t.constraint.kind = ConstraintKind::Asap;
            }
        }
        Deadline => task_mut(p, id)?.deadline = parse_date(p, text, true)?,
        Priority => task_mut(p, id)?.priority = text.trim().parse::<u16>().map_err(|_| "priority must be 0-1000")?.min(1000),
        TaskType => {
            task_mut(p, id)?.task_type = match text {
                "Fixed Duration" => plancraft_core::model::TaskType::FixedDuration,
                "Fixed Work" => plancraft_core::model::TaskType::FixedWork,
                _ => plancraft_core::model::TaskType::FixedUnits,
            }
        }
        EstimateMethod => {
            let t = task_mut(p, id)?;
            let work_guess = t.sched.work.max(t.duration);
            t.estimate = match text {
                "Complexity" => Some(Estimate::Complexity { points: 3.0 }),
                "Three-point" => {
                    Some(Estimate::ThreePoint { optimistic: work_guess * 3 / 4, most_likely: work_guess, pessimistic: work_guess * 3 / 2 })
                }
                "Work" => Some(Estimate::Work { minutes: work_guess }),
                _ => None,
            };
        }
        Points => {
            let v: f64 = text.trim().parse().map_err(|_| "points must be a number")?;
            task_mut(p, id)?.estimate = Some(Estimate::Complexity { points: v.max(0.0) });
        }
        Optimistic | MostLikely | Pessimistic => {
            let w = parse_work_text(p, text)?;
            let t = task_mut(p, id)?;
            let (mut o, mut m, mut pe) = match t.estimate {
                Some(Estimate::ThreePoint { optimistic, most_likely, pessimistic }) => (optimistic, most_likely, pessimistic),
                _ => (w, w, w),
            };
            match col {
                Optimistic => o = w,
                MostLikely => m = w,
                _ => pe = w,
            }
            t.estimate = Some(Estimate::ThreePoint { optimistic: o, most_likely: m, pessimistic: pe });
        }
        Deliverable => task_mut(p, id)?.deliverable = text.to_string(),
        Notes => {
            // The cell shows and edits only the first line; keep the rest of a longer note.
            let t = task_mut(p, id)?;
            t.notes = match t.notes.split_once('\n') {
                Some((_, rest)) if text.is_empty() => rest.to_string(),
                Some((_, rest)) => format!("{text}\n{rest}"),
                None => text.to_string(),
            };
        }
        _ => return Err("this field is calculated and cannot be edited".into()),
    }
    Ok(())
}

/// Late-task check used by filters and indicators.
pub fn is_late(p: &Project, t: &Task) -> bool {
    report::is_late(t, report::status_moment(p), p)
}

/// Sets (or with `None` clears) a task's Actual Start. A task that shows progress has started,
/// so its actual start can only be cleared once its % complete is 0 and it has no actual finish
/// (clearing it then would leave "half done, never started").
pub(crate) fn set_actual_start(p: &mut Project, id: TaskId, d: Option<DateTime>) -> Result<(), String> {
    let t = task_mut(p, id)?;
    if d.is_none() && (t.percent_complete > 0 || t.actual_finish.is_some()) {
        return Err("This task shows progress, so it has started: set % complete to 0 first to clear its Actual Start".into());
    }
    t.actual_start = d;
    Ok(())
}

/// Sets (or with `None` clears) a task's Actual Finish. Finishing makes it 100% complete and,
/// if it had no actual start, starts it on its scheduled start. A task that is 100% complete has
/// finished, so its actual finish can only be cleared once its % complete is lower.
pub(crate) fn set_actual_finish(p: &mut Project, id: TaskId, d: Option<DateTime>) -> Result<(), String> {
    let t = task_mut(p, id)?;
    if d.is_none() && t.actual_finish.is_some() && t.percent_complete >= 100 {
        return Err("This task is 100% complete, so it has finished: lower % complete first to clear its Actual Finish".into());
    }
    t.actual_finish = d;
    if d.is_some() {
        t.percent_complete = 100;
        if t.actual_start.is_none() {
            t.actual_start = t.sched.start;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The main table renders every date `dd-mm-yyyy`, like the pane and the date pickers
    /// (it used to say `Thu 10/15/26`).
    #[test]
    fn table_dates_render_dd_mm_yyyy() {
        let p = plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 15).unwrap());
        let none = |_: TaskId| false;
        let ctx = CellCtx { overallocated: &none, others_on: None };
        let idx = p.tasks.iter().position(|t| t.sched.start.is_some()).unwrap();
        let start = p.tasks[idx].sched.start.unwrap();
        for col in [TaskCol::Start, TaskCol::Finish] {
            let text = cell(col, &p, idx, &ctx).text;
            let b = text.as_bytes();
            assert_eq!((text.len(), b[2], b[5]), (10, b'-', b'-'), "{col:?} is not dd-mm-yyyy: {text}");
        }
        assert_eq!(cell(TaskCol::Start, &p, idx, &ctx).text, plancraft_core::dates::dmy(start.date()));
    }

    /// The Status and Row Mark columns (for the column chooser) show and edit their fields.
    #[test]
    fn status_and_row_mark_columns_show_and_edit() {
        use plancraft_core::model::RowMark as Mark;
        let mut p = Project::new("S", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let id = p.add_task("Task", None);
        let idx = p.task_index(id).unwrap();
        let none = |_: TaskId| false;
        let ctx = CellCtx { overallocated: &none, others_on: None };
        commit(TaskCol::Status, &mut p, id, crate::lang::status_name(TaskStatus::Done)).unwrap();
        assert_eq!(p.task(id).unwrap().percent_complete, 100, "done sets the percent too");
        plancraft_core::schedule::recalculate(&mut p);
        assert_eq!(cell(TaskCol::Status, &p, idx, &ctx).text, crate::lang::status_name(TaskStatus::Done));
        commit(TaskCol::RowMark, &mut p, id, crate::lang::mark_name(Mark::Blue)).unwrap();
        assert_eq!(p.task(id).unwrap().row_mark, Some(Mark::Blue));
        assert_eq!(cell(TaskCol::RowMark, &p, idx, &ctx).text, crate::lang::mark_name(Mark::Blue));
        commit(TaskCol::RowMark, &mut p, id, "").unwrap();
        assert_eq!(p.task(id).unwrap().row_mark, None);
        assert!(commit(TaskCol::Status, &mut p, id, "nonsense").is_err());
    }

    /// Regression: the Notes cell shows only the first line, and committing it used to replace
    /// the whole note with that line.
    #[test]
    fn editing_the_notes_cell_keeps_the_other_lines() {
        let mut p = Project::new("Notes", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let id = p.add_task("Task", None);
        p.task_mut(id).unwrap().notes = "first\nsecond\nthird".into();
        commit(TaskCol::Notes, &mut p, id, "changed").unwrap();
        assert_eq!(p.task(id).unwrap().notes, "changed\nsecond\nthird");
        commit(TaskCol::Notes, &mut p, id, "").unwrap();
        assert_eq!(p.task(id).unwrap().notes, "second\nthird");
        p.task_mut(id).unwrap().notes = "one line".into();
        commit(TaskCol::Notes, &mut p, id, "replaced").unwrap();
        assert_eq!(p.task(id).unwrap().notes, "replaced");
    }
}
