# SPDX-License-Identifier: AGPL-3.0-or-later
"""Op, observation and setup-section registries.

Area modules under ops/ and observe/ register their handlers with these
decorators, so adding an area never touches the protocol loop or the session.
A derived image (an add-on oracle in its own quarantined image) adds modules by
listing them in ORACLE_EXTRA_MODULES (comma-separated import paths); they are
imported after the built-in areas.

Handlers take (session, args) where args is the step's value as written in the
scenario. An op returns None or a JSON value recorded as the step's output under
the op's name, or raises errors.Rejected(kind) with one of the rejection kinds
it declared (dsl.md D7.3); an observation returns the JSON value recorded under
its name.

Actor scenarios (specs/conformance/sync-ops.md): an op declares
`scope="global"` when it runs whatever the current actor (it never works on a
collection), `needs_actors=True` when only actor scenarios may use it, and
`takes_dumps=True` when a caller that plays actors sends their raw dumps with
it. `scenario_check` functions validate a scenario before any collection
exists; `session_resource` factories give a session what it holds besides its
collections (a sync server), started after the setup and closed with it.
"""

from __future__ import annotations

import importlib
import os
import re
from typing import Any, Callable

Handler = Callable[[Any, Any], Any]

OPS: dict[str, Handler] = {}
# op -> the rejection kinds it may record: its closed list in dsl.md D7.2.
REJECTIONS: dict[str, frozenset[str]] = {}
# Ops that run whatever the current actor, ops only actor scenarios may use, and
# ops a caller that plays actors sends raw dumps with (sync-ops.md S2.3, S10.4).
GLOBAL_OPS: set[str] = set()
ACTOR_OPS: set[str] = set()
DUMP_OPS: set[str] = set()
OBSERVATIONS: dict[str, Handler] = {}
SETUP_SECTIONS: dict[str, tuple[int, Handler]] = {}
SCENARIO_CHECKS: list[Callable[[dict], None]] = []
# name -> factory(session) returning a resource with close() (and optionally
# describe(), a JSON value for the begin response under name), or None.
SESSION_RESOURCES: dict[str, Callable[[Any], Any]] = {}
_KIND = re.compile(r"^[a-z][a-z0-9_]*$")


def _register(table: dict, name: str, value: Any) -> None:
    if name in table:
        raise RuntimeError(f"{name!r} is registered twice")
    table[name] = value


def op(
    name: str,
    rejects: tuple[str, ...] = (),
    scope: str = "collection",
    needs_actors: bool = False,
    takes_dumps: bool = False,
) -> Callable[[Handler], Handler]:
    bad = [k for k in rejects if not _KIND.match(k)]
    if bad:
        raise RuntimeError(f"{name!r}: rejection kinds are lower-case identifiers, not {bad}")
    if scope not in ("collection", "global"):
        raise RuntimeError(f"{name!r}: scope is collection or global")

    def deco(fn: Handler) -> Handler:
        _register(OPS, name, fn)
        REJECTIONS[name] = frozenset(rejects)
        if scope == "global":
            GLOBAL_OPS.add(name)
        if needs_actors:
            ACTOR_OPS.add(name)
        if takes_dumps:
            DUMP_OPS.add(name)
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


def scenario_check(fn: Callable[[dict], None]) -> Callable[[dict], None]:
    """fn(scenario) raises errors.invalid(...) for a scenario it rejects."""
    SCENARIO_CHECKS.append(fn)
    return fn


def session_resource(name: str) -> Callable[[Callable[[Any], Any]], Callable[[Any], Any]]:
    def deco(factory: Callable[[Any], Any]) -> Callable[[Any], Any]:
        _register(SESSION_RESOURCES, name, factory)
        return factory

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
