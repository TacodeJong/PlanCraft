"""Command line for filling an MPP template from Microsoft Project XML."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from pymppwriter import MppReadError, read_project

from .dialog import ask_mpp, ask_xml
from .errors import MppFillError
from .export import DEFAULT_TEMPLATE, check_output, check_template, write_project
from .limits import MAX_FILE_BYTES
from .map import apply_planner_profile, to_project
from .mspdi import load_path
from .planner import DEFAULT_APP, DEFAULT_CALENDAR, DEFAULT_ORG, push_project


def _counted(count: int, singular: str, plural: str) -> str:
    return f"{count} {singular if count == 1 else plural}"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="mppfill",
        description="Fill a Microsoft Project .mpp template from a Project XML file. "
        "Microsoft Project is not required. The template file is never modified.",
    )
    commands = parser.add_subparsers(dest="command")

    fill = commands.add_parser(
        "xml-to-mpp",
        help="read MSPDI XML and write an .mpp from the template. With no file, a dialog opens.",
    )
    fill.add_argument("xml", nargs="?", type=Path, help="Microsoft Project XML file. A dialog opens when omitted.")
    fill.add_argument("--out", type=Path, help="new .mpp file to write. A dialog opens when omitted.")
    fill.add_argument(
        "--template",
        type=Path,
        default=DEFAULT_TEMPLATE,
        help=f"MPP template (default: {DEFAULT_TEMPLATE})",
    )
    fill.add_argument("--force", action="store_true", help="replace --out if it already exists")
    fill.add_argument("--strict", action="store_true", help="exit with status 2 when a schedule warning is reported")

    inspect = commands.add_parser("inspect", help="print the tasks pymppwriter can read back from an .mpp")
    inspect.add_argument("mpp", type=Path)

    push = commands.add_parser(
        "push",
        help="write an .mpp from XML and import it into Planner Premium",
    )
    push.add_argument("xml", nargs="?", type=Path, help="Microsoft Project XML file. Omit when --mpp is set.")
    push.add_argument("--mpp", type=Path, help="import this .mpp and skip the XML conversion")
    push.add_argument("--out", type=Path, help="where to write the .mpp. Defaults to the XML name with an .mpp suffix.")
    push.add_argument("--template", type=Path, default=DEFAULT_TEMPLATE, help=f"MPP template (default: {DEFAULT_TEMPLATE})")
    push.add_argument("--force", action="store_true", help="replace --out if it already exists")
    push.add_argument("--name", help="project name in Planner. Defaults to the file name.")
    push.add_argument("--org", default=os.environ.get("MPPFILL_ORG", DEFAULT_ORG), help="Dynamics URL")
    push.add_argument(
        "--calendar",
        default=os.environ.get("MPPFILL_CALENDAR", DEFAULT_CALENDAR),
        help="work-hour template id. Default is Standaardwerksjabloon.",
    )
    push.add_argument("--app", default=DEFAULT_APP, help="Power App id, used only to print the link")

    args = parser.parse_args(argv)
    try:
        if args.command == "push":
            return _push(args)
        if args.command in (None, "xml-to-mpp"):
            if args.command is None:
                args.xml = None
                args.out = None
                args.template = DEFAULT_TEMPLATE
                args.force = False
                args.strict = False
            return _fill(args)
        return _inspect(args.mpp)
    except MppFillError as exc:
        print(f"mppfill: {exc}", file=sys.stderr)
        return 1
    except MppReadError as exc:
        print(f"mppfill: {exc}", file=sys.stderr)
        return 1


def _fill(args: argparse.Namespace) -> int:
    source = args.xml if args.xml is not None else ask_xml()
    if source is None:
        print("Cancelled.")
        return 0
    picked_output = args.out is None
    dest = args.out if args.out is not None else ask_mpp(source.with_suffix(".mpp"))
    if dest is None:
        print("Cancelled.")
        return 0
    check_template(args.template)
    check_output(dest, args.template, source, args.force or picked_output)
    args.out = dest
    plan = load_path(source)
    project, notes = to_project(plan)
    warnings = write_project(project, args.template, args.out)
    print(f"Wrote {args.out}")
    print(
        ", ".join(
            (
                _counted(len(project.tasks), "task", "tasks"),
                _counted(len(project.relations), "dependency", "dependencies"),
                _counted(len(project.resources), "resource", "resources"),
            )
        )
    )
    if notes:
        print("Not written:")
        for note in notes:
            print(f"- {note}")
    if warnings:
        print("Schedule warnings:")
        for warning in warnings:
            print(f"- {warning}")
    if warnings and args.strict:
        return 2
    return 0


def _push(args: argparse.Namespace) -> int:
    if args.xml is None and args.mpp is None:
        raise MppFillError("pass an XML file, or pass --mpp")
    if args.xml is not None:
        dest = args.out if args.out is not None else args.xml.with_suffix(".mpp")
        check_template(args.template)
        check_output(dest, args.template, args.xml, args.force)
        plan = load_path(args.xml)
        project, notes = to_project(plan)
        profile = apply_planner_profile(project.tasks)
        warnings = write_project(project, args.template, dest)
        print(f"Wrote {dest}")
        if profile:
            print("Planner profile:")
            for note in profile:
                print(f"- {note}")
        if notes:
            print("Not written:")
            for note in notes:
                print(f"- {note}")
        if warnings:
            print("Schedule warnings:")
            for warning in warnings:
                print(f"- {warning}")
        mpp = dest
    else:
        mpp = args.mpp
    name = (args.name or mpp.stem).strip()
    if not name:
        raise MppFillError("the project name is empty")
    push_project(mpp, args.org, args.calendar, name, args.app)
    return 0


def _inspect(path: Path) -> int:
    if not path.is_file():
        raise MppFillError(f"{path} is not a file")
    if path.stat().st_size > MAX_FILE_BYTES:
        raise MppFillError(f"{path} is larger than {MAX_FILE_BYTES} bytes")
    project = read_project(str(path))
    print(project.title)
    print(f"start {project.start:%Y-%m-%d %H:%M}")
    print(
        ", ".join(
            (
                _counted(len(project.tasks), "task", "tasks"),
                _counted(len(project.relations), "dependency", "dependencies"),
                _counted(len(project.resources), "resource", "resources"),
            )
        )
    )
    for task in project.tasks:
        print(
            f"{task.uid}\tL{task.outline_level}\t{task.percent_complete}%\t"
            f"{task.duration_days}d\t{task.start:%Y-%m-%d %H:%M}\t{task.name}"
        )
    for link in project.relations:
        print(f"link {link.pred_uid} -> {link.succ_uid} {link.type} lag {link.lag_days}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
