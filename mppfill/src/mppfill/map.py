"""Map an MSPDI plan onto the pymppwriter model."""

from __future__ import annotations

from datetime import datetime

from pymppwriter import Assignment, Baseline, Calendar, CalendarException, Project, Relation, Resource, Task
from pymppwriter.writer import advance_working

from .errors import MppFillError
from .mspdi import XmlCalendar, XmlPlan, XmlTask


def to_project(plan: XmlPlan) -> tuple[Project, list[str]]:
    """Build a pymppwriter Project. Notes list what the MPP file will not carry."""
    notes = list(plan.notes)
    if not plan.tasks:
        if plan.start is None:
            raise MppFillError("the project has no start date and no tasks")
        project = _shell(plan, notes)
        project.tasks = []
        return project, notes

    by_uid: dict[int, XmlTask] = {}
    for task in plan.tasks:
        if task.uid in by_uid:
            raise MppFillError(f"duplicate task id {task.uid}")
        by_uid[task.uid] = task

    _parents(plan.tasks)
    _fill_summary_dates(plan.tasks)
    for task in plan.tasks:
        if task.start is None or task.finish is None:
            raise MppFillError(f"task {task.uid} ({task.name or 'untitled'}) has no start or finish")
        if task.finish < task.start:
            raise MppFillError(f"task {task.uid} ({task.name or 'untitled'}) finishes before it starts")
        if task.duration_days < 0:
            raise MppFillError(f"task {task.uid} has a negative duration")

    calendars, default_name, standard, names = _calendars(plan, notes)
    resources, resource_uids = _resources(plan, notes)
    assignments = _assignments(plan, resource_uids, by_uid, notes)
    relations = _relations(plan, by_uid)

    tasks = []
    for task in plan.tasks:
        calendar_name = None
        if task.calendar_uid not in (None, -1):
            calendar_name = names.get(task.calendar_uid)
            if calendar_name is None:
                if "a task calendar was left out because it is not a base calendar the writer can store" not in notes:
                    notes.append("a task calendar was left out because it is not a base calendar the writer can store")
            elif task.calendar_uid == plan.calendar_uid:
                calendar_name = None
        kwargs = {}
        if task.guid is not None:
            kwargs["guid"] = task.guid
        tasks.append(
            Task(
                uid=task.uid,
                name=task.name,
                start=task.start,
                finish=task.finish,
                duration_days=task.duration_days,
                outline_level=task.outline_level,
                parent_uid=task.parent_uid,
                duration_units=task.duration_units,
                estimated=task.estimated,
                calendar=calendar_name,
                notes=task.notes,
                wbs=task.wbs,
                constraint=task.constraint,
                constraint_date=task.constraint_date,
                deadline=task.deadline,
                percent_complete=task.percent_complete,
                priority=task.priority,
                task_type=task.task_type,
                effort_driven=task.effort_driven,
                manual=task.manual,
                text=dict(task.text),
                number=dict(task.number),
                date=dict(task.date_fields),
                flag=dict(task.flag),
                baselines={
                    slot: Baseline(
                        start=base.start,
                        finish=base.finish,
                        duration_days=base.duration_days,
                        work_hours=base.work_hours,
                    )
                    for slot, base in task.baselines.items()
                    if base.start is not None or base.finish is not None
                },
                **kwargs,
            )
        )

    _renumber_tasks(tasks, relations, assignments)

    start = plan.start or min(task.start for task in tasks)
    project = _shell(plan, notes)
    project.start = start
    project.tasks = tasks
    project.relations = relations
    project.resources = resources
    project.assignments = assignments
    project.calendar = standard
    project.calendars = calendars
    project.default_calendar = default_name
    return project, notes


def apply_planner_profile(tasks: list) -> list[str]:
    """Shape tasks the way Planner Premium can import them.

    Leaf tasks keep their start with start-no-earlier-than. Summaries stay
    unconstrained. Manual scheduling, deadlines and every other constraint are
    dropped. A milestone with no duration is one hour from its start before the
    file is written. A typed duration is kept. Planner rebuilds a task's length
    from its assignment, so a later edit of the task row alone leaves the
    assignment at zero and the row comes back as 0 days, which Planner stores
    as 100% complete.
    """
    summary_ids = {task.parent_uid for task in tasks if task.parent_uid}
    pinned = dropped_deadline = cleared_manual = milestones = 0
    for task in tasks:
        if task.manual:
            task.manual = False
            cleared_manual += 1
        if task.deadline is not None:
            task.deadline = None
            dropped_deadline += 1
        if task.uid in summary_ids:
            if task.constraint is not None or task.constraint_date is not None:
                task.constraint = None
                task.constraint_date = None
                pinned += 1
            continue
        if task.duration_days == 0 and task.start is not None:
            task.duration_days = 0.125
            task.finish = advance_working(task.start, 600)
            milestones += 1
        if task.constraint != "SNET" or task.constraint_date != task.start:
            task.constraint = "SNET"
            task.constraint_date = task.start
            pinned += 1
    notes: list[str] = []
    if pinned:
        notes.append("constraints were left out; each leaf start is kept with start-no-earlier-than")
    if dropped_deadline:
        notes.append("deadlines were left out; Planner Premium does not import them")
    if cleared_manual:
        notes.append("manually scheduled tasks were exported as automatically scheduled, keeping their start, finish and duration")
    if milestones:
        notes.append("milestones with no duration are one hour from their start, so an unfinished one stays at 0%")
    return notes


def _shell(plan: XmlPlan, notes: list[str]) -> Project:
    del notes
    return Project(
        title=plan.title,
        start=plan.start or datetime(datetime.now().year, 1, 1, 8, 0),
        author=plan.author,
        manager=plan.manager,
        company=plan.company,
        status_date=plan.status_date,
        currency_symbol=plan.currency_symbol,
        currency_code=plan.currency_code,
    )


def _renumber_tasks(tasks: list[Task], relations: list[Relation], assignments: list[Assignment]) -> None:
    """Number tasks 1..n in list order when the source ids go backwards.

    Project Viewer 365 drops a row whose unique id is not greater than every
    unique id above it. A plan reordered into outline order then shows only
    the tasks that happened to keep climbing, and both the Gantt and the task
    sheet stop early.
    """
    uids = [task.uid for task in tasks]
    if all(later > earlier for earlier, later in zip(uids, uids[1:])):
        return
    mapping = {uid: index for index, uid in enumerate(uids, start=1)}
    for task in tasks:
        task.uid = mapping[task.uid]
        if task.parent_uid:
            task.parent_uid = mapping[task.parent_uid]
    for link in relations:
        link.pred_uid = mapping[link.pred_uid]
        link.succ_uid = mapping[link.succ_uid]
    for assignment in assignments:
        assignment.task_uid = mapping[assignment.task_uid]


def _parents(tasks: list[XmlTask]) -> None:
    stack: list[XmlTask] = []
    for task in tasks:
        if task.outline_level < 1:
            raise MppFillError(f"task {task.uid} has outline level {task.outline_level}")
        while stack and stack[-1].outline_level >= task.outline_level:
            stack.pop()
        if task.outline_level > 1 and (not stack or stack[-1].outline_level != task.outline_level - 1):
            raise MppFillError(
                f"task {task.uid} ({task.name or 'untitled'}) is outline level {task.outline_level} "
                f"without a level {task.outline_level - 1} parent above it"
            )
        task.parent_uid = stack[-1].uid if task.outline_level > 1 else 0
        stack.append(task)


def _fill_summary_dates(tasks: list[XmlTask]) -> None:
    children: dict[int, list[XmlTask]] = {}
    for task in tasks:
        children.setdefault(task.parent_uid, []).append(task)
    seen: set[int] = set()
    for task in tasks:
        if task.parent_uid != 0:
            continue
        stack: list[tuple[XmlTask, bool]] = [(task, False)]
        while stack:
            current, done = stack.pop()
            if current.uid in seen and not done:
                continue
            if not done:
                seen.add(current.uid)
                stack.append((current, True))
                for child in reversed(children.get(current.uid, [])):
                    stack.append((child, False))
                continue
            kids = children.get(current.uid, [])
            starts = [child.start for child in kids if child.start is not None]
            finishes = [child.finish for child in kids if child.finish is not None]
            if current.start is None and starts:
                current.start = min(starts)
            if current.finish is None and finishes:
                current.finish = max(finishes)


def _calendars(plan: XmlPlan, notes: list[str]):
    bases = [cal for cal in plan.calendars if cal.is_base]
    chosen = next((cal for cal in bases if cal.uid == plan.calendar_uid), None)
    if chosen is None and bases:
        chosen = bases[0]
    standard = _as_calendar(chosen, "Standard") if chosen is not None else None
    extra: list[Calendar] = []
    names: dict[int, str] = {}
    default_name = None
    used = {"Standard"}
    if chosen is not None and chosen.name != "Standard":
        extra.append(_as_calendar(chosen, chosen.name))
        used.add(chosen.name)
        names[chosen.uid] = chosen.name
        default_name = chosen.name
    elif chosen is not None:
        names[chosen.uid] = "Standard"
    for cal in bases:
        if chosen is not None and cal.uid == chosen.uid:
            continue
        name = cal.name
        if name in used:
            name = f"{cal.name} ({cal.uid})"
            _note_dup = f"calendar {cal.name!r} was renamed to {name!r} because the name was already used"
            if _note_dup not in notes:
                notes.append(_note_dup)
        extra.append(_as_calendar(cal, name))
        used.add(name)
        names[cal.uid] = name
    return extra, default_name, standard, names


def _as_calendar(src: XmlCalendar, name: str) -> Calendar:
    return Calendar(
        name=name,
        week={day: None if ranges is None else list(ranges) for day, ranges in src.week.items()},
        exceptions=[CalendarException(item.start, item.finish, item.name) for item in src.exceptions],
    )


def _resources(plan: XmlPlan, notes: list[str]):
    seen: set[int] = set()
    out = []
    for resource in plan.resources:
        if resource.uid in seen:
            raise MppFillError(f"duplicate resource id {resource.uid}")
        if not resource.name:
            notes.append(f"resource {resource.uid} has no name and was left out")
            continue
        seen.add(resource.uid)
        kwargs = {}
        if resource.guid is not None:
            kwargs["guid"] = resource.guid
        out.append(
            Resource(
                uid=resource.uid,
                name=resource.name,
                initials=resource.initials,
                email=resource.email,
                max_units=resource.max_units,
                **kwargs,
            )
        )
    return out, seen


def _assignments(plan: XmlPlan, resource_uids: set[int], tasks: dict[int, XmlTask], notes: list[str]):
    seen: set[tuple[int, int]] = set()
    out = []
    for assignment in plan.assignments:
        if assignment.task_uid not in tasks:
            notes.append(f"an assignment refers to missing task {assignment.task_uid} and was left out")
            continue
        if assignment.resource_uid not in resource_uids:
            notes.append(f"an assignment refers to missing resource {assignment.resource_uid} and was left out")
            continue
        key = (assignment.task_uid, assignment.resource_uid)
        if key in seen:
            notes.append(f"a repeated assignment of resource {assignment.resource_uid} on task {assignment.task_uid} was left out")
            continue
        seen.add(key)
        if assignment.units < 0:
            raise MppFillError(
                f"assignment of resource {assignment.resource_uid} on task {assignment.task_uid} has negative units"
            )
        out.append(Assignment(assignment.task_uid, assignment.resource_uid, assignment.units))
    return out


def _relations(plan: XmlPlan, tasks: dict[int, XmlTask]) -> list[Relation]:
    out = []
    seen: set[tuple[int, int, str]] = set()
    for task in plan.tasks:
        for link in task.links:
            if link.pred_uid not in tasks:
                raise MppFillError(f"task {task.uid} depends on missing task {link.pred_uid}")
            if link.pred_uid == task.uid:
                raise MppFillError(f"task {task.uid} depends on itself")
            key = (link.pred_uid, task.uid, link.type)
            if key in seen:
                continue
            seen.add(key)
            out.append(Relation(link.pred_uid, task.uid, link.type, link.lag_days))
    return out
