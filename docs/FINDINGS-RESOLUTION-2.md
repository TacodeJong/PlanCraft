# Doc/code findings round 2 — resolution

Source: round-2 brief after F1–F12. Decisions applied 2026-11-23.

| ID | Verdict | Evidence | Change | Tests run |
|---|---|---|---|---|
| X1 | **A — docs only** | `session.rs` `execute_mut` / `edit_doc_mut` drop temp Session journal/events | ARCHITECTURE Commands + MCP tools; `plancraft-mcp` crate docs; F2 row corrected | `cargo xtask layers` |
| C1 | **Fixed** | Caps ignored `Lag::Elapsed` / `Percent` | `schedule::unapply_lag` is `pub(crate)`; `link_pred_finish_cap` + `earliest_successor_cap` use it with predecessor stored duration | `successor_cap_elapsed_lag_shortens_like_cpm`, `successor_cap_percent_lag_shortens_like_cpm` |
| C2 | **Option 1 — keep stored** | `max_dur == 0` used to set duration 0 | Same keep-stored path as `cap ≤ start`; ruleset updated | `zero_working_window_keeps_stored_duration` |
| D1–D8 | Doc fixes (prior turn) | See round-2 brief | SCHEDULING-RULESET + ARCHITECTURE | layers |

## X1 decision

Docs only. Long-lived `Session` (CLI) may accumulate journal/events. UI/MCP `execute_mut` builds a temporary Session and writes back only the workspace; document undo + recalculation + `ChangeSource` still apply via `try_edit`.

Active-doc behaviour (`try_edit_doc` restore / MCP `ws.active`) left as observed — not changed.

## C1 fix

Successor-cap lag math matches CPM `unapply_lag` (working, elapsed, percent of predecessor stored duration). Auto/Fill budgets with `3ed` / `50%` lags now shorten instead of ignoring the lag.

## C2 fix

Impossible window = `cap ≤ start` **or** `between(start, cap) == 0`. Always keep stored duration and emit the “no room left … keeping” note — never collapse the bar to zero.
