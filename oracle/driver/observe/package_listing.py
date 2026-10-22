# SPDX-License-Identifier: AGPL-3.0-or-later
"""package_listing {name} | {fixture}: the layout of a package
(specs/conformance/package-ops.md E3).

-> {entries: [{name, method}] in central-directory order,
    meta_version: <int> | null | "invalid",
    sqlite: {<entry name>: {ver, notes, cards, revlog, journal, opens}}}

meta_version is field 1 of the `meta` entry read as package/container.md P3
says (the driver's own reader of P3 and model/protobuf-records.md P2, not
pylib's protobuf, which skips a known field of another wire type). `sqlite`
holds every entry whose content after its zip method starts with the SQLite
header; nothing here decodes zstd. Each is copied to the worker's scratch:
`ver` and the row counts are read from one copy with plain sqlite3, `opens` is
Anki's collection open of another copy followed by an integrity check, false
on any failure. The scratch directory goes away afterwards; the session, the
package and the session directory are never touched.

An entry the reader cannot read (container.md P2: a method other than 0 or 8,
encryption, a bad deflate stream) is no SQLite entry, and a `meta` entry it
cannot read is "invalid" (P3). An archive that does not read as zip, a SQLite
entry that does not read to its end (a CRC mismatch), two SQLite entries of
one name, and a SQLite entry without readable col, notes, cards and revlog
tables are run errors (internal_error with a detail): E3 lets scenarios list
only readable packages.
"""

from __future__ import annotations

import os
import re
import shutil
import sqlite3
import tempfile
import zipfile
import zlib
from typing import Any

from anki.collection import Collection

from ..errors import DriverError, invalid, log_exception
from ..ops.storage_open import fixture_path
from ..registry import observation, scenario_check

# conformance/schema/scenario.schema.json any_package_name.
PACKAGE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*\.(colpkg|apkg)")
SQLITE_HEADER = b"SQLite format 3\x00"
READABLE = (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)  # container.md P2
META_LIMIT = 64 * 1024  # container.md P8
COUNTED = ("notes", "cards", "revlog")


def _bad(detail: str) -> DriverError:
    return DriverError("internal_error", detail=detail)


@scenario_check
def check_listing_steps(scenario: dict) -> None:
    """E3: at most one package_listing per observe step."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, args),) = step.items()
        if name == "observe":
            kinds = [spec for spec in args if isinstance(spec, dict) and "package_listing" in spec]
            if len(kinds) > 1:
                raise invalid("an observe step has at most one package_listing", step=index)


def _package(session, args: Any) -> str:
    if not isinstance(args, dict) or len(args) != 1 or not set(args) <= {"name", "fixture"}:
        raise invalid("package_listing takes {name} or {fixture}")
    if "fixture" in args:
        return fixture_path(args["fixture"], (".colpkg", ".apkg"), "package_listing fixture")
    name = args["name"]
    if not isinstance(name, str) or not PACKAGE_NAME.fullmatch(name):
        raise invalid("package_listing name is a file name of ASCII letters, digits, '.', '_' and '-' "
                      "that ends in .colpkg or .apkg")
    path = os.path.join(session.dir, name)
    if not os.path.isfile(path):
        raise DriverError("unknown_ref", ref=name)
    return path


class _Invalid(Exception):
    pass


def _varint(buf: bytes, i: int) -> tuple[int, int]:
    """protobuf-records.md P2: at most 10 bytes, the tenth 00 or 01."""
    value = 0
    for k in range(10):
        if i >= len(buf):
            raise _Invalid
        byte = buf[i]
        i += 1
        if k == 9 and byte > 1:
            raise _Invalid
        value |= (byte & 0x7F) << (7 * k)
        if byte < 0x80:
            return value, i
    raise _Invalid


def meta_version(buf: bytes) -> int | str:
    """container.md P3: field 1 as an int32 (low 32 bits), the last occurrence;
    0 without one; "invalid" for a message P2 does not read or field 1 with a
    wire type other than varint."""
    version, groups, i = 0, [], 0
    try:
        while i < len(buf):
            key, i = _varint(buf, i)
            field, wire = key >> 3, key & 7
            if key > 0xFFFFFFFF or field == 0 or wire > 5:
                raise _Invalid
            if wire == 4:
                if not groups or groups[-1] != field:
                    raise _Invalid
                groups.pop()
                continue
            if len(groups) >= 100:
                raise _Invalid
            if field == 1 and not groups:
                if wire != 0:
                    raise _Invalid
                value, i = _varint(buf, i)
                low = value & 0xFFFFFFFF
                version = low - (1 << 32) if low & 0x80000000 else low
            elif wire == 0:
                _, i = _varint(buf, i)
            elif wire == 1:
                i += 8
            elif wire == 5:
                i += 4
            elif wire == 2:
                length, i = _varint(buf, i)
                i += length
            else:
                groups.append(field)
            if i > len(buf):
                raise _Invalid
        if groups:
            raise _Invalid
    except _Invalid:
        return "invalid"
    return version


def _readable(info: zipfile.ZipInfo) -> bool:
    return info.compress_type in READABLE and not info.flag_bits & 0x1


def _meta(archive: zipfile.ZipFile, archive_entries: list[zipfile.ZipInfo]) -> int | str | None:
    metas = [info for info in archive_entries if info.filename == "meta"]
    if not metas:
        return None
    if not _readable(metas[0]):
        return "invalid"
    try:
        with archive.open(metas[0]) as f:
            data = f.read(META_LIMIT + 1)
    except (zipfile.BadZipFile, zlib.error, EOFError):
        return "invalid"
    return "invalid" if len(data) > META_LIMIT else meta_version(data)


def _starts_sqlite(archive: zipfile.ZipFile, info: zipfile.ZipInfo) -> bool:
    """False for an entry the reader cannot read (container.md P2)."""
    if not _readable(info):
        return False
    try:
        with archive.open(info) as f:
            return f.read(len(SQLITE_HEADER)) == SQLITE_HEADER
    except (zipfile.BadZipFile, zlib.error, EOFError):
        return False


def _extract(archive: zipfile.ZipFile, info: zipfile.ZipInfo, dest: str) -> None:
    try:
        with archive.open(info) as src, open(dest, "wb") as out:
            shutil.copyfileobj(src, out)
    except (zipfile.BadZipFile, zlib.error, EOFError) as exc:
        raise _bad(f"entry {info.filename!r} does not read") from exc


def _journal(path: str) -> str:
    with open(path, "rb") as f:
        header = f.read(20)
    modes = header[18:20]
    return "rollback" if modes == b"\x01\x01" else "wal" if modes == b"\x02\x02" else "other"


def _rows(path: str, entry: str) -> dict:
    con = sqlite3.connect(path)
    try:
        rows = con.execute("SELECT ver FROM col").fetchall()
        counts = {table: con.execute(f"SELECT count() FROM {table}").fetchone()[0] for table in COUNTED}
    except sqlite3.DatabaseError as exc:
        raise _bad(f"entry {entry!r} has no readable col, notes, cards or revlog table") from exc
    finally:
        con.close()
    if len(rows) != 1 or not isinstance(rows[0][0], int):
        raise _bad(f"entry {entry!r} has no single col row with an integer ver")
    return {"ver": rows[0][0], **counts}


def _opens(path: str, entry: str) -> bool:
    try:
        col = Collection(path)
    except Exception as exc:  # noqa: BLE001 - E3: false on any failure
        log_exception(exc, f"package_listing: Anki did not open entry {entry!r}")
        return False
    try:
        return col.db.all("SELECT * FROM pragma_integrity_check") == [["ok"]]
    except Exception as exc:  # noqa: BLE001
        log_exception(exc, f"package_listing: integrity check of entry {entry!r}")
        return False
    finally:
        try:
            col.close(downgrade=False)
        except Exception as exc:  # noqa: BLE001 - the copy goes away anyway
            log_exception(exc, f"package_listing: closing entry {entry!r}")


def _sqlite_facts(archive: zipfile.ZipFile, info: zipfile.ZipInfo, work: str) -> dict:
    read_copy, open_dir = os.path.join(work, "read.anki2"), os.path.join(work, "open")
    os.makedirs(open_dir)
    _extract(archive, info, read_copy)
    shutil.copyfile(read_copy, os.path.join(open_dir, "collection.anki2"))
    journal = _journal(read_copy)
    facts = _rows(read_copy, info.filename)
    return {**facts, "journal": journal, "opens": _opens(os.path.join(open_dir, "collection.anki2"), info.filename)}


@observation("package_listing")
def package_listing(session, args):
    path = _package(session, args)
    try:
        archive = zipfile.ZipFile(path)
    except (zipfile.BadZipFile, OSError) as exc:
        raise _bad("the package does not read as a zip archive") from exc
    scratch = tempfile.mkdtemp(prefix="listing-", dir=session.scratch)
    try:
        with archive:
            archive_entries = archive.infolist()
            sqlite: dict[str, dict] = {}
            for index, info in enumerate(archive_entries):
                if not _starts_sqlite(archive, info):
                    continue
                if info.filename in sqlite:
                    raise _bad(f"two SQLite entries are named {info.filename!r}")
                work = os.path.join(scratch, str(index))
                os.makedirs(work)
                sqlite[info.filename] = _sqlite_facts(archive, info, work)
            value = {
                "entries": [{"name": info.filename, "method": info.compress_type} for info in archive_entries],
                "meta_version": _meta(archive, archive_entries),
                "sqlite": sqlite,
            }
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    return value
