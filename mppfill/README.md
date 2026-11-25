# mppfill

Standalone tool that reads a Microsoft Project XML file and writes a native `.mpp` by filling a template. It does not schedule the plan. PlanCraft remains the scheduler. Microsoft Project is not required to run this tool.

The default template is `D:\Drenthe\PLANNER-cyclus.mpp`. That file stays outside this folder: it can carry personal metadata, and it is not redistributed.

pymppwriter **0.4.3** (MIT, pure Python, dependency `olefile`) writes MPP14, the format used by Project 2010 through Microsoft 365. It is early-stage. A file it writes can still be recalculated when Project opens it.

## Setup

```text
cd mppfill
python -m venv .venv
.venv\Scripts\python.exe -m pip install -r requirements.txt
.venv\Scripts\python.exe -m pip install -e .
```

Python 3.9 or newer.

## Fill the template from XML

```text
.venv\Scripts\mppfill.exe
.venv\Scripts\mppfill.exe xml-to-mpp plan.xml --out plan.mpp
```

With no file arguments, a dialog asks for the XML and then for where to save the `.mpp`.

`--template` overrides the template path. `MPPFILL_TEMPLATE` does the same when `--template` is omitted.

The command refuses to overwrite `plan.mpp`, the XML file, or the template. Pass `--force` to replace an existing output file. The template itself is only read.

Lines under **Not written** are fields the XML contained that this export does not carry. **Schedule warnings** come from pymppwriter: Project will accept the file and then move a task, or a task calendar shares no working time with its resources. `--strict` exits with status 2 when any schedule warning was reported.

```text
.venv\Scripts\mppfill.exe inspect plan.mpp
```

## Import into Planner Premium

`push` writes the `.mpp` and imports it into a new Planner project. Before writing, it applies the Planner profile from `docs/PLANNER-IMPORT.md`: leaf starts are pinned with start-no-earlier-than, and manual scheduling and deadlines are dropped. The first run opens a browser window so you can sign in. Later runs reuse that sign-in. The token is kept in your local profile (`%LOCALAPPDATA%\mppfill`) and is not printed. When the import has finished, the command opens the new project on its Tasks tab (Taken). The link is built from the project id the import returns, with `TabToFocus=PexWebTab`. Loading that tab is what lists the plan in Planner Premium.

```text
.venv\Scripts\mppfill.exe push plan.xml --out plan.mpp --force
```

`--mpp plan.mpp` imports a file that is already written and skips the XML step. `--name` sets the project name. `--org` and `--calendar` override the Dynamics address and the work-hour template; `MPPFILL_ORG` and `MPPFILL_CALENDAR` do the same. Each run creates a new import row. Do not upload a second file onto an existing row.

`inspect` prints what pymppwriter can read back. That is a smaller set than what `xml-to-mpp` writes. See the matrix below.

## What the template must be

pymppwriter clones task records out of the template and patches them. The template has to be an MPP14 file saved by a licensed desktop Project, ideally the same edition that will open the result.

`PLANNER-cyclus.mpp` stores the task unique id at the field-map offset, not at byte 0. pymppwriter 0.4.3 looks at byte 0, so on this file the leaf prototype has no name and every task would be written blank. `mppfill` keeps only a name entry on each prototype and clears progress and constraint values copied from a real task. A Project-saved copy of the finished plan is not a template: filling it makes Planner refuse the file. The working import is written in `docs/PLANNER-IMPORT.md`.

The same file contains the plan's resources. pymppwriter copies every resource row it finds in the template, so `mppfill` keeps the unnamed resource row and drops the rest. Resources in the output are the ones from the XML. Calendars that were already in the template stay in the file; the XML calendar is applied on top.

## Field matrix (pymppwriter 0.4.3)

Checked against the pinned library source, not the parts of its README that disagree with each other. Baseline cost is not written: the library stores a cost number whose scale it describes as not yet established. `read_project` does return baseline dates, duration and work when they are present.

| Field | XML → MPP | MPP → inspect |
| --- | --- | --- |
| Title, start, status date, currency, author, manager, company | written | title and start only |
| Task id, name, outline, parent, start, finish, duration, units, estimated | written | read |
| Milestone, summary rollup | written | duration and outline come back; the summary flag is implied by children |
| Notes, WBS, priority, task type, effort-driven, manual | written | notes, WBS, priority, manual |
| Percent complete | written | read |
| Constraints and deadlines | written | constraint type and date; deadline is not read |
| Text, number, date, flag fields | written | not read |
| Custom cost and duration, interim plans | reported, not written | not read |
| FS, SS, FF, SF, working lag and lead | written | read |
| Percent lag, cross-project links | reported, not written | — |
| Work resources, assignments, units | written | read |
| Material and cost resources, rates, contours, availability | reported, not written | not read |
| Base calendar week, non-working days, yearly holidays expanded to dates | written | not read |
| Working-time exceptions, dated work weeks, resource calendars | reported, not written | not read |
| Baselines 0–10 (start, finish, duration, work) | written | read |
| Baseline cost, timephased data | reported, not written | cost not read; timephased not read |
| Inactive tasks | reported, written as active | not read |

Summary start, finish and duration in the `.mpp` are the rollup of the children in working time. A leaf keeps the start, finish and duration from the XML. Project may still move an automatically scheduled task the next time it calculates, unless a constraint holds the date. This tool does not calculate critical path, float, or resource levelling.

## Tests

```text
.venv\Scripts\python.exe -m unittest discover -s tests -v
```

The template round-trip runs only when `D:\Drenthe\PLANNER-cyclus.mpp` (or `MPPFILL_TEMPLATE`) is present. It compares names, outline, the link, the resource and the percent complete after `read_project`, and it checks the template bytes did not change.
