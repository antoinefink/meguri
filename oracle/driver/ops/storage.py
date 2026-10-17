# SPDX-License-Identifier: AGPL-3.0-or-later
"""Stored rows and config keys (record-ops.md W1, W4).

db_exec: {sql, params?}   one statement through the collection's raw query API,
    with no begin or commit request: a commit request would set col.mod after
    a non-select statement. Anki itself clears the undo history and the study
    queues after a statement that does not start with `select`.
    -> {rows: [[value, ...], ...]}, by the storage class of each value: INTEGER
    int, REAL float, TEXT str (Anki's bridge already replaced invalid UTF-8
    with U+FFFD), BLOB {sha256} (it arrives as a list of byte values), NULL null.
set_config: {key, value, undoable?}   the collection's config setter. Without
    undoable (or false) with its default: no undo entry, undo steps kept
    (W4). undoable: true passes undoable=True: one "Update Config" step
    (undo-ops.md V3.1). The value reaches Anki's JSON encoder as the scenario
    gave it, so 1.0 stays a float.

Params bind as given: str TEXT, int INTEGER, float REAL, None NULL. A JSON
integer outside 64 bits cannot bind as INTEGER (Anki's bridge would bind a
large one as REAL), so it is invalid_scenario, as booleans, lists and mappings.
"""

from __future__ import annotations

import hashlib
from typing import Any

from ..errors import invalid
from ..registry import op

I64 = (-(2**63), 2**63 - 1)
CONFIG_INTEGERS = (-(2**63), 2**64 - 1)  # signed or unsigned 64-bit (W4)


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


def _check_json(value: Any) -> None:
    if isinstance(value, dict):
        for v in value.values():
            _check_json(v)
    elif isinstance(value, list):
        for v in value:
            _check_json(v)
    elif isinstance(value, int) and not isinstance(value, bool):
        if not CONFIG_INTEGERS[0] <= value <= CONFIG_INTEGERS[1]:
            raise invalid("a config integer must fit in a signed or unsigned 64-bit integer")


@op("set_config")
def set_config(session, args):
    if not isinstance(args, dict) or not {"key", "value"} <= set(args) <= {"key", "value", "undoable"}:
        raise invalid("set_config takes {key, value, undoable?}")
    key = args["key"]
    if not isinstance(key, str) or not key:
        raise invalid("set_config key is a non-empty string")
    undoable = args.get("undoable", False)
    if not isinstance(undoable, bool):
        raise invalid("set_config undoable is a boolean")
    _check_json(args["value"])
    session.col.set_config(key, args["value"], undoable=undoable)
