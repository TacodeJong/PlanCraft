//! Side inspectors (Craft / PhotoCraft panel host pattern).
//!
//! Visibility is owned here; each open surface draws PhotoCraft-style **panel** tab chrome
//! (`widgets::panel_tabs`, mirroring craft-ui `TabKind::Panel` in
//! `storytold/craft-libs` `crates/ui-widgets/src/tabs` + dock `group.rs`). Properties uses that
//! strip for General/Planning/…; Timeline and History share this host for show/hide.

/// Fixed side surfaces PlanCraft can show beside the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneId {
    Properties,
    Timeline,
    History,
}

/// Visibility of the inspector-style panes. Persistence stays in app settings / `UiState`.
#[derive(Debug, Clone, Default)]
pub struct PaneHost {
    pub properties: bool,
    pub timeline: bool,
    pub history: bool,
}

impl PaneHost {
    pub fn visible(&self, id: PaneId) -> bool {
        match id {
            PaneId::Properties => self.properties,
            PaneId::Timeline => self.timeline,
            PaneId::History => self.history,
        }
    }

    pub fn set(&mut self, id: PaneId, on: bool) {
        match id {
            PaneId::Properties => self.properties = on,
            PaneId::Timeline => self.timeline = on,
            PaneId::History => self.history = on,
        }
    }

    pub fn toggle(&mut self, id: PaneId) {
        self.set(id, !self.visible(id));
    }

    /// Keep host flags aligned with `UiState` / dialog visibility each frame.
    pub fn sync_from(&mut self, properties: bool, timeline: bool, history: bool) {
        if self.visible(PaneId::Properties) != properties {
            self.set(PaneId::Properties, properties);
        }
        if self.visible(PaneId::Timeline) != timeline {
            self.set(PaneId::Timeline, timeline);
        }
        if self.visible(PaneId::History) != history {
            self.set(PaneId::History, history);
        }
    }
}
