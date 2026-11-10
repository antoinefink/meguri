# SPDX-License-Identifier: AGPL-3.0-or-later
"""Browse's Find and Replace and Change Deck (specs/conformance/bulk-ops.md X1-X3).

find_replace: {notes?, find, replace, regex?, ignore_case?, in?} -> {count};
    rejections invalid_regex, space_in_replacement, field_count
change_deck:  {cards, deck} -> {count}; rejection filtered_deck
observe find_replace_fields {notes} -> [<field name>, ...]

find_replace is one call of the collection's find-and-replace (`in` all or a
field) or of the tag find-and-replace (`in: tags`), with the listed notes' real
ids in list order, or no id for all notes (Anki then takes every note), match
case = not ignore_case. Its count is Anki's. Deleted notes are passed on (Anki
skips them); a repeated reference is invalid_scenario before any call.

Anki refuses all three cases with the same invalid-input kind (its invalid
regex error has that kind too), so the driver tells them apart:
- tags: Anki checks the replacement for a separator (U+0020, U+3000) before it
  builds the pattern and refuses it outside its transaction; any other
  invalid-input failure is the pattern's. So a replacement holding a separator
  is space_in_replacement, anything else invalid_regex.
- fields: Anki compiles the pattern before it reads a note, and refuses a
  matched note whose field count differs while it writes. After an
  invalid-input failure the driver asks again with the same texts and flags and
  one note id no note has: only a pattern that does not compile fails again,
  and that is invalid_regex; otherwise field_count. Both failures roll back
  inside Anki's transaction, which empties its undo history and study queues,
  so the second call (no note, no change) changes nothing more.
Any other failure stays a run error.

change_deck is one call of the collection's set-deck with the listed real card
ids in list order and the deck's id (deck-ops.md K1: its stored human name,
read from the file at the step, so a filtered deck a db_exec inserted is
found). Anki refuses a filtered target with its filtered-deck error kind before
its transaction: filtered_deck.

find_replace_fields is the backend's field names for the listed notes' real
ids, as returned. Reading changes nothing. Request shapes are checked before
any collection exists, at most one request per observe step.
"""

from __future__ import annotations

from typing import Any

from anki import backend_pb2

from ..errors import DriverError, Rejected, invalid
from ..registry import observation, op, scenario_check
from ..session import parse_observation
from .decks import find_deck
from .model import KindProbe

INVALID_INPUT = backend_pb2.BackendError.INVALID_INPUT
FILTERED_DECK = backend_pb2.BackendError.FILTERED_DECK_ERROR
TAG_SEPARATORS = (" ", "　")
FIND_REPLACE_KEYS = {"notes", "find", "replace", "regex", "ignore_case", "in"}


class _Refused(Exception):
    """The backend's invalid-input kind, before the driver names it."""


def _distinct_refs(session, name: str, key: str, value: Any, kind: str) -> list[int]:
    """A non-empty list of references, each once, as real ids in order."""
    if not isinstance(value, list) or not value:
        raise invalid(f"{name} {key} is a non-empty list of references")
    ids = [session.resolve(ref, kind) for ref in value]
    seen: set[int] = set()
    for ref, real in zip(value, ids):
        if real in seen:
            raise invalid(f"{name} lists {ref} twice")
        seen.add(real)
    return ids


def _text(name: str, key: str, value: Any) -> str:
    if not isinstance(value, str):
        raise invalid(f"{name} {key} is a string; quote numbers, booleans and null")
    return value


def _flag(name: str, args: dict, key: str, default: bool) -> bool:
    value = args.get(key, default)
    if not isinstance(value, bool):
        raise invalid(f"{name} {key} is a boolean")
    return value


def _target(args: dict) -> str | None:
    """`in`: None for all fields, "" never; a field name; or the tags marker."""
    value = args.get("in", "all")
    if value == "all":
        return None
    if value == "tags":
        return value
    if isinstance(value, dict) and set(value) == {"field"}:
        field = value["field"]
        if isinstance(field, str) and field:
            return field
    raise invalid("find_replace in is all, tags or {field: <non-empty name>}")


def _call(session, method: str, kinds: dict, **kwargs):
    backend = session.col._backend
    return getattr(type(backend), method)(KindProbe(backend, kinds), **kwargs)


def _unused_note_id(session) -> int:
    return session.col.db.scalar("select coalesce(max(id), 0) + 1 from notes")


@op("find_replace", rejects=("invalid_regex", "space_in_replacement", "field_count"))
def find_replace(session, args):
    if not isinstance(args, dict) or not {"find", "replace"} <= set(args) or set(args) - FIND_REPLACE_KEYS:
        raise invalid("find_replace takes {notes?, find, replace, regex?, ignore_case?, in?}")
    note_ids = _distinct_refs(session, "find_replace", "notes", args["notes"], "n") if "notes" in args else []
    find = _text("find_replace", "find", args["find"])
    replace = _text("find_replace", "replace", args["replace"])
    regex = _flag("find_replace", args, "regex", False)
    match_case = not _flag("find_replace", args, "ignore_case", True)
    target = _target(args)
    refused = {INVALID_INPUT: _Refused}

    if target == "tags":
        try:
            changes = _call(session, "find_and_replace_tag", refused, note_ids=note_ids, search=find,
                            replacement=replace, regex=regex, match_case=match_case)
        except _Refused:
            spaced = any(sep in replace for sep in TAG_SEPARATORS)
            raise Rejected("space_in_replacement" if spaced else "invalid_regex") from None
        return {"count": changes.count}

    common = {"search": find, "replacement": replace, "regex": regex, "match_case": match_case,
              "field_name": target or ""}
    try:
        changes = _call(session, "find_and_replace", refused, nids=note_ids, **common)
    except _Refused:
        try:
            _call(session, "find_and_replace", refused, nids=[_unused_note_id(session)], **common)
        except _Refused:
            raise Rejected("invalid_regex") from None
        raise Rejected("field_count") from None
    return {"count": changes.count}


@op("change_deck", rejects=("filtered_deck",))
def change_deck(session, args):
    if not isinstance(args, dict) or set(args) != {"cards", "deck"}:
        raise invalid("change_deck takes {cards, deck}")
    card_ids = _distinct_refs(session, "change_deck", "cards", args["cards"], "c")
    deck_id = find_deck(session, args["deck"])

    def filtered():
        return Rejected("filtered_deck")

    changes = _call(session, "set_deck", {FILTERED_DECK: filtered}, card_ids=card_ids, deck_id=deck_id)
    return {"count": changes.count}


def _check_fields_request(args: Any) -> list:
    if not isinstance(args, dict) or set(args) != {"notes"}:
        raise invalid("find_replace_fields takes {notes}")
    notes = args["notes"]
    if not isinstance(notes, list) or not notes or len(set(map(repr, notes))) != len(notes):
        raise invalid("find_replace_fields notes is a non-empty list of distinct references")
    return notes


@scenario_check
def check_find_replace_fields_requests(scenario: dict) -> None:
    """At most one find_replace_fields request per observe step, each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        if not isinstance(step, dict) or len(step) != 1:
            continue
        ((name, specs),) = step.items()
        if name != "observe" or not isinstance(specs, list):
            continue
        requests = [args for kind, args in map(parse_observation, specs) if kind == "find_replace_fields"]
        if len(requests) > 1:
            raise invalid("an observe step has at most one find_replace_fields request", step=index)
        for args in requests:
            try:
                _check_fields_request(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


@observation("find_replace_fields")
def find_replace_fields(session, args):
    refs = _check_fields_request(args)
    note_ids = _distinct_refs(session, "find_replace_fields", "notes", refs, "n")
    return list(session.col._backend.field_names_for_notes(note_ids))
