//! Snapshot of UI/document state menus need, plus shared enablement / shortcut metadata.

use std::path::PathBuf;

use egui::{Key, KeyboardShortcut, Modifiers};

use crate::commands::Command;
use crate::lang::Lang;
use crate::state::ViewKind;
use crate::theme::ThemeChoice;

/// Everything the menu tree reads. Built while the workspace lock is held; then used without it.
#[derive(Clone, Debug)]
pub struct MenuSnapshot {
    pub has_doc: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub task_selected: bool,
    /// How many tasks are selected. Link needs at least two; Unlink may use one.
    pub selected_task_count: usize,
    pub resource_selected: bool,
    pub has_clipboard: bool,
    /// A TextEdit (notes, name, …) owns keyboard focus — not the task grid.
    /// Keeps Edit ▸ Paste enabled so macOS does not swallow ⌘V when the task clipboard is empty.
    pub text_edit_focused: bool,
    pub view: ViewKind,
    pub timeline_visible: bool,
    pub properties_visible: bool,
    pub word_wrap: bool,
    pub history_open: bool,
    pub theme: ThemeChoice,
    pub lang: Lang,
    pub show_critical: bool,
    pub show_paths: bool,
    pub show_slack: bool,
    pub show_baseline: bool,
    pub show_progress: bool,
    pub show_links: bool,
    pub show_resource_names: bool,
    pub show_project_summary: bool,
    pub baseline_slot: u8,
    pub multi_baselines: u16,
    pub network_critical_first: bool,
    pub network_summaries: bool,
    pub network_link_labels: bool,
    pub network_progress_marks: bool,
    pub network_ids_only: bool,
    /// Recent paths with whether they still exist on disk.
    pub recent: Vec<(PathBuf, bool)>,
    /// Open documents: handle, title, whether active.
    pub documents: Vec<(u64, String, bool)>,
    /// Save/close prompts grey document actions and reject raced clicks.
    pub modal: bool,
    pub start_page: bool,
}

impl MenuSnapshot {
    pub fn document_actions_ok(&self) -> bool {
        self.has_doc && !self.modal && !self.start_page
    }
}

/// Whether a command may run from a menu or shortcut right now.
pub fn command_enabled(cmd: &Command, s: &MenuSnapshot) -> bool {
    if s.modal {
        return matches!(cmd, Command::Quit | Command::About | Command::Settings | Command::AiSettings | Command::McpPanel);
    }
    let doc = s.document_actions_ok();
    let sel = doc && s.task_selected;
    let res = doc && s.resource_selected;
    let linkable = doc && s.selected_task_count >= 2;
    match cmd {
        Command::NewProject | Command::NewDemo | Command::Open | Command::OpenFromServer | Command::ImportSpreadsheet => true,
        Command::OpenPath(_) => true,
        Command::ClearRecent => !s.recent.is_empty(),
        Command::Save
        | Command::SaveAs
        | Command::Close
        | Command::Print
        | Command::ExportPdf
        | Command::ExportMspdi
        | Command::ExportGantt
        | Command::ExportPlanner => doc,
        Command::ExportCsv(_) => doc,
        Command::ExportReport => doc && s.view == ViewKind::Report,
        Command::ExportUsage => doc && s.view == ViewKind::TaskUsage,
        Command::SetTimescale(_) => doc,
        Command::Undo => doc && s.can_undo,
        Command::Redo => doc && s.can_redo,
        // Text fields need Cut/Copy/Paste even with no task clipboard (macOS eats disabled accelerators).
        Command::Cut | Command::Copy => sel || s.text_edit_focused,
        Command::Paste => doc && (s.has_clipboard || s.text_edit_focused),
        Command::FillDown | Command::Indent | Command::Outdent | Command::MoveUp | Command::MoveDown => sel,
        // Delete works on tasks, or on resources while a resource view is active (Mac Delete key /
        // Edit ▸ Delete). Resource Sheet redirects to DeleteResources.
        Command::DeleteSelection => {
            sel || (doc
                && s.resource_selected
                && matches!(
                    s.view,
                    ViewKind::ResourceSheet | ViewKind::ResourceUsage | ViewKind::Workload | ViewKind::TeamPlanner | ViewKind::Timesheet
                ))
        }
        // Link needs two or more selected tasks; Unlink may validly act on a single selection.
        Command::LinkSelected => linkable,
        Command::UnlinkSelected | Command::SetPercent(_) | Command::MarkOnTrack | Command::ToggleActive => sel,
        Command::SetMode(_)
        | Command::ClearDates
        | Command::TaskInfo
        | Command::TaskNotes
        | Command::ToggleTimeline
        | Command::ScrollToTask => sel,
        Command::InsertTask
        | Command::InsertMilestone
        | Command::InsertSummary
        | Command::RecurringTask
        | Command::SplitTask
        | Command::RejoinTasks
        | Command::QuickEntry
        | Command::Distill => doc,
        Command::Find(_) | Command::GoTo => doc,
        Command::InsertResource => doc,
        Command::DeleteResources | Command::ResourceInfo => res,
        Command::AssignResources => sel,
        Command::LevelAll | Command::ClearLeveling => doc,
        Command::ProjectInfo
        | Command::ChangeWorkingTime
        | Command::UpdateProject
        | Command::SetBaseline
        | Command::ClearBaseline
        | Command::UpdateProgressFromTime
        | Command::ExternalLinks
        | Command::RefreshExternal
        | Command::Statistics
        | Command::CustomFields => doc,
        Command::SetView(_) => true,
        Command::ZoomIn | Command::ZoomOut | Command::ZoomEntire => doc,
        Command::ZoomSelected => sel,
        Command::ToggleTimelinePane | Command::TogglePropertiesPane | Command::History => doc,
        Command::ToggleHighlightPaths
        | Command::ToggleShowCritical
        | Command::ToggleShowSlack
        | Command::ToggleShowBaseline
        | Command::ToggleShowProgress
        | Command::ToggleShowLinks
        | Command::ToggleShowResourceNames
        | Command::ToggleShowProjectSummary
        | Command::SetBaselineSlot(_)
        | Command::ToggleMultiBaseline(_) => doc,
        Command::ToggleNetworkCriticalFirst
        | Command::ToggleNetworkSummaries
        | Command::ToggleNetworkLinkLabels
        | Command::ToggleNetworkProgressMarks
        | Command::ToggleNetworkIdsOnly => doc && s.view == ViewKind::NetworkDiagram,
        Command::SetTheme(_) | Command::SetLang(_) | Command::SetStartup(_) | Command::SetOpenOverview(_) | Command::ToggleWordWrap => true,
        Command::SetChartStyle(_) => doc,
        Command::About | Command::Settings | Command::AiSettings | Command::McpPanel | Command::Quit | Command::StartPage => true,
        Command::ActivateDocument(_) => true,
        Command::MinimizeWindow | Command::HideApp => true,
        Command::Filter(_) | Command::Table(_) | Command::SortBy(_) | Command::GroupBy(_) => doc,
        Command::CollapseAll | Command::ExpandAll => doc,
        Command::ToggleColumn(_)
        | Command::InsertColumn { .. }
        | Command::HideColumn(_)
        | Command::MoveColumn { .. }
        | Command::MoveColumnTo { .. }
        | Command::ResetColumns => doc,
        Command::RestoreRecovered(_) | Command::DiscardRecovered(_) => true,
    }
}

/// Check/radio state for menu items; `None` means a plain item.
pub fn command_checked(cmd: &Command, s: &MenuSnapshot) -> Option<bool> {
    match cmd {
        Command::SetView(v) => Some(s.view == *v),
        Command::SetTheme(t) => Some(s.theme == *t),
        Command::SetLang(l) => Some(s.lang == *l),
        Command::ToggleTimelinePane => Some(s.timeline_visible),
        Command::TogglePropertiesPane => Some(s.properties_visible),
        Command::ToggleWordWrap => Some(s.word_wrap),
        Command::History => Some(s.history_open),
        Command::ToggleHighlightPaths => Some(s.show_paths),
        Command::ToggleShowCritical => Some(s.show_critical),
        Command::ToggleShowSlack => Some(s.show_slack),
        Command::ToggleShowBaseline => Some(s.show_baseline),
        Command::ToggleShowProgress => Some(s.show_progress),
        Command::ToggleShowLinks => Some(s.show_links),
        Command::ToggleShowResourceNames => Some(s.show_resource_names),
        Command::ToggleShowProjectSummary => Some(s.show_project_summary),
        Command::SetBaselineSlot(n) => Some(s.baseline_slot == *n),
        Command::ToggleMultiBaseline(n) => {
            let bit = 1u16 << n;
            Some(s.multi_baselines & bit != 0)
        }
        Command::ToggleNetworkCriticalFirst => Some(s.network_critical_first),
        Command::ToggleNetworkSummaries => Some(s.network_summaries),
        Command::ToggleNetworkLinkLabels => Some(s.network_link_labels),
        Command::ToggleNetworkProgressMarks => Some(s.network_progress_marks),
        Command::ToggleNetworkIdsOnly => Some(s.network_ids_only),
        Command::ActivateDocument(h) => s.documents.iter().find(|(id, _, _)| id == h).map(|(_, _, active)| *active),
        _ => None,
    }
}

/// Canonical shortcut metadata shared by menus and `PlanCraftApp::shortcuts`.
pub fn shortcut_for(cmd: &Command) -> Option<KeyboardShortcut> {
    let cmd_mod = Modifiers::COMMAND;
    match cmd {
        Command::NewProject => Some(KeyboardShortcut::new(cmd_mod, Key::N)),
        Command::Open => Some(KeyboardShortcut::new(cmd_mod, Key::O)),
        Command::Save => Some(KeyboardShortcut::new(cmd_mod, Key::S)),
        Command::SaveAs => Some(KeyboardShortcut::new(cmd_mod | Modifiers::SHIFT, Key::S)),
        Command::Print => Some(KeyboardShortcut::new(cmd_mod, Key::P)),
        Command::Undo => Some(KeyboardShortcut::new(cmd_mod, Key::Z)),
        Command::Redo => Some(KeyboardShortcut::new(cmd_mod | Modifiers::SHIFT, Key::Z)),
        Command::Cut => Some(KeyboardShortcut::new(cmd_mod, Key::X)),
        Command::Copy => Some(KeyboardShortcut::new(cmd_mod, Key::C)),
        Command::Paste => Some(KeyboardShortcut::new(cmd_mod, Key::V)),
        Command::FillDown => Some(KeyboardShortcut::new(cmd_mod, Key::D)),
        Command::Find(false) => Some(KeyboardShortcut::new(cmd_mod, Key::F)),
        Command::Find(true) => Some(KeyboardShortcut::new(cmd_mod | Modifiers::ALT, Key::F)),
        Command::GoTo => Some(KeyboardShortcut::new(Modifiers::NONE, Key::F5)),
        Command::Indent => Some(KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowRight)),
        Command::Outdent => Some(KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowLeft)),
        Command::MoveUp => Some(KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowUp)),
        Command::MoveDown => Some(KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowDown)),
        Command::LinkSelected => Some(KeyboardShortcut::new(cmd_mod, Key::F2)),
        Command::UnlinkSelected => Some(KeyboardShortcut::new(cmd_mod | Modifiers::SHIFT, Key::F2)),
        Command::TaskInfo => Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::F2)),
        Command::ZoomIn => Some(KeyboardShortcut::new(cmd_mod, Key::Equals)),
        Command::ZoomOut => Some(KeyboardShortcut::new(cmd_mod, Key::Minus)),
        Command::Distill => Some(KeyboardShortcut::new(cmd_mod | Modifiers::SHIFT, Key::D)),
        Command::QuickEntry => Some(KeyboardShortcut::new(cmd_mod | Modifiers::ALT, Key::N)),
        Command::ToggleHighlightPaths => Some(KeyboardShortcut::new(cmd_mod | Modifiers::ALT, Key::L)),
        Command::Settings => Some(KeyboardShortcut::new(cmd_mod, Key::Comma)),
        Command::Quit => Some(KeyboardShortcut::new(cmd_mod, Key::Q)),
        Command::Close => Some(KeyboardShortcut::new(cmd_mod, Key::W)),
        _ => None,
    }
}

/// Portable label for a shortcut (`Cmd+S`), suitable for muda and egui `shortcut_text`.
pub fn shortcut_label(cmd: &Command) -> Option<String> {
    portable_from_shortcut(shortcut_for(cmd)?)
}

/// Platform-facing hint for a portable chord (`Cmd+S` → `⌘S` on Mac, `Ctrl+S` on Windows/Linux).
pub fn platform_shortcut_label(portable: &str, mac: bool) -> String {
    crate::menu::model::Chord::parse(portable).map(|c| c.platform_label(mac)).unwrap_or_else(|| portable.to_string())
}

/// Tooltip body plus a platform shortcut when the command has one.
pub fn tip_with_shortcut(cmd: &Command, description: &str, mac: bool) -> String {
    match shortcut_label(cmd) {
        Some(portable) => format!("{description} ({})", platform_shortcut_label(&portable, mac)),
        None => description.to_string(),
    }
}

/// Whether this host should show Mac-style shortcut glyphs.
pub fn host_is_mac() -> bool {
    cfg!(target_os = "macos")
}

fn portable_from_shortcut(sc: KeyboardShortcut) -> Option<String> {
    let mut parts = Vec::new();
    // `Modifiers::COMMAND` sets `command` (and on Windows/Linux also `ctrl`). Always emit Cmd.
    let real_ctrl = sc.modifiers.ctrl && !sc.modifiers.command && !sc.modifiers.mac_cmd;
    if real_ctrl {
        parts.push("Ctrl");
    }
    if sc.modifiers.command || sc.modifiers.mac_cmd || (sc.modifiers.ctrl && !real_ctrl) {
        parts.push("Cmd");
    }
    if sc.modifiers.alt {
        parts.push("Alt");
    }
    if sc.modifiers.shift {
        parts.push("Shift");
    }
    let key = match sc.logical_key {
        Key::N => "N",
        Key::O => "O",
        Key::S => "S",
        Key::P => "P",
        Key::Z => "Z",
        Key::Y => "Y",
        Key::X => "X",
        Key::C => "C",
        Key::V => "V",
        Key::D => "D",
        Key::F => "F",
        Key::L => "L",
        Key::W => "W",
        Key::Q => "Q",
        Key::Comma => ",",
        Key::Equals => "=",
        Key::Minus => "-",
        Key::Plus => "+",
        Key::F2 => "F2",
        Key::F5 => "F5",
        Key::ArrowRight => "ArrowRight",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowUp => "ArrowUp",
        Key::ArrowDown => "ArrowDown",
        _ => return None,
    };
    parts.push(key);
    Some(parts.join("+"))
}
