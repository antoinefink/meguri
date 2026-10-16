# SPDX-License-Identifier: AGPL-3.0-or-later
"""fsrs_next {param_sets, inputs}: FSRS next states of a stored review card per
input (specs/conformance/float-determinism.md F3, F5).

Value: {rows: [[again days, again S, again D, hard days, hard S, hard D,
good days, good S, good D, easy days, easy S, easy D], ...]}, one row per input
in input order; days are the review part's scheduled_days, S and D its memory
state (shortest f32 decimals, as next_states gives them).

The scenario's collection is never read or written. Inputs are evaluated on one
scratch collection per worker process (FSRS on, load balancer off,
fsrsShortTermWithStepsEnabled off), created on first use in the worker's
scratch directory and kept until the process ends:

- one preset and one normal deck per distinct (parameter values, dr, max_ivl),
  made on first use and kept: the values under fsrsParams6, fsrsParams5 or
  fsrsWeights by their count (all three empty for none), desiredRetention dr,
  rev.maxIvl max_ivl, learning steps 1m 10m, relearning steps 10m, leech
  threshold 8; the deck sets no desired retention of its own;
- one card, rewritten per input (one stored-card update): id card_id, that
  deck, type 2, queue 2, ivl scheduled_days, due today, factor 2500, reps,
  lapses 0, left 0, odid 0, odue 0, data {s, d, lrt};
- lrt = next_day_at - (elapsed_days + 1) * 86400 + 1, the first second of the
  day elapsed_days days before today, so that whole days from lrt to the next
  rollover are elapsed_days (sched/fsrs-transitions T2.3) and lrt is in the
  past. next_day_at is read again after the states: when the day changed in
  between (a rollover on a real clock), the input is evaluated again.
"""

from __future__ import annotations

import json
import os
import re
import tempfile
from typing import Any

from anki.collection import Collection

from .. import convert, fsrs_model
from ..errors import DriverError, invalid
from ..registry import observation

MAX_INPUTS = 1000
INPUT_KEYS = frozenset(("id", "card_id", "reps", "s", "d", "elapsed_days", "scheduled_days", "dr", "params", "max_ivl"))
PARAM_COUNTS = (0, 17, 19, 21)
SET_NAME = re.compile(r"^[a-z0-9][a-z0-9-]*$")
DAY = 86400
RELEARN_STEPS = [10.0]
LEARN_STEPS = [1.0, 10.0]
LEECH_THRESHOLD = 8
FACTOR = 2500
# Again's day and memory state are its review part's (sched/fsrs-transitions T4).
RATINGS = ("again", "hard", "good", "easy")
MAX_RETRIES = 3


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def _is_num(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def _int_in(inp: dict, key: str, lo: int, hi: int) -> int:
    v = inp[key]
    if not _is_int(v) or not lo <= v <= hi:
        raise invalid(f"fsrs_next input {key} is an integer from {lo} to {hi}")
    return v


def _num_in(inp: dict, key: str, lo: float, hi: float, open_ends: bool = False) -> float:
    v = inp[key]
    if not _is_num(v) or not ((lo < v < hi) if open_ends else (lo <= v <= hi)):
        ends = f"above {lo} and below {hi}" if open_ends else f"from {lo} to {hi}"
        raise invalid(f"fsrs_next input {key} is a number {ends}")
    return v


def _check(args: Any) -> tuple[dict[str, list], list[dict]]:
    """float-determinism.md F5.1; anything else is invalid_scenario."""
    if not isinstance(args, dict) or set(args) != {"param_sets", "inputs"}:
        raise invalid("fsrs_next takes {param_sets, inputs}")
    sets, inputs = args["param_sets"], args["inputs"]
    if not isinstance(sets, dict) or not sets:
        raise invalid("fsrs_next param_sets is an object with at least one parameter set")
    for name, values in sets.items():
        if not SET_NAME.match(name):
            raise invalid(f"fsrs_next parameter set name {name!r} is not lower-case letters, digits and -")
        if not isinstance(values, list) or len(values) not in PARAM_COUNTS or not all(_is_num(x) for x in values):
            raise invalid(f"fsrs_next parameter set {name!r} is a list of 0, 17, 19 or 21 numbers")
    if not isinstance(inputs, list) or not 1 <= len(inputs) <= MAX_INPUTS:
        raise invalid(f"fsrs_next inputs is a list of 1 to {MAX_INPUTS} inputs")
    for inp in inputs:
        if not isinstance(inp, dict) or set(inp) != INPUT_KEYS:
            raise invalid(f"an fsrs_next input has exactly the members {', '.join(sorted(INPUT_KEYS))}")
        if not _is_int(inp["id"]) or inp["id"] < 0:
            raise invalid("fsrs_next input id is a non-negative integer")
        _int_in(inp, "card_id", 1, 2**53 - 1)
        _int_in(inp, "reps", 0, 4294967294)
        _num_in(inp, "s", 0.001, 36500)
        _num_in(inp, "d", 1, 10)
        _int_in(inp, "elapsed_days", 0, 20000)
        _int_in(inp, "scheduled_days", 1, 36500)
        _num_in(inp, "dr", 0, 1, open_ends=True)
        _int_in(inp, "max_ivl", 1, 36500)
        if not isinstance(inp["params"], str) or inp["params"] not in sets:
            raise invalid(f"fsrs_next input params {inp['params']!r} names no member of param_sets")
    return sets, inputs


class _Scratch:
    """The worker's scratch collection for fsrs_next."""

    def __init__(self, root: str) -> None:
        self.dir = tempfile.mkdtemp(prefix="fsrs-next-", dir=root)
        col = self.col = Collection(os.path.join(self.dir, "collection.anki2"))
        col.set_config("fsrs", True)
        col.set_config("loadBalancerEnabled", False)
        col.set_config("fsrsShortTermWithStepsEnabled", False)
        note = col.new_note(col.models.by_name("Basic"))
        note.fields[0] = "probe"
        col.add_note(note, 1)
        self.card = col.db.scalar("select id from cards where nid = ?", note.id)
        self.decks: dict[tuple, int] = {}

    def deck(self, params: list, dr: float, max_ivl: int) -> int:
        key = (tuple(float(x) for x in params), float(dr), max_ivl)
        if key in self.decks:
            return self.decks[key]
        n = len(self.decks) + 1
        decks = self.col.decks
        conf = decks.get_config(decks.add_config_returning_id(f"fsrs-next {n}"))
        for k in ("fsrsWeights", "fsrsParams5", "fsrsParams6"):
            conf[k] = []
        stored_under = fsrs_model.params_key(len(params))
        if stored_under is not None:
            conf[stored_under] = list(params)
        conf["desiredRetention"] = dr
        conf["rev"]["maxIvl"] = max_ivl
        conf["new"]["delays"] = list(LEARN_STEPS)
        conf["lapse"]["delays"] = list(RELEARN_STEPS)
        conf["lapse"]["leechFails"] = LEECH_THRESHOLD
        decks.update_config(conf)
        did = decks.id(f"fsrs-next {n}")
        deck = decks.get(did)
        deck["conf"] = conf["id"]
        deck.pop("desiredRetention", None)
        decks.save(deck)
        self.decks[key] = did
        return did

    def evaluate(self, inp: dict, did: int) -> list:
        col = self.col
        for _ in range(MAX_RETRIES):
            timing = col._backend.sched_timing_today()
            lrt = timing.next_day_at - (inp["elapsed_days"] + 1) * DAY + 1
            data = json.dumps({"s": inp["s"], "d": inp["d"], "lrt": lrt}, separators=(",", ":"))
            col.db.execute(
                "update cards set id = ?, did = ?, type = 2, queue = 2, ivl = ?, due = ?, factor = ?, reps = ?,"
                " lapses = 0, left = 0, odid = 0, odue = 0, data = ? where id = ?",
                inp["card_id"], did, inp["scheduled_days"], timing.days_elapsed, FACTOR, inp["reps"], data, self.card,
            )
            self.card = inp["card_id"]
            states = col._backend.get_scheduling_states(self.card)
            if col._backend.sched_timing_today().next_day_at == timing.next_day_at:
                return _row(states)
        raise DriverError("internal_error", detail="the scheduler's day kept changing while fsrs_next ran")


def _row(states) -> list:
    row: list = []
    for rating in RATINGS:
        state = convert.message(getattr(states, rating))
        review = state.get("review") if state.get("kind") == "relearning" else state
        if state.get("kind") not in ("relearning", "review") or "memory_state" not in (review or {}):
            raise DriverError("internal_error", detail=f"fsrs_next: {rating} is a {state.get('kind')} state without FSRS")
        memory = review["memory_state"]
        row += [review["scheduled_days"], memory["stability"], memory["difficulty"]]
    return row


_scratch: _Scratch | None = None


def _get_scratch(session) -> _Scratch:
    global _scratch
    if _scratch is None:
        _scratch = _Scratch(os.path.dirname(session.dir))
    return _scratch


@observation("fsrs_next")
def fsrs_next(session, args):
    sets, inputs = _check(args)
    scratch = _get_scratch(session)
    rows = []
    for inp in inputs:
        did = scratch.deck(sets[inp["params"]], inp["dr"], inp["max_ivl"])
        rows.append(scratch.evaluate(inp, did))
    return {"rows": rows}
