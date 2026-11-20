# Based on PlanCraft

This repository ([TacodeJong/PlanCraft](https://github.com/TacodeJong/PlanCraft)) is a **new desktop planner** built on [rallegretti/PlanCraft](https://github.com/rallegretti/PlanCraft) **v0.7.0**. Upstream remains CC0; this line stays on the same license.

Version **0.8.0** is the first published release of this line (branched from upstream `0.7.0`).

## Why it exists

PlanCraft 0.7.0 is a strong base. This line keeps that engine and file format, then adds scheduling rules, a different UI chrome, localization, and reporting/export work aimed at day-to-day project planning — not a thin skin over the upstream app.

## What 0.8.0 adds (high level)

- **Scheduling:** stretch / span group rules, dual date bounds, explicit Unrestricted / On / No-earlier / No-later / Between date modes per side, successor capping for stretch/span.
- **Import / export:** additional JSON import path; MSPDI fixes and extended attributes; Excel report / Gantt / Task Usage exports; Task Usage print.
- **Views & chrome:** properties pane; Overview; Deliveries / agenda; Report view; ArtCraft restyle; Lucide icons; native macOS menus (muda); EN/NL localization; dd-mm-yyyy dates.
- **Editing:** Quick Entry; Distill Plan (keychain-stored API keys); row marks; status ↔ Task Board sync; column chooser / drag-reorder; global Q/M/D timescale; subtree filter; scheduling warnings surfaced in UI and MCP.

## Tracking upstream

Keep `upstream` pointed at `https://github.com/rallegretti/PlanCraft.git` and merge selectively. Do not assume every upstream change applies cleanly to the Merlin ruleset or ArtCraft UI.

## Out of scope (for now)

Reminders, round-trip of the extra JSON import format, Distill from PDF/Word/Excel, and similar deferred items stay out unless product decisions change.
