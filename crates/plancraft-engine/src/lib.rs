//! # PlanCraft engine
//!
//! Craft-suite L5 façade: every project-changing action goes through
//! [`Session::execute`]. Desktop UI, CLI and MCP share this registry.
//!
//! UI-only actions (dialogs, theme, pane visibility) live in `plancraft-ui-egui`
//! as `UI_COMMANDS` and may emit [`Event`]s drained by the frontend each frame.

#![forbid(unsafe_code)]

pub mod command;
pub mod commands;
pub mod event;
pub mod session;

pub use command::{CommandError, CommandSpec, Params};
pub use event::Event;
pub use session::Session;
