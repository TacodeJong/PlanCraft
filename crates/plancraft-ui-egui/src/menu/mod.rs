//! Shared application menus: one model for the in-window bar, the macOS native bar, and shortcuts.
//!
//! Windows/Linux (and macOS when native install fails) draw [`bar`]. macOS installs [`mac`] into
//! AppKit. Both enqueue the same [`Command`] values that the ribbon and keyboard already use.

mod bar;
mod build;
mod model;
mod state;

#[cfg(target_os = "macos")]
mod mac;

pub use bar::draw_menu_row;
pub use build::{LayoutKind, build_menu_bar};
pub use model::{Chord, MenuBar};
pub use state::{MenuSnapshot, command_enabled, host_is_mac, platform_shortcut_label, shortcut_label, tip_with_shortcut};

use crate::commands::Command;

/// Events drained from the native (macOS) menu before the egui pass.
#[derive(Clone, Debug)]
pub enum NativeEvent {
    /// Mouse (or Return) chose a menu item: run the command like an in-window click.
    Click(Command),
    /// AppKit matched a key equivalent: inject as an egui key so shortcut rules still apply.
    Key(Chord),
}

/// Platform menu handle. On macOS this owns muda/AppKit menus; elsewhere it is a no-op stub.
pub struct NativeMenu {
    #[cfg(target_os = "macos")]
    inner: Option<mac::MacMenu>,
    /// True when a native bar is installed and the in-window conventional row should stay hidden.
    active: bool,
    /// Pending clicks that are layout-only (Settings ⌘,, Quit ⌘Q) with no egui shortcut owner.
    pending_clicks: Vec<Command>,
    /// Request a paste from the system clipboard (text field owns focus).
    pending_paste: bool,
}

impl NativeMenu {
    /// Install on the main thread after egui's context exists. Returns an inactive stub off macOS
    /// or when AppKit refuses the menu.
    pub fn install(ctx: &egui::Context) -> Self {
        #[cfg(target_os = "macos")]
        {
            match mac::MacMenu::try_install(ctx) {
                Some(inner) => Self { inner: Some(inner), active: true, pending_clicks: Vec::new(), pending_paste: false },
                None => Self { inner: None, active: false, pending_clicks: Vec::new(), pending_paste: false },
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = ctx;
            Self { active: false, pending_clicks: Vec::new(), pending_paste: false }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Before the egui pass: pull native events into pending queues / key injection.
    pub fn poll_into_raw_input(&mut self, raw: &mut egui::RawInput, text_editing: bool) {
        #[cfg(target_os = "macos")]
        if let Some(inner) = self.inner.as_mut() {
            for ev in inner.drain() {
                match ev {
                    NativeEvent::Click(cmd) => self.pending_clicks.push(cmd),
                    NativeEvent::Key(chord) => {
                        if text_editing && is_edit_clipboard(&chord) {
                            match chord.key.as_str() {
                                "C" => raw.events.push(egui::Event::Copy),
                                "X" => raw.events.push(egui::Event::Cut),
                                "V" => self.pending_paste = true,
                                _ => {}
                            }
                        } else if let Some(ev) = chord.key_event(true) {
                            raw.events.push(ev);
                        } else if let Some(cmd) = chord_command_fallback(&chord) {
                            // Layout-only accelerators (⌘,, ⌘Q) with no egui shortcut: treat as click.
                            self.pending_clicks.push(cmd);
                        }
                    }
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (raw, text_editing);
        }
    }

    /// Clicks that should run as commands this frame (after gate checks in the app).
    pub fn take_clicks(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.pending_clicks)
    }

    pub fn take_paste_request(&mut self) -> bool {
        std::mem::take(&mut self.pending_paste)
    }

    /// Sync labels/enabled/checked, or rebuild when structure changes.
    pub fn sync(&mut self, bar: &MenuBar) {
        #[cfg(target_os = "macos")]
        if let Some(inner) = self.inner.as_mut() {
            inner.sync(bar);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = bar;
        }
    }
}

fn is_edit_clipboard(c: &Chord) -> bool {
    c.cmd && !c.ctrl && !c.alt && !c.shift && matches!(c.key.as_str(), "C" | "X" | "V")
}

fn chord_command_fallback(c: &Chord) -> Option<Command> {
    if c.portable() == "Cmd+," {
        return Some(Command::Settings);
    }
    if c.portable() == "Cmd+Q" {
        return Some(Command::Quit);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{HIDE, Item, ItemRole, Menu, MenuRole, Node, structure_key};
    use crate::menu::state::{MenuSnapshot, command_checked};
    use crate::state::{VIEW_MENU_GROUPS, ViewKind, view_menu_views};
    use crate::theme::ThemeChoice;

    fn snap() -> MenuSnapshot {
        crate::lang::set_current(crate::lang::Lang::En);
        MenuSnapshot {
            has_doc: true,
            can_undo: true,
            can_redo: false,
            undo_label: Some("Edit".into()),
            task_selected: true,
            selected_task_count: 1,
            resource_selected: false,
            has_clipboard: false,
            view: ViewKind::Gantt,
            timeline_visible: true,
            properties_visible: false,
            history_open: false,
            theme: ThemeChoice::Dark,
            lang: crate::lang::Lang::En,
            show_critical: false,
            show_paths: false,
            show_slack: false,
            show_baseline: false,
            show_progress: true,
            show_links: true,
            show_resource_names: true,
            show_project_summary: false,
            baseline_slot: 0,
            multi_baselines: 0b111,
            network_critical_first: false,
            network_summaries: true,
            network_link_labels: false,
            network_progress_marks: false,
            network_ids_only: false,
            recent: vec![],
            documents: vec![(1, "Demo".into(), true)],
            modal: false,
            start_page: false,
        }
    }

    #[test]
    fn desktop_bar_has_file_edit_view_format_project_help() {
        crate::lang::set_current(crate::lang::Lang::En);
        let bar = build_menu_bar(&snap(), LayoutKind::Desktop);
        let titles: Vec<_> = bar.menus.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["File", "Edit", "View", "Format", "Project", "Window", "Help"]);
    }

    /// Edit ▸ Delete Resource and Project ▸ Delete Resource stay available when a resource is selected.
    #[test]
    fn delete_resource_appears_in_edit_and_project_menus() {
        use crate::menu::state::command_enabled;
        let mut s = snap();
        s.task_selected = false;
        s.resource_selected = true;
        s.view = ViewKind::ResourceSheet;
        assert!(command_enabled(&Command::DeleteSelection, &s));
        assert!(command_enabled(&Command::DeleteResources, &s));
        let bar = build_menu_bar(&s, LayoutKind::Desktop);
        let edit = bar.menus.iter().find(|m| m.title == "Edit").unwrap();
        assert!(
            edit.children.iter().any(|n| matches!(n, Node::Item(it) if it.id == "edit.delete" && it.label.contains("Resource"))),
            "Edit > Delete Resource on a resource view"
        );
        let project = bar.menus.iter().find(|m| m.title == "Project" || m.title == crate::lang::text("tab.project")).unwrap();
        assert!(
            project.children.iter().any(|n| matches!(n, Node::Item(it) if it.id == "project.deleteResource")),
            "Project menu lists Delete Resource"
        );
        assert!(
            project.children.iter().any(|n| matches!(n, Node::Item(it) if it.id == "project.addResource")),
            "Project menu lists Add Resource"
        );
        s.view = ViewKind::Gantt;
        assert!(!command_enabled(&Command::DeleteSelection, &s), "Gantt needs a task selection");
    }

    #[test]
    fn mac_layout_moves_about_settings_quit_into_app_menu() {
        crate::lang::set_current(crate::lang::Lang::En);
        let bar = build_menu_bar(&snap(), LayoutKind::Mac);
        assert_eq!(bar.menus.first().map(|m| m.role), Some(MenuRole::App));
        let app = bar.menus.first().expect("app menu");
        let roles: Vec<_> = app
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Item(it) => it.role,
                _ => None,
            })
            .collect();
        assert!(roles.contains(&ItemRole::About));
        assert!(roles.contains(&ItemRole::Settings));
        assert!(roles.contains(&ItemRole::Quit));
        assert!(roles.contains(&ItemRole::Hide));
        assert!(bar.menu(MenuRole::Window).is_some());
        assert!(bar.menu(MenuRole::Help).is_some());
        // Quit / About / Settings must not remain in File/Help after the move.
        for m in &bar.menus {
            if m.role == MenuRole::App {
                continue;
            }
            for it in bar_items_in(m) {
                assert_ne!(it.role, Some(ItemRole::About));
                assert_ne!(it.role, Some(ItemRole::Settings));
                assert_ne!(it.role, Some(ItemRole::Quit));
            }
        }
    }

    fn bar_items_in(m: &Menu) -> Vec<&Item> {
        fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Item>) {
            for n in nodes {
                match n {
                    Node::Item(it) => out.push(it),
                    Node::Submenu { children, .. } => walk(children, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&m.children, &mut out);
        out
    }

    #[test]
    fn undo_save_export_enablement() {
        let mut s = snap();
        assert!(command_enabled(&Command::Undo, &s));
        assert!(command_enabled(&Command::Save, &s));
        assert!(!command_enabled(&Command::ExportReport, &s));
        s.view = ViewKind::Report;
        assert!(command_enabled(&Command::ExportReport, &s));
        s.can_undo = false;
        assert!(!command_enabled(&Command::Undo, &s));
        s.modal = true;
        assert!(!command_enabled(&Command::Save, &s));
        assert!(command_enabled(&Command::Quit, &s));
    }

    #[test]
    fn open_recent_maps_to_open_path() {
        let mut s = snap();
        let path = std::path::PathBuf::from("/tmp/demo.plancraft");
        s.recent = vec![(path.clone(), true)];
        let bar = build_menu_bar(&s, LayoutKind::Desktop);
        let recent = bar.items().into_iter().find(|it| matches!(&it.command, Command::OpenPath(p) if p == &path)).expect("recent item");
        assert!(recent.enabled);
    }

    #[test]
    fn theme_and_view_checks() {
        let s = snap();
        assert_eq!(command_checked(&Command::SetTheme(ThemeChoice::Dark), &s), Some(true));
        assert_eq!(command_checked(&Command::SetTheme(ThemeChoice::Light), &s), Some(false));
        assert_eq!(command_checked(&Command::SetView(ViewKind::Gantt), &s), Some(true));
        assert_eq!(command_checked(&Command::ToggleTimelinePane, &s), Some(true));
        assert_eq!(command_checked(&Command::TogglePropertiesPane, &s), Some(false));
    }

    /// View ▸ Views follows the shared group metadata: same order, separators between groups,
    /// stable `view.set.<English>` ids, and SetView enable/checked metadata unchanged.
    #[test]
    fn views_submenu_matches_shared_group_metadata() {
        crate::lang::set_current(crate::lang::Lang::En);
        let s = snap();
        let bar = build_menu_bar(&s, LayoutKind::Desktop);
        let view = bar.menus.iter().find(|m| m.title == "View").expect("View menu");
        let children = view
            .children
            .iter()
            .find_map(|n| match n {
                Node::Submenu { label, children, .. } if label == "Views" => Some(children.as_slice()),
                _ => None,
            })
            .expect("Views submenu");

        let mut want: Vec<NodeKind> = Vec::new();
        for (i, group) in VIEW_MENU_GROUPS.iter().enumerate() {
            if i > 0 {
                want.push(NodeKind::Sep);
            }
            for &v in group.views {
                want.push(NodeKind::View(v));
            }
        }
        let got: Vec<NodeKind> = children
            .iter()
            .map(|n| match n {
                Node::Separator => NodeKind::Sep,
                Node::Item(it) => match it.command {
                    Command::SetView(v) => NodeKind::View(v),
                    _ => panic!("unexpected command in Views: {:?}", it.command),
                },
                other => panic!("unexpected node in Views: {other:?}"),
            })
            .collect();
        assert_eq!(got, want, "flattened group order with separators between groups");

        let set_views: Vec<_> = children
            .iter()
            .filter_map(|n| match n {
                Node::Item(it) => Some(it),
                _ => None,
            })
            .collect();
        assert_eq!(set_views.len(), ViewKind::ALL.len());
        assert_eq!(set_views[0].command, Command::SetView(ViewKind::Overview), "native Views menu leads with Overview");
        assert_eq!(set_views[1].command, Command::SetView(ViewKind::Gantt));
        for (it, v) in set_views.iter().zip(view_menu_views()) {
            assert_eq!(it.id, format!("view.set.{}", v.english()), "stable English id for {v:?}");
            assert_eq!(it.command, Command::SetView(v));
            assert!(it.enabled, "SetView stays enabled with a document");
            assert_eq!(it.kind.checked(), Some(v == ViewKind::Gantt), "current view checked");
        }
    }

    /// Overview is first in the shared Change-view order that native and custom menus both read.
    #[test]
    fn overview_leads_shared_view_menu_order() {
        let ordered: Vec<_> = view_menu_views().collect();
        assert_eq!(ordered.first(), Some(&ViewKind::Overview));
        assert_eq!(VIEW_MENU_GROUPS[0].views[0], ViewKind::Overview);
        assert_eq!(VIEW_MENU_GROUPS[0].views[1], ViewKind::Gantt);
        assert_eq!(ordered.len(), 16);
        assert_eq!(ViewKind::ALL[0], ViewKind::Gantt, "ALL enum scan order stays legacy");
    }

    #[derive(Debug, PartialEq, Eq)]
    enum NodeKind {
        Sep,
        View(ViewKind),
    }

    #[test]
    fn hide_moves_when_cmd_h_is_bound() {
        // PlanCraft binds Cmd+H to Find(true) in shortcuts; mac layout must not leave Hide on ⌘H.
        let bar = build_menu_bar(&snap(), LayoutKind::Mac);
        let hide = bar.items().into_iter().find(|it| it.id == HIDE).expect("hide");
        assert_ne!(hide.shortcut.as_deref(), Some("Cmd+H"));
        assert_eq!(hide.shortcut.as_deref(), Some("Ctrl+Cmd+H"));
    }

    #[test]
    fn chord_parse_and_key_event() {
        let c = Chord::parse("Cmd+Shift+Z").unwrap();
        assert!(c.cmd && c.shift && c.key == "Z");
        assert!(c.key_event(true).is_some());
        assert_eq!(Chord::parse("Cmd+,").unwrap().portable(), "Cmd+,");
    }

    #[test]
    fn structure_key_changes_with_recent_list() {
        let mut s = snap();
        let a = structure_key(&build_menu_bar(&s, LayoutKind::Desktop));
        s.recent.push((std::path::PathBuf::from("/a.plancraft"), true));
        let b = structure_key(&build_menu_bar(&s, LayoutKind::Desktop));
        assert_ne!(a, b);
    }

    #[test]
    fn link_needs_two_selected_tasks_unlink_allows_one() {
        use crate::menu::state::command_enabled;
        let mut s = snap();
        s.task_selected = false;
        s.selected_task_count = 0;
        assert!(!command_enabled(&Command::LinkSelected, &s));
        assert!(!command_enabled(&Command::UnlinkSelected, &s));

        s.task_selected = true;
        s.selected_task_count = 1;
        assert!(!command_enabled(&Command::LinkSelected, &s), "one task cannot form a link");
        assert!(command_enabled(&Command::UnlinkSelected, &s), "unlink may clear links on one task");

        s.selected_task_count = 2;
        assert!(command_enabled(&Command::LinkSelected, &s));
        assert!(command_enabled(&Command::UnlinkSelected, &s));
    }

    #[test]
    fn platform_shortcut_labels_for_mac_and_windows() {
        use crate::menu::state::{platform_shortcut_label, tip_with_shortcut};
        assert_eq!(platform_shortcut_label("Cmd+S", true), "Cmd+S");
        assert_eq!(platform_shortcut_label("Cmd+S", false), "Ctrl+S");
        assert_eq!(platform_shortcut_label("Cmd+Shift+F2", true), "Shift+Cmd+F2");
        assert_eq!(platform_shortcut_label("Cmd+Shift+F2", false), "Ctrl+Shift+F2");
        assert_eq!(platform_shortcut_label("Alt+Shift+ArrowRight", true), "Option+Shift+Right");
        assert_eq!(platform_shortcut_label("Alt+Shift+ArrowRight", false), "Alt+Shift+Right");
        let tip = tip_with_shortcut(&Command::Save, "Save", false);
        assert_eq!(tip, "Save (Ctrl+S)");
        let tip_mac = tip_with_shortcut(&Command::Save, "Save", true);
        assert_eq!(tip_mac, "Save (Cmd+S)");
    }

    #[test]
    fn modal_and_start_page_gate_document_actions() {
        use crate::menu::state::command_enabled;
        let mut s = snap();
        s.selected_task_count = 2;
        s.modal = true;
        assert!(!command_enabled(&Command::LinkSelected, &s));
        assert!(command_enabled(&Command::Quit, &s));
        s.modal = false;
        s.start_page = true;
        assert!(!command_enabled(&Command::LinkSelected, &s));
        assert!(!command_enabled(&Command::Save, &s));
    }
}
