# SPDX-License-Identifier: AGPL-3.0-or-later
"""tag_complete {cases: [{input, limit}, ...]} (specs/tags/complete.md C7).

Value: [[<tag name>, ...], ...], one list per case in case order: the
backend's tag completion with `input` as given and match_limit `limit` - 1
(Anki returns up to match_limit + 1 tags, Meguri's `limit` is that count,
C4), in the order returned. `limit` is an integer from 1 to 2^32 (match_limit
is a uint32); `input` any string that UTF-8 can carry. Reading changes
nothing. Request shapes are checked before any collection exists, at most one
request per observe step (its value is a list, O2).
"""

from __future__ import annotations

from typing import Any

from ..errors import DriverError, invalid
from ..registry import observation, scenario_check
from ..session import parse_observation

LIMIT_MAX = 2**32


def _check_case(case: Any) -> tuple[str, int]:
    if not isinstance(case, dict) or set(case) != {"input", "limit"}:
        raise invalid("a tag_complete case takes {input, limit}")
    text, limit = case["input"], case["limit"]
    if not isinstance(text, str):
        raise invalid("a tag_complete input is a string; quote numbers, booleans and null")
    try:
        text.encode("utf-8")
    except UnicodeEncodeError:
        raise invalid("a tag_complete input holds a lone surrogate") from None
    if isinstance(limit, bool) or not isinstance(limit, int) or not 1 <= limit <= LIMIT_MAX:
        raise invalid(f"a tag_complete limit is an integer from 1 to {LIMIT_MAX}")
    return text, limit


def check_request(args: Any) -> list[tuple[str, int]]:
    if not isinstance(args, dict) or set(args) != {"cases"}:
        raise invalid("tag_complete takes {cases}")
    cases = args["cases"]
    if not isinstance(cases, list) or not cases:
        raise invalid("tag_complete cases are a non-empty list")
    return [_check_case(case) for case in cases]


@scenario_check
def check_tag_complete_requests(scenario: dict) -> None:
    """At most one tag_complete request per observe step (O2), each well formed."""
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, specs),) = step.items()
        if name != "observe" or not isinstance(specs, list):
            continue
        requests = [args for kind, args in map(parse_observation, specs) if kind == "tag_complete"]
        if len(requests) > 1:
            raise invalid("an observe step has at most one tag_complete request", step=index)
        for args in requests:
            try:
                check_request(args)
            except DriverError as exc:
                exc.fields.setdefault("step", index)
                raise


@observation("tag_complete")
def tag_complete(session, args):
    backend = session.col._backend
    return [list(backend.complete_tag(input=text, match_limit=limit - 1)) for text, limit in check_request(args)]
