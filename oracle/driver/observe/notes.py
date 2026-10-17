# SPDX-License-Identifier: AGPL-3.0-or-later
"""fields_check {cases}: the field check on unsaved note drafts (notes area).
Request, value and error kinds: specs/conformance/note-ops.md N4; the behavior
is specs/notes/field-check.md K1-K4.

Value: [result, ...], one per case in case order:
  {state}                                  normal, empty, missing_cloze,
                                           notetype_not_cloze, field_not_cloze
  {state: duplicate, duplicates: [#nN]}    the notes of K4

A case's draft is a note object that is never saved:
  {notetype, fields}  a new note (id 0) of the note type with exactly that
                      stored name, fields set as add_note sets them (a list
                      from the first field, a mapping by name).
  {note, fields?}     the stored note, loaded when the step runs, fields
                      replaced as edit_note replaces them.

The state is the backend's note fields check on the draft. Only for
`duplicate`, the duplicates come from Anki's dupe search, the Dupe search node
for the draft's note type and raw first field (as the editor's Show Duplicates
builds it), less the draft's own note, as logical ids in ascending note id
order. The search normalizes and strips the text as the check does, but the
text travels through a search string, whose parser turns white space other
than U+0020 into spaces (and drops it at the end of an unquoted term). The
driver parses the string back and refuses (internal_error) a first field the
string does not carry intact, and a duplicate state for which the search finds
no other note, rather than record a list that is not the check's.

Every request's shape is checked before any collection exists; note types,
notes and field names when the step runs. The scenario's collection never
changes.
"""

from __future__ import annotations

import re
from typing import Any

from anki.collection import SearchNode
from anki.notes import NoteFieldsCheckResult

from ..errors import DriverError, invalid
from ..ops.notes import set_fields
from ..registry import observation, scenario_check
from ..session import parse_observation

NEW_NOTE_KEYS = {"notetype", "fields"}
STORED_NOTE_KEYS = {"note", "fields"}
NOTE_REF = re.compile(r"#n[1-9][0-9]*")

STATES = {
    NoteFieldsCheckResult.NORMAL: "normal",
    NoteFieldsCheckResult.EMPTY: "empty",
    NoteFieldsCheckResult.DUPLICATE: "duplicate",
    NoteFieldsCheckResult.MISSING_CLOZE: "missing_cloze",
    NoteFieldsCheckResult.NOTETYPE_NOT_CLOZE: "notetype_not_cloze",
    NoteFieldsCheckResult.FIELD_NOT_CLOZE: "field_not_cloze",
}


# --- request checks (no collection) --------------------------------------------


def _check_fields(fields: Any) -> None:
    if isinstance(fields, list):
        values = fields
    elif isinstance(fields, dict):
        values = list(fields.values())
    else:
        raise invalid("a case's fields are a list or a mapping of field name to value")
    if not all(isinstance(v, str) for v in values):
        raise invalid("field values are strings; quote numbers, booleans and null")


def _check_case(case: Any) -> None:
    if not isinstance(case, dict):
        raise invalid("a fields_check case is a mapping")
    if "note" in case:
        if set(case) - STORED_NOTE_KEYS:
            raise invalid("a stored-note case takes {note, fields?}")
        if not isinstance(case["note"], str) or not NOTE_REF.fullmatch(case["note"]):
            raise invalid("a stored-note case's note is a #nN reference")
    else:
        if set(case) != NEW_NOTE_KEYS:
            raise invalid("a new-note case takes {notetype, fields}")
        if not isinstance(case["notetype"], str) or not case["notetype"]:
            raise invalid("a new-note case's notetype is a note type name")
    if "fields" in case:
        _check_fields(case["fields"])


def check_request(args: Any) -> list[dict]:
    """note-ops.md N4's request shape, without the collection."""
    if not isinstance(args, dict) or set(args) != {"cases"}:
        raise invalid("fields_check takes {cases}")
    cases = args["cases"]
    if not isinstance(cases, list) or not cases:
        raise invalid("fields_check cases are a non-empty list")
    for case in cases:
        _check_case(case)
    return cases


@scenario_check
def check_fields_check_requests(scenario: dict) -> None:
    """At most one fields_check request per observe step (O2), each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe":
            continue
        requests = [args for kind, args in map(parse_observation, specs) if kind == "fields_check"]
        if len(requests) > 1:
            raise invalid("an observe step has at most one fields_check request", step=index)
        for args in requests:
            try:
                check_request(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


# --- drafts --------------------------------------------------------------------


def _notetype_id(col, name: str) -> int:
    """The note type stored under exactly this name (the column's unicase
    collation would also match other spellings)."""
    ids = [ntid for ntid, stored in col.db.all("select id, name from notetypes") if stored == name]
    if not ids:
        raise DriverError("unknown_ref", ref=name)
    return ids[0]


def _draft(session, case: dict):
    col = session.col
    if "note" in case:
        ref = case["note"]
        nid = session.resolve(ref, "n")
        if col.db.scalar("select 1 from notes where id = ?", nid) is None:
            raise DriverError("unknown_ref", ref=ref)
        note = col.get_note(nid)
        owner = f"{ref}'s note type"
    else:
        name = case["notetype"]
        note = col.new_note(col.models.get(_notetype_id(col, name)))
        owner = f"note type {name!r}"
    if "fields" in case:
        set_fields(note, case["fields"], owner)
    return note


def _duplicates(session, note) -> list[str]:
    col = session.col
    node = SearchNode(dupe=SearchNode.Dupe(notetype_id=note.mid, first_field=note.fields[0]))
    search = col.build_search_string(node)
    if col.build_search_string(search) != search:
        raise DriverError(
            "internal_error",
            observation="fields_check",
            detail="the dupe search string does not carry this first field intact",
        )
    nids = sorted(set(col.find_notes(search)) - {note.id})
    if not nids:
        raise DriverError(
            "internal_error",
            observation="fields_check",
            detail="the field check found a duplicate the dupe search does not",
        )
    return [session.note_ref(nid) for nid in nids]


def _result(session, note) -> dict:
    raw = note.fields_check()
    state = STATES.get(raw)
    if state is None:
        raise DriverError("internal_error", observation="fields_check", detail=f"unknown field check state {raw}")
    if state == "duplicate":
        return {"state": state, "duplicates": _duplicates(session, note)}
    return {"state": state}


# --- the observation -------------------------------------------------------------


@observation("fields_check")
def fields_check(session, args):
    drafts = [_draft(session, case) for case in check_request(args)]
    return [_result(session, note) for note in drafts]
