//! Modal-style windows.

pub mod ai;
pub mod baseline;
pub mod custom_fields;
pub mod distill;
pub mod find;
pub mod import;
pub mod mcp_panel;
pub mod misc;
pub mod planner_export;
pub mod planner_send;
pub mod print;
pub mod project_info;
pub mod quick_entry;
pub mod recurring;
pub mod resource_delete;
pub mod resource_info;
pub mod server;
pub mod task_info;
pub mod update_project;
pub mod working_time;

use plancraft_core::model::Project;
use plancraft_core::workspace::{Document, Workspace};

/// A dialog bound to the document it was opened for (by document handle). Task and resource ids
/// repeat across documents, so a dialog that applied to "the active document" would write one
/// plan's edits into another after a tab switch.
pub struct Bound<T> {
    pub doc: u64,
    pub dialog: T,
}

impl<T> Bound<T> {
    pub fn new(doc: &Document, dialog: T) -> Self {
        Self { doc: doc.handle, dialog }
    }
}

/// Session-routed undoable edit for a bound dialog's document (prefer over [`crate::edit::EditExt`]
/// when the caller already holds the workspace).
pub fn edit_bound<R>(ws: &mut Workspace, handle: u64, label: &str, f: impl FnOnce(&mut Project) -> Result<R, String>) -> Result<R, String> {
    crate::engine_bridge::edit_doc(ws, handle, label, f)
}

/// Shows a bound dialog against its own document; `show` returns whether it stays open. The
/// dialog closes when its document has been closed.
///
/// Prefer [`show_bound_ws`] + [`edit_bound`] for apply paths so mutations go through Session.
pub fn show_bound<T>(slot: &mut Option<Bound<T>>, ws: &mut Workspace, show: impl FnOnce(&mut T, &mut Document) -> bool) {
    let Some(b) = slot else { return };
    let keep = match ws.index_of(b.doc).and_then(|i| ws.docs.get_mut(i)) {
        Some(doc) => show(&mut b.dialog, doc),
        None => false,
    };
    if !keep {
        *slot = None;
    }
}

/// Like [`show_bound`], but passes the workspace so apply can use [`edit_bound`] / engine commands.
pub fn show_bound_ws<T>(slot: &mut Option<Bound<T>>, ws: &mut Workspace, show: impl FnOnce(&mut T, &mut Workspace, u64) -> bool) {
    let Some(b) = slot else { return };
    let handle = b.doc;
    if ws.index_of(handle).is_none() {
        *slot = None;
        return;
    }
    let keep = show(&mut b.dialog, ws, handle);
    if !keep {
        *slot = None;
    }
}

#[derive(Default)]
pub struct Dialogs {
    pub task_info: Option<Bound<task_info::TaskInfo>>,
    pub find: Option<Bound<find::FindReplace>>,
    pub goto: Option<Bound<find::GoTo>>,
    pub resource_info: Option<Bound<resource_info::ResourceInfo>>,
    pub project_info: Option<Bound<project_info::ProjectInfo>>,
    pub working_time: Option<Bound<working_time::WorkingTime>>,
    pub custom_fields: Option<Bound<custom_fields::CustomFields>>,
    pub print: Option<Bound<print::PrintDialog>>,
    pub update_project: Option<Bound<update_project::UpdateProject>>,
    pub set_baseline: Option<Bound<baseline::SetBaseline>>,
    pub clear_baseline: Option<Bound<baseline::ClearBaseline>>,
    pub recurring: Option<Bound<recurring::RecurringTask>>,
    /// Follows the active document's selection; reset when the active document changes.
    pub assign: Option<Bound<misc::AssignResources>>,
    pub resource_delete: Option<Bound<resource_delete::ResourceDelete>>,
    pub external: Option<Bound<misc::ExternalLinks>>,
    /// Import Spreadsheet windows, shown one at a time (not bound to a document: each makes a new
    /// one). Several files opened at once each get their turn.
    pub import: Vec<import::ImportSheet>,
    pub stats: bool,
    pub about: bool,
    pub history: bool,
    pub mcp: bool,
    pub distill: Option<Bound<distill::Distill>>,
    pub planner_export: Option<Bound<planner_export::PlannerExport>>,
    pub planner_send: Option<Bound<planner_send::PlannerSend>>,
    pub quick: Option<Bound<quick_entry::QuickEntry>>,
    pub ai_open: bool,
    pub ai: ai::AiState,
}
