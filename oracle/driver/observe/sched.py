# SPDX-License-Identifier: AGPL-3.0-or-later
"""Scheduler observations.

next_states [(#cN, ...)]: per card {current, again, hard, good, easy}, each a
  state {kind: new|learning|review|relearning|preview|rescheduling, ...fields},
  memory states included, f32 values as shortest f32 decimals. No labels.

fuzz_table: {card: #cN, pairs: [[card_id, reps], ...], intervals: [days, ...]}
  For each pair, the interval fuzz Anki applies when answering a card with that
  id and that review count (reps before the answer), at each interval:
  {intervals, rows: [[card_id, reps, delta_days...], ...]}. The card is
  rewritten with each id and count in turn and restored afterwards; its home
  preset bounds the fuzzed interval.

When the run request asked for fsrs_context (protocol.json), next_states also
records, for each card whose states FSRS computed, its home deck's effective
desired retention and the decay of its home preset's parameters
(sched/fsrs-transitions T2.1, T2.2), keyed /steps/<i>/next_states/#cN.
"""

from __future__ import annotations

from .. import convert, fsrs_model
from ..errors import invalid
from ..numbers import f32_value
from ..registry import observation

BUTTONS = ("current", "again", "hard", "good", "easy")


def _fsrs_context(col, cid: int) -> dict:
    card = col.get_card(cid)
    home = col._backend.get_deck(card.odid or card.did)
    preset = col._backend.get_deck_config(home.normal.config_id).config
    if home.normal.HasField("desired_retention"):
        retention = home.normal.desired_retention
    else:
        retention = preset.desired_retention
    params = list(preset.fsrs_params_6) or list(preset.fsrs_params_5) or list(preset.fsrs_params_4)
    return {"desired_retention": f32_value(retention), "decay": f32_value(fsrs_model.decay(params))}


@observation("next_states")
def next_states(session, args):
    out = {}
    context = session.fsrs_context
    fsrs = context is not None and session.col.get_config("fsrs", False) is True
    for ref, cid in session.card_targets(args):
        states = session.col._backend.get_scheduling_states(cid)
        out[ref] = {b: convert.message(getattr(states, b)) for b in BUTTONS}
        if fsrs:
            # The step's index among the outputs is its index in the response.
            context[f"/steps/{len(session.outputs)}/next_states/{ref}"] = _fsrs_context(session.col, cid)
    return out


def _uint(v, what):
    if isinstance(v, bool) or not isinstance(v, int) or v < 0:
        raise invalid(f"{what} must be a non-negative integer")
    return v


@observation("fuzz_table")
def fuzz_table(session, args):
    if not isinstance(args, dict) or set(args) != {"card", "pairs", "intervals"}:
        raise invalid("fuzz_table takes {card, pairs, intervals}")
    card_id = session.resolve(args["card"], "c")
    intervals = [_uint(i, "an interval") for i in args["intervals"]] if isinstance(args["intervals"], list) else None
    pairs = args["pairs"]
    if not intervals or not isinstance(pairs, list):
        raise invalid("fuzz_table needs a list of pairs and a non-empty list of intervals")
    col = session.col
    original_reps = col.db.scalar("select reps from cards where id = ?", card_id)
    current = card_id
    rows = []
    try:
        for pair in pairs:
            if not isinstance(pair, list) or len(pair) != 2:
                raise invalid("a pair is [card_id, reps]")
            pid, reps = _uint(pair[0], "a card id"), _uint(pair[1], "reps")
            # Anki's fuzz delta is defined for the last answer (reps - 1).
            col.db.execute("update cards set id = ?, reps = ? where id = ?", pid, reps + 1, current)
            current = pid
            rows.append([pid, reps] + [col.fuzz_delta(pid, ivl) for ivl in intervals])
    finally:
        col.db.execute("update cards set id = ?, reps = ? where id = ?", card_id, original_reps, current)
    return {"intervals": intervals, "rows": rows}
