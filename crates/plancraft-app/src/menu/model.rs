//! Platform-neutral menu tree shared by the in-window bar and the macOS muda backend.

use std::hash::{DefaultHasher, Hash, Hasher};

use egui::{Key, Modifiers};

use crate::commands::Command;

pub const APP_NAME: &str = "PlanCraft";
pub const HIDE: &str = "app.hide";
pub const MINIMIZE: &str = "window.minimize";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBar {
    pub menus: Vec<Menu>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub title: String,
    pub role: MenuRole,
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MenuRole {
    #[default]
    Normal,
    App,
    Window,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Item(Item),
    Separator,
    Submenu { label: String, children: Vec<Node>, role: Option<ItemRole> },
    Standard(Standard),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable muda id (unique within a bar via `#2` suffixes in the backend).
    pub id: String,
    pub command: Command,
    pub label: String,
    /// Portable form (`Cmd+Shift+Z`); `Cmd` is ⌘ / Ctrl-as-Command on Windows.
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub kind: ItemKind,
    pub role: Option<ItemRole>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Plain,
    Check(bool),
    Radio(bool),
}

impl ItemKind {
    pub fn checked(self) -> Option<bool> {
        match self {
            ItemKind::Plain => None,
            ItemKind::Check(v) | ItemKind::Radio(v) => Some(v),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemRole {
    About,
    Settings,
    Quit,
    Hide,
    Minimize,
    Appearance,
    Language,
    Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Standard {
    Services,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    BringAllToFront,
    /// Available for layout when Cmd+W is free; PlanCraft binds Close itself.
    #[allow(dead_code)]
    CloseWindow,
}

impl Item {
    pub fn plain(id: impl Into<String>, command: Command, label: impl Into<String>) -> Self {
        Self { id: id.into(), command, label: label.into(), shortcut: None, enabled: true, kind: ItemKind::Plain, role: None }
    }
}

impl MenuBar {
    pub fn items(&self) -> Vec<&Item> {
        fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Item>) {
            for n in nodes {
                match n {
                    Node::Item(it) => out.push(it),
                    Node::Submenu { children, .. } => walk(children, out),
                    Node::Separator | Node::Standard(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        for m in &self.menus {
            walk(&m.children, &mut out);
        }
        out
    }

    pub fn menu(&self, role: MenuRole) -> Option<&Menu> {
        self.menus.iter().find(|m| m.role == role)
    }
}

/// Hash of structure that requires a full rebuild (not label/enabled/checked alone).
pub fn structure_key(bar: &MenuBar) -> u64 {
    let mut h = DefaultHasher::new();
    for m in &bar.menus {
        (&m.title, m.role).hash(&mut h);
        if m.role == MenuRole::Window {
            let mut rest: Vec<Node> = m.children.iter().filter(|n| !is_document(n)).cloned().collect();
            while matches!(rest.last(), Some(Node::Separator)) {
                rest.pop();
            }
            hash_nodes(&rest, &mut h);
        } else {
            hash_nodes(&m.children, &mut h);
        }
    }
    h.finish()
}

pub fn window_key(bar: &MenuBar) -> u64 {
    let mut h = DefaultHasher::new();
    if let Some(m) = bar.menu(MenuRole::Window) {
        hash_nodes(&m.children, &mut h);
    }
    h.finish()
}

fn is_document(n: &Node) -> bool {
    matches!(n, Node::Item(it) if it.role == Some(ItemRole::Document))
}

fn hash_nodes(nodes: &[Node], h: &mut DefaultHasher) {
    for n in nodes {
        match n {
            Node::Item(it) => ('i', &it.id, it.kind.checked().is_some(), &it.shortcut, it.role).hash(h),
            Node::Separator => '-'.hash(h),
            Node::Submenu { label, children, role } => {
                ('[', label, role).hash(h);
                hash_nodes(children, h);
                ']'.hash(h);
            }
            Node::Standard(s) => ('s', s).hash(h),
        }
    }
}

/// Portable shortcut chord (`Cmd+Shift+Z`). `Cmd` is the Command key on macOS / Ctrl on Windows
/// in egui's `Modifiers::COMMAND`; `Ctrl` is always the real Control key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Chord {
    pub fn parse(s: &str) -> Option<Self> {
        let mut c = Chord { cmd: false, ctrl: false, alt: false, shift: false, key: String::new() };
        for part in s.split('+') {
            match part {
                "Cmd" => c.cmd = true,
                "Ctrl" => c.ctrl = true,
                "Alt" => c.alt = true,
                "Shift" => c.shift = true,
                "" => c.key = "+".into(),
                k if c.key.is_empty() => c.key = k.into(),
                _ => return None,
            }
        }
        (!c.key.is_empty()).then_some(c)
    }

    pub fn portable(&self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.cmd, "Cmd"), (self.alt, "Alt"), (self.shift, "Shift")] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }

    pub fn is_function_key(&self) -> bool {
        self.key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n))
    }

    pub fn native_ok(&self) -> bool {
        self.cmd || self.ctrl || self.is_function_key()
    }

    pub fn key_event(&self, pressed: bool) -> Option<egui::Event> {
        let key = match self.key.as_str() {
            "+" => Key::Plus,
            "'" => Key::Quote,
            ";" => Key::Semicolon,
            "=" => Key::Equals,
            "-" => Key::Minus,
            "[" => Key::OpenBracket,
            "]" => Key::CloseBracket,
            "\\" => Key::Backslash,
            "/" => Key::Slash,
            "," => Key::Comma,
            "." => Key::Period,
            k => Key::from_name(k)?,
        };
        let modifiers = Modifiers { alt: self.alt, ctrl: self.ctrl, shift: self.shift, mac_cmd: self.cmd, command: self.cmd };
        Some(egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers })
    }
}

/// Drop leading/trailing separators and collapse runs, at every depth.
pub fn tidy(nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for n in nodes {
        let n = match n {
            Node::Submenu { label, children, role } => Node::Submenu { label, children: tidy(children), role },
            other => other,
        };
        if matches!(n, Node::Separator) && matches!(out.last(), None | Some(Node::Separator)) {
            continue;
        }
        out.push(n);
    }
    while matches!(out.last(), Some(Node::Separator)) {
        out.pop();
    }
    out
}

/// Remove and return the first node matching `pred`, at any depth.
pub fn take(bar: &mut MenuBar, pred: impl Fn(&Node) -> bool + Copy) -> Option<Node> {
    fn walk(nodes: &mut Vec<Node>, pred: impl Fn(&Node) -> bool + Copy) -> Option<Node> {
        if let Some(i) = nodes.iter().position(pred) {
            return Some(nodes.remove(i));
        }
        nodes.iter_mut().find_map(|n| match n {
            Node::Submenu { children, .. } => walk(children, pred),
            _ => None,
        })
    }
    bar.menus.iter_mut().find_map(|m| walk(&mut m.children, pred))
}
