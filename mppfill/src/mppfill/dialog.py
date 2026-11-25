"""Windows file dialogs for choosing the XML source and the .mpp to write."""

from __future__ import annotations

from pathlib import Path


def ask_xml(start: Path | None = None) -> Path | None:
    """Open a file dialog for a Microsoft Project XML file. None means cancel."""
    folder = _start(start)
    chosen = _ask(
        save=False,
        title="Select Microsoft Project XML",
        filetypes=[("Microsoft Project XML", "*.xml"), ("All files", "*.*")],
        initialdir=folder,
        initialfile="",
    )
    return Path(chosen) if chosen else None


def ask_mpp(suggested: Path) -> Path | None:
    """Open a save dialog for the .mpp file. None means cancel."""
    chosen = _ask(
        save=True,
        title="Save Microsoft Project file",
        filetypes=[("Microsoft Project", "*.mpp")],
        initialdir=str(suggested.parent) if suggested.parent else None,
        initialfile=suggested.name,
    )
    if not chosen:
        return None
    path = Path(chosen)
    if path.suffix.lower() != ".mpp":
        path = path.with_suffix(".mpp")
    return path


def _start(start: Path | None) -> str | None:
    if start is not None and start.is_dir():
        return str(start)
    drenthe = Path(r"D:\Drenthe")
    if drenthe.is_dir():
        return str(drenthe)
    return None


def _ask(save: bool, title: str, filetypes: list[tuple[str, str]], initialdir: str | None, initialfile: str) -> str:
    import tkinter as tk
    from tkinter import filedialog

    root = tk.Tk()
    root.withdraw()
    try:
        root.attributes("-topmost", True)
    except tk.TclError:
        pass
    root.update()
    options = {
        "title": title,
        "filetypes": filetypes,
        "parent": root,
    }
    if initialdir:
        options["initialdir"] = initialdir
    if save:
        options["defaultextension"] = ".mpp"
        options["initialfile"] = initialfile
        chosen = filedialog.asksaveasfilename(**options)
    else:
        chosen = filedialog.askopenfilename(**options)
    root.destroy()
    return chosen
