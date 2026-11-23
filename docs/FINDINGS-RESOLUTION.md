# Doc inconsistency findings — resolution

Source: agent brief against `ARCHITECTURE.md` and `SCHEDULING-RULESET.md`.
Code is the source of truth. No behaviour changes.

| ID | Verdict | Evidence | Change | Tests run |
|---|---|---|---|---|
| F1 | Doc bug | `xtask/src/layers.rs` (`TABLE`, `INTRA_LAYER_ORDER`); `plancraft-server/Cargo.toml` → core only; `plancraft-ui-egui/Cargo.toml` → mcp + server + engine; `cargo xtask layers` clean | Fixed diagram (no server→engine arrow); table notes L2–L3 empty and L6 intra-order | `cargo xtask layers` |
| F2 | Doc bug (not code); journal claim corrected in round 2 (X1 A) | `plancraft-mcp/src/server.rs` Session + `try_edit`; `plancraft-engine/src/session.rs` `execute_mut` → `try_edit_doc` → `Document::try_edit` | Session when possible, else `try_edit(Agent)`; both end in document `try_edit` (undo + recalc). Session journal/events are **not** retained across `execute_mut` — see X1 A in FINDINGS-RESOLUTION-2 | layers |
| F3 | Intentional | `shares.rs` `own_finish_cap` / `hard_pinned_start`: soft finish seeds; soft start-latest does not | Ruleset: soft start-latest does not seed caps; seed table row added | `soft_successor_start_no_later_does_not_cap_auto_before_its_start` |
| F4 | Doc bug | `shares.rs` allotment: parallel fills get full remaining; serial splits | Size table + matrix: serial equal split / parallel full window | doc only |
| F5 | Doc bug / clarify | `model.rs` `Task::new` `fills_group: true`; serde default false | Defaults note: new tasks almost always run share resolve; Own is opt-in | doc only |
| F6 | Clarify | `share_of`: legacy fill needs unrestricted sides; Auto soft NLT still Fill | Explicit comparison table | doc only |
| F7 | Doc bug | `ARCHITECTURE` elapsed path in scheduling; `Lag::Elapsed` / `task.elapsed` | Time model: working vs elapsed (`ed`) storage | doc only |
| F8 | Doc bug | `schedule.rs` `project_calendar` “lags and summary durations”; `shares.rs` uses `project_calendar` | One wording in both docs: leaf CPM = task calendars; lags / summaries / share P & caps = project calendar | doc only |
| F9 | Intentional | `ARCHITECTURE` hostile input 1900–2300 / 5y vs calendar 1800–2500 | One sentence: input limit vs arithmetic guard | doc only |
| F10 | Clarify | `tests/summary_group_edges.rs` ASAP nest at parent window start | Merged matrix rows: `Ws` = positioned parent window start; descendants follow that edge | doc only |
| F11 | Doc bug | `state.rs` `ViewKind::ALL` length 16 | “sixteen” views; three Gantt variants each count | doc only |
| F12 | Cosmetic | Full hash already in Craft suite pin; abbreviated upstairs | Both pins use full hash + commit link | doc only |

## Code bugs found

None that require a behaviour change. Soft start-latest not seeding (F3) is intentional and documented.
