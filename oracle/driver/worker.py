# SPDX-License-Identifier: AGPL-3.0-or-later
"""Protocol loop of one long-lived oracle worker (conformance/schema/protocol.json).

One JSON request per stdin line, one JSON response per stdout line, in order.
Requests: hello | run | begin | step | end | check, each with an optional `id`
echoed back. A failing request gets {"ok": false, "error": {...}} and the worker keeps
serving; so does a request with NaN or Infinity, or one whose response JSON
cannot carry. stdout carries protocol lines only: everything else the process
prints (Python or Anki's Rust backend) goes to stderr.

The worker serves exactly one time zone, $TZ, for its whole life (R1871); its
clock is the libfaketime file set up by entrypoint.sh (container) or
oracle/macos/run-native.sh (native lane, float-determinism.md F6). At start it
checks that the frozen clock reaches Python and Anki (timecontrol.py) and
answers time_control in hello: a container worker that fails the check refuses
to start; a native one runs on the real clock and answers "none".

A run request with fsrs_context: true also gets, per next_states card pointer,
the desired retention and decay its FSRS states used (protocol.json, F7.2).

Actor scenarios (specs/conformance/sync-ops.md S10) run whole with `run`, or
interactively: begin takes caller_actors and ids and answers with the setup's
ids and the session resources' descriptions (sync: endpoints and account);
step takes done for a step a caller-played actor ran and dumps with an op that
takes them (converge), and answers with the real ids the step created.

check (tools/mg oracle check, specs/conformance/collection-check.md) works on
a copy of a collection directory outside any scenario: collection_check.py.

At start the worker removes every proxy variable from its environment, so a
stray one cannot reroute a sync client, and in a recording run
(ORACLE_SYNC_RECORDER) routes HTTP through the recorder, loopback excepted
(sync-ops.md S8.1, S9). Children it starts get no proxy variable.
"""

from __future__ import annotations

import json
import os
import sys
import time
from typing import Any, TextIO

from . import collection_check, meta, registry, timecontrol
from .clock import Clock, zone
from .errors import DriverError, log_exception
from .session import Session


class Worker:
    def __init__(
        self, tz: str, scratch: str, clock: Clock, handoff_root: str | None = None, time_control: str = "faketime"
    ) -> None:
        self.tz = tz
        self.scratch = scratch
        self.clock = clock
        self.handoff_root = handoff_root
        self.hello = meta.hello(tz, time_control)
        self.session: Session | None = None

    def handle(self, req: Any) -> dict:
        if not isinstance(req, dict) or not isinstance(req.get("cmd"), str):
            raise DriverError("protocol_error", detail="a request is an object with a string cmd")
        cmd = req["cmd"]
        if cmd == "hello":
            return dict(self.hello)
        if cmd == "run":
            # A whole scenario, or one segment of a live chain (steps, resume,
            # handoff; dsl.md D9.2). An actor scenario runs whole only.
            self._no_open_session()
            scenario = req.get("scenario")
            if isinstance(scenario, dict) and "actors" in scenario and {"steps", "resume", "handoff"} & set(req):
                raise DriverError("invalid_scenario", detail="an actor scenario runs whole (sync-ops.md S10.1)")
            fsrs_context = req.get("fsrs_context", False)
            if not isinstance(fsrs_context, bool):
                raise DriverError("protocol_error", detail="fsrs_context is a boolean")
            started = time.perf_counter()
            session = Session(
                req.get("scenario"), self.tz, self.scratch, self.clock,
                resume=req.get("resume"), handoff_root=self.handoff_root,
            )
            if fsrs_context:
                session.fsrs_context = {}
            state = None
            try:
                first, end = _step_range(req.get("steps"), len(session.steps))
                observations = session.run_range(first, end)
                if "handoff" in req:
                    state = session.hand_off(self.handoff_root, req["handoff"])
            finally:
                session.close()
            resp = {
                "scenario_id": session.scenario["id"],
                "observations": observations,
                "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
            }
            if state is not None:
                resp["state"] = state
            if session.fsrs_context is not None:
                resp["fsrs_context"] = session.fsrs_context
            return resp
        if cmd == "begin":
            self._no_open_session()
            session = Session(
                req.get("scenario"), self.tz, self.scratch, self.clock,
                caller_actors=req.get("caller_actors"), caller_ids=req.get("ids"),
            )
            self.session = session
            resp = {"scenario_id": session.scenario["id"], "steps": len(session.steps)}
            if session.is_actor_scenario:
                resp["ids"] = session.setup_ids
                for name, resource in session.resources.items():
                    if hasattr(resource, "describe"):
                        resp[name] = resource.describe()
            return resp
        if cmd == "step":
            # index: the step's position in the session output (what `end`
            # returns). Ad-hoc steps leave the scenario cursor where it is.
            session = self._open_session()
            index = len(session.outputs)
            done, dumps = req.get("done"), req.get("dumps")
            if "step" in req:
                if done is not None or dumps is not None:
                    raise DriverError("protocol_error", detail="an ad-hoc step comes without done or dumps")
                if not session.runs_here(req["step"]):
                    raise DriverError("protocol_error", detail="the caller runs this actor's steps")
                return {"index": index, "observation": session.run_step(index, req["step"])}
            if session.next_step >= len(session.steps):
                raise DriverError("no_more_steps", steps=len(session.steps))
            step = session.steps[session.next_step]
            counts = session.id_counts()
            if not session.runs_here(step):
                if done is None or dumps is not None:
                    raise DriverError("protocol_error", detail=f"step {session.next_step} belongs to a caller-played actor: send done")
                observation = session.record_caller_step(done)
            else:
                if done is not None:
                    raise DriverError("protocol_error", detail=f"the worker runs step {session.next_step}: send no done")
                session.caller_dumps = _check_dumps(session, step, dumps)
                try:
                    observation = session.run_step(index, step, scenario_index=session.next_step)
                finally:
                    session.caller_dumps = None
            session.next_step += 1  # a failed step stays next
            resp = {"index": index, "observation": observation}
            if session.is_actor_scenario:
                resp["ids"] = session.ids_since(counts)
            return resp
        if cmd == "check":
            self._no_open_session()
            started = time.perf_counter()
            report = collection_check.run(req, self.tz, self.scratch, self.clock)
            return {"check": report, "elapsed_ms": round((time.perf_counter() - started) * 1000, 3)}
        if cmd == "end":
            session = self._open_session()
            self.session = None
            session.close()
            return {"scenario_id": session.scenario["id"], "observations": {"steps": session.outputs}}
        raise DriverError("unknown_cmd", cmd=cmd)

    def _no_open_session(self) -> None:
        if self.session is not None:
            raise DriverError("session_open", detail="end the interactive session first")

    def _open_session(self) -> Session:
        if self.session is None:
            raise DriverError("no_session", detail="begin a session first")
        return self.session

    def serve(self, inp: TextIO, out: TextIO) -> None:
        for line in inp:
            line = line.strip()
            if not line:
                continue
            req_id = None
            try:
                req = json.loads(line, parse_constant=_reject_constant)
                if isinstance(req, dict):
                    req_id = req.get("id")
                resp = {"ok": True, **self.handle(req)}
            except json.JSONDecodeError:
                resp = {"ok": False, "error": {"kind": "protocol_error", "detail": "request is not JSON"}}
            except DriverError as exc:
                resp = {"ok": False, "error": exc.to_json()}
            except Exception as exc:  # noqa: BLE001 - never let one request kill the worker
                log_exception(exc, "internal_error")
                resp = {"ok": False, "error": {"kind": "internal_error", "exception": type(exc).__name__}}
            if req_id is not None:
                resp["id"] = req_id
            out.write(_response_line(resp))
            out.flush()
        if self.session is not None:
            self.session.close()


def _check_dumps(session: Session, step: dict, dumps: Any) -> dict | None:
    """dumps names exactly the caller-played actors, and comes only with an op
    that takes them when the caller plays actors (sync-ops.md S10.4)."""
    ((name, _),) = step.items()
    wanted = name in registry.DUMP_OPS and session.caller_actors
    if not wanted:
        if dumps is not None:
            raise DriverError("protocol_error", detail=f"step {session.next_step} takes no dumps")
        return None
    if (
        not isinstance(dumps, dict)
        or set(dumps) != set(session.caller_actors)
        or not all(isinstance(v, dict) for v in dumps.values())
    ):
        raise DriverError("protocol_error", detail="dumps holds one raw dump for each caller-played actor")
    return dumps


def _step_range(steps: Any, count: int) -> tuple[int, int]:
    """A run request's steps: [first, end) of the scenario's steps."""
    if steps is None:
        return 0, count
    if (
        not isinstance(steps, list)
        or len(steps) != 2
        or not all(isinstance(x, int) and not isinstance(x, bool) for x in steps)
        or not 0 <= steps[0] <= steps[1] <= count
    ):
        raise DriverError("protocol_error", detail=f"steps is [first, end) within the scenario's {count} steps")
    return steps[0], steps[1]


def _reject_constant(name: str):
    raise DriverError("protocol_error", detail=f"{name} is not JSON")


def _dumps(value: Any) -> str:
    """JSON text that also encodes as UTF-8, or ValueError/TypeError."""
    text = json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
    text.encode("utf-8")  # a lone surrogate raises UnicodeEncodeError, a ValueError
    return text


def _response_line(resp: dict) -> str:
    """A response JSON cannot carry (a set, bytes, NaN, a lone surrogate from an
    area module or in the echoed id) becomes an internal_error, so the worker
    keeps serving."""
    try:
        return _dumps(resp) + "\n"
    except (TypeError, ValueError) as exc:
        log_exception(exc, "internal_error: the response has no JSON form")
        err: dict[str, Any] = {"ok": False, "error": {"kind": "internal_error", "exception": type(exc).__name__}}
        if "id" in resp:
            try:
                _dumps(resp["id"])
                err["id"] = resp["id"]
            except (TypeError, ValueError):
                pass
        return _dumps(err) + "\n"


PROXY_VARIABLES = ("http_proxy", "https_proxy", "all_proxy", "no_proxy")


def proxy_environment(environ) -> None:
    """Drop proxy variables in any letter case; in a recording run, route HTTP
    through the recorder except to loopback (sync-ops.md S8.1, S9)."""
    for key in list(environ):
        if key.lower() in PROXY_VARIABLES:
            del environ[key]
    recorder = environ.get("ORACLE_SYNC_RECORDER")
    if recorder:
        environ["http_proxy"] = f"http://{recorder}"
        environ["no_proxy"] = "127.0.0.1,localhost"


def main() -> int:
    proxy_environment(os.environ)
    # Protocol output gets its own descriptor; fd 1 and sys.stdout go to stderr,
    # so a stray print in Python or Rust cannot corrupt the stream.
    proto_out = os.fdopen(os.dup(1), "w", encoding="utf-8", buffering=1)
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    if "ANKI_TEST_MODE" in os.environ:
        print("oracle: ANKI_TEST_MODE is set; Anki would disable fuzz and shift its clock", file=sys.stderr)
        return 78
    tz = os.environ.get("TZ", "")
    try:
        zone(tz)
    except DriverError:
        print(f"oracle: TZ={tz!r} is not an IANA time zone", file=sys.stderr)
        return 78
    platform = meta.platform_name()
    if platform not in meta.PLATFORMS:
        print(f"oracle: {platform} is not an oracle platform ({', '.join(meta.PLATFORMS)})", file=sys.stderr)
        return 78
    native = meta.is_native()
    clock_file = os.environ.get("FAKETIME_TIMESTAMP_FILE")
    scratch = os.environ.get("ORACLE_SCRATCH")
    if not clock_file or not scratch or (not native and "libfaketime" not in os.environ.get("LD_PRELOAD", "")):
        launcher = "oracle/macos/run-native.sh" if native else "entrypoint.sh"
        print(f"oracle: start workers through {launcher} (libfaketime is not set up)", file=sys.stderr)
        return 78
    time_control = timecontrol.check(clock_file, scratch)
    if time_control != "faketime" and not native:
        print("oracle: the frozen clock does not reach Python or Anki (libfaketime)", file=sys.stderr)
        return 78
    registry.load_all()
    worker = Worker(tz, scratch, Clock(clock_file), os.environ.get("ORACLE_HANDOFF") or None, time_control)
    worker.serve(sys.stdin, proto_out)
    return 0
