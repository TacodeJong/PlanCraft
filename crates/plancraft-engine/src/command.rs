//! Command registry types (FilmCraft / DesignCraft `CommandSpec` shape).

use serde_json::Value;

use crate::session::Session;

/// JSON parameters for a command (agents, CLI, MCP).
pub type Params = Value;

/// Why a command did not run, or failed while running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// No command with this id is registered.
    Unknown(String),
    /// `enabled` returned `Err(reason)`.
    Disabled(String),
    /// `run` returned an error message.
    Failed(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "unknown command '{id}'"),
            Self::Disabled(reason) => write!(f, "{reason}"),
            Self::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// One registered command. Menus, shortcuts, CLI and MCP all dispatch these.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu path segments; empty = not listed in menus.
    pub menu: &'static [&'static str],
    /// Human shortcut hint (`Cmd` = ⌘ on macOS, Ctrl elsewhere); `None` = none.
    pub shortcut: Option<&'static str>,
    /// JSON Schema-ish params docs for agents (`{}` when none).
    pub params: &'static str,
    pub enabled: fn(&Session) -> Result<(), String>,
    pub run: fn(&mut Session, &Params) -> Result<Value, String>,
    /// When true, successful runs are appended to the session journal.
    pub journal: bool,
}

/// All registered specs (engine commands only; UI commands stay in ui-egui).
pub fn registry() -> &'static [CommandSpec] {
    crate::commands::ALL
}

/// Look up a command by id.
pub fn find(id: &str) -> Option<&'static CommandSpec> {
    registry().iter().find(|c| c.id == id)
}

/// Stable list of command ids (CLI `commands`, agent discovery).
pub fn ids() -> impl Iterator<Item = &'static str> {
    registry().iter().map(|c| c.id)
}
