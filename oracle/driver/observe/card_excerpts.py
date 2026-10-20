# SPDX-License-Identifier: AGPL-3.0-or-later
"""card_excerpts (specs/conformance/card-excerpt-ops.md E1-E3).

{card_excerpts: [#cN, ...]} -> [{question, deck} | {error: missing | damaged}, ...]:
    one entry per reference, in request order (a repeated reference gets an
    entry at each place). A non-empty list of card references, at most one
    request per observe step, no text form; a card not created yet is
    unknown_ref.

The rows are always cards: when Anki reads the config key
browserTableShowNotesMode as true, the step is invalid_scenario. The backend's
active browser columns become question and deck for the request (backend
state, nothing stored), then each entry is the backend's Browse row for the
card's real id, its two cell texts reported as Anki returns them (it adds no
isolation marks to these two cells).

Row errors are told apart by the backend error's kind, read from the raw error
as ops/model.py's KindProbe does (pylib raises DELETED as a plain
BackendError): DELETED (the card no longer exists) is missing; NOT_FOUND (a
missing note, note type, deck, home deck or template) is damaged; an
INVALID_INPUT error is damaged only when its message is the backend's own
errors_please_check_database text (Anki's Check Database request). Anything
else is a run error (anki_error), without its message. A row error never stops
the other rows.

Nothing is written. Like every Browse row, each row computes today: in a
collection that lacks the config key rollover, that first day computation
writes it, as any day query does (scenarios observe timing first).
"""

from __future__ import annotations

import re
from typing import Any

from anki import backend_pb2
from anki._backend import backend_exception_to_pylib
from anki.collection import Config

from ..errors import DriverError, invalid
from ..registry import observation, scenario_check
from ..session import parse_observation

KIND = "card_excerpts"
COLUMNS = ("question", "deck")
CARD_REF = re.compile(r"^#c[1-9][0-9]*$")


def check_request(args: Any) -> list[str]:
    if not isinstance(args, list) or not args or not all(isinstance(r, str) and CARD_REF.match(r) for r in args):
        raise invalid("card_excerpts takes a non-empty list of card references (#cN)")
    return args


@scenario_check
def check_card_excerpts_requests(scenario: dict) -> None:
    """E1: at most one request per observe step, no text form, each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe":
            continue
        requests = [spec for spec in specs if parse_observation(spec)[0] == KIND]
        if len(requests) > 1:
            raise invalid("an observe step has at most one card_excerpts request", step=index)
        for spec in requests:
            if isinstance(spec, str):
                raise invalid("card_excerpts has no text form: {card_excerpts: [#cN, ...]}", step=index)
            try:
                check_request(spec[KIND])
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


class _RowError(Exception):
    def __init__(self, kind: str) -> None:
        super().__init__(kind)
        self.kind = kind


class _RowProbe:
    """Stands in for the RustBackend in one generated backend call: the row
    errors raise _RowError, any other error the exception pylib would raise."""

    def __init__(self, backend, check_database: str) -> None:
        self._rust = backend._backend
        self._check_database = check_database

    def _run_command(self, service: int, method: int, data: bytes) -> bytes:
        try:
            return self._rust.command(service, method, data)
        except Exception as error:  # noqa: BLE001 - pylib's own handling, below
            error_bytes = bytes(error.args[0])
        err = backend_pb2.BackendError()
        err.ParseFromString(error_bytes)
        kinds = backend_pb2.BackendError
        if err.kind == kinds.DELETED:
            raise _RowError("missing")
        if err.kind == kinds.NOT_FOUND_ERROR or (
            err.kind == kinds.INVALID_INPUT and err.message == self._check_database
        ):
            raise _RowError("damaged")
        raise backend_exception_to_pylib(err)


@observation(KIND)
def card_excerpts(session, args):
    ids = [session.resolve(ref, "c") for ref in check_request(args)]
    col = session.col
    if col.get_config_bool(Config.Bool.BROWSER_TABLE_SHOW_NOTES_MODE):
        raise invalid("card_excerpts reads cards: browserTableShowNotesMode reads as true")
    backend = col._backend
    backend.set_active_browser_columns(list(COLUMNS))
    probe = _RowProbe(backend, col.tr.errors_please_check_database())
    out = []
    for cid in ids:
        try:
            row = type(backend).browser_row_for_id(probe, cid)
        except _RowError as exc:
            out.append({"error": exc.kind})
            continue
        if len(row.cells) != len(COLUMNS):
            raise DriverError("internal_error", detail="a Browse row has another number of cells than columns")
        out.append({column: cell.text for column, cell in zip(COLUMNS, row.cells)})
    return out
