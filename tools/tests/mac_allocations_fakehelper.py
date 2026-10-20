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
        home = patch.object(Path, "home", return_value=Path(self.tmp.name) / "home")
        home.start()
        self.addCleanup(home.stop)
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
        if getattr(proc, "role", None) and self.mode.startswith(proc.role + "-verify-"):
            self.assertEqual(self.m.read(self.root / (proc.role + ".launch.json"))["pid"], pid)
            raise RuntimeError(proc.role + " procargs access failure after Popen")
        if self.mode == "xcrun-exec" and proc.role == "trace" and self.now < 0.1:
            return dict(proc.record, executable="/usr/bin/xcrun"), ["xcrun"] + proc.argv
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
        role = "observer" if "--observe" in args else "trace" if args[:2] == ["xcrun", "xctrace"] else "waiter"
        pid = {"trace": 201, "waiter": 200, "observer": 202}[role]
        executable = {"trace": "/Xcode space's/usr/bin/xctrace", "waiter": "/usr/bin/notifyutil", "observer": self.own["executable"]}[role]
        argv = [executable] + (args[2:] if role == "trace" else args[1:])
        intent = self.m.read(self.root / (role + ".launch.json"))
        self.assertNotIn("pid", intent)  # Intent is durable before any spawn.
        self.assertEqual(intent["executable"], executable)
        self.assertEqual(intent["args"], argv[1:])
        self.assertEqual(intent["uid"], os.getuid())
        self.assertEqual(intent["run"], self.root.parent.name)

        class Proc:
            def __init__(self):
                self.pid, self.role, self.argv, self.rc = pid, role, argv, None
                self.record = test.record(pid, executable)

            def poll(self):
                if role == "trace" and test.mode == "attach-failure":
                    self.rc = 64
                elif role == "trace" and not test.mode.startswith("trace-verify-") and test.mode not in ("stop", "reuse", "kill-fallback", "missing-notify") and test.now >= 0.2:
                    self.rc = 0
                return self.rc

            def wait(self, timeout=None):
                test.events.append((self.pid, "wait", timeout))
                if self.rc is None and test.mode.startswith(role + "-verify-"):
                    test.now += timeout
                    raise subprocess.TimeoutExpired(argv, timeout)
                if role == "waiter" and self.rc is None:
                    self.rc = 0
                return self.rc

            def send_signal(self, sig):
                if self.rc is None:
                    test.kill(self.pid, sig)

            def kill(self):
                self.send_signal(signal.SIGKILL)

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
        if (self.mode == "kill-fallback" or self.mode == proc.role + "-verify-kill") and sig != signal.SIGKILL:
            return
        if self.mode == proc.role + "-verify-unknown":
            return
        proc.rc = 0 if sig == signal.SIGINT else -int(sig)

    def observe(self):
        test = self

        class Thread:
            def __init__(self, target, **kwargs):
                self.target = target

            def start(self):
                test.events.append("notification-thread")
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
        with patch.object(self.m.subprocess, "run", return_value=types.SimpleNamespace(returncode=0, stdout="999999 S Tue Oct 20 00:00:00 2026", stderr="")), patch.object(self.m, "argv_for") as argv:
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

    def test_failed_remember_reaps_owned_children(self):
        for mode in ("waiter-verify-error", "trace-verify-error", "trace-verify-kill"):
            with self.subTest(mode=mode):
                self.mode = mode
                self.now = self.lookups = 0
                self.events = []
                self.processes = {}
                (self.root / "notification.json").unlink(missing_ok=True)
                meta = self.observe()
                role = mode.split("-")[0]
                pid = 200 if role == "waiter" else 201
                self.assertEqual(meta["status"], "error")
                self.assertTrue(any("after Popen" in e for e in meta["errors"]))
                self.assertIsNotNone(self.processes[pid].rc)
                self.assertIn((pid, signal.SIGTERM if role == "waiter" else signal.SIGINT), self.events)
                self.assertIn((pid, "wait", 2), self.events)
                saved = meta["launches"][role]
                self.assertTrue(saved["cleanup_confirmed"])
                self.assertEqual(saved["exit"], self.processes[pid].rc)
                if mode.endswith("kill"):
                    self.assertIn((pid, signal.SIGKILL), self.events)
                    self.assertTrue(saved["forced"])
                if role == "waiter":
                    self.assertNotIn("notification-thread", self.events)
                    self.assertNotIn(201, self.processes)
                self.assertLess(self.now, 3)
                import shutil
                shutil.rmtree(self.root / "allocations.trace", ignore_errors=True)

    def test_observer_launcher_uses_current_interpreter_identity(self):
        xcode = Path(self.tmp.name).resolve() / "Xcode space's.app/Contents/Developer"
        launcher = xcode / "usr/bin/python3"
        interpreter = xcode / "Library/Frameworks/Python3.framework/Versions/3.9/Resources/Python.app/Contents/MacOS/Python"
        for path in (launcher, interpreter):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        self.assertEqual(os.path.realpath(launcher), str(launcher))
        self.assertNotEqual(os.path.realpath(launcher), str(interpreter))
        self.own = self.record(os.getpid(), str(interpreter))
        (self.root / "config.json").unlink()

        def popen(args, **kwargs):
            proc = self.popen(args, **kwargs)
            (self.root / "ready").touch()
            return proc

        with patch.object(self.m, "__file__", str(self.root / "mac-allocations.py")), \
             patch.object(sys, "argv", ["helper", "--start", str(self.root), self.binary, "run-test"]), \
             patch.object(sys, "executable", str(launcher)), \
             patch.object(self.m, "target", return_value=None), \
             patch.object(self.m, "identity", side_effect=self.identity) as identity, \
             patch.object(self.m.subprocess, "Popen", side_effect=popen) as spawn, \
             patch.object(self.m.time, "monotonic", lambda: self.now), \
             patch.object(self.m.time, "sleep", self.sleep):
            self.assertEqual(self.m.main(), 0)
        args = self.m.child_args("observer", self.cfg, self.root)
        self.assertEqual(spawn.call_args.args[0], [str(launcher)] + args)
        self.assertEqual([call.args[0] for call in identity.call_args_list], [os.getpid(), 202])
        self.assertEqual(self.processes[202].argv, [str(interpreter)] + args)
        self.assertEqual(self.m.read(self.root / "observer.json"), self.record(202, str(interpreter)))
        self.assertEqual(self.m.read(self.root / "observer.launch.json"), {
            "run": "run-test", "uid": os.getuid(), "executable": str(interpreter),
            "args": args, "cleanup_confirmed": False, "pid": 202,
        })

    def test_unknown_parent_identity_refuses_observer_spawn(self):
        (self.root / "config.json").unlink()
        for own in (None, RuntimeError("parent procargs unknown")):
            with self.subTest(own=own), \
                 patch.object(self.m, "__file__", str(self.root / "mac-allocations.py")), \
                 patch.object(sys, "argv", ["helper", "--start", str(self.root), self.binary, "run-test"]), \
                 patch.object(self.m, "target", return_value=None), \
                 patch.object(self.m, "identity", side_effect=[own]) as identity, \
                 patch.object(self.m.subprocess, "Popen") as popen:
                self.assertRaisesRegex(RuntimeError, "unknown", self.m.main)
                identity.assert_called_once_with(os.getpid())
                popen.assert_not_called()
            for name in ("config.json", "observer.json", "observer.launch.json"):
                self.assertFalse((self.root / name).exists())
            self.assertFalse(self.m.registration(self.root).exists())

    def test_failed_observer_remember_reaps_child(self):
        self.mode = "observer-verify-error"
        (self.root / "config.json").unlink()
        with patch.object(self.m, "__file__", str(self.root / "mac-allocations.py")), \
             patch.object(sys, "argv", ["helper", "--start", str(self.root), self.binary, "run-test"]), \
             patch.object(self.m, "target", return_value=None), \
             patch.object(self.m, "identity", self.identity), \
             patch.object(self.m.subprocess, "Popen", self.popen), \
             patch.object(self.m.os, "kill", self.kill):
            self.assertRaisesRegex(RuntimeError, "after Popen", self.m.main)
        self.assertIn((202, signal.SIGTERM), self.events)
        self.assertIn((202, "wait", 2), self.events)
        self.assertIsNotNone(self.processes[202].rc)
        self.assertTrue(self.m.read(self.root / "observer.launch.json")["cleanup_confirmed"])

    def test_transient_xcrun_exec_is_verified(self):
        self.mode = "xcrun-exec"
        self.assertEqual(self.observe()["status"], "pass")
        self.assertEqual(self.m.read(self.root / "trace.json")["executable"], "/Xcode space's/usr/bin/xctrace")

    def test_defunct_and_live_procargs_failure(self):
        def ps(status):
            return types.SimpleNamespace(returncode=0, stdout=str(os.getuid()) + " " + status + " Tue Oct 20 00:00:00 2026", stderr="")

        for status in ("Z", "Z+", "X"):
            with patch.object(self.m.subprocess, "run", return_value=ps(status)), patch.object(self.m, "argv_for") as argv:
                self.assertIsNone(self.m.identity(123))
                argv.assert_not_called()
        with patch.object(self.m, "argv_for", side_effect=RuntimeError("access denied")):
            for status in ("Z", "X", "S"):
                with patch.object(self.m.subprocess, "run", side_effect=[ps("S"), types.SimpleNamespace(returncode=0, stdout=status, stderr="")]):
                    if status == "S":
                        self.assertRaisesRegex(RuntimeError, "access denied", self.m.identity, 123)
                    else:
                        self.assertIsNone(self.m.identity(123))

    def test_interrupted_launch_stages_block_until_certified(self):
        code = (SOURCE.parent / "mac-vm-lib.sh").read_text().split("<<'PYALLOC'\n", 1)[1].split("\nPYALLOC", 1)[0]
        cfg = dict(self.cfg, target_pid=123)
        self.m.write(self.root / "config.json", cfg)
        (self.root / "mac-allocations.py").write_text("retained helper")
        (self.root / "evidence").write_text("partial trace/logs")
        self.m.write(self.root / "observer.json", self.record(301, sys.executable))
        marker = self.m.registration(self.root)
        marker.parent.mkdir(parents=True)
        signals = []

        def run(args, **_kwargs):
            self.assertEqual(args[0], "pgrep")
            # No process discovery is required to find unresolved launch state.
            return subprocess.CompletedProcess(args, 1, "", "")

        def recover(args):
            self.assertEqual(args, ["python3", str(self.root / "mac-allocations.py"), "--orphan-stop", str(self.root)])
            return self.m.stop(self.root, orphan=True)

        def kill(pid, sig):
            signals.append((pid, sig))
            self.kill(pid, sig)

        for stage in ("intent", "pid", "verified", "reused", "direct-wait"):
            with self.subTest(stage=stage):
                import shutil
                shutil.rmtree(self.root / "allocations.trace", ignore_errors=True)
                self.m.write(marker, {"root": str(self.root)})
                self.m.write(self.root / "metadata.json", {"errors": [], "observer_ended_at": "done"})
                path = self.root / "trace.json"
                path.unlink(missing_ok=True)
                intent = {"run": self.root.parent.name, "uid": os.getuid(), "executable": "/Xcode space's/usr/bin/xctrace",
                          "args": self.m.child_args("trace", cfg, self.root), "cleanup_confirmed": False}
                self.m.write(self.root / "trace.launch.json", intent)
                signals.clear()
                self.processes.clear()
                if stage != "intent":
                    proc = self.popen(["xcrun", "xctrace"] + intent["args"])
                    intent["pid"] = proc.pid
                    if stage in ("verified", "reused"):
                        self.m.write(path, proc.record)
                    if stage == "reused":
                        proc.record = dict(proc.record, start="unrelated new process")
                    if stage == "direct-wait":
                        # A persisted successful wait certifies the old child,
                        # even when the PID is now occupied by someone else.
                        intent.update(cleanup_confirmed=True, exit=-15)
                        proc.record = dict(proc.record, start="unrelated new process")
                    self.m.write(self.root / "trace.launch.json", intent)
                with patch.object(self.m, "identity", self.identity), \
                     patch.object(self.m.subprocess, "run", run), \
                     patch.object(self.m.subprocess, "call", recover), \
                     patch.object(self.m.os, "kill", kill), \
                     patch.object(self.m.time, "monotonic", lambda: self.now), \
                     patch.object(self.m.time, "sleep", self.sleep):
                    if stage in ("verified", "direct-wait"):
                        exec(code, {})
                        self.assertFalse(marker.exists())
                        self.assertEqual(signals, [(201, signal.SIGINT)] if stage == "verified" else [])
                    else:
                        for _attempt in range(2):
                            self.assertRaisesRegex(RuntimeError, "cleanup unconfirmed", exec, code, {})
                        self.assertTrue(marker.exists())
                        self.assertFalse(self.m.read(self.root / "metadata.json")["cleanup_confirmed"])
                        self.assertEqual(signals, [])
                self.assertTrue((self.root / "mac-allocations.py").exists())
                self.assertEqual((self.root / "evidence").read_text(), "partial trace/logs")

    def test_failed_direct_cleanup_retains_unknown_state(self):
        self.mode = "trace-verify-unknown"
        meta = self.observe()
        saved = meta["launches"]["trace"]
        self.assertEqual(meta["status"], "error")
        self.assertFalse(saved["cleanup_confirmed"])
        self.assertIn("cleanup_error", saved)
        self.assertIsNone(self.processes[201].rc)
        self.assertIn((201, signal.SIGINT), self.events)
        self.assertIn((201, signal.SIGKILL), self.events)
        self.assertEqual(self.events.count((201, "wait", 2)), 2)
        self.assertLessEqual(self.now, 4.1)
        # The observer has since exited. Its child still lacks saved identity;
        # a later stop must neither signal the PID nor certify cleanup.
        self.m.write(self.root / "observer.json", self.record(301, sys.executable))
        self.events.clear()
        with patch.object(self.m, "identity", self.identity), patch.object(self.m.os, "kill", self.kill):
            self.assertEqual(self.m.stop(self.root), 2)
        self.assertEqual(self.events, [])
        self.assertTrue((self.root / "allocations.trace/data").exists())
        self.assertFalse(self.m.read(self.root / "metadata.json")["cleanup_confirmed"])

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
                self.events = []
                self.processes = {}
                meta = self.observe()
                self.assertEqual(meta["status"], "error")
                if mode == "attach-failure":
                    self.assertEqual(meta["launches"]["trace"]["exit"], 64)
                    self.assertTrue(meta["launches"]["trace"]["cleanup_confirmed"])
                    self.assertFalse(any(isinstance(e, tuple) and len(e) == 2 and e[0] == 201 for e in self.events))

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
            return subprocess.CompletedProcess(args, 0 if pid in bank else 1, str(os.getuid()) + " S " + start if pid in bank else "", "")

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

    def test_orphan_defunct_and_access_failure(self):
        code = (SOURCE.parent / "mac-vm-lib.sh").read_text().split("<<'PYALLOC'\n", 1)[1].split("\nPYALLOC", 1)[0]
        for status in ("Z", "X", "S", "defunct-race"):
            with self.subTest(status=status):
                def run(args, **_kwargs):
                    if args[0] == "pgrep":
                        return subprocess.CompletedProcess(args, 0, "403\n", "")
                    self.assertEqual(args[0], "ps")
                    value = ("Z" if status == "defunct-race" else status) if args[-1] == "stat=" else str(os.getuid()) + " " + ("S" if status == "defunct-race" else status) + " Tue Oct 20 10:20:30 2026"
                    return subprocess.CompletedProcess(args, 0, value, "")

                with patch.object(subprocess, "run", run), \
                     patch.object(self.m.ctypes, "CDLL", return_value=types.SimpleNamespace(sysctl=lambda *_args: 1)) as libc, \
                     patch.object(os, "kill") as kill, patch.object(subprocess, "call") as recover:
                    if status == "S":
                        self.assertRaisesRegex(RuntimeError, "argv unknown", exec, code, {})
                    else:
                        exec(code, {})
                    if status in ("Z", "X"):
                        libc.assert_not_called()
                    kill.assert_not_called()
                    recover.assert_not_called()

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
