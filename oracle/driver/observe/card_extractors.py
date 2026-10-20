# SPDX-License-Identifier: AGPL-3.0-or-later
"""card_extractors {inputs}: Anki's card-data SQL functions on given SQL values
(specs/conformance/extractor-ops.md E1-E5, specs/search/card-extractors.md).

Value: {rows: [{id, s, d, dr, r, rr, pos}, ...]}, one row per input in input
order.

The scenario's collection is never read or written. Every input is one SELECT
on the connection of one scratch collection per worker process (created on
first use in the worker's scratch directory, kept until the process ends,
never written), which calls each function once:

  s, d, dr  extract_fsrs_variable(data, 's' | 'd' | 'dr')
  r         extract_fsrs_retrievability(data, due, ivl, today, 0, now)
  rr        extract_fsrs_relative_retrievability(data, due, ivl, today, 0, now)
  pos       extract_original_position(data)

The 0 is the next day boundary, which neither function reads. Anki's
extract_fsrs_variable panics on any key but s, d and dr (a panic kills the
worker), so the keys are SQL literals here and nothing else is ever passed.

Binding (E2): the five arguments are numbered parameters, each bound once.
int is INTEGER, float REAL, str TEXT, None NULL. Anki's bridge sends values as
JSON and binds a list of byte values as a BLOB: blob_hex binds that list, and
text_hex wraps it in CAST(? AS TEXT), which keeps the bytes as they are.

Reading: the bridge returns rows as JSON, where an infinity has no form and
arrives as null, like NULL. So each REAL function is wrapped in
min(max(x, -MAX), MAX), MAX the largest double: multi-argument min and max
return NULL for a NULL argument and the argument itself otherwise, so NULL
stays NULL, -0.0 and every finite f32 pass unchanged, and an infinity becomes
MAX or -MAX, which no widened f32 equals.
"""

from __future__ import annotations

import math
import os
import re
import sys
import tempfile
from typing import Any

from anki.collection import Collection

from ..errors import DriverError, invalid
from ..numbers import f32_value, to_f32
from ..registry import observation, scenario_check
from ..session import parse_observation

MAX_INPUTS = 1000
INPUT_KEYS = frozenset(("id", "data", "due", "ivl", "today", "now"))
ARGUMENTS = ("data", "due", "ivl", "today", "now")  # ?1 to ?5
EXACT_INTEGERS = 2**53  # dsl.md D2: a plain integer lies within +-2^53
I64 = (-(2**63), 2**63 - 1)
INT_TEXT = re.compile(r"-?[0-9]{1,19}")
HEX = re.compile(r"(?:[0-9a-fA-F]{2})*")
MAX = sys.float_info.max
F32_MAX = 3.4028234663852886e38
MEMBERS = ("s", "d", "dr", "r", "rr")  # the REAL members, in SELECT order; pos follows


def _sql(args: list[str]) -> str:
    data, due, ivl, today, now = args

    def real(call: str) -> str:
        return f"min(max({call}, -{MAX!r}), {MAX!r})"

    calls = [real(f"extract_fsrs_variable({data}, '{key}')") for key in ("s", "d", "dr")]
    calls.append(real(f"extract_fsrs_retrievability({data}, {due}, {ivl}, {today}, 0, {now})"))
    calls.append(real(f"extract_fsrs_relative_retrievability({data}, {due}, {ivl}, {today}, 0, {now})"))
    calls.append(f"extract_original_position({data})")
    return "select " + ", ".join(calls)


def _hex(value: Any, key: str) -> list[int]:
    if not isinstance(value, str) or not HEX.fullmatch(value):
        raise invalid(f"card_extractors {key} is an even number of hex digits")
    return list(bytes.fromhex(value))


def _bind(value: Any, n: int) -> tuple[str, Any]:
    """An E2 scenario value -> (its SQL text with parameter ?n, the value bound there)."""
    param = f"?{n}"
    if value is None or isinstance(value, str):
        return param, value
    if isinstance(value, bool):
        raise invalid("a card_extractors value is not a boolean")
    if isinstance(value, int):
        if abs(value) > EXACT_INTEGERS:
            raise invalid("a card_extractors integer beyond 2^53 is written {int: \"<decimal>\"}")
        return param, value
    if isinstance(value, float):
        if not math.isfinite(value):
            raise invalid("a card_extractors number is finite")
        return param, value
    if isinstance(value, dict) and len(value) == 1:
        ((key, text),) = value.items()
        if key == "int":
            if not isinstance(text, str) or not INT_TEXT.fullmatch(text) or not I64[0] <= int(text) <= I64[1]:
                raise invalid("card_extractors int is decimal text within the signed 64-bit range")
            return param, int(text)
        if key == "text_hex":
            return f"CAST({param} AS TEXT)", _hex(text, key)
        if key == "blob_hex":
            return param, _hex(text, key)
    raise invalid("a card_extractors value is an integer, a number, a string, null, {int}, {text_hex} or {blob_hex}")


def check_request(args: Any) -> list[tuple[str, str, list]]:
    """extractor-ops.md E1, E2 -> [(id, sql, params)]; anything else is invalid_scenario."""
    if not isinstance(args, dict) or set(args) != {"inputs"}:
        raise invalid("card_extractors takes {inputs}")
    inputs = args["inputs"]
    if not isinstance(inputs, list) or not 1 <= len(inputs) <= MAX_INPUTS:
        raise invalid(f"card_extractors inputs is a list of 1 to {MAX_INPUTS} inputs")
    calls, seen = [], set()
    for inp in inputs:
        if not isinstance(inp, dict) or set(inp) != INPUT_KEYS:
            raise invalid(f"a card_extractors input has exactly the members {', '.join(sorted(INPUT_KEYS))}")
        ident = inp["id"]
        if not isinstance(ident, str) or not ident:
            raise invalid("card_extractors input id is a non-empty string")
        if ident in seen:
            raise invalid(f"card_extractors input id {ident!r} is used twice")
        seen.add(ident)
        bound = [_bind(inp[name], n) for n, name in enumerate(ARGUMENTS, 1)]
        calls.append((ident, _sql([text for text, _ in bound]), [value for _, value in bound]))
    return calls


@scenario_check
def check_card_extractors_requests(scenario: dict) -> None:
    """At most one card_extractors request per observe step (E1), each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe":
            continue
        requests = [args for kind, args in map(parse_observation, specs) if kind == "card_extractors"]
        if len(requests) > 1:
            raise invalid("an observe step has at most one card_extractors request", step=index)
        for args in requests:
            try:
                check_request(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


def real_value(value: Any, member: str) -> Any:
    """A clamped REAL result (module docstring) -> its E3 value."""
    if value is None:
        return None
    if not isinstance(value, float):
        raise DriverError("internal_error", detail=f"card_extractors: {member} is not a REAL or NULL")
    if value == MAX:
        return "inf"
    if value == -MAX:
        return "-inf"
    if not (math.isfinite(value) and abs(value) <= F32_MAX and to_f32(value) == value):
        raise DriverError("internal_error", detail=f"card_extractors: {member} is a REAL that is not a widened f32")
    return f32_value(value)


def _row(ident: str, values: list) -> dict:
    if len(values) != len(MEMBERS) + 1:
        raise DriverError("internal_error", detail="card_extractors: the SELECT gave another number of columns")
    row: dict[str, Any] = {"id": ident}
    for member, value in zip(MEMBERS, values):
        row[member] = real_value(value, member)
    pos = values[-1]
    if pos is not None and (not isinstance(pos, int) or isinstance(pos, bool)):
        raise DriverError("internal_error", detail="card_extractors: pos is not an INTEGER or NULL")
    row["pos"] = pos
    return row


class _Scratch:
    """The worker's scratch collection: only its connection is used."""

    def __init__(self, root: str) -> None:
        self.dir = tempfile.mkdtemp(prefix="card-extractors-", dir=root)
        self.col = Collection(os.path.join(self.dir, "collection.anki2"))


_scratch: _Scratch | None = None


def _get_scratch(session) -> _Scratch:
    global _scratch
    if _scratch is None:
        _scratch = _Scratch(os.path.dirname(session.dir))
    return _scratch


@observation("card_extractors")
def card_extractors(session, args):
    calls = check_request(args)
    db = _get_scratch(session).col.db
    return {"rows": [_row(ident, db.first(sql, *params)) for ident, sql, params in calls]}
