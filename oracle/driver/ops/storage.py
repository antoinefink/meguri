# SPDX-License-Identifier: AGPL-3.0-or-later
"""Stored rows (record-ops.md W1).

db_exec: {sql, params?}   one statement through the collection's raw query API,
    with no begin or commit request: a commit request would set col.mod after
    a non-select statement. Anki itself clears the undo history and the study
    queues after a statement that does not start with `select`.
    -> {rows: [[value, ...], ...]}, by the storage class of each value: INTEGER
    int, REAL float, TEXT str (Anki's bridge already replaced invalid UTF-8
    with U+FFFD), BLOB {sha256} (it arrives as a list of byte values), NULL null.

Params bind as given: str TEXT, int INTEGER, float REAL, None NULL. A JSON
integer outside 64 bits cannot bind as INTEGER (Anki's bridge would bind a
large one as REAL), so it is invalid_scenario, as booleans, lists and mappings.

db_exec is unit/U23's op (U23 also adds set_config to this module); the code
is the same, so the two land as one.
"""

from __future__ import annotations

import hashlib
from typing import Any

from ..errors import invalid
from ..registry import op

I64 = (-(2**63), 2**63 - 1)


def _param(value: Any) -> Any:
    if value is None or isinstance(value, (str, float)):
        return value
    if isinstance(value, int) and not isinstance(value, bool):
        if not I64[0] <= value <= I64[1]:
            raise invalid("an integer param must fit in a signed 64-bit integer")
        return value
    raise invalid("params are strings, numbers or null; write bytes with CAST(? AS BLOB)")


def _value(value: Any) -> Any:
    if isinstance(value, list):
        return {"sha256": hashlib.sha256(bytes(value)).hexdigest()}
    if value is None or isinstance(value, (int, float, str)):
        return value
    raise TypeError(f"unexpected {type(value).__name__} from the database")


@op("db_exec")
def db_exec(session, args):
    if not isinstance(args, dict) or "sql" not in args:
        raise invalid("db_exec takes {sql, params?}")
    unknown = sorted(set(args) - {"sql", "params"})
    if unknown:
        raise invalid(f"unknown db_exec keys: {', '.join(unknown)}")
    sql = args["sql"]
    if not isinstance(sql, str) or not sql:
        raise invalid("db_exec sql is a non-empty string")
    params = args.get("params", [])
    if not isinstance(params, list):
        raise invalid("db_exec params is a list")
    rows = session.col.db.all(sql, *(_param(p) for p in params))
    return {"rows": [[_value(v) for v in row] for row in rows]}
