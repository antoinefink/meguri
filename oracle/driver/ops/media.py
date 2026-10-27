# SPDX-License-Identifier: AGPL-3.0-or-later
"""The media folder: outside changes, adds, the trash and what a media sync
would send (specs/conformance/media-ops.md F1-F12).

setup.media_files: [{name, text} | {name, fill: {size, byte?}} | {name, dir: true}]
    written through the file system after every other setup section
media_outside (op): [<media_files entry> | {name, remove: true}, ...] -> none
add_media (op): {name, text | fill} -> {name: the stored name}
trash_media (op): {names: [...]} -> none
restore_media_trash, empty_media_trash (ops): {} -> none
media_dir (observation) -> {name: {sha1, size} | {dir: true}}
media_trash (observation) -> [{name, sha1, size}], by UTF-8 name, then sha1
media_pending (observation) -> {name: sha1 | null}

The folder is the collection's own (`collection.media` beside the file) and the
trash is Anki's `media.trash` beside the folder. Outside writes bypass Anki and
then stamp every entry they wrote, and the folder last, with the frozen clock's
instant in whole seconds (F12): the kernel would stamp real time, which the
frozen clock never reaches, and registration tells changes by those times.

Names are checked as F3 says; variants (equal after NFC under Anki's own
`unicase` collation, asked through the collection's database) are
invalid_scenario, except in a scenario tagged media-case-variants (F14.4),
where only names equal after NFC are, and where the media folder must be
case-sensitive (a probe file; elsewhere media_files is unsupported_setup and
media_outside unsupported_op). Driver choices on top of F3/F4/F6, so that no golden holds
what a scenario must not ask: an outside file over a subdirectory or a
subdirectory over a file is invalid_scenario, and trash_media names distinct
regular files present under exactly that name (Anki stops at a missing name and
still marks the rest deleted).

add_media declares no rejection: `invalid_name` and `ignored_name` are
Meguri's only (F5), and a name Anki cannot store is a run error.

media_pending registers the folder the way a media sync begins (F11): it zeroes
the folder marker (meta.dirMod) in `collection.media.db2` so that the scan
cannot be skipped, starts a media sync to an endpoint nothing listens on (the
registration commits before the network call), waits for it to end with
Anki's network error, and reads the rows with dirty = 1. Any other outcome is a
run error, never a value. The marker and rows are read with sqlite3 while no
media operation runs: Anki opens the media database per call.
"""

from __future__ import annotations

import hashlib
import os
import sqlite3
import tempfile
import time
import unicodedata
from typing import Any

from anki.errors import NetworkError
from anki.media import media_paths_from_col_path
from anki.sync import SyncAuth

from ..errors import DriverError, invalid
from ..registry import observation, op, setup_section

MAX_FILL = 209_715_200  # F2
MAX_NAME_BYTES = 255  # F3
CHUNK = 1 << 20
TRASH = "media.trash"
# Nothing listens on port 1 and the worker has no route out: the sync fails
# at its first request, after the registration.
UNREACHABLE = "http://127.0.0.1:1/"
SYNC_DEADLINE_SECS = 120
Contents = bytes | tuple[int, int]  # text bytes, or (size, byte) of a fill


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def _utf8(value: str, what: str) -> bytes:
    try:
        return value.encode("utf-8")
    except UnicodeEncodeError as exc:
        raise invalid(f"{what} is not valid Unicode text") from exc


def _name(value: Any) -> str:
    """F3: an exact entry name."""
    if not isinstance(value, str) or not value:
        raise invalid("a media name is a non-empty string")
    if len(_utf8(value, "a media name")) > MAX_NAME_BYTES:
        raise invalid(f"media name {value!r} is longer than {MAX_NAME_BYTES} UTF-8 bytes")
    if "/" in value or "\x00" in value or value in (".", ".."):
        raise invalid(f"media name {value!r} holds / or U+0000, or is . or ..")
    return value


def _contents(args: dict) -> Contents:
    """F2: exactly one of text and fill (the caller checked the keys)."""
    if "text" in args:
        if not isinstance(args["text"], str):
            raise invalid("media text is a string")
        return _utf8(args["text"], "media text")
    fill = args["fill"]
    if not isinstance(fill, dict) or "size" not in fill or set(fill) - {"size", "byte"}:
        raise invalid("fill is {size, byte?}")
    size, byte = fill["size"], fill.get("byte", 0)
    if not _is_int(size) or not 0 <= size <= MAX_FILL:
        raise invalid(f"fill size is an integer from 0 to {MAX_FILL}")
    if not _is_int(byte) or not 0 <= byte <= 255:
        raise invalid("fill byte is an integer from 0 to 255")
    return (size, byte)


def _one_of_text_fill(args: dict) -> bool:
    return ("text" in args) != ("fill" in args)


def _entry(value: Any, removable: bool) -> tuple[str, str, Contents | None]:
    """A media_files entry (and, in media_outside, a removal) ->
    (file | dir | remove, name, contents)."""
    if not isinstance(value, dict) or "name" not in value:
        raise invalid("a media entry is {name, text | fill | dir: true}" + (" or {name, remove: true}" if removable else ""))
    name = _name(value["name"])
    keys = set(value) - {"name"}
    if keys == {"dir"} and value["dir"] is True:
        return "dir", name, None
    if removable and keys == {"remove"} and value["remove"] is True:
        return "remove", name, None
    if keys <= {"text", "fill"} and _one_of_text_fill(value):
        return "file", name, _contents(value)
    raise invalid(f"media entry {name!r} is {{name, text | fill | dir: true}}" + (" or {name, remove: true}" if removable else ""))


def _data(contents: Contents) -> bytes:
    if isinstance(contents, bytes):
        return contents
    size, byte = contents
    return bytes([byte]) * size


def _write(path: str, contents: Contents) -> None:
    """Create the file or replace its bytes in place."""
    with open(path, "wb") as f:
        if isinstance(contents, bytes):
            f.write(contents)
            return
        size, byte = contents
        chunk = bytes([byte]) * min(size, CHUNK)
        while size:
            n = min(size, CHUNK)
            f.write(chunk[:n])
            size -= n


def _variants(col, a: str, b: str) -> bool:
    """a and b equal after NFC under unicase (text/unicode-primitives U2)."""
    return bool(col.db.scalar("select ? = ? collate unicase", unicodedata.normalize("NFC", a), unicodedata.normalize("NFC", b)))


CASE_VARIANTS_TAG = "media-case-variants"


def _case_variants(session) -> bool:
    return CASE_VARIANTS_TAG in (session.scenario.get("tags") or [])


def _case_sensitive(folder: str) -> bool:
    """Whether the folder tells names apart by case: a probe file created in
    it and looked up under its swapped case. _apply stamps the folder after
    any probe, so the probe leaves no trace a registration could see."""
    fd, path = tempfile.mkstemp(prefix=".Case-Probe-", dir=folder)
    os.close(fd)
    try:
        head, tail = os.path.split(path)
        return not os.path.lexists(os.path.join(head, tail.swapcase()))
    finally:
        os.remove(path)


def _require_case_sensitive(session, error: DriverError) -> None:
    """F14.4: a scenario tagged media-case-variants runs only where the media
    folder is case-sensitive (Linux); elsewhere case variants would collapse
    into one file, so the step fails as one the driver cannot run."""
    if _case_variants(session) and not _case_sensitive(_folder(session)):
        raise error


def _clash(session, a: str, b: str) -> bool:
    """F3/F4's refusal: variants, or in a scenario tagged media-case-variants
    (F14.4) only names equal after NFC (bytes)."""
    if _case_variants(session):
        return unicodedata.normalize("NFC", a) == unicodedata.normalize("NFC", b)
    return _variants(session.col, a, b)


def _folder(session) -> str:
    return session.col.media.dir()


def _apply(session, entries: list[tuple[str, str, Contents | None]]) -> None:
    """F3/F4: apply the entries in order through the file system, then stamp
    every entry written and the folder with the clock's instant (F12)."""
    folder = _folder(session)
    written: dict[str, None] = {}
    for kind, name, contents in entries:
        path = os.path.join(folder, name)
        present = os.listdir(folder)
        if kind == "remove":
            if name not in present:
                raise invalid(f"{name!r} is not in the media folder")
            if os.path.isdir(path):
                os.rmdir(path)
            else:
                os.remove(path)
            written.pop(path, None)
            continue
        clash = next((other for other in present if other != name and _clash(session, other, name)), None)
        if clash is not None:
            raise invalid(f"{name!r} is a case or normalization variant of {clash!r} in the media folder")
        if name in present and os.path.isdir(path) != (kind == "dir"):
            raise invalid(f"{name!r} is already in the media folder as a " + ("file" if kind == "dir" else "subdirectory"))
        if kind == "dir":
            if name not in present:
                os.mkdir(path)
        else:
            _write(path, contents)
        written[path] = None
    now = session.clock.now
    for path in written:
        os.utime(path, (now, now))
    os.utime(folder, (now, now))


@setup_section("media_files", order=90)
def media_files(session, entries):
    if not isinstance(entries, list):
        raise invalid("setup.media_files is a list of entries")
    parsed = [_entry(e, removable=False) for e in entries]
    _require_case_sensitive(session, DriverError(
        "unsupported_setup", sections=["media_files"],
        detail="media-case-variants needs a case-sensitive media folder (media-ops.md F14.4)"))
    names = [name for _, name, _ in parsed]
    for i, a in enumerate(names):
        for b in names[i + 1:]:
            if a == b or _clash(session, a, b):
                raise invalid(f"setup.media_files names {a!r} and {b!r} are equal after NFC under unicase")
    _apply(session, parsed)


@op("media_outside")
def media_outside(session, args):
    if not isinstance(args, list) or not args:
        raise invalid("media_outside takes a non-empty list of entries")
    parsed = [_entry(e, removable=True) for e in args]
    _require_case_sensitive(session, DriverError(
        "unsupported_op", op="media_outside",
        detail="media-case-variants needs a case-sensitive media folder (media-ops.md F14.4)"))
    _apply(session, parsed)


@op("add_media")
def add_media(session, args):
    if not isinstance(args, dict) or "name" not in args or set(args) - {"name", "text", "fill"} or not _one_of_text_fill(args):
        raise invalid("add_media takes {name, text | fill}")
    name = args["name"]
    if not isinstance(name, str):
        raise invalid("add_media name is a string")
    _utf8(name, "add_media name")
    return {"name": session.col.media.write_data(name, _data(_contents(args)))}


@op("trash_media")
def trash_media(session, args):
    if not isinstance(args, dict) or set(args) != {"names"}:
        raise invalid("trash_media takes {names: [...]}")
    names = args["names"]
    if not isinstance(names, list) or not names or not all(isinstance(n, str) and n for n in names):
        raise invalid("trash_media names is a non-empty list of names")
    if len(set(names)) != len(names):
        raise invalid("trash_media names are distinct")
    folder = _folder(session)
    present = set(os.listdir(folder))
    for name in names:
        if name not in present or not os.path.isfile(os.path.join(folder, name)):
            raise invalid(f"trash_media names files in the media folder; {name!r} is not one (media-ops.md F6)")
    session.col.media.trash_files(names)


def _no_arguments(op_name: str, args: Any) -> None:
    if not isinstance(args, dict) or args:
        raise invalid(f"{op_name} takes {{}}")


@op("restore_media_trash")
def restore_media_trash(session, args):
    _no_arguments("restore_media_trash", args)
    session.col.media.restore_trash()


@op("empty_media_trash")
def empty_media_trash(session, args):
    _no_arguments("empty_media_trash", args)
    session.col.media.empty_trash()


def _file(path: str) -> dict:
    digest, size = hashlib.sha1(), 0
    with open(path, "rb") as f:
        while block := f.read(CHUNK):
            digest.update(block)
            size += len(block)
    return {"sha1": digest.hexdigest(), "size": size}


def _entries(folder: str):
    """(name, path, is_dir) for every entry; scenarios and Anki make only
    regular files and subdirectories."""
    with os.scandir(folder) as it:
        for e in it:
            if e.is_dir(follow_symlinks=False):
                yield e.name, e.path, True
            elif e.is_file(follow_symlinks=False):
                yield e.name, e.path, False
            else:
                raise RuntimeError("a media folder entry is neither a regular file nor a directory")


def _no_observation_arguments(kind: str, args: Any) -> None:
    if args is not None:
        raise invalid(f"{kind} takes no arguments")


@observation("media_dir")
def media_dir(session, args):
    _no_observation_arguments("media_dir", args)
    found = {name: {"dir": True} if is_dir else _file(path) for name, path, is_dir in _entries(_folder(session))}
    return dict(sorted(found.items()))


@observation("media_trash")
def media_trash(session, args):
    _no_observation_arguments("media_trash", args)
    trash = os.path.join(os.path.dirname(_folder(session)), TRASH)
    if not os.path.isdir(trash):
        return []
    items = []
    for name, path, is_dir in _entries(trash):
        if is_dir:
            raise RuntimeError("the media trash holds a directory")
        items.append({"name": name, **_file(path)})
    return sorted(items, key=lambda i: (i["name"].encode("utf-8"), i["sha1"]))


def _register(col) -> None:
    """F11 step 1: Anki's own registration, the first step of a media sync."""
    db2 = media_paths_from_col_path(col.path)[1]
    if os.path.exists(db2):
        con = sqlite3.connect(db2, isolation_level=None, timeout=30)
        try:
            con.execute("update meta set dirMod = 0")
        finally:
            con.close()
    col.sync_media(SyncAuth(hkey="meguri-oracle", endpoint=UNREACHABLE))
    deadline = time.monotonic() + SYNC_DEADLINE_SECS
    while True:
        try:
            status = col.media_sync_status()
        except NetworkError:
            return
        if not status.active:
            raise RuntimeError("the media sync ended without the network error")
        if time.monotonic() > deadline:
            col.abort_media_sync()
            raise RuntimeError("the media sync did not end")
        time.sleep(0.002)


@observation("media_pending")
def media_pending(session, args):
    _no_observation_arguments("media_pending", args)
    col = session.col
    _register(col)
    con = sqlite3.connect(media_paths_from_col_path(col.path)[1], isolation_level=None, timeout=30)
    try:
        rows = con.execute("select fname, csum from media where dirty = 1").fetchall()
    finally:
        con.close()
    return dict(sorted(rows))
