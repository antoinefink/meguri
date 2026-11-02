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


@contextlib.contextmanager
def metadata_view(overrides):
    """Generated ownership evidence; never chown or access installed bundles."""
    actual_stat, actual_fstat = os.stat, os.fstat

    def view(s):
        values = overrides.get((s.st_dev, s.st_ino))
        if values is None:
            return s
        fields = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_size",
                  "st_mtime_ns", "st_ctime_ns", "st_nlink")
        result = {name: getattr(s, name) for name in fields}
        result.update(values)
        return types.SimpleNamespace(**result)

    with patch.object(os, "stat", side_effect=lambda *a, **kw: view(actual_stat(*a, **kw))), \
            patch.object(os, "fstat", side_effect=lambda fd: view(actual_fstat(fd))):
        yield


class SourceAuthority(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.source = Path(self.temp.name).resolve() / "Generated.app"
        self.source.mkdir(mode=0o755)
        self.directory = self.source / "Contents"
        self.directory.mkdir(mode=0o775)
        self.directory.chmod(0o775)
        self.file = self.directory / "payload"
        self.file.write_bytes(b"generated")
        self.file.chmod(0o664)
        self.executable = self.directory / "executable"
        self.executable.write_bytes(b"generated executable")
        self.executable.chmod(0o775)
        self.overrides = {}
        for path in (self.source, self.directory, self.file, self.executable):
            s = path.stat()
            self.overrides[(s.st_dev, s.st_ino)] = {"st_uid": 0, "st_gid": 0}
        self.root_user = m.pwd.struct_passwd(("root", "*", 0, 0, "root", "/var/root", "/bin/sh"))
        self.wheel = m.grp.struct_group(("wheel", "*", 0, ["root"]))
        self.addCleanup(patch.stopall)
        patch.object(m.sys, "platform", "darwin").start()
        self.by_name = patch.object(m.grp, "getgrnam", side_effect=lambda name: self.wheel).start()
        self.by_gid = patch.object(m.grp, "getgrgid", side_effect=lambda gid: self.wheel).start()
        patch.object(m.pwd, "getpwnam", side_effect=lambda name: self.root_user).start()
        patch.object(m.pwd, "getpwuid", side_effect=lambda uid: self.root_user).start()

    def test_root_system_group_modes_are_admitted_with_exact_content_and_pins(self):
        with metadata_view(self.overrides), m.Directory(str(self.source)) as directory:
            authority = m.InstalledSource(directory)
            original = m.inventory(directory, installed_source=authority)
            self.assertIsNotNone(authority.identity)
            self.assertEqual(original["entries"], 3)
            self.assertEqual(original["bytes"], len(b"generatedgenerated executable"))
            self.assertEqual(m.inventory(directory, installed_source=authority), original)
            self.file.write_bytes(b"different")
            self.assertNotEqual(m.inventory(directory, installed_source=authority), original)
        self.by_name.assert_called_with("wheel")
        self.by_gid.assert_called_with(0)
        self.assertEqual(self.directory.stat().st_mode & 0o7777, 0o775)
        self.assertEqual(self.file.stat().st_mode & 0o7777, 0o664)
        self.assertEqual(self.executable.stat().st_mode & 0o7777, 0o775)

    def test_untrusted_entry_group_owner_and_bundle_owner_are_refused(self):
        for path, field in ((self.file, "st_gid"), (self.file, "st_uid"), (self.source, "st_uid")):
            s = path.stat()
            values = self.overrides[(s.st_dev, s.st_ino)]
            with self.subTest(path=path.name, field=field):
                values[field] = 12345
                try:
                    with metadata_view(self.overrides), self.assertRaises(m.Refused):
                        with m.Directory(str(self.source)) as directory:
                            m.inventory(directory, installed_source=m.InstalledSource(directory))
                finally:
                    values[field] = 0

    def test_group_name_number_members_and_bidirectional_lookup_are_verified(self):
        for group in (m.grp.struct_group(("wheel", "*", 20, ["root"])),
                      m.grp.struct_group(("other", "*", 0, ["root"]))):
            self.wheel = group
            with self.subTest(group=tuple(group)), metadata_view(self.overrides), \
                    m.Directory(str(self.source)) as directory, self.assertRaises(m.Refused):
                m.inventory(directory, installed_source=m.InstalledSource(directory))
        self.wheel = m.grp.struct_group(("wheel", "*", 0, ["root", "untrusted"]))
        foreign = m.pwd.struct_passwd(("untrusted", "*", 501, 20, "", "/generated", "/bin/sh"))
        with patch.object(m.pwd, "getpwnam", side_effect=lambda name: self.root_user if name == "root" else foreign), \
                metadata_view(self.overrides), m.Directory(str(self.source)) as directory, \
                self.assertRaises(m.Refused):
            m.inventory(directory, installed_source=m.InstalledSource(directory))
        self.wheel = m.grp.struct_group(("wheel", "*", 0, ["root"]))
        with patch.object(m.grp, "getgrgid", return_value=m.grp.struct_group(("other", "*", 0, []))), \
                metadata_view(self.overrides), m.Directory(str(self.source)) as directory, \
                self.assertRaises(m.Refused):
            m.inventory(directory, installed_source=m.InstalledSource(directory))

    def test_pinned_identity_and_source_owner_mode_changes_are_refused(self):
        with metadata_view(self.overrides), m.Directory(str(self.source)) as directory:
            authority = m.InstalledSource(directory)
            m.inventory(directory, installed_source=authority)
            self.wheel = m.grp.struct_group(("wheel", "*", 0, []))
            with self.assertRaises(m.Refused):
                m.inventory(directory, installed_source=authority)
        self.wheel = m.grp.struct_group(("wheel", "*", 0, ["root"]))
        with metadata_view(self.overrides), m.Directory(str(self.source)) as directory:
            authority = m.InstalledSource(directory)
            before = m.inventory(directory, installed_source=authority)
            self.file.chmod(0o644)
            self.assertNotEqual(m.inventory(directory, installed_source=authority), before)
            s = self.file.stat()
            self.overrides[(s.st_dev, s.st_ino)]["st_uid"] = 12345
            self.assertNotEqual(m.inventory(directory, installed_source=authority), before)

    def test_world_write_and_set_id_are_never_admitted(self):
        for mode in (0o666, 0o777, 0o4664, 0o2664):
            self.file.chmod(mode)
            with self.subTest(mode=oct(mode)), metadata_view(self.overrides), \
                    m.Directory(str(self.source)) as directory, self.assertRaises(m.Refused):
                m.inventory(directory, installed_source=m.InstalledSource(directory))

    def test_no_unscoped_or_boolean_authority(self):
        with metadata_view(self.overrides), m.Directory(str(self.source)) as directory:
            for kwargs in ({}, {"installed_source": True}, {"private_root": True},
                           {"installed_source": {"uid": 0, "gid": 0, "trusted": True}}):
                with self.subTest(kwargs=kwargs), self.assertRaises(m.Refused):
                    m.inventory(directory, **kwargs)

    def test_current_unprivileged_membership_is_not_os_source_authority(self):
        caller = os.getuid() or 501
        with patch.object(os, "getuid", return_value=caller), patch.object(os, "geteuid", return_value=caller), \
                patch.object(os, "getgid", return_value=20), patch.object(os, "getegid", return_value=20), \
                patch.object(os, "getgroups", return_value=[0, 20]), metadata_view(self.overrides), \
                m.Directory(str(self.source)) as directory, self.assertRaises(m.Refused):
            m.inventory(directory, installed_source=m.InstalledSource(directory))


class SourceOnly(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.source = self.base / "Selected Original.app"
        self.developer = self.source / "Contents/Developer"
        self.developer.mkdir(parents=True)
        self.private = self.base / "private"
        self.private.mkdir(mode=0o700)
        self.context = {"private": str(self.private)}
        for name, value in (
                ("Info.plist", {"CFBundleIdentifier": "com.apple.dt.Xcode", "CFBundleShortVersionString": "27.2"}),
                ("version.plist", {"CFBundleShortVersionString": "27.2", "ProductBuildVersion": "18C123"})):
            (self.source / "Contents" / name).write_bytes(plistlib.dumps(value))
        self.identity = ("generated root", "generated wheel", (501, 501, 20, 20, (20,)))
        self.commands = []
        self.real_command = m.command
        self.addCleanup(patch.stopall)
        actual_platform, actual_no_replace = sys.platform, m.no_replace

        def no_replace(src_dir, src, dst_dir, dst):
            # Simulate Darwin admission, but publish using the executing OS.
            with patch.object(m.sys, "platform", actual_platform):
                return actual_no_replace(src_dir, src, dst_dir, dst)

        patch.object(m.sys, "platform", "darwin").start()
        patch.object(m, "no_replace", side_effect=no_replace).start()
        patch.object(m.InstalledSource, "system_identity", return_value=self.identity).start()
        patch.object(m, "command", side_effect=self.command).start()
        for name in ("VM", "host", "guest", "instance", "inventory", "free_space", "guest_root",
                     "activation", "receive", "release_proof"):
            patch.object(m, name, side_effect=AssertionError("forbidden source-only operation: " + name)).start()

    def command(self, argv, **kwargs):
        self.commands.append((argv, kwargs))
        if argv == ["/usr/bin/xcode-select", "-p"]:
            return 0, str(self.developer).encode()
        self.assertEqual(argv, ["/usr/bin/codesign", "--verify", "--deep", "--strict",
                                "--all-architectures", "-R",
                                '=identifier "com.apple.dt.Xcode" and (anchor apple or (anchor apple generic and '
                                'certificate leaf[field.1.2.840.113635.100.6.1.9] exists))', str(self.source)])
        self.assertEqual(kwargs["source_diagnostics"].path, str(self.private))
        return 0, b""

    def check(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(m.check_source(self.context), 0)
        return json.loads(output.getvalue())

    def test_facts_emits_exact_xcode_identifier_and_apple_or_production_store_requirement(self):
        # Assert generated argv only; the mock does not evaluate signatures.
        with m.Directory(str(self.source)) as bundle, m.Directory(str(self.private)) as private:
            for diagnostics in (None, private):
                with self.subTest(private_diagnostics=diagnostics is not None), \
                        patch.object(m, "command", return_value=(0, b"")) as command:
                    self.assertEqual(m.facts(bundle, source_diagnostics=diagnostics),
                                     {"version": "27.2", "build": "18C123"})
                kwargs = {} if diagnostics is None else {"source_diagnostics": private}
                command.assert_called_once_with(
                    ["/usr/bin/codesign", "--verify", "--deep", "--strict", "--all-architectures", "-R",
                     '=identifier "com.apple.dt.Xcode" and (anchor apple or (anchor apple generic and '
                     'certificate leaf[field.1.2.840.113635.100.6.1.9] exists))', str(self.source)], **kwargs)

    def test_derived_original_source_authority_without_vm_inventory_or_source_writes(self):
        before = {str(p.relative_to(self.source)): (p.stat().st_mode, p.read_bytes() if p.is_file() else None)
                  for p in self.source.rglob("*")}
        with patch.dict(os.environ, {"DEVELOPER_DIR": "/generated/override", "SDKROOT": "override",
                                     "TOOLCHAINS": "override"}):
            public = self.check()
        self.assertEqual(public, {"scope": "SOURCE-CHECK-ONLY", "outcome": "source-checked",
                                 "checks": {"source_facts": True, "source_signature": True,
                                            "source_binding": True, "source_authority": True}})
        self.assertEqual(len(self.commands), 3)
        for argv, kwargs in (self.commands[0], self.commands[-1]):
            self.assertEqual(argv, ["/usr/bin/xcode-select", "-p"])
            self.assertFalse({"DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS"} & set(kwargs["env"]))
        binding = json.loads((self.private / "source-check.json").read_bytes())
        self.assertEqual(binding["selected"], str(self.developer))
        self.assertEqual(binding["bundle"], str(self.source))
        self.assertEqual(binding["authority"], json.loads(json.dumps(self.identity)))
        with m.Directory(str(self.source)) as original, m.Directory(str(self.developer)) as developer:
            self.assertEqual(binding["bundle_pins"], original.check())
            self.assertEqual(binding["developer_pins"], developer.check())
        self.assertEqual((self.private / "source-check.json").stat().st_mode & 0o777, 0o600)
        self.assertEqual(set(p.name for p in self.private.iterdir()), {"source-check.json"})
        self.assertEqual(before, {str(p.relative_to(self.source)): (p.stat().st_mode, p.read_bytes() if p.is_file() else None)
                                  for p in self.source.rglob("*")})
        self.assertNotIn(str(self.base), json.dumps(public))

    def test_original_binding_substitution_refuses(self):
        original = self.command
        def changed(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "/usr/bin/codesign":
                self.source.rename(self.base / "Saved.app")
                self.developer.mkdir(parents=True)
            return result
        with patch.object(m, "command", side_effect=changed), self.assertRaises(m.Refused):
            self.check()

    def test_changed_selection_refuses(self):
        def changed(argv, **kwargs):
            result = self.command(argv, **kwargs)
            if len(self.commands) == 3:
                return 0, str(self.base / "Other.app/Contents/Developer").encode()
            return result
        with patch.object(m, "command", side_effect=changed), self.assertRaises(m.Refused):
            self.check()

    def test_changed_authority_refuses(self):
        with patch.object(m.InstalledSource, "system_identity",
                          side_effect=[self.identity, self.identity, ("changed",)]), self.assertRaises(m.Refused):
            self.check()

    def test_no_caller_accepted_values_or_bundle_route_options(self):
        for key in ("facts", "expected", "selection", "bundle", "route", "authority", "signature", "verified"):
            with self.subTest(key=key), self.assertRaises(m.Refused):
                m.check_source(dict(self.context, **{key: True}))
        for args in (["--check-source", "--bundle", str(self.source)],
                     ["--check-source", "--route", "generated"], ["--source", str(self.source)]):
            with patch.object(sys, "argv", [str(SPEC.origin)] + args), \
                    patch.object(signal, "signal"), \
                    patch.object(os, "dup", side_effect=AssertionError("must reject before context")), \
                    self.assertRaises(m.Refused):
                m.main()
        self.assertEqual(self.commands, [])

    def test_failed_signature_keeps_original_private_streams_and_status(self):
        # Run the real command wrapper around one generated Popen handle only.
        original_command = m.command.side_effect
        raw_out, raw_err = b"generated private stdout\x00", b"generated nested requirement failure\n/private/identity"
        self_test = self
        class FailedChild:
            pid, stdin, returncode = 12345, None, None

            def __init__(self, argv, **kwargs):
                self_test.assertEqual(argv[-1], str(self_test.source))
                self_test.assertEqual(argv[1:-1], ["--verify", "--deep", "--strict",
                                                  "--all-architectures", "-R",
                                                  '=identifier "com.apple.dt.Xcode" and (anchor apple or '
                                                  '(anchor apple generic and certificate '
                                                  'leaf[field.1.2.840.113635.100.6.1.9] exists))'])
                kwargs["stdout"].write(raw_out)
                kwargs["stderr"].write(raw_err)

            def communicate(self, data, timeout):
                self.returncode = 23

            def wait(self, timeout):
                return self.returncode

        def dispatch(argv, **kwargs):
            if argv[0] == "/usr/bin/codesign":
                return self.real_command(argv, **kwargs)
            return original_command(argv, **kwargs)

        out, err = io.StringIO(), io.StringIO()
        with patch.object(m, "command", side_effect=dispatch), patch.object(subprocess, "Popen", FailedChild), \
                patch.object(os, "killpg"), patch.object(sys, "argv", [str(SPEC.origin), "--check-source"]), \
                patch.object(m, "main", side_effect=lambda: m.check_source(self.context)), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertEqual(m.run(), 1)
        self.assertEqual(json.loads(out.getvalue()), {"scope": "SOURCE-CHECK-ONLY", "outcome": "refused", "checks": {}})
        self.assertEqual((self.private / "source-command.stdout").read_bytes(), raw_out)
        self.assertEqual((self.private / "source-command.stderr").read_bytes(), raw_err)
        failed = json.loads((self.private / "source-command.json").read_bytes())
        self.assertEqual(failed["returncode"], 23)
        self.assertEqual(failed["argv"][-1], str(self.source))
        self.assertTrue((self.private / "source-check.json").is_file())
        for p in self.private.iterdir():
            self.assertEqual(p.stat().st_mode & 0o777, 0o600)
        for private in (str(self.base), raw_out.decode(), raw_err.decode(), "Selected Original.app"):
            self.assertNotIn(private, out.getvalue() + err.getvalue())
        self.assertNotIn("queued", out.getvalue())

    def test_failed_generated_child_overflow_retention_is_bounded_and_private(self):
        limit, exit_status = 1024 * 1024, 29
        markers = {"stdout": b"generated private stdout /private/output\x00",
                   "stderr": b"generated private stderr /private/error\n"}
        tails = {"stdout": b"private stdout overflow tail", "stderr": b"private stderr overflow tail"}
        original_command, original_popen = m.command.side_effect, subprocess.Popen
        children = []
        program = "import sys\n" + "".join(
            f"sys.{name}.buffer.write({markers[name]!r} + b'x' * {limit} + {tails[name]!r})\n"
            f"sys.{name}.buffer.flush()\n" for name in ("stdout", "stderr"))
        program += f"raise SystemExit({exit_status})\n"

        def generated_child(argv, **kwargs):
            self.assertEqual(argv, ["/usr/bin/codesign", "--verify", "--deep", "--strict",
                                    "--all-architectures", "-R",
                                    '=identifier "com.apple.dt.Xcode" and (anchor apple or (anchor apple generic and '
                                    'certificate leaf[field.1.2.840.113635.100.6.1.9] exists))', str(self.source)])
            child = original_popen([sys.executable, "-B", "-c", program], **kwargs)
            children.append(child)
            return child

        def dispatch(argv, **kwargs):
            if argv[0] == "/usr/bin/codesign":
                return self.real_command(argv, **kwargs)
            return original_command(argv, **kwargs)

        out, err = io.StringIO(), io.StringIO()
        with patch.object(m, "command", side_effect=dispatch), \
                patch.object(subprocess, "Popen", side_effect=generated_child), \
                patch.object(sys, "argv", [str(SPEC.origin), "--check-source"]), \
                patch.object(m, "main", side_effect=lambda: m.check_source(self.context)), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertEqual(m.run(), 1)
        self.assertEqual(len(children), 1)
        self.assertEqual(children[0].poll(), exit_status)
        self.assertEqual(json.loads(out.getvalue()),
                         {"scope": "SOURCE-CHECK-ONLY", "outcome": "refused", "checks": {}})
        failed = json.loads((self.private / "source-command.json").read_bytes())
        self.assertEqual(failed["returncode"], exit_status)
        self.assertEqual(failed["argv"][-1], str(self.source))
        for name in ("stdout", "stderr"):
            retained = self.private / ("source-command." + name)
            self.assertEqual(retained.stat().st_size, limit)
            self.assertEqual(retained.read_bytes(), markers[name] + b"x" * (limit - len(markers[name])))
            self.assertEqual(failed["streams"][name], {
                "original_bytes": len(markers[name]) + limit + len(tails[name]),
                "retained_bytes": limit, "overflow": True})
            for marker in (markers[name].decode(), tails[name].decode()):
                self.assertNotIn(marker, out.getvalue() + err.getvalue())
        self.assertEqual({p.name for p in self.private.iterdir()}, {
            "source-check.json", "source-command.json", "source-command.stdout", "source-command.stderr"})
        for p in self.private.iterdir():
            self.assertEqual(p.stat().st_mode & 0o777, 0o600)
        for private in (str(self.base), "Selected Original.app", "original_bytes", "retained_bytes", "overflow"):
            self.assertNotIn(private, out.getvalue() + err.getvalue())


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
        self.assertIn((["/usr/bin/codesign", "--verify", "--deep", "--strict", "--all-architectures", "-R",
                        '=identifier "com.apple.dt.Xcode" and (anchor apple or (anchor apple generic and '
                        'certificate leaf[field.1.2.840.113635.100.6.1.9] exists))', str(self.target)], {}), self.commands)
        self.assertEqual(self.selection, str(self.target / "Contents/Developer"))

    def test_existing_identical_bundle_is_reused_without_copy(self):
        self.staged()
        m.guest("publish", self.context)
        state = self.prepare()
        self.assertTrue(state["existing"])
        before = set(self.root.iterdir())
        self.assertEqual(m.guest("publish", self.context)["outcome"], "already-selected")
        self.assertEqual(set(self.root.iterdir()), before)

    def private_group_archive(self):
        # Generate a private current-user-owned input independently of the
        # installed-source authority, with the audited copied permission kinds.
        source = self.root / "generated-input.app"
        self.source.rename(source)
        self.source = source
        source.chmod(0o700)
        (source / "Contents").chmod(0o775)
        (source / "Contents/Info.plist").chmod(0o664)
        (source / "Contents/Developer/usr/bin/xcodebuild").chmod(0o775)
        with m.Directory(str(self.root)) as root, m.Directory(str(source)) as bundle:
            original = m.inventory(bundle, private_root=root)
        self.expected.update(content=original["content"], bytes=original["bytes"], entries=original["entries"])
        raw = io.BytesIO()

        def archive_identity(member):
            member.uid, member.gid = 12345, 54321
            member.uname, member.gname = "untrusted-archive-owner", "untrusted-archive-group"
            return member

        with tarfile.open(fileobj=raw, mode="w") as archive:
            archive.add(source, arcname=".", filter=archive_identity)
        raw.seek(0)
        return raw

    def test_private_copy_preserves_group_write_without_archive_ownership_and_reuses(self):
        raw = self.private_group_archive()
        self.prepare()
        with patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)):
            m.extract(self.context)
        stage = self.stage_path()
        for relative, mode in (("Contents", 0o775), ("Contents/Info.plist", 0o664),
                               ("Contents/Developer/usr/bin/xcodebuild", 0o775)):
            s = (stage / relative).stat()
            self.assertEqual(s.st_mode & 0o7777, mode)
            self.assertEqual(s.st_uid, os.getuid())
            self.assertEqual(s.st_gid, stage.stat().st_gid)
        with m.Directory(str(stage)) as bundle, self.assertRaises(m.Refused):
            m.inventory(bundle)
        self.assertEqual(m.guest("publish", self.context), {"outcome": "activated", "match": True})
        self.assertTrue(self.prepare()["existing"])
        self.assertEqual(m.guest("publish", self.context), {"outcome": "already-selected", "match": True})

    def test_private_root_mode_changes_block_extraction_and_reuse(self):
        raw = self.private_group_archive()
        self.prepare()
        for mode in (0o750, 0o770, 0o777):
            self.root.chmod(mode)
            try:
                with self.subTest(mode=oct(mode)), patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), \
                        self.assertRaises(m.Refused):
                    m.extract(self.context)
            finally:
                self.root.chmod(0o700)
        self.assertEqual(list(self.stage_path().iterdir()), [])
        with patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)):
            m.extract(self.context)
        m.guest("publish", self.context)
        self.root.chmod(0o750)
        try:
            with self.assertRaises(m.Refused):
                self.prepare()
        finally:
            self.root.chmod(0o700)

    def test_private_root_owner_changes_block_extraction_and_inventory(self):
        raw = self.private_group_archive()
        self.prepare()
        s = self.root.stat()
        # UID 0 is otherwise admitted by Directory for ancestors; it cannot
        # replace current-user ownership of the private copy authority.
        foreign = 0 if os.getuid() else 12345
        with metadata_view({(s.st_dev, s.st_ino): {"st_uid": foreign}}):
            with patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), self.assertRaises(m.Refused):
                m.extract(self.context)
            with self.assertRaises(m.Refused):
                self.prepare()
        self.assertEqual(list(self.stage_path().iterdir()), [])

    def test_foreign_copied_files_directories_and_links_block_publication_and_reuse(self):
        raw = self.private_group_archive()
        self.prepare()
        with patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)):
            m.extract(self.context)
        for published in (False, True):
            if published:
                m.guest("publish", self.context)
            bundle_path = self.target if published else self.stage_path()
            for relative in ("Contents/Info.plist", "Contents/Developer", "Contents/Developer/current"):
                s = (bundle_path / relative).lstat()
                foreign = 0 if os.getuid() else 12345
                with self.subTest(published=published, relative=relative), \
                        metadata_view({(s.st_dev, s.st_ino): {"st_uid": foreign}}), self.assertRaises(m.Refused):
                    if published:
                        self.prepare()
                    else:
                        m.guest("publish", self.context)

    def test_private_root_pins_remain_required_during_copy(self):
        raw = self.private_group_archive()
        self.prepare()
        with m.Directory(str(self.root)) as root, m.Directory(str(self.stage_path())) as bundle:
            actual_read = m.tarfile.open

            def changed_root(*args, **kwargs):
                self.root.chmod(0o750)
                return actual_read(*args, **kwargs)

            try:
                with patch.object(m.tarfile, "open", side_effect=changed_root), \
                        patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), self.assertRaises(m.Refused):
                    m.receive(bundle, self.expected, private_root=root)
            finally:
                self.root.chmod(0o700)
        self.assertEqual(list(self.stage_path().iterdir()), [])

    def test_copied_group_write_has_no_boolean_or_outside_root_bypass(self):
        self.prepare()
        (self.source / "Contents/Info.plist").chmod(0o664)
        with m.Directory(str(self.root)) as root, m.Directory(str(self.source)) as outside:
            with self.assertRaises(m.Refused):
                m.inventory(outside, private_root=root)
        for authority in (None, True, {"owned": True, "mode": 0o700}):
            raw = io.BytesIO()
            with tarfile.open(fileobj=raw, mode="w") as archive:
                member = tarfile.TarInfo("payload")
                member.mode = 0o664
                archive.addfile(member)
            raw.seek(0)
            with m.Directory(str(self.stage_path())) as bundle, \
                    patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), self.assertRaises(m.Refused):
                m.receive(bundle, {"entries": 1, "bytes": 0}, private_root=authority)
        self.assertEqual(list(self.stage_path().iterdir()), [])

    def test_private_copy_still_refuses_world_write_and_set_id(self):
        self.prepare()
        for mode in (0o666, 0o777, 0o4664, 0o2664):
            raw = io.BytesIO()
            with tarfile.open(fileobj=raw, mode="w") as archive:
                member = tarfile.TarInfo("payload")
                member.mode = mode
                archive.addfile(member)
            raw.seek(0)
            with self.subTest(mode=oct(mode)), m.Directory(str(self.root)) as root, \
                    m.Directory(str(self.stage_path())) as bundle, \
                    patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), self.assertRaises(m.Refused):
                m.receive(bundle, {"entries": 1, "bytes": 0}, private_root=root)
        self.assertEqual(list(self.stage_path().iterdir()), [])

    def test_supplied_context_root_is_not_copy_authority(self):
        raw = self.private_group_archive()
        self.prepare()
        self.context["state"]["root"] = str(self.base)
        with patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), self.assertRaises(m.Refused):
            m.extract(self.context)
        self.assertEqual(list(self.stage_path().iterdir()), [])

    def test_receiver_checks_actual_copied_link_owner(self):
        raw = self.private_group_archive()
        self.prepare()
        owners = {}
        actual_symlink = os.symlink

        def foreign_link(*args, **kwargs):
            actual_symlink(*args, **kwargs)
            s = os.stat(args[1], dir_fd=kwargs["dir_fd"], follow_symlinks=False)
            owners[(s.st_dev, s.st_ino)] = {"st_uid": 0 if os.getuid() else 12345}

        with metadata_view(owners), patch.object(os, "symlink", side_effect=foreign_link), \
                patch.object(sys, "stdin", types.SimpleNamespace(buffer=raw)), self.assertRaises(m.Refused):
            m.extract(self.context)
        self.assertFalse(self.target.exists())

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
