# SPDX-License-Identifier: AGPL-3.0-or-later
"""Card Info and Stats-screen graph data (specs/conformance/stats-ops.md H9,
H10; stats/card-info C3, C6.1, C13; stats/graphs-common G14, graphs-revlog V8,
graphs-cards C12). Observations only; no op.

{card_stats: [#cN, ...]} -> {"#cN": <stats>}: the backend's card-stats
  response for each card, every member written (null for an unset optional),
  card_id and note_id as logical refs, review_kind as its number, f32 values
  as shortest f32 decimals. Anki rewrites a card's data with an `lrt` while it
  computes this when the data has none (stats/card-info C13.3): that write is
  Anki's, and the scenarios follow C13.4. A failure is a run error
  (anki_error), never a value.

{stats: [{scope, history, parts}, ...]} -> [{<part>: <message>} | {error}]:
  the backend's graph data for the scope's search text and the history's days
  (year 365, all 0). The search text first runs alone as an unsorted card
  search, its errors mapped as the `search` kind maps them (search-ops.md S1;
  a database error there is search_failed); only then is the graph data asked
  for, and a database error raised then is unreadable_row (G14). A part is one
  message of the response, whole, in G14's form: protobuf field names, map
  keys as decimal strings, scalars at their default and empty maps left out,
  sub-messages written when set (Anki sets every one), lists written. V8's
  rule for reviews, true_retention, hours and buttons (every message member
  and list written, empty or not) gives the same text, since Anki sets every
  message and its lists always hold 24 or 4 items. With history year the
  all_time members of true_retention, hours and buttons are dropped (V8).

Neither reads nor writes display text (R1887): the Python-side stats backend
returns numbers and names only.
"""

from __future__ import annotations

from typing import Any

import anki.errors
from anki.collection import SearchNode
from google.protobuf.descriptor import FieldDescriptor as FD
from google.protobuf.message import Message

from ..errors import DriverError, invalid
from ..numbers import f32_value
from ..observe.search import REGEX_PREFIX, Reasons, _error, _search_error
from ..registry import observation

HISTORY_DAYS = {"year": 365, "all": 0}
# part -> the GraphsResponse field it is, or None for the context scalars.
PARTS = {
    "reviews": "reviews",
    "true_retention": "true_retention",
    "hours": "hours",
    "buttons": "buttons",
    "future_due": "future_due",
    "card_counts": "card_counts",
    "intervals": "intervals",
    "stability": "stability",
    "difficulty": "difficulty",
    "retrievability": "retrievability",
    "added": "added",
    "context": None,
}
# Parts whose all_time member history `year` leaves out (V8).
ALL_TIME_PARTS = frozenset({"true_retention", "hours", "buttons"})
CONTEXT_FIELDS = ("rollover_hour", "fsrs")


# --- graph data ----------------------------------------------------------------


def _scalar(fd: FD, value: Any) -> Any:
    if fd.type == FD.TYPE_FLOAT:
        return f32_value(value)
    if fd.type == FD.TYPE_MESSAGE:
        return _graph_message(value)
    if fd.type in (FD.TYPE_ENUM, FD.TYPE_BYTES, FD.TYPE_STRING, FD.TYPE_DOUBLE):
        raise DriverError("internal_error", detail=f"graph data field {fd.name} has an unexpected type")
    return value


def _graph_message(msg: Message) -> dict[str, Any]:
    """G14's form of one graph-data message."""
    out: dict[str, Any] = {}
    for fd in msg.DESCRIPTOR.fields:
        value = getattr(msg, fd.name)
        if fd.message_type is not None and fd.message_type.GetOptions().map_entry:
            if len(value):
                vfd = fd.message_type.fields_by_name["value"]
                out[fd.name] = {str(k): _scalar(vfd, v) for k, v in sorted(value.items())}
        elif fd.is_repeated:
            out[fd.name] = [_scalar(fd, v) for v in value]
        elif fd.type == FD.TYPE_MESSAGE:
            if msg.HasField(fd.name):
                out[fd.name] = _graph_message(value)
        elif value:
            out[fd.name] = _scalar(fd, value)
    return out


def _part(resp: Message, part: str, history: str) -> dict[str, Any]:
    field = PARTS[part]
    if field is None:
        return {name: getattr(resp, name) for name in CONTEXT_FIELDS if getattr(resp, name)}
    if not resp.HasField(field):
        raise DriverError("internal_error", detail=f"the graph data has no {field}")
    value = _graph_message(getattr(resp, field))
    if history == "year" and part in ALL_TIME_PARTS:
        value.pop("all_time", None)
    return value


def _requests(args: Any) -> list[tuple[tuple[str, str | None], str, list[str]]]:
    if not isinstance(args, list) or not args:
        raise invalid("stats takes a non-empty list of {scope, history, parts}")
    out = []
    for req in args:
        if not isinstance(req, dict) or set(req) != {"scope", "history", "parts"}:
            raise invalid("a stats request is {scope, history, parts}")
        scope = req["scope"]
        if scope == "collection":
            parsed: tuple[str, str | None] = ("collection", None)
        elif isinstance(scope, dict) and len(scope) == 1 and isinstance(scope.get("deck"), str) and scope["deck"]:
            parsed = ("deck", scope["deck"])
        elif isinstance(scope, dict) and len(scope) == 1 and isinstance(scope.get("search"), str):
            parsed = ("search", scope["search"])
        else:
            raise invalid("a stats scope is collection, {deck: <name>} or {search: <text>}")
        history = req["history"]
        if not isinstance(history, str) or history not in HISTORY_DAYS:
            raise invalid("a stats history is year or all")
        parts = req["parts"]
        if (
            not isinstance(parts, list)
            or not parts
            or not all(isinstance(p, str) and p in PARTS for p in parts)
            or len(set(parts)) != len(parts)
        ):
            raise invalid(f"stats parts is a non-empty list of distinct names from {sorted(PARTS)}")
        out.append((parsed, history, parts))
    return out


def _search_text(col, names: set[str], scope: tuple[str, str | None]) -> str:
    kind, value = scope
    if kind == "collection":
        return ""
    if kind == "deck":
        if value not in names:
            raise DriverError("unknown_ref", ref=value)
        return col.build_search_string(SearchNode(deck=value))
    return value


def _search_failure(reasons: Reasons, col, search: str) -> dict | None:
    """The search's error output, as the `search` kind gives it, or None."""
    try:
        col.find_cards(search, order=False)
    except anki.errors.SearchError as exc:
        return _search_error(reasons, exc)
    except anki.errors.InvalidInput as exc:
        if not str(exc).startswith(REGEX_PREFIX):
            raise
        return _error("invalid_regex")
    except anki.errors.DBError:
        return _error("search_failed")
    return None


@observation("stats")
def stats(session, args):
    requests = _requests(args)
    col = session.col
    names = {entry.name for entry in col.decks.all_names_and_ids()}
    searches = [_search_text(col, names, scope) for scope, _, _ in requests]
    reasons = Reasons(col.tr)
    out = []
    for search, (_, history, parts) in zip(searches, requests):
        failure = _search_failure(reasons, col, search)
        if failure is not None:
            out.append(failure)
            continue
        try:
            resp = col._backend.graphs(search=search, days=HISTORY_DAYS[history])
        except anki.errors.DBError:
            out.append(_error("unreadable_row"))
            continue
        out.append({part: _part(resp, part, history) for part in parts})
    return out


# --- card stats ----------------------------------------------------------------


def _optional(msg: Message, name: str, convert=lambda v: v) -> Any:
    return convert(getattr(msg, name)) if msg.HasField(name) else None


def _memory_state(msg: Message) -> dict[str, float] | None:
    if not msg.HasField("memory_state"):
        return None
    state = msg.memory_state
    return {"stability": f32_value(state.stability), "difficulty": f32_value(state.difficulty)}


def _revlog_entry(entry: Message) -> dict[str, Any]:
    return {
        "time": entry.time,
        "review_kind": int(entry.review_kind),
        "button_chosen": entry.button_chosen,
        "interval": entry.interval,
        "ease": entry.ease,
        "taken_secs": f32_value(entry.taken_secs),
        "memory_state": _memory_state(entry),
        "last_interval": entry.last_interval,
    }


def _card_stats(session, resp: Message) -> dict[str, Any]:
    return {
        "card_id": session.card_ref(resp.card_id),
        "note_id": session.note_ref(resp.note_id),
        "deck": resp.deck,
        "original_deck": _optional(resp, "original_deck"),
        "card_type": resp.card_type,
        "notetype": resp.notetype,
        "preset": resp.preset,
        "custom_data": resp.custom_data,
        "added": resp.added,
        "first_review": _optional(resp, "first_review"),
        "latest_review": _optional(resp, "latest_review"),
        "due_date": _optional(resp, "due_date"),
        "due_position": _optional(resp, "due_position"),
        "interval": resp.interval,
        "ease": resp.ease,
        "reviews": resp.reviews,
        "lapses": resp.lapses,
        "average_secs": f32_value(resp.average_secs),
        "total_secs": f32_value(resp.total_secs),
        "desired_retention": _optional(resp, "desired_retention", f32_value),
        "fsrs_retrievability": _optional(resp, "fsrs_retrievability", f32_value),
        "memory_state": _memory_state(resp),
        "fsrs_params": [f32_value(v) for v in resp.fsrs_params],
        "revlog": [_revlog_entry(e) for e in resp.revlog],
    }


@observation("card_stats")
def card_stats(session, args):
    if not isinstance(args, list) or not args:
        raise invalid("card_stats takes a non-empty list of card references")
    targets = session.card_targets(args)
    if len({ref for ref, _ in targets}) != len(targets):
        raise invalid("card_stats names each card at most once")
    col = session.col
    return {ref: _card_stats(session, col.card_stats_data(cid)) for ref, cid in targets}
