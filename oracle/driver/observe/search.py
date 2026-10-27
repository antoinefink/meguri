# SPDX-License-Identifier: AGPL-3.0-or-later
"""search and canonical_search (specs/conformance/search-ops.md S1-S3).

search [{query, kind?: cards | notes, order?: {column, reverse?}}] ->
    [{ids: [...]} | error, ...]: Anki's card or note search on the scenario's
    collection. Without order: no order, and ids are the logical refs (#cN or
    #nN) by number, then the real ids no logical id names, ascending. With
    order (browser-ops.md B1, B2): the backend's search with the built-in sort
    for that column key and direction; ids in that order, unnamed rows as real
    ids at their place, and each equal-key run listed as an unordered result
    is (refs by number, then real ids ascending) within the positions it
    holds. A run is found from Anki alone: the search is also sorted the other
    way, and two consecutive rows belong to one run when they keep their
    relative order there (Anki reverses every key but leaves equal-key rows in
    one order for both directions).
canonical_search [text, ...] -> [{text} | parse error, ...]: the backend's
    search-string builder given the text as one search (what the browser puts
    back in its search field). It reads and changes nothing in the collection.

Anki reports a search error only as a message in the backend's language. The
kind is read by matching the message against the backend's own translations of
every search-error message, rendered with sentinel arguments; Fluent wraps each
placeable in U+2068 ... U+2069, and those marks cut provided and context out of
the message (so neither may hold a mark). Two reasons Anki leaves untranslated
(id lists, dupe:) match as Anki writes them. Backticks in provided and context
are already apostrophes in the message. An invalid-input error whose message is
a regex parse error is invalid_regex, and a database error while a search runs
is search_failed. Anything else, a message that matches no kind or more than
one included, is anki_error: never a guess. No message text leaves the driver
(R1887).

A sorted search follows B1's error order: a parse error, then unsortable, then
the run errors. Anki refuses a column it cannot sort (an unknown key included)
with an invalid-input error, raised after the query is parsed and its SQL
written, so a bad regex can come first. An invalid-input error is therefore
unsortable exactly when the empty search sorted by the same column in the same
mode is refused too; otherwise it is read as for an unsorted search. pylib's
find_cards and find_notes quietly drop a sort the column does not support, so
sorted searches call the backend search with the built-in sort order.
"""

from __future__ import annotations

import re
from typing import Any, Callable

import anki.errors
from anki import search_pb2

from ..errors import DriverError, invalid
from ..registry import observation

FSI, PDI = "⁨", "⁩"  # Fluent's isolation marks around a placeable

# Translation method (ftl search) -> kind, for the messages without arguments.
PLAIN = {
    "search_misplaced_and": "misplaced_and",
    "search_misplaced_or": "misplaced_or",
    "search_empty_group": "empty_group",
    "search_unopened_group": "unopened_group",
    "search_unclosed_group": "unclosed_group",
    "search_empty_quote": "empty_quote",
    "search_unclosed_quote": "unclosed_quote",
    "search_missing_key": "missing_key",
    "search_invalid_flag_2": "invalid_flag",
    "search_invalid_other": "malformed",
}
# Messages with (context, provided).
NUMBERS = {
    "search_invalid_number": "invalid_number",
    "search_invalid_whole_number": "invalid_whole_number",
    "search_invalid_positive_whole_number": "invalid_positive_whole_number",
    "search_invalid_negative_whole_number": "invalid_negative_whole_number",
    "search_invalid_answer_button": "invalid_answer_button",
}
# Messages with one argument, the provided text.
PROVIDED = {
    "search_unknown_escape": ("val", "unknown_escape"),
    "search_invalid_prop_operator": ("val", "invalid_operator"),
}
# search_invalid_argument(term, argument): the term names the kind.
ARGUMENT_TERMS = {"is:": "invalid_state", "prop:": "invalid_property"}
# Reasons Anki writes in English whatever the language.
ID_LIST = re.compile(r"expected only digits and commas in ((?:[nN]|[cC]|[dD])[iI][dD]:)")
DUPE = "invalid 'dupe:' search"
REGEX_PREFIX = "<pre>regex parse error"

Classifier = Callable[[str], "dict | None"]


def _error(kind: str, provided: str | None = None, context: str | None = None) -> dict:
    out = {"error": kind}
    if provided is not None:
        out["provided"] = provided
    if context is not None:
        out["context"] = context
    return out


def _rendered(render: Callable[..., str], names: tuple[str, ...], what: str) -> tuple[str, dict[str, str]]:
    """The message rendered with one sentinel per argument; each sentinel must
    appear once, between isolation marks, or its argument cannot be cut out."""
    sentinels = {name: f"MEGURISENTINEL{i}X" for i, name in enumerate(names)}
    text = render(**sentinels)
    for name, sentinel in sentinels.items():
        if text.count(sentinel) != 1 or text.count(FSI + sentinel + PDI) != 1:
            raise DriverError("internal_error", detail=f"{what} does not show its {name} once between isolation marks")
    return text, sentinels


def _pattern(render: Callable[..., str], names: tuple[str, ...], what: str) -> re.Pattern:
    """The message as a pattern with one named group per argument: the text
    between that placeable's isolation marks."""
    text, sentinels = _rendered(render, names, what)
    by_sentinel = {FSI + s + PDI: name for name, s in sentinels.items()}
    pieces = re.split("(" + "|".join(map(re.escape, by_sentinel)) + ")", text)
    body = "".join(
        f"{FSI}(?P<{by_sentinel[p]}>[^{FSI}{PDI}]*){PDI}" if i % 2 else re.escape(p) for i, p in enumerate(pieces)
    )
    return re.compile(body, re.DOTALL)


def _matcher(pattern: re.Pattern, make: Callable[[re.Match], "dict | None"]) -> Classifier:
    def match(reason: str) -> dict | None:
        m = pattern.fullmatch(reason)
        return make(m) if m else None

    return match


class Reasons:
    """Search error messages -> error outputs, from `tr`, a collection's
    translations."""

    def __init__(self, tr: Any) -> None:
        text, sentinels = _rendered(tr.search_invalid_search, ("reason",), "search_invalid_search")
        # The text around the reason, its isolation marks included.
        self.before, self.after = text.split(sentinels["reason"])
        table: list[Classifier] = []
        for method, kind in PLAIN.items():
            plain = getattr(tr, method)()
            table.append(lambda r, plain=plain, kind=kind: _error(kind) if r == plain else None)
        for method, kind in NUMBERS.items():
            table.append(_matcher(
                _pattern(getattr(tr, method), ("context", "provided"), method),
                lambda m, kind=kind: _error(kind, provided=m["provided"], context=m["context"]),
            ))
        for method, (name, kind) in PROVIDED.items():
            table.append(_matcher(
                _pattern(getattr(tr, method), (name,), method),
                lambda m, name=name, kind=kind: _error(kind, provided=m[name]),
            ))
        table.append(_matcher(
            _pattern(tr.search_invalid_argument, ("term", "argument"), "search_invalid_argument"),
            lambda m: _error(ARGUMENT_TERMS[m["term"]], provided=m["argument"]) if m["term"] in ARGUMENT_TERMS else None,
        ))
        table.append(_matcher(ID_LIST, lambda m: _error("invalid_id_list", context=m[1])))
        table.append(lambda r: _error("invalid_dupe") if r == DUPE else None)
        self.table = table

    def classify(self, message: str) -> dict | None:
        """The error output for a search error's message, or None when it
        matches no kind or more than one."""
        if (
            len(message) < len(self.before) + len(self.after)
            or not message.startswith(self.before)
            or not message.endswith(self.after)
        ):
            return None
        reason = message[len(self.before):len(message) - len(self.after)]
        found = [out for match in self.table if (out := match(reason)) is not None]
        return found[0] if len(found) == 1 else None


def _search_error(reasons: Reasons, exc: anki.errors.SearchError) -> dict:
    out = reasons.classify(str(exc))
    if out is None:
        raise exc  # anki_error through the session's guard, without the message
    return out


def _order(value: Any) -> tuple[str, bool]:
    if (
        not isinstance(value, dict)
        or set(value) - {"column", "reverse"}
        or not isinstance(value.get("column"), str)
        or not isinstance(value.get("reverse", False), bool)
    ):
        raise invalid("a search request's order is {column: <text>, reverse?: <boolean>}")
    return value["column"], value.get("reverse", False)


def _requests(args: Any) -> list[tuple[str, str, "tuple[str, bool] | None"]]:
    if not isinstance(args, list) or not args:
        raise invalid("search takes a non-empty list of {query, kind?, order?}")
    out = []
    for req in args:
        if not isinstance(req, dict) or not isinstance(req.get("query"), str) or set(req) - {"query", "kind", "order"}:
            raise invalid("a search request is {query: <text>, kind?: cards | notes, order?: {column, reverse?}}")
        kind = req.get("kind", "cards")
        if kind not in ("cards", "notes"):
            raise invalid("a search request's kind is cards or notes")
        out.append((req["query"], kind, _order(req["order"]) if "order" in req else None))
    return out


def _sorted(col, kind: str, query: str, column: str, reverse: bool) -> list[int]:
    """The backend's card or note search with its built-in sort."""
    order = search_pb2.SortOrder(builtin=search_pb2.SortOrder.Builtin(column=column, reverse=reverse))
    run = col._backend.search_cards if kind == "cards" else col._backend.search_notes
    return list(run(search=query, order=order))


def _unsortable(col, kind: str, column: str, reverse: bool) -> bool:
    """True when Anki refuses to sort the empty search by column in this mode."""
    try:
        _sorted(col, kind, "", column, reverse)
    except anki.errors.InvalidInput:
        return True
    return False


def _logical(rows: list[int], prefix: str, index: dict[int, int]) -> list:
    """rows as an unordered result lists them: refs by number, then real ids."""
    named = sorted(index[i] for i in rows if i in index)
    return [f"{prefix}{n}" for n in named] + sorted(i for i in rows if i not in index)


def _runs(found: list[int], other: list[int], prefix: str, index: dict[int, int]) -> list:
    """found (one direction) with each equal-key run in logical order; other is
    the same search sorted the other way (B2)."""
    where = {rid: i for i, rid in enumerate(other)}
    if len(where) != len(other) or len(other) != len(found) or set(where) != set(found):
        raise DriverError("internal_error", detail="the search sorted the other way returned other rows")
    out: list = []
    run: list[int] = []
    for rid in found:
        if run and where[run[-1]] > where[rid]:
            out.extend(_logical(run, prefix, index))
            run = []
        run.append(rid)
    out.extend(_logical(run, prefix, index))
    return out


@observation("search")
def search(session, args):
    requests = _requests(args)
    col = session.col
    reasons = Reasons(col.tr)
    numbers = {
        "cards": ("#c", {cid: n for n, cid in enumerate(session.cards, 1)}),
        "notes": ("#n", {nid: n for n, nid in enumerate(session.notes, 1)}),
    }
    out = []
    for query, kind, order in requests:
        try:
            if order is None:
                find = col.find_cards if kind == "cards" else col.find_notes
                found = list(find(query, order=False))
            else:
                found = _sorted(col, kind, query, *order)
        except anki.errors.SearchError as exc:
            out.append(_search_error(reasons, exc))
            continue
        except anki.errors.InvalidInput as exc:
            if order is not None and _unsortable(col, kind, *order):
                out.append(_error("unsortable"))
                continue
            if not str(exc).startswith(REGEX_PREFIX):
                raise
            out.append(_error("invalid_regex"))
            continue
        except anki.errors.DBError:
            out.append(_error("search_failed"))
            continue
        prefix, index = numbers[kind]
        if order is None:
            out.append({"ids": _logical(found, prefix, index)})
        else:
            column, reverse = order
            other = _sorted(col, kind, query, column, not reverse)
            out.append({"ids": _runs(found, other, prefix, index)})
    return out


@observation("canonical_search")
def canonical_search(session, args):
    if not isinstance(args, list) or not args or not all(isinstance(t, str) for t in args):
        raise invalid("canonical_search takes a non-empty list of search texts")
    col = session.col
    reasons = Reasons(col.tr)
    out = []
    for text in args:
        try:
            out.append({"text": col.build_search_string(text)})
        except anki.errors.SearchError as exc:
            out.append(_search_error(reasons, exc))
    return out
