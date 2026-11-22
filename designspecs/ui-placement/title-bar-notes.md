# Title bar — craft-ui / craft-window

Pin: [storytold/craft-libs](https://github.com/storytold/craft-libs) rev `77c7c62345c1f4213f59a07d9fdc156ecde0b540` (PhotoCraft).

## Upstream

| API | Source |
|---|---|
| `craft_window::policy::{decide_for, apply, Frame, MacInset, TitleBarPref, Backend}` | `crates/window/src/policy.rs` |
| `craft_ui_shell::shell::{Title, TitleBar}` | `crates/ui-shell/src/shell/title_bar/` |
| `InstallSpec` + `shell::embed::install` | PhotoCraft `app/framework.rs` / ui-shell embed |

## PlanCraft wiring

- Workspace deps: `craft-window`, `craft-ui-shell` (+ egui/winit `[patch.crates-io]` forks).
- `plancraft-ui-egui::craft_shell`: `decide_main_decision` / `apply_frame` / `install_runtime` / `show_title_bar`.
- macOS default: `Frame::Inset` (traffic lights over the app bar). Override: `PLANCRAFT_TITLEBAR` / `CRAFT_TITLEBAR`.
