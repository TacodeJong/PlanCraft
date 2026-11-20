# PlanCraft fork backlog

Last updated: 2026-11-20  
Branch: fork-main

> **Maintainer note:** Update this file after every batch commit; move items Done ↔ In progress ↔ Queued.

## In progress

*(none)*

## Queued (ordered)

1. Optional follow-through: fuller scheduler review vs `Scheduler.swift` / speed characterization (speed gates previously unverified — do not treat as green). Stretch/span successor capping is already merged (`14f342e`).

## Done (recent)

| Commit | Summary |
|--------|---------|
| `3b109ab` | **Export to Microsoft Planner (MVP):** Plan 1+ PSS path (buckets/tasks/deps/assignments) + basic Graph board; Entra device-code auth; refresh token / optional client secret in keyring; File → Export to Microsoft Planner; mapping unit tests. Spec: [`docs/PLANNER-EXPORT.md`](docs/PLANNER-EXPORT.md). Lag/lead, Graph beta deps, live tenant e2e deferred. |
| `14f342e` | Merge `scheduler-equivalence` (`0f20388`): stretch/span successor capping + cyclus 1.7 FNLT predecessor test; kept UI date-mode tests |
| `8a7831c` | Merge `native-menus` (`750d518`): muda AppKit File/Edit/View/Format/Project/Help + shared `Command` model; Export Usage wired into File menu; ArtCraft ribbon kept |
| `45ae50e` | UI mega-batch (rest of the batch `299d825` started): full dialog restyle with ArtCraft headers; Properties/Task Information parity incl. next/prev, date pickers, numeric predecessors, resource edit; General tab everyday fields + Progress tab; datepicker layout; holidays shaded on gantt/timeline **+ testable `shaded_days` helper**; scheduling warnings on row/pane/MCP + Statistics click jumps to the task; prominent Lucide row marks; **explicit Unrestricted/Op/Niet eerder/Niet later/Tussen date modes per side** (Swift `DateMode` parity, Unrestricted clears pins/bounds); **Status ↔ Task Board sync** (canonical columns mirror the status, custom columns stay explicit, old files migrate); **global Q/M/D timescale selector** (View-toolbar chips, chart-zoom + usage-bucket presets, wheel-zoom sync, per-user persisted, disabled off time axis, Excel Gantt follows the shown tier, Quarter buckets added); **Task Usage print + Excel export** (core writer, frozen identity columns, assignment rows, Print window entry, localized commands); Gantt Excel identity columns before the period grid; global subtree filter hardening (deleted focus clears with a toast; prints/exports honor the scope) |
| `4fdd2dd` | Fork baseline on PlanCraft v0.7.0: Merlin stretch/span ruleset + dual date bounds; planner JSON import; deliveries + agenda; properties pane; overview (tiles, day strip); row marks + status cascade; filters / subtree focus; report view; Excel report (outline hierarchy, pivot Data sheet, coloured Gantt cells); MSPDI export fixes + Planner extended attributes; EN/NL localization; date pickers; design styles + group colours; path highlight; notes rows; resource replace-on-delete; Quick Entry (outline + sheet); Distill Plan (keychain-stored keys); MCP assignment editing |
| `e430db8` | Conditional property fields; dd-mm-yyyy dates everywhere; pane localization; per-user column chooser |
| `67472bb` | ArtCraft family restyle: brutalist dark default, derived light, bundled type, chrome, summary brackets |
| `27f5759` | Replace emoji icons with tinted Lucide SVG icons |
| `7fdd5dc` | Drag column headers to reorder the task table |
| `299d825` | Assignment units in their own column (not behind resource names) — first slice of current UI batch |

Also covered by the fork work above (not always separate commits): milestone diamonds, resource/predecessor dropdowns, group bar coloring, status workflow labels (Niet gestart / Gestart / Bezig / Gereed).

## Deliberately out

- Reminders (macOS delivery model undecided; see Open product decisions)
- Planner-JSON **round-trip** export (import stays; no write-back to the old format)
- Distill PDF / Word / Excel input (text/Markdown only for now)
- English-only choice lists (keep localized pickers)
- Microsoft 365 calendar push, task emails, appointment automation, recipient status mail, instructions field (Swift backlog; never built there either)
- Upstream non-Swift items (sprints, swimlanes, portfolio view, …)

## Open product decisions

1. **Reminders on macOS** — native notification crate (signing/permission), `osascript` display notification, or in-app only (bell + Overview card)? Recommend in-app first.
2. **Distill follow-ups** — ship PDF/Word parsing later, or stay text/Markdown? Any additional AI providers beyond the landed keychain path?
3. **MSPDI silent fields** — remaining fork fields still need export notes / ExtendedAttribute mapping completeness (bounded in next-batch §1); keep MSPDI correct as the interchange format.
4. **Row-mark notes (`markNote`)** — import currently drops the reason text; when to add `mark_note` + tooltip?
5. **Milestone types** — managed type list vs deliverable text only?
6. **Export to Microsoft Planner** — **DONE 2026-11-20** (MVP on `fork-main`)
   - Plan 1+ PSS path for dependencies; basic Graph board without deps; Entra device-code + keyring.
   - Remaining gaps (not MVP-blocking): lag/lead, Graph beta `plans_v2` deps until permissions GA, live tenant e2e, durable id-map sync / update push.
