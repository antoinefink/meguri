# SPDX-License-Identifier: AGPL-3.0-or-later
"""Scheduling ops on chosen cards (specs/conformance/sched-ops.md J1-J9).

bury:           {cards} | {notes}   the user's bury (queue -3) of the cards, or
    of every card of the notes
suspend:        {cards} | {notes}   Suspend Card or Suspend Note (model-ops.md
    L3's {cards} form is this op)
unsuspend:      {cards}   the restore of buried and suspended cards
unbury_deck:    {deck, mode: all | manual | siblings}   the deck's unbury with
    the all, user-only and scheduler-only modes
set_due:        {cards, days, context?: reviewer | browser}   Set Due Date with
    the text as typed, remembered under the context's string key; rejection
    invalid_days
forget:         {cards, restore_position?, reset_counts?, context?}   Reset
    (schedule as new, with its log row); both booleans false when missing
grade_now:      {cards, rating: again | hard | good | easy}   the backend's
    Grade Now
empty_filtered: {deck}   Empty of a filtered deck; rejection not_filtered,
    decided before calling Anki
None of them has an output.

J1: `cards` and `notes` list at least one reference, each once. A repeated one
is invalid_scenario before any call (Anki fails some of these ops on it with a
database error and runs others twice). References to deleted cards and notes
are passed on (Anki skips them; Grade Now fails, a run error). The ids reach
Anki in the listed order. Decks are named as deck-ops.md K1 says: the stored
name in human form, matched exactly against the collection file at the step,
so a filtered deck a db_exec inserted is found; filtered or not is read from
that row's kind blob, never from a cache.

set_due: Anki parses the text before it reads any card, and refuses a bad text
with invalid input; it also refuses a card whose home deck is filtered with
invalid input, inside its transaction (set-due-date S4.1). After an
invalid-input failure the driver asks again with the same text, no card and no
key: only a refused text fails again, and that is invalid_days. Any other
failure stays a run error.
"""

from __future__ import annotations

from typing import Any

import anki.errors
from anki.config import Config
from anki.decks_pb2 import Deck
from anki.scheduler_pb2 import ScheduleCardsAsNewRequest, UnburyDeckRequest

from ..errors import DriverError, Rejected, invalid
from ..registry import op
from .sched import RATINGS

UNBURY_MODES = {
    "all": UnburyDeckRequest.ALL,
    "manual": UnburyDeckRequest.USER_ONLY,
    "siblings": UnburyDeckRequest.SCHED_ONLY,
}
SET_DUE_KEYS = {"reviewer": Config.String.SET_DUE_REVIEWER, "browser": Config.String.SET_DUE_BROWSER}
RESET_CONTEXTS = {"reviewer": ScheduleCardsAsNewRequest.REVIEWER, "browser": ScheduleCardsAsNewRequest.BROWSER}
NATIVE_SEPARATOR = "\x1f"


def _args(name: str, args: Any, required: set[str], optional: frozenset[str] = frozenset()) -> dict:
    if not isinstance(args, dict) or not required <= set(args) or set(args) - required - optional:
        keys = ", ".join(sorted(required) + [f"{k}?" for k in sorted(optional)])
        raise invalid(f"{name} takes {{{keys}}}")
    return args


def _refs(session, name: str, key: str, value: Any, kind: str) -> list[int]:
    """J1: at least one reference, each listed once, as real ids in order."""
    if not isinstance(value, list) or not value:
        raise invalid(f"{name} {key} is a list of at least one reference")
    ids = [session.resolve(ref, kind) for ref in value]
    seen: set[int] = set()
    for ref, real in zip(value, ids):
        if real in seen:
            raise invalid(f"{name} lists {ref} twice (sched-ops.md J1)")
        seen.add(real)
    return ids


def _cards(session, name: str, args: dict) -> list[int]:
    return _refs(session, name, "cards", args["cards"], "c")


def _cards_or_notes(session, name: str, args: Any) -> tuple[str, list[int]]:
    """J2, J3: exactly one of {cards} and {notes}."""
    if not isinstance(args, dict) or set(args) not in ({"cards"}, {"notes"}):
        raise invalid(f"{name} takes {{cards}} or {{notes}}")
    ((key, value),) = args.items()
    return key, _refs(session, name, key, value, "c" if key == "cards" else "n")


def _choice(name: str, args: dict, key: str, choices: dict) -> Any:
    """An optional enum argument: None when missing, never null."""
    if key not in args:
        return None
    value = args[key]
    if not isinstance(value, str) or value not in choices:
        raise invalid(f"{name} {key} is one of {', '.join(sorted(choices))}")
    return choices[value]


def _flag(name: str, args: dict, key: str) -> bool:
    value = args.get(key, False)
    if not isinstance(value, bool):
        raise invalid(f"{name} {key} is a boolean")
    return value


def _deck(session, name: str, value: Any) -> int:
    """deck-ops.md K1: the deck whose stored human name is exactly value."""
    if not isinstance(value, str) or not value:
        raise invalid(f"{name} deck is a deck's stored name")
    rows = session.col.db.all("select id, name from decks")
    ids = [did for did, stored in rows if stored.replace(NATIVE_SEPARATOR, "::") == value]
    if not ids:
        raise DriverError("unknown_ref", ref=value)
    if len(ids) > 1:
        raise invalid(f"{len(ids)} decks are named {value!r}")
    return ids[0]


def _is_filtered(session, did: int) -> bool:
    # The database proxy returns a blob as a list of byte values.
    kind = session.col.db.scalar("select kind from decks where id = ?", did)
    return Deck.KindContainer.FromString(bytes(kind)).WhichOneof("kind") == "filtered"


@op("bury")
def bury(session, args):
    key, ids = _cards_or_notes(session, "bury", args)
    if key == "cards":
        session.col.sched.bury_cards(ids, manual=True)
    else:
        session.col.sched.bury_notes(ids)


@op("suspend")
def suspend(session, args):
    key, ids = _cards_or_notes(session, "suspend", args)
    if key == "cards":
        session.col.sched.suspend_cards(ids)
    else:
        session.col.sched.suspend_notes(ids)


@op("unsuspend")
def unsuspend(session, args):
    args = _args("unsuspend", args, {"cards"})
    session.col.sched.unsuspend_cards(_cards(session, "unsuspend", args))


@op("unbury_deck")
def unbury_deck(session, args):
    args = _args("unbury_deck", args, {"deck", "mode"})
    mode = _choice("unbury_deck", args, "mode", UNBURY_MODES)
    session.col.sched.unbury_deck(_deck(session, "unbury_deck", args["deck"]), mode)


def _text_refused(session, days: str) -> bool:
    """Whether Anki refuses the days text on its own (see the module notes)."""
    try:
        session.col.sched.set_due_date([], days)
    except anki.errors.InvalidInput:
        return True
    return False


@op("set_due", rejects=("invalid_days",))
def set_due(session, args):
    args = _args("set_due", args, {"cards", "days"}, frozenset({"context"}))
    days = args["days"]
    if not isinstance(days, str):
        raise invalid("set_due days is the text as typed, a string; quote numbers")
    key = _choice("set_due", args, "context", SET_DUE_KEYS)
    card_ids = _cards(session, "set_due", args)
    try:
        session.col.sched.set_due_date(card_ids, days, config_key=key)
    except anki.errors.InvalidInput:
        if _text_refused(session, days):
            raise Rejected("invalid_days") from None
        raise


@op("forget")
def forget(session, args):
    args = _args("forget", args, {"cards"}, frozenset({"restore_position", "reset_counts", "context"}))
    restore_position = _flag("forget", args, "restore_position")
    reset_counts = _flag("forget", args, "reset_counts")
    context = _choice("forget", args, "context", RESET_CONTEXTS)
    session.col.sched.schedule_cards_as_new(
        _cards(session, "forget", args),
        restore_position=restore_position,
        reset_counts=reset_counts,
        context=context,
    )


@op("grade_now")
def grade_now(session, args):
    args = _args("grade_now", args, {"cards", "rating"})
    rating = args["rating"]
    if not isinstance(rating, str) or rating not in RATINGS:
        raise invalid("grade_now rating is again, hard, good or easy")
    session.col._backend.grade_now(card_ids=_cards(session, "grade_now", args), rating=RATINGS[rating])


@op("empty_filtered", rejects=("not_filtered",))
def empty_filtered(session, args):
    args = _args("empty_filtered", args, {"deck"})
    did = _deck(session, "empty_filtered", args["deck"])
    if not _is_filtered(session, did):
        raise Rejected("not_filtered")
    session.col.sched.empty_filtered_deck(did)
