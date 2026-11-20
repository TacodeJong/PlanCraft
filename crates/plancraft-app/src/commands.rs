//! User commands issued from the menu bar, toolbar, menus, keyboard shortcuts and views.

use plancraft_core::model::ScheduleMode;

use crate::columns::TaskTable;
use crate::state::{TaskFilter, ViewKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    // File
    NewProject,
    NewDemo,
    Open,
    OpenPath(std::path::PathBuf),
    /// The Open from Server window: a plan shared from a PlanCraft Server.
    OpenFromServer,
    Save,
    SaveAs,
    Close,
    Undo,
    Redo,
    /// Save one of the plan's tables (tasks, resources, assignments, logged time) as CSV.
    ExportCsv(plancraft_core::tabular::CsvTable),
    /// Pick a CSV or spreadsheet file to import as a new plan.
    ImportSpreadsheet,
    /// The Print window (preview, page setup, print, save as PDF).
    Print,
    /// Save the current view as a PDF with the plan's page setup.
    ExportPdf,
    /// Save the plan as a Microsoft Project XML (MSPDI) file.
    ExportMspdi,
    /// Save the Report view's visible rows as an Excel workbook.
    ExportReport,
    /// Coloured-cell Gantt workbook of the tasks the current filter leaves visible.
    ExportGantt,
    /// The design style, stored on the plan and used for this session.
    SetChartStyle(plancraft_core::model::ChartStyle),
    Quit,
    StartPage,
    /// Restore a recovery copy (by its stem in the recovery folder).
    RestoreRecovered(String),
    /// Delete a recovery copy.
    DiscardRecovered(String),
    // Tasks
    InsertTask,
    InsertMilestone,
    InsertSummary,
    /// The Recurring Task window, for a new recurring task.
    RecurringTask,
    /// Split Task: the next click on a Gantt bar splits that task.
    SplitTask,
    /// Remove the splits planned in the selected tasks.
    RejoinTasks,
    DeleteSelection,
    Indent,
    Outdent,
    MoveUp,
    MoveDown,
    LinkSelected,
    UnlinkSelected,
    SetPercent(u8),
    MarkOnTrack,
    ToggleActive,
    SetMode(ScheduleMode),
    TaskInfo,
    TaskNotes,
    ToggleTimeline,
    ScrollToTask,
    Cut,
    Copy,
    Paste,
    /// Copy the top selected cell's value into the selected cells below it (Ctrl+D).
    FillDown,
    /// The Find window (`true`: with Replace).
    Find(bool),
    /// Go To a task ID or a date (F5).
    GoTo,
    // Resources
    InsertResource,
    DeleteResources,
    ResourceInfo,
    AssignResources,
    LevelAll,
    ClearLeveling,
    // Project
    ProjectInfo,
    ChangeWorkingTime,
    UpdateProject,
    SetBaseline,
    ClearBaseline,
    UpdateProgressFromTime,
    ExternalLinks,
    RefreshExternal,
    Statistics,
    McpPanel,
    /// Turn a pasted brief into tasks the user reviews before they are added.
    Distill,
    /// Provider, model and API key for Distill.
    AiSettings,
    /// The Quick Entry sheet: several lines become tasks in one undo step.
    QuickEntry,
    // View
    SetView(ViewKind),
    ZoomIn,
    ZoomOut,
    ZoomEntire,
    ZoomSelected,
    Filter(TaskFilter),
    Table(TaskTable),
    /// Sort task views by a field key (`true` = descending); `None` restores the outline order.
    SortBy(Option<(String, bool)>),
    /// Group task views by a field key; `None` ungroups.
    GroupBy(Option<String>),
    /// Insert a column showing field `key` before column `at` of the current table.
    InsertColumn {
        at: usize,
        key: String,
    },
    HideColumn(usize),
    MoveColumn {
        at: usize,
        right: bool,
    },
    /// Back to the current table's default columns.
    ResetColumns,
    /// Switch the column with field key `key` on or off for this user (the Columns menu;
    /// saved in the app settings, not the plan).
    ToggleColumn(String),
    CustomFields,
    CollapseAll,
    ExpandAll,
    /// Highlight the selected task's predecessor and successor chain (Alt+Ctrl+L).
    ToggleHighlightPaths,
    History,
    SetTheme(crate::theme::ThemeChoice),
    SetLang(crate::lang::Lang),
    SetStartup(crate::settings::Startup),
    SetOpenOverview(bool),
    About,
}

/// Context menu shared by the task table and the Gantt chart.
pub fn task_context_menu(ui: &mut egui::Ui, emit: &mut dyn FnMut(Command)) {
    let mut item = |ui: &mut egui::Ui, icon: &'static str, label: &str, cmd: Command| {
        let img = crate::icons::image(icon, 14.0, crate::theme::pal().TEXT);
        if ui.add(egui::Button::image_and_text(img, label)).clicked() {
            emit(cmd);
            ui.close();
        }
    };
    item(ui, "info", "Task Information…", Command::TaskInfo);
    item(ui, "pen-line", "Notes…", Command::TaskNotes);
    item(ui, "users-round", "Assign Resources…", Command::AssignResources);
    ui.separator();
    item(ui, "plus", "Insert Task", Command::InsertTask);
    item(ui, "diamond", "Insert Milestone", Command::InsertMilestone);
    item(ui, "trash-2", "Delete Task", Command::DeleteSelection);
    ui.separator();
    item(ui, "indent-increase", "Indent", Command::Indent);
    item(ui, "indent-decrease", "Outdent", Command::Outdent);
    item(ui, "link", "Link Tasks", Command::LinkSelected);
    item(ui, "unlink", "Unlink Tasks", Command::UnlinkSelected);
    ui.separator();
    item(ui, "check", "Mark 100% Complete", Command::SetPercent(100));
    item(ui, "calendar", "Add to Timeline", Command::ToggleTimeline);
    item(ui, "pause", "Inactivate / Activate", Command::ToggleActive);
    item(ui, "split", "Split Task", Command::SplitTask);
    item(ui, "rotate-ccw", "Rejoin Split Task", Command::RejoinTasks);
    ui.separator();
    item(ui, "scissors", "Cut", Command::Cut);
    item(ui, "copy", "Copy", Command::Copy);
    item(ui, "clipboard-paste", "Paste", Command::Paste);
    item(ui, "arrow-down", "Fill Down (Ctrl+D)", Command::FillDown);
    ui.separator();
    item(ui, "search", "Find… (Ctrl+F)", Command::Find(false));
    item(ui, "arrow-right", "Go To… (F5)", Command::GoTo);
}
