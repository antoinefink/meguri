#!/usr/bin/env python3
"""Generated files only; Apple calls are doubles, never real app/account data."""

import contextlib
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("cloud_media_probe", Path(__file__).resolve().parents[1] /
                                              "scripts/cloud-media-probe.py")
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
BUILD = "20261111T010101Z-1"
STAGE = "20261111T010102Z-2"
PREP = "20261111T010103Z-3"
VERIFY = "20261111T010104Z-4"
BUNDLE = "org.fixture.normal.dev"
DEVICE = "fixture-device-private"
SECRET = "private-native-diagnostic-sentinel"


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


class ProbeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "checkout"
        self.home = self.base / "trusted-home"
        self.home.mkdir(mode=0o700)
        self.collection = self.home / "Library/Containers" / BUNDLE / "Data/Library/Application Support/Collection"
        self.media = self.collection / "collection.media"
        self.media.mkdir(parents=True)
        self.existing = self.media / "keep.bin"
        self.existing.write_bytes(b"existing media")
        self.database = self.collection / "collection.anki2"
        self.database.write_bytes(b"collection sentinel")
        self.protected = {path: (path.read_bytes(), path.stat().st_mtime_ns) for path in (self.existing, self.database)}
        self.app = self.root / p.PREVIEW
        (self.app / "Contents/MacOS").mkdir(parents=True)
        self.exe = self.app / "Contents/MacOS/Meguri"
        self.exe.write_bytes(b"generated development executable")
        self.info = {"CFBundleIdentifier": BUNDLE, "CFBundleExecutable": "Meguri", "MeguriCloudEnvironment": "development",
                     "MeguriCloudContainer": "iCloud.org.fixture.development", "MeguriCloudSchemaConfigured": True,
                     "MeguriCardInspectionBuild": "personalRelease", "LSUIElement": False}
        self.write_info()
        self.build = self.receipt("build-app", BUILD, ["--platform", "mac", "--personal-preview"], {
            "build": {"platform": "mac", "config": "Release", "app": str(self.app), "action": "build",
                      "standalone": False, "personal_preview": True},
            "personal_preview_product": {"signature_verified": True, "provisioning_verified": True,
                "native_visible_application": True, "test_hooks": False, "fixture_read_scope": False,
                "cloud_environment": "development", "card_inspection_build": "personalRelease", "bundle_id": BUNDLE,
                "executable_sha256": p.digest(self.exe.read_bytes())}})
        self.settings = {"PRODUCT_BUNDLE_IDENTIFIER": BUNDLE, "MEGURI_SIGNING": "team",
                         "MEGURI_CLOUD_CONTAINER_development": self.info["MeguriCloudContainer"],
                         "MEGURI_CLOUD_CONTAINER_production": "iCloud.org.fixture.production",
                         "MEGURI_CLOUD_CONTAINER": self.info["MeguriCloudContainer"],
                         "MEGURI_CLOUD_CONFIGURATION": "development", "MEGURI_CLOUD_ENVIRONMENT": "development",
                         "MEGURI_CLOUD_SCHEMA_CONFIGURED_development": "YES"}
        self.settings_path = self.root / "build/mg/build-app" / BUILD / "personal-preview-settings.json"
        write_json(self.settings_path, [{"target": "Meguri", "buildSettings": self.settings}])
        self.prep = self.receipt("run", PREP, ["--platform", "iphone", "--physical-device", DEVICE, "--config", "Debug"], {
            "normal_launch_verified": True, "build": {"platform": "ios-device", "config": "Debug",
            "app": str(self.root / p.PHONE), "personal_preview": False}})
        self.prepared = {"version": 2, "job": "run", "device": DEVICE, "configuration": "Debug", "normal_launch_verified": True,
                         "artifacts": {"products": {"app": {"bundle_id": BUNDLE, "path": str(self.root / p.PHONE),
                             "signature": {"verified": True, "development": True}}}}}
        self.prepared_path = self.root / "build/mg/run" / PREP / "physical/preparation.json"
        write_json(self.prepared_path, self.prepared)
        self.calls = []
        self.copy_mode = "exact"
        self.native_patch = patch.object(p.subprocess, "run", side_effect=self.native_double)
        self.native_patch.start()
        self.addCleanup(self.native_patch.stop)

    def tearDown(self):
        for path, state in self.protected.items():
            self.assertEqual((path.read_bytes(), path.stat().st_mtime_ns), state)

    def write_info(self):
        (self.app / "Contents/Info.plist").write_bytes(plistlib.dumps(self.info))

    def receipt(self, job, run, args, details):
        value = {"schema_version": 1, "job": job, "run_id": run, "root": str(self.root), "status": "pass",
                 "exit_code": 0, "dry_run": False, "placement": "host", "args": args, "details": details}
        self.save(job, run, value)
        return value

    def save(self, job, run, value):
        write_json(self.root / "build/mg" / job / run / "result.json", value)

    def native_double(self, command, **kwargs):
        self.calls.append(command)
        self.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
        kwargs["stdout"].write(SECRET.encode())
        kwargs["stderr"].write(SECRET.encode())
        if command[0] == "/usr/bin/codesign":
            self.assertEqual(command, ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(self.app)])
            return SimpleNamespace(returncode=0)
        dest = p.private_path(self.home, VERIFY)
        expected = ["/usr/bin/xcrun", "devicectl", "device", "copy", "from", "--device", DEVICE,
                    "--source", "Library/Application Support/Collection/collection.media/" + p.filename(STAGE),
                    "--domain-type", "appDataContainer", "--domain-identifier", BUNDLE,
                    "--destination", str(dest / "peer.bin"), "--timeout", "60", "--json-output", str(dest / "phone-copy.json")]
        self.assertEqual(command, expected)
        write_json(dest / "phone-copy.json", {"info": {"outcome": "failed" if self.copy_mode == "json-fail" else "success"},
                                             "result": {"private": SECRET}})
        raw = (p.private_path(self.home, STAGE) / "source.bin").read_bytes()
        if self.copy_mode == "symlink":
            (dest / "peer.bin").symlink_to(self.existing)
        elif self.copy_mode == "directory":
            (dest / "peer.bin").mkdir()
        elif self.copy_mode != "missing":
            (dest / "peer.bin").write_bytes(raw if self.copy_mode in ("exact", "fail", "json-fail") else b"x" * (p.SIZE if self.copy_mode == "wrong-hash" else 5))
        return SimpleNamespace(returncode=1 if self.copy_mode == "fail" else 0)

    def stage(self, run=STAGE):
        with contextlib.ExitStack() as held:
            private = p.new_private(held, self.home, run)
            return p.stage(p.Inputs(held, self.root), held, self.home, run, BUILD, private)

    def staged_receipt(self):
        report = self.stage()
        return self.receipt("cloud-media-probe", STAGE, ["stage", "--preview-build", BUILD], {"probe": report})

    def verify(self):
        with contextlib.ExitStack() as held:
            private = p.new_private(held, self.home, VERIFY)
            return p.verify_phone(p.Inputs(held, self.root), self.home, STAGE, PREP, private)

    def test_stage_retains_fresh_exact_payload_and_staged_only_evidence(self):
        first = self.stage()
        raw = (p.private_path(self.home, STAGE) / "source.bin").read_bytes()
        self.assertEqual(len(raw), p.SIZE)
        self.assertEqual(first["sha256"], p.digest(raw))
        self.assertEqual((self.media / first["filename"]).read_bytes(), raw)
        self.assertEqual(first["outcome"], "staged-only")
        self.assertFalse(first["transfer_verified"])
        self.assertFalse(first["signed_lane_acceptance"])
        self.assertFalse((self.collection / (".meguri-media-probe-" + STAGE + ".staging")).exists())
        second = self.stage(VERIFY)
        self.assertNotEqual(first["sha256"], second["sha256"])

    def test_no_overwrite_existing_bytes_or_symlink_and_failure_state_retained(self):
        for run, symlink in ((STAGE, False), (VERIFY, True)):
            dest = self.media / p.filename(run)
            if symlink:
                dest.symlink_to(self.existing)
            else:
                dest.write_bytes(b"never overwrite")
            with self.assertRaises(p.Refused):
                self.stage(run)
            self.assertTrue(dest.is_symlink() if symlink else dest.read_bytes() == b"never overwrite")
            self.assertTrue((p.private_path(self.home, run) / "source.bin").is_file())
            self.assertTrue((self.collection / (".meguri-media-probe-" + run + ".staging") / "payload.bin").is_file())

    def test_media_symlink_escape_is_refused_without_touching_target(self):
        self.existing.unlink()
        self.media.rmdir()
        outside = self.base / "outside"
        outside.mkdir()
        self.media.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(OSError):
            self.stage()
        self.assertEqual(list(outside.iterdir()), [])
        # Restore protected sentinel for teardown; no probe-created path exists.
        self.media.unlink()
        self.media.mkdir()
        self.existing.write_bytes(self.protected[self.existing][0])
        os.utime(self.existing, ns=(self.existing.stat().st_atime_ns, self.protected[self.existing][1]))

    def test_missing_media_root_is_not_created(self):
        self.existing.unlink()
        self.media.rmdir()
        with self.assertRaises(OSError):
            self.stage()
        self.assertFalse(self.media.exists())
        self.media.mkdir()
        self.existing.write_bytes(self.protected[self.existing][0])
        os.utime(self.existing, ns=(self.existing.stat().st_atime_ns, self.protected[self.existing][1]))

    def test_directory_binding_change_refused_before_publication(self):
        original = p.write_new

        def changed(parent, name, raw):
            original(parent, name, raw)
            if name == "payload.bin":
                self.media.rename(self.collection / "moved-media")
                self.media.mkdir()

        with patch.object(p, "write_new", side_effect=changed), self.assertRaises(p.Refused):
            self.stage()
        self.assertFalse((self.media / p.filename(STAGE)).exists())
        self.media.rmdir()
        (self.collection / "moved-media").rename(self.media)

    def test_product_hash_identity_schema_and_hooks_fail_closed(self):
        variants = [("test_hooks", True), ("provisioning_verified", False), ("cloud_environment", "production"),
                    ("executable_sha256", "0" * 64), ("native_visible_application", False)]
        for index, (key, value) in enumerate(variants, 10):
            mutated = copy.deepcopy(self.build)
            mutated["details"]["personal_preview_product"][key] = value
            self.save("build-app", BUILD, mutated)
            with self.assertRaises(p.Refused):
                self.stage("20261111T010102Z-" + str(index))
        self.save("build-app", BUILD, self.build)
        self.info["MeguriCloudSchemaConfigured"] = False
        self.write_info()
        with self.assertRaises(p.Refused):
            self.stage()
        self.assertEqual(self.calls, [])

    def test_passing_receipts_must_be_original_here_and_not_dry_run(self):
        for index, (key, value) in enumerate((("status", "fail"), ("dry_run", True), ("root", "/elsewhere")), 20):
            mutated = copy.deepcopy(self.build)
            mutated[key] = value
            self.save("build-app", BUILD, mutated)
            with self.assertRaises(p.Refused):
                self.stage("20261111T010102Z-" + str(index))
        self.assertEqual(self.calls, [])

    def test_exact_peer_bytes_accepted_with_closed_command_and_private_output(self):
        self.staged_receipt()
        report = self.verify()
        self.assertEqual(report["outcome"], "peer-byte-match")
        self.assertTrue(report["transfer_verified"])
        self.assertFalse(report["signed_lane_acceptance"])
        public = json.dumps(report)
        for private in (SECRET, DEVICE, BUNDLE, str(self.home), str(self.root)):
            self.assertNotIn(private, public)
        dest = p.private_path(self.home, VERIFY)
        self.assertEqual(dest.stat().st_mode & 0o777, 0o700)
        for name in ("phone-copy.stdout", "phone-copy.stderr"):
            self.assertEqual((dest / name).read_text(), SECRET)
        self.assertIn(SECRET, (dest / "phone-copy.json").read_text())
        self.assertEqual(len(self.calls), 3)  # stage signature, verify signature, one copy-from

    def test_failed_native_copy_cannot_pass_even_with_exact_file(self):
        self.staged_receipt()
        self.copy_mode = "fail"
        with self.assertRaisesRegex(p.Refused, "native-command-failed"):
            self.verify()
        self.assertEqual(len(self.calls), 3)

    def test_peer_wrong_hash_size_symlink_directory_or_absence_never_passes(self):
        self.staged_receipt()
        for mode in ("wrong-hash", "short", "symlink", "directory", "missing", "json-fail"):
            self.copy_mode = mode
            with self.assertRaises((p.Refused, OSError)):
                self.verify()
            # Test-owned generated evidence only; production never recursively cleans.
            import shutil
            shutil.rmtree(p.private_path(self.home, VERIFY))

    def test_source_tampering_refused_before_any_device_copy(self):
        self.staged_receipt()
        (p.private_path(self.home, STAGE) / "source.bin").write_bytes(b"x" * p.SIZE)
        with self.assertRaisesRegex(p.Refused, "source-refused"):
            self.verify()
        self.assertFalse(any("devicectl" in call for call in self.calls))

    def test_source_symlink_refused_without_copying_any_device_file(self):
        self.staged_receipt()
        source = p.private_path(self.home, STAGE) / "source.bin"
        source.unlink()
        source.symlink_to(self.existing)
        with self.assertRaises(OSError):
            self.verify()
        self.assertFalse(any("devicectl" in call for call in self.calls))

    def test_failed_codesign_does_not_publish(self):
        with patch.object(p.subprocess, "run", return_value=SimpleNamespace(returncode=1)), \
                self.assertRaisesRegex(p.Refused, "native-command-failed"):
            self.stage()
        self.assertFalse((self.media / p.filename(STAGE)).exists())

    def test_manifest_cannot_choose_arbitrary_remote_path(self):
        staged = self.staged_receipt()
        staged["details"]["probe"]["filename"] = "../../private-file"
        self.save("cloud-media-probe", STAGE, staged)
        with self.assertRaises(p.Refused):
            self.verify()
        self.assertEqual(len(self.calls), 1)

    def test_preparation_overrides_or_different_bundle_refused(self):
        self.staged_receipt()
        self.prep["args"] += ["--route", "settings"]
        self.save("run", PREP, self.prep)
        with self.assertRaises(p.Refused):
            self.verify()
        import shutil
        shutil.rmtree(p.private_path(self.home, VERIFY))
        self.prep["args"] = self.prep["args"][:-2]
        self.save("run", PREP, self.prep)
        self.prepared["artifacts"]["products"]["app"]["bundle_id"] = "org.fixture.other.dev"
        write_json(self.prepared_path, self.prepared)
        with self.assertRaises(p.Refused):
            self.verify()
        self.assertFalse(any("devicectl" in call for call in self.calls))

    def test_original_receipt_tampering_during_native_call_refused(self):
        real = self.native_double

        def tamper(command, **kwargs):
            result = real(command, **kwargs)
            self.build["details"]["unexpected"] = True
            self.save("build-app", BUILD, self.build)
            return result

        with patch.object(p.subprocess, "run", side_effect=tamper), self.assertRaises(p.Refused):
            self.stage()
        self.assertFalse((self.media / p.filename(STAGE)).exists())

    def test_cli_linux_and_invalid_inputs_are_inert_and_private(self):
        for arguments in (["stage", "--preview-build", BUILD], ["stage", "--preview-build", "../../" + SECRET],
                          ["verify-phone", "--stage", STAGE, "--preparation", PREP, "--filename", SECRET]):
            out, err = io.StringIO(), io.StringIO()
            with patch.object(p.sys, "platform", "linux"), patch.object(p.pwd, "getpwuid") as home_query, \
                    contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                rc = p.main(["--root", str(self.root), "--run-id", VERIFY, *arguments])
            self.assertEqual(rc, 1)
            home_query.assert_not_called()
            self.assertEqual(json.loads(out.getvalue())["outcome"], "failed")
            self.assertNotIn(SECRET, out.getvalue() + err.getvalue())
            self.assertEqual(err.getvalue(), "")
        self.assertEqual(self.calls, [])
        self.assertFalse(p.private_path(self.home, VERIFY).exists())


if __name__ == "__main__":
    unittest.main()
