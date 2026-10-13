# SPDX-License-Identifier: AGPL-3.0-or-later
"""Protocol loop of one long-lived oracle worker (conformance/schema/protocol.json).

One JSON request per stdin line, one JSON response per stdout line, in order.
Requests: hello | run | begin | step | end, each with an optional `id` echoed
back. A failing request gets {"ok": false, "error": {...}} and the worker keeps
serving. stdout carries protocol lines only: everything else the process prints
(Python or Anki's Rust backend) goes to stderr.

The worker serves exactly one time zone, $TZ, for its whole life (R1871); its
clock is the libfaketime file set up by entrypoint.sh.
"""

from __future__ import annotations

import json
import os
import sys
import time
import traceback
from typing import Any, TextIO

from . import meta, registry
from .clock import Clock, zone
from .errors import DriverError
from .session import Session


class Worker:
    def __init__(self, tz: str, scratch: str, clock: Clock) -> None:
        self.tz = tz
        self.scratch = scratch
        self.clock = clock
        self.hello = meta.hello(tz)
        self.session: Session | None = None

    def handle(self, req: Any) -> dict:
        if not isinstance(req, dict) or not isinstance(req.get("cmd"), str):
            raise DriverError("protocol_error", detail="a request is an object with a string cmd")
        cmd = req["cmd"]
        if cmd == "hello":
            return dict(self.hello)
        if cmd == "run":
            self._no_open_session()
            started = time.perf_counter()
            session = Session(req.get("scenario"), self.tz, self.scratch, self.clock)
            try:
                observations = session.run_all()
            finally:
                session.close()
            return {
                "scenario_id": session.scenario["id"],
                "observations": observations,
                "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
            }
        if cmd == "begin":
            self._no_open_session()
            self.session = Session(req.get("scenario"), self.tz, self.scratch, self.clock)
            return {"scenario_id": self.session.scenario["id"], "steps": len(self.session.steps)}
        if cmd == "step":
            session = self._open_session()
            if "step" in req:
                index = len(session.outputs)
                return {"index": index, "observation": session.run_step(index, req["step"])}
            index = len(session.outputs)
            if index >= len(session.steps):
                raise DriverError("no_more_steps", steps=len(session.steps))
            return {"index": index, "observation": session.run_step(index, session.steps[index])}
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
                req = json.loads(line)
                if isinstance(req, dict):
                    req_id = req.get("id")
                resp = {"ok": True, **self.handle(req)}
            except json.JSONDecodeError:
                resp = {"ok": False, "error": {"kind": "protocol_error", "detail": "request is not JSON"}}
            except DriverError as exc:
                resp = {"ok": False, "error": exc.to_json()}
            except Exception as exc:  # noqa: BLE001 - never let one request kill the worker
                traceback.print_exc(file=sys.stderr)
                resp = {"ok": False, "error": {"kind": "internal_error", "exception": type(exc).__name__}}
            if req_id is not None:
                resp["id"] = req_id
            out.write(json.dumps(resp, ensure_ascii=False, separators=(",", ":"), allow_nan=False) + "\n")
            out.flush()
        if self.session is not None:
            self.session.close()


def main() -> int:
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
    clock_file = os.environ.get("FAKETIME_TIMESTAMP_FILE")
    scratch = os.environ.get("ORACLE_SCRATCH")
    if not clock_file or not scratch or "libfaketime" not in os.environ.get("LD_PRELOAD", ""):
        print("oracle: start workers through entrypoint.sh (libfaketime is not set up)", file=sys.stderr)
        return 78
    registry.load_all()
    worker = Worker(tz, scratch, Clock(clock_file))
    worker.serve(sys.stdin, proto_out)
    return 0
