# SPDX-License-Identifier: AGPL-3.0-or-later
"""db_dump [{tables: [...]}]: the stored rows of the current collection
(specs/conformance/observations.md O8).

{<table>: {<primary key as text>: {<column>: <value>}}} for the twelve tables
of a schema-18 file. A value follows the SQLite storage class of what is
stored: INTEGER int, REAL float (JSON writes the shortest round-trip decimal),
TEXT str, NULL None, BLOB {"sha256": <hex of the bytes>}. The observation
hides what Anki draws at random: notes.guid reads "#random", and the digests of
fields.config and templates.config skip the field's and template's random id
(top-level protobuf fields 9 and 8). `dump(col, raw=True)` keeps them: that is
the raw dump `converge` compares (sync-ops.md S7.2).

Columns come from SQLite's table-valued pragma inside a SELECT: a bare PRAGMA
goes through the DB proxy as a modification, which would mark the collection
modified on the next commit.
"""

from __future__ import annotations

import hashlib
from typing import Any

from ..errors import invalid
from ..registry import observation

TABLES = (
    "col", "notes", "cards", "revlog", "graves", "decks", "deck_config",
    "notetypes", "fields", "templates", "tags", "config",
)
# table -> the columns whose values, joined by ",", key a row
KEYS = {
    "col": ("id",), "notes": ("id",), "cards": ("id",), "revlog": ("id",),
    "decks": ("id",), "deck_config": ("id",), "notetypes": ("id",),
    "graves": ("oid", "type"), "fields": ("ntid", "ord"), "templates": ("ntid", "ord"),
    "tags": ("tag",), "config": ("KEY",),
}
# Random ids inside protobuf configs: (table, column) -> top-level field number.
RANDOM_PROTO_FIELDS = {("fields", "config"): 9, ("templates", "config"): 8}
RANDOM_TEXT = {("notes", "guid"): "#random"}


def _varint(buf: bytes, pos: int) -> tuple[int, int] | None:
    value = shift = 0
    while pos < len(buf) and shift < 64:
        byte = buf[pos]
        pos += 1
        value |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return value, pos
        shift += 7
    return None


def without_field(blob: bytes, number: int) -> bytes:
    """blob without every top-level occurrence of field `number`, other bytes
    in order; blob itself when it is not a sequence of well-formed fields."""
    kept, pos = [], 0
    while pos < len(blob):
        start = pos
        tag = _varint(blob, pos)
        if tag is None:
            return blob
        key, pos = tag
        field, wire = key >> 3, key & 7
        if field < 1:
            return blob
        if wire == 0:
            value = _varint(blob, pos)
            if value is None:
                return blob
            pos = value[1]
        elif wire == 1:
            pos += 8
        elif wire == 2:
            length = _varint(blob, pos)
            if length is None:
                return blob
            pos = length[1] + length[0]
        elif wire == 5:
            pos += 4
        else:
            return blob
        if pos > len(blob):
            return blob
        if field != number:
            kept.append(blob[start:pos])
    return b"".join(kept)


def _columns(col, table: str) -> list[str]:
    return col.db.list(f"select name from pragma_table_info('{table}') order by cid")


def dump_table(col, table: str, raw: bool) -> dict[str, dict[str, Any]]:
    columns = _columns(col, table)
    # typeof() gives the storage class; a BLOB travels as hex text.
    exprs = ", ".join(
        f"typeof(\"{c}\"), case typeof(\"{c}\") when 'blob' then hex(\"{c}\") else \"{c}\" end" for c in columns
    )
    # Column names as the stored definition spells them: a table an old release
    # created keeps its letter case (config's `key` at schemas 14 to 16).
    keys = [next(c for c in columns if c.lower() == k.lower()) for k in KEYS[table]]
    rows: dict[str, dict[str, Any]] = {}
    for flat in col.db.all(f"select {exprs} from \"{table}\""):
        row: dict[str, Any] = {}
        for i, name in enumerate(columns):
            kind, value = flat[2 * i], flat[2 * i + 1]
            if kind == "blob":
                data = bytes.fromhex(value)
                if not raw and (table, name) in RANDOM_PROTO_FIELDS:
                    data = without_field(data, RANDOM_PROTO_FIELDS[(table, name)])
                value = {"sha256": hashlib.sha256(data).hexdigest()}
            elif kind == "real":
                value = float(value)
            if not raw and (table, name) in RANDOM_TEXT:
                value = RANDOM_TEXT[(table, name)]
            row[name] = value
        rows[",".join(str(row[k]) for k in keys)] = row
    return rows


def dump(col, tables: tuple[str, ...] | list[str] = TABLES, raw: bool = False) -> dict[str, dict]:
    return {t: dump_table(col, t, raw) for t in tables}


def _tables(args: Any) -> list[str]:
    if args is None:
        return list(TABLES)
    if not isinstance(args, dict) or set(args) != {"tables"} or not isinstance(args["tables"], list):
        raise invalid("db_dump takes nothing or {tables: [...]}")
    tables = args["tables"]
    unknown = [t for t in tables if t not in TABLES]
    if unknown or len(set(tables)) != len(tables):
        raise invalid(f"db_dump tables are distinct names among {', '.join(TABLES)}")
    return tables


@observation("db_dump")
def db_dump(session, args):
    return dump(session.col, _tables(args))
