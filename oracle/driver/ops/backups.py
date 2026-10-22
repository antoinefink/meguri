# SPDX-License-Identifier: AGPL-3.0-or-later
"""Backups (specs/conformance/backup-ops.md F1-F7) and the backup trigger op
(specs/conformance/backup-trigger-ops.md H1).

The scenario's backups folder is <session dir>/backups (F1): the first op that
needs it creates it, and Session.hand_off copies it with the session directory.

add_backup_files {names: [name]} | {series: {from, until, every}} -> none.
    Empty regular files in the folder (F2). A series names the backups of the
    wall times from, from + every, ... up to and including until, stepped as
    naive calendar arithmetic (no zone rule). The name rules, repeats within the
    step, more than 20,000 files and until before from are invalid_scenario
    before any collection; a name already in the folder is invalid_scenario at
    the step, before any file is created.
force_backup {} | {copy_as} -> {name} (F3). Anki's collection backup into the
    folder with force on, waiting for its thread (write, then thinning). name
    is the one regular file the call added (listings before and after); copy_as
    then copies it to the session directory, replacing a file of that name. A
    backup Anki skips (create_backup false: nothing changed since this
    opening's last backup) is anki_error. A call that added no file overwrote a
    seeded name, which F7 forbids: invalid_scenario.
save_backup_settings {daily, weekly, monthly, minimum_interval_mins} -> none
    (F4). Anki's preferences save carrying only the backup group.
backup {force: bool} -> {name | null} (backup-trigger-ops.md H1). Anki's
    collection backup into the folder with that force flag, waiting for its
    thread. name is the one regular file the call added, null when Anki made no
    backup (create_backup false: unchanged since this opening's last backup or,
    unforced, an entry younger than the minimum interval). A backup Anki reports
    made with no file added replaced a file of its name, which H3 forbids:
    invalid_scenario.
backup_files (observation) -> [name] (F5): the folder's regular files, sorted
    by UTF-8 bytes; [] while no op has created the folder.
backup_settings (observation) -> {daily, weekly, monthly, minimum_interval_mins}
    (F6): the backup group of Anki's preferences.

F7 allows one force_backup until the next collection load (start, open,
reopen, import, handoff), checked before any collection; backup steps do not
count (backup-trigger-ops.md H3).
"""

from __future__ import annotations

import os
import re
import shutil
from datetime import datetime, timedelta
from typing import Any

from anki.collection import Preferences

from ..clock import parse_duration_secs
from ..errors import DriverError, invalid
from ..registry import observation, op, scenario_check
from . import colpkg

FOLDER = "backups"
MAX_FILES = 20_000
MAX_NAME_BYTES = 255
U32_MAX = 4_294_967_295
LIMITS = ("daily", "weekly", "monthly", "minimum_interval_mins")
SERIES_KEYS = {"from", "until", "every"}
_RESET_STEPS = {"open", "reopen", "import", "handoff"}
# conformance/schema/scenario.schema.json wall_time; fullmatch.
_WALL_TIME = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}")


def folder_path(session) -> str:
    return os.path.join(session.dir, FOLDER)


def _folder(session) -> str:
    path = folder_path(session)
    os.makedirs(path, exist_ok=True)
    return path


def _regular_files(path: str) -> list[str]:
    """The regular files directly in path (no directory, link or other entry)."""
    with os.scandir(path) as entries:
        return [e.name for e in entries if e.is_file(follow_symlinks=False)]


def _file_name(value: Any) -> str:
    if not isinstance(value, str) or not value:
        raise invalid("a backup file name is a non-empty string")
    try:
        size = len(value.encode("utf-8"))
    except UnicodeEncodeError:
        raise invalid("a backup file name is valid UTF-8") from None
    if size > MAX_NAME_BYTES:
        raise invalid(f"a backup file name has at most {MAX_NAME_BYTES} bytes of UTF-8")
    if "/" in value or "\x00" in value or value in (".", ".."):
        raise invalid("a backup file name has no / or NUL and is not . or ..")
    if value.startswith(".meguri-"):
        raise invalid("a backup file name does not begin with .meguri- (U32's unpublished work)")
    return value


def _wall_time(value: Any, what: str) -> datetime:
    if not isinstance(value, str) or not _WALL_TIME.fullmatch(value):
        raise invalid(f"series {what} is a wall time YYYY-MM-DDTHH:MM:SS with no offset")
    try:
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%S")
    except ValueError:
        raise invalid(f"series {what} {value!r} is not a calendar date and time") from None


def backup_name(wall: datetime) -> str:
    """specs/package/backups.md B2.1 for a wall time."""
    return (
        f"backup-{wall.year:04d}-{wall.month:02d}-{wall.day:02d}-"
        f"{wall.hour:02d}.{wall.minute:02d}.{wall.second:02d}.colpkg"
    )


def _series(args: Any) -> list[str]:
    if not isinstance(args, dict) or set(args) != SERIES_KEYS:
        raise invalid("series is {from, until, every}")
    start = _wall_time(args["from"], "from")
    until = _wall_time(args["until"], "until")
    every = parse_duration_secs(args["every"])
    if every < 1:
        raise invalid("series every is at least one second")
    if until < start:
        raise invalid("series until is not before from")
    span = until - start
    count = (span.days * 86400 + span.seconds) // every + 1
    if count > MAX_FILES:
        raise invalid(f"one add_backup_files step creates at most {MAX_FILES} files")
    return [backup_name(start + timedelta(seconds=i * every)) for i in range(count)]


def files_to_add(args: Any) -> list[str]:
    """The file names of an add_backup_files step, checked (F2)."""
    if not isinstance(args, dict) or len(args) != 1 or not set(args) <= {"names", "series"}:
        raise invalid("add_backup_files takes exactly one of {names} and {series}")
    if "series" in args:
        return _series(args["series"])
    names = args["names"]
    if not isinstance(names, list) or not names:
        raise invalid("add_backup_files names is a non-empty list")
    if len(names) > MAX_FILES:
        raise invalid(f"one add_backup_files step creates at most {MAX_FILES} files")
    checked = [_file_name(n) for n in names]
    if len(set(checked)) != len(checked):
        raise invalid("add_backup_files names repeats a name")
    return checked


def _copy_as(args: Any) -> str | None:
    if not isinstance(args, dict) or not set(args) <= {"copy_as"}:
        raise invalid("force_backup takes {} or {copy_as}")
    if "copy_as" not in args:
        return None
    return colpkg.colpkg_name(args["copy_as"], "force_backup copy_as")


def limits(args: Any) -> dict[str, int]:
    """The four values of a save_backup_settings step (F4)."""
    if not isinstance(args, dict) or set(args) != set(LIMITS):
        raise invalid("save_backup_settings takes exactly {daily, weekly, monthly, minimum_interval_mins}")
    for key in LIMITS:
        value = args[key]
        if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value <= U32_MAX:
            raise invalid(f"save_backup_settings {key} is an integer from 0 to {U32_MAX}")
    return {key: args[key] for key in LIMITS}


def _force(args: Any) -> bool:
    if not isinstance(args, dict) or set(args) != {"force"} or not isinstance(args["force"], bool):
        raise invalid("backup takes {force: true|false}")
    return args["force"]


_ARG_CHECKS = {
    "add_backup_files": files_to_add,
    "force_backup": _copy_as,
    "save_backup_settings": limits,
    "backup": _force,
}


@scenario_check
def check_backup_steps(scenario: dict) -> None:
    """F2-F4 and H1 arguments, and F7: one force_backup per opening."""
    since_opening = 0
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, args),) = step.items()
        if name in _ARG_CHECKS:
            try:
                _ARG_CHECKS[name](args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise
        if name in _RESET_STEPS:
            since_opening = 0
        elif name == "force_backup":
            since_opening += 1
            if since_opening > 1:
                raise invalid("force_backup is limited to one call until the next collection load "
                              "(start, open, reopen, import, handoff; backup-ops.md F7)", step=index)


@op("add_backup_files")
def add_backup_files(session, args):
    names = files_to_add(args)
    path = _folder(session)
    taken = [n for n in names if os.path.lexists(os.path.join(path, n))]
    if taken:
        raise invalid(f"{len(taken)} of the names are already in the backups folder, the first {taken[0]!r}")
    for name in names:
        with open(os.path.join(path, name), "xb"):
            pass


def _create_backup(session, force: bool, op_name: str, rule: str) -> str | None:
    """Anki's collection backup into the folder, waited for: the one regular
    file it added, or None when Anki made no backup."""
    path = _folder(session)
    before = set(_regular_files(path))
    created = session.col.create_backup(backup_folder=path, force=force, wait_for_completion=True)
    if not created:
        return None
    added = sorted(set(_regular_files(path)) - before)
    if not added:
        raise invalid(f"the backup added no file: it took the name of a seeded file ({rule})")
    if len(added) != 1:
        raise DriverError("internal_error", op=op_name, detail=f"the backup added {len(added)} files")
    return added[0]


@op("force_backup")
def force_backup(session, args):
    copy_as = _copy_as(args)
    name = _create_backup(session, True, "force_backup", "backup-ops.md F7")
    if name is None:
        raise DriverError("anki_error", op="force_backup",
                          detail="Anki skipped the backup: the collection has not changed since this opening's last one")
    if copy_as is not None:
        shutil.copyfile(os.path.join(folder_path(session), name), os.path.join(session.dir, copy_as))
    return {"name": name}


@op("backup")
def backup(session, args):
    return {"name": _create_backup(session, _force(args), "backup", "backup-trigger-ops.md H3")}


@op("save_backup_settings")
def save_backup_settings(session, args):
    values = limits(args)
    session.col.set_preferences(Preferences(backups=Preferences.BackupLimits(**values)))


@observation("backup_files")
def backup_files(session, args):
    if args not in (None, []):
        raise invalid("backup_files takes no argument")
    path = folder_path(session)
    if not os.path.isdir(path):
        return []
    return sorted(_regular_files(path), key=os.fsencode)


@observation("backup_settings")
def backup_settings(session, args):
    if args not in (None, []):
        raise invalid("backup_settings takes no argument")
    backups = session.col.get_preferences().backups
    return {key: getattr(backups, key) for key in LIMITS}
