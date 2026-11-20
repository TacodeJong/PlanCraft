//! UI state that is not part of the project document.

use std::collections::{BTreeSet, HashSet};
use std::time::Instant;

use plancraft_core::dates::prelude::*;
use plancraft_core::prelude::*;
use plancraft_core::workload::Granularity;

use crate::columns::TaskTable;
use crate::widgets::grid::GridState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewKind {
    Gantt,
    TrackingGantt,
    MultipleBaselines,
    NetworkDiagram,
    Board,
    TaskUsage,
    Milestones,
    Overview,
    Deliveries,
    Report,
    ResourceSheet,
    ResourceUsage,
    Workload,
    TeamPlanner,
    Timesheet,
    Calendar,
}

impl ViewKind {
    pub fn key(self) -> &'static str {
        match self {
            ViewKind::Gantt => "view.gantt",
            ViewKind::TrackingGantt => "view.tracking",
            ViewKind::MultipleBaselines => "view.baselines",
            ViewKind::NetworkDiagram => "view.network",
            ViewKind::Board => "view.board",
            ViewKind::TaskUsage => "view.task_usage",
            ViewKind::Milestones => "view.milestones",
            ViewKind::Overview => "view.overview",
            ViewKind::Deliveries => "view.deliveries",
            ViewKind::Report => "view.report",
            ViewKind::ResourceSheet => "view.resources",
            ViewKind::ResourceUsage => "view.resource_usage",
            ViewKind::Workload => "view.workload",
            ViewKind::TeamPlanner => "view.team",
            ViewKind::Timesheet => "view.timesheet",
            ViewKind::Calendar => "view.calendar",
        }
    }

    pub fn label(self) -> &'static str {
        crate::lang::text(self.key())
    }

    /// The English name, for `PLANCRAFT_VIEW` and other stable lookups.
    pub fn english(self) -> &'static str {
        crate::lang::tr(crate::lang::Lang::En, self.key())
    }

    pub fn icon(self) -> &'static str {
        match self {
            ViewKind::Gantt => "📊",
            ViewKind::TrackingGantt => "📈",
            ViewKind::MultipleBaselines => "📚",
            ViewKind::NetworkDiagram => "🔀",
            ViewKind::Board => "📇",
            ViewKind::TaskUsage => "📝",
            ViewKind::Milestones => "♦",
            ViewKind::Overview => "◎",
            ViewKind::Deliveries => "✉",
            ViewKind::Report => "📄",
            ViewKind::ResourceSheet => "👥",
            ViewKind::ResourceUsage => "📋",
            ViewKind::Workload => "📶",
            ViewKind::TeamPlanner => "👤",
            ViewKind::Timesheet => "⏱",
            ViewKind::Calendar => "📅",
        }
    }

    pub const ALL: [ViewKind; 16] = [
        ViewKind::Gantt,
        ViewKind::TrackingGantt,
        ViewKind::MultipleBaselines,
        ViewKind::NetworkDiagram,
        ViewKind::Board,
        ViewKind::TaskUsage,
        ViewKind::Milestones,
        ViewKind::Overview,
        ViewKind::Deliveries,
        ViewKind::Report,
        ViewKind::Calendar,
        ViewKind::ResourceSheet,
        ViewKind::ResourceUsage,
        ViewKind::Workload,
        ViewKind::TeamPlanner,
        ViewKind::Timesheet,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTab {
    Task,
    Resource,
    Report,
    Project,
    View,
    Format,
}

impl MenuTab {
    pub const ALL: [MenuTab; 6] = [MenuTab::Task, MenuTab::Resource, MenuTab::Project, MenuTab::Report, MenuTab::View, MenuTab::Format];
    pub fn key(self) -> &'static str {
        match self {
            MenuTab::Task => "tab.task",
            MenuTab::Resource => "tab.resource",
            MenuTab::Report => "tab.report",
            MenuTab::Project => "tab.project",
            MenuTab::View => "tab.view",
            MenuTab::Format => "tab.format",
        }
    }

    pub fn label(self) -> &'static str {
        crate::lang::text(self.key())
    }

    pub fn english(self) -> &'static str {
        crate::lang::tr(crate::lang::Lang::En, self.key())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TaskFilter {
    #[default]
    All,
    Critical,
    Incomplete,
    Completed,
    Milestones,
    Late,
    Unassigned,
    Summary,
}

/// How a task page groups the rows it already filtered. None leaves the outline (or the toolbar group) as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PageGroup {
    #[default]
    None,
    Milestones,
    Critical,
    Priority,
    Resources,
    Status,
}

impl PageGroup {
    pub const ALL: [PageGroup; 6] =
        [PageGroup::None, PageGroup::Milestones, PageGroup::Critical, PageGroup::Priority, PageGroup::Resources, PageGroup::Status];

    pub fn label(self) -> &'static str {
        crate::lang::text(match self {
            PageGroup::None => "group.none",
            PageGroup::Milestones => "group.milestones",
            PageGroup::Critical => "group.critical",
            PageGroup::Priority => "group.priority",
            PageGroup::Resources => "group.resources",
            PageGroup::Status => "group.status",
        })
    }
}

impl TaskFilter {
    pub const ALL: [TaskFilter; 8] = [
        TaskFilter::All,
        TaskFilter::Critical,
        TaskFilter::Incomplete,
        TaskFilter::Completed,
        TaskFilter::Late,
        TaskFilter::Milestones,
        TaskFilter::Summary,
        TaskFilter::Unassigned,
    ];
    pub fn label(self) -> &'static str {
        crate::lang::text(match self {
            TaskFilter::All => "filter.all",
            TaskFilter::Critical => "filter.critical",
            TaskFilter::Incomplete => "filter.incomplete",
            TaskFilter::Completed => "filter.completed",
            TaskFilter::Milestones => "filter.milestones",
            TaskFilter::Late => "filter.late",
            TaskFilter::Unassigned => "filter.unassigned",
            TaskFilter::Summary => "filter.summary",
        })
    }
}

/// Network Diagram drag in progress.
#[derive(Clone, Debug)]
pub enum NetDrag {
    /// Moving the view (diagram point under the pointer when the drag started).
    Pan { grab: egui::Pos2 },
    /// Drawing a link from a box.
    Link { from: TaskId },
}

/// A card (or the selected cards) being dragged on the Task Board.
#[derive(Clone, Debug)]
pub struct BoardDrag {
    pub tasks: Vec<TaskId>,
}

/// The Board Column window: rename a column and set its % complete, or add one.
#[derive(Clone, Debug)]
pub struct ColumnEdit {
    /// The column changed (`None`: a new one).
    pub id: Option<plancraft_core::ids::BoardColumnId>,
    pub name: String,
    pub percent: String,
    pub error: Option<String>,
}

impl ColumnEdit {
    pub fn new_column() -> Self {
        Self { id: None, name: String::new(), percent: String::new(), error: None }
    }

    pub fn existing(c: &plancraft_core::board::BoardColumn) -> Self {
        Self { id: Some(c.id), name: c.name.clone(), percent: c.percent_complete.map(|v| v.to_string()).unwrap_or_default(), error: None }
    }
}

/// Task Board view state.
pub struct BoardState {
    pub scroll: egui::Vec2,
    pub drag: Option<BoardDrag>,
    pub edit: Option<ColumnEdit>,
    /// What the open context menu is for: a column header (`Some(id)`) or cards (`None`).
    pub menu: Option<Option<plancraft_core::ids::BoardColumnId>>,
    /// Where the cards were drawn on the last frame (columns start at its left edge).
    pub body: egui::Rect,
    /// Scroll the selected card into view on the next frame.
    pub reveal_pending: bool,
}

impl Default for BoardState {
    fn default() -> Self {
        Self { scroll: egui::Vec2::ZERO, drag: None, edit: None, menu: None, body: egui::Rect::NOTHING, reveal_pending: false }
    }
}

/// Network Diagram view: where it is scrolled to, zoom, options and the cached layout.
pub struct NetworkState {
    /// Diagram point at the top-left corner of the view.
    pub offset: egui::Pos2,
    pub zoom: f32,
    pub opts: plancraft_core::network::NetworkOptions,
    pub show_summaries: bool,
    pub link_labels: bool,
    pub progress_marks: bool,
    pub drag: Option<NetDrag>,
    /// Zoom out to show the whole diagram on the next frame.
    pub fit_pending: bool,
    /// Scroll the selected task into view on the next frame.
    pub reveal_pending: bool,
    /// Zoom to the selected tasks on the next frame.
    pub fit_selected_pending: bool,
    /// Layout of the last frame, with the key it was made for.
    pub cache: Option<(u64, std::sync::Arc<plancraft_core::network::Diagram>)>,
    /// Document the view was last shown for (another one starts at the top left).
    pub shown_for: Option<u64>,
    /// Where the view was drawn on the last frame (for zooming about its centre).
    pub view_rect: egui::Rect,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            offset: egui::Pos2::ZERO,
            zoom: 1.0,
            opts: Default::default(),
            show_summaries: true,
            link_labels: false,
            progress_marks: true,
            drag: None,
            fit_pending: false,
            reveal_pending: true,
            fit_selected_pending: false,
            cache: None,
            shown_for: None,
            view_rect: egui::Rect::NOTHING,
        }
    }
}

/// Gantt-chart drag in progress.
#[derive(Clone, Debug)]
pub enum BarDrag {
    Move {
        task: TaskId,
        grab_dx: f32,
        origin_start: DateTime,
    },
    Resize {
        task: TaskId,
    },
    Progress {
        task: TaskId,
    },
    Link {
        from: TaskId,
    },
    /// A later part of a split task (part `k`, from 0), moved on its own.
    Part {
        task: TaskId,
        k: usize,
        grab_dx: f32,
        origin: DateTime,
    },
    /// The right end of part `k` (not the last) of a split task.
    PartEnd {
        task: TaskId,
        k: usize,
    },
}

#[derive(Clone, Debug)]
pub struct GanttState {
    /// Time at the left edge of the chart.
    pub left: DateTime,
    pub px_per_day: f32,
    pub drag: Option<BarDrag>,
    pub show_critical: bool,
    /// Highlight the selected task's predecessor and successor chain.
    pub show_paths: bool,
    pub show_baseline: bool,
    /// Split Task: the next click on a bar splits the task there.
    pub split_mode: bool,
    /// The baseline (0–10) the Gantt Chart and Tracking Gantt draw under each bar.
    pub baseline_slot: u8,
    /// Baselines shown in the Multiple Baselines Gantt (bit n = Baseline n).
    pub multi_baselines: u16,
    pub show_slack: bool,
    pub show_links: bool,
    pub show_resource_names: bool,
    pub show_progress: bool,
    pub show_project_summary: bool,
    pub initialized_for: Option<uuid::Uuid>,
}

impl Default for GanttState {
    fn default() -> Self {
        Self {
            left: plancraft_core::dates::now(),
            px_per_day: 24.0,
            drag: None,
            show_critical: false,
            show_paths: false,
            show_baseline: false,
            split_mode: false,
            baseline_slot: 0,
            multi_baselines: 0b111,
            show_slack: false,
            show_links: true,
            show_resource_names: true,
            show_progress: true,
            show_project_summary: false,
            initialized_for: None,
        }
    }
}

impl GanttState {
    pub fn x_of(&self, chart_left: f32, t: DateTime) -> f32 {
        let mins = t.signed_duration_since(self.left).num_minutes() as f32;
        chart_left + mins / 1440.0 * self.px_per_day
    }

    pub fn time_at(&self, chart_left: f32, x: f32) -> DateTime {
        let days = (x - chart_left) / self.px_per_day;
        plancraft_core::dates::add_minutes(self.left, (days * 1440.0) as i64)
    }

    /// Zoom presets (px per day), from years to hours.
    pub const ZOOMS: [f32; 9] = [1.0, 2.5, 5.0, 10.0, 16.0, 24.0, 40.0, 72.0, 140.0];

    pub fn zoom(&mut self, dir: i32, anchor_x: f32, chart_left: f32) {
        let anchor_t = self.time_at(chart_left, anchor_x);
        let idx = Self::ZOOMS.iter().position(|z| *z >= self.px_per_day - 0.01).unwrap_or(5) as i32;
        let n = (idx + dir).clamp(0, Self::ZOOMS.len() as i32 - 1) as usize;
        self.px_per_day = Self::ZOOMS.get(n).copied().unwrap_or(self.px_per_day);
        // Keep the anchor time under the cursor.
        let days = (anchor_x - chart_left) / self.px_per_day;
        self.left = plancraft_core::dates::add_minutes(anchor_t, -((days * 1440.0) as i64));
    }

    /// Continuous zoom by a factor, keeping the time under `anchor_x` fixed.
    pub fn zoom_by(&mut self, factor: f32, anchor_x: f32, chart_left: f32) {
        let anchor_t = self.time_at(chart_left, anchor_x);
        self.px_per_day = (self.px_per_day * factor).clamp(0.5, 160.0);
        let days = (anchor_x - chart_left) / self.px_per_day;
        self.left = plancraft_core::dates::add_minutes(anchor_t, -((days * 1440.0) as i64));
    }

    pub fn scroll_to(&mut self, t: DateTime) {
        self.left = plancraft_core::dates::add_minutes(t, -2 * 1440);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub at: Instant,
}

#[derive(Default)]
pub struct Selection {
    pub tasks: BTreeSet<TaskId>,
    pub anchor: Option<TaskId>,
    pub resources: BTreeSet<ResourceId>,
    pub resource_anchor: Option<ResourceId>,
}

impl Selection {
    pub fn single_task(&self) -> Option<TaskId> {
        if self.tasks.len() == 1 { self.tasks.iter().next().copied() } else { self.anchor.filter(|a| self.tasks.contains(a)) }
    }
    pub fn select_task(&mut self, id: TaskId) {
        self.tasks.clear();
        self.tasks.insert(id);
        self.anchor = Some(id);
    }
    pub fn single_resource(&self) -> Option<ResourceId> {
        if self.resources.len() == 1 {
            self.resources.iter().next().copied()
        } else {
            self.resource_anchor.filter(|a| self.resources.contains(a))
        }
    }
    pub fn select_resource(&mut self, id: ResourceId) {
        self.resources.clear();
        self.resources.insert(id);
        self.resource_anchor = Some(id);
    }
}

/// How a document's task views are sorted and grouped (kept per document).
#[derive(Clone, Debug, Default)]
pub struct ViewOrder {
    pub sort: Option<(String, bool)>,
    pub group: Option<String>,
    pub collapsed_groups: HashSet<String>,
    /// Tasks whose notes row is open. Kept per document, not in the plan file.
    pub notes_open: HashSet<TaskId>,
}

pub struct UiState {
    pub view: ViewKind,
    /// Interface language. Dutch unless the user picks English.
    pub lang: crate::lang::Lang,
    pub menu_tab: MenuTab,
    /// The Start Page is shown instead of the views.
    pub start_page: bool,
    pub sel: Selection,
    pub task_grid: GridState,
    pub resource_grid: GridState,
    pub task_usage_grid: GridState,
    pub resource_usage_grid: GridState,
    pub misc_grid: GridState,
    pub collapsed: HashSet<TaskId>,
    /// Tasks whose notes are shown as a row under them. Not stored in the plan.
    pub notes_open: HashSet<TaskId>,
    pub collapsed_resources: HashSet<ResourceId>,
    pub filter: TaskFilter,
    /// The activity the task views are narrowed to, plus every view's other criteria.
    pub focus: Option<ActivityFocus>,
    pub status_filter: Option<TaskStatus>,
    pub kind_filter: TaskKindFilter,
    pub date_window: DateWindow,
    pub date_from: Option<Date>,
    pub date_to: Option<Date>,
    /// Empty means every resource. Otherwise a task must be assigned to one of these.
    pub filter_resources: Vec<ResourceId>,
    pub search: String,
    /// Sort of task views: field key and descending.
    pub sort: Option<(String, bool)>,
    /// Field key task views are grouped by.
    pub group: Option<String>,
    /// Collapsed group headers (by label).
    pub collapsed_groups: HashSet<String>,
    /// Sort, grouping and collapsed groups of the documents not active now (by handle): field
    /// keys like `custom:3` mean different fields in different plans.
    pub view_orders: std::collections::HashMap<u64, ViewOrder>,
    /// Column widths of task tables, by table and field key (survive column changes).
    pub column_widths: std::collections::HashMap<(TaskTable, String), f32>,
    pub table: TaskTable,
    pub gantt: GanttState,
    pub network: NetworkState,
    pub board: BoardState,
    pub table_width: f32,
    pub timeline_visible: bool,
    /// The properties pane on the right of the active view.
    pub properties_visible: bool,
    /// Width of the properties pane, in points.
    pub properties_width: f32,
    /// Design style for this session. Copied from the plan when that plan becomes active.
    pub chart_style: plancraft_core::model::ChartStyle,
    /// The document handle `chart_style` was last taken from.
    pub style_doc: Option<u64>,
    /// Grouping chosen on the page filter bar.
    pub page_group: PageGroup,
    /// Resource pages: a category, or every category.
    pub resource_category: Option<ResourceCategory>,
    /// Overview: one resource name, or everyone.
    pub agenda_resource: Option<String>,
    /// Overview day strip: show only items that start or finish on this day.
    pub agenda_day: Option<Date>,
    pub granularity: Granularity,
    pub usage_from: Option<Date>,
    pub timesheet_week: Date,
    pub timesheet_resource: Option<ResourceId>,
    pub calendar_month: Date,
    pub toasts: Vec<Toast>,
    pub clipboard: Vec<Task>,
    /// In the active plan, if opened from a server: who else is on each task (names, joined).
    pub others_on: std::collections::HashMap<TaskId, String>,
}

impl Default for UiState {
    fn default() -> Self {
        let today = plancraft_core::dates::today();
        let monday = plancraft_core::dates::shift_days(today, -(today.weekday().num_days_from_monday() as i64));
        Self {
            view: ViewKind::Gantt,
            lang: crate::lang::Lang::Nl,
            menu_tab: MenuTab::Task,
            start_page: false,
            sel: Selection::default(),
            task_grid: GridState::default(),
            resource_grid: GridState::default(),
            task_usage_grid: GridState::default(),
            resource_usage_grid: GridState::default(),
            misc_grid: GridState::default(),
            collapsed: HashSet::new(),
            notes_open: HashSet::new(),
            collapsed_resources: HashSet::new(),
            filter: TaskFilter::All,
            focus: None,
            status_filter: None,
            kind_filter: TaskKindFilter::All,
            date_window: DateWindow::All,
            date_from: None,
            date_to: None,
            filter_resources: Vec::new(),
            search: String::new(),
            sort: None,
            group: None,
            collapsed_groups: HashSet::new(),
            view_orders: Default::default(),
            column_widths: std::collections::HashMap::new(),
            table: TaskTable::Entry,
            gantt: GanttState::default(),
            network: NetworkState::default(),
            board: BoardState::default(),
            table_width: 720.0,
            timeline_visible: true,
            properties_visible: false,
            properties_width: 340.0,
            chart_style: plancraft_core::model::ChartStyle::Blue,
            style_doc: None,
            page_group: PageGroup::None,
            resource_category: None,
            agenda_resource: None,
            agenda_day: None,
            granularity: Granularity::Day,
            usage_from: None,
            timesheet_week: monday,
            timesheet_resource: None,
            calendar_month: today.with_day(1).unwrap_or(today),
            toasts: Vec::new(),
            clipboard: Vec::new(),
            others_on: Default::default(),
        }
    }
}

/// How a focused activity limits the task views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivityFocus {
    pub task: TaskId,
    pub scope: FocusScope,
}

/// Task and subtasks follows the outline. Dependency path follows links both ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FocusScope {
    #[default]
    Subtree,
    Path,
}

impl FocusScope {
    pub const ALL: [FocusScope; 2] = [FocusScope::Subtree, FocusScope::Path];

    pub fn label(self) -> &'static str {
        crate::lang::text(match self {
            FocusScope::Subtree => "filter.subtree",
            FocusScope::Path => "filter.path",
        })
    }
}

/// Tasks, milestones or groups. `All` leaves the outline as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TaskKindFilter {
    #[default]
    All,
    Tasks,
    Milestones,
    Groups,
}

impl TaskKindFilter {
    pub const ALL: [TaskKindFilter; 4] = [TaskKindFilter::All, TaskKindFilter::Tasks, TaskKindFilter::Milestones, TaskKindFilter::Groups];

    pub fn label(self) -> &'static str {
        crate::lang::text(match self {
            TaskKindFilter::All => "filter.type_all",
            TaskKindFilter::Tasks => "filter.tasks",
            TaskKindFilter::Milestones => "filter.milestones",
            TaskKindFilter::Groups => "filter.groups",
        })
    }
}

/// Which dates a task view keeps. `Range` uses [`UiState::date_from`] and [`UiState::date_to`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DateWindow {
    #[default]
    All,
    Overdue,
    ThisWeek,
    NextTwoWeeks,
    Range,
}

impl DateWindow {
    pub const ALL: [DateWindow; 5] =
        [DateWindow::All, DateWindow::Overdue, DateWindow::ThisWeek, DateWindow::NextTwoWeeks, DateWindow::Range];

    pub fn label(self) -> &'static str {
        crate::lang::text(match self {
            DateWindow::All => "filter.dates_all",
            DateWindow::Overdue => "filter.overdue",
            DateWindow::ThisWeek => "filter.this_week",
            DateWindow::NextTwoWeeks => "filter.next_two",
            DateWindow::Range => "filter.date_range",
        })
    }
}

impl UiState {
    /// The label for `key` in this session's language.
    pub fn t(&self, key: &'static str) -> &'static str {
        crate::lang::tr(self.lang, key)
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), kind, at: Instant::now() });
    }
}
