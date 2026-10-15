# SPDX-License-Identifier: AGPL-3.0-or-later
"""check_db (specs/conformance/collection-check.md K5, K10): Anki's Check
Database on the current collection.

check_db: {}   -> {<kind>: count} for every K5 kind, or the rejection corrupt
    when the integrity check finds the file corrupt (nothing changed).

Anki reports problems only as messages in the backend's language. Each kind is
read from its message under the backend's own translations: the message for a
count of 1 matches exactly, any other count matches the message rendered with a
sentinel count, the sentinel's digits standing for the number. Every message
must match exactly one kind, and each kind may appear once; anything else is a
driver error, never a guess. notetypes_recovered has no number in its message:
1 when reported. The corrupt refusal is a DB error whose text is the corrupt
message itself.

The messages never leave the driver (R1887).
"""

from __future__ import annotations

import re
from typing import Any, Callable

import anki.errors

from ..errors import DriverError, Rejected, invalid
from ..registry import op

# K5 kind -> the translation method of its message (ftl database-check), in
# the order of the K5 table.
COUNTED = {
    "card_properties": "database_check_card_properties",
    "new_positions_too_high": "database_check_new_card_high_due",
    "card_last_review_time": "database_check_card_last_review_time_empty",
    "cards_without_note": "database_check_card_missing_note",
    "decks_missing": "database_check_missing_decks",
    "field_counts": "database_check_field_count",
    "duplicate_card_ords": "database_check_duplicate_card_ords",
    "cards_without_template": "database_check_missing_templates",
    "revlog_properties": "database_check_revlog_properties",
    "notes_invalid_utf8": "database_check_notes_with_invalid_utf8",
    "future_ids": "database_check_fixed_invalid_ids",
}
UNCOUNTED = {"notetypes_recovered": "database_check_notetypes_recovered"}
KINDS = tuple(COUNTED) + tuple(UNCOUNTED)
SENTINEL = 9876543

Matcher = Callable[[str], "int | None"]


def _counted_matcher(render: Callable[..., str], kind: str) -> Matcher:
    one = render(count=1)
    other = render(count=SENTINEL)
    parts = other.split(str(SENTINEL))
    if len(parts) != 2:
        raise DriverError("internal_error", detail=f"the {kind} message does not show its count once")
    pattern = re.compile(re.escape(parts[0]) + "([0-9]+)" + re.escape(parts[1]))

    def match(text: str) -> int | None:
        if text == one:
            return 1
        m = pattern.fullmatch(text)
        return int(m.group(1)) if m else None

    return match


def matchers(tr: Any) -> list[tuple[str, Matcher]]:
    """(kind, text -> count or None) for every K5 kind, from `tr`, a
    collection's translations."""
    out = [(kind, _counted_matcher(getattr(tr, method), kind)) for kind, method in COUNTED.items()]
    for kind, method in UNCOUNTED.items():
        text = getattr(tr, method)()
        out.append((kind, lambda t, text=text: 1 if t == text else None))
    return out


def problem_counts(tr: Any, problems: list[str]) -> dict[str, int]:
    """K5 counts from Check Database's messages."""
    table = matchers(tr)
    counts = dict.fromkeys(KINDS, 0)
    seen: set[str] = set()
    for index, text in enumerate(problems):
        found = [(kind, n) for kind, match in table if (n := match(text)) is not None]
        if len(found) != 1:
            raise DriverError("internal_error", detail=f"Check Database message {index} matches {len(found)} K5 kinds")
        ((kind, n),) = found
        if kind in seen:
            raise DriverError("internal_error", detail=f"Check Database reported {kind} twice")
        seen.add(kind)
        counts[kind] = n
    return counts


def check_database(col) -> dict[str, int] | None:
    """Run Check Database as the desktop command runs it (integrity check,
    optimize, repairs); its K5 counts, or None when the file is corrupt."""
    try:
        problems = list(col._backend.check_database())
    except anki.errors.DBError as exc:
        if str(exc) == col.tr.database_check_corrupt():
            return None
        raise
    return problem_counts(col.tr, problems)


@op("check_db", rejects=("corrupt",))
def check_db(session, args):
    if args != {}:
        raise invalid("check_db takes {}")
    counts = check_database(session.col)
    if counts is None:
        raise Rejected("corrupt")
    return counts
