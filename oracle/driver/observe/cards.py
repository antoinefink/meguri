# SPDX-License-Identifier: AGPL-3.0-or-later
"""card [(#cN, ...)] and revlog [(#cN, ...)]: stored rows, by logical card id.

card:   the cards row with nid as #nN, did/odid as deck names (null when 0) and
        data parsed as JSON (kept as text when it is not JSON).
revlog: the card's review-log rows in id order; cid as #cN.
Without arguments: every card.
"""

from __future__ import annotations

import json

from ..registry import observation

CARD_COLUMNS = (
    "id", "nid", "did", "ord", "mod", "usn", "type", "queue", "due", "ivl", "factor",
    "reps", "lapses", "left", "odue", "odid", "flags", "data",
)
REVLOG_COLUMNS = ("id", "cid", "usn", "ease", "ivl", "lastIvl", "factor", "time", "type")


def _data(text: str):
    if text == "":
        return {}
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


@observation("card")
def card(session, args):
    out = {}
    for ref, cid in session.card_targets(args):
        row = session.col.db.first(f"select {', '.join(CARD_COLUMNS)} from cards where id = ?", cid)
        if row is None:
            out[ref] = None
            continue
        rec = dict(zip(CARD_COLUMNS, row))
        rec["id"] = ref
        rec["nid"] = session.note_ref(rec["nid"])
        rec["did"] = session.deck_name(rec["did"])
        rec["odid"] = session.deck_name(rec["odid"])
        rec["data"] = _data(rec["data"])
        out[ref] = rec
    return out


@observation("revlog")
def revlog(session, args):
    out = {}
    for ref, cid in session.card_targets(args):
        rows = session.col.db.all(f"select {', '.join(REVLOG_COLUMNS)} from revlog where cid = ? order by id", cid)
        out[ref] = [dict(zip(REVLOG_COLUMNS, r), cid=ref) for r in rows]
    return out
