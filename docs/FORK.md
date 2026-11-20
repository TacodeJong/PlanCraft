# This fork

This repository is a fork of [rallegretti/PlanCraft](https://github.com/rallegretti/PlanCraft) **v0.7.0**, maintained at [TacodeJong/PlanCraft](https://github.com/TacodeJong/PlanCraft). Upstream remains CC0; this fork stays on the same license.

Version **0.8.0** marks the first published release of the fork (branched from upstream `0.7.0`).

## Why it exists

It adapts PlanCraft so existing Project Planner documents open here, schedule with PlanCraft’s engine under Merlin-style rules, and pick up UI and reporting features the Swift Project Planner app had that stock PlanCraft 0.7.0 did not.

## What this fork adds (high level)

- **Scheduling:** stretch / span group rules, dual date bounds, explicit Unrestricted / On / No-earlier / No-later / Between date modes per side (Swift `DateMode` parity), successor capping aligned with Project Planner.
- **Import / export:** Project Planner JSON import; MSPDI fixes and Planner extended attributes; Excel report / Gantt / Task Usage exports; Task Usage print.
- **Views & chrome:** properties pane; Overview; Deliveries / agenda; Report view; ArtCraft restyle; Lucide icons; native macOS menus (muda); EN/NL localization; dd-mm-yyyy dates.
- **Editing:** Quick Entry; Distill Plan (keychain-stored API keys); row marks; status ↔ Task Board sync; column chooser / drag-reorder; global Q/M/D timescale; subtree filter; scheduling warnings surfaced in UI and MCP.

## Tracking upstream

Keep `upstream` pointed at `https://github.com/rallegretti/PlanCraft.git` and merge selectively. Do not assume every upstream change applies cleanly to the Merlin ruleset or ArtCraft UI.

## Out of scope (for now)

Reminders, Planner-JSON round-trip export, Distill from PDF/Word/Excel, and other items listed under “Deliberately out” in the project backlog stay deferred unless product decisions change.
