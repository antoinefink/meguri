# SPDX-License-Identifier: AGPL-3.0-or-later
"""browser_rows (specs/conformance/browser-ops.md B3, B4).

browser_rows {columns: [<column key>, ...], rows: [<ref>, ...]} ->
    [{tint, cells: {<column key>: <cell>}} | {error: deleted | damaged}, ...]:
    one entry per ref, in request order. columns are distinct keys among the
    19, rows distinct references; at most one request per observe step, no
    text form (checked before any collection). The rows are cards or notes as
    Anki itself reads the config key browserTableShowNotesMode (a #cN ref in
    notes mode or a #nN ref in cards mode is invalid_scenario). The requested
    columns become the backend's active browser columns (backend state for
    this call, nothing stored), then each row is the backend's Browse row for
    the ref's real id, read in Anki's language (English in the oracle image).

Cells: every U+2068 and U+2069 is removed from a cell's text, then the text is
read by its column with B4's forms only. A text no form reads is recorded as
{unparsed: <text>}, which no Meguri cell equals, so the scenario fails and the
spec writer looks at it. Tints come from the row colour enum, by name.

Row errors are told apart by the backend error's kind exactly as card_excerpts
does (its _RowProbe): DELETED is deleted here (card_excerpts calls it
missing); NOT_FOUND, and an INVALID_INPUT whose message is the backend's own
errors_please_check_database text, are damaged. Anything else is a run error
(anki_error), without its message.

Nothing is written. Like every Browse row, each row computes today: in a
collection that lacks the config key rollover, that first day computation
writes it, as any day query does.

No reopen first (render_status reopens): a reopen would drop the undo queue,
a held queue and sync status memory. Rows read note types and decks through
the backend's caches, which db_exec does not clear, so scenarios' db_exec steps
write only cards, notes and config (browser-ops.md B5).
"""

from __future__ import annotations

import re
from typing import Any

from anki import search_pb2
from anki.collection import Config

from ..errors import DriverError, invalid
from ..registry import observation, scenario_check
from ..session import parse_observation
from .card_excerpts import _RowError, _RowProbe

KIND = "browser_rows"
COLUMNS = (
    "noteFld", "question", "answer", "template", "deck", "cardDue", "cardEase", "cardIvl", "cardReps", "cardLapses",
    "noteCrt", "cardMod", "noteMod", "note", "noteTags", "originalPosition", "stability", "difficulty", "retrievability",
)
TEXT = frozenset({"noteFld", "question", "answer", "deck", "note", "noteTags"})
COUNTS = frozenset({"cardReps", "cardLapses"})  # and notes-mode template
PERCENTS = frozenset({"cardEase", "difficulty", "retrievability"})
DATE_TIMES = frozenset({"noteCrt", "cardMod", "noteMod"})
SPANS = frozenset({"cardIvl", "stability"})
ROW_ERRORS = {"missing": "deleted", "damaged": "damaged"}  # card_excerpts kind -> B3 kind

_Row = search_pb2.BrowserRow
TINTS = {
    _Row.COLOR_DEFAULT: "none",
    _Row.COLOR_FLAG_RED: "flag_1",
    _Row.COLOR_FLAG_ORANGE: "flag_2",
    _Row.COLOR_FLAG_GREEN: "flag_3",
    _Row.COLOR_FLAG_BLUE: "flag_4",
    _Row.COLOR_FLAG_PINK: "flag_5",
    _Row.COLOR_FLAG_TURQUOISE: "flag_6",
    _Row.COLOR_FLAG_PURPLE: "flag_7",
    _Row.COLOR_MARKED: "marked",
    _Row.COLOR_SUSPENDED: "suspended",
    _Row.COLOR_BURIED: "buried",
}
UNITS = {
    "second": "seconds", "seconds": "seconds", "minute": "minutes", "minutes": "minutes",
    "hour": "hours", "hours": "hours", "day": "days", "days": "days",
    "month": "months", "months": "months", "year": "years", "years": "years",
}

ISOLATION = str.maketrans("", "", "⁨⁩")
COUNT = re.compile(r"[0-9]+")
INTEGER = re.compile(r"-?[0-9]+")
PERCENT = re.compile(r"(-?[0-9]+)%")
NEW = re.compile(r"New #(-?[0-9]+)")
DATE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}")
DATE_TIME = re.compile(r"([0-9]{4}-[0-9]{2}-[0-9]{2}) @ ([0-9]{2}:[0-9]{2})")
SPAN = re.compile(r"(-?[0-9]+(?:\.[0-9]+)?) ([a-z]+)")


def _due_body(text: str) -> dict | None:
    if text == "(filtered)":
        return {"filtered": True}
    if m := NEW.fullmatch(text):
        return {"new_position": int(m[1])}
    if DATE.fullmatch(text):
        return {"date": text}
    return None


def _due(text: str) -> dict | None:
    """cardDue: filtered, a new position or a date, each also in parentheses
    (wrapped)."""
    cell = _due_body(text)
    if cell is None and len(text) >= 2 and text[0] == "(" and text[-1] == ")":
        cell = _due_body(text[1:-1])
        if cell is not None:
            cell["wrapped"] = True
    return cell


def parse_cell(column: str, text: str, notes_mode: bool) -> Any:
    """Anki's English cell text -> the B4 cell, or {unparsed: <text>}."""
    t = text.translate(ISOLATION)
    if column in TEXT or (column == "template" and not notes_mode):
        return {"text": t}
    if column in COUNTS or column == "template":
        return {"count": int(t)} if COUNT.fullmatch(t) else {"unparsed": t}
    if column == "cardDue":
        if t == "":
            return None
        cell = _due(t)
    elif column in PERCENTS:
        if t == "":
            return None
        if column == "cardEase" and t == "(new)":
            return {"marker": "new"}
        m = PERCENT.fullmatch(t)
        cell = {"percent": int(m[1])} if m else None
    elif column == "originalPosition":
        if t == "":
            return None
        cell = {"position": int(t)} if INTEGER.fullmatch(t) else None
    elif column in DATE_TIMES:
        m = DATE_TIME.fullmatch(t)
        cell = {"datetime": f"{m[1]} {m[2]}"} if m else None
    elif column in SPANS:
        if t == "":
            return None
        if column == "cardIvl" and t in ("(new)", "(learning)"):
            return {"marker": t[1:-1]}
        m = SPAN.fullmatch(t)
        cell = {"span": {"unit": UNITS[m[2]], "amount": m[1]}} if m and m[2] in UNITS else None
    else:
        raise DriverError("internal_error", detail=f"no cell form for column {column!r}")
    return cell if cell is not None else {"unparsed": t}


def check_request(args: Any) -> tuple[list[str], list[str]]:
    if not isinstance(args, dict) or set(args) != {"columns", "rows"}:
        raise invalid("browser_rows takes {columns: [column keys], rows: [refs]}")
    columns, rows = args["columns"], args["rows"]
    if (
        not isinstance(columns, list)
        or not columns
        or not all(isinstance(c, str) and c in COLUMNS for c in columns)
        or len(set(columns)) != len(columns)
    ):
        raise invalid("browser_rows columns is a non-empty list of distinct column keys (browser/config-keys K3)")
    if not isinstance(rows, list) or not rows or not all(isinstance(r, str) for r in rows) or len(set(rows)) != len(rows):
        raise invalid("browser_rows rows is a non-empty list of distinct references")
    return columns, rows


@scenario_check
def check_browser_rows_requests(scenario: dict) -> None:
    """B3: at most one request per observe step, no text form, each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe":
            continue
        requests = [spec for spec in specs if parse_observation(spec)[0] == KIND]
        if len(requests) > 1:
            raise invalid("an observe step has at most one browser_rows request", step=index)
        for spec in requests:
            if isinstance(spec, str):
                raise invalid("browser_rows has no text form: {browser_rows: {columns, rows}}", step=index)
            try:
                check_request(spec[KIND])
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


@observation(KIND)
def browser_rows(session, args):
    columns, refs = check_request(args)
    col = session.col
    notes_mode = col.get_config_bool(Config.Bool.BROWSER_TABLE_SHOW_NOTES_MODE)
    ids = [session.resolve(ref, "n" if notes_mode else "c") for ref in refs]
    backend = col._backend
    backend.set_active_browser_columns(columns)
    probe = _RowProbe(backend, col.tr.errors_please_check_database())
    out = []
    for rid in ids:
        try:
            row = type(backend).browser_row_for_id(probe, rid)
        except _RowError as exc:
            out.append({"error": ROW_ERRORS[exc.kind]})
            continue
        if len(row.cells) != len(columns):
            raise DriverError("internal_error", detail="a Browse row has another number of cells than columns")
        if row.color not in TINTS:
            raise DriverError("internal_error", detail=f"unknown Browse row colour {row.color}")
        cells = {c: parse_cell(c, cell.text, notes_mode) for c, cell in zip(columns, row.cells)}
        out.append({"tint": TINTS[row.color], "cells": cells})
    return out
