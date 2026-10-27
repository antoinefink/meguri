# SPDX-License-Identifier: AGPL-3.0-or-later
"""add_defaults and notetype_last_deck (specs/notes/add-defaults.md D1, D2, D7).

add_defaults {review_card?: #cN}
    -> {deck: <deck name>, notetype: <note type name>}
     | {error: invalid_argument}   no note type (D1, editor/config-keys K4.1)
     | {error: not_found}          no deck candidate and no deck 1 (D1)
    The backend's defaults request with the home deck of the card (odid when
    not 0, else did), or 0 without a card. A #cN whose card no longer exists
    is sent as 0, as D1 treats it. The error is read from the backend error's
    kind, never the exception class; any other failure stays a run error.
notetype_last_deck {notetype: <name>} | {notetype_id: <integer>}
    -> {deck: <deck name> | null}
    The backend's default-deck request for that note type id (not pylib's
    wrapper, which answers none whenever addToCur is on: D2 does not read
    addToCur), 0 read as null. notetype names a note type by its exact stored
    name; notetype_id is the raw id form for a leftover key of a note type that
    does not exist, and naming an existing one is invalid_scenario.

Deck names are the stored names in human form (`decks` observation); note
type names are stored names. Anki answers both through its deck and note type
caches, which a db_exec write does not clear, so both reopen first
(oracle/AGENTS.md), after their request and references are checked. Neither
writes anything.

Request shapes are checked before any collection exists, and each kind at most
once per observe step (its value is one object).
"""

from __future__ import annotations

import re
from functools import partial
from typing import Any

from anki import backend_pb2

from ..errors import DriverError, invalid
from ..ops.decks import human_name
from ..ops.model import KindProbe
from ..registry import observation, scenario_check
from ..session import parse_observation

CARD_REF = re.compile(r"#c[1-9][0-9]*")
I64_MIN, I64_MAX = -(2**63), 2**63 - 1
ERRORS = {
    backend_pb2.BackendError.INVALID_INPUT: "invalid_argument",
    backend_pb2.BackendError.NOT_FOUND_ERROR: "not_found",
}
KINDS = ("add_defaults", "notetype_last_deck")


class _Refused(Exception):
    def __init__(self, kind: str) -> None:
        super().__init__(kind)
        self.kind = kind


def _probe(backend) -> KindProbe:
    """A backend error of a kind in ERRORS raises _Refused, any other the
    exception pylib would raise."""
    return KindProbe(backend, {kind: partial(_Refused, name) for kind, name in ERRORS.items()})


# --- request checks (no collection) --------------------------------------------


def _check_add_defaults(args: Any) -> str | None:
    if not isinstance(args, dict) or set(args) - {"review_card"}:
        raise invalid("add_defaults takes {review_card?: #cN}")
    ref = args.get("review_card")
    if "review_card" in args and (not isinstance(ref, str) or not CARD_REF.fullmatch(ref)):
        raise invalid("add_defaults review_card is a #cN reference")
    return ref


def _check_last_deck(args: Any) -> tuple[str, Any]:
    if not isinstance(args, dict) or len(args) != 1 or set(args) - {"notetype", "notetype_id"}:
        raise invalid("notetype_last_deck takes {notetype: <name>} or {notetype_id: <integer>}")
    ((key, value),) = args.items()
    if key == "notetype":
        if not isinstance(value, str) or not value:
            raise invalid("notetype_last_deck notetype is a note type name")
    elif isinstance(value, bool) or not isinstance(value, int) or not I64_MIN <= value <= I64_MAX:
        raise invalid("notetype_last_deck notetype_id is a signed 64-bit integer")
    return key, value


CHECKS = {"add_defaults": _check_add_defaults, "notetype_last_deck": _check_last_deck}


@scenario_check
def check_add_defaults_requests(scenario: dict) -> None:
    """Each kind at most once per observe step (O2), each request well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe" or not isinstance(specs, list):
            continue
        kinds = [(kind, args) for kind, args in map(parse_observation, specs) if kind in KINDS]
        for kind in KINDS:
            if sum(1 for k, _ in kinds if k == kind) > 1:
                raise invalid(f"an observe step has at most one {kind} request", step=index)
        for kind, args in kinds:
            try:
                CHECKS[kind](args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


# --- names -------------------------------------------------------------------------


def _deck_name(col, did: int) -> str:
    name = col.db.scalar("select name from decks where id = ?", did)
    if name is None:
        raise DriverError("internal_error", detail="the backend answered a deck the decks table does not hold")
    return human_name(name)


def _notetype_name(col, ntid: int) -> str:
    name = col.db.scalar("select name from notetypes where id = ?", ntid)
    if name is None:
        raise DriverError("internal_error", detail="the backend answered a note type the notetypes table does not hold")
    return name


def _notetype_id(col, name: str) -> int:
    """The note type stored under exactly this name (the column's unicase
    collation would also match other spellings)."""
    ids = [ntid for ntid, stored in col.db.all("select id, name from notetypes") if stored == name]
    if not ids:
        raise DriverError("unknown_ref", ref=name)
    return ids[0]


# --- the observations ----------------------------------------------------------------


@observation("add_defaults")
def add_defaults(session, args):
    ref = _check_add_defaults(args)
    cid = session.resolve(ref, "c") if ref is not None else None
    session.reopen()
    col = session.col
    home = 0
    if cid is not None:
        row = col.db.first("select odid, did from cards where id = ?", cid)
        if row is not None:
            home = row[0] or row[1]
    backend = col._backend
    try:
        out = type(backend).defaults_for_adding(_probe(backend), home_deck_of_current_review_card=home)
    except _Refused as refused:
        return {"error": refused.kind}
    return {"deck": _deck_name(col, out.deck_id), "notetype": _notetype_name(col, out.notetype_id)}


@observation("notetype_last_deck")
def notetype_last_deck(session, args):
    key, value = _check_last_deck(args)
    # Refusals first, on the open collection (db_exec writes are in the file;
    # only Anki's caches are stale): the reopen empties Anki's undo history.
    if key == "notetype":
        ntid = _notetype_id(session.col, value)
    else:
        if session.col.db.scalar("select 1 from notetypes where id = ?", value) is not None:
            raise invalid("notetype_last_deck notetype_id names an existing note type: name it with notetype")
        ntid = value
    session.reopen()
    col = session.col
    did = col._backend.default_deck_for_notetype(ntid=ntid)
    return {"deck": _deck_name(col, did) if did else None}
