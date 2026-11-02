#!/usr/bin/env python3
"""Generated bundles/archives and fake command results only. No Apple operations."""
import importlib.util
import contextlib
import io
import json
import os
from pathlib import Path
import plistlib
import select
import shlex
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import types
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("vm_xcode_stage", Path(__file__).parents[1] / "scripts/vm-xcode-stage.py")
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


class Staging(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.source = self.base / "Selected Xcode.app"
        self.root = self.base / "MeguriXcode"
        self.root.mkdir(mode=0o700)
        (self.source / "Contents/Developer/usr/bin").mkdir(parents=True)
        for name, value in (
                ("Info.plist", {"CFBundleIdentifier": "com.apple.dt.Xcode", "CFBundleShortVersionString": "27.2"}),
                ("version.plist", {"CFBundleShortVersionString": "27.2", "ProductBuildVersion": "18C123"})):
            (self.source / "Contents" / name).write_bytes(plistlib.dumps(value))
        self.payload = self.source / "Contents/Developer/usr/bin/xcodebuild"
        self.payload.write_bytes(b"generated executable content\x00")
        self.payload.chmod(0o755)
        (self.source / "Contents/Developer/current").symlink_to("usr")
        os.link(self.payload, self.source / "Contents/Developer/usr/bin/linked")
        self.commands, self.ready, self.sudo, self.selection, self.version = [], 0, 0, "old", "18C123"
        self.license_ready = 0
        self.identity = ["generated-uuid", "generated-boot"]
        self.addCleanup(patch.stopall)
        patch.object(m, "command", side_effect=self.command).start()
        patch.object(m, "instance", side_effect=lambda: self.identity).start()
        patch.object(m, "guest_root", side_effect=lambda: m.Directory(str(self.root))).start()
        patch.object(m, "free_space").start()
        with m.Directory(str(self.source)) as d:
            original = m.inventory(d)
            self.expected = {"content": original["content"], "bytes": original["bytes"],
                             "entries": original["entries"], "facts": m.facts(d)}
        self.context = {"expected": self.expected, "instance": list(self.identity)}
        self.target = self.root / m.target_name(self.expected["facts"])

    def command(self, argv, **kwargs):
        self.commands.append((argv, kwargs))
        if argv[0] == "/usr/bin/codesign":
            return 0, b""
        if "-checkFirstLaunchStatus" in argv:
            return self.ready, b""
        if "-license" in argv:
            return self.license_ready, b""
        if argv[:2] == ["/usr/bin/sudo", "-n"]:
            if self.sudo == 0:
                self.selection = argv[-1]
            return self.sudo, b""
        if argv == ["/usr/bin/xcode-select", "-p"]:
            return 0, self.selection.encode()
        if argv == ["/usr/bin/xcodebuild", "-version"]:
            return 0, ("Xcode 27.2\nBuild version " + self.version + "\n").encode()
        if argv == ["/bin/ps", "-A", "-o", "pid=", "-o", "ppid="]:
            return 0, (str(os.getpid()) + " " + str(os.getppid()) + "\n").encode()
        self.fail("unexpected command: " + repr(argv))

    def prepare(self):
        self.context["state"] = m.guest("prepare", self.context)
        return self.context["state"]

    def stage_path(self):
        return self.root / self.context["state"]["work"] / "Xcode.app"

    def receive(self):
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w") as archive:
            archive.add(self.source, arcname=".", recursive=True)
        raw.seek(0)
        def supervised(bundle, work, context):
            m.receive(bundle, context["expected"])
        with patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), \
                patch.object(m, "run_receiver_owner", side_effect=supervised), \
                patch.object(os, "fstatvfs", return_value=types.SimpleNamespace(f_bavail=100 * m.GIB, f_frsize=1)):
            m.guest("receive", self.context)

    def staged(self):
        self.prepare()
        self.receive()

    def test_complete_copy_links_signature_and_actual_selection(self):
        self.staged()
        result = m.guest("publish", self.context)
        self.assertEqual(result, {"outcome": "activated", "match": True})
        self.assertEqual(os.readlink(self.target / "Contents/Developer/current"), "usr")
        a, b = [os.stat(self.target / "Contents/Developer/usr/bin" / name) for name in ("xcodebuild", "linked")]
        self.assertEqual(a.st_ino, b.st_ino)
        with m.Directory(str(self.target)) as d:
            self.assertEqual(m.inventory(d)["content"], self.expected["content"])
        self.assertTrue(any("anchor apple" in argv for argv, _ in self.commands))
        self.assertEqual(self.selection, str(self.target / "Contents/Developer"))

    def test_existing_identical_bundle_is_reused_without_copy(self):
        self.staged()
        m.guest("publish", self.context)
        state = self.prepare()
        self.assertTrue(state["existing"])
        before = set(self.root.iterdir())
        self.assertEqual(m.guest("publish", self.context)["outcome"], "already-selected")
        self.assertEqual(set(self.root.iterdir()), before)

    def test_activation_completion_is_required_after_successful_receiver_join(self):
        # The guest is a generated local process, with fake Apple command
        # results. It keeps activating after the simulated SSH caller exits.
        self.staged()
        for mode in ("cancelled", "lost-ssh", "completed"):
            with self.subTest(mode=mode):
                private = self.base / mode
                private.mkdir(mode=0o700)
                ready, finish = private / "activating", private / "finish"
                driver = private / "guest.py"
                driver.write_text(
                    "import importlib.util,json,time\n"
                    "from pathlib import Path\n"
                    "s=importlib.util.spec_from_file_location('stage'," + repr(SPEC.origin) + ")\n"
                    "m=importlib.util.module_from_spec(s);s.loader.exec_module(m)\n"
                    "m.instance=lambda:" + repr(self.identity) + "\n"
                    "m.guest_root=lambda:m.Directory(" + repr(str(self.root)) + ")\n"
                    "selected='old'\n"
                    "def command(argv,**kwargs):\n"
                    " global selected\n"
                    " if argv[:2]==['/usr/bin/sudo','-n']:\n"
                    "  Path(" + repr(str(ready)) + ").touch()\n"
                    "  deadline=time.monotonic()+10\n"
                    "  while not Path(" + repr(str(finish)) + ").exists():\n"
                    "   assert time.monotonic()<deadline\n"
                    "   time.sleep(0.01)\n"
                    "  selected=argv[-1]\n"
                    " if argv==['/usr/bin/xcode-select','-p']: return 0,selected.encode()\n"
                    " if argv==['/usr/bin/xcodebuild','-version']: return 0,b'Xcode 27.2\\nBuild version 18C123\\n'\n"
                    " return 0,b''\n"
                    "m.command=command\n"
                    "print(json.dumps(m.guest('publish'," + repr(self.context) + ")))\n")
                child = subprocess.Popen([sys.executable, "-B", str(driver)],
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    deadline = time.monotonic() + 5
                    while not ready.exists():
                        self.assertIsNone(child.poll())
                        self.assertLess(time.monotonic(), deadline)
                        time.sleep(0.01)
                    with m.Directory(str(private)) as directory:
                        with m.release_proof(directory) as safety:
                            m.write_new(directory, "receiver-joined.json", {"joined": True})
                            safety.update(pending=True, joined=True)
                            vm = m.VM.__new__(m.VM)
                            vm.safety, vm.ssh, vm.check = safety, "/generated/ssh", lambda: None
                            vm.guest_command = lambda *args: "generated-publish"

                            def transport(argv, **kwargs):
                                self.assertEqual(argv, [vm.ssh, "generated-publish"])
                                self.assertEqual(kwargs["timeout"], 180)
                                self.assertTrue(safety["pending"])
                                self.assertFalse(safety["joined"])
                                self.assertFalse((private / "lease-safe.json").exists())
                                if mode == "cancelled":
                                    raise m.Cancelled()
                                if mode == "lost-ssh":
                                    raise m.Refused()
                                finish.touch()
                                out, err = child.communicate(timeout=5)
                                self.assertEqual((child.returncode, err), (0, b""))
                                return 0, out

                            with patch.object(m, "command", side_effect=transport):
                                if mode == "completed":
                                    self.assertEqual(vm.call("publish", self.context),
                                                     {"outcome": "activated", "match": True})
                                    self.assertTrue(safety["joined"])
                                else:
                                    with self.assertRaises(m.Cancelled if mode == "cancelled" else m.Refused):
                                        vm.call("publish", self.context)
                                    self.assertIsNone(child.poll())
                                    self.assertTrue(safety["uncertain"])
                            if mode != "completed":
                                # A later successful probe cannot join the
                                # original, still-active publication operation.
                                with patch.object(m, "command", return_value=(0, b"probe")):
                                    vm.call_command("generated-probe")
                        self.assertEqual((private / "lease-safe.json").exists(), mode == "completed")
                finally:
                    finish.touch()
                    out, err = child.communicate(timeout=5)
                    self.assertEqual((child.returncode, err), (0, b""))
                # Subsequent modes reuse the verified, already-published bundle.
                self.prepare()

    def test_partial_or_foreign_target_is_not_overwritten(self):
        self.target.mkdir()
        marker = self.target / "foreign"
        marker.write_text("keep")
        with self.assertRaises(m.Refused):
            self.prepare()
        self.assertEqual(marker.read_text(), "keep")

    def test_changed_content_and_link_are_rejected_before_publication(self):
        for change in ("content", "link"):
            with self.subTest(change=change):
                self.staged()
                stage = self.stage_path()
                if change == "content":
                    (stage / "Contents/Developer/usr/bin/xcodebuild").write_bytes(b"changed")
                else:
                    link = stage / "Contents/Developer/current"
                    link.unlink()
                    link.symlink_to("usr/bin")
                with self.assertRaises(m.Refused):
                    m.guest("publish", self.context)
                self.assertFalse(self.target.exists())

    def test_original_directory_substitution_and_source_change(self):
        with m.Directory(str(self.source)) as d:
            before = m.inventory(d)
            self.payload.write_bytes(b"different source")
            self.assertNotEqual(m.inventory(d), before)
            saved = self.source.with_name("saved.app")
            self.source.rename(saved)
            self.source.mkdir()
            with self.assertRaises(m.Refused):
                d.check()

    def host_refuses_copy_change(self, change):
        lease = self.base / "lease"
        lease.mkdir(mode=0o700)
        holder = lease / "holder.json"
        holder.write_text(json.dumps({"job": "vm-xcode-stage", "run_id": "generated-run", "pid": os.getpid(),
                                      "worktree": str(self.base)}))
        holder.chmod(0o600)
        self.selection = str(self.source / "Contents/Developer")
        events = []

        class FakeVM:
            identity = ["generated", "boot"]

            def __init__(self, *args):
                pass

            def call(self, operation, context):
                events.append(operation)
                return {"existing": False}

            def copy(self, *args, **kwargs):
                events.append("copy")
                change(holder)

            def close(self):
                events.append("close")

        with patch.object(m, "VM", FakeVM), self.assertRaises(m.Refused):
            m.host({"selection": {}, "lease_slot": str(lease), "root": str(self.base),
                    "run": "generated-run", "private": str(self.base)})
        self.assertEqual(events, ["prepare", "copy", "close"])

    def test_original_source_content_change_during_copy_blocks_publication(self):
        self.host_refuses_copy_change(lambda _: self.payload.write_bytes(b"source changed during copy"))

    def test_original_source_selection_change_during_copy_blocks_publication(self):
        def change(_):
            self.selection = str(self.base / "Other.app/Contents/Developer")
        self.host_refuses_copy_change(change)

    def test_original_lease_change_during_copy_blocks_publication(self):
        self.host_refuses_copy_change(lambda holder: holder.write_text("{}"))

    def test_external_and_chained_escaping_links_are_not_followed(self):
        link = self.source / "Contents/Developer/current"
        link.unlink()
        link.symlink_to("../../../../private")
        with m.Directory(str(self.source)) as d, self.assertRaises(m.Refused):
            m.inventory(d)

    def test_archive_cannot_traverse_or_replace_links(self):
        for name, symlink in (("../escape", False), ("/absolute", False), ("escape", True)):
            self.prepare()
            raw = io.BytesIO()
            with tarfile.open(fileobj=raw, mode="w") as archive:
                item = tarfile.TarInfo(name)
                item.mode = 0o644
                if symlink:
                    item.type, item.linkname = tarfile.SYMTYPE, "../../outside"
                archive.addfile(item)
            raw.seek(0)
            with patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), self.assertRaises(m.Refused):
                with m.Directory(str(self.stage_path())) as bundle:
                    m.receive(bundle, self.expected)
            self.assertFalse((self.base / "escape").exists())

    def test_insufficient_space_fails_before_large_copy(self):
        patch.stopall()
        for free in (40 * m.GIB, 40 * m.GIB - 1, 2 * m.GIB):
            with m.Directory(str(self.root)) as directory, patch.object(os, "fstatvfs", return_value=types.SimpleNamespace(
                    f_bavail=free, f_frsize=1)), self.assertRaises(m.Refused):
                m.free_space(directory)
        with m.Directory(str(self.root)) as directory, patch.object(os, "fstatvfs", return_value=types.SimpleNamespace(
                f_bavail=42 * m.GIB, f_frsize=1)):
            m.free_space(directory)
            with self.assertRaises(m.Refused):
                m.free_space(directory, 41 * m.GIB)

    def test_post_copy_reserve_includes_payload_and_staging_overhead(self):
        patch.stopall()
        for free in (45 * m.GIB, 61 * m.GIB, 61 * m.GIB + 8192):
            with self.subTest(free=free), m.Directory(str(self.root)) as directory, \
                    patch.object(os, "fstatvfs", return_value=types.SimpleNamespace(f_bavail=free, f_frsize=1)), \
                    self.assertRaises(m.Refused):
                m.free_space(directory, 20 * m.GIB, 1)
        with m.Directory(str(self.root)) as directory, patch.object(os, "fstatvfs", return_value=types.SimpleNamespace(
                f_bavail=61 * m.GIB + 8193, f_frsize=1)):
            m.free_space(directory, 20 * m.GIB, 1)

    def test_receiver_refuses_oversize_before_writing_unverified_payload(self):
        self.prepare()
        for count, size in ((1, self.expected["bytes"] + 1), (self.expected["entries"] + 1, 0)):
            raw = io.BytesIO()
            with tarfile.open(fileobj=raw, mode="w") as archive:
                for index in range(count):
                    item = tarfile.TarInfo("generated-" + str(index))
                    item.mode, item.size = 0o600, size
                    archive.addfile(item, io.BytesIO(b"x" * size))
            raw.seek(0)
            # Each refusal retains only this run's bounded partial output.
            fresh = self.base / ("bounded-" + str(count))
            fresh.mkdir(mode=0o700)
            with m.Directory(str(fresh)) as bundle, patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), \
                    self.assertRaises(m.Refused):
                m.receive(bundle, self.expected)
            self.assertLessEqual(len(list(fresh.iterdir())), self.expected["entries"])
            self.assertLessEqual(sum(p.stat().st_size for p in fresh.iterdir()), self.expected["bytes"])
            if size:
                self.assertEqual(list(fresh.iterdir()), [])

    def test_no_replace_publication_preserves_concurrent_target(self):
        self.staged()
        self.target.mkdir()
        (self.target / "foreign").write_bytes(b"preserve")
        with self.assertRaises(OSError):
            m.guest("publish", self.context)
        self.assertTrue((self.target / "foreign").exists())
        self.assertTrue(self.stage_path().is_dir())
        self.assertFalse(any(a[0] == "/usr/bin/sudo" for a, _ in self.commands))

    def test_readiness_and_license_failure_never_attempt_activation(self):
        self.staged()
        self.ready = 1
        self.assertEqual(m.guest("publish", self.context), {"outcome": "staged", "match": False})
        self.assertTrue(self.target.exists())
        self.assertFalse(any(a[0] == "/usr/bin/sudo" for a, _ in self.commands))

    def test_pending_license_after_first_launch_check_never_attempts_activation(self):
        self.staged()
        self.license_ready = 1
        self.assertEqual(m.guest("publish", self.context), {"outcome": "staged", "match": False})
        self.assertTrue(any("-checkFirstLaunchStatus" in a for a, _ in self.commands))
        self.assertTrue(any("-license" in a for a, _ in self.commands))
        self.assertFalse(any(a[0] == "/usr/bin/sudo" for a, _ in self.commands))

    def test_failed_sudo_stays_staged_and_never_claims_selection(self):
        self.staged()
        self.sudo = 1
        self.assertEqual(m.guest("publish", self.context), {"outcome": "staged", "match": False})
        self.assertEqual(self.selection, "old")
        self.assertTrue(self.target.exists())
        self.assertFalse(any(a == ["/usr/bin/xcodebuild", "-version"] for a, _ in self.commands))

    def test_actual_mismatched_selected_build_is_refused(self):
        self.staged()
        self.version = "18B456"
        with self.assertRaises(m.Refused):
            m.guest("publish", self.context)
        self.assertTrue(self.target.exists())

    def test_sudo_success_with_wrong_actual_selected_path_is_refused(self):
        self.staged()

        def mismatch(argv, **kwargs):
            if argv == ["/usr/bin/xcode-select", "-p"] and any(a[0] == "/usr/bin/sudo" for a, _ in self.commands):
                return 0, b"/different/Xcode.app/Contents/Developer"
            return self.command(argv, **kwargs)

        with patch.object(m, "command", side_effect=mismatch), self.assertRaises(m.Refused):
            m.guest("publish", self.context)
        self.assertFalse(any(a == ["/usr/bin/xcodebuild", "-version"] for a, _ in self.commands))

    def test_changed_vm_instance_never_receives_or_publishes(self):
        self.prepare()
        self.identity = ["replacement-vm", "other-boot"]
        with self.assertRaises(m.Refused):
            self.receive()
        self.assertFalse(self.target.exists())

    def test_no_license_accept_account_or_other_data_mutations(self):
        self.staged()
        m.guest("publish", self.context)
        allowed = {"/usr/bin/codesign", "/usr/bin/xcode-select", "/usr/bin/xcodebuild", "/usr/bin/sudo",
                   str(self.target / "Contents/Developer/usr/bin/xcodebuild")}
        for argv, _ in self.commands:
            self.assertIn(argv[0], allowed)
            self.assertNotIn("accept", argv)
            self.assertNotIn("-runFirstLaunch", argv)
        self.assertEqual(set(self.base.iterdir()), {self.source, self.root})


class Transport(unittest.TestCase):
    def test_linked_private_candidates_are_rejected_before_selection_or_vm(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp).resolve()
            physical = base / "physical"
            physical.mkdir(mode=0o700)
            private = physical / "private"
            private.mkdir(mode=0o700)
            alias = base / "logical"
            alias.symlink_to(physical, target_is_directory=True)
            linked = base / "linked-private"
            linked.symlink_to(private, target_is_directory=True)
            for candidate in (alias / "private", linked):
                with self.subTest(candidate=candidate), patch.object(m, "command") as command, \
                        patch.object(m, "VM") as vm, self.assertRaises((m.Refused, OSError)):
                    m.host({"selection": {}, "lease_slot": str(base / "lease"), "root": str(base),
                            "run": "generated-run", "private": str(candidate)})
                command.assert_not_called()
                vm.assert_not_called()
                self.assertEqual(list(private.iterdir()), [])

    def test_error_diagnostics_are_private_fixed_classes_and_own_frames_only(self):
        sensitive = "generated-private-message /generated/path route command input"

        class SensitiveName(m.Refused):
            pass

        def fail():
            try:
                m.need(False)
            except m.Refused as error:
                error.args = (sensitive,)
                raise error from ValueError(sensitive)

        for failure, expected, functions in (
                (fail, "Refused", ["run", "need"]),
                (SensitiveName(sensitive), "Refused", ["run"]),
                (NotADirectoryError(sensitive), "NotADirectoryError", ["run"])):
            for guest in (False, True):
                public, private = io.StringIO(), io.StringIO()
                with self.subTest(expected=expected, guest=guest), \
                        patch.object(m, "main", side_effect=failure), \
                        patch.object(sys, "argv", ["stage"] + (["--guest"] if guest else [])), \
                        contextlib.redirect_stdout(public), contextlib.redirect_stderr(private):
                    self.assertEqual(m.run(), 1)
                if guest:
                    self.assertEqual((public.getvalue(), private.getvalue()), ("", ""))
                    continue
                self.assertEqual(json.loads(public.getvalue()), m.report("refused"))
                diagnostic = json.loads(private.getvalue())
                self.assertEqual(set(diagnostic), {"exception", "stack"})
                self.assertEqual(diagnostic["exception"], expected)
                self.assertEqual([frame["function"] for frame in diagnostic["stack"]], functions)
                for frame in diagnostic["stack"]:
                    self.assertEqual(set(frame), {"function", "line"})
                    self.assertIs(type(frame["line"]), int)
                    self.assertGreater(frame["line"], 0)
                self.assertNotIn(sensitive, public.getvalue() + private.getvalue())
                self.assertNotIn("SensitiveName", public.getvalue() + private.getvalue())

    def test_control_completion_keeps_pending_receiver(self):
        with tempfile.TemporaryDirectory() as temp, m.Directory(str(Path(temp).resolve())) as directory:
            with m.release_proof(directory) as safety:
                safety.update(pending=True, joined=False)
                vm = m.VM.__new__(m.VM)
                vm.safety, vm.ssh, vm.check = safety, "/generated/ssh", lambda: None
                with patch.object(m, "command", return_value=(0, b"completed")):
                    vm.call_command("generated-control")
                self.assertTrue(safety["pending"])
                self.assertFalse(safety["joined"])
            self.assertFalse((Path(temp) / "lease-safe.json").exists())

    def test_every_guest_control_call_fences_prior_receiver_proof(self):
        for operation in ("identity", "prepare", "publish", "join-receiver"):
            for mode in ("completed", "cancelled", "lost-ssh"):
                with self.subTest(operation=operation, mode=mode), tempfile.TemporaryDirectory() as temp, \
                        m.Directory(str(Path(temp).resolve())) as directory:
                    with m.release_proof(directory) as safety:
                        safety.update(pending=True, joined=True)
                        vm = m.VM.__new__(m.VM)
                        vm.safety, vm.ssh, vm.check = safety, "/generated/ssh", lambda: None
                        vm.guest_command = lambda name, context: name

                        def transport(argv, **kwargs):
                            self.assertTrue(safety["pending"])
                            self.assertFalse(safety["joined"])
                            if mode != "completed":
                                raise m.Cancelled() if mode == "cancelled" else m.Refused()
                            return 0, b'{}'

                        with patch.object(m, "command", side_effect=transport):
                            try:
                                if operation == "identity":
                                    vm.call_command("generated-identity")
                                else:
                                    vm.call(operation, {})
                            except (m.Cancelled, m.Refused):
                                self.assertNotEqual(mode, "completed")
                        self.assertEqual(safety["uncertain"], mode != "completed")
                    self.assertEqual((Path(temp) / "lease-safe.json").exists(), mode == "completed")

    def test_actual_mvl_tar_transport_uses_pinned_source_and_safe_receiver(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            source = root / "Source Xcode.app"
            source.mkdir()
            (source / "payload").write_bytes(b"generated data\x00")
            (source / "alias").symlink_to("payload")
            dest = root / "applications/work/Xcode.app"
            dest.mkdir(parents=True, mode=0o700)
            script = root / "private-ssh"
            receiver = (
                "import importlib.util,sys;"
                "assert sys.argv[1:]==['receiver-token'];"
                "s=importlib.util.spec_from_file_location('stage'," + repr(str(SPEC.origin)) + ");"
                "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);"
                "d=m.Directory(" + repr(str(dest)) + ");m.receive(d,{'entries':2,'bytes':15});d.close()")
            script.write_text("#!/bin/bash\nexec " + shlex.join([sys.executable, "-B", "-c", receiver]) + ' "$@"\n')
            script.chmod(0o700)
            vm = m.VM.__new__(m.VM)
            vm.ssh, vm.check, vm.guest_command = str(script), lambda: None, lambda *args: "receiver-token"
            vm.call = lambda *args: {"joined": True, "token": "generated", "returncode": 0}
            with m.Directory(str(source)) as bundle:
                vm.copy(bundle, {"state": {"root": str(root / "applications"), "work": "work", "receiver": "generated"}})
                with m.Directory(str(dest)) as copied:
                    self.assertEqual(m.inventory(bundle)["content"], m.inventory(copied)["content"])
            self.assertEqual(os.readlink(dest / "alias"), "payload")

    def test_cancellation_joins_original_group_without_unbounded_pipe_communication(self):
        events = []

        class Child:
            returncode = None
            pid, stdin = 12345, None

            def communicate(self, *args, **kwargs):
                events.append("communicate")
                raise m.Cancelled()

            def wait(self, timeout):
                self_test.assertEqual(timeout, m.SHUTDOWN)
                events.append("wait")
                if self.returncode is None:
                    raise subprocess.TimeoutExpired("generated", timeout)
                return self.returncode

        self_test = self
        def killpg(pid, sig):
            self.assertEqual(pid, child.pid)
            events.append(sig)
            if sig == signal.SIGKILL:
                child.returncode = -sig

        child = Child()
        with patch.object(subprocess, "Popen", return_value=child) as start, \
                patch.object(os, "killpg", side_effect=killpg), self.assertRaises(m.Cancelled):
            m.command(["generated-transport"])
        self.assertEqual(start.call_count, 1)
        self.assertTrue(start.call_args.kwargs["start_new_session"])
        self.assertNotEqual(start.call_args.kwargs["stdout"], subprocess.PIPE)
        self.assertEqual(events, ["communicate", signal.SIGTERM, "wait", signal.SIGKILL, "wait"])

    def test_live_descendant_holding_pipes_is_stopped_on_hangup(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ready = root / "ready"
            fixture = root / "descendant.py"
            fixture.write_text("""
import os, signal, sys
from pathlib import Path
if os.fork() == 0:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    os.write(int(sys.argv[2]), b'live')
    Path(sys.argv[1]).touch()
    while True: signal.pause()
os.wait()
""")
            reader, writer = os.pipe()
            self.addCleanup(os.close, reader)
            driver = root / "driver.py"
            driver.write_text(
                "import importlib.util,signal,sys\n"
                "s=importlib.util.spec_from_file_location('stage'," + repr(SPEC.origin) + ")\n"
                "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);m.SHUTDOWN=0.2\n"
                "def cancel(*args): raise m.Cancelled()\n"
                "signal.signal(signal.SIGHUP,cancel)\n"
                "try: m.command([sys.executable," + repr(str(fixture)) + "," + repr(str(ready)) + ",sys.argv[1]],pass_fds=(int(sys.argv[1]),))\n"
                "except m.Cancelled: sys.exit(130)\n")
            child = subprocess.Popen([sys.executable, "-B", str(driver), str(writer)], pass_fds=(writer,))
            os.close(writer)
            try:
                deadline = time.monotonic() + 5
                while not ready.exists():
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.01)
                # A real descendant retains this pipe AND the command's output
                # descriptors after the immediate child dies on SIGTERM.
                self.assertEqual(os.read(reader, 4), b"live")
                child.send_signal(signal.SIGHUP)
                self.assertEqual(child.wait(timeout=5), 130)
                os.set_blocking(reader, False)
                self.assertTrue(select.select([reader], [], [], 5)[0])
                self.assertEqual(os.read(reader, 1), b"")
            finally:
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=5)

    def test_still_running_guest_receiver_is_joined_before_release_callback(self):
        # Generated local processes stand for the SSH foreground supervisor and
        # its original extraction child; no real VM, SDK or account is used.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            work_path = root / "work"
            bundle_path = work_path / "Xcode.app"
            bundle_path.mkdir(parents=True, mode=0o700)
            ready = root / "live-receiver"
            with m.Directory(str(work_path)) as work, m.Directory(str(bundle_path)) as bundle:
                context = {"state": {"root": str(root), "work": "work", "receiver": "generated-original",
                                     "work_pins": work.check(), "bundle_pins": bundle.check()},
                           "instance": ["generated", "boot"]}
                fixture = root / "receiver.py"
                fixture.write_text("import signal\nfrom pathlib import Path\n"
                                   "signal.signal(signal.SIGTERM,signal.SIG_IGN)\n"
                                   "Path(" + repr(str(ready)) + ").touch()\n"
                                   "while True: signal.pause()\n")
                supervisor = root / "supervisor.py"
                supervisor.write_text(
                    "import importlib.util,sys\n"
                    "s=importlib.util.spec_from_file_location('stage'," + repr(SPEC.origin) + ")\n"
                    "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);m.SHUTDOWN=0.2\n"
                    "m.STAGING_SOURCE='generated'\n"
                    "m.guest_argv=lambda *args:[sys.executable,'-B'," + repr(str(fixture)) + "]\n"
                    "with m.Directory(" + repr(str(work_path)) + ") as work,m.Directory(" + repr(str(bundle_path)) + ") as bundle:\n"
                    " try: m.run_receiver_owner(bundle,work," + repr(context) + ")\n"
                    " except m.Refused: pass\n")
                child = subprocess.Popen([sys.executable, "-B", str(supervisor)])
                try:
                    deadline = time.monotonic() + 5
                    while not ready.exists():
                        self.assertLess(time.monotonic(), deadline)
                        time.sleep(0.01)
                    self.assertIsNone(child.poll())
                    self.assertFalse((work_path / "receiver-joined.json").exists())
                    events = []
                    vm = m.VM.__new__(m.VM)
                    vm.check = lambda: None
                    vm.ssh = "/generated/ssh"
                    vm.guest_command = lambda *args: "generated-receiver"
                    def join(operation, private):
                        self.assertEqual(operation, "join-receiver")
                        events.append("stop-original")
                        result = m.join_receiver(work, private)
                        self.assertEqual(result["returncode"], -signal.SIGKILL)
                        events.append("original-joined")
                        return result
                    vm.call = join
                    with patch.object(m, "command", side_effect=m.Cancelled), \
                            patch.object(m, "instance", return_value=context["instance"]), self.assertRaises(m.Cancelled):
                        vm.copy(bundle, context, on_join=lambda: events.append("release-permitted"))
                    child.wait(timeout=5)
                    self.assertEqual(events, ["stop-original", "original-joined", "release-permitted"])
                finally:
                    if child.poll() is None:
                        # Stop only this generated supervisor's original child.
                        if not (work_path / "receiver-stop.json").exists():
                            m.write_new(work, "receiver-stop.json", {"token": context["state"]["receiver"],
                                                                     "instance": context["instance"]})
                    child.wait(timeout=5)

    def test_missing_guest_join_proof_never_permits_release(self):
        vm = m.VM.__new__(m.VM)
        vm.check, vm.ssh, vm.guest_command = lambda: None, "/generated/ssh", lambda *args: "generated"
        vm.call = lambda *args: (_ for _ in ()).throw(m.Refused())
        released = []
        bundle = types.SimpleNamespace(fd=123)
        with patch.object(m, "command", side_effect=m.Cancelled), self.assertRaises(m.Refused):
            vm.copy(bundle, {"state": {"root": "/generated", "work": "work"}}, on_join=lambda: released.append(True))
        self.assertEqual(released, [])

    def test_unconfirmed_local_group_join_stays_uncertain_with_bounded_waits(self):
        child = types.SimpleNamespace(pid=12345)
        from unittest.mock import Mock
        child.wait = Mock(side_effect=subprocess.TimeoutExpired("generated", m.SHUTDOWN))
        with patch.object(os, "killpg") as kill, self.assertRaises(m.Uncertain):
            m.join_original(child)
        self.assertEqual([call.args for call in kill.call_args_list],
                         [(child.pid, signal.SIGTERM), (child.pid, signal.SIGKILL)])
        self.assertEqual([call.kwargs for call in child.wait.call_args_list],
                         [{"timeout": m.SHUTDOWN}, {"timeout": m.SHUTDOWN}])

    def test_release_proof_requires_confirmed_original_join_and_retains_guard(self):
        for mode in ("no-copy", "pending", "joined", "uncertain"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp, \
                    m.Directory(str(Path(temp).resolve())) as directory:
                try:
                    with m.release_proof(directory) as state:
                        state["pending"] = mode in ("pending", "joined")
                        state["joined"] = mode == "joined"
                        if mode == "uncertain":
                            raise m.Uncertain()
                except m.Uncertain:
                    pass
                self.assertEqual((Path(temp) / "lease-safe.json").exists(), mode in ("no-copy", "joined"))

    def test_public_report_contains_no_paths_routes_or_success_override(self):
        result = m.report("staged", {"version": "27.2", "build": "18C123"}, selection_matches=False)
        self.assertEqual(set(result), {"scope", "outcome", "version", "build", "checks"})
        self.assertEqual(result["scope"], "VM-XCODE-MAINTENANCE-ONLY")
        self.assertFalse(result["checks"]["selection_matches"])


if __name__ == "__main__":
    unittest.main()
