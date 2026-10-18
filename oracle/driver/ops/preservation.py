# SPDX-License-Identifier: AGPL-3.0-or-later
"""The preservation suite's op (specs/conformance/normalized-dump.md N5).

adopt: {}   every note of the collection without a logical id gets the next
    #nN in ascending note id order, then every card without one the next #cN
    in ascending card id order. -> {notes, cards}: how many got one. It only
    reads (SELECT statements, which Anki's DB proxy runs without marking the
    collection modified or clearing its undo history), so nothing in the
    collection changes. Not in actor scenarios: invalid_scenario before any
    collection exists.
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import op, scenario_check


@scenario_check
def no_adopt_with_actors(scenario):
    if not scenario.get("actors"):
        return
    for index, step in enumerate(scenario.get("steps", [])):
        if isinstance(step, dict) and "adopt" in step:
            raise invalid("an actor scenario has no adopt (normalized-dump.md N5)", step=index)


def _number(ids: list[int], stored: list[int]) -> int:
    known = set(ids)
    new = [i for i in stored if i not in known]
    ids.extend(new)
    return len(new)


@op("adopt")
def adopt(session, args):
    if not isinstance(args, dict) or args:
        raise invalid("adopt takes {}")
    if session.is_actor_scenario:
        raise invalid("an actor scenario has no adopt (normalized-dump.md N5)")
    db = session.col.db
    notes = _number(session.notes, db.list("select id from notes order by id"))
    cards = _number(session.cards, db.list("select id from cards order by id"))
    return {"notes": notes, "cards": cards}
