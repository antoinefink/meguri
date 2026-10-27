# SPDX-License-Identifier: AGPL-3.0-or-later
"""Change Note Type (specs/conformance/change-notetype-ops.md H1-H3): the
change screen's info query and its save, as Browse and the screen call them.

change_notetype_info: {notes: [#nN, ...], notetype: <stored name>}
    -> {old_notetype, new_notetype, old_fields, new_fields, old_templates,
        new_templates, fields, templates, discarded_fields, dropped_templates,
        removed_cards, field_content, card_counts, schema};
    rejections not_found, multiple_notetypes
change_notetype: {fields?: [old ordinal | null], templates?: [...]}
    -> {notes: <count>}; rejections no_changes, schema_changed, not_found

The selection check is Browse's single-note-type check done in SQL (pylib
raises its multiple-note-types error as a plain invalid input): the first
listed note missing is not_found; otherwise the notes of the first note's note
type among the listed ids must be as many as the list, so a missing later
note, one of another note type, or a note listed twice (M11) is
multiple_notetypes. Then the model manager's change info for the two note
types gives the names, the default maps (-1 for none, no template entries
when either note type is cloze) and the schema stamp. The loss preview is the
screen's unused items for the default maps; removed_cards, field_content and
card_counts come from the stored rows by M5 and M6.

Each info step first forgets what the session kept, then, unless refused,
keeps the selection, both note types' sizes, the default maps and the info's
request (stamp, note type ids, old name) for the change. After the selection
check (so a refusal leaves Anki's undo history alone) it reopens the
collection: Anki's change info reads note types through the backend's cache,
which a db_exec does not clear. The kept info lives in the worker's memory
(`Actor.data`): a resumed segment starts without it, and a scenario check
refuses a change_notetype with no change_notetype_info step since the start
or the latest handoff, so a whole run refuses what a live chain cannot run.

change_notetype checks the maps against the kept note types (invalid_scenario
otherwise), then refuses no_changes when the target is the old note type, the
field map is the identity and the template map the identity or absent (M7;
the screen's own check saves the absent case, a cloze note type changed to
itself, which no scenario does), then schema_changed when col.scm is not the kept
stamp (Anki's own refusal is a generic invalid input), then not_found when a
kept note no longer exists (Anki's next check, made here before the reopen so
the refusal leaves Anki's undo history alone), then reopens (the change reads
both note types, and card generation the templates, through the cache) and
runs the model manager's change of note type with the kept request, the
selection and the maps. Anki's not-found error is still not_found; Anki rolls
the change back. Cards the change creates get no logical id.
"""

from __future__ import annotations

from typing import Any

import anki.errors
from anki import notetypes_pb2

from ..errors import Rejected, invalid
from ..registry import op, scenario_check
from .notetypes import KINDS, notetype_id

KEY = "change_notetype"
INFO_KEYS = {"notes", "notetype"}
CHANGE_KEYS = {"fields", "templates"}
FIELD_SEPARATOR = "\x1f"
CLOZE = KINDS["cloze"]


def _uint(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _from_wire(values) -> list[int | None]:
    return [None if v == -1 else v for v in values]


def _to_wire(values: list[int | None]) -> list[int]:
    return [-1 if v is None else v for v in values]


def _unused(mapping: list[int | None], count: int) -> list[int]:
    used = {v for v in mapping if v is not None}
    return [i for i in range(count) if i not in used]


def _marks(ids: list[int]) -> str:
    return ", ".join("?" * len(ids))


def _selection(session, refs: Any) -> list[int]:
    if not isinstance(refs, list) or not refs:
        raise invalid("change_notetype_info notes is a list of at least one #nN")
    return [session.resolve(ref, "n") for ref in refs]


def _old_notetype(session, nids: list[int]) -> int:
    """Browse's single-note-type check (M4): the first note's note type."""
    db = session.col.db
    old = db.scalar("select mid from notes where id = ?", nids[0])
    if old is None:
        raise Rejected("not_found")
    matching = db.scalar(f"select count() from notes where mid = ? and id in ({_marks(nids)})", old, *nids)
    if matching != len(nids):
        raise Rejected("multiple_notetypes")
    return old


def _kind(session, ntid: int) -> int:
    return session.col._backend.get_notetype(ntid).config.kind


def _field_content(session, nids: list[int], field_count: int) -> list[int]:
    counts = [0] * field_count
    for (flds,) in session.col.db.all(f"select flds from notes where id in ({_marks(nids)})", *nids):
        parts = flds.split(FIELD_SEPARATOR)
        for i in range(min(field_count, len(parts))):
            if parts[i] != "":
                counts[i] += 1
    return counts


def _card_counts(session, nids: list[int]) -> list[list[int]]:
    rows = session.col.db.all(
        f"select ord, count() from cards where nid in ({_marks(nids)}) group by ord order by ord", *nids
    )
    return [[ord_, count] for ord_, count in rows]


def _removed_cards(card_counts, dropped, old_kind: int, new_kind: int, new_templates: int) -> int:
    """M6: the cards the change would delete for the default maps."""
    if dropped is not None:
        return sum(count for ord_, count in card_counts if ord_ in dropped)
    if old_kind == CLOZE and new_kind != CLOZE:
        return sum(count for ord_, count in card_counts if ord_ >= new_templates)
    return 0


@op("change_notetype_info", rejects=("not_found", "multiple_notetypes"))
def change_notetype_info(session, args):
    session.current.data.pop(KEY, None)
    if not isinstance(args, dict) or set(args) != INFO_KEYS:
        raise invalid("change_notetype_info takes {notes: [#nN, ...], notetype}")
    refs = _selection(session, args["notes"])
    new = notetype_id(session, args["notetype"])
    old = _old_notetype(session, refs)
    nids = list(dict.fromkeys(refs))
    session.reopen()
    info = session.col.models.change_notetype_info(old_notetype_id=old, new_notetype_id=new)
    request = info.input
    fields = _from_wire(request.new_fields)
    templates = _from_wire(request.new_templates) if len(request.new_templates) else None
    old_kind, new_kind = _kind(session, old), _kind(session, new)
    counts = _card_counts(session, nids)
    dropped = None if templates is None else _unused(templates, len(info.old_template_names))
    session.current.data[KEY] = {
        "request": request,
        "notes": nids,
        "same_notetype": old == new,
        "sizes": (len(info.old_field_names), len(info.new_field_names),
                  len(info.old_template_names), len(info.new_template_names)),
        "fields": fields,
        "templates": templates,
    }
    return {
        "old_notetype": info.old_notetype_name,
        "new_notetype": session.col.db.scalar("select name from notetypes where id = ?", new),
        "old_fields": list(info.old_field_names),
        "new_fields": list(info.new_field_names),
        "old_templates": list(info.old_template_names),
        "new_templates": list(info.new_template_names),
        "fields": fields,
        "templates": templates,
        "discarded_fields": _unused(fields, len(info.old_field_names)),
        "dropped_templates": dropped,
        "removed_cards": _removed_cards(counts, dropped, old_kind, new_kind, len(info.new_template_names)),
        "field_content": _field_content(session, nids, len(info.old_field_names)),
        "card_counts": counts,
        "schema": request.current_schema,
    }


def _map(value: Any, what: str, length: int, old_count: int, unique: bool) -> list[int | None]:
    if not isinstance(value, list) or len(value) != length:
        raise invalid(f"change_notetype {what} has one entry per {what[:-1]} of the new note type ({length})")
    for entry in value:
        if entry is not None and (not _uint(entry) or entry >= old_count):
            raise invalid(f"change_notetype {what} entries are null or an old ordinal below {old_count}")
    chosen = [entry for entry in value if entry is not None]
    if unique and len(set(chosen)) != len(chosen):
        raise invalid(f"change_notetype {what} names an old ordinal twice")
    return value


@op("change_notetype", rejects=("no_changes", "schema_changed", "not_found"))
def change_notetype(session, args):
    if not isinstance(args, dict) or set(args) - CHANGE_KEYS:
        raise invalid("change_notetype takes {fields?, templates?}")
    kept = session.current.data.get(KEY)
    if kept is None:
        raise invalid("change_notetype needs an earlier successful change_notetype_info step")
    old_fields, new_fields, old_templates, new_templates = kept["sizes"]
    fields = kept["fields"]
    if "fields" in args:
        fields = _map(args["fields"], "fields", new_fields, old_fields, unique=False)
    templates = kept["templates"]
    if "templates" in args:
        if templates is None:
            raise invalid("change_notetype takes no templates when either note type is cloze")
        templates = _map(args["templates"], "templates", new_templates, old_templates, unique=True)
    identity = lambda m: m == list(range(len(m)))  # noqa: E731
    if kept["same_notetype"] and identity(fields) and (templates is None or identity(templates)):
        raise Rejected("no_changes")
    request = notetypes_pb2.ChangeNotetypeRequest()
    request.CopyFrom(kept["request"])
    if request.current_schema != session.col.db.scalar("select scm from col"):
        raise Rejected("schema_changed")
    nids = kept["notes"]
    if session.col.db.scalar(f"select count() from notes where id in ({_marks(nids)})", *nids) != len(nids):
        raise Rejected("not_found")
    session.reopen()
    request.note_ids[:] = kept["notes"]
    request.new_fields[:] = _to_wire(fields)
    request.new_templates[:] = _to_wire(templates) if templates is not None else []
    try:
        session.col.models.change_notetype_of_notes(request)
    except anki.errors.NotFoundError:
        raise Rejected("not_found") from None
    return {"notes": len(kept["notes"])}


@scenario_check
def check_info_before_change(scenario: dict) -> None:
    """H1/H2: a change_notetype acts on a change_notetype_info step of its own
    segment. A handoff forgets the kept info (the other side never had it)."""
    seen = False
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, _args),) = step.items()
        if name == "handoff":
            seen = False
        elif name == "change_notetype_info":
            seen = True
        elif name == "change_notetype" and not seen:
            raise invalid("change_notetype needs a change_notetype_info step since the start "
                          "or the latest handoff", step=index)
