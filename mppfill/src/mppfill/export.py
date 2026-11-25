"""Write a pymppwriter project into a copy based on an MPP template."""

from __future__ import annotations

import os
import warnings
from pathlib import Path

from pymppwriter import MppWriter, Project, ScheduleWarning, validate

from .errors import MppFillError
from .limits import MAX_FILE_BYTES, OLE_MAGIC
from .template_fix import prepare_template, publish_schedule

DEFAULT_TEMPLATE = Path(os.environ.get("MPPFILL_TEMPLATE", r"D:\Drenthe\PLANNER-cyclus.mpp"))


def check_template(path: Path) -> None:
    _check_mpp(path, "template")


def check_output(path: Path, template: Path, source: Path | None, force: bool) -> None:
    if path.suffix.lower() != ".mpp":
        raise MppFillError(f"the output path must end in .mpp ({path})")
    try:
        dest = path.resolve()
        template_path = template.resolve()
        source_path = source.resolve() if source is not None else None
    except OSError as exc:
        raise MppFillError(f"cannot use {path}: {exc.strerror or exc}") from exc
    if dest == template_path:
        raise MppFillError("refusing to overwrite the template")
    if source_path is not None and dest == source_path:
        raise MppFillError("refusing to overwrite the imported file")
    if dest.exists() and not force:
        raise MppFillError(f"{path} already exists; pass --force to replace it")
    if dest.exists() and not dest.is_file():
        raise MppFillError(f"{path} is not a file")
    parent = dest.parent
    if not parent.is_dir():
        raise MppFillError(f"{parent} does not exist")


def write_project(project: Project, template: Path, dest: Path) -> list[str]:
    """Write `dest` from `template`. Returns schedule warnings. Does not replace an existing file itself."""
    check_template(template)
    try:
        validate(project)
    except ValueError as exc:
        raise MppFillError(str(exc)) from exc
    temporary = dest.with_name(dest.name + ".tmp")
    if temporary.exists():
        raise MppFillError(f"{temporary} is in the way; remove it and try again")
    caught: list[warnings.WarningMessage] = []
    try:
        writer = MppWriter(str(template))
        prepare_template(writer)
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always", ScheduleWarning)
            writer.write(project, str(temporary))
        publish_schedule(temporary)
        os.replace(temporary, dest)
    except MppFillError:
        _discard(temporary)
        raise
    except ValueError as exc:
        _discard(temporary)
        raise MppFillError(str(exc)) from exc
    except OSError as exc:
        _discard(temporary)
        raise MppFillError(f"cannot write {dest}: {exc.strerror or exc}") from exc
    except Exception as exc:
        _discard(temporary)
        raise MppFillError(f"the template could not be filled ({exc})") from exc
    return [str(item.message) for item in caught if issubclass(item.category, Warning)]


def _check_mpp(path: Path, role: str) -> None:
    try:
        info = path.stat()
    except OSError as exc:
        raise MppFillError(f"cannot read {role} {path}: {exc.strerror or exc}") from exc
    if not path.is_file():
        raise MppFillError(f"{role} {path} is not a file")
    if info.st_size > MAX_FILE_BYTES:
        raise MppFillError(f"{role} {path} is larger than {MAX_FILE_BYTES} bytes")
    if info.st_size < len(OLE_MAGIC):
        raise MppFillError(f"{role} {path} is not a Microsoft Project file")
    try:
        with path.open("rb") as handle:
            magic = handle.read(len(OLE_MAGIC))
    except OSError as exc:
        raise MppFillError(f"cannot read {role} {path}: {exc.strerror or exc}") from exc
    if magic != OLE_MAGIC:
        raise MppFillError(f"{role} {path} is not a Microsoft Project file")


def _discard(path: Path) -> None:
    try:
        path.unlink()
    except OSError:
        pass
