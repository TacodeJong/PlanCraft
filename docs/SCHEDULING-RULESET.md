# Scheduling ruleset (stretch, Auto & successor caps)

PlanCraft’s group-share scheduler mirrors the Project Planner app
(`Scheduler.resolvingPercentDurations`, `groupShare`, `limitingSuccessorShares`) and the
backward hard-date pressure described in `Docs/Scheduling.md`.

Code: `crates/plancraft-core/src/shares.rs` (`resolve` → allotments → `cap_for_successors`),
then `schedule::recalculate` (CPM). Overview of CPM itself: [ARCHITECTURE.md](ARCHITECTURE.md)
(“Scheduling”).

This document covers **how durations are shared inside a group**, **how Auto differs from
typed length**, and **how successor-date caps (and their warnings) are calculated** — including
failure modes that produce misleading “no room left” notes.

---

## Where it sits in a recalculation

1. **Share resolve** (`shares::resolve`) — when any share driver is present: turn Fill /
   Finish-On / percent into concrete working minutes; shrink those budgets so pinned
   successors still hold; emit notes.
2. **CPM** (`schedule::recalculate`) — forward/backward passes, constraints, ALAP, slack,
   roll-ups. Uses the resolved minutes for that pass; **stored** `Task.duration` stays the
   fallback (unchanged by Fill).

Share resolution runs when project-wide stretch/span rules are on, **or** when any task has
`DurationMode::Auto`, `fills_group`, or a positive `duration_percent`. Because new tasks
default to `fills_group = true`, almost every non-trivial plan runs share resolve.

---

## Size of group (per-task)

The Properties / Task Information **Size of group** control sets per-task flags (Swift
`GroupSizeChoice`). These work **without** flipping project-wide stretch/span:

| Choice | Flags | Behaviour |
|---|---|---|
| Spans its activities / Own duration | `fills_group=false`, `duration_percent=None` | Stored duration (or children for summaries) |
| % of group | `duration_percent = q` (clears fill) | **Span**: `P × q / 100` |
| Remaining space in group | `fills_group=true`, `duration_percent=None`, both sides Unrestricted | **Fill**: all-or-nothing among sibling fills — if **any** fill depends on another, **every** fill divides remaining by the fill count; only when none depends on another does each get the full remaining window |

**Defaults:** `Task::new` / `add_task` set `fills_group = true` (Swift Remaining space). A missing
`fills_group` field on deserialize still yields `false`, so older PlanCraft files keep stored
durations until the user opts in via Size of group. New tasks therefore enter the Fill path
(when both date sides stay Unrestricted); “Size = Own → CPM as usual” is an explicit choice,
not the create default.

---

## Duration mode: Fixed vs Auto

| Mode | Meaning |
|---|---|
| **Fixed** | Typed / stored duration is the plan (editable). Legacy `fills_group` / `%` still apply when set. |
| **Auto** | Duration is calculated from the dependency / date window (read-only while Auto). Leaf only; summaries use `fills_group`. |

### How Auto is classified (`share_of`)

| Auto row | Share class | Length comes from |
|---|---|---|
| Finish side is **On** (finish earliest = finish latest) | **FinishOn** | Early start → pinned finish (date rule wins over typed length and over equal sibling fill) |
| Otherwise | **Fill** | Remaining group window (same path as Remaining space) |
| Soft Finish-no-later alone (not On) | Still **Fill** | Soft NLT must not leave Auto stuck on the typed duration |
| Paired On/On (or hard start floor + finish deadline) via `date_duration_bounds` | Not a share | Working duration from that window before CPM |

Typing an own duration clears Auto / fill / percent (Size → Own). Clear Dates on parented rows
drops user date bounds/deadline but keeps ASAP/ALAP and Size-of-group / Auto flags.

---

## Date modes and soft vs hard pins

`DateMode` (Unrestricted / On / No earlier / No later / Between) is read from each side’s
earliest/latest bounds. Legacy Remaining space (`fills_group`) applies only while **both**
sides are Unrestricted. Choosing Remaining space clears date windows on both sides.

**Hard** pins move the plan and always seed successor caps:

- Fixed latest start / finish (`start_latest_fixed` / `finish_latest_fixed`)
- Constraints: MSO, SNLT, MFO, FNLT

**Soft** (advisory) pins — bound fields **without** the fixed flag — are honoured only for
**Auto** rows when building successor caps (`include_soft = true`). Stretch / percent use hard
only. Only soft **finish-latest** seeds a finish cap; soft **start-latest** does **not** seed
(and must not invent a predecessor cap before the bar’s real start). Soft finish caps that land
**at or before** the row’s probe start are ignored (they are not a usable window).

### Auto vs legacy Fill when dates are set

| Driver | Own start/finish dates | Share class |
|---|---|---|
| Legacy `fills_group` | Any own date mode other than Unrestricted/Unrestricted | **Not a fill** — stored duration |
| `DurationMode::Auto` | Soft Finish-no-later (not On) | Still **Fill** |
| `DurationMode::Auto` | Finish **On** | **FinishOn** (pin-driven length) |

---

## Behaviour matrix

| Situation | Result |
|---|---|
| Leaf with stored duration, FS chain, Size = Own | CPM as usual (not the create default — see Defaults) |
| `fills_group`, both sides **Unrestricted** | **Fill**: all-or-nothing (`fills_are_independent`) — any link among fills → split by fill count; else each gets full remaining `P` |
| `fills_group` with any own start/finish date | **Not a fill** (`groupShare` is nil); stored duration |
| `DurationMode::Auto`, finish not On | **Fill** (even with soft NLT) |
| `DurationMode::Auto`, finish **On** | **FinishOn**: early start → pin |
| `duration_percent = q` | **Span**: `P × q / 100` (still applies with date bounds) |
| Group window `P` | Allotted share, else start→latest finish (own or earliest ancestor deadline/FNLT/finish_latest; start may use ancestor SNET), else span of non-share children (share children measured at zero), else successor hard-date window |
| Nested fill/percent group | Parent allotment becomes child `P`; `group_min` keeps the nested span |
| Nested Auto/fill under unrestricted summary | Ancestor finish/start bounds trickle into the nested group’s `P` (summary need not be Fill) |
| Nested summary with `fills_group` + unrestricted | Classifies as Fill; allotment feeds children |
| Nested share allotment `L` in parent window `[Ws, We]` | Place a block of length `L` by alignment (below); children schedule inside that placed block |
| Nested share + **ASAP** | Positioned block aims for `S = Ws`, `F = S + L`. Roll-up start is `min(child starts, Ws)` — children already before `Ws` pull the group earlier (`nested_percent_asap_starts_at_parent_window`) |
| Nested share + **ALAP** | Positioned block aims for `F = We`, `S = F − L`. Roll-up finish is `max(child finishes, We)` — children past `We` push the group later (`nested_percent_alap_finishes_with_parent_window`) |
| Parent positioned edge → descendants | Nested ASAP children start at the positioned parent start when it is the floor; they do not independently re-derive `Ws` |
| Nested share children shorter than `L` | Group bar still spans the full `L` (pad) |
| Nested share children longer than `L` | Group grows; “needs more than its share… grows to fit” note |
| Independent parallel fill children | Each occupies the full remaining window (`parallel_stretch_children_each_fill_the_fixed_window`) |
| Any link among fill siblings | **All** fills (including an unlinked peer) divide remaining by fill count — e.g. A→B plus independent C → each gets `P/3`, not full window for C |
| Serially linked fill children | Divide the remaining window along the serial chain (`serial_stretch_children_split_a_fixed_window`) |
| Parallel **fixed** sibling | Does **not** shrink Auto remaining (overlap in time); serial fixed predecessor still does |
| ALAP on a nested share summary | Positions the group block at the parent-window end via `effective_alap` (own ALAP **or** nearest ancestor ALAP). A nested summary that is ASAP itself cannot opt out of an ancestor’s ALAP; leaf ASAP/ALAP inside the moved window is unchanged |
| Edit Size of group on a selected summary | Changes that summary only; selected/hidden descendants retain their own `%` / fill policy |
| Child fixed SNLT / start-latest | Caps ancestor group early start (`fixedStartCap`) and unrestricted siblings |
| Leaf On/On (or hard start floor + finish deadline) | Working duration from the date window before CPM (`date_duration_bounds` → `apply_date_driven_durations`). Manual tasks with both typed dates read as On/On via `start_window`/`finish_window`, so they get the same date-driven length |
| Percent sum ≤ / = / > 100% | No hard cap; group grows; slack/deadline shows the overrun |
| No room for fills in a windowed group | Fills keep stored duration; note on the group |
| No window at all | Fills keep stored duration; “no window” note |
| Hard successor pin (FNLT / MFO / MSO / SNLT / fixed bounds) | Share budgets shrink so the predecessor finishes by the pinned successor start; notes when shortened |
| Soft successor pin | Same, but **only** for Auto; ignored if soft cap ≤ probe start |
| Impossible window (effective cap ≤ start) | Keep stored duration (not a zero bar); note that there is no room before the successor cap |
| Typing Own duration | Clears `fills_group` / `duration_percent` / Auto (Size of group → Own) |
| Clear Dates (parented rows) | Drops user date bounds/deadline; keeps ASAP/ALAP, `fills_group`, `duration_percent`, Auto |
| Stored `duration` | Unchanged (fallback); `sched.duration` carries the resolved length |
| Calendars / weekends / holidays | Leaf CPM uses each task’s calendar; share `P`, successor caps, and lag on cap edges use the **project** calendar (same as CPM lags / summary durations) |

---

## Allotment algorithm (before caps)

For each parent with children:

1. Measure group length `P` (allotment, ancestor/own finish window, sibling span, or successor window).
2. Assign **percent** rows: `P × q / 100`; subtract from remaining.
3. Assign **FinishOn** rows: `between(early_start, min(finish_pin, window_end))`. If a FinishOn
   sits **before** fills in the dependency order among siblings, subtract that length from
   remaining (serial). Parallel FinishOn does not starve fills.
4. Subtract **fixed** siblings that **precede** any fill in the sibling dependency graph
   (serial only — parallel fixed neighbours must not starve Auto).
5. Give **Fill** rows the remaining window (`fills_are_independent`):
   - **No** fill depends on another fill → each gets the full remaining amount.
   - **Any** fill→fill link among siblings → **all** fills divide remaining by fill count
     (remainder minutes distributed). A mixed graph (A→B plus independent C) still splits
     three ways — C does not keep a full window.
6. If remaining is zero → fills keep **stored** duration as a budget (so capping can still
   shrink them); group note if the group was windowed.

Unresolved Auto (no parent / unsupported) gets an honest task note via `note_unresolved_auto`.

---

## Successor capping (how “no room” / “shortened” are calculated)

After allotments, `cap_for_successors` ensures stretch / percent / Auto budgets do not cross
pinned successor dates (Swift `limitingSuccessorShares`).

### 1. Budgets

Every row that received a plan duration, plus share rows that fell back to stored duration,
enters the budget list. Auto rows set `include_soft = true`; others hard-only.

### 2. Early-start probe + re-cap passes

Clone the project, force Fixed modes, apply plan durations, then **zero every share budget**
and recalculate. Each budget row’s window start is that **early** ASAP start — not the start
after inflated fill lengths (those let soft Finish-On predecessors slip and made later Auto
rows look like they had “no room”).

Each pass sets the budget to `min(original fill, start→cap)` (and may **grow back** when a
later pass moves the start earlier because a predecessor Auto also shortened). A single
early probe can still over-allow duration: soft Finish-On predecessors slip under the first
capped lengths, the real start moves later, and the bar would finish after the cap (cyclus
`#101` Auto → `#102` milestone → SS‹ `#129` soft On). Re-capping against the slipped start
is what lets successor-driven (Opv) links see the predecessor on the successor’s date.
Shrink-only iteration left `#101` stuck at mid-pass `2.4d` after its start moved back to
Tuesday — finishing Thursday instead of Friday before the Monday pin.

### 3. Build finish-cap maps (`successor_hard_finishes`)

Run twice when any Auto is present: hard (`include_soft = false`) and soft (`true`).

**Seed** each task’s own pin, using the task’s **stored** duration only (never the probe fill
allotment):

| Pin | Seeded finish |
|---|---|
| Hard pinned start (fixed start-latest / MSO / SNLT) | start + stored duration (or start if milestone) |
| Fixed finish-latest | that date |
| Soft finish-latest | that date, only if `include_soft` |
| Soft start-latest | **not seeded** (intentional; only hard/fixed start-latest / MSO / SNLT seed a start) |
| MFO / FNLT constraint | constraint date |

**Propagate backward** along dependency edges (repeated reverse outline passes until stable):

| Link | Predecessor finish must be ≤ |
|---|---|
| FS / SS | successor **start** hard, pulled back by lag (`unapply_lag`: working, elapsed, or % of predecessor stored duration) |
| FF / SF | successor **finish** hard, same lag treatment |

Successor **start** hard = pinned start, else `finish_hard − stored duration` (milestone → finish).

Keep the **earliest** finish date that still satisfies every successor.

> **Why stored duration matters:** converting `MFO − probe_fill` with a multi-week Auto
> allotment walks the cap months earlier than the typed 1d length. That invented impossible
> July caps on August bars (e.g. cyclus chain through an ALAP Auto into a board MFO). Caps
> always use **stored** length when backing a finish pin into a start bound.

### 4. Effective cap per budget row

```
hard = hard_finish[i]
soft = soft_finish[i]          # only computed if any Auto

if Auto:
    if soft exists and soft > probe_start:
        cap = min(soft, hard) if hard else soft
    else:
        cap = hard             # ignore soft ≤ start
else:
    cap = hard
```

### 5. Apply

| Condition | Action | User-visible note |
|---|---|---|
| No cap | Leave budget | — |
| `cap ≤ probe_start`, or `between(start, cap) == 0` | Keep **stored** duration (never collapse to zero) | `… has no room left because successor cap is {date}; keeping {dur}` |
| Working minutes `start→cap` < budget (and > 0) | Shorten budget to that window | `… shortened … because successor cap is {date}` |
| Window ≥ budget | Leave budget | — |

Notes land on the task (`sched.warnings` after recalculate) via `SharePlan.task_notes`.

### Flow

```text
allotments
    │
    ▼
pass 0: zero share budgets ──► earliest ASAP starts
    │
    ├─► hard finish caps (stored durations)
    └─► soft finish caps (Auto only)
            │
            ▼
     per budget: pick cap ──► shorten
            │
            ▼
pass 1..n: re-probe with capped lengths ──► shrink again if soft preds slipped
            │
            ▼
     notes from original allotment → final budget
```

---

## Reading cap warnings

| Note pattern | Meaning | Often legitimate? |
|---|---|---|
| `shortened from … to … because successor cap is D` | Fill/Auto wanted more than `start→D` | Yes — a real pin (or soft pin after start) binds the bar |
| `has no room left … keeping {stored}` | Effective cap ≤ probe start | Only when a **hard** pin truly sits at/before that start. Soft-only or fill-inflated caps must not produce this |
| Group `has no room left for its unrestricted activities` | Group window exhausted before fills | Yes — fills fall back to stored length |
| `Finishes after successor … pinned` / `Pinned date overrides the finish-to-start link` | CPM overrun flags | Only when **project-wide** `ScheduleRules` are enabled (`flag_pinned_successor_overruns`); per-task Fill/Auto alone does not emit these |
| `Cannot finish by ancestor summary … fixed finish date` | CPM / summary deadline (not share-cap) | Separate from successor-cap notes |

If a hover date on a warning looks months earlier than the bar on the Gantt, treat it as a
cap-math bug candidate: check soft vs hard, and whether some downstream Auto fill was
subtracted from an MFO/FNLT.

---

## Intentional notes / approximations

- When Project Planner leaves a fill out of `shareBudgets` (no room), PlanCraft still registers
  the stored duration as a budget so successor capping can shrink it. Share-cap notes still fire
  for Auto/Fill without project-wide rules. The separate CPM warnings that a bar finishes after
  a pinned successor (`flag_pinned_successor_overruns`) run **only** when `settings.rules` is
  enabled — so “bars must not cross a pinned successor without a warning” is true for share-cap
  notes always, and for those CPM pin warnings only with the ruleset on.
- Progressive binary-search redistribution of independent fill branches (Swift’s multi-pass
  freeze of binding paths) is approximated by a single probe-and-cap pass; nested percent
  re-resolution uses the allotted group minimum from the first pass.
- Finish-On probe strips the finish pin temporarily so early start is not glued to the On date
  (otherwise length fell back to the typed duration).

---

## Tests

- Unit: `crates/plancraft-core/src/shares.rs` (`shares::tests`) — nest fill/own, overflow,
  impossible cap keep-stored, On/On date-driven duration, Auto FinishOn vs Fill, soft NLT still
  fills, parallel fixed does not starve Auto, soft successor before start ignored, successor
  cap uses stored duration (not fill allotment)
- Auto duration: `crates/plancraft-core/tests/auto_duration.rs`
- Summary edges / `fixedStartCap` / ALAP pad: `crates/plancraft-core/tests/summary_group_edges.rs`
- Warning attribution: `crates/plancraft-core/tests/share_warning_locations.rs`
- Matrix + cyclus 1.7: `crates/plancraft-core/tests/scheduling.rs`
  (`stretch_span_successor_matrix`, `cyclus_beleidsbrief_predecessor_respects_pinned_successor`)
- Speed gate: `crates/plancraft-core/tests/speed.rs` (unchanged bound)
