//! Events drained by frontends after [`crate::Session::execute`].

use serde_json::Value;

/// Something the UI (or another frontend) should react to.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The active plan changed (edits, open, close, undo).
    ProjectChanged,
    /// Brief status message for a toast / status bar.
    Toast(String),
    /// Ask the desktop shell to open a dialog or surface (`id` matches a UI command).
    OpenUi(String),
    /// Arbitrary payload for frontends that understand it.
    Custom { kind: String, data: Value },
}
