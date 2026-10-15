# SPDX-License-Identifier: AGPL-3.0-or-later
"""The worker side of `tools/mg oracle check` (specs/conformance/collection-check.md).

check: {dir, now, decks?, render_notes?} -> the report's {now, tz, timing,
counts, decks, render, check_db, changed_tables} (K3-K8), or the run errors
no_collection and open_failed (K7).

The worker copies <dir>/collection.anki2 (and its -wal) into its scratch, sets
its clock to `now` and opens the copy with the Python layer, in English, as the
desktop client does. Anki holds an open collection under an exclusive lock, so
the table digests (K3 steps 6 and 8) read the file with SQLite while it is
closed: after steps 1-5 the collection closes, is digested, opens again (which
writes nothing to a file Anki already opened) and runs Check Database, then
closes and is digested again. A digest covers each O8 table's rows in key
order, every column as its storage class and raw bytes (text never decoded).
"""

from __future__ import annotations

import hashlib
import os
import shutil
import sqlite3
import struct
import tempfile
from typing import Any

import anki.errors
from anki._backend import RustBackend
from anki.collection import Collection
from anki.template import TemplateRenderContext

from .clock import Clock
from .errors import DriverError, log_exception
from .observe import timing
from .observe.db_dump import KEYS, TABLES
from .ops.maintenance import check_database

COUNTED_TABLES = (
    ("notes", "notes"), ("cards", "cards"), ("revlog", "revlog"), ("notetypes", "notetypes"),
    ("decks", "decks"), ("presets", "deck_config"), ("tags", "tags"), ("graves", "graves"),
)
REQUEST_KEYS = {"cmd", "id", "dir", "now", "decks", "render_notes"}
I64 = (-(2**63), 2**63 - 1)


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def check_request(req: dict) -> tuple[str, int, list[str], list[int]]:
    unknown = sorted(set(req) - REQUEST_KEYS)
    decks = req.get("decks", [])
    notes = req.get("render_notes", [])
    if (
        unknown
        or not isinstance(req.get("dir"), str)
        or not req["dir"]
        or not _is_int(req.get("now"))
        or req["now"] < 0
        or not isinstance(decks, list)
        or not all(isinstance(d, str) for d in decks)
        or not isinstance(notes, list)
        or not all(_is_int(n) and I64[0] <= n <= I64[1] for n in notes)
    ):
        raise DriverError("protocol_error", detail="check is {dir, now, decks?: [names], render_notes?: [note ids]}")
    return req["dir"], req["now"], decks, notes


def run(req: dict, tz: str, scratch: str, clock: Clock) -> dict:
    directory, now, deck_names, note_ids = check_request(req)
    source = os.path.join(directory, "collection.anki2")
    if not os.path.isfile(source):
        raise DriverError("no_collection", detail="the directory holds no collection.anki2")
    work = tempfile.mkdtemp(prefix="check-", dir=scratch)
    try:
        path = os.path.join(work, "collection.anki2")
        shutil.copyfile(source, path)
        if os.path.isfile(source + "-wal"):
            shutil.copyfile(source + "-wal", path + "-wal")
        clock.set(now)
        col = _open(path)
        try:
            report: dict[str, Any] = {"now": now, "tz": tz}
            report["counts"] = _stage("counts", lambda: {
                name: col.db.scalar(f"select count() from {table}") for name, table in COUNTED_TABLES
            })
            report["timing"] = _stage("timing", lambda: timing.value(col))
            report["decks"] = _stage("decks", lambda: deck_counts(col, deck_names))
            report["render"] = _stage("render", lambda: render_notes(col, note_ids))
        finally:
            _close(col)
        # A corrupt file may fail to digest; Check Database then says so.
        try:
            before = table_digests(path)
        except sqlite3.DatabaseError as exc:
            log_exception(exc, "digesting the collection before Check Database")
            before = None
        col = _stage("reopen", lambda: _new_collection(path))
        try:
            counts = _stage("check_db", lambda: check_database(col))
        finally:
            _close(col)
        if counts is None:
            report["check_db"] = {"error": "corrupt"}
            report["changed_tables"] = []
            return report
        if before is None:
            raise DriverError("internal_error", op="check", detail="the file could not be digested, yet Check Database found it sound")
        after = _stage("digest", lambda: table_digests(path))
        report["check_db"] = counts
        report["changed_tables"] = [t for t in TABLES if before[t] != after[t]]
        return report
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _new_collection(path: str) -> Collection:
    return Collection(path, backend=RustBackend(langs=["en"]))


def _open(path: str) -> Collection:
    try:
        return _new_collection(path)
    except Exception as exc:  # noqa: BLE001 - whatever Anki raises, it could not open the file
        log_exception(exc, "open_failed")
        raise DriverError("open_failed", exception=type(exc).__name__) from None


def _close(col: Collection) -> None:
    try:
        col.close(downgrade=False)
    except Exception as exc:  # noqa: BLE001 - the copy goes away anyway
        log_exception(exc, "closing the checked collection")
        raise DriverError("anki_error", op="check", exception=type(exc).__name__, detail="closing") from None


def _stage(name: str, fn):
    try:
        return fn()
    except DriverError:
        raise
    except anki.errors.AnkiException as exc:
        log_exception(exc, f"anki_error in check ({name})")
        raise DriverError("anki_error", op="check", exception=type(exc).__name__, detail=name) from None
    except Exception as exc:  # noqa: BLE001 - a driver bug: keep the worker alive
        log_exception(exc, f"internal_error in check ({name})")
        raise DriverError("internal_error", op="check", exception=type(exc).__name__, detail=name) from None


def deck_counts(col: Collection, names: list[str]) -> dict[str, Any]:
    """K6: the deck list's new, learning and review counts of each named deck."""
    if not names:
        return {}
    tree = col.sched.deck_due_tree()
    out: dict[str, Any] = {}
    for name in names:
        did = col.decks.id_for_name(name)
        node = col.decks.find_deck_in_tree(tree, did) if did else None
        out[name] = None if node is None else {
            "new": node.new_count, "learning": node.learn_count, "review": node.review_count,
        }
    return out


def render_notes(col: Collection, note_ids: list[int]) -> dict[str, Any]:
    """K8: each note's cards in ord order, rendered as for review."""
    out: dict[str, Any] = {}
    for nid in note_ids:
        if not col.db.scalar("select count() from notes where id = ?", nid):
            out[str(nid)] = None
            continue
        cids = col.db.list("select id from cards where nid = ? order by ord, id", nid)
        out[str(nid)] = [render_card(col.get_card(cid)) for cid in cids]
    return out


def render_card(card) -> dict:
    ctx = TemplateRenderContext.from_existing_card(card, browser=False)
    entry = {"card": card.id, "ord": card.ord}
    try:
        partial = ctx._partially_render()
    except anki.errors.TemplateError:
        return {**entry, "status": "template_error", "question_bytes": 0, "answer_bytes": 0}
    output = ctx.render()
    return {
        **entry,
        "status": "empty" if partial.is_empty else "ok",
        "question_bytes": len(output.question_text.encode("utf-8")),
        "answer_bytes": len(output.answer_text.encode("utf-8")),
    }


# --- digests ---------------------------------------------------------------------


def _unicase(a: str, b: str) -> int:
    """Stands in for Anki's collation so SQLite can open cursors on its
    indexes; the digests never order by it."""
    x, y = a.casefold(), b.casefold()
    return (x > y) - (x < y)


def _value_bytes(kind: str, value: Any) -> bytes:
    if kind == "null":
        return b"n"
    if kind == "integer":
        return b"i" + struct.pack("<q", value)
    if kind == "real":
        return b"r" + struct.pack("<d", value)
    tag = b"t" if kind == "text" else b"b"
    data = value if isinstance(value, bytes) else str(value).encode("utf-8")
    return tag + struct.pack("<Q", len(data)) + data


def table_digests(path: str) -> dict[str, str]:
    """sha256 per O8 table of a closed collection file."""
    con = sqlite3.connect(path)
    try:
        con.text_factory = bytes
        con.create_collation("unicase", _unicase)
        con.execute("pragma query_only = 1")
        return {table: _digest(con, table) for table in TABLES}
    finally:
        con.close()


def _digest(con: sqlite3.Connection, table: str) -> str:
    columns = [row[0].decode("utf-8") for row in con.execute(f"select name from pragma_table_info('{table}') order by cid")]
    keys = [next(c for c in columns if c.lower() == k.lower()) for k in KEYS[table]]
    exprs = ", ".join(f'typeof("{c}"), "{c}"' for c in columns)
    order = ", ".join(f'"{k}" collate binary' for k in keys)
    h = hashlib.sha256()
    for row in con.execute(f'select {exprs} from "{table}" order by {order}'):
        for i in range(0, len(row), 2):
            h.update(_value_bytes(row[i].decode("ascii"), row[i + 1]))
        h.update(b"\xff")
    return h.hexdigest()
