#!/usr/bin/env python3
"""Fast scripted process/clock fakes for mac-allocations.py; no real attach."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import struct
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / "scripts/mac-allocations.py"


class Allocations(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("allocations", SOURCE)
        self.m = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.m)
        self.tmp = tempfile.TemporaryDirectory(prefix="allocations quote's space ")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / "build/mg/ui-test/run-test/allocations"
        self.root.mkdir(parents=True)
        self.binary = str(Path(self.tmp.name) / "Build/Products/Profile/Meguri.app/Contents/MacOS/Meguri")
        self.cfg = {"binary": self.binary, "notification": "com.antoinefink.meguri.mg.allocations.run-test"}
        self.m.write(self.root / "config.json", self.cfg)
        self.now = 0
        self.lookups = 0
        self.events = []
        self.processes = {}
        self.mode = "normal"
        self.args = [self.binary, "-MeguriRoute", "spike/card-swap", "a space's value"]
        self.own = self.record(os.getpid(), sys.executable)
        self.selected = self.record(123, self.binary)

    def record(self, pid, executable):
        return {"pid": pid, "uid": os.getuid(), "start": "Tue Oct 20 10:20:30 2026", "executable": executable}

    def identity(self, pid):
        if pid == os.getpid():
            return self.own, [sys.executable] + self.m.child_args("observer", self.cfg, self.root)
        if pid == 123:
            return self.selected, self.args
        proc = self.processes.get(pid)
        if proc is None or proc.rc is not None:
            return None
        record = dict(proc.record)
        if self.mode == "reuse" and self.now >= 0.3 and proc.role == "trace":
            record["start"] = "another process start"
        return record, proc.argv

    def run_command(self, args, **kwargs):
        self.assertEqual(args, ["pgrep", "-x", "Meguri"])
        self.lookups += 1
        self.events.append("lookup")
        if self.mode == "no-pid" or (self.mode == "late" and self.now < 31):
            return subprocess.CompletedProcess(args, 1, "", "")
        if self.mode == "multiple":
            self.processes[124] = types.SimpleNamespace(rc=None, record=self.record(124, self.binary), argv=self.args)
            return subprocess.CompletedProcess(args, 0, "123\n124\n", "")
        if self.mode == "changed-target" and self.lookups == 2:
            self.selected = dict(self.selected, start="new app start")
        return subprocess.CompletedProcess(args, 0, "123\n", "")

    def popen(self, args, **kwargs):
        self.events.append(args)
        test = self
        role = "trace" if args[:2] == ["xcrun", "xctrace"] else "waiter"
        pid = 201 if role == "trace" else 200
        executable = "/Xcode space's/usr/bin/xctrace" if role == "trace" else "/usr/bin/notifyutil"
        argv = [executable] + (args[2:] if role == "trace" else args[1:])

        class Proc:
            def __init__(self):
                self.pid, self.role, self.argv, self.rc = pid, role, argv, None
                self.record = test.record(pid, executable)

            def poll(self):
                if role == "trace" and test.mode == "attach-failure":
                    self.rc = 64
                elif role == "trace" and test.mode not in ("stop", "reuse", "kill-fallback", "missing-notify") and test.now >= 0.2:
                    self.rc = 0
                return self.rc

            def wait(self):
                if role == "waiter":
                    self.rc = 0
                return self.rc

        proc = Proc()
        self.processes[pid] = proc
        if role == "trace" and self.mode != "missing-trace":
            trace = self.root / "allocations.trace"
            trace.mkdir()
            (trace / "data").write_text("fake allocation stacks")
        return proc

    def sleep(self, seconds):
        self.now += seconds
        if self.mode in ("stop", "reuse", "kill-fallback", "missing-notify") and self.now >= 0.4:
            (self.root / "stop").touch()

    def kill(self, pid, sig):
        self.events.append((pid, sig))
        proc = self.processes[pid]
        if self.mode == "kill-fallback" and sig != signal.SIGKILL:
            return
        proc.rc = 0 if sig == signal.SIGINT else -int(sig)

    def observe(self):
        test = self

        class Thread:
            def __init__(self, target, **kwargs):
                self.target = target

            def start(self):
                if test.mode != "missing-notify":
                    self.target()

            def join(self, **kwargs):
                pass

        with patch.object(self.m, "identity", self.identity), \
             patch.object(self.m.subprocess, "run", self.run_command), \
             patch.object(self.m.subprocess, "Popen", self.popen), \
             patch.object(self.m.subprocess, "check_output", return_value="/Xcode space's/usr/bin/xctrace\n"), \
             patch.object(self.m.shutil, "which", return_value="/usr/bin/notifyutil"), \
             patch.object(self.m.time, "monotonic", lambda: self.now), \
             patch.object(self.m.time, "sleep", self.sleep), \
             patch.object(self.m.os, "kill", self.kill), \
             patch.object(self.m.signal, "signal"), \
             patch.object(self.m.threading, "Thread", Thread):
            self.m.observe(self.root)
        return self.m.read(self.root / "metadata.json")

    def test_exact_numeric_vm_attach(self):
        meta = self.observe()
        self.assertEqual(meta["status"], "pass")
        self.assertFalse(meta["ordinary_performance_receipt"])
        self.assertEqual(meta["coverage"], "uncertified/partial")
        self.assertTrue(meta["start_seen"])
        self.assertIn("tracing_started_notification_at", meta)
        commands = [e for e in self.events if isinstance(e, list)]
        self.assertEqual(commands[0], ["/usr/bin/notifyutil", "-1", self.cfg["notification"]])
        self.assertEqual(commands[1], ["xcrun", "xctrace", "record", "--template", "Allocations", "--attach", "123", "--time-limit", "90s", "--output", str(self.root / "allocations.trace"), "--no-prompt", "--notify-tracing-started", self.cfg["notification"]])
        self.assertEqual(meta["target"]["executable"], self.binary)
        self.assertNotIn("a space's value", json.dumps(meta))

    def test_wrong_routes(self):
        for route in ("spike/capture", "spike/typein", "spike/card-swap-extra"):
            with self.subTest(route=route):
                self.args[2] = route
                self.assertEqual(self.observe()["status"], "error")
                self.assertFalse(any(isinstance(e, list) for e in self.events))
        self.args = [self.binary, "-MeguriRoute", "spike/card-swap", "-MeguriRoute", "spike/capture"]
        self.assertEqual(self.observe()["status"], "error")

    def test_bounded_missing_wrong_and_late_binary(self):
        for mode in ("no-pid", "late", "wrong-binary"):
            with self.subTest(mode=mode):
                self.now = self.lookups = 0
                self.mode = mode
                self.selected = self.record(123, self.binary + "wrong" if mode == "wrong-binary" else self.binary)
                self.assertEqual(self.observe()["status"], "error")
                self.assertLessEqual(self.now, 30.11)
                self.assertLessEqual(self.lookups, 301)
                self.assertFalse(any(isinstance(e, list) for e in self.events))

    def test_multiple(self):
        self.mode = "multiple"
        self.assertIn("multiple matching apps", self.observe()["errors"])
        self.assertFalse(any(isinstance(e, list) for e in self.events))

    def test_recheck_start_identity(self):
        self.mode = "changed-target"
        meta = self.observe()
        self.assertIn("target changed immediately before attach", meta["errors"])
        self.assertFalse(any(isinstance(e, list) and e[:2] == ["xcrun", "xctrace"] for e in self.events))

    def test_uid_and_numeric_identity(self):
        self.assertRaises(RuntimeError, self.m.identity, "123")
        self.assertRaises(RuntimeError, self.m.identity, 0)
        with patch.object(self.m.subprocess, "run", return_value=types.SimpleNamespace(returncode=0, stdout="999999 Tue Oct 20 00:00:00 2026", stderr="")), patch.object(self.m, "argv_for") as argv:
            self.assertRaises(RuntimeError, self.m.identity, 123)
            argv.assert_not_called()
        with patch.object(self.m.subprocess, "run", return_value=types.SimpleNamespace(returncode=1, stdout="", stderr="fork: Resource temporarily unavailable")):
            self.assertRaises(RuntimeError, self.m.identity, 123)

    def test_real_argv_boundaries(self):
        import ctypes
        args = [self.binary, "-MeguriRoute", "spike/card-swap", "space's quoted value"]
        raw = struct.pack("i", len(args)) + os.fsencode(self.binary) + b"\0\0\0" + b"\0".join(os.fsencode(a) for a in args) + b"\0ENV=value\0"

        def sysctl(_mib, _length, buf, size, *_unused):
            ctypes.memmove(buf, raw, len(raw))
            size._obj.value = len(raw)
            return 0

        with patch.object(self.m.ctypes, "CDLL", return_value=types.SimpleNamespace(sysctl=sysctl)):
            self.assertEqual(self.m.argv_for(123), (self.binary, args))
        self.args = [self.binary, "one argument containing -MeguriRoute spike/card-swap"]
        self.assertEqual(self.observe()["status"], "error")

    def test_normal_stop_finalizes(self):
        self.mode = "stop"
        meta = self.observe()
        self.assertEqual(meta["status"], "pass")
        self.assertIn((201, signal.SIGINT), self.events)
        self.assertEqual(meta["trace_exit"], 0)

    def test_pid_reuse_unsignalled(self):
        self.mode = "reuse"
        meta = self.observe()
        self.assertEqual(meta["status"], "error")
        self.assertTrue(any("saved process identity or arguments no longer match" in e for e in meta["errors"]))
        self.assertFalse(any(isinstance(e, tuple) and e[0] == 201 for e in self.events))

    def test_notification_trace_attach_and_finalization_failures(self):
        for mode in ("missing-notify", "missing-trace", "attach-failure", "kill-fallback"):
            with self.subTest(mode=mode):
                if (self.root / "allocations.trace").exists():
                    import shutil
                    shutil.rmtree(self.root / "allocations.trace")
                for name in ("stop", "notification.json", "trace.json", "waiter.json"):
                    (self.root / name).unlink(missing_ok=True)
                self.now = self.lookups = 0
                self.mode = mode
                self.processes = {}
                self.assertEqual(self.observe()["status"], "error")

    def test_remote_stop_order_idempotence_and_reuse(self):
        cfg = dict(self.cfg, target_pid=123)
        self.m.write(self.root / "config.json", cfg)
        self.m.write(self.root / "metadata.json", {"errors": [], "observer_ended_at": "done"})
        for role, pid in (("observer", 301), ("trace", 302), ("waiter", 303)):
            self.m.write(self.root / (role + ".json"), self.record(pid, role))
        calls = []

        def stop_owned(role, *args):
            self.assertTrue((self.root / "stop").exists())
            calls.append(role)

        with patch.object(self.m, "stop_owned", stop_owned), patch.object(self.m, "matches", return_value=False):
            self.assertEqual(self.m.stop(self.root), 0)
            self.assertEqual(calls, ["observer", "trace", "waiter"])
            self.assertEqual(self.m.stop(self.root), 0)
        with patch.object(self.m, "identity", return_value=(self.record(301, "reused"), [])), patch.object(self.m.os, "kill") as kill:
            self.assertEqual(self.m.stop(self.root), 2)
            kill.assert_not_called()

    def test_orphan_scope_and_identity(self):
        # Execute the actual embedded narrow recovery, with fake ps/procargs.
        # A different notification or Instruments template must survive.
        import ctypes
        code = (SOURCE.parent / "mac-vm-lib.sh").read_text().split("<<'PYALLOC'\n", 1)[1].split("\nPYALLOC", 1)[0]
        root = self.root
        cfg = dict(self.cfg, target_pid=123)
        commands = {
            401: ["/usr/bin/python3"] + self.m.child_args("observer", cfg, root),
            402: ["/Xcode/usr/bin/xctrace"] + self.m.child_args("trace", cfg, root),
            403: ["/usr/bin/notifyutil", "-1", cfg["notification"]],
            404: ["/Xcode/usr/bin/xctrace", "record", "--template", "Allocations", "--attach", "123", "--time-limit", "90s", "--output", str(root / "allocations.trace"), "--no-prompt", "--notify-tracing-started", "someone.else"],
            405: ["/Xcode/usr/bin/xctrace", "record", "--template", "Time Profiler", "--all-processes"],
        }
        bank = dict(commands)
        recovered, killed = [], []
        ps_calls = {}
        reuse_waiter = False

        def run(args, **_kwargs):
            if args[0] == "pgrep":
                import re
                ids = [pid for pid, argv in bank.items() if re.search(args[-1], " ".join(argv))]
                return subprocess.CompletedProcess(args, 0 if ids else 1, "\n".join(map(str, ids)), "")
            self.assertEqual(args[0], "ps")
            pid = int(args[args.index("-p") + 1])
            ps_calls[pid] = ps_calls.get(pid, 0) + 1
            start = "changed start" if reuse_waiter and pid == 403 and ps_calls[pid] >= 2 else "Tue Oct 20 10:20:30 2026"
            return subprocess.CompletedProcess(args, 0 if pid in bank else 1, str(os.getuid()) + " " + start if pid in bank else "", "")

        def sysctl(mib, _length, buf, size, *_unused):
            argv = bank[mib[2]]
            raw = struct.pack("i", len(argv)) + os.fsencode(argv[0]) + b"\0\0" + b"\0".join(os.fsencode(a) for a in argv) + b"\0"
            ctypes.memmove(buf, raw, len(raw))
            size._obj.value = len(raw)
            return 0

        def recover(args):
            self.assertEqual(args, ["python3", str(root / "mac-allocations.py"), "--orphan-stop", str(root)])
            recovered.append(args)
            for pid in (401, 402, 403):
                bank.pop(pid, None)
            return 0

        def kill(pid, sig):
            killed.append((pid, sig))
            bank.pop(pid)

        with patch.object(subprocess, "run", run), patch.object(subprocess, "call", recover), \
             patch.object(ctypes, "CDLL", return_value=types.SimpleNamespace(sysctl=sysctl)), \
             patch.object(self.m.shutil, "which", return_value="/usr/bin/notifyutil"), \
             patch.object(os, "kill", kill):
            exec(code, {})
            self.assertEqual(len(recovered), 1)
            self.assertEqual(set(bank), {404, 405})
            self.assertEqual(killed, [])
            # A lone notification waiter is still recoverable without a
            # surviving observer. Other profilers remain untouched.
            bank[403] = commands[403]
            exec(code, {})
            self.assertEqual(killed, [(403, signal.SIGTERM)])
            self.assertEqual(set(bank), {404, 405})
            bank[403] = commands[403]
            ps_calls.clear()
            reuse_waiter = True
            self.assertRaisesRegex(RuntimeError, "waiter identity changed", exec, code, {})
            self.assertEqual(killed, [(403, signal.SIGTERM)])

    def test_refuse_restart_or_preexisting_target(self):
        with patch.object(self.m, "__file__", str(self.root / "mac-allocations.py")), \
             patch.object(sys, "argv", ["helper", "--start", str(self.root), self.binary, "run-test"]), \
             patch.object(self.m.subprocess, "Popen") as popen, \
             patch.object(self.m, "target", return_value=(self.selected, self.args[1:])):
            self.assertRaisesRegex(RuntimeError, "one observer per run", self.m.main)
            (self.root / "config.json").unlink()
            self.assertRaisesRegex(RuntimeError, "already running", self.m.main)
            popen.assert_not_called()


if __name__ == "__main__":
    unittest.main()
