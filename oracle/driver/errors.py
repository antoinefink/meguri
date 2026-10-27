# SPDX-License-Identifier: AGPL-3.0-or-later
"""Structured errors and exception logging. A worker reports errors and stays alive.

Error objects carry a `kind` plus driver-authored fields (op name, step index,
exception class). They never carry Anki's message text, which is localized
(R1887).

Worker and harness stderr ends up in tools/mg logs that implementers read
(R1955, R1956), so exceptions are logged with log_exception: the class and the
driver frames (file:line) only, never a source line, a frame outside the driver
(Anki's) or the message. ORACLE_FULL_TRACEBACKS=1 prints whole tracebacks
instead; it is for an oracle maintainer running a worker by hand, and no
tools/mg job sets it.
"""

from __future__ import annotations

import os
import sys
import traceback
from typing import Any

_DRIVER = os.path.dirname(os.path.abspath(__file__))
_ORACLE_HOME = os.path.dirname(_DRIVER)


class DriverError(Exception):
    def __init__(self, kind: str, **fields: Any) -> None:
        super().__init__(kind)
        self.kind = kind
        self.fields = fields

    def to_json(self) -> dict[str, Any]:
        return {"kind": self.kind, **self.fields}


class Rejected(Exception):
    """Anki refused an op as part of its behavior (dsl.md D7.3): the step
    records {op: {"error": kind}} and the run goes on. Raise it only when the
    op changed nothing (Anki rolls a refused backend op back; D7.3's one
    exception is the undo history a refused deck-package import empties,
    package-ops.md E6.4), and only with a kind the op declared
    (`registry.op(name, rejects=...)`)."""

    def __init__(self, kind: str) -> None:
        super().__init__(kind)
        self.kind = kind


def invalid(detail: str, **fields: Any) -> DriverError:
    """The scenario does not follow the format (a driver-authored detail)."""
    return DriverError("invalid_scenario", detail=detail, **fields)


def log_exception(exc: BaseException, context: str) -> None:
    """One stderr line: `oracle: <context>: <Class> at driver/x.py:12, ...`."""
    if os.environ.get("ORACLE_FULL_TRACEBACKS") == "1":
        traceback.print_exception(type(exc), exc, exc.__traceback__, file=sys.stderr)
        return
    frames, outside = [], 0
    for frame, lineno in traceback.walk_tb(exc.__traceback__):
        path = os.path.abspath(frame.f_code.co_filename)
        if path.startswith(_DRIVER + os.sep):
            frames.append(f"{os.path.relpath(path, _ORACLE_HOME)}:{lineno}")
        else:
            outside += 1
    where = ", ".join(frames) or "no driver frame"
    more = f" (+{outside} frames outside the driver)" if outside else ""
    print(f"oracle: {context}: {type(exc).__name__} at {where}{more}", file=sys.stderr, flush=True)


def install_excepthook() -> None:
    """Uncaught exceptions of a worker or harness process log as log_exception."""
    sys.excepthook = lambda _type, exc, _tb: log_exception(exc, "uncaught exception")
