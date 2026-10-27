# SPDX-License-Identifier: AGPL-3.0-or-later
"""Empty Cards (specs/conformance/empty-cards-ops.md Y1-Y3): the scan and the
delete, as Anki's Empty Cards screen calls them.

empty_cards: {} -> [{notetype, notes: [{note, ords, cards, all_empty}]}]
delete_empty_cards: {keep_notes: bool} -> {deleted, kept_valid: 0}

empty_cards reopens the collection first (the report reads note types and
their templates through the backend's cache, which a db_exec does not clear),
then calls the collection's empty-cards report and keeps its notes
(note id, the empty card ids, already sorted by ordinal then id, and the
all-empty flag). The output reads each listed card's ordinal and the note's
card count from the rows and groups the notes, in the report's order, under
their note type's stored name; `note` is the logical id, or the real id as an
integer for a note without one. The report's text is not observed (labels are
display text).

delete_empty_cards builds the screen's list from the kept report: every
listed card, but for a note flagged all empty with keep_notes all but its
first listed card. One call of the collection's removal of cards and the notes
they leave empty (undoable, op Empty Cards); `deleted` is the count it returns
(listed cards that no longer exist are skipped), `kept_valid` is always 0:
Anki does not look again (E5). The kept report lives in the worker's memory
(`Actor.data`): a resumed segment starts without it, and a scenario check
refuses a delete_empty_cards with no empty_cards step since the start or the
latest handoff, so a whole run refuses what a live chain cannot run.
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import op, scenario_check

KEY = "empty_cards"


def _note_value(session, nid: int):
    return f"#n{session.notes.index(nid) + 1}" if nid in session.notes else nid


@op("empty_cards")
def empty_cards(session, args):
    if args != {}:
        raise invalid("empty_cards takes {}")
    session.reopen()
    report = session.col.get_empty_cards()
    kept = [(note.note_id, list(note.card_ids), note.will_delete_note) for note in report.notes]
    session.current.data[KEY] = kept
    db = session.col.db
    groups: list[tuple[int, list[dict]]] = []
    for nid, cids, all_empty in kept:
        ntid = db.scalar("select mid from notes where id = ?", nid)
        ords = dict(db.all("select id, ord from cards where nid = ?", nid))
        entry = {
            "note": _note_value(session, nid),
            "ords": [ords[cid] for cid in cids],
            "cards": len(ords),
            "all_empty": all_empty,
        }
        if groups and groups[-1][0] == ntid:
            groups[-1][1].append(entry)
        else:
            groups.append((ntid, [entry]))
    return [
        {"notetype": db.scalar("select name from notetypes where id = ?", ntid), "notes": notes}
        for ntid, notes in groups
    ]


@op("delete_empty_cards")
def delete_empty_cards(session, args):
    if not isinstance(args, dict) or set(args) != {"keep_notes"} or not isinstance(args["keep_notes"], bool):
        raise invalid("delete_empty_cards takes {keep_notes: bool}")
    kept = session.current.data.get(KEY)
    if kept is None:
        raise invalid("delete_empty_cards needs an earlier empty_cards step")
    listed: list[int] = []
    for _nid, cids, all_empty in kept:
        listed.extend(cids[1:] if args["keep_notes"] and all_empty else cids)
    removed = session.col.remove_cards_and_orphaned_notes(listed)
    return {"deleted": removed.count, "kept_valid": 0}


@scenario_check
def check_scan_before_delete(scenario: dict) -> None:
    """Y1/Y2: a delete_empty_cards acts on an empty_cards step of its own
    segment. A handoff forgets the kept scan (the other side never had it)."""
    seen = False
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, _args),) = step.items()
        if name == "handoff":
            seen = False
        elif name == "empty_cards":
            seen = True
        elif name == "delete_empty_cards" and not seen:
            raise invalid("delete_empty_cards needs an empty_cards step since the start "
                          "or the latest handoff", step=index)
