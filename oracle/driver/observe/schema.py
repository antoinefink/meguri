# SPDX-License-Identifier: AGPL-3.0-or-later
"""schema_objects: the objects of the collection file (specs/conformance/storage-ops.md P3).

[{type, name, tbl_name, sql}] for every row of the main database's
sqlite_master (temporary objects live in sqlite_temp_master and are not
listed), sorted by type, then name, in code point order. sql is normalized:
every `--` comment removed up to the end of its line, each run of spaces, tabs,
carriage returns and line feeds made one space, then trimmed; null when SQLite
stores no text (automatic indexes).
"""

from __future__ import annotations

import re

from ..errors import invalid
from ..registry import observation

_COMMENT = re.compile(r"--[^\n]*")
_SPACE = re.compile(r"[ \t\r\n]+")


def normalize(sql: str | None) -> str | None:
    if sql is None:
        return None
    return _SPACE.sub(" ", _COMMENT.sub("", sql)).strip(" ")


# The rows listing() takes; file_objects (ops/schema11.py) reads them from a session file.
MASTER_QUERY = "select type, name, tbl_name, sql from sqlite_master"


def listing(rows) -> list[dict]:
    """sqlite_master rows (type, name, tbl_name, sql) -> the observation value."""
    objects = [{"type": t, "name": n, "tbl_name": tb, "sql": normalize(sql)} for t, n, tb, sql in rows]
    return sorted(objects, key=lambda o: (o["type"], o["name"]))


@observation("schema_objects")
def schema_objects(session, args):
    if args is not None:
        raise invalid("schema_objects takes no arguments")
    return listing(session.col.db.all(MASTER_QUERY))
