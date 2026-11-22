# Panel tab chrome — PhotoCraft inspectors

Pin: [storytold/craft-libs](https://github.com/storytold/craft-libs) rev `77c7c62345c1f4213f59a07d9fdc156ecde0b540`.

PhotoCraft side docks use craft-ui **panel** tabs (lighter active fill, muted idle labels, `…` for overflow). PlanCraft’s properties column uses the same chrome; planner fields stay PlanCraft-specific.

## Upstream (tabbed panel frames)

| Piece | File | Behaviour used |
|---|---|---|
| `TabKind::Panel` | `crates/ui-widgets/src/tabs/rules.rs` | `FitMode::Shrink { keep_selected: true }`, `OverflowMenu::HiddenOnly`, no close, clip labels |
| `Tab` paint | `crates/ui-widgets/src/tabs/tab.rs` | Selected = panel surface; idle = muted on strip |
| `TabStrip` | `crates/ui-widgets/src/tabs/strip.rs` | Collection host; `.kind(TabKind::Panel)` |
| Dock group header | `crates/ui-panels/src/docking/group.rs` | Non-closable panels → `TabKind::Panel`; strip + overflow after shown tabs |
| Integration tests | `crates/ui-widgets/tests/it/tabs.rs` | `panel_tabs_keep_the_shown_tab_whole_and_overflow_from_the_end` |

## Why a local mirror

Live `TabStrip` needs the craft-ui component runtime (`StyleCx`, `ShowCollection`, tokens). PlanCraft currently depends on `craft-window` + `craft-ui-shell` for the title bar / `InstallSpec`, not full `craft-ui` / `craft-ui-panels` docking. Until that lands, `widgets::panel_tabs` mirrors the Panel kind’s structure and fit rules.

## PlanCraft wiring

| Surface | Role |
|---|---|
| `widgets::panel_tabs` | Strip: active lighter fill (`PANEL` on `BG`), `…` = hidden tabs only |
| `views/properties.rs` | Task inspectors: General / Planning / Resources / Progress / Notes as panel tabs; empty/resource host = single “Properties” tab |
| `panes::PaneHost` | Show/hide for Properties / Timeline / History (visibility host; Timeline is a Gantt band, History a dialog — not stacked dock groups yet) |

Content under the strip remains planner fields only; chrome matches PhotoCraft panels.
