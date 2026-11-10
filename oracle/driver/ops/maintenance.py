# SPDX-License-Identifier: AGPL-3.0-or-later
"""check_db (specs/conformance/collection-check.md K5, K10): Anki's Check
Database on the current collection; optimize (specs/conformance/
maintenance-ops.md M1): the desktop's automatic optimize on it.

check_db: {}   -> {<kind>: count} for every K5 kind, or a rejection (K10,
    nothing changed): corrupt when the integrity check finds the file corrupt,
    failed when Check Database stops with any other backend error (its repairs
    run in one transaction, which the error rolls back; the driver then
    reopens, since the rollback leaves Anki's deck and note type caches as
    the repairs left them).
optimize: {}   -> nothing. The collection's own optimize call: vacuum, then
    analyze, each through the raw query API with no begin or commit request.
    Anki then marks its proxy modified (a commit request would stamp col.mod;
    no op sends one, as for db_exec) and empties its undo history and study
    queues. A failure is a run error, never a rejection.
corrupt_page: {table} -> nothing (maintenance-ops.md M2). Closes the
    collection, folds a leftover WAL into the main file with a plain SQLite
    connection, overwrites the table's root page with 0xFF bytes and opens the
    collection again, so that the next check_db's integrity check finds the
    file corrupt. A failure is a run error, never a rejection. The oracle
    refuses it in actor scenarios and before a handoff (M2.3).

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

import os
import re
import sqlite3
from typing import Any, Callable

import anki.errors
from anki.collection import Collection

from ..errors import DriverError, Rejected, invalid
from ..registry import op, scenario_check

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


def database_problems(col) -> list[str] | None:
    """Run Check Database as the desktop command runs it (integrity check,
    optimize, repairs); its messages, or None when the file is corrupt."""
    try:
        return list(col._backend.check_database())
    except anki.errors.DBError as exc:
        if str(exc) == col.tr.database_check_corrupt():
            return None
        raise


def check_database(col) -> dict[str, int] | None:
    """Check Database's K5 counts, or None when the file is corrupt."""
    problems = database_problems(col)
    return None if problems is None else problem_counts(col.tr, problems)


@op("check_db", rejects=("corrupt", "failed"))
def check_db(session, args):
    if args != {}:
        raise invalid("check_db takes {}")
    try:
        counts = check_database(session.col)
    except anki.errors.BackendError as exc:
        # K10 failed: Anki raised before committing anything (a value out of
        # its stored range, a taken id, a refused deck add, a row it cannot
        # read). Its rollback empties the undo history and study queues but
        # not the deck and note type caches, which may still hold rows the
        # rollback removed (repaired note types, added parent decks): reopen
        # so that later steps read the file. The reopen loses nothing the
        # rollback kept. Nothing is logged; the message never leaves (R1887).
        session.reopen()
        raise Rejected("failed") from exc
    if counts is None:
        raise Rejected("corrupt")
    return counts


@op("optimize")
def optimize(session, args):
    if args != {}:
        raise invalid("optimize takes {}")
    session.col.optimize()


# The twelve tables of storage/schema-18 S2 (maintenance-ops.md M2.1).
SCHEMA_TABLES = (
    "col", "notes", "cards", "revlog", "graves", "deck_config",
    "config", "tags", "fields", "templates", "notetypes", "decks",
)


def damage_root_page(path: str, table: str) -> int:
    """M2.2 steps 1 to 3 on the closed collection file at path: fold a
    leftover WAL into it, then overwrite the table's root page with 0xFF
    bytes. Returns the page number written."""
    db = sqlite3.connect(path)
    try:
        if os.path.exists(path + "-wal"):
            db.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchall()
        row = db.execute("SELECT rootpage FROM sqlite_schema WHERE type = 'table' AND name = ?", (table,)).fetchone()
    finally:
        db.close()
    if row is None or not isinstance(row[0], int) or row[0] < 2:
        raise DriverError("internal_error", detail=f"the collection has no table {table} with a root page past page 1")
    page = row[0]
    with open(path, "r+b") as f:
        header = f.read(100)
        if len(header) < 100:
            raise DriverError("internal_error", detail="the collection file has no SQLite header")
        size = int.from_bytes(header[16:18], "big")
        size = 65536 if size == 1 else size
        f.seek((page - 1) * size)
        f.write(b"\xff" * size)
        f.flush()
        os.fsync(f.fileno())
    return page


@op("corrupt_page")
def corrupt_page(session, args):
    if not isinstance(args, dict) or set(args) != {"table"} or not isinstance(args["table"], str) \
            or args["table"] not in SCHEMA_TABLES:
        raise invalid("corrupt_page takes {table: <a storage/schema-18 S2 table>}")
    session.close_collection()
    damage_root_page(session.path, args["table"])
    # M2.2 step 4: Anki's open reads no table M2.3 lets a scenario damage.
    session.col = Collection(session.path)


@scenario_check
def check_corrupt_page_placement(scenario: dict) -> None:
    """M2.3: no corrupt_page in an actor scenario or before a handoff (the
    other side would resume from a damaged file)."""
    steps = scenario.get("steps", []) or []
    names = [next(iter(step)) if isinstance(step, dict) and len(step) == 1 else None for step in steps]
    if "corrupt_page" not in names:
        return
    first = names.index("corrupt_page")
    if scenario.get("actors"):
        raise invalid("corrupt_page is not allowed in an actor scenario (maintenance-ops.md M2.3)", step=first)
    if "handoff" in names[first:]:
        raise invalid("corrupt_page is not allowed before a handoff (maintenance-ops.md M2.3)",
                      step=first + names[first:].index("handoff"))
