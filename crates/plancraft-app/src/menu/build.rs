//! Build the shared menu tree for desktop (in-window) or macOS (AppKit) layout.

use crate::commands::Command;
use crate::lang;
use crate::state::ViewKind;
use crate::theme::ThemeChoice;

use super::model::{APP_NAME, HIDE, Item, ItemKind, ItemRole, MINIMIZE, Menu, MenuBar, MenuRole, Node, Standard, take, tidy};
use super::state::{MenuSnapshot, command_checked, command_enabled, shortcut_label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    Desktop,
    Mac,
}

pub fn build_menu_bar(s: &MenuSnapshot, kind: LayoutKind) -> MenuBar {
    let bar = logical_menus(s);
    match kind {
        LayoutKind::Desktop => bar,
        LayoutKind::Mac => mac_layout(bar, s),
    }
}

fn logical_menus(s: &MenuSnapshot) -> MenuBar {
    MenuBar {
        menus: vec![
            Menu { title: lang::text("menu.file").into(), role: MenuRole::Normal, children: file_menu(s) },
            Menu { title: "Edit".into(), role: MenuRole::Normal, children: edit_menu(s) },
            Menu { title: lang::text("tab.view").into(), role: MenuRole::Normal, children: view_menu(s) },
            Menu { title: lang::text("tab.format").into(), role: MenuRole::Normal, children: format_menu(s) },
            Menu { title: lang::text("tab.project").into(), role: MenuRole::Normal, children: project_menu(s) },
            Menu { title: "Window".into(), role: MenuRole::Window, children: window_docs(s) },
            Menu { title: "Help".into(), role: MenuRole::Help, children: help_menu(s) },
        ],
    }
}

fn item(s: &MenuSnapshot, id: &str, cmd: Command, label: &str) -> Node {
    let enabled = command_enabled(&cmd, s);
    let kind = match command_checked(&cmd, s) {
        Some(v)
            if matches!(
                cmd,
                Command::SetView(_)
                    | Command::SetTheme(_)
                    | Command::SetLang(_)
                    | Command::SetBaselineSlot(_)
                    | Command::ActivateDocument(_)
            ) =>
        {
            ItemKind::Radio(v)
        }
        Some(v) => ItemKind::Check(v),
        None => ItemKind::Plain,
    };
    let mut it = Item::plain(id, cmd.clone(), label);
    it.enabled = enabled;
    it.kind = kind;
    it.shortcut = shortcut_label(&cmd);
    Node::Item(it)
}

fn file_menu(s: &MenuSnapshot) -> Vec<Node> {
    let mut nodes = vec![
        item(s, "file.new", Command::NewProject, lang::text("menu.new")),
        item(s, "file.sample", Command::NewDemo, lang::text("menu.sample")),
        item(s, "file.open", Command::Open, lang::text("menu.open")),
        item(s, "file.openServer", Command::OpenFromServer, lang::text("menu.open_server")),
        Node::Submenu { label: lang::text("menu.recent").into(), children: recent_menu(s), role: None },
        Node::Separator,
        item(s, "file.save", Command::Save, lang::text("menu.save")),
        item(s, "file.saveAs", Command::SaveAs, lang::text("menu.save_as")),
        item(s, "file.print", Command::Print, lang::text("menu.print")),
        item(s, "file.exportPdf", Command::ExportPdf, lang::text("menu.export_pdf")),
        item(s, "file.import", Command::ImportSpreadsheet, lang::text("menu.import")),
        Node::Submenu {
            label: lang::text("menu.export_csv").into(),
            children: plancraft_core::tabular::CsvTable::ALL
                .iter()
                .map(|t| item(s, &format!("file.exportCsv.{}", t.label()), Command::ExportCsv(*t), &format!("{}…", t.label())))
                .collect(),
            role: None,
        },
        item(s, "file.exportXml", Command::ExportMspdi, lang::text("menu.export_xml")),
        item(s, "file.exportReport", Command::ExportReport, lang::text("menu.export_report")),
        item(s, "file.exportGantt", Command::ExportGantt, lang::text("menu.export_gantt")),
        Node::Separator,
        item(s, "file.projectInfo", Command::ProjectInfo, lang::text("menu.project_info")),
        item(s, "file.start", Command::StartPage, lang::text("menu.start")),
        item(s, "file.close", Command::Close, lang::text("menu.close")),
        Node::Separator,
        item(s, "file.quick", Command::QuickEntry, lang::text("menu.quick")),
        item(s, "file.distill", Command::Distill, lang::text("menu.distill")),
        item(s, "file.ai", Command::AiSettings, lang::text("menu.ai")),
        item(s, "file.settings", Command::Settings, "Settings…"),
        item(s, "file.mcp", Command::McpPanel, lang::text("menu.mcp")),
        Node::Separator,
        {
            let mut quit = Item::plain("file.quit", Command::Quit, lang::text("menu.exit"));
            quit.enabled = command_enabled(&Command::Quit, s);
            quit.shortcut = shortcut_label(&Command::Quit);
            quit.role = Some(ItemRole::Quit);
            Node::Item(quit)
        },
    ];
    // Settings role for mac app-menu lift.
    if let Some(Node::Item(it)) = nodes.iter_mut().find(|n| matches!(n, Node::Item(i) if i.id == "file.settings")) {
        it.role = Some(ItemRole::Settings);
    }
    tidy(nodes)
}

fn recent_menu(s: &MenuSnapshot) -> Vec<Node> {
    let mut nodes = Vec::new();
    if s.recent.is_empty() {
        nodes.push(Node::Item(Item {
            id: "file.recent.empty".into(),
            command: Command::ClearRecent,
            label: lang::text("menu.no_recent").into(),
            shortcut: None,
            enabled: false,
            kind: ItemKind::Plain,
            role: None,
        }));
    } else {
        for (i, (path, exists)) in s.recent.iter().enumerate() {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
            nodes.push(Node::Item(Item {
                id: format!("file.recent.{i}"),
                command: Command::OpenPath(path.clone()),
                label: name,
                shortcut: None,
                enabled: *exists && !s.modal,
                kind: ItemKind::Plain,
                role: None,
            }));
        }
        nodes.push(Node::Separator);
        nodes.push(item(s, "file.clearRecent", Command::ClearRecent, "Clear Menu"));
    }
    tidy(nodes)
}

fn edit_menu(s: &MenuSnapshot) -> Vec<Node> {
    let undo_label = match &s.undo_label {
        Some(l) => format!("{} {l}", lang::text("tb.history").trim_end_matches(['…', '.'])),
        None => "Undo".into(),
    };
    // Prefer a clear Undo/Redo pair.
    let undo = {
        let mut it = Item::plain("edit.undo", Command::Undo, if s.undo_label.is_some() { undo_label } else { "Undo".into() });
        it.enabled = command_enabled(&Command::Undo, s);
        it.shortcut = shortcut_label(&Command::Undo);
        Node::Item(it)
    };
    tidy(vec![
        undo,
        item(s, "edit.redo", Command::Redo, "Redo"),
        Node::Separator,
        item(s, "edit.cut", Command::Cut, "Cut"),
        item(s, "edit.copy", Command::Copy, "Copy"),
        item(s, "edit.paste", Command::Paste, "Paste"),
        item(s, "edit.fillDown", Command::FillDown, "Fill Down"),
        Node::Separator,
        item(s, "edit.find", Command::Find(false), "Find…"),
        item(s, "edit.replace", Command::Find(true), "Replace…"),
        item(s, "edit.goto", Command::GoTo, "Go To…"),
        Node::Separator,
        item(s, "edit.history", Command::History, lang::text("tb.history")),
    ])
}

fn view_menu(s: &MenuSnapshot) -> Vec<Node> {
    let mut views = Vec::new();
    for v in ViewKind::ALL {
        views.push(item(s, &format!("view.set.{}", v.english()), Command::SetView(v), v.label()));
    }
    let mut theme = Vec::new();
    for t in ThemeChoice::ALL {
        theme.push(item(s, &format!("view.theme.{}", crate::lang::theme_name(t)), Command::SetTheme(t), crate::lang::theme_name(t)));
    }
    let mut langs = Vec::new();
    for l in lang::Lang::ALL {
        let code = match l {
            lang::Lang::Nl => "nl",
            lang::Lang::En => "en",
        };
        langs.push(item(s, &format!("view.lang.{code}"), Command::SetLang(l), l.label()));
    }
    // Tag Appearance / Language for mac app-menu lift.
    let appearance = Node::Submenu { label: "Appearance".into(), children: theme, role: Some(ItemRole::Appearance) };
    let language = Node::Submenu { label: lang::text("tb.language").into(), children: langs, role: Some(ItemRole::Language) };
    tidy(vec![
        Node::Submenu { label: "Views".into(), children: views, role: None },
        Node::Separator,
        item(s, "view.zoomIn", Command::ZoomIn, "Zoom In"),
        item(s, "view.zoomOut", Command::ZoomOut, "Zoom Out"),
        item(s, "view.zoomEntire", Command::ZoomEntire, lang::text("tb.entire")),
        item(s, "view.zoomSelected", Command::ZoomSelected, lang::text("tb.selected")),
        Node::Separator,
        item(s, "view.timeline", Command::ToggleTimelinePane, lang::text("tb.timeline")),
        item(s, "view.properties", Command::TogglePropertiesPane, lang::text("tb.properties")),
        item(s, "view.history", Command::History, lang::text("tb.history")),
        Node::Separator,
        appearance,
        language,
    ])
}

fn format_menu(s: &MenuSnapshot) -> Vec<Node> {
    if s.view == ViewKind::NetworkDiagram {
        return tidy(vec![
            item(s, "fmt.net.criticalFirst", Command::ToggleNetworkCriticalFirst, "Critical First"),
            item(s, "fmt.net.summaries", Command::ToggleNetworkSummaries, "Summary Tasks"),
            Node::Separator,
            item(s, "fmt.net.linkLabels", Command::ToggleNetworkLinkLabels, "Link Labels"),
            item(s, "fmt.net.progress", Command::ToggleNetworkProgressMarks, "Progress Marks"),
            item(s, "fmt.net.idsOnly", Command::ToggleNetworkIdsOnly, "IDs Only"),
        ]);
    }
    let mut nodes = vec![
        item(s, "fmt.critical", Command::ToggleShowCritical, lang::text("fmt.critical")),
        item(s, "fmt.paths", Command::ToggleHighlightPaths, lang::text("fmt.paths")),
        item(s, "fmt.slack", Command::ToggleShowSlack, "Slack"),
        item(s, "fmt.baseline", Command::ToggleShowBaseline, "Baseline"),
    ];
    let mut slots = Vec::new();
    for n in 0..=plancraft_core::baseline::MAX_BASELINE {
        let name = plancraft_core::baseline::PlanSlot::Baseline(n).name();
        slots.push(item(s, &format!("fmt.baselineSlot.{n}"), Command::SetBaselineSlot(n), &name));
    }
    nodes.push(Node::Submenu { label: "Baseline Slot".into(), children: slots, role: None });
    nodes.extend([
        Node::Separator,
        item(s, "fmt.progress", Command::ToggleShowProgress, "Progress"),
        item(s, "fmt.links", Command::ToggleShowLinks, "Links"),
        item(s, "fmt.resources", Command::ToggleShowResourceNames, "Resource Names"),
        item(s, "fmt.summary", Command::ToggleShowProjectSummary, "Project Summary"),
    ]);
    tidy(nodes)
}

fn project_menu(s: &MenuSnapshot) -> Vec<Node> {
    tidy(vec![
        item(s, "project.info", Command::ProjectInfo, lang::text("tb.project_info")),
        item(s, "project.workingTime", Command::ChangeWorkingTime, lang::text("tb.working_time")),
        Node::Separator,
        item(s, "project.links", Command::ExternalLinks, lang::text("tb.links")),
        item(s, "project.refresh", Command::RefreshExternal, "Refresh Links"),
        Node::Separator,
        item(s, "project.custom", Command::CustomFields, lang::text("tb.custom")),
        item(s, "project.update", Command::UpdateProject, lang::text("tb.update")),
        Node::Separator,
        item(s, "project.baseline", Command::SetBaseline, lang::text("tb.baseline")),
        item(s, "project.clearBaseline", Command::ClearBaseline, "Clear Baseline…"),
        Node::Separator,
        item(s, "project.stats", Command::Statistics, lang::text("tb.statistics")),
        item(s, "project.mcp", Command::McpPanel, lang::text("tb.agents")),
    ])
}

fn help_menu(s: &MenuSnapshot) -> Vec<Node> {
    let mut about = Item::plain("help.about", Command::About, lang::text("menu.about"));
    about.role = Some(ItemRole::About);
    about.enabled = command_enabled(&Command::About, s);
    tidy(vec![
        Node::Item(about),
        item(s, "help.mcp", Command::McpPanel, lang::text("menu.mcp")),
        item(s, "help.ai", Command::AiSettings, lang::text("menu.ai")),
    ])
}

fn window_docs(s: &MenuSnapshot) -> Vec<Node> {
    s.documents
        .iter()
        .map(|(h, title, active)| {
            let mut it = Item::plain(format!("window.doc.{h}"), Command::ActivateDocument(*h), title.clone());
            it.kind = ItemKind::Radio(*active);
            it.role = Some(ItemRole::Document);
            it.enabled = !s.modal;
            Node::Item(it)
        })
        .collect()
}

fn mac_layout(mut bar: MenuBar, s: &MenuSnapshot) -> MenuBar {
    let bound: std::collections::HashMap<String, String> = bar
        .items()
        .iter()
        .filter_map(|it| Some((super::model::Chord::parse(it.shortcut.as_deref()?)?.portable(), it.id.clone())))
        .collect();
    let taken = |sc: &str, own: Option<&str>| bound.get(sc).filter(|cmd| Some(cmd.as_str()) != own).cloned();

    let about = take(&mut bar, |n| matches!(n, Node::Item(it) if it.role == Some(ItemRole::About)));
    let settings = take(&mut bar, |n| matches!(n, Node::Item(it) if it.role == Some(ItemRole::Settings)));
    let quit = take(&mut bar, |n| matches!(n, Node::Item(it) if it.role == Some(ItemRole::Quit)));
    let appearance = take(&mut bar, |n| matches!(n, Node::Submenu { role, .. } if *role == Some(ItemRole::Appearance)));
    let language = take(&mut bar, |n| matches!(n, Node::Submenu { role, .. } if *role == Some(ItemRole::Language)));

    let mut app_children = Vec::new();
    if let Some(Node::Item(mut it)) = about {
        it.label = format!("About {APP_NAME}");
        app_children.extend([Node::Item(it), Node::Separator]);
    }
    if let Some(Node::Item(mut it)) = settings {
        it.label = "Settings…".into();
        if taken("Cmd+,", Some(&it.id)).is_none() && it.shortcut.is_none() {
            it.shortcut = Some("Cmd+,".into());
        }
        app_children.push(Node::Item(it));
    }
    if let Some(n) = language {
        app_children.push(n);
    }
    if let Some(n) = appearance {
        app_children.push(n);
    }
    if !app_children.is_empty() && !matches!(app_children.last(), Some(Node::Separator)) {
        app_children.push(Node::Separator);
    }
    app_children.extend([Node::Standard(Standard::Services), Node::Separator]);

    let mut hide = Item::plain(HIDE, Command::HideApp, format!("Hide {APP_NAME}"));
    hide.role = Some(ItemRole::Hide);
    // Replace keeps Cmd+H in egui shortcuts, so Hide moves to ⌃⌘H (Photoshop-style).
    let _ = taken("Cmd+H", None);
    if taken("Ctrl+Cmd+H", None).is_none() {
        hide.shortcut = Some("Ctrl+Cmd+H".into());
    }
    app_children.push(Node::Item(hide));
    app_children.extend([Node::Standard(Standard::HideOthers), Node::Standard(Standard::ShowAll), Node::Separator]);

    if let Some(Node::Item(mut quit)) = quit {
        quit.label = format!("Quit {APP_NAME}");
        if taken("Cmd+Q", Some(&quit.id)).is_none() {
            quit.shortcut = Some("Cmd+Q".into());
        } else {
            quit.shortcut = None;
        }
        app_children.push(Node::Item(quit));
    }

    bar.menus.insert(0, Menu { title: APP_NAME.into(), role: MenuRole::App, children: tidy(app_children) });

    // Window chrome.
    let minimize = if taken("Cmd+M", None).is_some() {
        let mut it = Item::plain(MINIMIZE, Command::MinimizeWindow, "Minimize");
        it.role = Some(ItemRole::Minimize);
        if taken("Ctrl+Cmd+M", None).is_none() {
            it.shortcut = Some("Ctrl+Cmd+M".into());
        }
        Node::Item(it)
    } else {
        Node::Standard(Standard::Minimize)
    };
    if let Some(window) = bar.menus.iter_mut().find(|m| m.role == MenuRole::Window) {
        let docs = std::mem::take(&mut window.children);
        let mut children = vec![minimize, Node::Standard(Standard::Zoom), Node::Separator];
        children.extend([Node::Standard(Standard::BringAllToFront), Node::Separator]);
        children.extend(docs);
        window.children = tidy(children);
    }

    // Close uses Cmd+W via Command::Close; do not add AppKit's Close Window (would clash).
    let _ = taken("Cmd+W", None);

    for m in &mut bar.menus {
        m.children = tidy(std::mem::take(&mut m.children));
    }
    bar.menus.retain(|m| !m.children.is_empty() || m.role == MenuRole::Help);

    let _ = s;
    bar
}
