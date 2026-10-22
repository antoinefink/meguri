# SPDX-License-Identifier: AGPL-3.0-or-later
"""Legacy packages (model-ops.md L6, L7) and the export op's forms.

export (op): the legacy package form {format: "apkg", name, legacy: true}
    -> {notes: <count>}; the collection package form {format: "colpkg", name}
    -> none (package-ops.md E1, ops/colpkg.py)
package_col (observation): {name, columns: [ver | conf | models | decks | dconf | tags]}
    -> {<column>: value}

The legacy form writes the whole collection (limit none) with Anki's package
exporter, scheduling and presets in, media out, in the legacy layout, to
<session dir>/<name>: Session.hand_off copies that directory, so the package
crosses a handoff. The other export forms are the package area's drafts
(dsl.md D7.2): a scenario that uses one fails with unsupported_op before any
collection exists, as it did when the driver had no export.

package_col never opens the package's collection with Anki (opening upgrades it
to schema 18, which empties the JSON columns). It picks the entry by the meta
version (1: collection.anki2, 2: collection.anki21, any other: invalid_scenario)
or, without meta, collection.anki21 when present else collection.anki2, copies
it to the worker's scratch and reads the col row with Python's sqlite3. JSON
columns parse as legacy_json's objects do, so ints stay ints and floats stay
floats. L7 makes an archive that does not read, a missing entry and a column
that is not JSON run errors without naming a kind; protocol.json has none for a
malformed package, so they are internal_error with a detail.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sqlite3
import tempfile
import zipfile
from typing import Any

from anki.collection import ExportAnkiPackageOptions
from anki.import_export_pb2 import PackageMetadata
from google.protobuf.message import DecodeError

from ..errors import DriverError, invalid
from ..registry import observation, op, scenario_check
from . import colpkg

# conformance/schema/scenario.schema.json package_name; fullmatch, since `$`
# also matches before a final newline.
PACKAGE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*\.apkg")
LEGACY_KEYS = {"format", "name", "legacy"}
COLUMNS = ("ver", "conf", "models", "decks", "dconf", "tags")
ENTRY_BY_VERSION = {1: "collection.anki2", 2: "collection.anki21"}


def _package_name(value: Any, what: str) -> str:
    if not isinstance(value, str) or not PACKAGE_NAME.fullmatch(value):
        raise invalid(f"{what} is a file name of ASCII letters, digits, '.', '_' and '-' that ends in .apkg")
    return value


def _export_form(args: Any) -> tuple[str, str]:
    """('legacy' | 'colpkg', file name) of a form the driver has;
    unsupported_op for a draft form."""
    if isinstance(args, dict) and args.get("format") == "colpkg" and "legacy" not in args:
        if set(args) != {"format", "name"}:
            raise invalid("the collection package export form is exactly {format: colpkg, name}")
        return "colpkg", colpkg.colpkg_name(args["name"], "export name")
    if isinstance(args, dict) and "legacy" not in args:
        raise DriverError("unsupported_op", op="export")
    if (
        not isinstance(args, dict)
        or set(args) != LEGACY_KEYS
        or args["format"] != "apkg"
        or args["legacy"] is not True
    ):
        raise invalid("the legacy export form is exactly {format: apkg, name, legacy: true}")
    return "legacy", _package_name(args["name"], "export name")


@scenario_check
def check_export_steps(scenario: dict) -> None:
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, args),) = step.items()
        if name == "export":
            try:
                _export_form(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


@op("export")
def export(session, args):
    form, name = _export_form(args)
    if form == "colpkg":
        colpkg.export(session, name)
        return None
    options = ExportAnkiPackageOptions(with_scheduling=True, with_deck_configs=True, with_media=False, legacy=True)
    notes = session.col.export_anki_package(out_path=os.path.join(session.dir, name), options=options, limit=None)
    return {"notes": notes}


def _requested(args: Any) -> tuple[str, list[str]]:
    if not isinstance(args, dict) or set(args) != {"name", "columns"}:
        raise invalid("package_col takes {name, columns}")
    name = _package_name(args["name"], "package_col name")
    columns = args["columns"]
    if (
        not isinstance(columns, list)
        or not columns
        or not all(isinstance(c, str) and c in COLUMNS for c in columns)
        or len(set(columns)) != len(columns)
    ):
        raise invalid(f"package_col columns is a non-empty list, without repeats, of {', '.join(COLUMNS)}")
    return name, columns


def _bad_package(detail: str) -> DriverError:
    return DriverError("internal_error", detail=detail)


def _entry(archive: zipfile.ZipFile) -> str:
    names = set(archive.namelist())
    if "meta" in names:
        meta = PackageMetadata()
        try:
            meta.ParseFromString(archive.read("meta"))
        except DecodeError as exc:
            raise _bad_package("the package's meta entry is not a package metadata message") from exc
        if meta.version not in ENTRY_BY_VERSION:
            raise invalid(f"package version {meta.version} holds no schema-11 collection (model-ops.md L7)")
        entry = ENTRY_BY_VERSION[meta.version]
    else:
        entry = "collection.anki21" if "collection.anki21" in names else "collection.anki2"
    if entry not in names:
        raise _bad_package(f"the package has no {entry} entry")
    return entry


def _col_row(session, path: str, columns: list[str]) -> tuple:
    try:
        archive = zipfile.ZipFile(path)
    except zipfile.BadZipFile as exc:
        raise _bad_package("the package is not a zip archive") from exc
    scratch = tempfile.mkdtemp(prefix="package-", dir=session.scratch)
    try:
        with archive:
            db = os.path.join(scratch, "collection")
            with archive.open(_entry(archive)) as src, open(db, "wb") as dst:
                shutil.copyfileobj(src, dst)
        con = sqlite3.connect(db)
        try:
            rows = con.execute(f"SELECT {', '.join(columns)} FROM col").fetchall()
        except sqlite3.DatabaseError as exc:
            raise _bad_package("the package's collection entry has no readable col table") from exc
        finally:
            con.close()
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    if len(rows) != 1:
        raise _bad_package(f"the package's col table has {len(rows)} rows")
    return rows[0]


def _unique_members(pairs: list[tuple[str, Any]]) -> dict:
    obj = dict(pairs)
    if len(obj) != len(pairs):
        # The observation's JSON cannot hold both members.
        raise _bad_package("a JSON column of the package repeats a member name")
    return obj


def _value(column: str, stored: Any) -> Any:
    if column == "ver":
        if not isinstance(stored, int):
            raise _bad_package("the package's col.ver is not an integer")
        return stored
    if not isinstance(stored, str):
        raise _bad_package(f"the package's col.{column} is not text")
    try:
        return json.loads(stored, object_pairs_hook=_unique_members)
    except json.JSONDecodeError as exc:
        raise _bad_package(f"the package's col.{column} is not JSON") from exc


@observation("package_col")
def package_col(session, args):
    name, columns = _requested(args)
    path = os.path.join(session.dir, name)
    if not os.path.isfile(path):
        raise DriverError("unknown_ref", ref=name)
    row = _col_row(session, path, columns)
    return {column: _value(column, stored) for column, stored in zip(columns, row)}
