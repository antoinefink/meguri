# SPDX-License-Identifier: AGPL-3.0-or-later
"""Notes.

add_note / setup.notes: {notetype, deck?, fields: [str] | {field: str}, tags?: [str]}
Each note gets the next #nN; its cards get the next #cN in template order.
Field values and tags must be strings: str(1.0) or str(True) is Python's text,
which a Meguri adapter would not reproduce.
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import op, setup_section
from .decks import deck_id

NOTE_KEYS = {"notetype", "deck", "fields", "tags"}


def _text(value, what: str) -> str:
    if not isinstance(value, str):
        raise invalid(f"{what} are strings; quote numbers, booleans and null")
    return value


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
    fields = spec.get("fields", [])
    if isinstance(fields, list):
        if len(fields) > len(note.fields):
            raise invalid(f"note type {name!r} has {len(note.fields)} fields")
        for i, value in enumerate(fields):
            note.fields[i] = _text(value, "field values")
    elif isinstance(fields, dict):
        for key, value in fields.items():
            if key not in note.keys():
                raise invalid(f"note type {name!r} has no field {key!r}")
            note[key] = _text(value, "field values")
    else:
        raise invalid("fields is a list or a mapping")
    tags = spec.get("tags", [])
    if not isinstance(tags, list):
        raise invalid("tags is a list")
    note.tags = [_text(t, "tags") for t in tags]
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
