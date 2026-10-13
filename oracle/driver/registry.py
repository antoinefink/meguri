# SPDX-License-Identifier: AGPL-3.0-or-later
"""Op, observation and setup-section registries.

Area modules under ops/ and observe/ register their handlers with these
decorators, so adding an area never touches the protocol loop or the session.
A derived image (an add-on oracle in its own quarantined image) adds modules by
listing them in ORACLE_EXTRA_MODULES (comma-separated import paths); they are
imported after the built-in areas.

Handlers take (session, args) where args is the step's value as written in the
scenario. An op returns None or a JSON value recorded as the step's output under
the op's name; an observation returns the JSON value recorded under its name.
"""

from __future__ import annotations

import importlib
import os
from typing import Any, Callable

Handler = Callable[[Any, Any], Any]

OPS: dict[str, Handler] = {}
OBSERVATIONS: dict[str, Handler] = {}
SETUP_SECTIONS: dict[str, tuple[int, Handler]] = {}


def _register(table: dict, name: str, value: Any) -> None:
    if name in table:
        raise RuntimeError(f"{name!r} is registered twice")
    table[name] = value


def op(name: str) -> Callable[[Handler], Handler]:
    def deco(fn: Handler) -> Handler:
        _register(OPS, name, fn)
        return fn

    return deco


def observation(name: str) -> Callable[[Handler], Handler]:
    def deco(fn: Handler) -> Handler:
        _register(OBSERVATIONS, name, fn)
        return fn

    return deco


def setup_section(name: str, order: int) -> Callable[[Handler], Handler]:
    """`setup.<name>` handler; sections run in ascending order."""

    def deco(fn: Handler) -> Handler:
        _register(SETUP_SECTIONS, name, (order, fn))
        return fn

    return deco


_loaded = False


def load_all() -> None:
    global _loaded
    if _loaded:
        return
    from . import observe, ops  # noqa: F401 - registration side effects

    for mod in filter(None, (m.strip() for m in os.environ.get("ORACLE_EXTRA_MODULES", "").split(","))):
        importlib.import_module(mod)
    _loaded = True
