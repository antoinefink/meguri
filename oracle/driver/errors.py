# SPDX-License-Identifier: AGPL-3.0-or-later
"""Structured errors. A worker reports them and stays alive.

Error objects carry a `kind` plus driver-authored fields (op name, step index,
exception class). They never carry Anki's message text, which is localized
(R1887); tracebacks go to the worker's stderr only.
"""

from __future__ import annotations

from typing import Any


class DriverError(Exception):
    def __init__(self, kind: str, **fields: Any) -> None:
        super().__init__(kind)
        self.kind = kind
        self.fields = fields

    def to_json(self) -> dict[str, Any]:
        return {"kind": self.kind, **self.fields}


def invalid(detail: str, **fields: Any) -> DriverError:
    """The scenario does not follow the format (a driver-authored detail)."""
    return DriverError("invalid_scenario", detail=detail, **fields)
