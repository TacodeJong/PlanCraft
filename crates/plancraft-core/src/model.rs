//! The PlanCraft project model.
//!
//! A [`Project`] contains an ordered outline of [`Task`]s (regular tasks,
//! milestones and summary tasks), the [`Resource`]s that perform the work,
//! [`Assignment`]s linking the two, working-time [`Calendar`]s, tracked
//! [`TimeEntry`]s and references to [`ExternalProject`]s whose tasks may be
//! used as predecessors.
//!
//! Fields grouped in [`TaskSchedule`] are *computed* by the scheduler; they
//! are persisted so that other documents (and AI agents) can read dates
//! without re-running the engine, but they are always overwritten on
//! recalculation.

use crate::dates::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::calendar::{Calendar, CalendarException};
use crate::duration::{DurationUnit, Lag, Minutes, TimeSettings};
use crate::ids::*;

pub const FILE_FORMAT: &str = "plancraft";
pub const FILE_VERSION: u32 = 1;
pub const FILE_EXTENSION: &str = "plancraft";

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_true(b: &bool) -> bool {
    *b
}
fn yes() -> bool {
    true
}
fn default_priority() -> u16 {
    500
}
fn default_units() -> f64 {
    1.0
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// What a row in the task outline represents.
///
/// Summary tasks are not a kind: any task followed by tasks with a deeper
/// outline level is a summary task.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    #[default]
    Task,
    /// A checkpoint in time that completes when its member tasks and predecessors finish.
    Milestone,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleMode {
    /// Dates are computed by the scheduling engine.
    #[default]
    Auto,
    /// Dates are entered by the user and only used as given.
    Manual,
}

/// Which of duration, units and work stays fixed when one of the others changes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    #[default]
    FixedUnits,
    FixedDuration,
    FixedWork,
}

/// Whether a leaf task keeps a typed duration or sizes itself from the available window.
///
/// Distinct from [`ScheduleMode`]: that chooses how *dates* are set; this chooses how *duration*
/// is set. Default [`DurationMode::Fixed`] keeps old plans unchanged. [`DurationMode::Auto`] is a
/// per-task opt-in that reuses group-share fill without turning on project-wide stretch rules.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DurationMode {
    /// The stored duration is the plan (editable).
    #[default]
    Fixed,
    /// Duration is calculated from the dependency/date window (read-only while Auto).
    Auto,
}

impl DurationMode {
    pub const ALL: [DurationMode; 2] = [DurationMode::Fixed, DurationMode::Auto];

    fn is_fixed(&self) -> bool {
        *self == DurationMode::Fixed
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    /// As soon as possible.
    #[default]
    Asap,
    /// As late as possible.
    Alap,
    /// Start no earlier than.
    Snet,
    /// Start no later than.
    Snlt,
    /// Finish no earlier than.
    Fnet,
    /// Finish no later than.
    Fnlt,
    /// Must start on.
    Mso,
    /// Must finish on.
    Mfo,
}

/// Optional project-wide stretch/span flags (Project Planner JSON import turns them on).
///
/// Size-in-group (`fills_group` / `duration_percent`) no longer requires these; they still force
/// a share-resolution pass for older opted-in plans.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct ScheduleRules {
    pub stretch: bool,
    pub span: bool,
}

impl ScheduleRules {
    /// Stretch and span together, for a Project Planner import or an opted-in plan.
    pub fn on() -> Self {
        Self { stretch: true, span: true }
    }

    pub fn enabled(self) -> bool {
        self.stretch || self.span
    }

    pub fn is_off(&self) -> bool {
        !self.enabled()
    }
}

/// Earliest and latest dates on one task. Each side can be set at once.
///
/// A minimum always pushes the early date. An unfixed maximum only limits the late date.
/// A fixed maximum moves the plan. "On" is an earliest date equal to its latest date.
/// Absent on plans that only use [`Constraint`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct DateBounds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_earliest: Option<DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_latest: Option<DateTime>,
    /// When set, `start_latest` caps the early start. When clear, it only limits the late start.
    #[serde(default, skip_serializing_if = "is_false")]
    pub start_latest_fixed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_earliest: Option<DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_latest: Option<DateTime>,
    /// When set, `finish_latest` pulls the early finish. When clear, a missed date is reported.
    #[serde(default, skip_serializing_if = "is_false")]
    pub finish_latest_fixed: bool,
}

impl DateBounds {
    pub fn is_empty(&self) -> bool {
        self.start_earliest.is_none()
            && self.start_latest.is_none()
            && self.finish_earliest.is_none()
            && self.finish_latest.is_none()
            && !self.start_latest_fixed
            && !self.finish_latest_fixed
    }
}

/// How one side (start or finish) of a task is restricted, read from its earliest/latest pair.
/// The mapping mirrors the Project Planner app's `DateMode`: none → unrestricted, equal → on,
/// minimum only → no earlier than, maximum only → no later than, both → between.
///
/// An unrestricted side carries no date of its own: it is placed by predecessors, its group and
/// any backward pressure from successors. Stretch (remaining space) applies only while both sides
/// are unrestricted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateMode {
    Unrestricted,
    On,
    NoEarlier,
    NoLater,
    Between,
}

impl DateMode {
    pub const ALL: [DateMode; 5] = [DateMode::Unrestricted, DateMode::On, DateMode::NoEarlier, DateMode::NoLater, DateMode::Between];

    /// The mode a minimum/maximum pair expresses.
    pub fn of(min: Option<DateTime>, max: Option<DateTime>) -> DateMode {
        match (min, max) {
            (None, None) => DateMode::Unrestricted,
            (Some(a), Some(b)) if a == b => DateMode::On,
            (Some(_), Some(_)) => DateMode::Between,
            (Some(_), None) => DateMode::NoEarlier,
            (None, Some(_)) => DateMode::NoLater,
        }
    }

    /// New minimum and maximum after switching to this mode, keeping existing dates where they
    /// fit. `anchor` is used when the side has no date yet (normally the scheduled date).
    /// Between opens a one-day window when there is no later maximum yet.
    pub fn window(self, min: Option<DateTime>, max: Option<DateTime>, anchor: DateTime) -> (Option<DateTime>, Option<DateTime>) {
        match self {
            DateMode::Unrestricted => (None, None),
            DateMode::On => {
                let day = min.or(max).unwrap_or(anchor);
                (Some(day), Some(day))
            }
            DateMode::NoEarlier => (Some(min.or(max).unwrap_or(anchor)), None),
            DateMode::NoLater => (None, Some(max.or(min).unwrap_or(anchor))),
            DateMode::Between => {
                let lo = min.or(max).unwrap_or(anchor);
                let hi = max.filter(|d| *d > lo).or_else(|| lo.checked_add(crate::dates::days(1)).ok()).unwrap_or(lo);
                (Some(lo), Some(hi))
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DateMode::Unrestricted => "Unrestricted",
            DateMode::On => "On",
            DateMode::NoEarlier => "No Earlier Than",
            DateMode::NoLater => "No Later Than",
            DateMode::Between => "Between",
        }
    }
}

impl ConstraintKind {
    pub const ALL: [ConstraintKind; 8] = [
        ConstraintKind::Asap,
        ConstraintKind::Alap,
        ConstraintKind::Snet,
        ConstraintKind::Snlt,
        ConstraintKind::Fnet,
        ConstraintKind::Fnlt,
        ConstraintKind::Mso,
        ConstraintKind::Mfo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConstraintKind::Asap => "As Soon As Possible",
            ConstraintKind::Alap => "As Late As Possible",
            ConstraintKind::Snet => "Start No Earlier Than",
            ConstraintKind::Snlt => "Start No Later Than",
            ConstraintKind::Fnet => "Finish No Earlier Than",
            ConstraintKind::Fnlt => "Finish No Later Than",
            ConstraintKind::Mso => "Must Start On",
            ConstraintKind::Mfo => "Must Finish On",
        }
    }

    pub fn needs_date(self) -> bool {
        !matches!(self, ConstraintKind::Asap | ConstraintKind::Alap)
    }

    /// Whether the constraint date refers to the start (otherwise the finish).
    pub fn is_start_constraint(self) -> bool {
        matches!(self, ConstraintKind::Snet | ConstraintKind::Snlt | ConstraintKind::Mso)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Constraint {
    pub kind: ConstraintKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime>,
}

/// Dependency type between a predecessor and a successor.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
pub enum LinkType {
    /// Finish-to-Start.
    #[default]
    FS,
    /// Start-to-Start.
    SS,
    /// Finish-to-Finish.
    FF,
    /// Start-to-Finish.
    SF,
}

impl LinkType {
    pub const ALL: [LinkType; 4] = [LinkType::FS, LinkType::SS, LinkType::FF, LinkType::SF];

    pub fn code(self) -> &'static str {
        match self {
            LinkType::FS => "FS",
            LinkType::SS => "SS",
            LinkType::FF => "FF",
            LinkType::SF => "SF",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LinkType::FS => "Finish-to-Start (FS)",
            LinkType::SS => "Start-to-Start (SS)",
            LinkType::FF => "Finish-to-Finish (FF)",
            LinkType::SF => "Start-to-Finish (SF)",
        }
    }

    pub fn parse(s: &str) -> Option<LinkType> {
        match s.trim().to_ascii_uppercase().as_str() {
            "FS" => Some(LinkType::FS),
            "SS" => Some(LinkType::SS),
            "FF" => Some(LinkType::FF),
            "SF" => Some(LinkType::SF),
            _ => None,
        }
    }
}

/// Reference to a task in this project or in an external project document.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum TaskRef {
    Local(TaskId),
    External { project: Uuid, task: TaskId },
}

impl TaskRef {
    pub fn local(&self) -> Option<TaskId> {
        match self {
            TaskRef::Local(id) => Some(*id),
            TaskRef::External { .. } => None,
        }
    }
}

impl From<TaskId> for TaskRef {
    fn from(id: TaskId) -> Self {
        TaskRef::Local(id)
    }
}

/// A predecessor link stored on the successor task.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Dependency {
    pub predecessor: TaskRef,
    #[serde(default)]
    pub kind: LinkType,
    #[serde(default, skip_serializing_if = "Lag::is_zero")]
    pub lag: Lag,
}

impl Dependency {
    pub fn fs(pred: impl Into<TaskRef>) -> Self {
        Self { predecessor: pred.into(), kind: LinkType::FS, lag: Lag::None }
    }
}

/// How much effort a task is expected to take before resources are assigned.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Estimate {
    /// Relative complexity in points; converted with `ProjectSettings::minutes_per_point`.
    Complexity { points: f64 },
    /// Three-point (PERT) estimate of work in minutes.
    ThreePoint { optimistic: Minutes, most_likely: Minutes, pessimistic: Minutes },
    /// Direct work estimate in minutes.
    Work { minutes: Minutes },
}

impl Estimate {
    /// Expected work in minutes.
    pub fn expected_work(&self, settings: &ProjectSettings) -> Minutes {
        match *self {
            Estimate::Complexity { points } => {
                crate::limits::clamp_minutes((points * settings.minutes_per_point as f64).round() as Minutes)
            }
            Estimate::ThreePoint { optimistic, most_likely, pessimistic } => {
                ((optimistic as f64 + 4.0 * most_likely as f64 + pessimistic as f64) / 6.0).round() as Minutes
            }
            Estimate::Work { minutes } => minutes,
        }
    }

    /// Standard deviation of a three-point estimate (0 for other methods).
    pub fn std_dev(&self) -> Minutes {
        match *self {
            Estimate::ThreePoint { optimistic, pessimistic, .. } => ((pessimistic as f64 - optimistic as f64) / 6.0).round() as Minutes,
            _ => 0,
        }
    }
}

/// A pause in a task: after `work` working minutes of the task, work stops until `resume`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Split {
    pub work: Minutes,
    pub resume: DateTime,
}

/// A split planned in a task: after `work` working minutes of the task, work pauses for `length`
/// working minutes. Unlike a [`Split`] (a pause in work already under way, until a fixed date),
/// it moves with the task.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Gap {
    pub work: Minutes,
    pub length: Minutes,
}

/// A snapshot of planned values used to measure variance.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Baseline {
    pub start: DateTime,
    pub finish: DateTime,
    pub duration: Minutes,
    pub work: Minutes,
    pub cost: f64,
    pub saved: DateTime,
}

/// Start and finish dates saved as an interim plan (the Start1–10 and Finish1–10 fields).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interim {
    pub start: DateTime,
    pub finish: DateTime,
}

/// Workflow status, independent of percent complete. Missing on old files, which read as not started.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    NotStarted,
    Started,
    Busy,
    Done,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 4] = [TaskStatus::NotStarted, TaskStatus::Started, TaskStatus::Busy, TaskStatus::Done];

    /// Serde token (`not_started`, `started`, `busy`, `done`). MSPDI stores this, never the Dutch label.
    pub fn token(self) -> &'static str {
        match self {
            TaskStatus::NotStarted => "not_started",
            TaskStatus::Started => "started",
            TaskStatus::Busy => "busy",
            TaskStatus::Done => "done",
        }
    }

    pub fn from_token(text: &str) -> Option<Self> {
        Some(match text.trim() {
            "not_started" => TaskStatus::NotStarted,
            "started" => TaskStatus::Started,
            "busy" => TaskStatus::Busy,
            "done" => TaskStatus::Done,
            _ => return None,
        })
    }

    /// Dutch label shown in the properties pane, Task Information, the report and the filter strip.
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::NotStarted => "Niet gestart",
            TaskStatus::Started => "Gestart",
            TaskStatus::Busy => "Bezig",
            TaskStatus::Done => "Gereed",
        }
    }

    fn is_not_started(&self) -> bool {
        *self == TaskStatus::NotStarted
    }

    /// A parent's displayed status. Every child the same value keeps that value.
    /// All done is done. Any mix, or any started or busy child, is in progress.
    pub fn rollup(children: &[TaskStatus]) -> TaskStatus {
        let Some(first) = children.first().copied() else { return TaskStatus::NotStarted };
        if children.iter().all(|s| *s == first) {
            return first;
        }
        TaskStatus::Busy
    }
}

/// A personal highlight on a whole outline row, for a question or a flag.
/// It does not change the bar color or the schedule.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RowMark {
    Amber,
    Blue,
    Pink,
    Green,
}

impl RowMark {
    pub const ALL: [RowMark; 4] = [RowMark::Amber, RowMark::Blue, RowMark::Pink, RowMark::Green];

    pub fn label(self) -> &'static str {
        match self {
            RowMark::Amber => "Amber",
            RowMark::Blue => "Blue",
            RowMark::Pink => "Pink",
            RowMark::Green => "Green",
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            RowMark::Amber => "amber",
            RowMark::Blue => "blue",
            RowMark::Pink => "pink",
            RowMark::Green => "green",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "amber" => Some(RowMark::Amber),
            "blue" => Some(RowMark::Blue),
            "pink" => Some(RowMark::Pink),
            "green" => Some(RowMark::Green),
            _ => None,
        }
    }
}

/// Values computed by the scheduler. Never edit directly.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct TaskSchedule {
    pub start: Option<DateTime>,
    pub finish: Option<DateTime>,
    pub early_start: Option<DateTime>,
    pub early_finish: Option<DateTime>,
    pub late_start: Option<DateTime>,
    pub late_finish: Option<DateTime>,
    /// Working minutes the task can slip without delaying the project.
    pub total_slack: Minutes,
    /// Working minutes the task can slip without delaying any successor.
    pub free_slack: Minutes,
    pub critical: bool,
    pub is_summary: bool,
    /// Planned work (assignments, or the estimate when unassigned).
    pub work: Minutes,
    pub actual_work: Minutes,
    pub cost: f64,
    /// Duration of summary tasks (computed span); leaf tasks mirror `Task::duration`.
    pub duration: Minutes,
    pub percent_complete: f32,
    /// End of the completed part of an in-progress task (Microsoft Project's Stop field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<DateTime>,
    /// Start of the remaining part of an in-progress task: the next working moment after the stop,
    /// or the start of the next part when the task pauses at its stop (Microsoft Project's Resume).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<DateTime>,
    /// The working parts (start, finish) of a split task, in order; empty when it is not split.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<(DateTime, DateTime)>,
    /// The span of a manually scheduled summary's subtasks (its own dates are `start` and
    /// `finish`); `None` for every other task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollup_start: Option<DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollup_finish: Option<DateTime>,
    /// Human-readable scheduling problems (constraint conflicts, missed deadlines...).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: TaskId,
    pub name: String,
    #[serde(default = "one_u16")]
    pub outline_level: u16,
    #[serde(default)]
    pub kind: TaskKind,
    #[serde(default)]
    pub mode: ScheduleMode,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub active: bool,
    /// Planned duration in working minutes, or in elapsed minutes when `elapsed` is set.
    #[serde(default)]
    pub duration: Minutes,
    /// Duration is a placeholder estimate (shown with a trailing `?`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub estimated: bool,
    /// The duration is elapsed time (`33ed`): it runs around the clock, nights, weekends and
    /// holidays included, instead of in working time.
    #[serde(default, skip_serializing_if = "is_false")]
    pub elapsed: bool,
    #[serde(default)]
    pub task_type: TaskType,
    #[serde(default, skip_serializing_if = "is_false")]
    pub effort_driven: bool,
    #[serde(default)]
    pub constraint: Constraint,
    /// Both start bounds and both finish bounds. Empty bounds leave the single [`Constraint`] in charge.
    #[serde(default, skip_serializing_if = "DateBounds::is_empty")]
    pub bounds: DateBounds,
    /// Share the parent group's remaining space (Size-in-group → Remaining space).
    ///
    /// Stretching applies when both start and finish are [`DateMode::Unrestricted`] (Project Planner
    /// `groupShare`); any own date on either side keeps the stored duration instead.
    /// New tasks default to `true` (Swift `fillsGroup`); missing field on deserialize stays `false`
    /// so old PlanCraft files keep stored durations until the user opts in.
    #[serde(default, skip_serializing_if = "is_false")]
    pub fills_group: bool,
    /// Fixed (typed) vs Auto (window-calculated) duration. Absent on old files → Fixed.
    #[serde(default, skip_serializing_if = "DurationMode::is_fixed")]
    pub duration_mode: DurationMode,
    /// Duration as a percentage of the parent group's span (Size-in-group → % of group).
    /// Applies whenever set; project-wide [`ScheduleRules::span`] is not required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarId>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ignore_resource_calendars: bool,
    #[serde(default = "default_priority")]
    pub priority: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predecessors: Vec<Dependency>,
    /// For milestones: tasks whose completion makes up this milestone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<TaskRef>,
    /// For milestones: the output achieved when the milestone is reached.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub deliverable: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<Estimate>,
    #[serde(default)]
    pub percent_complete: u8,
    /// Workflow status. Old files omit it and load as not started. Percent complete stays independent
    /// except when a status change sets 100 (done) or 0 (not started).
    #[serde(default, skip_serializing_if = "TaskStatus::is_not_started")]
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_start: Option<DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_finish: Option<DateTime>,
    /// Manually scheduled start (manual mode) or user-typed start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_start: Option<DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_finish: Option<DateTime>,
    /// Pauses in an in-progress task (set with a later Resume date, or by rescheduling uncompleted
    /// work): the task's working time is laid out from its actual start, and after each split's
    /// `work` it stops until that split's `resume`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<Split>,
    /// Splits planned in the task (Split Task): pauses that move with it. See [`Gap`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<Gap>,
    /// A recurring task: this summary's subtasks are its occurrences (see [`crate::recurring`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<crate::recurring::Recurrence>,
    /// An occurrence of a recurring task (made by it, and changed with it).
    #[serde(default, skip_serializing_if = "is_false")]
    pub occurrence: bool,
    /// The Task Board column the task is in (Board Status); `None`: the first column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_status: Option<BoardColumnId>,
    /// Whether the task shows on the Task Board (Show on Board).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub show_on_board: bool,
    /// Delay added by resource leveling (working minutes).
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub leveling_delay: Minutes,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub fixed_cost: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Baseline (number 0): the one variance is measured against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Baseline>,
    /// Baselines 1–10, by number (see [`crate::baseline`]).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty", deserialize_with = "crate::baseline::lenient_slots")]
    pub baselines: std::collections::BTreeMap<u8, Baseline>,
    /// Interim plans 1–10, by number.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty", deserialize_with = "crate::baseline::lenient_slots")]
    pub interim: std::collections::BTreeMap<u8, Interim>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_on_timeline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar_color: Option<[u8; 3]>,
    /// Whole-row highlight for a question or flag the planner wants to see in the outline.
    /// Independent of `bar_color`. Absent on plans that never set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_mark: Option<RowMark>,
    /// Values of the project's custom fields (see [`crate::custom`]).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub custom: std::collections::BTreeMap<CustomFieldId, crate::custom::CustomValue>,
    #[serde(default)]
    pub sched: TaskSchedule,
}

fn one_u16() -> u16 {
    1
}
fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

/// The earlier of two optional dates, where a date always beats none (unlike `Option`'s own
/// ordering, which puts `None` first).
fn opt_min(a: Option<DateTime>, b: Option<DateTime>) -> Option<DateTime> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    }
}

impl Task {
    /// The start side's minimum and maximum, the legacy single constraint included
    /// (SNET → minimum, SNLT → maximum, MSO → both). A manually scheduled task's typed
    /// start reads as an On pin.
    pub fn start_window(&self) -> (Option<DateTime>, Option<DateTime>) {
        if self.mode == ScheduleMode::Manual
            && let Some(d) = self.manual_start
        {
            return (Some(d), Some(d));
        }
        let mut min = self.bounds.start_earliest;
        let mut max = self.bounds.start_latest;
        match self.constraint.kind {
            ConstraintKind::Snet => min = min.max(self.constraint.date),
            ConstraintKind::Snlt => max = opt_min(max, self.constraint.date),
            ConstraintKind::Mso => {
                min = min.or(self.constraint.date);
                max = max.or(self.constraint.date);
            }
            _ => {}
        }
        (min, max)
    }

    /// The finish side's minimum and maximum (FNET → minimum, FNLT → maximum, MFO → both),
    /// as [`Task::start_window`] for the other side.
    pub fn finish_window(&self) -> (Option<DateTime>, Option<DateTime>) {
        if self.mode == ScheduleMode::Manual
            && let Some(d) = self.manual_finish
        {
            return (Some(d), Some(d));
        }
        let mut min = self.bounds.finish_earliest;
        let mut max = self.bounds.finish_latest;
        match self.constraint.kind {
            ConstraintKind::Fnet => min = min.max(self.constraint.date),
            ConstraintKind::Fnlt => max = opt_min(max, self.constraint.date),
            ConstraintKind::Mfo => {
                min = min.or(self.constraint.date);
                max = max.or(self.constraint.date);
            }
            _ => {}
        }
        (min, max)
    }

    /// How the start side is restricted (see [`DateMode`]).
    pub fn start_mode(&self) -> DateMode {
        let (min, max) = self.start_window();
        DateMode::of(min, max)
    }

    /// How the finish side is restricted (see [`DateMode`]).
    pub fn finish_mode(&self) -> DateMode {
        let (min, max) = self.finish_window();
        DateMode::of(min, max)
    }

    /// Whether the row carries any user-imposed date (bounds, dated constraint, deadline, or
    /// manual anchors). Placement (ASAP/ALAP) alone does not count. Swift `hasDateConstraints`.
    pub fn has_date_constraints(&self) -> bool {
        !self.bounds.is_empty()
            || self.constraint.kind.needs_date()
            || self.deadline.is_some()
            || self.manual_start.is_some()
            || self.manual_finish.is_some()
            || self.mode == ScheduleMode::Manual
    }

    /// Paired dates that drive working duration before share resolution (Swift `dateDurationBounds`):
    /// both sides **On**, or a hard start-floor + fixed finish-deadline window.
    pub fn date_duration_bounds(&self) -> Option<(DateTime, DateTime)> {
        if self.is_milestone() {
            return None;
        }
        let (s_min, s_max) = self.start_window();
        let (f_min, f_max) = self.finish_window();
        let start = s_min?;
        let finish = f_max?;
        let start_mode = DateMode::of(s_min, s_max);
        let end_mode = DateMode::of(f_min, f_max);
        let exact = start_mode == DateMode::On && end_mode == DateMode::On;
        let fixed_window = self.bounds.start_earliest.is_some()
            && self.bounds.finish_latest_fixed
            && matches!(start_mode, DateMode::NoEarlier | DateMode::On)
            && matches!(end_mode, DateMode::NoLater | DateMode::On);
        if !(exact || fixed_window) || finish < start {
            return None;
        }
        Some((start, finish))
    }

    /// Clears every user-imposed date so only the group, predecessors and placement position the
    /// activity (both sides Unrestricted). Keeps ASAP/ALAP placement, `fills_group` and
    /// `duration_percent`. Used for Remaining space and Clear Dates (Swift `clearDateConstraints`).
    pub fn clear_date_constraints(&mut self) {
        self.bounds = DateBounds::default();
        if self.constraint.kind.needs_date() {
            self.constraint = Constraint::default();
        }
        // Soft hand deadline clears with other date limits (Swift `latestFinish`).
        self.deadline = None;
        self.manual_start = None;
        self.manual_finish = None;
        if self.mode == ScheduleMode::Manual {
            self.mode = ScheduleMode::Auto;
        }
        // Keeps `fills_group` / `duration_percent` so Remaining space / % of group stay eligible.
    }

    /// Whether one side takes its date from dependencies/group rollup (`true`) or from its own
    /// exact anchor (`false`).
    ///
    /// Only an `On` side is an exact anchor. Other modes (`No Earlier Than`, `No Later Than`,
    /// `Between`) keep a bound/range while the side itself remains dynamic.
    pub fn side_is_dynamic(&self, finish: bool) -> bool {
        self.side_anchor(finish).is_none()
    }

    /// The exact anchor of one side (`On` mode), if any.
    pub fn side_anchor(&self, finish: bool) -> Option<DateTime> {
        let (min, max) = if finish { self.finish_window() } else { self.start_window() };
        min.zip(max).and_then(|(a, b)| (a == b).then_some(a))
    }

    pub fn new(id: TaskId, name: impl Into<String>, minutes_per_day: Minutes) -> Self {
        Self {
            id,
            name: name.into(),
            outline_level: 1,
            kind: TaskKind::Task,
            mode: ScheduleMode::Auto,
            active: true,
            duration: minutes_per_day,
            estimated: true,
            elapsed: false,
            task_type: TaskType::FixedUnits,
            effort_driven: false,
            constraint: Constraint::default(),
            bounds: DateBounds::default(),
            // Swift `fillsGroup = true`. Serde missing-field default stays false for old files.
            fills_group: true,
            duration_mode: DurationMode::Fixed,
            duration_percent: None,
            deadline: None,
            calendar: None,
            ignore_resource_calendars: false,
            priority: 500,
            predecessors: Vec::new(),
            members: Vec::new(),
            deliverable: String::new(),
            estimate: None,
            percent_complete: 0,
            status: TaskStatus::NotStarted,
            actual_start: None,
            actual_finish: None,
            manual_start: None,
            manual_finish: None,
            splits: Vec::new(),
            gaps: Vec::new(),
            recurrence: None,
            occurrence: false,
            board_status: None,
            show_on_board: true,
            leveling_delay: 0,
            fixed_cost: 0.0,
            notes: String::new(),
            baseline: None,
            baselines: std::collections::BTreeMap::new(),
            interim: std::collections::BTreeMap::new(),
            show_on_timeline: false,
            bar_color: None,
            row_mark: None,
            custom: std::collections::BTreeMap::new(),
            sched: TaskSchedule::default(),
        }
    }

    pub fn milestone(id: TaskId, name: impl Into<String>) -> Self {
        let mut t = Self::new(id, name, 0);
        t.kind = TaskKind::Milestone;
        t.duration = 0;
        t.estimated = false;
        t
    }

    pub fn is_milestone(&self) -> bool {
        self.kind == TaskKind::Milestone
    }

    pub fn is_summary(&self) -> bool {
        self.sched.is_summary
    }

    pub fn is_complete(&self) -> bool {
        self.percent_complete >= 100
    }

    /// Working time done so far: % complete of the duration (Microsoft Project's Actual Duration).
    pub fn actual_duration(&self) -> Minutes {
        if self.is_milestone() {
            return 0;
        }
        self.duration.max(0).saturating_mul(Minutes::from(self.percent_complete.min(100))) / 100
    }

    /// Working time still to do (Microsoft Project's Remaining Duration).
    pub fn remaining_duration(&self) -> Minutes {
        if self.is_milestone() {
            return 0;
        }
        self.duration.max(0).saturating_sub(self.actual_duration())
    }

    /// Actual and remaining duration as shown in tables and reports. A summary's come from its
    /// rolled-up duration and % complete; a leaf's from [`Task::actual_duration`].
    pub fn duration_progress(&self) -> (Minutes, Minutes) {
        if !self.sched.is_summary {
            return (self.actual_duration(), self.remaining_duration());
        }
        let total = self.sched.duration.max(0);
        let pct = f64::from(self.sched.percent_complete);
        let pct = if pct.is_finite() { pct.clamp(0.0, 100.0) } else { 0.0 };
        let done = ((total as f64) * pct / 100.0).round() as Minutes;
        let done = done.clamp(0, total);
        (done, total.saturating_sub(done))
    }

    /// Whether adding or removing people keeps total work the same. Fixed Work tasks always are.
    pub fn is_effort_driven(&self) -> bool {
        self.effort_driven || self.task_type == TaskType::FixedWork
    }
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// How a resource is costed and consumed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    /// People, teams, companies or equipment that spend working time.
    #[default]
    Work,
    /// Consumables measured in units (`material_label`).
    Material,
    /// A fixed cost item (travel, licences...).
    Cost,
}

/// Who or what a resource is.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResourceCategory {
    #[default]
    TeamMember,
    Team,
    PartnerCompany,
    Vendor,
    Contractor,
    Equipment,
    Facility,
    Other,
}

impl ResourceCategory {
    pub const ALL: [ResourceCategory; 8] = [
        ResourceCategory::TeamMember,
        ResourceCategory::Team,
        ResourceCategory::PartnerCompany,
        ResourceCategory::Vendor,
        ResourceCategory::Contractor,
        ResourceCategory::Equipment,
        ResourceCategory::Facility,
        ResourceCategory::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResourceCategory::TeamMember => "Team member",
            ResourceCategory::Team => "Team",
            ResourceCategory::PartnerCompany => "Partner company",
            ResourceCategory::Vendor => "Vendor",
            ResourceCategory::Contractor => "Contractor",
            ResourceCategory::Equipment => "Equipment",
            ResourceCategory::Facility => "Facility",
            ResourceCategory::Other => "Other",
        }
    }
}

/// A period during which a resource's maximum units differ from the default.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct AvailabilityPeriod {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Date>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Date>,
    /// 1.0 = 100%.
    pub units: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    pub id: ResourceId,
    pub name: String,
    #[serde(default)]
    pub kind: ResourceType,
    #[serde(default)]
    pub category: ResourceCategory,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub initials: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    /// Label of material units, e.g. "tons".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material_label: String,
    /// Maximum units of work available (1.0 = one full-time person).
    #[serde(default = "default_units")]
    pub max_units: f64,
    /// Date-bound overrides of `max_units`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub availability: Vec<AvailabilityPeriod>,
    /// Cost per hour of work (or per unit for material resources).
    #[serde(default)]
    pub standard_rate: f64,
    #[serde(default)]
    pub overtime_rate: f64,
    #[serde(default)]
    pub cost_per_use: f64,
    /// Base calendar; `None` uses the project calendar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarId>,
    /// Personal non-working time / altered working time (vacations, part-time days).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<CalendarException>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Resource {
    pub fn new(id: ResourceId, name: impl Into<String>) -> Self {
        let name = name.into();
        let initials = name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect::<String>().to_uppercase();
        Self {
            id,
            name,
            kind: ResourceType::Work,
            category: ResourceCategory::TeamMember,
            initials,
            group: String::new(),
            email: String::new(),
            material_label: String::new(),
            max_units: 1.0,
            availability: Vec::new(),
            standard_rate: 0.0,
            overtime_rate: 0.0,
            cost_per_use: 0.0,
            calendar: None,
            exceptions: Vec::new(),
            notes: String::new(),
        }
    }

    /// Maximum units available on a date, honoring availability periods.
    pub fn units_on(&self, d: Date) -> f64 {
        self.availability
            .iter()
            .rev()
            .find(|p| p.from.is_none_or(|f| d >= f) && p.to.is_none_or(|t| d <= t))
            .map(|p| p.units)
            .unwrap_or(self.max_units)
    }
}

// ---------------------------------------------------------------------------
// Assignments & time tracking
// ---------------------------------------------------------------------------

/// Shape of how an assignment's work is spread across its span.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkContour {
    #[default]
    Flat,
    FrontLoaded,
    BackLoaded,
    Bell,
}

impl WorkContour {
    pub const ALL: [WorkContour; 4] = [WorkContour::Flat, WorkContour::FrontLoaded, WorkContour::BackLoaded, WorkContour::Bell];

    /// Relative intensity at position `x` in `[0, 1]` across the assignment.
    pub fn weight(self, x: f64) -> f64 {
        match self {
            WorkContour::Flat => 1.0,
            WorkContour::FrontLoaded => 1.75 - 1.5 * x,
            WorkContour::BackLoaded => 0.25 + 1.5 * x,
            WorkContour::Bell => 0.2 + 1.6 * (1.0 - (2.0 * x - 1.0).powi(2)),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WorkContour::Flat => "Flat",
            WorkContour::FrontLoaded => "Front loaded",
            WorkContour::BackLoaded => "Back loaded",
            WorkContour::Bell => "Bell",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Assignment {
    pub id: AssignmentId,
    pub task: TaskId,
    pub resource: ResourceId,
    /// Units of the resource applied (1.0 = 100%); quantity for material resources.
    #[serde(default = "default_units")]
    pub units: f64,
    /// Planned work in minutes (work resources).
    #[serde(default)]
    pub work: Minutes,
    /// Actual work entered directly, in addition to tracked time entries.
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub actual_work: Minutes,
    /// Fixed cost of the assignment (cost resources).
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub cost: f64,
    #[serde(default)]
    pub contour: WorkContour,
}

impl Assignment {
    /// What the assignment costs with this resource: work hours at the standard rate, material
    /// units at the unit price (each plus the cost per use), or a cost resource's fixed amount.
    pub fn cost_with(&self, r: &Resource) -> f64 {
        match r.kind {
            ResourceType::Work => self.work as f64 / 60.0 * r.standard_rate + r.cost_per_use,
            ResourceType::Material => self.units * r.standard_rate + r.cost_per_use,
            ResourceType::Cost => self.cost,
        }
    }
}

/// Time spent by a resource on a task, as entered on a timesheet.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TimeEntry {
    pub id: TimeEntryId,
    pub task: TaskId,
    pub resource: ResourceId,
    pub date: Date,
    pub minutes: Minutes,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

// ---------------------------------------------------------------------------
// Cross-document links
// ---------------------------------------------------------------------------

/// Last known state of a task in another document.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExternalTaskSnapshot {
    pub id: TaskId,
    pub name: String,
    pub start: Option<DateTime>,
    pub finish: Option<DateTime>,
    #[serde(default)]
    pub percent_complete: u8,
    #[serde(default)]
    pub milestone: bool,
    #[serde(default)]
    pub duration: Minutes,
}

/// Another `.plancraft` document whose tasks are referenced by this project.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExternalProject {
    /// Project id stored inside the other document.
    pub id: Uuid,
    pub name: String,
    /// Path to the other document, relative to this document's directory when possible.
    pub path: String,
    /// Cached copies of referenced tasks so scheduling works when the file is unavailable.
    #[serde(default)]
    pub tasks: Vec<ExternalTaskSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<DateTime>,
}

impl ExternalProject {
    pub fn task(&self, id: TaskId) -> Option<&ExternalTaskSnapshot> {
        self.tasks.iter().find(|t| t.id == id)
    }
}

/// Bar, summary, milestone and critical colors, on top of the light or dark theme.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChartStyle {
    #[default]
    Blue,
    Graphite,
    Green,
    Orange,
    HighContrast,
}

impl ChartStyle {
    pub const ALL: [ChartStyle; 5] =
        [ChartStyle::Blue, ChartStyle::Graphite, ChartStyle::Green, ChartStyle::Orange, ChartStyle::HighContrast];

    pub fn label(self) -> &'static str {
        match self {
            ChartStyle::Blue => "Blue",
            ChartStyle::Graphite => "Graphite",
            ChartStyle::Green => "Green",
            ChartStyle::Orange => "Orange",
            ChartStyle::HighContrast => "High Contrast",
        }
    }

    /// The light-theme task bar, used when a task has no color of its own.
    pub fn bar_rgb(self) -> [u8; 3] {
        match self {
            ChartStyle::Blue => [0x5A, 0x86, 0xD6],
            ChartStyle::Graphite => [0x6E, 0x74, 0x80],
            ChartStyle::Green => [0x2F, 0x8F, 0x62],
            ChartStyle::Orange => [0xE0, 0x8A, 0x2F],
            ChartStyle::HighContrast => [0x0B, 0x2E, 0x6B],
        }
    }

    fn is_default(&self) -> bool {
        *self == ChartStyle::Blue
    }
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectSettings {
    #[serde(flatten)]
    pub time: TimeSettings,
    pub duration_unit: DurationUnit,
    pub currency: String,
    /// Work represented by one complexity point.
    pub minutes_per_point: Minutes,
    /// Tasks with total slack at or below this are critical.
    pub critical_slack: Minutes,
    /// New tasks default to auto scheduling.
    pub new_tasks_auto: bool,
    /// Assigned tasks follow resource calendars (working time common to all assigned resources).
    pub use_resource_calendars: bool,
    /// Update task % complete automatically from tracked time.
    pub progress_from_time: bool,
    /// The baseline (0–10) earned value is measured against.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub earned_value_baseline: u8,
    /// Group stretch and percent-of-group. Off for every plan that does not set it.
    #[serde(default, skip_serializing_if = "ScheduleRules::is_off")]
    pub rules: ScheduleRules,
    /// Chart colors. Blue is the original palette and is omitted from saved files.
    #[serde(default, skip_serializing_if = "ChartStyle::is_default")]
    pub chart_style: ChartStyle,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            time: TimeSettings::default(),
            duration_unit: DurationUnit::Days,
            currency: "$".into(),
            minutes_per_point: 4 * 60,
            critical_slack: 0,
            new_tasks_auto: true,
            use_resource_calendars: true,
            progress_from_time: false,
            earned_value_baseline: 0,
            rules: ScheduleRules::default(),
            chart_style: ChartStyle::Blue,
        }
    }
}

/// Values computed for the whole project.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ProjectSchedule {
    pub start: Option<DateTime>,
    pub finish: Option<DateTime>,
    pub work: Minutes,
    pub actual_work: Minutes,
    pub cost: f64,
    pub percent_complete: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub calculated_at: Option<DateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub format: String,
    pub version: u32,
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub manager: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub notes: String,
    pub start: DateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_date: Option<Date>,
    pub calendar: CalendarId,
    #[serde(default)]
    pub settings: ProjectSettings,
    pub calendars: Vec<Calendar>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub assignments: Vec<Assignment>,
    #[serde(default)]
    pub time_entries: Vec<TimeEntry>,
    #[serde(default)]
    pub external_projects: Vec<ExternalProject>,
    /// User-defined task fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_fields: Vec<crate::custom::CustomFieldDef>,
    /// Columns chosen for task tables, by table name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<crate::custom::TableLayout>,
    /// The Task Board's columns, in order (see [`crate::board`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub board_columns: Vec<crate::board::BoardColumn>,
    /// Page Setup for printing and PDF export.
    #[serde(default, skip_serializing_if = "crate::print::PageSetup::is_default", deserialize_with = "crate::print::lenient_page_setup")]
    pub page_setup: crate::print::PageSetup,
    #[serde(default)]
    pub ids: IdCounters,
    #[serde(default)]
    pub sched: ProjectSchedule,
}

impl Project {
    /// Creates an empty project starting at 08:00 on `start`.
    pub fn new(name: impl Into<String>, start: Date) -> Self {
        let mut ids = IdCounters::default();
        let std_id = ids.next_calendar();
        let h24 = ids.next_calendar();
        let night = ids.next_calendar();
        let settings = ProjectSettings::default();
        let start_time = crate::calendar::shift(start.and_time(Time::MIN), Minutes::from(settings.time.default_start_time));
        let board_columns = crate::board::default_columns(&mut ids);
        Self {
            format: FILE_FORMAT.into(),
            version: FILE_VERSION,
            id: Uuid::new_v4(),
            name: name.into(),
            author: String::new(),
            manager: String::new(),
            company: String::new(),
            notes: String::new(),
            start: start_time,
            status_date: None,
            calendar: std_id,
            settings,
            calendars: vec![Calendar::standard(std_id), Calendar::twenty_four_hours(h24), Calendar::night_shift(night)],
            tasks: Vec::new(),
            resources: Vec::new(),
            assignments: Vec::new(),
            time_entries: Vec::new(),
            external_projects: Vec::new(),
            custom_fields: Vec::new(),
            tables: Vec::new(),
            board_columns,
            page_setup: crate::print::PageSetup::default(),
            ids,
            sched: ProjectSchedule::default(),
        }
    }

    pub fn calendar(&self, id: CalendarId) -> Option<&Calendar> {
        self.calendars.iter().find(|c| c.id == id)
    }

    /// The project's base calendar (falls back to the first calendar, then to a standard week).
    pub fn base_calendar(&self) -> &Calendar {
        static FALLBACK: std::sync::LazyLock<Calendar> = std::sync::LazyLock::new(|| Calendar::standard(CalendarId(0)));
        self.calendar(self.calendar).or_else(|| self.calendars.first()).unwrap_or(&FALLBACK)
    }

    pub fn task_index(&self, id: TaskId) -> Option<usize> {
        self.tasks.iter().position(|t| t.id == id)
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn resource(&self, id: ResourceId) -> Option<&Resource> {
        self.resources.iter().find(|r| r.id == id)
    }

    pub fn resource_mut(&mut self, id: ResourceId) -> Option<&mut Resource> {
        self.resources.iter_mut().find(|r| r.id == id)
    }

    pub fn resource_by_name(&self, name: &str) -> Option<&Resource> {
        let n = name.trim();
        self.resources.iter().find(|r| r.name.eq_ignore_ascii_case(n) || (!r.initials.is_empty() && r.initials.eq_ignore_ascii_case(n)))
    }

    pub fn assignment(&self, id: AssignmentId) -> Option<&Assignment> {
        self.assignments.iter().find(|a| a.id == id)
    }

    pub fn assignments_for_task(&self, id: TaskId) -> impl Iterator<Item = &Assignment> {
        self.assignments.iter().filter(move |a| a.task == id)
    }

    pub fn assignments_for_resource(&self, id: ResourceId) -> impl Iterator<Item = &Assignment> {
        self.assignments.iter().filter(move |a| a.resource == id)
    }

    pub fn external(&self, id: Uuid) -> Option<&ExternalProject> {
        self.external_projects.iter().find(|e| e.id == id)
    }

    /// 1-based row number shown as "ID" in task tables.
    pub fn row_number(&self, id: TaskId) -> Option<usize> {
        self.task_index(id).map(|i| i + 1)
    }

    /// Index range of the descendants of the task at `idx` (exclusive of `idx`; empty when out of range).
    pub fn descendants_range(&self, idx: usize) -> std::ops::Range<usize> {
        let Some(level) = self.tasks.get(idx).map(|t| t.outline_level) else {
            return 0..0;
        };
        let first = idx + 1;
        let end = self.tasks.iter().skip(first).position(|t| t.outline_level <= level).map_or(self.tasks.len(), |k| first + k);
        first..end
    }

    /// Index of the parent summary of the task at `idx`.
    pub fn parent_index(&self, idx: usize) -> Option<usize> {
        let level = self.tasks.get(idx)?.outline_level;
        self.tasks.get(..idx)?.iter().rposition(|t| t.outline_level < level)
    }

    /// Status shown for the task at `idx`. A leaf shows its stored status. A summary shows the
    /// roll-up of its children: all done is done, every child the same value keeps it, and any mix
    /// is in progress.
    pub fn displayed_status(&self, idx: usize) -> TaskStatus {
        let own = self.tasks.get(idx).map(|t| t.status).unwrap_or_default();
        if !self.has_children(idx) {
            return own;
        }
        let level = self.tasks.get(idx).map(|t| t.outline_level).unwrap_or(1);
        let next = level.saturating_add(1);
        let children: Vec<TaskStatus> = self
            .descendants_range(idx)
            .filter(|&i| self.tasks.get(i).is_some_and(|t| t.outline_level == next))
            .map(|i| self.displayed_status(i))
            .collect();
        if children.is_empty() { own } else { TaskStatus::rollup(&children) }
    }

    /// Whether the task at `idx` has children.
    pub fn has_children(&self, idx: usize) -> bool {
        match (self.tasks.get(idx), self.tasks.get(idx + 1)) {
            (Some(t), Some(next)) => next.outline_level > t.outline_level,
            _ => false,
        }
    }

    /// Outline number like "1.2.3" for the task at `idx`.
    pub fn wbs(&self, idx: usize) -> String {
        let mut counters: Vec<usize> = Vec::new();
        for t in self.tasks.iter().take(idx.saturating_add(1)) {
            let lvl = usize::from(t.outline_level.max(1));
            counters.resize(lvl, 0);
            if let Some(c) = counters.last_mut() {
                *c += 1;
            }
        }
        counters.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(".")
    }

    /// Display name of a task reference, e.g. `"Design"` or `"Website\Launch"`.
    pub fn task_ref_name(&self, r: &TaskRef) -> String {
        match r {
            TaskRef::Local(id) => self.task(*id).map(|t| t.name.clone()).unwrap_or_else(|| format!("<missing {id}>")),
            TaskRef::External { project, task } => match self.external(*project) {
                Some(e) => format!("{}\\{}", e.name, e.task(*task).map(|t| t.name.clone()).unwrap_or_else(|| task.to_string())),
                None => format!("<external {task}>"),
            },
        }
    }

    pub fn currency(&self) -> &str {
        &self.settings.currency
    }
}
