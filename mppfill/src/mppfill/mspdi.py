"""Read Microsoft Project XML (MSPDI) into plain records.

The element names and code numbers follow Microsoft's published Project XML
Data Interchange schema. This module does not import pymppwriter.
"""

from __future__ import annotations

import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from datetime import date, datetime
from pathlib import Path

from .errors import MppFillError
from .limits import MAX_EXCEPTIONS, MAX_EXCEPTION_SPAN_DAYS, MAX_FILE_BYTES, MAX_LINKS, MAX_RESOURCES, MAX_TASKS


def _note(notes: list[str], message: str) -> None:
    if message not in notes:
        notes.append(message)


@dataclass
class XmlLink:
    pred_uid: int
    type: str
    lag_days: float


@dataclass
class XmlBaseline:
    start: datetime | None
    finish: datetime | None
    duration_days: float
    work_hours: float


@dataclass
class XmlTask:
    uid: int
    row_id: int
    name: str
    outline_level: int
    start: datetime | None
    finish: datetime | None
    duration_days: float
    duration_units: str
    estimated: bool
    milestone: bool
    notes: str
    wbs: str | None
    priority: int
    percent_complete: int
    constraint: str | None
    constraint_date: datetime | None
    deadline: datetime | None
    manual: bool
    task_type: str
    effort_driven: bool
    calendar_uid: int | None
    guid: bytes | None
    parent_uid: int = 0
    text: dict[int, str] = field(default_factory=dict)
    number: dict[int, float] = field(default_factory=dict)
    date_fields: dict[int, datetime] = field(default_factory=dict)
    flag: dict[int, bool] = field(default_factory=dict)
    baselines: dict[int, XmlBaseline] = field(default_factory=dict)
    links: list[XmlLink] = field(default_factory=list)


@dataclass
class XmlResource:
    uid: int
    name: str
    initials: str
    email: str
    max_units: float
    guid: bytes | None


@dataclass
class XmlAssignment:
    task_uid: int
    resource_uid: int
    units: float


@dataclass
class XmlException:
    start: date
    finish: date
    name: str


@dataclass
class XmlCalendar:
    uid: int
    name: str
    is_base: bool
    week: dict[int, list[tuple[int, int]] | None] = field(default_factory=dict)
    exceptions: list[XmlException] = field(default_factory=list)


@dataclass
class XmlPlan:
    title: str
    start: datetime | None
    status_date: datetime | None
    author: str | None
    manager: str | None
    company: str | None
    currency_symbol: str | None
    currency_code: str | None
    minutes_per_day: int
    calendar_uid: int | None
    calendars: list[XmlCalendar]
    tasks: list[XmlTask]
    resources: list[XmlResource]
    assignments: list[XmlAssignment]
    notes: list[str]


# PjField ids for task custom columns, Text1.. and so on. Published enumeration.
_TEXT = (
    188743731, 188743734, 188743737, 188743740, 188743743, 188743746, 188743747, 188743748, 188743749, 188743750,
    188743997, 188743998, 188743999, 188744000, 188744001, 188744002, 188744003, 188744004, 188744005, 188744006,
    188744007, 188744008, 188744009, 188744010, 188744011, 188744012, 188744013, 188744014, 188744015, 188744016,
)
_NUMBER = (
    188743767, 188743768, 188743769, 188743770, 188743771, 188743982, 188743983, 188743984, 188743985, 188743986,
    188743987, 188743988, 188743989, 188743990, 188743991, 188743992, 188743993, 188743994, 188743995, 188743996,
)
_COST = (188743786, 188743787, 188743788, 188743938, 188743939, 188743940, 188743941, 188743942, 188743943, 188743944)
_DATE = (188743945, 188743946, 188743947, 188743948, 188743949, 188743950, 188743951, 188743952, 188743953, 188743954)
_DURATION = (188743783, 188743784, 188743785, 188743955, 188743956, 188743957, 188743958, 188743959, 188743960, 188743961)
_FLAG = (
    188743752, 188743753, 188743754, 188743755, 188743756, 188743757, 188743758, 188743759, 188743760, 188743761,
    188743972, 188743973, 188743974, 188743975, 188743976, 188743977, 188743978, 188743979, 188743980, 188743981,
)
_INTERIM = {
    188743732, 188743735, 188743738, 188743741, 188743744, 188743962, 188743964, 188743966, 188743968, 188743970,
    188743733, 188743736, 188743739, 188743742, 188743745, 188743963, 188743965, 188743967, 188743969, 188743971,
}
_SLOT = {fid: ("text", i + 1) for i, fid in enumerate(_TEXT)}
_SLOT.update({fid: ("number", i + 1) for i, fid in enumerate(_NUMBER)})
_SLOT.update({fid: ("date", i + 1) for i, fid in enumerate(_DATE)})
_SLOT.update({fid: ("flag", i + 1) for i, fid in enumerate(_FLAG)})

_CONSTRAINTS = {0: None, 1: "ALAP", 2: "MSO", 3: "MFO", 4: "SNET", 5: "SNLT", 6: "FNET", 7: "FNLT"}
_NEEDS_DATE = {"MSO", "MFO", "SNET", "SNLT", "FNET", "FNLT"}
_TASK_TYPES = {0: "fixed_units", 1: "fixed_duration", 2: "fixed_work"}
_LINK_TYPES = {0: "FF", 1: "FS", 2: "SF", 3: "SS"}
_UNITS = {3: "m", 4: "m", 5: "h", 6: "h", 7: "d", 8: "d", 9: "w", 10: "w", 11: "mo", 12: "mo"}


def load_path(path: Path) -> XmlPlan:
    """Read an MSPDI file. Refuses oversized files and documents with a DOCTYPE."""
    try:
        size = path.stat().st_size
    except OSError as exc:
        raise MppFillError(f"cannot read {path}: {exc.strerror or exc}") from exc
    if not path.is_file():
        raise MppFillError(f"{path} is not a file")
    if size > MAX_FILE_BYTES:
        raise MppFillError(f"{path} is larger than {MAX_FILE_BYTES} bytes")
    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise MppFillError(f"cannot read {path}: {exc.strerror or exc}") from exc
    return load_bytes(raw, path.name)


def load_bytes(raw: bytes, name: str = "plan.xml") -> XmlPlan:
    if len(raw) > MAX_FILE_BYTES:
        raise MppFillError(f"{name} is larger than {MAX_FILE_BYTES} bytes")
    if raw.startswith(b"\xff\xfe") or raw.startswith(b"\xfe\xff"):
        text = raw.decode("utf-16")
    else:
        text = raw.decode("utf-8-sig")
    head = text.lstrip("\ufeff")[:4096].lower()
    if "<!doctype" in head or "<!entity" in head:
        raise MppFillError(f"{name} contains a document type or entity declaration and was refused")
    try:
        root = ET.fromstring(text)
    except ET.ParseError as exc:
        raise MppFillError(f"{name} is not well-formed XML ({exc})") from exc
    if _local(root.tag) != "Project":
        raise MppFillError(f"{name} is not a Microsoft Project XML file (root element is {_local(root.tag)})")
    return _project(root)


def _project(root: ET.Element) -> XmlPlan:
    notes: list[str] = []
    minutes = _int(_text(root, "MinutesPerDay")) or 480
    if minutes <= 0:
        _note(notes, "MinutesPerDay was not a positive number; 480 was used")
        minutes = 480
    plan = XmlPlan(
        title=_text(root, "Title") or _text(root, "Name") or "Project",
        start=_when(_text(root, "StartDate")),
        status_date=_when(_text(root, "StatusDate")),
        author=_text(root, "Author") or None,
        manager=_text(root, "Manager") or None,
        company=_text(root, "Company") or None,
        currency_symbol=_text(root, "CurrencySymbol") or None,
        currency_code=_text(root, "CurrencyCode") or None,
        minutes_per_day=minutes,
        calendar_uid=_int(_text(root, "CalendarUID")),
        calendars=[],
        tasks=[],
        resources=[],
        assignments=[],
        notes=notes,
    )
    calendars = _child(root, "Calendars")
    if calendars is not None:
        for node in _children(calendars, "Calendar"):
            plan.calendars.append(_calendar(node, minutes, notes))
    tasks = _child(root, "Tasks")
    if tasks is not None:
        for node in _children(tasks, "Task"):
            task = _task(node, plan, notes)
            if task is not None:
                plan.tasks.append(task)
    if len(plan.tasks) > MAX_TASKS:
        raise MppFillError(f"the file has more than {MAX_TASKS} tasks")
    resources = _child(root, "Resources")
    if resources is not None:
        for node in _children(resources, "Resource"):
            resource = _resource(node, notes)
            if resource is not None:
                plan.resources.append(resource)
    if len(plan.resources) > MAX_RESOURCES:
        raise MppFillError(f"the file has more than {MAX_RESOURCES} resources")
    assignments = _child(root, "Assignments")
    if assignments is not None:
        for node in _children(assignments, "Assignment"):
            assignment = _assignment(node, notes)
            if assignment is not None:
                plan.assignments.append(assignment)
    if any(node.tag.endswith("TimephasedData") or _local(node.tag) == "TimephasedData" for node in root.iter()):
        _note(notes, "timephased data was left out")
    plan.tasks.sort(key=lambda t: (t.row_id, t.uid))
    return plan


def _task(node: ET.Element, plan: XmlPlan, notes: list[str]) -> XmlTask | None:
    if _flag(_text(node, "IsNull")):
        return None
    uid = _int(_text(node, "UID"))
    if uid is None or uid <= 0:
        return None
    outline = _int(_text(node, "OutlineLevel")) or 1
    if outline <= 0:
        return None
    if _text(node, "Active") is not None and not _flag(_text(node, "Active")):
        _note(notes, "inactive tasks were written as active; pymppwriter has no inactive flag")
    start = _when(_text(node, "Start"))
    finish = _when(_text(node, "Finish"))
    minutes, _rounded = _duration(_text(node, "Duration"))
    units, elapsed, estimated_fmt = _duration_format(_int(_text(node, "DurationFormat")))
    if elapsed:
        _note(notes, "elapsed durations were written as working time of the same length")
    milestone = _flag(_text(node, "Milestone"))
    if milestone or (minutes == 0 and _text(node, "Duration") is not None):
        duration_days = 0.0
        milestone = True
    elif minutes is None:
        duration_days = 1.0
        if start is not None and finish is not None and finish >= start:
            span = (finish - start).total_seconds() / 60.0
            duration_days = round(span / plan.minutes_per_day, 4)
            _note(notes, "a task had no duration; working days were estimated from its start and finish")
    else:
        duration_days = round(minutes / plan.minutes_per_day, 4)
    constraint = _CONSTRAINTS.get(_int(_text(node, "ConstraintType")) if _text(node, "ConstraintType") else 0, None)
    if _text(node, "ConstraintType") and _int(_text(node, "ConstraintType")) not in _CONSTRAINTS:
        _note(notes, f"task {uid} has an unknown constraint type; it was left out")
        constraint = None
    constraint_date = _when(_text(node, "ConstraintDate"))
    if constraint in _NEEDS_DATE and constraint_date is None:
        _note(notes, f"task {uid} constraint {constraint} had no date and was left out")
        constraint = None
    percent = _int(_text(node, "PercentComplete")) or 0
    if percent < 0 or percent > 100:
        _note(notes, f"task {uid} percent complete was outside 0..100 and was clamped")
        percent = min(100, max(0, percent))
    priority = _int(_text(node, "Priority"))
    if priority is None:
        priority = 500
    elif priority < 0 or priority > 1000:
        _note(notes, f"task {uid} priority was outside 0..1000 and was clamped")
        priority = min(1000, max(0, priority))
    task_type = _TASK_TYPES.get(_int(_text(node, "Type")) if _text(node, "Type") else 0, "fixed_units")
    if _text(node, "Type") and _int(_text(node, "Type")) not in _TASK_TYPES:
        _note(notes, f"task {uid} has an unknown task type; fixed units was used")
    notes_text = _text(node, "Notes") or ""
    if notes_text.startswith("{\\rtf") or notes_text.lstrip().startswith("<"):
        _note(notes, "task notes were written as plain text; rich text and HTML formatting is not kept")
    task = XmlTask(
        uid=uid,
        row_id=_int(_text(node, "ID")) or uid,
        name=_text(node, "Name") or "",
        outline_level=outline,
        start=start,
        finish=finish,
        duration_days=duration_days,
        duration_units=units,
        estimated=estimated_fmt or _flag(_text(node, "Estimated")),
        milestone=milestone,
        notes=notes_text,
        wbs=_text(node, "WBS") or None,
        priority=priority,
        percent_complete=percent,
        constraint=constraint,
        constraint_date=constraint_date,
        deadline=_when(_text(node, "Deadline")),
        manual=_flag(_text(node, "Manual")),
        task_type=task_type,
        effort_driven=_flag(_text(node, "EffortDriven")),
        calendar_uid=_int(_text(node, "CalendarUID")),
        guid=_guid(_text(node, "GUID"), uid, notes),
    )
    for link_node in _children(node, "PredecessorLink"):
        link = _link(link_node, plan.minutes_per_day, uid, notes)
        if link is not None:
            task.links.append(link)
    if sum(len(t.links) for t in (*plan.tasks, task)) > MAX_LINKS:
        raise MppFillError(f"the file has more than {MAX_LINKS} dependencies")
    _custom(node, task, notes)
    _baselines(node, task, plan.minutes_per_day, notes)
    return task


def _link(node: ET.Element, minutes_per_day: int, succ: int, notes: list[str]) -> XmlLink | None:
    if _flag(_text(node, "CrossProject")):
        _note(notes, "links to tasks in other projects were left out")
        return None
    pred = _int(_text(node, "PredecessorUID"))
    if pred is None or pred <= 0:
        _note(notes, f"task {succ} has a predecessor link without a task id; it was left out")
        return None
    code = _int(_text(node, "Type"))
    kind = _LINK_TYPES.get(1 if code is None else code)
    if kind is None:
        _note(notes, f"task {succ} has a predecessor link of unknown type; Finish-to-Start was used")
        kind = "FS"
    lag_format = _int(_text(node, "LagFormat"))
    if lag_format in (19, 20):
        _note(notes, "percent lags were left out; those links were written with no lag")
        return XmlLink(pred, kind, 0.0)
    elapsed = lag_format in (4, 6, 8, 10, 12)
    lag_tenths = _int(_text(node, "LinkLag")) or 0
    lag_minutes = lag_tenths / 10.0
    if elapsed:
        _note(notes, "elapsed link lag was written as working time of the same length")
    return XmlLink(pred, kind, round(lag_minutes / minutes_per_day, 4))


def _custom(node: ET.Element, task: XmlTask, notes: list[str]) -> None:
    saw_cost = saw_duration = saw_interim = False
    for attr in _children(node, "ExtendedAttribute"):
        fid = _int(_text(attr, "FieldID"))
        if fid is None:
            continue
        value = _text(attr, "Value")
        if value is None:
            continue
        if fid in _COST:
            saw_cost = True
            continue
        if fid in _DURATION:
            saw_duration = True
            continue
        if fid in _INTERIM:
            saw_interim = True
            continue
        slot = _SLOT.get(fid)
        if slot is None:
            _note(notes, f"extended attribute {fid} on task {task.uid} is not a supported custom field and was left out")
            continue
        kind, number = slot
        if kind == "text":
            task.text[number] = value
        elif kind == "number":
            number_value = _float(value)
            if number_value is None:
                _note(notes, f"task {task.uid} Number{number} was not a number and was left out")
            else:
                task.number[number] = number_value
        elif kind == "date":
            when = _when(value)
            if when is None:
                _note(notes, f"task {task.uid} Date{number} was not a date and was left out")
            else:
                task.date_fields[number] = when
        else:
            task.flag[number] = _flag(value)
    if saw_cost:
        _note(notes, "custom cost fields were left out; pymppwriter writes text, number, date and flag fields")
    if saw_duration:
        _note(notes, "custom duration fields were left out; pymppwriter writes text, number, date and flag fields")
    if saw_interim:
        _note(notes, "interim plans (Start1/Finish1 and the rest) were left out")


def _baselines(node: ET.Element, task: XmlTask, minutes_per_day: int, notes: list[str]) -> None:
    saw_cost = False
    for base in _children(node, "Baseline"):
        if _flag(_text(base, "Interim")):
            _note(notes, "interim baseline rows were left out")
            continue
        number = _int(_text(base, "Number"))
        if number is None or not 0 <= number <= 10:
            _note(notes, f"task {task.uid} has a baseline outside slots 0..10; it was left out")
            continue
        minutes, _ = _duration(_text(base, "Duration"))
        work, _ = _duration(_text(base, "Work"))
        if _text(base, "Cost") not in (None, "", "0", "0.0"):
            saw_cost = True
        task.baselines[number] = XmlBaseline(
            start=_when(_text(base, "Start")),
            finish=_when(_text(base, "Finish")),
            duration_days=round((minutes or 0) / minutes_per_day, 4),
            work_hours=round((work or 0) / 60.0, 4),
        )
    if saw_cost:
        _note(notes, "baseline cost was left out; pymppwriter 0.4.3 has not established the cost scale")


def _resource(node: ET.Element, notes: list[str]) -> XmlResource | None:
    if _flag(_text(node, "IsNull")):
        return None
    uid = _int(_text(node, "UID"))
    if uid is None or uid <= 0:
        return None
    kind = _int(_text(node, "Type"))
    if _flag(_text(node, "IsCostResource")) or kind == 2:
        _note(notes, "cost resources were left out")
        return None
    if kind == 0:
        _note(notes, "material resources were left out")
        return None
    if _text(node, "StandardRate") not in (None, "", "0", "0.0"):
        _note(notes, "resource rates and costs were left out")
    if _child(node, "AvailabilityPeriods") is not None:
        _note(notes, "resource availability periods were left out")
    units = _float(_text(node, "MaxUnits"))
    if units is None:
        units = 1.0
    elif units > 10:
        units = units / 100.0
        _note(notes, "resource max units above 10 were read as a percentage")
    return XmlResource(
        uid=uid,
        name=_text(node, "Name") or "",
        initials=_text(node, "Initials") or "",
        email=_text(node, "EmailAddress") or "",
        max_units=units,
        guid=_guid(_text(node, "GUID"), uid, notes),
    )


def _assignment(node: ET.Element, notes: list[str]) -> XmlAssignment | None:
    task = _int(_text(node, "TaskUID"))
    resource = _int(_text(node, "ResourceUID"))
    if task is None or resource is None or task <= 0 or resource <= 0:
        return None
    if _text(node, "WorkContour") not in (None, "", "0"):
        _note(notes, "work contours were left out; assignments are flat")
    if _text(node, "ActualWork") not in (None, "", "PT0H0M0S"):
        _note(notes, "assignment actual work was left out; progress is taken from the task percent complete")
    units = _float(_text(node, "Units"))
    if units is None:
        units = 1.0
    elif units > 10:
        units = units / 100.0
        _note(notes, "assignment units above 10 were read as a percentage")
    return XmlAssignment(task, resource, units)


def _calendar(node: ET.Element, minutes_per_day: int, notes: list[str]) -> XmlCalendar:
    del minutes_per_day
    uid = _int(_text(node, "UID")) or 0
    is_base = _flag(_text(node, "IsBaseCalendar")) if _text(node, "IsBaseCalendar") is not None else True
    cal = XmlCalendar(uid=uid, name=_text(node, "Name") or f"Calendar {uid}", is_base=is_base)
    if not is_base:
        _note(notes, "resource calendars were left out; pymppwriter gives each resource a copy of the base calendar")
        return cal
    week = _child(node, "WeekDays")
    if week is not None:
        for day in _children(week, "WeekDay"):
            _weekday(day, cal, notes)
    if _child(node, "WorkWeeks") is not None:
        _note(notes, "dated work weeks were left out; pymppwriter exceptions are whole non-working days")
    exceptions = _child(node, "Exceptions")
    if exceptions is not None:
        for ex in _children(exceptions, "Exception"):
            _exception(ex, cal, notes)
    return cal


def _weekday(node: ET.Element, cal: XmlCalendar, notes: list[str]) -> None:
    day_type = _int(_text(node, "DayType"))
    if day_type == 0:
        period = _child(node, "TimePeriod")
        if period is None:
            return
        start = _day(_text(period, "FromDate"))
        finish = _day(_text(period, "ToDate")) or start
        if start is None or finish is None:
            return
        if _working(node):
            _note(notes, "working-time calendar exceptions were left out")
            return
        _add_exception(cal, start, finish, "Exception", notes)
        return
    if day_type is None:
        return
    weekday = (day_type - 2) % 7 if 1 <= day_type <= 7 else None
    if weekday is None:
        return
    if _text(node, "DayWorking") is not None and not _flag(_text(node, "DayWorking")):
        cal.week[weekday] = None
        return
    ranges = _ranges(node)
    if ranges:
        cal.week[weekday] = ranges


def _exception(node: ET.Element, cal: XmlCalendar, notes: list[str]) -> None:
    period = _child(node, "TimePeriod")
    if period is None:
        return
    start = _day(_text(period, "FromDate"))
    finish = _day(_text(period, "ToDate")) or start
    if start is None or finish is None:
        return
    if finish < start:
        start, finish = finish, start
    name = _text(node, "Name") or "Exception"
    if _working(node):
        _note(notes, "working-time calendar exceptions were left out")
        return
    kind = _int(_text(node, "Type")) or 1
    if kind == 2:
        month0 = _int(_text(node, "Month"))
        day = _int(_text(node, "MonthDay")) or start.day
        month = (month0 + 1) if month0 is not None and 0 <= month0 <= 11 else start.month
        year = start.year
        added = 0
        while year <= finish.year and added < 40:
            try:
                on = date(year, month, day)
            except ValueError:
                year += 1
                continue
            if start <= on <= finish:
                _add_exception(cal, on, on, name, notes)
                added += 1
            year += 1
        if year <= finish.year:
            _note(notes, f"calendar exception {name!r} repeats for more than 40 years; later years were left out")
        return
    if kind not in (1, 7, 8, 9) or (_int(_text(node, "Period")) not in (None, 1)):
        _note(notes, f"recurring calendar exception {name!r} was left out")
        return
    if (finish - start).days > MAX_EXCEPTION_SPAN_DAYS:
        _note(notes, f"calendar exception {name!r} is longer than {MAX_EXCEPTION_SPAN_DAYS} days and was left out")
        return
    _add_exception(cal, start, finish, name, notes)


def _add_exception(cal: XmlCalendar, start: date, finish: date, name: str, notes: list[str]) -> None:
    if len(cal.exceptions) >= MAX_EXCEPTIONS:
        _note(notes, "a calendar has more exceptions than the import keeps; later ones were left out")
        return
    cal.exceptions.append(XmlException(start, finish, name))


def _working(node: ET.Element) -> bool:
    if _flag(_text(node, "DayWorking")):
        return True
    times = _child(node, "WorkingTimes")
    return times is not None and any(_local(child.tag) == "WorkingTime" for child in list(times))


def _ranges(node: ET.Element) -> list[tuple[int, int]]:
    times = _child(node, "WorkingTimes")
    if times is None:
        return []
    out = []
    for slot in _children(times, "WorkingTime"):
        start = _clock(_text(slot, "FromTime"))
        end = _clock(_text(slot, "ToTime"))
        if start is None or end is None or end <= start:
            continue
        out.append((start, end))
    return out


def _duration(value: str | None) -> tuple[int | None, bool]:
    """xsd:duration in whole minutes, and whether a fraction of a minute was rounded."""
    if not value:
        return None, False
    text = value.strip()
    neg = text.startswith("-")
    if neg:
        text = text[1:]
    if not text.startswith("P"):
        return None, False
    rest = text[1:]
    number = ""
    in_time = False
    minutes = 0.0
    any_unit = False
    for ch in rest:
        if ch == "T" and not number and not in_time:
            in_time = True
            continue
        if ch.isdigit() or ch == ".":
            number += ch
            continue
        if not number:
            return None, False
        try:
            qty = float(number)
        except ValueError:
            return None, False
        number = ""
        any_unit = True
        factor = {
            ("Y", False): 365.0 * 1440.0,
            ("M", False): 30.0 * 1440.0,
            ("W", False): 7.0 * 1440.0,
            ("D", False): 1440.0,
            ("H", True): 60.0,
            ("M", True): 1.0,
            ("S", True): 1.0 / 60.0,
        }.get((ch, in_time))
        if factor is None or not _finite(qty):
            return None, False
        minutes += qty * factor
    if number or not any_unit or not _finite(minutes):
        return None, False
    signed = -minutes if neg else minutes
    rounded = int(round(signed))
    return rounded, abs(signed - rounded) > 1e-6


def _duration_format(code: int | None) -> tuple[str, bool, bool]:
    if code is None:
        return "d", False, False
    estimated = 35 <= code <= 52
    base = code - 32 if estimated else code
    elapsed = base in (4, 6, 8, 10, 12)
    return _UNITS.get(base, "d"), elapsed, estimated


def _when(value: str | None) -> datetime | None:
    if not value:
        return None
    text = value.strip()
    if "T" in text:
        day, time = text.split("T", 1)
    else:
        day, time = text, ""
    try:
        year, month, dom = (int(p) for p in day.split("-", 2))
        stamp = datetime(year, month, dom)
    except (TypeError, ValueError):
        return None
    if not time:
        return stamp
    time = time.rstrip("Z")
    for sign in ("+", "-"):
        if sign in time[1:]:
            time = time.split(sign, 1)[0]
            break
    time = time.split(".", 1)[0]
    parts = time.split(":")
    try:
        hour = int(parts[0]) if parts and parts[0] else 0
        minute = int(parts[1]) if len(parts) > 1 and parts[1] else 0
        second = int(parts[2]) if len(parts) > 2 and parts[2] else 0
    except ValueError:
        return None
    if hour == 24 and minute == 0 and second == 0:
        hour, minute, second = 23, 59, 59
    if not (0 <= hour <= 23 and 0 <= minute <= 59 and 0 <= second <= 59):
        return None
    return stamp.replace(hour=hour, minute=minute, second=second)


def _day(value: str | None) -> date | None:
    stamp = _when(value)
    return stamp.date() if stamp else None


def _clock(value: str | None) -> int | None:
    if not value:
        return None
    text = value.strip().split(".", 1)[0]
    parts = text.split(":")
    try:
        hour = int(parts[0])
        minute = int(parts[1]) if len(parts) > 1 else 0
        second = int(parts[2]) if len(parts) > 2 else 0
    except ValueError:
        return None
    if hour == 24 and minute == 0 and second == 0:
        return 1440
    if not (0 <= hour <= 23 and 0 <= minute <= 59 and 0 <= second <= 59):
        return None
    return hour * 60 + minute


def _guid(value: str | None, uid: int, notes: list[str]) -> bytes | None:
    if not value:
        return None
    import uuid

    try:
        return uuid.UUID(value.strip().strip("{}")).bytes_le
    except ValueError:
        _note(notes, f"item {uid} has a GUID that is not a UUID; a new one will be assigned on export")
        return None


def _local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def _child(node: ET.Element, name: str) -> ET.Element | None:
    for child in list(node):
        if _local(child.tag) == name:
            return child
    return None


def _children(node: ET.Element, name: str):
    return [child for child in list(node) if _local(child.tag) == name]


def _text(node: ET.Element, name: str) -> str | None:
    child = _child(node, name)
    if child is None or child.text is None:
        return None
    return child.text.strip()


def _int(value: str | None) -> int | None:
    if value is None or value == "":
        return None
    try:
        return int(value)
    except ValueError:
        try:
            number = float(value)
        except ValueError:
            return None
        if number != int(number):
            return None
        return int(number)


def _float(value: str | None) -> float | None:
    if value is None or value == "":
        return None
    try:
        number = float(value)
    except ValueError:
        return None
    if not _finite(number):
        return None
    return number


def _flag(value: str | None) -> bool:
    return value is not None and value.strip().lower() in {"1", "true"}


def _finite(number: float) -> bool:
    return number == number and number not in (float("inf"), float("-inf"))
