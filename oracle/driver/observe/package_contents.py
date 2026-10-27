# SPDX-License-Identifier: AGPL-3.0-or-later
"""package_media and package_db_dump: what a package holds, read without
importing it (specs/conformance/package-ops.md E10, E11).

package_media {name} | {fixture} -> [{name, size, sha1, entry_size}, ...]
    The format comes from the `meta` entry as model-ops.md L7 reads it
    (observe/package_listing.py's P3 reader): version 3 is the latest format,
    1 or 2, or no `meta` entry, a legacy one; any other version is
    invalid_scenario. Latest: one item per record of the zstd media map
    (protobuf MediaEntries), in map order, name, size and lower-case hex sha1
    as the record holds them, entry_size the decoded size of entry <index>
    (null without one). Legacy: the JSON map's members in ascending member
    number, size and sha1 computed from entry <number> after its zip method,
    entry_size equal to size, all three null without that entry. No `media`
    entry: [].
package_db_dump {name | fixture, tables?, raw?} -> db_dump's value (O8) of
    the package's collection entry: collection.anki21b decoded (latest), else
    the entry L7 names (meta 1: collection.anki2, 2: collection.anki21, no
    meta: collection.anki21 when present, else collection.anki2). The entry is
    written to the worker's scratch and read with Python's sqlite3 (a unicase
    collation registered, no Anki open, no upgrade); without `tables` every O8
    table the file holds, a listed table it lacks is a run error.

The oracle image has no zstd library. Zstd entries are decoded by Anki itself:
the driver writes a scratch collection package (meta 3, a scratch collection
in one of its own raw-block frames, the entries to decode copied byte for
byte as its numbered media entries, and a media map naming them d0, d1, ...)
and runs Anki's collection package import of it into a scratch profile with a
backend of its own (no collection open); the import writes each entry's
decoded bytes to the scratch media folder unchanged. The scratch collection is
a new empty collection made once per worker.

Neither observation changes the scenario's collection, the package or the
session directory; scratch files are removed. An archive that does not read
as zip, a missing entry, an entry or map that does not decode, and a file
SQLite cannot read are run errors (internal_error with a detail).
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import sqlite3
import tempfile
import zipfile
import zlib
from typing import Any

from anki._backend import RustBackend
from anki.collection import Collection
from anki.import_export_pb2 import MediaEntries

from ..errors import DriverError, invalid
from ..ops.storage_open import fixture_path
from ..registry import observation, scenario_check
from .db_dump import TABLES, dump_table
from .package_listing import meta_version

PACKAGE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*\.(colpkg|apkg)")
ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"
RAW_BLOCK = 128 * 1024
LEGACY_ENTRY = {1: "collection.anki2", 2: "collection.anki21"}
KINDS = ("package_media", "package_db_dump")

_decoder: dict[str, Any] = {}


def _bad(detail: str) -> DriverError:
    return DriverError("internal_error", detail=detail)


@scenario_check
def check_contents_steps(scenario: dict) -> None:
    """E10, E11: at most one of each per observe step."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, args),) = step.items()
        if name == "observe" and isinstance(args, list):
            for kind in KINDS:
                if len([s for s in args if isinstance(s, dict) and kind in s]) > 1:
                    raise invalid(f"an observe step has at most one {kind}", step=index)


def _package(session, args: Any, kind: str, extra: set[str]) -> str:
    if not isinstance(args, dict) or not args.keys() & {"name", "fixture"} or set(args) - {"name", "fixture"} - extra:
        raise invalid(f"{kind} takes {{name}} or {{fixture}}" + (" with tables? and raw?" if extra else ""))
    if "fixture" in args:
        if "name" in args:
            raise invalid(f"{kind} takes one of name and fixture")
        return fixture_path(args["fixture"], (".colpkg", ".apkg"), f"{kind} fixture")
    name = args["name"]
    if not isinstance(name, str) or not PACKAGE_NAME.fullmatch(name):
        raise invalid(f"{kind} name is a file name of ASCII letters, digits, '.', '_' and '-' "
                      "that ends in .colpkg or .apkg")
    path = os.path.join(session.dir, name)
    if not os.path.isfile(path):
        raise DriverError("unknown_ref", ref=name)
    return path


def _open(path: str) -> zipfile.ZipFile:
    try:
        return zipfile.ZipFile(path)
    except (zipfile.BadZipFile, OSError) as exc:
        raise _bad("the package does not read as a zip archive") from exc


def _read(archive: zipfile.ZipFile, name: str) -> bytes:
    try:
        return archive.read(name)
    except (zipfile.BadZipFile, zlib.error, EOFError, NotImplementedError, RuntimeError) as exc:
        raise _bad(f"entry {name!r} does not read") from exc


def _version(archive: zipfile.ZipFile, names: set[str]) -> int | None:
    """3 for the latest format, 1 or 2 for a legacy one, None without meta."""
    if "meta" not in names:
        return None
    version = meta_version(_read(archive, "meta"))
    if version not in (1, 2, 3):
        raise invalid(f"package meta version {version} is neither the latest format nor a legacy one (model-ops.md L7)")
    return version


# --- zstd through Anki --------------------------------------------------------


def raw_frame(data: bytes) -> bytes:
    """One zstd frame (RFC 8878 3.1.1): single segment, content size, raw
    blocks of at most 128 KiB, no checksum."""
    n = len(data)
    if n < 256:
        flag, size = 0, n.to_bytes(1, "little")
    elif n < 65536 + 256:
        flag, size = 1, (n - 256).to_bytes(2, "little")
    elif n < 1 << 32:
        flag, size = 2, n.to_bytes(4, "little")
    else:
        flag, size = 3, n.to_bytes(8, "little")
    out = bytearray(ZSTD_MAGIC + bytes([flag << 6 | 0x20]) + size)
    chunks = [data[i:i + RAW_BLOCK] for i in range(0, n, RAW_BLOCK)] or [b""]
    for k, chunk in enumerate(chunks):
        out += (len(chunk) << 3 | int(k == len(chunks) - 1)).to_bytes(3, "little") + chunk
    return bytes(out)


def _scratch_collection(scratch: str) -> bytes:
    if "collection" not in _decoder:
        work = tempfile.mkdtemp(prefix="decoder-col-", dir=scratch)
        try:
            col = Collection(os.path.join(work, "collection.anki2"))
            col.close(downgrade=False)
            with open(os.path.join(work, "collection.anki2"), "rb") as f:
                _decoder["collection"] = f.read()
        finally:
            shutil.rmtree(work, ignore_errors=True)
    return _decoder["collection"]


def _backend() -> RustBackend:
    if "backend" not in _decoder:
        _decoder["backend"] = RustBackend()
    return _decoder["backend"]


def decode_frames(session, frames: list[bytes]) -> list[bytes]:
    """The decoded content of each zstd frame sequence, by Anki's decoder."""
    if not frames:
        return []
    work = tempfile.mkdtemp(prefix="decode-", dir=session.scratch)
    try:
        package = os.path.join(work, "decode.colpkg")
        entries = MediaEntries()
        for i in range(len(frames)):
            entries.entries.add(name=f"d{i}", size=0, sha1=bytes(20))
        with zipfile.ZipFile(package, "w") as z:
            z.writestr("meta", b"\x08\x03")
            z.writestr("collection.anki21b", raw_frame(_scratch_collection(session.scratch)))
            for i, frame in enumerate(frames):
                z.writestr(str(i), frame)
            z.writestr("media", raw_frame(entries.SerializeToString()))
        profile = os.path.join(work, "profile")
        media = os.path.join(profile, "collection.media")
        os.makedirs(media)
        try:
            _backend().import_collection_package(
                col_path=os.path.join(profile, "collection.anki2"), backup_path=package,
                media_folder=media, media_db=os.path.join(profile, "collection.media.db2"),
            )
        except Exception as exc:  # noqa: BLE001 - a frame that does not decode
            raise _bad("a zstd entry of the package does not decode") from exc
        out = []
        for i in range(len(frames)):
            with open(os.path.join(media, f"d{i}"), "rb") as f:
                out.append(f.read())
        return out
    finally:
        shutil.rmtree(work, ignore_errors=True)


# --- package_media ------------------------------------------------------------


def _latest_media(session, archive: zipfile.ZipFile, names: set[str]) -> list[dict]:
    (data,) = decode_frames(session, [_read(archive, "media")])
    entries = MediaEntries()
    try:
        entries.ParseFromString(data)
    except Exception as exc:  # noqa: BLE001 - protobuf's DecodeError
        raise _bad("the package's media map does not decode") from exc
    present = [i for i in range(len(entries.entries)) if str(i) in names]
    decoded = dict(zip(present, decode_frames(session, [_read(archive, str(i)) for i in present])))
    return [
        {
            "name": entry.name,
            "size": entry.size,
            "sha1": entry.sha1.hex(),
            "entry_size": len(decoded[i]) if i in decoded else None,
        }
        for i, entry in enumerate(entries.entries)
    ]


def _legacy_media(archive: zipfile.ZipFile, names: set[str]) -> list[dict]:
    try:
        members = json.loads(_read(archive, "media"))
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        raise _bad("the package's media map is not JSON") from exc
    if not isinstance(members, dict) or not all(isinstance(v, str) for v in members.values()):
        raise _bad("the package's media map is not an object of names")
    try:
        numbered = sorted((int(k), k, v) for k, v in members.items())
    except ValueError as exc:
        raise _bad("a media map member is not a number") from exc
    out = []
    for _, key, name in numbered:
        if key in names:
            data = _read(archive, key)
            size, sha1 = len(data), hashlib.sha1(data).hexdigest()
            out.append({"name": name, "size": size, "sha1": sha1, "entry_size": size})
        else:
            out.append({"name": name, "size": None, "sha1": None, "entry_size": None})
    return out


@observation("package_media")
def package_media(session, args):
    path = _package(session, args, "package_media", set())
    with _open(path) as archive:
        names = set(archive.namelist())
        version = _version(archive, names)
        if "media" not in names:
            return []
        if version == 3:
            return _latest_media(session, archive, names)
        return _legacy_media(archive, names)


# --- package_db_dump ----------------------------------------------------------


class _PlainDb:
    def __init__(self, con: sqlite3.Connection) -> None:
        self._con = con

    def list(self, sql: str) -> list:
        return [row[0] for row in self._con.execute(sql)]

    def all(self, sql: str) -> list:
        return [list(row) for row in self._con.execute(sql)]


class _PlainCol:
    def __init__(self, con: sqlite3.Connection) -> None:
        self.db = _PlainDb(con)


def _unicase(a: str, b: str) -> int:
    a, b = a.casefold(), b.casefold()
    return (a > b) - (a < b)


def _dump_args(args: dict) -> tuple[list[str] | None, bool]:
    tables = args.get("tables")
    raw = args.get("raw", False)
    if not isinstance(raw, bool):
        raise invalid("package_db_dump raw is true or false")
    if tables is not None:
        if (
            not isinstance(tables, list) or not tables or len(set(tables)) != len(tables)
            or any(t not in TABLES for t in tables)
        ):
            raise invalid(f"package_db_dump tables are distinct names among {', '.join(TABLES)}")
    return tables, raw


def _collection_entry(session, archive: zipfile.ZipFile, names: set[str]) -> bytes:
    version = _version(archive, names)
    if version == 3:
        if "collection.anki21b" not in names:
            raise _bad("the package has no collection.anki21b entry")
        (data,) = decode_frames(session, [_read(archive, "collection.anki21b")])
        return data
    if version is None:
        entry = "collection.anki21" if "collection.anki21" in names else "collection.anki2"
    else:
        entry = LEGACY_ENTRY[version]
    if entry not in names:
        raise _bad(f"the package has no {entry} entry")
    return _read(archive, entry)


@observation("package_db_dump")
def package_db_dump(session, args):
    if not isinstance(args, dict):
        raise invalid("package_db_dump takes {name | fixture, tables?, raw?}")
    tables, raw = _dump_args(args)
    path = _package(session, args, "package_db_dump", {"tables", "raw"})
    with _open(path) as archive:
        data = _collection_entry(session, archive, set(archive.namelist()))
    work = tempfile.mkdtemp(prefix="package-dump-", dir=session.scratch)
    try:
        db = os.path.join(work, "collection")
        with open(db, "wb") as f:
            f.write(data)
        con = sqlite3.connect(db)
        try:
            con.create_collation("unicase", _unicase)
            try:
                held = {r[0] for r in con.execute("SELECT name FROM sqlite_master WHERE type = 'table'")}
            except sqlite3.DatabaseError as exc:
                raise _bad("the package's collection entry is not a readable SQLite file") from exc
            held = {next((t for t in TABLES if t.lower() == h.lower()), h) for h in held}
            if tables is None:
                tables = [t for t in TABLES if t in held]
            missing = [t for t in tables if t not in held]
            if missing:
                raise _bad(f"the package's collection has no {', '.join(missing)} table")
            col = _PlainCol(con)
            try:
                return {t: dump_table(col, t, raw) for t in tables}
            except (sqlite3.DatabaseError, KeyError, StopIteration) as exc:
                raise _bad("the package's collection entry does not read") from exc
        finally:
            con.close()
    finally:
        shutil.rmtree(work, ignore_errors=True)

