# SPDX-License-Identifier: AGPL-3.0-or-later
"""decks and deck_tree (specs/conformance/deck-ops.md K11, K12).

decks:     {<stored name in human form>: {id, filtered, config_id,
           study_collapsed, browser_collapsed, mtime_secs, usn}} for every
           row of the decks table, each read through Anki's deck getter (the
           stored row, not its cache); config_id null for a filtered deck.
deck_tree: [{name, children}] from Anki's deck tree without a timestamp, so
           without counts and without the unbury of a counted tree; the
           backend leaves out the default deck when it hides it and decks
           whose parent is missing.
"""

from __future__ import annotations

from ..errors import DriverError, invalid
from ..registry import observation
from ..ops.decks import human_name


def _no_args(kind: str, args) -> None:
    if args:
        raise invalid(f"{kind} takes no arguments")


@observation("decks")
def decks(session, args):
    _no_args("decks", args)
    backend = session.col._backend
    out: dict[str, dict] = {}
    for did, native in session.col.db.all("select id, name from decks order by id"):
        key = human_name(native)
        if key in out:
            raise DriverError("internal_error", detail="two decks have the same stored name in human form")
        deck = backend.get_deck(did)
        kind = deck.WhichOneof("kind")
        if kind not in ("normal", "filtered"):
            raise DriverError("internal_error", detail="a deck has neither kind")
        normal = kind == "normal"
        out[key] = {
            "id": did,
            "filtered": not normal,
            "config_id": deck.normal.config_id if normal else None,
            "study_collapsed": deck.common.study_collapsed,
            "browser_collapsed": deck.common.browser_collapsed,
            "mtime_secs": deck.mtime_secs,
            "usn": deck.usn,
        }
    return out


def _node(node) -> dict:
    return {"name": node.name, "children": [_node(child) for child in node.children]}


@observation("deck_tree")
def deck_tree(session, args):
    _no_args("deck_tree", args)
    return [_node(child) for child in session.col.decks.deck_tree().children]
