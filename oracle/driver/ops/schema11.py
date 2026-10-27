# SPDX-License-Identifier: AGPL-3.0-or-later
"""The schema-11 copy of the collection and its reads (specs/conformance/storage-ops.md P4-P8).

schema11_copy (op): {name}   Anki's downgrade to schema 11 of a copy of the
    collection file, written to <session dir>/<name> (replacing a file of that
    name); the collection stays open and unchanged.
    -> {} or the rejection invalid_json (Anki's JSON error kind: a config value
    the downgrade cannot read), when nothing is written.
file_col (observation): {name, columns: [crt|mod|scm|ver|dty|usn|ls|conf|models|decks|dconf|tags]}
    -> {<column>: value} from the file's single col row: integer columns as
    stored, JSON columns parsed as legacy_json's objects are (ints stay ints,
    floats stay floats).
file_objects (observation): {name} -> schema_objects (observe/schema.py) of the file.
file_query (observation): {name, sql} -> {rows: [[value, ...], ...]}, one
    statement that only reads; values as db_exec writes them (BLOB {sha256},
    text that is not UTF-8 with U+FFFD, as Anki's bridge gives db_exec).

Anki downgrades only the collection it closes, and its plain close with the
downgrade logs a failed downgrade and returns normally, leaving the file at
schema 18. So the session collection closes without the downgrade, its file is
copied to scratch and opened there as its own Collection, and that copy goes
through the legacy collection-package export without media, which closes it
with the downgrade and propagates a failure. The package's collection.anki21
entry (meta version 2, deflated) is the downgraded file: it replaces <name> by
a rename on the same tmpfs. The collection then opens again, also after a
failure. pylib raises the JSON error kind as a plain BackendError, so it is
read from the backend error (model.KindProbe); any other failure is a run
error.

A session file name is ASCII letters, digits, '.', '_' and '-', starting with a
letter or digit, ending in .anki2, never collection.anki2. open {file}
(storage_open.py) and the observations name files that exist in the session
directory, else unknown_ref. Session.hand_off copies that directory, so a
session file crosses a handoff.

Each observation copies the file byte for byte to the worker's scratch and
opens the copy with Python's sqlite3, never with Anki (opening upgrades to 18
and empties the JSON columns), with Anki's unicase collation stood in for and
no function, then deletes the copy. A plain open reads the journal mode from
the file header.

file_query runs the statement under an authorizer that allows a SELECT with the
reads, functions, recursive CTEs and table-valued pragma functions inside it,
and nothing else: a statement that would write, a PRAGMA statement (whose first
action is the pragma, not a SELECT) and a statement with no query are
invalid_scenario, as is more than one statement (Python's sqlite3 refuses it
before running any). A statement SQLite fails (a misnamed table, a syntax
error) is invalid_scenario too. A file SQLite cannot read (not a database,
corrupt), a col table without exactly one row and a column that does not
parse are internal_error with a detail, as package_col's malformed packages:
P5-P7 name no kind for these run errors and protocol.json has none.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
import re
import shutil
import sqlite3
import tempfile
import zipfile
from contextlib import contextmanager
from typing import Any, Iterator

from anki import backend_pb2
from anki.collection import Collection
from anki.import_export_pb2 import PackageMetadata

from ..errors import DriverError, Rejected, invalid
from ..observe.schema import MASTER_QUERY, listing
from ..registry import observation, op
from .model import KindProbe

# conformance/schema/scenario.schema.json session_file; fullmatch, since `$`
# also matches before a final newline.
# Lower case only: on a case-insensitive volume Collection.anki2 is the collection.
SESSION_FILE = re.compile(r"[a-z0-9][a-z0-9._-]*\.anki2")
COLLECTION = "collection.anki2"
LEGACY_VERSION = 2  # PackageMetadata version of a legacy package: collection.anki21
LEGACY_ENTRY = "collection.anki21"
COLUMNS = ("crt", "mod", "scm", "ver", "dty", "usn", "ls", "conf", "models", "decks", "dconf", "tags")
INT_COLUMNS = ("crt", "mod", "scm", "ver", "dty", "usn", "ls")
# What a SELECT may do besides selecting; a pragma only after the SELECT
# (table-valued pragma functions).
_READS = frozenset({sqlite3.SQLITE_READ, sqlite3.SQLITE_FUNCTION, sqlite3.SQLITE_RECURSIVE})
_UNREADABLE = frozenset({"SQLITE_NOTADB", "SQLITE_CORRUPT"})


def session_file_name(value: Any, what: str) -> str:
    if not isinstance(value, str) or not SESSION_FILE.fullmatch(value) or value == COLLECTION:
        raise invalid(
            f"{what} is a file name of lower-case ASCII letters, digits, '.', '_' and '-' that starts with a letter or digit"
            " and ends in .anki2, other than collection.anki2"
        )
    return value


def session_file(session, name: str) -> str:
    """The path of the session file name, or unknown_ref when there is none."""
    path = os.path.join(session.dir, name)
    if not os.path.isfile(path):
        raise DriverError("unknown_ref", ref=name)
    return path


def unicase_collation(a: str, b: str) -> int:
    """Stands in for Anki's `unicase` collation on sqlite3 connections outside
    Anki: full case folding, then code point order ('ß' equals 'SS')."""
    x, y = a.casefold(), b.casefold()
    return (x > y) - (x < y)


# --- schema11_copy ----------------------------------------------------------------


class _JsonRefused(Exception):
    pass


def _export_legacy(path: str, out: str) -> bool:
    """Anki's legacy collection package of the collection file at path, without
    media; False when the downgrade met Anki's JSON error kind."""
    col = Collection(path)
    backend = col._backend
    # As Collection.export_collection_package: the backend closes the collection.
    col.close_for_full_sync()
    probe = KindProbe(backend, backend_pb2.BackendError.JSON_ERROR, _JsonRefused)
    try:
        type(backend).export_collection_package(probe, out_path=out, include_media=False, legacy=True)
    except _JsonRefused:
        return False
    return True


def _extract(package: str, dest: str) -> None:
    with zipfile.ZipFile(package) as archive:
        meta = PackageMetadata()
        meta.ParseFromString(archive.read("meta"))
        if meta.version != LEGACY_VERSION:
            raise DriverError("internal_error", detail=f"the legacy export wrote package version {meta.version}")
        with archive.open(LEGACY_ENTRY) as src, open(dest, "wb") as dst:
            shutil.copyfileobj(src, dst)


@op("schema11_copy", rejects=("invalid_json",))
def schema11_copy(session, args):
    if not isinstance(args, dict) or set(args) != {"name"}:
        raise invalid("schema11_copy takes {name}")
    name = session_file_name(args["name"], "schema11_copy name")
    scratch = tempfile.mkdtemp(prefix="schema11-", dir=session.scratch)
    session.close_collection()
    try:
        copy = os.path.join(scratch, COLLECTION)
        shutil.copyfile(session.path, copy)
        package = os.path.join(scratch, "legacy.colpkg")
        if not _export_legacy(copy, package):
            raise Rejected("invalid_json")
        downgraded = os.path.join(scratch, name)
        _extract(package, downgraded)
        os.replace(downgraded, os.path.join(session.dir, name))
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
        session.col = Collection(session.path)


# --- reading a session file -------------------------------------------------------


def _malformed(detail: str) -> DriverError:
    return DriverError("internal_error", detail=detail)


def _lossy(data: bytes) -> str:
    return data.decode("utf-8", "replace")


def _not_json(token: str) -> Any:
    raise ValueError(f"{token} is not JSON")


def _finite(text: str) -> float:
    value = float(text)
    if not math.isfinite(value):
        raise ValueError("a number beyond the 64-bit float range")
    return value


def col_value(column: str, stored: Any, name: str) -> Any:
    """A col column as file_col records it (storage-ops.md P5): an integer
    column as stored, a JSON column parsed as legacy_json's objects are, so ints
    stay ints and floats stay floats. Another storage class, text that is not
    JSON (NaN, Infinity and numbers beyond the 64-bit float range included) and
    a repeated member name are internal_error."""
    if column in INT_COLUMNS:
        if not isinstance(stored, int):
            raise _malformed(f"{name}'s col.{column} is not an integer")
        return stored
    if not isinstance(stored, str):
        raise _malformed(f"{name}'s col.{column} is not text")

    def unique(pairs: list[tuple[str, Any]]) -> dict:
        obj = dict(pairs)
        if len(obj) != len(pairs):
            # The observation's JSON cannot hold both members.
            raise _malformed(f"{name}'s col.{column} repeats a member name")
        return obj

    try:
        return json.loads(stored, object_pairs_hook=unique, parse_constant=_not_json, parse_float=_finite)
    except ValueError as exc:  # json.JSONDecodeError is one
        raise _malformed(f"{name}'s col.{column} is not JSON") from exc


@contextmanager
def _reading(session, name: str, text_factory=str) -> Iterator[sqlite3.Connection]:
    """A connection on a scratch copy of the session file name."""
    path = session_file(session, name)
    scratch = tempfile.mkdtemp(prefix="file-", dir=session.scratch)
    try:
        copy = os.path.join(scratch, name)
        shutil.copyfile(path, copy)
        con = sqlite3.connect(copy, isolation_level=None)
        try:
            con.text_factory = text_factory
            con.create_collation("unicase", unicase_collation)
            yield con
        finally:
            con.close()
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def _request(kind: str, args: Any, keys: set[str]) -> str:
    if not isinstance(args, dict) or set(args) != keys:
        raise invalid(f"{kind} takes {{{', '.join(sorted(keys))}}}")
    return session_file_name(args["name"], f"{kind} name")


@observation("file_col")
def file_col(session, args):
    name = _request("file_col", args, {"name", "columns"})
    columns = args["columns"]
    if (
        not isinstance(columns, list)
        or not columns
        or not all(isinstance(c, str) and c in COLUMNS for c in columns)
        or len(set(columns)) != len(columns)
    ):
        raise invalid(f"file_col columns is a non-empty list, without repeats, of {', '.join(COLUMNS)}")
    with _reading(session, name) as con:
        try:
            rows = con.execute(f"SELECT {', '.join(columns)} FROM col").fetchall()
        except sqlite3.Error as exc:
            raise _malformed(f"{name} has no readable col table") from exc
    if len(rows) != 1:
        raise _malformed(f"{name}'s col table has {len(rows)} rows")
    return {column: col_value(column, stored, name) for column, stored in zip(columns, rows[0])}


@observation("file_objects")
def file_objects(session, args):
    name = _request("file_objects", args, {"name"})
    with _reading(session, name) as con:
        try:
            rows = con.execute(MASTER_QUERY).fetchall()
        except sqlite3.Error as exc:
            raise _malformed(f"{name} is not a readable SQLite database") from exc
    return listing(rows)


def _value(value: Any) -> Any:
    if isinstance(value, bytes):
        return {"sha256": hashlib.sha256(value).hexdigest()}
    return value


@observation("file_query")
def file_query(session, args):
    name = _request("file_query", args, {"name", "sql"})
    sql = args["sql"]
    if not isinstance(sql, str) or not sql.strip():
        raise invalid("file_query sql is one SQL statement")
    seen = {"select": False, "denied": False}

    def gate(action: int, *_: Any) -> int:
        if action == sqlite3.SQLITE_SELECT:
            seen["select"] = True
        elif not (action in _READS or (action == sqlite3.SQLITE_PRAGMA and seen["select"])):
            seen["denied"] = True
            return sqlite3.SQLITE_DENY
        return sqlite3.SQLITE_OK

    with _reading(session, name, text_factory=_lossy) as con:
        con.set_authorizer(gate)
        try:
            rows = con.execute(sql).fetchall()
        except sqlite3.ProgrammingError as exc:
            raise invalid("file_query sql is exactly one SQL statement") from exc
        except sqlite3.Error as exc:
            code = getattr(exc, "sqlite_errorname", "")
            if seen["denied"]:
                raise invalid("file_query sql only reads: one SELECT") from exc
            if code in _UNREADABLE:
                raise _malformed(f"{name} is not a readable SQLite database") from exc
            raise invalid(f"the file_query statement failed on {name} ({code})") from exc
    if not seen["select"]:
        raise invalid("file_query sql only reads: one SELECT")
    return {"rows": [[_value(v) for v in row] for row in rows]}
