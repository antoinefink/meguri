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

An actor scenario (specs/conformance/sync-ops.md) has one collection per actor
the worker plays, each in its own subdirectory, all created at clock.start; the
setup runs on the first actor only. `col`, `notetypes`, `decks` and `presets`
are the current actor's, so ops written for one collection work unchanged; the
name maps are looked up again in the current collection before each of its
steps, since a sync can bring note types and decks. The logical ids are one
table for the whole scenario. Actors the caller plays (sync-ops.md S10.4) have
no collection here: the caller runs their steps and reports the outputs
(`record_caller_step`). Session resources (registry.session_resource, the sync
server) start after the setup and stop at close.

A scenario whose first step is `open` (storage-ops.md P1.3) has no setup and
starts with no collection: the open brings it. After an open the op refused,
no collection is open and only another open may run.
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

SCENARIO_KEYS = {"id", "description", "tags", "clock", "setup", "steps", "compare", "actors", "server"}
CLOCK_KEYS = {"tz", "start", "mode"}
CLOCK_MODES = ("frozen", "step")
SERVER_KEYS = {"clock_offset_secs"}
MAX_CLOCK_OFFSET_SECS = 86400
_SPEC = re.compile(r"^([a-z][a-z0-9_]*)(?:\((.*)\))?$")
_REF = re.compile(r"^#([nc])([1-9][0-9]*)$")
ACTOR_NAME = re.compile(r"^(meguri(_([2-9]|[1-9][0-9]+))?|client_[a-z])$")
MEGURI_ACTOR = re.compile(r"^meguri(_([2-9]|[1-9][0-9]+))?$")


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


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
    if "actors" in scenario:
        actors = scenario["actors"]
        if (
            not isinstance(actors, list)
            or not actors
            or not all(isinstance(a, str) and ACTOR_NAME.match(a) for a in actors)
            or len(set(actors)) != len(actors)
        ):
            raise invalid("actors is a non-empty list of distinct names: meguri, meguri_<n>, client_a ... client_z")
    if "server" in scenario:
        if "actors" not in scenario:
            raise invalid("server needs actors")
        server = scenario["server"]
        if not isinstance(server, dict) or set(server) - SERVER_KEYS:
            raise invalid("server is {clock_offset_secs?}")
        offset = server.get("clock_offset_secs", 0)
        if not _is_int(offset) or abs(offset) > MAX_CLOCK_OFFSET_SECS:
            raise invalid(f"server.clock_offset_secs is an integer from -{MAX_CLOCK_OFFSET_SECS} to {MAX_CLOCK_OFFSET_SECS}")


def _check_steps(steps: list, actors: bool) -> None:
    """Reject unknown ops and observations, handoffs that do not alternate or
    mix with actors, and actor ops without actors, before touching a
    collection."""
    side = None
    for index, step in enumerate(steps):
        if not isinstance(step, dict) or len(step) != 1:
            raise invalid("a step is a mapping with one key", step=index)
        ((name, args),) = step.items()
        if name == "handoff":
            if actors:
                raise invalid("an actor scenario has no handoff (sync-ops.md S1)", step=index)
            if args == side:
                raise invalid("handoffs alternate between meguri and oracle", step=index)
            side = args
        if name in registry.ACTOR_OPS and not actors:
            raise invalid(f"{name} needs actors", step=index)
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


def _opens_first(scenario: dict) -> bool:
    """True for a scenario that opens a fixture: it has no setup and its first
    step is open (storage-ops.md P1.3); invalid_scenario for an open anywhere
    else, and in an actor scenario, whose actors each start with a new
    collection (sync-ops.md S2.2)."""
    steps = scenario.get("steps", [])
    opens = [i for i, step in enumerate(steps) if isinstance(step, dict) and "open" in step]
    if not opens:
        return False
    if scenario.get("actors"):
        raise invalid("an actor scenario has no open step", step=opens[0])
    if opens[0] != 0 or scenario.get("setup"):
        raise invalid("a scenario with an open step has no setup and opens first", step=opens[0])
    return True


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


class Actor:
    """One collection of the session: the only one of a scenario without
    actors (name None), or one actor's. `played` is False for an actor the
    caller plays, which has no collection here. `data` holds area state (the
    sync host key)."""

    def __init__(self, name: str | None, directory: str, played: bool = True) -> None:
        self.name = name
        self.dir = directory
        self.played = played
        self.col: Collection | None = None
        self.notetypes: dict[str, int] = {}
        self.decks: dict[str, int] = {}
        self.presets: dict[str, int] = {}
        self.data: dict[str, Any] = {}

    @property
    def path(self) -> str:
        return os.path.join(self.dir, "collection.anki2")


def _check_new_ids(ids: Any, what: str) -> dict:
    if (
        not isinstance(ids, dict)
        or set(ids) != {"notes", "cards"}
        or not all(isinstance(ids[k], list) and all(_is_int(x) for x in ids[k]) for k in ids)
    ):
        raise DriverError("protocol_error", detail=f"{what} is {{notes: [ids], cards: [ids]}}")
    return ids


class Session:
    def __init__(
        self,
        scenario: dict,
        worker_tz: str,
        scratch: str,
        clock: Clock,
        resume: dict | None = None,
        handoff_root: str | None = None,
        caller_actors: Any = None,
        caller_ids: Any = None,
    ) -> None:
        check_scenario(scenario)
        self.actor_names: list[str] = list(scenario.get("actors", []))
        _check_steps(scenario.get("steps", []), bool(self.actor_names))
        opens_first = _opens_first(scenario)
        for check in registry.SCENARIO_CHECKS:
            check(scenario)
        caller = self._caller_actors(caller_actors, caller_ids)
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
        self.scratch = scratch
        self.dir = tempfile.mkdtemp(prefix="col-", dir=scratch)
        self.caller_actors = frozenset(caller)
        if self.actor_names:
            self.actors = {
                name: Actor(name, os.path.join(self.dir, name), played=name not in self.caller_actors)
                for name in self.actor_names
            }
        else:
            self.actors = {None: Actor(None, self.dir)}
        self.current: Actor = next(iter(self.actors.values()))
        self.notes: list[int] = list(resume["notes"]) if resume else []
        self.cards: list[int] = list(resume["cards"]) if resume else []
        # The real ids the worker's own setup created (an actor scenario's begin).
        self.setup_ids: dict[str, list[int]] = {"notes": [], "cards": []}
        self.steps: list = scenario.get("steps", [])
        self.outputs: list[dict] = []
        self.next_step = 0  # interactive mode: the next scenario step (ad-hoc steps do not move it)
        self.step_index: int | None = None  # the scenario index of the step running now
        self.caller_dumps: dict | None = None  # raw dumps sent with the step running now
        # A run request's fsrs_context (protocol.json): next_states fills it,
        # keyed by card pointer, when the request asked for it.
        self.fsrs_context: dict[str, dict] | None = None
        self.resources: dict[str, Any] = {}
        try:
            if resume:
                shutil.copytree(source, self.dir, dirs_exist_ok=True)
                self.col = Collection(self.path)
                self._names_from_collection(scenario.get("setup", {}))
            elif not opens_first:
                for actor in self.actors.values():
                    if actor.played:
                        os.makedirs(actor.dir, exist_ok=True)
                        actor.col = Collection(actor.path)
                if self.current.played:
                    self._guard(None, "setup", lambda: self._setup(scenario.get("setup", {})))
                    self.setup_ids = {"notes": list(self.notes), "cards": list(self.cards)}
                elif caller_ids is not None:
                    self.notes.extend(caller_ids["notes"])
                    self.cards.extend(caller_ids["cards"])
            for name, factory in registry.SESSION_RESOURCES.items():
                resource = factory(self)
                if resource is not None:
                    self.resources[name] = resource
        except BaseException:
            self.close()
            raise

    def _caller_actors(self, caller_actors: Any, caller_ids: Any) -> list[str]:
        """begin's caller_actors and ids (sync-ops.md S10.4), checked against
        the scenario's actors."""
        caller = [] if caller_actors is None else caller_actors
        if not isinstance(caller, list) or not all(isinstance(a, str) for a in caller) or len(set(caller)) != len(caller):
            raise DriverError("protocol_error", detail="caller_actors is a list of distinct actor names")
        if caller and not self.actor_names:
            raise DriverError("protocol_error", detail="caller_actors needs an actor scenario")
        for name in caller:
            if name not in self.actor_names or not MEGURI_ACTOR.match(name):
                raise DriverError("protocol_error", detail=f"{name!r} is not one of the scenario's Meguri actors")
        plays_first = bool(caller) and self.actor_names[0] in caller
        if plays_first and caller_ids is None:
            raise DriverError("protocol_error", detail="the caller plays the first actor: send the setup's ids")
        if not plays_first and caller_ids is not None:
            raise DriverError("protocol_error", detail="ids come only from a caller that plays the first actor")
        if caller_ids is not None:
            _check_new_ids(caller_ids, "ids")
        return caller

    # --- the current collection (the current actor's) ----------------------------

    @property
    def is_actor_scenario(self) -> bool:
        return bool(self.actor_names)

    @property
    def col(self) -> Collection | None:
        return self.current.col

    @col.setter
    def col(self, value: Collection | None) -> None:
        self.current.col = value

    @property
    def notetypes(self) -> dict[str, int]:
        return self.current.notetypes

    @property
    def decks(self) -> dict[str, int]:
        return self.current.decks

    @property
    def presets(self) -> dict[str, int]:
        return self.current.presets

    @property
    def path(self) -> str:
        return self.current.path

    def switch_actor(self, name: Any) -> None:
        if name not in self.actors or name is None:
            raise invalid(f"actor {name!r} is not in actors")
        self.current = self.actors[name]

    def refresh_names(self) -> None:
        """Look the scenario's note type, deck and preset names up again in
        the current collection (sync-ops.md S2.2)."""
        self.notetypes.clear()
        self.decks.clear()
        self.presets.clear()
        self._names_from_collection(self.scenario.get("setup", {}))

    def _names_from_collection(self, setup: dict) -> None:
        """The note types, decks and presets of the current collection by the
        names the scenario's setup gave them (a resumed session, an actor)."""
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

    def close_collection(self) -> None:
        """Close the open collection, if any, without the downgrade."""
        col, self.col = self.col, None
        if col is not None:
            col.close(downgrade=False)

    def reopen(self) -> None:
        """Close the current collection and open it again from its file (a
        handoff within one run, the reopen op)."""
        self.col.close(downgrade=False)
        self.col = None
        self.col = Collection(self.path)

    def hand_off(self, root: str | None, rel: Any) -> dict:
        """Close the collection and copy the session directory to rel under
        the handoff directory; the state the other side resumes from."""
        dest = handoff_path(root, rel)
        if self.col is None:
            raise invalid("no collection is open to hand over; after a refused open only another open may run")
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

    def runs_here(self, step: Any) -> bool:
        """False for a step the caller runs: a collection step while an actor
        the caller plays is current."""
        if not self.is_actor_scenario or not isinstance(step, dict) or len(step) != 1:
            return True
        ((name, _),) = step.items()
        return name in registry.GLOBAL_OPS or self.current.played

    def id_counts(self) -> tuple[int, int]:
        return len(self.notes), len(self.cards)

    def ids_since(self, counts: tuple[int, int]) -> dict[str, list[int]]:
        return {"notes": self.notes[counts[0]:], "cards": self.cards[counts[1]:]}

    def record_caller_step(self, done: Any) -> dict:
        """A step a caller-played actor ran: its output and the real ids it
        created, in creation order (sync-ops.md S10.4)."""
        if not isinstance(done, dict) or set(done) != {"output", "notes", "cards"} or not isinstance(done["output"], dict):
            raise DriverError("protocol_error", detail="done is {output: {...}, notes: [ids], cards: [ids]}")
        _check_new_ids({"notes": done["notes"], "cards": done["cards"]}, "done")
        self.notes.extend(done["notes"])
        self.cards.extend(done["cards"])
        self.outputs.append(done["output"])
        return done["output"]

    def run_step(self, index: int, step: Any, scenario_index: int | None = None) -> dict:
        """Run one step; index is its position in the outputs, scenario_index
        its index among the scenario's steps (index when missing)."""
        if not isinstance(step, dict) or len(step) != 1:
            raise invalid("a step is a mapping with one key", step=index)
        ((name, args),) = step.items()
        if not self.is_actor_scenario and self.col is None and name != "open":
            raise invalid("no collection is open; after a refused open only another open may run", step=index)
        self.step_index = index if scenario_index is None else scenario_index
        if self.is_actor_scenario and name not in registry.GLOBAL_OPS:
            if not self.current.played:
                raise DriverError("internal_error", step=index, detail="the caller runs this actor's steps")
            self._guard(index, name, self.refresh_names)
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
        for name, resource in reversed(list(self.resources.items())):
            try:
                resource.close()
            except Exception as exc:  # noqa: BLE001 - close the rest anyway
                log_exception(exc, f"closing the session's {name}")
        self.resources = {}
        for actor in self.actors.values():
            try:
                if actor.col is not None:
                    actor.col.close(downgrade=False)
            except Exception as exc:  # noqa: BLE001 - the directory goes away anyway
                log_exception(exc, "closing the collection")
            finally:
                actor.col = None
        shutil.rmtree(self.dir, ignore_errors=True)
