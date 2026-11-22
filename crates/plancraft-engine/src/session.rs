//! Engine session: workspace + journal + events. Sole mutation façade for frontends.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use plancraft_core::model::Project;
use plancraft_core::workspace::{ChangeSource, Document, Workspace};
use serde_json::{Value, json};

use crate::command::{CommandError, Params, find};
use crate::event::Event;

/// One journalled command run (newest last).
#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub id: String,
    pub params: Value,
    pub ok: bool,
}

/// Headless session shared by desktop, CLI and MCP.
#[derive(Debug, Default)]
pub struct Session {
    pub workspace: Workspace,
    /// Attribution for edits started through this session (`User` in the app, `Agent` in MCP).
    pub source: ChangeSource,
    events: Vec<Event>,
    journal: Vec<JournalEntry>,
}

impl Session {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace, source: ChangeSource::User, events: Vec::new(), journal: Vec::new() }
    }

    pub fn with_source(mut self, source: ChangeSource) -> Self {
        self.source = source;
        self
    }

    /// Run a registered command by id. Panics inside `run` become [`CommandError::Failed`].
    pub fn execute(&mut self, id: &str, params: &Params) -> Result<Value, CommandError> {
        let spec = find(id).ok_or_else(|| CommandError::Unknown(id.to_string()))?;
        if let Err(reason) = (spec.enabled)(self) {
            return Err(CommandError::Disabled(reason));
        }
        let result = catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params)));
        match result {
            Ok(Ok(v)) => {
                if spec.journal {
                    self.journal.push(JournalEntry { id: id.to_string(), params: params.clone(), ok: true });
                }
                Ok(v)
            }
            Ok(Err(msg)) => {
                if spec.journal {
                    self.journal.push(JournalEntry { id: id.to_string(), params: params.clone(), ok: false });
                }
                Err(CommandError::Failed(msg))
            }
            Err(_) => {
                if spec.journal {
                    self.journal.push(JournalEntry { id: id.to_string(), params: params.clone(), ok: false });
                }
                Err(CommandError::Failed(format!("command '{id}' panicked")))
            }
        }
    }

    pub fn push_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    pub fn active_doc(&self) -> Option<&Document> {
        self.workspace.active_doc()
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.workspace.active_doc_mut()
    }

    /// Undoable edit on the active document, attributed with [`Self::source`].
    pub fn edit_active<R>(&mut self, label: &str, f: impl FnOnce(&mut Project) -> R) -> Result<R, String> {
        self.try_edit_active(label, |p| Ok::<R, plancraft_core::ops::PlanError>(f(p)))
    }

    /// Like [`Self::edit_active`], but the closure may fail and roll the document back.
    pub fn try_edit_active<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Project) -> plancraft_core::ops::PlanResult<R>,
    ) -> Result<R, String> {
        let source = self.source;
        let doc = self.active_doc_mut().ok_or_else(|| "no open document".to_string())?;
        let r = doc.try_edit(label, source, f).map_err(|e| e.to_string())?;
        self.push_event(Event::ProjectChanged);
        Ok(r)
    }

    /// Undoable edit on the document with `handle` (activates it for the duration).
    pub fn try_edit_doc<R>(
        &mut self,
        handle: u64,
        label: &str,
        f: impl FnOnce(&mut Project) -> plancraft_core::ops::PlanResult<R>,
    ) -> Result<R, String> {
        let i = self
            .workspace
            .index_of(handle)
            .ok_or_else(|| "document was closed".to_string())?;
        let prev = self.workspace.active;
        self.workspace.active = Some(i);
        let result = self.try_edit_active(label, f);
        if self.workspace.index_of(handle).is_some() {
            self.workspace.active = prev.filter(|&j| j < self.workspace.docs.len()).or(Some(i));
        }
        result
    }

    /// Run a registered command against an existing workspace (UI / MCP shared mutex).
    pub fn execute_mut(
        workspace: &mut Workspace,
        source: ChangeSource,
        id: &str,
        params: &Params,
    ) -> Result<Value, CommandError> {
        let taken = std::mem::take(workspace);
        let mut session = Self::new(taken).with_source(source);
        let result = session.execute(id, params);
        *workspace = session.workspace;
        result
    }

    /// Labeled edit on a document in a shared workspace (dialog apply / paste adapters).
    pub fn edit_doc_mut<R>(
        workspace: &mut Workspace,
        source: ChangeSource,
        handle: u64,
        label: &str,
        f: impl FnOnce(&mut Project) -> Result<R, String>,
    ) -> Result<R, String> {
        let taken = std::mem::take(workspace);
        let mut session = Self::new(taken).with_source(source);
        let result = session.try_edit_doc(handle, label, |p| {
            f(p).map_err(plancraft_core::ops::PlanError::Invalid)
        });
        *workspace = session.workspace;
        result
    }

    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<u64, String> {
        let handle = self.workspace.open(path).map_err(|e| e.to_string())?;
        self.push_event(Event::ProjectChanged);
        Ok(handle)
    }

    pub fn add_project(&mut self, project: Project, path: Option<PathBuf>) -> u64 {
        let handle = self.workspace.add(project, path);
        self.push_event(Event::ProjectChanged);
        handle
    }

    pub fn require_active(&self) -> Result<(), String> {
        if self.workspace.active_doc().is_some() {
            Ok(())
        } else {
            Err("no open document".into())
        }
    }
}

/// Helper: empty object result.
pub fn ok_empty() -> Value {
    json!({})
}

/// Helper: document handle in the result.
pub fn ok_doc(handle: u64) -> Value {
    json!({ "document": handle })
}
