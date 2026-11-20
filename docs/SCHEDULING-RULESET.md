# Scheduling ruleset (stretch & span)

PlanCraft’s optional ruleset mirrors the Project Planner app’s group-share scheduler
(`Scheduler.resolvingPercentDurations`, `groupShare`, `limitingSuccessorShares`) and the
backward hard-date pressure described in `Docs/Scheduling.md`.

The ruleset is **off** by default (`ScheduleRules` both flags clear). Existing plans and
upstream Microsoft Project–style behaviour are unchanged until a project turns it on
(Project Planner JSON import turns it on automatically).

## Behaviour matrix

| Situation | Ruleset OFF | Ruleset ON (Swift-equivalent) |
|---|---|---|
| Leaf with stored duration, FS chain | CPM as upstream | Same |
| `fills_group`, both sides **Unrestricted** | Stored duration | **Fill**: equal share of remaining group window `P` |
| `fills_group` with any own start/finish date | Stored duration | **Not a fill** (`groupShare` is nil); stored duration |
| `duration_percent = q` | Stored duration | **Span**: `P × q / 100` (still applies with date bounds) |
| Group window `P` | — | Allotted share, else start→latest finish, else span of non-share children (share children measured at zero), else successor hard-date window |
| Nested fill/percent group | — | Parent allotment becomes child `P`; `group_min` keeps the nested span |
| Percent sum ≤ / = / > 100% | — | No hard cap; group grows; slack/deadline shows the overrun |
| No room for fills in a windowed group | — | Fills keep stored duration; note on the group |
| No window at all | — | Fills keep stored duration; “no window” note |
| Fixed FNLT / MFO / MSO / SNLT **successor** | Successor pin moves that task; predecessor may overrun | Share budgets shrink so the predecessor finishes by the pinned successor start; bars must not cross; notes when shortened / collapsed |
| Impossible window (cap ≤ start) | Overlap + warnings | Share collapsed to zero; both rows warned if an FS overrun remains |
| Stored `duration` | Unchanged | Unchanged (fallback); `sched.duration` carries the resolved length |
| Calendars / weekends / holidays | Working-time CPM | Same calendars for `P`, caps, and lags |

## Date modes

`DateMode` (Unrestricted / On / No earlier / No later / Between) is read from each side’s
earliest/latest bounds, matching Project Planner. Stretch applies only while **both** sides
are Unrestricted.

## Successor hard dates

Hard dates that move the plan (fixed latest start/finish, or MSO / SNLT / MFO / FNLT) seed a
finish cap. Dependency edges carry that cap backward (FS/SS/FF/SF, working lags). Stretch and
percent rows are shortened to fit; a pinned successor that is not itself a fill keeps its own
duration.

## Intentional notes

- When Project Planner leaves a fill out of `shareBudgets` (no room), PlanCraft still registers
  the stored duration as a budget so successor capping can shrink it. Bars must not cross a
  pinned successor without a warning.
- Progressive binary-search redistribution of independent fill branches (Swift’s multi-pass
  freeze of binding paths) is approximated by a single probe-and-cap pass; nested percent
  re-resolution uses the allotted group minimum from the first pass.

## Tests

- Unit: `crates/plancraft-core/src/shares.rs` (`shares::tests`)
- Matrix + cyclus 1.7: `crates/plancraft-core/tests/scheduling.rs`
  (`stretch_span_successor_matrix`, `cyclus_beleidsbrief_predecessor_respects_pinned_successor`)
- Speed gate: `crates/plancraft-core/tests/speed.rs` (unchanged bound)
