# SPDX-License-Identifier: AGPL-3.0-or-later
"""Deck packages (specs/conformance/package-ops.md E6, E7).

import, deck package form {format: apkg, fixture | name, merge_notetypes,
    update_notes, update_notetypes, with_scheduling, with_deck_configs}
    (ops/colpkg.py dispatches here) -> the import log {found_notes, new,
    updated, duplicate, conflicting, missing_notetype, missing_deck,
    first_field_match, empty_first_field}; rejections too_new, corrupt,
    notetype_kind_conflict. Anki's deck package import of the file with those
    options into the open session collection, its media folder and media
    database; nothing closes. The import is one undoable "Import" step and
    writes the five option keys (Anki does both). Notes and cards it adds get
    no logical id; a log entry names a target note by its #nN, else by its real
    id as an integer, and conflicting and missing_notetype entries by the
    package note's id (E6.3).
export, deck package form {format: apkg, name, limit, scheduling, presets,
    media} (ops/package.py dispatches here) -> {notes: <count>}; rejections
    invalid_ids, media_check_required. Anki's deck package export with that
    limit and those options, older-client support off, to <session dir>/<name>,
    replacing a file of that name (Anki's own atomic rename); the collection
    stays open. limit: collection | {deck: <stored name>} | {notes: [#nN...]} |
    {cards: [#cN...]}.

Refusals are read from the backend error as ops/colpkg.py reads them (a probe
in place of the RustBackend in one generated call): the import's are its
import errors told apart by the backend's own translations ("too new", "not a
valid file", "cannot merge note types of different kinds"), plus the error
kinds the collection package import maps to corrupt and a JSON error (a legacy
media map that does not parse); the export's are invalid input whose message
is the backend's "please check database" (ids more than a day ahead) or
"please check media" translation. Anki rolls a refused import back (one
transaction, which also empties its undo history and study queues; only a
file that does not read as zip is refused before it and keeps them, E6.4) and
writes no package for a refused export. Any other failure stays a run error.
No message leaves the driver (R1887).

A scenario that writes note types, decks or presets with db_exec runs `reopen`
before an import or export reads them (Anki's caches survive db_exec; the
driver does not reopen, which would empty the undo history an export or a
successful import keeps).
"""

from __future__ import annotations

import os
import re
from typing import Any

from anki import backend_pb2
from anki._backend import backend_exception_to_pylib
from anki.collection import CardIdsLimit, DeckIdLimit, NoteIdsLimit, pb_export_limit
from anki.import_export_pb2 import ExportAnkiPackageOptions, ImportAnkiPackageOptions, ImportAnkiPackageUpdateCondition

from ..errors import DriverError, Rejected, invalid
from .colpkg import _CORRUPT_KINDS, _ErrorProbe, _Failed
from .decks import find_deck
from .storage_open import fixture_path, fixture_rel

# conformance/schema/scenario.schema.json package_name; fullmatch, since `$`
# also matches before a final newline.
APKG_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*\.apkg")
_ERR = backend_pb2.BackendError
UPDATE = {
    "if_newer": ImportAnkiPackageUpdateCondition.IMPORT_ANKI_PACKAGE_UPDATE_CONDITION_IF_NEWER,
    "always": ImportAnkiPackageUpdateCondition.IMPORT_ANKI_PACKAGE_UPDATE_CONDITION_ALWAYS,
    "never": ImportAnkiPackageUpdateCondition.IMPORT_ANKI_PACKAGE_UPDATE_CONDITION_NEVER,
}
BOOL_OPTIONS = ("merge_notetypes", "with_scheduling", "with_deck_configs")
UPDATE_OPTIONS = ("update_notes", "update_notetypes")
IMPORT_KEYS = {"format", *BOOL_OPTIONS, *UPDATE_OPTIONS}
EXPORT_KEYS = {"format", "name", "limit", "scheduling", "presets", "media"}
LOG_LISTS = (
    "new", "updated", "duplicate", "conflicting", "missing_notetype", "missing_deck",
    "first_field_match", "empty_first_field",
)
# E6.3: these lists name the package note, the others the target note.
PACKAGE_IDS = ("conflicting", "missing_notetype")
_IMPORT_CORRUPT_KINDS = _CORRUPT_KINDS | {_ERR.JSON_ERROR}


def apkg_name(value: Any, what: str) -> str:
    if not isinstance(value, str) or not APKG_NAME.fullmatch(value):
        raise invalid(f"{what} is a file name of ASCII letters, digits, '.', '_' and '-' that ends in .apkg")
    return value


def _flag(args: dict, key: str) -> bool:
    if not isinstance(args[key], bool):
        raise invalid(f"{key} is true or false")
    return args[key]


# --- import -------------------------------------------------------------------


def import_form(args: Any) -> tuple[str, str, ImportAnkiPackageOptions]:
    """('fixture' | 'name', value, options) of a deck package import (E6.1)."""
    keys = set(args) if isinstance(args, dict) else set()
    if keys - {"fixture", "name"} != IMPORT_KEYS or len(keys & {"fixture", "name"}) != 1 or args["format"] != "apkg":
        raise invalid("the deck package import is {format: apkg, fixture | name, merge_notetypes, update_notes, "
                      "update_notetypes, with_scheduling, with_deck_configs}, every option given")
    if "fixture" in args:
        source = ("fixture", fixture_rel(args["fixture"], (".apkg",), "import fixture"))
    else:
        source = ("name", apkg_name(args["name"], "import name"))
    for key in UPDATE_OPTIONS:
        if not isinstance(args[key], str) or args[key] not in UPDATE:
            raise invalid(f"{key} is if_newer, always or never")
    options = ImportAnkiPackageOptions(
        merge_notetypes=_flag(args, "merge_notetypes"),
        update_notes=UPDATE[args["update_notes"]],
        update_notetypes=UPDATE[args["update_notetypes"]],
        with_scheduling=_flag(args, "with_scheduling"),
        with_deck_configs=_flag(args, "with_deck_configs"),
    )
    return source[0], source[1], options


def _import_refusal(tr, error: backend_pb2.BackendError) -> str | None:
    if error.kind == _ERR.IMPORT_ERROR:
        if error.message == tr.errors_collection_too_new():
            return "too_new"
        if error.message == tr.importing_the_provided_file_is_not_a():
            return "corrupt"
        if error.message == tr.importing_cannot_merge_notetypes_of_different_kinds():
            return "notetype_kind_conflict"
        return None
    return "corrupt" if error.kind in _IMPORT_CORRUPT_KINDS else None


def _log_entry(session, note, target: bool) -> dict:
    nid = note.id.nid
    ref: Any = nid
    if target and nid in session.notes:
        ref = f"#n{session.notes.index(nid) + 1}"
    return {"note": ref, "fields": list(note.fields)}


def import_package(session, args) -> dict:
    kind, value, options = import_form(args)
    if kind == "fixture":
        package = fixture_path(value, (".apkg",), "import fixture")
    else:
        package = os.path.join(session.dir, value)
        if not os.path.isfile(package):
            raise DriverError("unknown_ref", ref=value)
    col = session.col
    backend = col._backend
    try:
        response = type(backend).import_anki_package(_ErrorProbe(backend), package_path=package, options=options)
    except _Failed as exc:
        refusal = _import_refusal(col.tr, exc.error)
        if refusal is None:
            raise backend_exception_to_pylib(exc.error) from None
        raise Rejected(refusal) from None
    log = response.log
    out: dict[str, Any] = {"found_notes": log.found_notes}
    for key in LOG_LISTS:
        out[key] = [_log_entry(session, note, key not in PACKAGE_IDS) for note in getattr(log, key)]
    return out


# --- export -------------------------------------------------------------------


def _limit(session, value: Any):
    if value == "collection":
        return None
    if not isinstance(value, dict) or len(value) != 1:
        raise invalid("export limit is collection, {deck}, {notes} or {cards}")
    ((key, item),) = value.items()
    if key == "deck":
        return DeckIdLimit(find_deck(session, item, "export limit deck"))
    if key in ("notes", "cards"):
        if not isinstance(item, list) or not item:
            raise invalid(f"export limit {key} is a non-empty list of references")
        ids = [session.resolve(ref, key[0]) for ref in item]
        return NoteIdsLimit(ids) if key == "notes" else CardIdsLimit(ids)
    raise invalid("export limit is collection, {deck}, {notes} or {cards}")


def check_export_form(args: dict) -> str:
    """The file name of a deck package export (E7); invalid_scenario otherwise."""
    if set(args) != EXPORT_KEYS or args["format"] != "apkg":
        raise invalid("the deck package export is exactly {format: apkg, name, limit, scheduling, presets, media}")
    for key in ("scheduling", "presets", "media"):
        _flag(args, key)
    limit = args["limit"]
    if limit != "collection" and not (isinstance(limit, dict) and len(limit) == 1 and set(limit) <= {"deck", "notes", "cards"}):
        raise invalid("export limit is collection, {deck}, {notes} or {cards}")
    return apkg_name(args["name"], "export name")


def _export_refusal(tr, error: backend_pb2.BackendError) -> str | None:
    if error.kind == _ERR.INVALID_INPUT:
        if error.message == tr.errors_please_check_database():
            return "invalid_ids"
        if error.message == tr.errors_please_check_media():
            return "media_check_required"
    return None


def export(session, args) -> dict:
    name = check_export_form(args)
    limit = _limit(session, args["limit"])
    options = ExportAnkiPackageOptions(
        with_scheduling=args["scheduling"], with_deck_configs=args["presets"], with_media=args["media"], legacy=False
    )
    col = session.col
    backend = col._backend
    try:
        notes = type(backend).export_anki_package(
            _ErrorProbe(backend), out_path=os.path.join(session.dir, name), options=options, limit=pb_export_limit(limit)
        )
    except _Failed as exc:
        refusal = _export_refusal(col.tr, exc.error)
        if refusal is None:
            raise backend_exception_to_pylib(exc.error) from None
        raise Rejected(refusal) from None
    return {"notes": notes}
