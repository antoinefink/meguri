# SPDX-License-Identifier: AGPL-3.0-or-later
"""Answering.

answer: {card: #cN, rating: again|hard|good|easy, time_ms?: int}
The card moves to the state Anki computes for that rating at the current frozen
instant, which is also the review time recorded in the log.

The scheduling ops on chosen cards (suspend, bury, Grade Now, ...) are
sched_manual.py.
"""

from __future__ import annotations

from anki.scheduler_pb2 import CardAnswer

from ..errors import invalid
from ..registry import op

RATINGS = {
    "again": CardAnswer.AGAIN,
    "hard": CardAnswer.HARD,
    "good": CardAnswer.GOOD,
    "easy": CardAnswer.EASY,
}


@op("answer")
def answer(session, args):
    if not isinstance(args, dict) or set(args) - {"card", "rating", "time_ms"}:
        raise invalid("answer takes {card, rating, time_ms?}")
    card_id = session.resolve(args.get("card"), "c")
    rating = args.get("rating")
    if rating not in RATINGS:
        raise invalid("rating is again, hard, good or easy")
    time_ms = args.get("time_ms", 0)
    if isinstance(time_ms, bool) or not isinstance(time_ms, int) or time_ms < 0:
        raise invalid("time_ms is a non-negative integer")
    states = session.col._backend.get_scheduling_states(card_id)
    session.col.sched.answer_card(
        CardAnswer(
            card_id=card_id,
            current_state=states.current,
            new_state=getattr(states, rating),
            rating=RATINGS[rating],
            answered_at_millis=session.clock.now_ms(),
            milliseconds_taken=time_ms,
        )
    )

