//! macOS AppKit menus via muda (no GTK: default features off).

#![allow(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, channel};

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSEventType};

use crate::commands::Command;

use super::NativeEvent;
use super::model::{Chord, HIDE, MINIMIZE, MenuBar, MenuRole, Node, Standard, structure_key, window_key};

enum Handle {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

struct Entry {
    handle: Handle,
    command: Command,
    chord: Option<Chord>,
    label: String,
    enabled: bool,
}

struct Chosen {
    id: String,
    key: bool,
    mods: [bool; 4],
}

pub struct MacMenu {
    menu: Option<Menu>,
    entries: HashMap<String, Entry>,
    rx: Receiver<Chosen>,
    structure: u64,
    window: Option<(Submenu, Vec<String>, u64)>,
}

impl MacMenu {
    pub fn try_install(ctx: &egui::Context) -> Option<Self> {
        // Never crash for a menu: if AppKit refuses, the in-window row is the fallback.
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (tx, rx) = channel();
            let repaint = ctx.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let event = MainThreadMarker::new().and_then(|mtm| NSApplication::sharedApplication(mtm).currentEvent());
                let key = event.as_ref().is_some_and(|ev| ev.r#type() == NSEventType::KeyDown);
                let flags = event.as_ref().map(|ev| ev.modifierFlags()).unwrap_or(NSEventModifierFlags::empty());
                let mods = [
                    NSEventModifierFlags::Command,
                    NSEventModifierFlags::Control,
                    NSEventModifierFlags::Option,
                    NSEventModifierFlags::Shift,
                ]
                .map(|m| flags.contains(m));
                let _ = tx.send(Chosen { id: e.id.0.clone(), key, mods });
                repaint.request_repaint();
            }));
            MacMenu { menu: None, entries: HashMap::new(), rx, structure: 0, window: None }
        }));
        match built {
            Ok(menu) => Some(menu),
            Err(_) => {
                eprintln!("plancraft: could not install the macOS menu bar; using the in-window menus");
                None
            }
        }
    }

    pub fn sync(&mut self, bar: &MenuBar) {
        if self.menu.is_none() || structure_key(bar) != self.structure {
            self.build(bar);
            return;
        }
        if self.window.as_ref().map(|w| w.2) != Some(window_key(bar)) {
            self.rebuild_window(bar);
        }
        let by_id: HashMap<&str, &super::model::Item> = bar.items().into_iter().map(|it| (it.id.as_str(), it)).collect();
        for (muda_id, e) in &mut self.entries {
            let base = muda_id.split('#').next().unwrap_or(muda_id.as_str());
            let Some(it) = by_id.get(base) else { continue };
            if e.label != it.label {
                let text = escape(&it.label);
                match &e.handle {
                    Handle::Plain(h) => h.set_text(text),
                    Handle::Check(h) => h.set_text(text),
                }
                e.label = it.label.clone();
            }
            if e.enabled != it.enabled {
                match &e.handle {
                    Handle::Plain(h) => h.set_enabled(it.enabled),
                    Handle::Check(h) => h.set_enabled(it.enabled),
                }
                e.enabled = it.enabled;
            }
            if let (Handle::Check(h), Some(c)) = (&e.handle, it.kind.checked())
                && h.is_checked() != c
            {
                h.set_checked(c);
            }
        }
    }

    pub fn drain(&mut self) -> Vec<NativeEvent> {
        let mut out = Vec::new();
        while let Ok(chosen) = self.rx.try_recv() {
            let Some(e) = self.entries.get(&chosen.id) else { continue };
            if e.command_is_hide() || chosen.id == HIDE || chosen.id.starts_with(&format!("{HIDE}#")) {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).hide(None);
                }
                continue;
            }
            if chosen.id == MINIMIZE || chosen.id.starts_with(&format!("{MINIMIZE}#")) {
                out.push(NativeEvent::Click(Command::MinimizeWindow));
                continue;
            }
            let [cmd, ctrl, alt, shift] = chosen.mods;
            let key = e.chord.as_ref().filter(|c| chosen.key && (cmd || ctrl || !(c.cmd || c.ctrl))).map(|c| Chord {
                cmd,
                ctrl,
                alt,
                shift,
                key: c.key.clone(),
            });
            out.push(match key {
                Some(chord) => NativeEvent::Key(chord),
                None => NativeEvent::Click(e.command.clone()),
            });
        }
        out
    }

    fn build(&mut self, bar: &MenuBar) {
        if let Some(old) = self.menu.take() {
            old.remove_for_nsapp();
        }
        self.entries.clear();
        self.window = None;
        let menu = Menu::new();
        let mut special = Vec::new();
        for m in &bar.menus {
            let (sub, ids) = self.submenu(m);
            let _ = menu.append(&sub);
            if m.role == MenuRole::Window {
                self.window = Some((sub.clone(), ids, window_key(bar)));
            }
            if matches!(m.role, MenuRole::Window | MenuRole::Help) {
                special.push((m.role, sub));
            }
        }
        menu.init_for_nsapp();
        for (role, sub) in special {
            match role {
                MenuRole::Window => sub.set_as_windows_menu_for_nsapp(),
                _ => sub.set_as_help_menu_for_nsapp(),
            }
        }
        self.menu = Some(menu);
        self.structure = structure_key(bar);
    }

    fn rebuild_window(&mut self, bar: &MenuBar) {
        let (Some(menu), Some((old, ids, _))) = (self.menu.clone(), self.window.take()) else {
            return self.build(bar);
        };
        let Some((at, spec)) = bar.menus.iter().enumerate().find(|(_, m)| m.role == MenuRole::Window) else {
            return self.build(bar);
        };
        if menu.remove(&old).is_err() {
            return self.build(bar);
        }
        for id in ids {
            self.entries.remove(&id);
        }
        let (sub, ids) = self.submenu(spec);
        if menu.insert(&sub, at).is_err() {
            return self.build(bar);
        }
        sub.set_as_windows_menu_for_nsapp();
        self.window = Some((sub, ids, window_key(bar)));
    }

    fn submenu(&mut self, m: &super::model::Menu) -> (Submenu, Vec<String>) {
        let before: HashSet<String> = self.entries.keys().cloned().collect();
        let sub = Submenu::new(escape(&m.title), true);
        self.append(&sub, &m.children);
        let ids = self.entries.keys().filter(|id| !before.contains(*id)).cloned().collect();
        (sub, ids)
    }

    fn append(&mut self, sub: &Submenu, nodes: &[Node]) {
        for n in nodes {
            match n {
                Node::Separator => {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                }
                Node::Submenu { label, children, .. } => {
                    let s = Submenu::new(escape(label), !children.is_empty());
                    self.append(&s, children);
                    let _ = sub.append(&s);
                }
                Node::Standard(item) => {
                    let p = match item {
                        Standard::Services => PredefinedMenuItem::services(Some("Services")),
                        Standard::HideOthers => PredefinedMenuItem::hide_others(Some("Hide Others")),
                        Standard::ShowAll => PredefinedMenuItem::show_all(Some("Show All")),
                        Standard::Minimize => PredefinedMenuItem::minimize(None),
                        Standard::Zoom => PredefinedMenuItem::maximize(Some("Zoom")),
                        Standard::BringAllToFront => PredefinedMenuItem::bring_all_to_front(None),
                        Standard::CloseWindow => PredefinedMenuItem::close_window(None),
                    };
                    let _ = sub.append(&p);
                }
                Node::Item(it) => {
                    let id = self.unique_id(&it.id);
                    let chord = it.shortcut.as_deref().and_then(Chord::parse);
                    let accel = chord.as_ref().filter(|c| c.native_ok()).and_then(accelerator);
                    let handle = match it.kind.checked() {
                        Some(c) => {
                            let h = CheckMenuItem::with_id(id.clone(), escape(&it.label), it.enabled, c, accel);
                            let _ = sub.append(&h);
                            Handle::Check(h)
                        }
                        None => {
                            let h = MenuItem::with_id(id.clone(), escape(&it.label), it.enabled, accel);
                            let _ = sub.append(&h);
                            Handle::Plain(h)
                        }
                    };
                    self.entries
                        .insert(id, Entry { handle, command: it.command.clone(), chord, label: it.label.clone(), enabled: it.enabled });
                }
            }
        }
    }

    fn unique_id(&self, id: &str) -> String {
        let mut unique = id.to_string();
        let mut n = 2u32;
        while self.entries.contains_key(&unique) {
            unique = format!("{id}#{n}");
            n = n.saturating_add(1);
        }
        unique
    }
}

impl Entry {
    fn command_is_hide(&self) -> bool {
        matches!(self.command, Command::HideApp)
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&&")
}

fn accelerator(c: &Chord) -> Option<Accelerator> {
    let mut mods = Modifiers::empty();
    for (on, m) in [(c.cmd, Modifiers::META), (c.ctrl, Modifiers::CONTROL), (c.alt, Modifiers::ALT), (c.shift, Modifiers::SHIFT)] {
        if on {
            mods |= m;
        }
    }
    const LETTERS: [Code; 26] = [
        Code::KeyA,
        Code::KeyB,
        Code::KeyC,
        Code::KeyD,
        Code::KeyE,
        Code::KeyF,
        Code::KeyG,
        Code::KeyH,
        Code::KeyI,
        Code::KeyJ,
        Code::KeyK,
        Code::KeyL,
        Code::KeyM,
        Code::KeyN,
        Code::KeyO,
        Code::KeyP,
        Code::KeyQ,
        Code::KeyR,
        Code::KeyS,
        Code::KeyT,
        Code::KeyU,
        Code::KeyV,
        Code::KeyW,
        Code::KeyX,
        Code::KeyY,
        Code::KeyZ,
    ];
    const DIGITS: [Code; 10] = [
        Code::Digit0,
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    let k = c.key.as_str();
    let mut chars = k.chars();
    let code = match (chars.next(), chars.next()) {
        (Some(ch), None) if ch.is_ascii_alphabetic() => {
            let idx = (ch.to_ascii_uppercase() as usize).checked_sub('A' as usize)?;
            *LETTERS.get(idx)?
        }
        (Some(ch), None) if ch.is_ascii_digit() => {
            let idx = (ch as usize).checked_sub('0' as usize)?;
            *DIGITS.get(idx)?
        }
        _ => match k {
            "=" => Code::Equal,
            "-" => Code::Minus,
            "," => Code::Comma,
            "." => Code::Period,
            "/" => Code::Slash,
            "[" => Code::BracketLeft,
            "]" => Code::BracketRight,
            "\\" => Code::Backslash,
            "F2" => Code::F2,
            "F5" => Code::F5,
            "ArrowRight" => Code::ArrowRight,
            "ArrowLeft" => Code::ArrowLeft,
            "ArrowUp" => Code::ArrowUp,
            "ArrowDown" => Code::ArrowDown,
            _ => return None,
        },
    };
    Some(Accelerator::new(mods, code))
}
