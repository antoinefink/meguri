# SPDX-License-Identifier: AGPL-3.0-or-later
"""Notes.

add_note / setup.notes: {notetype, deck?, fields: [str] | {field: str}, tags?: [str]}
Each note gets the next #nN; its cards get the next #cN in template order.
Field values and tags must be strings: str(1.0) or str(True) is Python's text,
which a Meguri adapter would not reproduce.

edit_note: {note: #nN, fields?, tags?}   (record-ops.md W2) the stored note,
    loaded when the step runs; a fields list replaces from the first field, a
    mapping the named fields; tags replace all tags. Saved with the
    collection's note update (undoable). A deleted note is Anki's NotFoundError
    (anki_error).
delete_notes: {notes: [#nN, ...]}   (record-ops.md W3) the collection's note
    removal (undoable) with the listed notes that still exist; when none does,
    nothing is called.
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import op, setup_section
from .decks import deck_id

NOTE_KEYS = {"notetype", "deck", "fields", "tags"}
EDIT_KEYS = {"note", "fields", "tags"}


def _text(value, what: str) -> str:
    if not isinstance(value, str):
        raise invalid(f"{what} are strings; quote numbers, booleans and null")
    return value


def _set_fields(note, fields, owner: str) -> None:
    """fields as add_note takes them, onto note; owner names its note type."""
    if isinstance(fields, list):
        if len(fields) > len(note.fields):
            raise invalid(f"{owner} has {len(note.fields)} fields")
        for i, value in enumerate(fields):
            note.fields[i] = _text(value, "field values")
    elif isinstance(fields, dict):
        for key, value in fields.items():
            if key not in note.keys():
                raise invalid(f"{owner} has no field {key!r}")
            note[key] = _text(value, "field values")
    else:
        raise invalid("fields is a list or a mapping")


def _tags(tags) -> list[str]:
    if not isinstance(tags, list):
        raise invalid("tags is a list")
    return [_text(t, "tags") for t in tags]


def add(session, spec) -> dict:
    if not isinstance(spec, dict):
        raise invalid("a note is a mapping")
    unknown = sorted(set(spec) - NOTE_KEYS)
    if unknown:
        raise invalid(f"unknown note keys: {', '.join(unknown)}")
    name = spec.get("notetype")
    if name not in session.notetypes:
        raise invalid(f"note type {name!r} is not in setup.notetypes")
    col = session.col
    note = col.new_note(col.models.get(session.notetypes[name]))
    _set_fields(note, spec.get("fields", []), f"note type {name!r}")
    note.tags = _tags(spec.get("tags", []))
    col.add_note(note, deck_id(session, spec.get("deck")))
    session.notes.append(note.id)
    new_cards = col.db.list("select id from cards where nid = ? order by ord", note.id)
    session.cards.extend(new_cards)
    return {"note": f"#n{len(session.notes)}", "cards": [session.card_ref(c) for c in new_cards]}


@setup_section("notes", order=50)
def notes(session, items):
    if not isinstance(items, list):
        raise invalid("setup.notes is a list")
    for item in items:
        add(session, item)


@op("add_note")
def add_note(session, args):
    return add(session, args)


@op("edit_note")
def edit_note(session, args):
    if not isinstance(args, dict) or "note" not in args:
        raise invalid("edit_note takes {note, fields?, tags?}")
    unknown = sorted(set(args) - EDIT_KEYS)
    if unknown:
        raise invalid(f"unknown edit_note keys: {', '.join(unknown)}")
    ref = args["note"]
    nid = session.resolve(ref, "n")
    tags = _tags(args["tags"]) if "tags" in args else None
    note = session.col.get_note(nid)
    if "fields" in args:
        _set_fields(note, args["fields"], f"{ref}'s note type")
    if tags is not None:
        note.tags = tags
    session.col.update_note(note)


@op("delete_notes")
def delete_notes(session, args):
    if not isinstance(args, dict) or set(args) != {"notes"}:
        raise invalid("delete_notes takes {notes: [#nN, ...]}")
    refs = args["notes"]
    if not isinstance(refs, list) or not refs:
        raise invalid("delete_notes takes a list of at least one note")
    listed = list(dict.fromkeys(session.resolve(ref, "n") for ref in refs))
    marks = ", ".join("?" * len(listed))
    stored = set(session.col.db.list(f"select id from notes where id in ({marks})", *listed))
    existing = [nid for nid in listed if nid in stored]
    if existing:
        session.col.remove_notes(existing)
