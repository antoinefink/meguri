# SPDX-License-Identifier: AGPL-3.0-or-later
"""One scenario on one fresh collection.

The session owns the collection (in a new directory under the worker's scratch
tmpfs, deleted on close), the worker clock and the logical ids: notes are #n1,
#n2... and cards #c1, #c2... in creation order (a note's cards by template
order), as both Meguri and the oracle record them (R1884).

Output: one entry per scenario step. An op step records {} or {op: result};
an observe step records {kind: value} for each requested observation.
"""

from __future__ import annotations

import os
import re
import shutil
import sys
import tempfile
import traceback
from typing import Any

import anki.errors
from anki.collection import Collection

from . import registry
from .clock import Clock, parse_instant, zone
from .errors import DriverError, invalid

SCENARIO_KEYS = {"id", "description", "tags", "clock", "setup", "steps", "compare"}
CLOCK_KEYS = {"tz", "start", "mode"}
CLOCK_MODES = ("frozen", "step")
_SPEC = re.compile(r"^([a-z][a-z0-9_]*)(?:\((.*)\))?$")
_REF = re.compile(r"^#([nc])([1-9][0-9]*)$")


def check_scenario(scenario: Any) -> None:
    if not isinstance(scenario, dict):
        raise invalid("a scenario is a mapping")
    unknown = sorted(set(scenario) - SCENARIO_KEYS)
    if unknown:
        raise invalid(f"unknown scenario keys: {', '.join(unknown)}")
    if not isinstance(scenario.get("id"), str) or not scenario["id"]:
        raise invalid("id must be a non-empty string")
    clock = scenario.get("clock")
    if not isinstance(clock, dict) or "tz" not in clock or "start" not in clock:
        raise invalid("clock must have tz and start")
    unknown = sorted(set(clock) - CLOCK_KEYS)
    if unknown:
        raise invalid(f"unknown clock keys: {', '.join(unknown)}")
    if clock.get("mode", "frozen") not in CLOCK_MODES:
        raise invalid("clock.mode must be frozen or step")
    if not isinstance(scenario.get("setup", {}), dict):
        raise invalid("setup must be a mapping")
    if not isinstance(scenario.get("steps", []), list):
        raise invalid("steps must be a list")


def _check_steps(steps: list) -> None:
    """Reject unknown ops and observations before touching a collection."""
    for index, step in enumerate(steps):
        if not isinstance(step, dict) or len(step) != 1:
            raise invalid("a step is a mapping with one key", step=index)
        ((name, args),) = step.items()
        if name == "observe":
            if not isinstance(args, list):
                raise invalid("observe takes a list", step=index)
            for spec in args:
                try:
                    obs = parse_observation(spec)[0]
                except DriverError as exc:
                    exc.fields.setdefault("step", index)
                    raise
                if obs not in registry.OBSERVATIONS:
                    raise DriverError("unsupported_observation", observation=obs, step=index)
        elif name not in registry.OPS:
            raise DriverError("unsupported_op", op=name, step=index)


def parse_observation(spec: Any) -> tuple[str, Any]:
    """'timing', 'card(#c1, #c2)' or {fuzz_table: {...}} -> (name, args)."""
    if isinstance(spec, str):
        m = _SPEC.match(spec.strip())
        if not m:
            raise invalid(f"observation {spec!r} must look like name or name(arg, ...)")
        if m.group(2) is None:
            return m.group(1), None
        return m.group(1), [a.strip() for a in m.group(2).split(",") if a.strip()]
    if isinstance(spec, dict) and len(spec) == 1:
        ((name, args),) = spec.items()
        return name, args
    raise invalid("an observation is a string or a mapping with one key")


class Session:
    def __init__(self, scenario: dict, worker_tz: str, scratch: str, clock: Clock) -> None:
        check_scenario(scenario)
        _check_steps(scenario.get("steps", []))
        self.scenario = scenario
        self.tz = zone(scenario["clock"]["tz"])
        if self.tz.key != worker_tz:
            raise DriverError("tz_mismatch", scenario_tz=self.tz.key, worker_tz=worker_tz)
        self.clock = clock
        self.clock.set(parse_instant(scenario["clock"]["start"], self.tz))
        self.dir = tempfile.mkdtemp(prefix="col-", dir=scratch)
        self.col: Collection | None = None
        self.notes: list[int] = []
        self.cards: list[int] = []
        self.notetypes: dict[str, int] = {}
        self.decks: dict[str, int] = {}
        self.presets: dict[str, int] = {}
        self.steps: list = scenario.get("steps", [])
        self.outputs: list[dict] = []
        self.next_step = 0  # interactive mode: the next scenario step (ad-hoc steps do not move it)
        try:
            self.col = Collection(os.path.join(self.dir, "collection.anki2"))
            self._guard(None, "setup", lambda: self._setup(scenario.get("setup", {})))
        except BaseException:
            self.close()
            raise

    # --- setup and steps --------------------------------------------------------

    def _setup(self, setup: dict) -> None:
        unknown = sorted(set(setup) - set(registry.SETUP_SECTIONS))
        if unknown:
            raise DriverError("unsupported_setup", sections=unknown)
        for name, (_, fn) in sorted(registry.SETUP_SECTIONS.items(), key=lambda kv: kv[1][0]):
            if name in setup:
                fn(self, setup[name])

    def run_step(self, index: int, step: Any) -> dict:
        if not isinstance(step, dict) or len(step) != 1:
            raise invalid("a step is a mapping with one key", step=index)
        ((name, args),) = step.items()
        if name == "observe":
            out = self._guard(index, "observe", lambda: self._observe(index, args))
        else:
            handler = registry.OPS.get(name)
            if handler is None:
                raise DriverError("unsupported_op", op=name, step=index)
            result = self._guard(index, name, lambda: handler(self, args))
            out = {} if result is None else {name: result}
        self.outputs.append(out)
        return out

    def run_all(self) -> dict:
        for i, step in enumerate(self.steps):
            self.run_step(i, step)
        return {"steps": self.outputs}

    def _observe(self, index: int, specs: Any) -> dict:
        if not isinstance(specs, list):
            raise invalid("observe takes a list")
        out: dict[str, Any] = {}
        for spec in specs:
            name, args = parse_observation(spec)
            handler = registry.OBSERVATIONS.get(name)
            if handler is None:
                raise DriverError("unsupported_observation", observation=name, step=index)
            value = handler(self, args)
            if name in out:
                if not (isinstance(out[name], dict) and isinstance(value, dict)) or set(out[name]) & set(value):
                    raise invalid(f"observation {name} is requested twice for the same target")
                out[name].update(value)
            else:
                out[name] = value
        return out

    def _guard(self, index: int | None, op: str, fn):
        """Attach the step to driver errors; turn Anki and driver failures into
        structured errors without their (localized) message text."""
        try:
            return fn()
        except DriverError as exc:
            if index is not None:
                exc.fields.setdefault("step", index)
            raise
        except anki.errors.AnkiException as exc:
            traceback.print_exc(file=sys.stderr)
            fields = {"op": op, "exception": type(exc).__name__}
            if index is not None:
                fields["step"] = index
            raise DriverError("anki_error", **fields) from exc
        except Exception as exc:  # a driver bug: keep the worker alive, say so
            traceback.print_exc(file=sys.stderr)
            fields = {"op": op, "exception": type(exc).__name__}
            if index is not None:
                fields["step"] = index
            raise DriverError("internal_error", **fields) from exc

    # --- logical ids ------------------------------------------------------------

    def resolve(self, ref: Any, kind: str) -> int:
        """'#c2' -> the real id of the second card created (kind 'c' or 'n')."""
        m = _REF.match(ref) if isinstance(ref, str) else None
        if not m or m.group(1) != kind:
            want = "#c<N>" if kind == "c" else "#n<N>"
            raise invalid(f"{ref!r} is not a {want} reference")
        table = self.cards if kind == "c" else self.notes
        n = int(m.group(2))
        if n > len(table):
            raise DriverError("unknown_ref", ref=ref)
        return table[n - 1]

    def card_targets(self, args: Any) -> list[tuple[str, int]]:
        """Observation args -> [(ref, card id)]: every card when args are empty."""
        if not args:
            return [(f"#c{i + 1}", cid) for i, cid in enumerate(self.cards)]
        if isinstance(args, str):
            args = [args]
        if not isinstance(args, list):
            raise invalid("card observations take card references")
        return [(ref, self.resolve(ref, "c")) for ref in args]

    def card_ref(self, card_id: int) -> str:
        return f"#c{self.cards.index(card_id) + 1}" if card_id in self.cards else str(card_id)

    def note_ref(self, note_id: int) -> str:
        return f"#n{self.notes.index(note_id) + 1}" if note_id in self.notes else str(note_id)

    def deck_name(self, deck_id: int) -> str | None:
        if not deck_id:
            return None
        deck = self.col.decks.get(deck_id, default=False)
        return deck["name"] if deck else str(deck_id)

    # --- teardown ---------------------------------------------------------------

    def close(self) -> None:
        try:
            if self.col is not None:
                self.col.close(downgrade=False)
        except Exception:  # noqa: BLE001 - the directory goes away anyway
            traceback.print_exc(file=sys.stderr)
        finally:
            self.col = None
            shutil.rmtree(self.dir, ignore_errors=True)
