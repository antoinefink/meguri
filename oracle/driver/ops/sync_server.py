# SPDX-License-Identifier: AGPL-3.0-or-later
"""The sync server of an actor scenario, its fault proxy and its endpoints
(specs/conformance/sync-ops.md S3, S8, S9).

Every actor-scenario session starts Anki's own sync server
(`python -m anki.syncserver` from the pinned wheel) as a child of the worker:
a new SYNC_BASE under the worker's scratch tmpfs, a port the worker picks, one
account with fixed test credentials, no proxy variables, and the worker's
libfaketime clock, so it shares the scenario clock. With
server.clock_offset_secs it reads a second clock file that the worker clock
keeps at scenario clock + offset (Clock.follow). The session waits at most
10 s for it to accept connections, else sync_unavailable. Its output goes to
the worker's stderr. Session close stops it and deletes its base directory.

The server listens on loopback, or on every address of the worker (loopback
and its oracle-internal address, the container's only interfaces) when a fault
proxy or the recorder must reach it: the converge client always syncs over
loopback (S8.1), so a recording run needs both.

A run has a fault proxy when the scenario has a net_fault step or the caller
plays actors: a toxiproxy proxy, created through the API that
ORACLE_SYNC_FAULTS names, with a name unique to the run, listen port 0 and
upstream <worker address>:<server port>; every actor syncs through it, and it
is deleted at session end for replay only. A mixed run's caller owns its
proxy from creation and removes it after all published edges are gone. The
worker address is the local address of the
route to the toxiproxy (or recorder) host. Nothing connects to the server
through it before the first step.

In a recording run (ORACLE_SYNC_RECORDER; worker.proxy_environment routes HTTP
through it) each sync step's endpoint carries the prefix /<actor>/<step>/ and
never points at loopback.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from typing import Any

from ..errors import DriverError, invalid
from ..registry import session_resource

# Test credentials of the scenario's account, the same in every run so that
# recordings do not change from one run to the next (S3.1).
USERNAME = "meguri-oracle"
PASSWORD = "meguri-oracle-test"
READY_TIMEOUT_SECS = 10.0
STOP_TIMEOUT_SECS = 5.0
API_TIMEOUT_SECS = 5.0
PROXY_VARIABLES = ("http_proxy", "https_proxy", "all_proxy", "no_proxy")
TOXICS = {"to_client": "downstream", "to_server": "upstream"}


def _unavailable(detail: str) -> DriverError:
    return DriverError("sync_unavailable", detail=detail)


def _host_port(value: str, what: str) -> tuple[str, int]:
    host, sep, port = value.rpartition(":")
    if not sep or not host or not port.isdigit():
        raise _unavailable(f"{what} is host:port, not {value!r}")
    return host, int(port)


def _route_address(host: str, port: int) -> str:
    """The worker's own address on the route to host (no packet is sent)."""
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.connect((socket.gethostbyname(host), port))
            return s.getsockname()[0]
    except OSError as exc:
        raise _unavailable(f"no route from the worker to {host}") from exc


def _free_port(host: str) -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind((host, 0))
        return s.getsockname()[1]


class Toxiproxy:
    """The toxiproxy HTTP API, never through a proxy variable."""

    def __init__(self, api: str) -> None:
        self.host, self.port = _host_port(api, "ORACLE_SYNC_FAULTS")
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def call(self, method: str, path: str, body: Any = None, ok_statuses: tuple[int, ...] = ()) -> Any:
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(f"http://{self.host}:{self.port}{path}", data=data, method=method)
        if data is not None:
            req.add_header("Content-Type", "application/json")
        try:
            with self.opener.open(req, timeout=API_TIMEOUT_SECS) as resp:
                text = resp.read()
        except urllib.error.HTTPError as exc:
            if exc.code in ok_statuses:
                return None
            raise _unavailable(f"toxiproxy refused {method} {path} ({exc.code})") from exc
        except (OSError, ValueError) as exc:
            raise _unavailable(f"toxiproxy API at {self.host}:{self.port} is unreachable") from exc
        return json.loads(text) if text.strip() else None


class FaultProxy:
    """The run's toxiproxy proxy in front of the server (S5, S8.1)."""

    def __init__(self, api: Toxiproxy, upstream: str, owner: str | None = None) -> None:
        self.api = api
        self.caller_owned = owner is not None
        prefix = f"meguri-edge-{owner}" if self.caller_owned else "meguri-oracle"
        self.name = f"{prefix}-{uuid.uuid4().hex}"
        created = api.call("POST", "/proxies", {
            "name": self.name, "listen": "0.0.0.0:0", "upstream": upstream, "enabled": True,
        })
        try:
            listen = created["listen"]
            self.port = int(listen.rpartition(":")[2])
        except (TypeError, KeyError, ValueError) as exc:
            if not self.caller_owned:
                self.delete()
            raise _unavailable("toxiproxy did not report the proxy's listen port") from exc
        self.host = api.host

    def reset(self, direction: str) -> None:
        # 409: that toxic is already set, which changes nothing (S5).
        self.api.call("POST", f"/proxies/{self.name}/toxics", {
            "name": f"reset_{direction}", "type": "reset_peer", "stream": TOXICS[direction],
            "toxicity": 1.0, "attributes": {"timeout": 0},
        }, ok_statuses=(409,))

    def clear(self) -> None:
        for toxic in self.api.call("GET", f"/proxies/{self.name}/toxics") or []:
            self.api.call("DELETE", f"/proxies/{self.name}/toxics/{toxic['name']}", ok_statuses=(404,))

    def delete(self) -> None:
        self.api.call("DELETE", f"/proxies/{self.name}", ok_statuses=(404,))


class SyncRun:
    """One actor-scenario session's server, fault proxy and endpoints."""

    def __init__(self, session) -> None:
        self.session = session
        self.proc: subprocess.Popen | None = None
        self.base: str | None = None
        self.clock_file: str | None = None
        self.proxy: FaultProxy | None = None
        owner = None
        self.route_worker = None
        if session.caller_actors:
            owner = os.environ.get("ORACLE_SYNC_EDGE_OWNER", "")
            self.route_worker = os.environ.get("ORACLE_SYNC_WORKER", "")
            if (
                not re.fullmatch(r"[0-9a-f]{32}", owner)
                or not re.fullmatch(r"[0-9a-f]{32}", self.route_worker)
                or owner == self.route_worker
            ):
                raise _unavailable("mixed routes require distinct trusted ORACLE_SYNC_EDGE_OWNER and ORACLE_SYNC_WORKER tokens")
        self.recorder = os.environ.get("ORACLE_SYNC_RECORDER") or None
        steps = session.scenario.get("steps", [])
        needs_faults = bool(session.caller_actors) or any(
            isinstance(s, dict) and "net_fault" in s for s in steps
        )
        faults = os.environ.get("ORACLE_SYNC_FAULTS") or None
        if needs_faults and not faults:
            raise _unavailable("this run needs a fault proxy: start the worker with ORACLE_SYNC_FAULTS=<toxiproxy API host:port>")
        api = Toxiproxy(faults) if needs_faults else None
        if api is not None:
            self.address = _route_address(api.host, api.port)
        elif self.recorder:
            self.address = _route_address(*_host_port(self.recorder, "ORACLE_SYNC_RECORDER"))
        else:
            self.address = "127.0.0.1"
        listen = "0.0.0.0" if (api is not None or self.recorder) else "127.0.0.1"
        try:
            self._start_server(listen, session.scenario.get("server", {}).get("clock_offset_secs", 0))
            if api is not None:
                self.proxy = FaultProxy(api, f"{self.address}:{self.port}", owner=owner)
        except BaseException:
            self.close()
            raise
        direct = f"http://127.0.0.1:{self.port}/"
        if self.proxy is not None:
            self.actor_base = f"http://{self.proxy.host}:{self.proxy.port}/"
        elif self.recorder:
            self.actor_base = f"http://{self.address}:{self.port}/"
        else:
            self.actor_base = direct
        self.converge_base = direct

    def _start_server(self, listen: str, offset: int) -> None:
        scratch = self.session.scratch
        self.base = tempfile.mkdtemp(prefix="sync-", dir=scratch)
        self.port = _free_port(listen)
        env = {k: v for k, v in os.environ.items() if k.lower() not in PROXY_VARIABLES}
        env.update(SYNC_BASE=self.base, SYNC_HOST=listen, SYNC_PORT=str(self.port), SYNC_USER1=f"{USERNAME}:{PASSWORD}")
        env["NO_COLOR"] = "1"  # plain log lines in the worker's stderr
        for key in [k for k in env if k.startswith("SYNC_USER") and k != "SYNC_USER1"]:
            del env[key]
        if offset:
            fd, self.clock_file = tempfile.mkstemp(prefix="server-clock-", dir=scratch)
            os.close(fd)
            self.session.clock.follow(self.clock_file, offset)
            env["FAKETIME_TIMESTAMP_FILE"] = self.clock_file
        self.proc = subprocess.Popen(
            [sys.executable, "-m", "anki.syncserver"],
            stdin=subprocess.DEVNULL, stdout=sys.stderr.fileno(), stderr=sys.stderr.fileno(),
            env=env, cwd=self.base,
        )
        deadline = time.monotonic() + READY_TIMEOUT_SECS
        while True:
            if self.proc.poll() is not None:
                raise _unavailable(f"the sync server exited ({self.proc.returncode}) before it accepted connections")
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.5):
                    return
            except OSError:
                pass
            if time.monotonic() >= deadline:
                raise _unavailable(f"the sync server did not accept connections within {READY_TIMEOUT_SECS:.0f} s")
            time.sleep(0.05)

    # --- what ops use ---------------------------------------------------------------

    def actor_endpoint(self, actor: str, step_index: int) -> str:
        """The base URL an oracle actor syncs with at this step (S8.1, S9)."""
        if self.recorder:
            return f"{self.actor_base}{actor}/{step_index}/"
        return self.actor_base

    def fault(self, args: Any) -> None:
        if not isinstance(args, dict) or args.get("kind") not in ("reset", "clear"):
            raise invalid("net_fault takes {kind: reset, direction: to_client|to_server} or {kind: clear}")
        if args["kind"] == "clear":
            if set(args) != {"kind"}:
                raise invalid("net_fault clear takes no other key")
            self.proxy.clear()
            return
        if set(args) != {"kind", "direction"} or args["direction"] not in TOXICS:
            raise invalid("net_fault reset takes direction: to_client or to_server")
        self.proxy.reset(args["direction"])

    def describe(self) -> dict:
        """The begin response's `sync` member (protocol.json sync_info)."""
        return {
            "endpoints": {
                "actors": self.actor_base,
                "converge": self.converge_base,
                "upstream": f"{self.address}:{self.port}" if self.proxy is not None else None,
                "recorder": self.recorder,
            },
            "account": {"username": USERNAME, "password": PASSWORD},
            "route": {"proxy": self.proxy.name, "worker": self.route_worker} if self.route_worker is not None else None,
        }

    def close(self) -> None:
        try:
            if self.proxy is not None and not self.proxy.caller_owned:
                self.proxy.delete()
        finally:
            self.proxy = None
            if self.proc is not None:
                self.proc.terminate()
                try:
                    self.proc.wait(timeout=STOP_TIMEOUT_SECS)
                except subprocess.TimeoutExpired:
                    self.proc.kill()
                    self.proc.wait()
                self.proc = None
            if self.clock_file is not None:
                self.session.clock.unfollow(self.clock_file)
                for path in (self.clock_file, f"{self.clock_file}.tmp"):
                    try:
                        os.unlink(path)
                    except FileNotFoundError:
                        pass
                self.clock_file = None
            if self.base is not None:
                shutil.rmtree(self.base, ignore_errors=True)
                self.base = None


@session_resource("sync")
def sync_run(session):
    return SyncRun(session) if session.is_actor_scenario else None
