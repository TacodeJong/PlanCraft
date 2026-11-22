//! Craft suite window chrome: live `craft-window` + `craft-ui-shell`.
//!
//! ## Upstream (storytold/craft-libs @ `77c7c62345c1f4213f59a07d9fdc156ecde0b540`)
//! - `craft_window::policy::{decide_for, apply, Frame, MacInset, TitleBarPref, Backend}`
//!   (`crates/window/src/policy.rs`): macOS → [`Frame::Inset`] (full-size content view,
//!   traffic lights over the app bar).
//! - `craft_ui_shell::shell::{Title, TitleBar}` (`crates/ui-shell/src/shell/title_bar/`):
//!   PhotoCraft title slot (`photocraft` `crates/ui-egui/src/panels/title_bar.rs`).
//! - Runtime: `InstallSpec` + `shell::embed::install` (PhotoCraft `app/framework.rs`),
//!   then each frame `embed::before_input` with [`RootEnv`] mode (PhotoCraft pattern).
//!
//! egui/winit: workspace `[patch.crates-io]` matches `craft::egui_fork::REV`
//! (`8a3c5ec…`) and PhotoCraft’s `storytold/winit` pin.

use std::sync::{Arc, OnceLock};

use craft_host::target::{Backend as HostBackend, ShellInfo};
use craft_theme::{ModeChoice, ThemeDef, format as theme_format};
use craft_ui_shell::fonts::I18nScripts;
use craft_ui_shell::runtime::env::RootEnv;
use craft_ui_shell::runtime::frame::FrameInput;
use craft_ui_shell::shell::embed;
use craft_ui_shell::shell::{Title, TitleBar, TitleBarOutput};
use craft_ui_shell::{InstallError, InstallSpec};
use craft_window::Prefix;
use craft_window::policy::{self, Backend, Decision, Frame, MacInset, TitleBarPref, decide_for};
use craft_window::system::SystemWindows;
use egui::{Context, Ui};

use crate::theme::ThemeChoice;

/// Pin documented in ARCHITECTURE.md / title-bar-notes.md.
#[allow(dead_code)] // referenced by docs / future runtime version checks
pub const CRAFT_LIBS_REV: &str = "77c7c62345c1f4213f59a07d9fdc156ecde0b540";

/// Decided frame for PlanCraft’s main window (`PLANCRAFT_TITLEBAR` / `CRAFT_TITLEBAR` override).
#[allow(dead_code)] // used by callers that only need `Frame`, not the full `Decision`
pub fn decide_main_frame() -> Frame {
    decide_main_decision().frame
}

/// Full decision (frame + override metadata) for install / viewport.
pub fn decide_main_decision() -> Decision {
    decide_for(Prefix::new("PLANCRAFT"), Backend::from_env(), TitleBarPref::Integrated)
}

/// `craft_window::policy::apply` — Inset → full-size content view + hidden title bar.
pub fn apply_frame(builder: egui::ViewportBuilder, frame: &Frame) -> egui::ViewportBuilder {
    policy::apply(builder, frame)
}

/// Leading inset (points) reserved for macOS traffic lights under [`Frame::Inset`].
#[allow(dead_code)] // TitleBar reserves lights via WindowHost; kept for non-shell layout
pub fn mac_leading_inset(frame: &Frame) -> f32 {
    match frame {
        Frame::Inset(MacInset { leading, .. }) => *leading,
        _ => 0.0,
    }
}

/// Install the craft-ui runtime once (eframe `CreationContext`), with the window’s frame decision.
/// TitleBar / AppShell read `WindowHost::frame` from this install.
///
/// Fonts use PlanCraft’s ArtCraft faces via [`crate::theme::ui_fonts`] — a bare `UiFonts::new`
/// would replace Proportional/Monospace only and leave named families (`mono`, `display`, …) empty
/// (tofu / missing glyphs).
pub fn install_runtime(ctx: &Context, decision: Decision) {
    let fonts = crate::theme::ui_fonts(&I18nScripts);
    let spec = InstallSpec::new(ShellInfo::default().backend(HostBackend::from_env()), Arc::new(SystemWindows))
        .app_id("plancraft")
        .frame(decision);
    match embed::install(ctx, spec, &fonts) {
        Ok(_) => {}
        Err(InstallError::AlreadyInstalled) => {}
        Err(error) => eprintln!("plancraft: craft-ui runtime install failed: {error}"),
    }
}

/// PhotoCraft `embed::before_input`: feed [`RootEnv`] mode (+ shell theme) each frame so
/// `TitleBar`’s `SurfaceKind::Chrome` resolves dark/light with the app — default RootEnv is Light.
pub fn before_input(ctx: &Context, choice: ThemeChoice) {
    embed::before_input(ctx, |_| FrameInput::new(root_env(choice)));
}

fn root_env(choice: ThemeChoice) -> RootEnv {
    let root = RootEnv::new().mode(mode_choice(choice));
    match shell_theme() {
        Some(theme) => root.library_theme(theme),
        None => root,
    }
}

fn mode_choice(choice: ThemeChoice) -> ModeChoice {
    match choice {
        ThemeChoice::System => ModeChoice::System,
        ThemeChoice::Light => ModeChoice::Light,
        ThemeChoice::Dark => ModeChoice::Dark,
    }
}

/// Dark chrome/surfaces: PhotoCraft Pro (`craft.pro` @ 77c7c623 `themes/pro.json`).
/// Suite chrome defaults to Same(panel), so chrome is pinned to panel `#323232`.
/// Light stays PlanCraft warm paper (`theme::LIGHT` / craft-style-plan).
fn shell_theme() -> Option<Arc<ThemeDef>> {
    static THEME: OnceLock<Option<Arc<ThemeDef>>> = OnceLock::new();
    THEME
        .get_or_init(|| {
            // Dark hexes copied from craft-libs `themes/pro.json` dark (+ chrome = panel).
            const JSON: &str = r##"{
  "schema": "craft-design-theme",
  "schema_version": 1,
  "id": "plancraft.shell",
  "name": "PlanCraft shell",
  "version": 1,
  "variants": "both",
  "seeds": {
    "accent": "#2d81ff",
    "neutral": "#323232",
    "status": {
      "bad": "#f87171",
      "good": "#22c55e",
      "info": "#2d81ff",
      "warn": "#ffc14d"
    }
  },
  "light": {
    "surface.chrome": "#e9e7e2",
    "surface.background": "#f6f5f2",
    "surface.canvas": "#e9e7e2",
    "surface.panel": "#fbfaf8",
    "surface.sunken": "#ffffff",
    "text.normal": "#1d1d21",
    "text.muted": "#636264",
    "accent.solid": "#2d81ff",
    "state.selected": "#d8e4f5"
  },
  "dark": {
    "surface.chrome": "#323232",
    "surface.background": "#1e1e1e",
    "surface.canvas": "#282828",
    "surface.panel": "#323232",
    "surface.sunken": "#242424",
    "text.normal": "#dedede",
    "text.muted": "#b2b2b2",
    "accent.solid": "#2d81ff",
    "state.selected": "#1b2b47"
  }
}"##;
            match theme_format::parse(JSON.as_bytes()) {
                Ok(file) => Some(Arc::new(file.theme)),
                Err(error) => {
                    eprintln!("plancraft: shell theme parse failed ({error}); TitleBar uses suite defaults");
                    None
                }
            }
        })
        .clone()
}

/// PhotoCraft-style title bar: brand/leading, centred document [`Title`], trailing.
/// Requires [`install_runtime`] on this context.
pub fn show_title_bar(
    ui: &mut Ui,
    doc_title: &str,
    mut leading: impl FnMut(&mut Ui),
    mut trailing: impl FnMut(&mut Ui),
) -> TitleBarOutput {
    let title = if doc_title.is_empty() { Title::new("PlanCraft") } else { Title::new(doc_title) };
    TitleBar::new().title_when_native().show_titled(
        ui,
        title,
        |ui, _| leading(ui),
        |ui, _| trailing(ui),
    )
}
