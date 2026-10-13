# SPDX-License-Identifier: AGPL-3.0-or-later
"""One scenario on one fresh collection.

The session owns the collection (in a new directory under the worker's scratch
tmpfs, deleted on close), the worker clock and the logical ids: notes are #n1,
#n2... and cards #c1, #c2... in creation order (a note's cards by template
order), as both Meguri and the oracle record them (R1884).

Output: one entry per scenario step. An op step records {} or {op: result},
or {op: {"error": kind}} when Anki refused it with one of the op's rejection
kinds (errors.Rejected); an observe step records {kind: value} for each
requested observation.

A segment of a live chain (dsl.md D9.2) starts from a handoff instead of the
setup: the handed session directory is copied into scratch and opened at the
handed clock and ids. `hand_off` closes the collection and copies the session
directory into the handoff directory for the other side.
"""

from __future__ import annotations

import os
import re
import shutil
import tempfile
from typing import Any

import anki.errors
from anki.collection import Collection

from . import registry
from .clock import Clock, parse_instant, zone
from .errors import DriverError, Rejected, invalid, log_exception

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
    """Reject unknown ops and observations, and handoffs that do not
    alternate, before touching a collection."""
    side = None
    for index, step in enumerate(steps):
        if not isinstance(step, dict) or len(step) != 1:
            raise invalid("a step is a mapping with one key", step=index)
        ((name, args),) = step.items()
        if name == "handoff":
            if args == side:
                raise invalid("handoffs alternate between meguri and oracle", step=index)
            side = args
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


def check_state(state: Any) -> dict:
    """A handoff state (protocol.json handoff_state)."""
    ints = lambda v: isinstance(v, list) and all(isinstance(x, int) and not isinstance(x, bool) for x in v)  # noqa: E731
    if (
        not isinstance(state, dict)
        or set(state) != {"dir", "now", "notes", "cards"}
        or not isinstance(state["dir"], str)
        or not isinstance(state["now"], int)
        or isinstance(state["now"], bool)
        or state["now"] < 0
        or not ints(state["notes"])
        or not ints(state["cards"])
    ):
        raise DriverError("protocol_error", detail="resume is {dir, now, notes, cards}")
    return state


def handoff_path(root: str | None, rel: Any) -> str:
    """rel inside the handoff directory, or handoff_error."""
    if not root:
        raise DriverError("handoff_error", detail="start the worker with ORACLE_HANDOFF=<shared directory>")
    if not isinstance(rel, str) or not rel or rel.startswith("/") or ".." in rel.split("/"):
        raise DriverError("handoff_error", detail="a handoff directory is a relative path inside ORACLE_HANDOFF")
    path = os.path.realpath(os.path.join(root, rel))
    if not path.startswith(os.path.realpath(root) + os.sep):
        raise DriverError("handoff_error", detail="a handoff directory is a relative path inside ORACLE_HANDOFF")
    return path


class Session:
    def __init__(
        self,
        scenario: dict,
        worker_tz: str,
        scratch: str,
        clock: Clock,
        resume: dict | None = None,
        handoff_root: str | None = None,
    ) -> None:
        check_scenario(scenario)
        _check_steps(scenario.get("steps", []))
        self.scenario = scenario
        self.tz = zone(scenario["clock"]["tz"])
        if self.tz.key != worker_tz:
            raise DriverError("tz_mismatch", scenario_tz=self.tz.key, worker_tz=worker_tz)
        if resume is not None:
            resume = check_state(resume)
            source = handoff_path(handoff_root, resume["dir"])
            if not os.path.isfile(os.path.join(source, "collection.anki2")):
                raise DriverError("handoff_error", detail="the handoff directory holds no collection.anki2")
        self.clock = clock
        self.clock.set(resume["now"] if resume else parse_instant(scenario["clock"]["start"], self.tz))
        self.dir = tempfile.mkdtemp(prefix="col-", dir=scratch)
        self.col: Collection | None = None
        self.notes: list[int] = list(resume["notes"]) if resume else []
        self.cards: list[int] = list(resume["cards"]) if resume else []
        self.notetypes: dict[str, int] = {}
        self.decks: dict[str, int] = {}
        self.presets: dict[str, int] = {}
        self.steps: list = scenario.get("steps", [])
        self.outputs: list[dict] = []
        self.next_step = 0  # interactive mode: the next scenario step (ad-hoc steps do not move it)
        try:
            if resume:
                shutil.copytree(source, self.dir, dirs_exist_ok=True)
                self.col = Collection(self.path)
                self._names_from_collection(scenario.get("setup", {}))
            else:
                self.col = Collection(self.path)
                self._guard(None, "setup", lambda: self._setup(scenario.get("setup", {})))
        except BaseException:
            self.close()
            raise

    @property
    def path(self) -> str:
        return os.path.join(self.dir, "collection.anki2")

    def _names_from_collection(self, setup: dict) -> None:
        """A resumed session's note types, decks and presets by the names the
        scenario's setup gave them."""
        col = self.col
        for item in setup.get("notetypes", []) or []:
            name = item.get("name") if isinstance(item, dict) else None
            ntid = col.models.id_for_name(name) if isinstance(name, str) else None
            if ntid:
                self.notetypes[name] = ntid
        for item in setup.get("decks", []) or []:
            name = item if isinstance(item, str) else item.get("name") if isinstance(item, dict) else None
            did = col.decks.id_for_name(name) if isinstance(name, str) else None
            if did:
                self.decks[name] = did
        for conf in col.decks.all_config():
            self.presets[conf["name"]] = conf["id"]

    def reopen(self) -> None:
        """Close the collection and open it again from its file (a handoff
        within one run)."""
        self.col.close(downgrade=False)
        self.col = None
        self.col = Collection(self.path)

    def hand_off(self, root: str | None, rel: Any) -> dict:
        """Close the collection and copy the session directory to rel under
        the handoff directory; the state the other side resumes from."""
        dest = handoff_path(root, rel)
        if os.path.exists(dest):
            raise DriverError("handoff_error", detail="the handoff directory already exists")
        try:
            self.col.close(downgrade=False)
        except Exception as exc:
            raise self._failed("anki_error", None, "handoff", exc) from exc
        finally:
            self.col = None
        shutil.copytree(self.dir, dest)
        return {"dir": rel, "now": self.clock.now, "notes": list(self.notes), "cards": list(self.cards)}

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
            try:
                result = self._guard(index, name, lambda: handler(self, args))
            except Rejected as exc:
                if exc.kind not in registry.REJECTIONS.get(name, frozenset()):
                    raise DriverError(
                        "internal_error", op=name, step=index, detail=f"rejection {exc.kind} is not in the op's list"
                    ) from exc
                out = {name: {"error": exc.kind}}
            else:
                out = {} if result is None else {name: result}
        self.outputs.append(out)
        return out

    def run_all(self) -> dict:
        return self.run_range(0, len(self.steps))

    def run_range(self, first: int, end: int) -> dict:
        """Steps [first, end) with their scenario indices; their outputs."""
        for i in range(first, end):
            self.run_step(i, self.steps[i])
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
        except Rejected:
            raise
        except anki.errors.AnkiException as exc:
            raise self._failed("anki_error", index, op, exc) from exc
        except Exception as exc:  # a driver bug: keep the worker alive, say so
            raise self._failed("internal_error", index, op, exc) from exc

    @staticmethod
    def _failed(kind: str, index: int | None, op: str, exc: Exception) -> DriverError:
        fields = {"op": op, "exception": type(exc).__name__}
        if index is not None:
            fields["step"] = index
        log_exception(exc, f"{kind} in {op}" + (f" (step {index})" if index is not None else ""))
        return DriverError(kind, **fields)

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
        except Exception as exc:  # noqa: BLE001 - the directory goes away anyway
            log_exception(exc, "closing the collection")
        finally:
            self.col = None
            shutil.rmtree(self.dir, ignore_errors=True)
