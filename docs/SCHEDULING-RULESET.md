# Scheduling ruleset (stretch & span)

PlanCraft’s group-share scheduler mirrors the Project Planner app
(`Scheduler.resolvingPercentDurations`, `groupShare`, `limitingSuccessorShares`) and the
backward hard-date pressure described in `Docs/Scheduling.md`.

## Size of group (per-task)

The Properties / Task Information **Size of group** control sets per-task flags (Swift
`GroupSizeChoice`). These work **without** flipping project-wide stretch/span:

| Choice | Flags | Behaviour |
|---|---|---|
| Spans its activities / Own duration | `fills_group=false`, `duration_percent=None` | Stored duration (or children for summaries) |
| % of group | `duration_percent = q` (clears fill) | **Span**: `P × q / 100` |
| Remaining space in group | `fills_group=true`, `duration_percent=None`, both sides Unrestricted | **Fill**: equal share of remaining group window `P` |

`DurationMode::Auto` reuses the same fill path (leaf only; summaries use `fills_group`).

**Defaults:** `Task::new` / `add_task` set `fills_group = true` (Swift). Deserializing a missing
`fills_group` field still yields `false`, so old PlanCraft files keep stored durations until the
user opts in via Size of group.

## Project-wide ruleset

[`ScheduleRules`] stretch/span flags remain available (Project Planner JSON import turns them on).
They are no longer required for Size-in-group fill/percent. Share resolution runs when rules are
on, or when any task has Auto / `fills_group` / a positive `duration_percent`.

## Behaviour matrix

| Situation | Result |
|---|---|
| Leaf with stored duration, FS chain, Size = Own | CPM as usual |
| `fills_group`, both sides **Unrestricted** | **Fill**: equal share of remaining group window `P` |
| `fills_group` with any own start/finish date | **Not a fill** (`groupShare` is nil); stored duration |
| `duration_percent = q` | **Span**: `P × q / 100` (still applies with date bounds) |
| Group window `P` | Allotted share, else start→latest finish (own or earliest ancestor deadline/FNLT/finish_latest; start may use ancestor SNET), else span of non-share children (share children measured at zero), else successor hard-date window |
| Nested fill/percent group | Parent allotment becomes child `P`; `group_min` keeps the nested span |
| Nested Auto/fill under unrestricted summary | Ancestor finish/start bounds trickle into the nested group’s `P` (summary need not be Fill) |
| Nested summary with `fills_group` + unrestricted | Classifies as Fill; allotment feeds children |
| Nested share allotment `L` in parent window `[Ws, We]` | Place a block of length `L` by alignment (below); children schedule inside it |
| Nested share + **ASAP** | `S = Ws`, `F = S + L` (empty trails). `Ws` may be later than the parent’s own start when links delay the nest |
| Nested share + **ALAP** | `F = We`, `S = F − L` (empty leads). `We` is the parent window end (deadline / allotted finish) |
| Parent group edge → descendants | Every nested group/leaf starts from its positioned parent start edge at every depth, whether the edge came from share placement, a date rule, dependency, or manual dates |
| Nested share children shorter than `L` | Group bar still spans the full `L` (pad) |
| Nested share children longer than `L` | Group grows; “needs more than its share… grows to fit” note |
| Independent parallel fill children | Each occupies the full remaining window; overlapping rows do not divide time |
| Serially linked fill children | Divide the remaining window along the serial chain |
| ALAP on a nested share summary | Positions the group block at the parent-window end; descendants move with that window but retain their own ASAP/ALAP policy |
| Edit Size of group on a selected summary | Changes that summary only; selected/hidden descendants retain their own `%` / fill policy |
| Child fixed SNLT / start-latest | Caps ancestor group early start (`fixedStartCap`) and unrestricted siblings |
| Leaf On/On (or hard start floor + finish deadline) | Working duration from the date window before CPM (date-driven) |
| Percent sum ≤ / = / > 100% | No hard cap; group grows; slack/deadline shows the overrun |
| No room for fills in a windowed group | Fills keep stored duration; note on the group |
| No window at all | Fills keep stored duration; “no window” note |
| Fixed FNLT / MFO / MSO / SNLT **successor** | Share budgets shrink so the predecessor finishes by the pinned successor start; bars must not cross; notes when shortened |
| Impossible window (cap ≤ start) | Keep stored duration (not a zero bar); note that there is no room before the successor cap |
| Typing Own duration | Clears `fills_group` / `duration_percent` (Size of group → Own) |
| Clear Dates (parented rows) | Drops user date bounds/deadline; keeps ASAP/ALAP, `fills_group`, `duration_percent` |
| Stored `duration` | Unchanged (fallback); `sched.duration` carries the resolved length |
| Calendars / weekends / holidays | Working-time CPM; same calendars for `P`, caps, and lags |

## Date modes

`DateMode` (Unrestricted / On / No earlier / No later / Between) is read from each side’s
earliest/latest bounds, matching Project Planner. Fill applies only while **both** sides
are Unrestricted. Choosing Remaining space clears date windows on both sides.

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

- Unit: `crates/plancraft-core/src/shares.rs` (`shares::tests`) — nest fill/own, overflow,
  impossible cap keep-stored, On/On date-driven duration
- Summary edges / `fixedStartCap` / ALAP pad: `crates/plancraft-core/tests/summary_group_edges.rs`
- Warning attribution: `crates/plancraft-core/tests/share_warning_locations.rs`
- Matrix + cyclus 1.7: `crates/plancraft-core/tests/scheduling.rs`
  (`stretch_span_successor_matrix`, `cyclus_beleidsbrief_predecessor_respects_pinned_successor`)
- Speed gate: `crates/plancraft-core/tests/speed.rs` (unchanged bound)
