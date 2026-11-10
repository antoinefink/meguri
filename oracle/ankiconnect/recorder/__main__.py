# SPDX-License-Identifier: AGPL-3.0-or-later
"""Record one AnkiConnect transcript scenario (specs/servers/transcripts T16).

  python -m recorder --id <scenario id> --format yaml|json < scenario > golden

Runs in a fresh oracle/ankiconnect container (entrypoint `record`):

1. Parse and check the scenario (canon's YAML 1.2 reader, T3); compute
   scenario_sha256 (conformance/dsl D3).
2. Write the clock file at clock.start and start the pinned Anki (the
   entrypoint's `anki` mode: libfaketime, offscreen) in clock.tz, with the
   add-on config of T4. Anki's stdout and stderr go to a file in the
   container's /scratch and are never printed (localized text, Anki paths).
3. Wait for the meguri_recorder add-on's `ready` and for AnkiConnect's
   listening socket, then ask the add-on (file channel, `control`) to run the
   setup on Anki's open collection with the driver's setup sections (T5).
4. Run the steps in order. An exchange is written as raw bytes on a fresh
   socket (T6.2) and read back (T8); then the add-on's snapshot of notes,
   cards, decks and note types moves the tokens (T10.1) and the reply is
   normalized (T10). `observe` runs the driver's observations in the add-on.
   Every snapshot is checked against the frozen clock (ClockCheck): a clock
   read that escaped libfaketime fails the run as clock_escaped.
5. Print the golden (T9, conformance/dsl D11 text) on stdout.

stderr carries only recorder-authored JSON lines: `{"warning": ...}` (T10.1's
ids left in place) and `{"error": {kind, ...}}` for a run error (exit 1;
exit 2 for an invalid scenario). Nothing from Anki reaches either stream.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import select
import signal
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

from driver import canon, meta
from driver.clock import parse_duration_secs, parse_instant, write_epoch, zone
from driver.errors import DriverError

from . import transcript as tx
from .transcript import RunError

HOME = Path(os.environ.get("ORACLE_ANKICONNECT_HOME", "/opt/ankiconnect"))
SCRATCH = Path(os.environ.get("ORACLE_RECORDER_SCRATCH", "/scratch"))
WORKER = SCRATCH / "worker"
CONTROL = SCRATCH / "recorder"
ANKI_BASE = SCRATCH / "anki"
ANKI_LOG = SCRATCH / "anki.log"
ADDON_DIR = HOME / "addons21" / "2055492159"
START_SECS = 180.0
REPLY_SECS = 60.0  # T6.4
CONTROL_SECS = 120.0
CLOSE_WAIT_SECS = 2.0
FAKETIME_CACHE_SECS = 1  # entrypoint.sh: libfaketime re-reads the clock file after 1-2 s
ADDON_BUILD = "2025-11-09"


def log(obj: dict) -> None:
    print(json.dumps(obj, sort_keys=True), file=sys.stderr, flush=True)


# --- identity (T9) --------------------------------------------------------------------


def addon_pin() -> dict:
    values = {}
    for line in (HOME / "ADDON_PIN").read_text().splitlines():
        if "=" in line and not line.startswith("#"):
            key, _, value = line.partition("=")
            values[key.strip()] = value.strip()
    return values


def oracle_identity() -> dict:
    pin = addon_pin()
    return {
        "anki_version": meta.anki_version(),
        "lock_sha256": meta.lock_sha256(),
        "ankiconnect_lock_sha256": hashlib.sha256((HOME / "requirements.lock").read_bytes()).hexdigest(),
        "addon": {"build": pin.get("build", ADDON_BUILD), "sha256": pin["archive_sha256"]},
        "image": os.environ.get("ORACLE_ANKICONNECT_IMAGE", "unknown"),
        "driver_sha256": tx.recorder_sha256(meta.DRIVER_DIR, meta.driver_files(), HOME),
    }


# --- the add-on channel -----------------------------------------------------------------


class Control:
    """One request at a time: request.json in, response-<n>.json out, both
    written whole (rename), handled on Anki's main thread by the add-on."""

    def __init__(self, anki: subprocess.Popen) -> None:
        self.anki = anki
        self.n = 0

    def call(self, op: str, step: int | None = None, **args) -> dict:
        self.n += 1
        request = {"id": self.n, "op": op, **args}
        tmp = CONTROL / ".request.json.tmp"
        tmp.write_text(json.dumps(request), encoding="utf-8")
        os.replace(tmp, CONTROL / "request.json")
        path = CONTROL / f"response-{self.n}.json"
        deadline = time.monotonic() + CONTROL_SECS
        while not path.exists():
            if self.anki.poll() is not None:
                raise RunError("anki_exited", op=op, **({} if step is None else {"step": step}))
            if time.monotonic() > deadline:
                raise RunError("timeout", op=op, **({} if step is None else {"step": step}))
            time.sleep(0.02)
        response = json.loads(path.read_text(encoding="utf-8"))
        path.unlink()
        if "error" in response:
            error = response["error"]
            fields = {k: v for k, v in error.items() if k != "kind"}
            if step is not None:
                fields.setdefault("step", step)
            raise RunError(error.get("kind", "internal_error"), op=op, **fields)
        return response["ok"]


def start_anki(scenario: dict, tz: str, start: int) -> subprocess.Popen:
    for d in (WORKER, CONTROL):
        d.mkdir(parents=True, exist_ok=True)
    write_epoch(str(WORKER / "clock"), start)
    defaults = json.loads((ADDON_DIR / "config.json").read_text(encoding="utf-8"))
    env = {
        **os.environ,
        "TZ": tz,
        "ORACLE_SCRATCH": str(WORKER),
        "ORACLE_RECORDER_DIR": str(CONTROL),
        "ORACLE_ANKI_BASE": str(ANKI_BASE),
        "ORACLE_ANKICONNECT_CONFIG": json.dumps(tx.addon_config(scenario, defaults)),
        "ORACLE_FAKETIME_CACHE_SECS": str(FAKETIME_CACHE_SECS),
    }
    env.pop("ORACLE_FULL_TRACEBACKS", None)
    out = open(ANKI_LOG, "wb")
    return subprocess.Popen(
        [str(HOME / "entrypoint.sh"), "anki"], env=env, stdin=subprocess.DEVNULL,
        stdout=out, stderr=subprocess.STDOUT, start_new_session=True,
    )


def listening(port: int) -> bool:
    want = f":{port:04X}"
    for table in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            lines = Path(table).read_text().splitlines()[1:]
        except OSError:
            continue
        for line in lines:
            parts = line.split()
            if len(parts) > 3 and parts[1].endswith(want) and parts[3] == "0A":
                return True
    return False


def wait_ready(anki: subprocess.Popen) -> None:
    deadline = time.monotonic() + START_SECS
    while not ((CONTROL / "ready").exists() and listening(tx.PORT)):
        if anki.poll() is not None:
            raise RunError("anki_exited", detail="Anki stopped before its profile opened")
        if time.monotonic() > deadline:
            raise RunError("timeout", detail="Anki's profile or AnkiConnect's socket did not come up")
        time.sleep(0.05)


def wait_clock(control: Control, instant: int, step: int | None) -> None:
    """Until Anki's clock reads instant: libfaketime takes a written instant
    only once its cache has expired (FAKETIME_CACHE_SECS, whole seconds)."""
    deadline = time.monotonic() + 10 + 2 * FAKETIME_CACHE_SECS
    while (seen := control.call("now", step=step)) != instant:
        if time.monotonic() > deadline:
            raise RunError("clock_escaped", clock=instant, seen=seen, detail="Anki's clock did not reach the scenario clock",
                           **({} if step is None else {"step": step}))
        time.sleep(0.2)


def stop_anki(anki: subprocess.Popen) -> None:
    if anki.poll() is None:
        try:
            os.killpg(anki.pid, signal.SIGTERM)
            anki.wait(10)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            try:
                os.killpg(anki.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


# --- one exchange (T6.4, T8) -------------------------------------------------------------


def exchange(data: bytes, step: int) -> dict:
    """Send the request, read the reply while writing, then see whether the
    server closed. A prompt the add-on shows with no queued answer is
    prompt_not_answered (T7.2)."""
    try:
        sock = socket.create_connection(("127.0.0.1", tx.PORT), timeout=5)
    except ConnectionRefusedError:
        raise RunError("refused", step=step) from None
    sent_error: list[BaseException] = []

    def writer() -> None:
        try:
            sock.sendall(data)
        except OSError as exc:  # the server replied (or closed) before the whole request
            sent_error.append(exc)

    thread = threading.Thread(target=writer, daemon=True)
    thread.start()
    buf = b""
    deadline = time.monotonic() + REPLY_SECS
    closed = False
    try:
        while True:
            if (CONTROL / "prompt_waiting").exists():
                raise RunError("prompt_not_answered", step=step)
            if time.monotonic() > deadline:
                raise RunError("timeout", step=step)
            ready, _, _ = select.select([sock], [], [], 0.1)
            if not ready:
                continue
            try:
                chunk = sock.recv(65536)
            except ConnectionResetError:
                if not buf:
                    raise RunError("reset", step=step) from None
                closed = True
                break
            if not chunk:
                closed = True
                break
            buf += chunk
            parts = tx.split_head(buf)
            if parts is None:
                continue
            head, rest = parts
            status, headers = tx.parse_head(head)
            if 100 <= status < 200:
                buf = rest  # an interim reply; T8 records it only for declared scenarios
                continue
            length = tx.content_length(headers)
            if length is not None and len(rest) >= length:
                break
        if not buf:
            raise RunError("closed_without_reply", step=step)
        parts = tx.split_head(buf)
        if parts is None:
            raise RunError("bad_framing", step=step, detail="the server closed inside the reply head")
        head, rest = parts
        status, headers = tx.parse_head(head)
        length = tx.content_length(headers)
        if length is not None:
            if len(rest) < length:
                raise RunError("bad_framing", step=step, detail="the server closed before Content-Length bytes")
            extra, rest = rest[length:], rest[:length]
            if not closed and not extra:
                closed = _closes(sock)
        return tx.reply_value(status, headers, rest, closed)
    finally:
        sock.close()
        thread.join(1)


def _closes(sock: socket.socket) -> bool:
    deadline = time.monotonic() + CLOSE_WAIT_SECS
    while (left := deadline - time.monotonic()) > 0:
        ready, _, _ = select.select([sock], [], [], left)
        if ready:
            try:
                return sock.recv(1) == b""
            except ConnectionResetError:
                return True
    return False


# --- the run -----------------------------------------------------------------------------


def load(text: str, form: str):
    if form == "json":
        return json.loads(text, object_pairs_hook=canon._reject_duplicate_json_keys)
    return canon.load_yaml_text(text)


def media_write(args: dict, env: tx.Env, clock_now: int, step: int) -> None:
    if "text" in args:
        data = args["text"].encode("utf-8")
    elif "base64" in args:
        data = tx.body_bytes({"base64": args["base64"]}, env, step)
    else:
        data = tx.fixture_bytes(args["fixture"], env, step)
    folder = env.media_dir
    path = os.path.join(folder, args["name"])
    with open(path, "wb") as f:
        f.write(data)
    os.utime(path, (clock_now, clock_now))
    os.utime(folder, (clock_now, clock_now))


def run(scenario: dict, sha: str) -> dict:
    clock = scenario["clock"]
    try:
        tzinfo = zone(clock["tz"])
        start = parse_instant(clock["start"], tzinfo)
    except DriverError as exc:
        raise RunError(exc.kind, **exc.fields) from None
    identity = oracle_identity()
    anki = start_anki(scenario, tzinfo.key, start)
    try:
        wait_ready(anki)
        control = Control(anki)
        wait_clock(control, start, None)
        setup = control.call("setup", scenario=scenario)
        tokens = tx.Tokens()
        env = tx.Env(tokens, start)
        env.fixture_base = setup.get("fixture_base")
        env.fixtures_root = os.environ.get("ORACLE_FIXTURES")
        snap = control.call("snapshot")
        env.media_dir, env.profile, env.tts_needle = snap["media_dir"], snap["profile"], snap["tts_needle"]
        clock_check = tx.ClockCheck(start)
        clock_check.check(snap, start, None)
        tokens.start(snap, setup["notes"], setup["cards"])
        norm = tx.Normalizer(env)
        now = start
        permissions = CONTROL / "permission"
        outputs = []
        for index, step in enumerate(scenario["steps"]):
            if "http" in step:
                http = step["http"]
                data = tx.request_bytes(http, env, index)
                reply = exchange(data, index)
                snap = control.call("snapshot", step=index)
                clock_check.check(snap, now, index)
                tokens.update(snap)
                reply, leftovers = norm.reply(reply, tx.request_json(http))
                for pointer in leftovers:
                    log({"warning": "id_left_in_place", "scenario": scenario["id"], "step": index,
                         "pointer": f"/steps/{index}/reply{pointer}"})
                outputs.append({"reply": reply})
                continue
            ((name, args),) = step.items()
            if name == "advance":
                try:
                    now += parse_duration_secs(args)
                except DriverError as exc:
                    raise RunError(exc.kind, step=index, **exc.fields) from None
                write_epoch(str(WORKER / "clock"), now)
                wait_clock(control, now, index)
                outputs.append({})
            elif name == "observe":
                value = control.call("observe", step=index, specs=args,
                                     notes=tokens.logical("n"), cards=tokens.logical("c"))
                outputs.append(norm.observation(value))
            elif name == "media_dir_write":
                media_write(args, env, now, index)
                outputs.append({})
            elif name == "permission":
                with open(permissions, "a", encoding="ascii") as f:
                    f.write(args["answer"] + "\n")
                outputs.append({})
            else:  # interaction: Meguri's side only (T7.5)
                outputs.append({})
        if permissions.exists() and permissions.read_text().strip():
            raise RunError("permission_unused", detail="a scripted permission answer met no prompt (T7.2)")
    finally:
        stop_anki(anki)
    return {
        "scenario_id": scenario["id"],
        "scenario_sha256": sha,
        "oracle": identity,
        "tz": tzinfo.key,
        "clock": {"start": clock["start"], "mode": clock.get("mode", "frozen")},
        "observations": {"steps": outputs},
    }


def main() -> int:
    parser = argparse.ArgumentParser(prog="recorder")
    parser.add_argument("--id", required=True, help="the id the scenario's path gives (T2)")
    parser.add_argument("--format", choices=("yaml", "json"), default="yaml")
    args = parser.parse_args()
    try:
        try:
            scenario = load(sys.stdin.read(), args.format)
        except Exception as exc:  # noqa: BLE001 - the reader's own error class only
            raise tx.invalid(f"the scenario does not parse ({type(exc).__name__})") from None
        tx.check_scenario(scenario, args.id)
        golden = run(scenario, canon.scenario_sha256(scenario))
    except RunError as exc:
        log({"error": exc.to_json()})
        if os.environ.get("ORACLE_FULL_TRACEBACKS") == "1" and ANKI_LOG.exists():
            sys.stderr.write(ANKI_LOG.read_text(errors="replace")[-20000:])
        return 2 if exc.kind == "invalid_scenario" else 1
    sys.stdout.write(canon.golden_text(golden))
    return 0


if __name__ == "__main__":
    sys.exit(main())
