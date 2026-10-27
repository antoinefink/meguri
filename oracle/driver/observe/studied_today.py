# SPDX-License-Identifier: AGPL-3.0-or-later
"""The studied-today observations (specs/conformance/stats-ops.md H2, H5-H7).

studied_today_raw -> {cards, time_ms}: the row count and the summed stored
  `time` (milliseconds) that Anki's Home studied-today line is built from at
  the current instant (H2).
studied_today_text -> str: Anki's Home studied-today line for the scenario's
  collection at the current instant (H5).
{studied_today_format: {inputs: [[cards, time_ms], ...]}} -> [str]: Anki's
  studied-today message for each pair, with time_ms / 1000 seconds (H6).
{today_stats: [collection | {deck: name} | {search: text}, ...]} ->
  [{answers, time_ms, correct, learn, review, relearn, filtered, mature,
  mature_correct}]: the `today` part of Anki's Stats graph data per scope (H7).

pylib hands out only the Home line's text, so studied_today_raw reads its two
numbers with the query the backend's line runs (rows after the start of the
scheduler's day 0, Manual and Rescheduled rows left out) and keeps them only
when the backend renders exactly that line from them and another line from
one card more, so the line pins the count. That text never leaves the driver.

The two lines are English text with Anki's direction marks removed and each
line feed replaced by one space: R1448 requires this one text to match Anki's
(observations.md O1). Collections use the default backend language, en. The
message for given numbers needs an open collection but reads none: it runs on
one scratch collection per worker process, never on the scenario's.
"""

from __future__ import annotations

import os
import tempfile
from typing import Any

from anki.collection import Collection, SearchNode
from anki.lang import without_unicode_isolation

from ..errors import DriverError, invalid
from ..registry import observation

DAY_SECS = 86_400
# Revlog kinds the Home line leaves out: Manual (4) and Rescheduled (5).
MANUAL, RESCHEDULED = 4, 5
HOME_QUERY = "SELECT COUNT(), COALESCE(SUM(time), 0) FROM revlog WHERE id > ? AND type != ? AND type != ?"
MAX_CARDS = 2**32 - 1  # the message's uint32 card count
MAX_TIME_MS = 2**53 - 1  # integers a binary64 holds exactly
HISTORY_DAYS = 365  # the Today values never depend on it (stats/studied-today T3.1)
# Value member <- the graph data's Today field (H7).
TODAY_FIELDS = (
    ("answers", "answer_count"),
    ("time_ms", "answer_millis"),
    ("correct", "correct_count"),
    ("learn", "learn_count"),
    ("review", "review_count"),
    ("relearn", "relearn_count"),
    ("filtered", "early_review_count"),
    ("mature", "mature_count"),
    ("mature_correct", "mature_correct"),
)


def _line(text: str) -> str:
    return without_unicode_isolation(text).replace("\n", " ")


_scratch: Collection | None = None


def _scratch_col(session) -> Collection:
    global _scratch
    if _scratch is None:
        folder = tempfile.mkdtemp(prefix="studied-today-", dir=os.path.dirname(session.dir))
        _scratch = Collection(os.path.join(folder, "collection.anki2"))
    return _scratch


def _no_args(kind: str, args: Any) -> None:
    if args is not None:
        raise invalid(f"{kind} takes no arguments")


def _int_in(value: Any, low: int, high: int) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and low <= value <= high


def _pairs(args: Any) -> list[tuple[int, int]]:
    if not isinstance(args, dict) or set(args) != {"inputs"}:
        raise invalid("studied_today_format takes {inputs: [[cards, time_ms], ...]}")
    inputs = args["inputs"]
    if not isinstance(inputs, list) or not inputs:
        raise invalid("studied_today_format inputs is a non-empty list of [cards, time_ms] pairs")
    pairs = []
    for pair in inputs:
        if not isinstance(pair, list) or len(pair) != 2:
            raise invalid("a studied_today_format input is a [cards, time_ms] pair")
        cards, time_ms = pair
        if not _int_in(cards, 0, MAX_CARDS):
            raise invalid(f"studied_today_format cards is an integer from 0 to {MAX_CARDS}")
        if not _int_in(time_ms, -MAX_TIME_MS, MAX_TIME_MS):
            raise invalid(f"studied_today_format time_ms is an integer from -{MAX_TIME_MS} to {MAX_TIME_MS}")
        pairs.append((cards, time_ms))
    return pairs


def _scopes(args: Any) -> list[tuple[str, str | None]]:
    """[(kind, name or text)]: ("collection", None), ("deck", name), ("search", text)."""
    if not isinstance(args, list) or not args:
        raise invalid("today_stats takes a non-empty list of scopes")
    scopes: list[tuple[str, str | None]] = []
    for scope in args:
        if scope == "collection":
            scopes.append(("collection", None))
        elif isinstance(scope, dict) and len(scope) == 1 and isinstance(scope.get("deck"), str) and scope["deck"]:
            scopes.append(("deck", scope["deck"]))
        elif isinstance(scope, dict) and len(scope) == 1 and isinstance(scope.get("search"), str):
            scopes.append(("search", scope["search"]))
        else:
            raise invalid("a today_stats scope is collection, {deck: <name>} or {search: <text>}")
    return scopes


@observation("studied_today_raw")
def studied_today_raw(session, args):
    _no_args("studied_today_raw", args)
    col = session.col
    next_day_at = col.sched._timing_today().next_day_at
    start_ms = (next_day_at - DAY_SECS) * 1000
    cards, time_ms = col.db.first(HOME_QUERY, start_ms, MANUAL, RESCHEDULED)
    line = col.studied_today()
    render = col._backend.studied_today_message
    if render(cards=cards, seconds=time_ms / 1000) != line or render(cards=cards + 1, seconds=time_ms / 1000) == line:
        raise DriverError("internal_error", detail="the studied-today numbers do not render Anki's own studied-today line")
    return {"cards": cards, "time_ms": time_ms}


@observation("studied_today_text")
def studied_today_text(session, args):
    _no_args("studied_today_text", args)
    return _line(session.col.studied_today())


@observation("studied_today_format")
def studied_today_format(session, args):
    pairs = _pairs(args)
    render = _scratch_col(session)._backend.studied_today_message
    return [_line(render(cards=cards, seconds=time_ms / 1000)) for cards, time_ms in pairs]


@observation("today_stats")
def today_stats(session, args):
    scopes = _scopes(args)
    col = session.col
    names = {entry.name for entry in col.decks.all_names_and_ids()}
    searches = []
    for kind, value in scopes:
        if kind == "collection":
            searches.append("")
        elif kind == "deck":
            if value not in names:
                raise DriverError("unknown_ref", ref=value)
            searches.append(col.build_search_string(SearchNode(deck=value)))
        else:
            searches.append(value)
    out = []
    for search in searches:
        today = col._backend.graphs(search=search, days=HISTORY_DAYS).today
        out.append({member: getattr(today, field) for member, field in TODAY_FIELDS})
    return out
