# SPDX-License-Identifier: AGPL-3.0-or-later
"""Caller-owned route and container-local holder boundaries (sync-ops S8.4)."""

from __future__ import annotations

import json
import os
import select
import signal
import subprocess
import sys
import unittest
import uuid
from types import SimpleNamespace
from unittest.mock import Mock, patch

from driver.errors import DriverError
from driver.harness import ORACLE_HOME, WorkerProcess
from driver.ops.sync_server import SyncRun, Toxiproxy


class RouteOwnership(unittest.TestCase):
    def setUp(self):
        self.owner, self.worker = uuid.uuid4().hex, uuid.uuid4().hex
        self.env = {"ORACLE_SYNC_EDGE_OWNER": self.owner, "ORACLE_SYNC_WORKER": self.worker,
                    "ORACLE_SYNC_FAULTS": "sync-faults:8474", "ORACLE_SYNC_RECORDER": ""}
        self.session = SimpleNamespace(caller_actors={"meguri"}, scenario={"steps": []})
        self.api = Mock(host="sync-faults")
        self.api.call.return_value = {"listen": "0.0.0.0:4711"}
        self.patchers = [patch.dict(os.environ, self.env),
                         patch("driver.ops.sync_server.Toxiproxy", return_value=self.api),
                         patch("driver.ops.sync_server._route_address", return_value="192.0.2.10"),
                         patch.object(SyncRun, "_start_server", lambda run, *args: setattr(run, "port", 4712))]
        for patcher in self.patchers:
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_mixed_owner_and_worker_are_required_before_binding_or_proxy_creation(self):
        invalid = [("", self.worker), (self.owner, ""), ("A" * 32, self.worker),
                   (self.owner, self.worker + "0"), (self.owner, self.owner),
                   ("x" * 32, self.worker), (self.owner, self.worker + "\n")]
        for owner, worker in invalid:
            with self.subTest(owner_valid=owner == self.owner, worker_valid=worker == self.worker):
                with patch.dict(os.environ, ORACLE_SYNC_EDGE_OWNER=owner, ORACLE_SYNC_WORKER=worker), \
                     patch.object(SyncRun, "_start_server") as start:
                    with self.assertRaises(DriverError) as raised:
                        SyncRun(self.session)
                    self.assertEqual(raised.exception.kind, "sync_unavailable")
                    start.assert_not_called()
        self.api.call.assert_not_called()

    def test_mixed_route_is_owned_from_creation_and_close_never_deletes_it(self):
        run = SyncRun(self.session)
        route = run.describe()["route"]
        self.assertEqual(route["worker"], self.worker)
        self.assertRegex(route["proxy"], rf"^meguri-edge-{self.owner}-[0-9a-f]{{32}}$")
        method, path, body = self.api.call.call_args.args
        self.assertEqual((method, path), ("POST", "/proxies"))
        self.assertEqual(body["name"], route["proxy"])
        self.assertEqual(body["upstream"], "192.0.2.10:4712")
        run.close()
        run.close()
        self.assertEqual(self.api.call.call_count, 1)

    def test_malformed_create_reply_leaves_mixed_proxy_for_owner_sweep(self):
        self.api.call.return_value = {}
        with self.assertRaises(DriverError):
            SyncRun(self.session)
        self.assertEqual(self.api.call.call_count, 1)
        self.assertTrue(self.api.call.call_args.args[2]["name"].startswith(f"meguri-edge-{self.owner}-"))

    def test_lost_create_reply_leaves_owner_name_available_to_sweep(self):
        self.api.call.side_effect = DriverError("sync_unavailable")
        with self.assertRaises(DriverError):
            SyncRun(self.session)
        self.assertEqual(self.api.call.call_count, 1)
        self.assertTrue(self.api.call.call_args.args[2]["name"].startswith(f"meguri-edge-{self.owner}-"))

    def test_replay_route_is_null_and_close_deletes_its_proxy(self):
        self.session.caller_actors = set()
        self.session.scenario["steps"] = [{"net_fault": {"kind": "clear"}}]
        with patch.dict(os.environ, ORACLE_SYNC_EDGE_OWNER="", ORACLE_SYNC_WORKER=""):
            run = SyncRun(self.session)
        self.assertIsNone(run.describe()["route"])
        name = self.api.call.call_args.args[2]["name"]
        self.assertRegex(name, r"^meguri-oracle-[0-9a-f]{32}$")
        run.close()
        self.assertEqual(self.api.call.call_args.args, ("DELETE", f"/proxies/{name}"))

    def test_malformed_replay_create_reply_still_deletes_proxy(self):
        self.session.caller_actors = set()
        self.session.scenario["steps"] = [{"net_fault": {"kind": "clear"}}]
        self.api.call.return_value = {}
        with self.assertRaises(DriverError):
            SyncRun(self.session)
        self.assertEqual(self.api.call.call_count, 2)
        self.assertEqual(self.api.call.call_args.args[0], "DELETE")


@unittest.skipUnless(os.environ.get("ORACLE_SYNC_FAULTS"), "tools/mg must provide sync-faults")
class DriverRouteLifetime(unittest.TestCase):
    def setUp(self):
        self.owner, self.worker = uuid.uuid4().hex, uuid.uuid4().hex
        self.env = {"ORACLE_SYNC_EDGE_OWNER": self.owner, "ORACLE_SYNC_WORKER": self.worker}
        self.api = Toxiproxy(os.environ["ORACLE_SYNC_FAULTS"])
        self.scenario = {"id": "t/route-lifetime", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
                         "actors": ["client_a", "meguri"], "steps": []}
        self.addCleanup(self.delete_owned_proxies)

    def owned_proxies(self):
        return {name: proxy for name, proxy in self.api.call("GET", "/proxies").items()
                if name.startswith(f"meguri-edge-{self.owner}-")}

    def delete_owned_proxies(self):
        for name in self.owned_proxies():
            self.api.call("DELETE", f"/proxies/{name}", ok_statuses=(404,))

    def begin(self):
        return {"cmd": "begin", "scenario": self.scenario, "caller_actors": ["meguri"]}

    def test_end_keeps_proxy_until_caller_cleanup_then_worker_can_be_reused(self):
        with WorkerProcess("UTC", extra_env=self.env) as worker:
            begun = worker.request(self.begin())
            self.assertTrue(begun["ok"])
            route = begun["sync"]["route"]
            self.assertEqual(route["worker"], self.worker)
            self.assertTrue(worker.request({"cmd": "end"})["ok"])
            self.assertEqual(set(self.owned_proxies()), {route["proxy"]})
            self.delete_owned_proxies()
            next_begin = worker.request(self.begin())
            self.assertTrue(next_begin["ok"])
            self.assertNotEqual(next_begin["sync"]["route"]["proxy"], route["proxy"])
            self.assertTrue(worker.request({"cmd": "end"})["ok"])

    def test_driver_eof_keeps_mixed_proxy(self):
        with WorkerProcess("UTC", extra_env=self.env) as worker:
            begun = worker.request(self.begin())
            self.assertTrue(begun["ok"])
        self.assertEqual(set(self.owned_proxies()), {begun["sync"]["route"]["proxy"]})

    def test_lost_begin_reply_and_eof_still_leave_owned_proxy(self):
        with WorkerProcess("UTC", extra_env=self.env) as worker:
            worker.proc.stdin.write(json.dumps(self.begin()) + "\n")
            worker.proc.stdin.flush()
            worker.proc.stdin.close()
            worker.proc.wait(timeout=30)
            worker.proc.stdin = None
        self.assertEqual(len(self.owned_proxies()), 1)


class WorkerHolder(unittest.TestCase):
    def test_holder_survives_driver_exit_and_closed_stdio_until_signalled(self):
        # Bootstrap writes readiness only after the holder installs both handlers.
        for stop_signal in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signal=stop_signal):
                read_fd, write_fd = os.pipe()
                bootstrap = """
import os, runpy, signal, sys
pause = signal.pause
def ready():
    signal.pause = pause
    os.write(int(sys.argv[1]), b"R")
    pause()
signal.pause = ready
runpy.run_path(sys.argv[2], run_name="__main__")
"""
                env = {key: value for key, value in os.environ.items() if key != "LD_PRELOAD"}
                proc = subprocess.Popen([sys.executable, "-u", "-c", bootstrap, str(write_fd),
                                         str(ORACLE_HOME / "worker_holder.py")],
                                        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.DEVNULL, pass_fds=(write_fd,), env=env)
                os.close(write_fd)
                try:
                    self.assertTrue(select.select([read_fd], [], [], 5)[0])
                    self.assertEqual(os.read(read_fd, 1), b"R")
                    driver = subprocess.run([sys.executable, "-c", "raise SystemExit(7)"],
                                            env=env, capture_output=True, timeout=5)
                    self.assertEqual(driver.returncode, 7)
                    self.assertIsNone(proc.poll())
                    proc.send_signal(stop_signal)
                    self.assertEqual(proc.wait(timeout=5), 0)
                finally:
                    os.close(read_fd)
                    if proc.poll() is None:
                        proc.kill()
                    proc.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
