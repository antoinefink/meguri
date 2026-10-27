"""Focused storage stubs; never attach a disk or run Cargo."""
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[2] / "mg.d/conformance-scratch.py"
SPEC = importlib.util.spec_from_file_location("conformance_scratch", SOURCE)
scratch = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scratch)


class ScratchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.run = "20261027T000000Z-123"
        self.run_dir = self.root / "build/mg/conformance" / self.run
        self.run_dir.mkdir(parents=True)
        self.subject = scratch.Scratch(str(self.root), self.run, str(self.run_dir), "a" * 32)
        self.calls = []
        self.attached = False
        self.fail_attach = False
        self.fail_detach = False
        self.wrong_image = False
        self.wrong_mount = False
        self.underlay = self.run_dir / "fake-underlay"
        self.addCleanup(patch.stopall)
        patch.object(scratch.subprocess, "run", side_effect=self.hdiutil).start()
        patch.object(scratch.os.path, "ismount", side_effect=lambda p: self.attached and p == self.subject.mount).start()
        # The fake filesystem cannot change st_dev. Exercise the actual guard
        # separately below; all owner records and hdiutil parsing remain real.
        self.mount_guard = patch.object(scratch.Scratch, "mounted", autospec=True,
                                       side_effect=lambda _: scratch.require(self.attached, "not mounted"))
        self.mount_guard.start()

    def hdiutil(self, argv, **kwargs):
        self.assertEqual(argv[0], "hdiutil")
        self.calls.append(argv[1:])
        action = argv[1]
        output, rc = b"", 0
        if action == "create":
            self.assertEqual(argv[2:], ["-size", "2g", "-type", "SPARSE", "-fs", "Case-sensitive APFS",
                                      "-volname", "mg-conformance", str(self.subject.image)])
            self.subject.image.write_bytes(b"sparse image stub")
        elif action == "attach":
            self.assertEqual(argv[2:], ["-nobrowse", "-noautoopen", "-mountpoint", str(self.subject.mount),
                                      str(self.subject.image)])
            self.subject.mount.rename(self.underlay)
            self.subject.mount.mkdir(mode=0o700)
            self.attached = True
            if self.fail_attach:
                rc = 1  # Also models a failed attach which did partially mount.
        elif action == "info":
            images = []
            if self.attached:
                images = [{"image-path": str(self.subject.image) + (".foreign" if self.wrong_image else ""),
                           "system-entities": [
                               {"dev-entry": "/dev/disk17"},
                               {"dev-entry": "/dev/disk17s1"},
                               {"dev-entry": "/dev/disk18s1", "mount-point":
                                str(self.subject.mount) + ("-foreign" if self.wrong_mount else "")},
                           ]}]
            output = plistlib.dumps({"images": images})
        elif action == "detach":
            self.assertEqual(argv[2:], ["/dev/disk17"])
            if self.fail_detach:
                rc = 1
            else:
                shutil.rmtree(self.subject.mount)
                self.underlay.rename(self.subject.mount)
                self.attached = False
        else:
            self.fail("unexpected hdiutil command " + repr(argv))
        return subprocess.CompletedProcess(argv, rc, stdout=output)

    def assert_removed(self):
        self.assertFalse(self.subject.directory.exists())
        self.assertFalse(self.attached)

    def test_prepare_check_cleanup_and_exact_probe(self):
        created = []
        original = Path.open

        def capture(path, *args, **kwargs):
            if args and args[0] == "x":
                created.append(path.name)
            return original(path, *args, **kwargs)

        with patch.object(Path, "open", capture):
            self.subject.prepare()
        self.assertIn("case-probe", created)
        self.assertIn("Case-probe", created)
        self.assertEqual(list((self.subject.mount / "tmp").iterdir()), [])
        self.subject.check()
        self.subject.cleanup()
        self.assert_removed()
        calls = len(self.calls)
        self.subject.cleanup()
        self.assertEqual(len(self.calls), calls)  # Idempotent, no disk command.

    def test_case_collision_refuses_without_losing_exact_probe_name(self):
        original = Path.open

        def folded(path, *args, **kwargs):
            if path.name == "Case-probe" and args and args[0] == "x":
                raise FileExistsError(17, "case-insensitive stub", str(path))
            return original(path, *args, **kwargs)

        with patch.object(Path, "open", folded), self.assertRaises(FileExistsError):
            self.subject.prepare()
        self.assert_removed()
        self.assertEqual(sum(c[0] == "attach" for c in self.calls), 1)

    def test_failed_partial_mount_detaches_only_owned_image(self):
        self.fail_attach = True
        with self.assertRaisesRegex(scratch.ScratchError, "attach failed"):
            self.subject.prepare()
        self.assert_removed()
        self.assertEqual([c for c in self.calls if c[0] == "detach"], [["detach", "/dev/disk17"]])

    def test_failed_mount_without_attachment_still_removes_image(self):
        original = self.hdiutil

        def no_mount(argv, **kwargs):
            if argv[1] == "attach":
                self.calls.append(argv[1:])
                return subprocess.CompletedProcess(argv, 1, stdout=b"")
            return original(argv, **kwargs)

        with patch.object(scratch.subprocess, "run", side_effect=no_mount):
            with self.assertRaises(scratch.ScratchError):
                self.subject.prepare()
        self.assert_removed()
        self.assertFalse(any(c[0] == "detach" for c in self.calls))

    def test_cancel_after_attach_cleans_without_repeating_attach(self):
        original = self.hdiutil

        def cancelled(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[1] == "attach":
                self.subject.cancelled = True
            return result

        with patch.object(scratch.subprocess, "run", side_effect=cancelled):
            with self.assertRaisesRegex(scratch.ScratchError, "cancelled"):
                self.subject.prepare()
        self.assert_removed()
        self.assertEqual(sum(c[0] == "attach" for c in self.calls), 1)

    def test_foreign_or_symlink_roots_are_never_adopted(self):
        self.subject.directory.mkdir()
        sentinel = self.subject.directory / "foreign"
        sentinel.write_text("keep")
        with self.assertRaises(FileExistsError):
            self.subject.prepare()
        with self.assertRaises((scratch.ScratchError, FileNotFoundError)):
            self.subject.cleanup()
        self.assertEqual(sentinel.read_text(), "keep")
        sentinel.unlink()
        self.subject.directory.rmdir()
        self.subject.directory.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            self.subject.prepare()
        with self.assertRaises(scratch.ScratchError):
            self.subject.cleanup()
        self.assertEqual(self.calls, [])

    def test_foreign_owner_mount_or_image_retains_resources(self):
        self.subject.prepare()
        owner = json.loads(self.subject.record.read_text())
        foreign = dict(owner, token="b" * 32)
        self.subject.record.write_text(json.dumps(foreign))
        with self.assertRaises(scratch.ScratchError):
            self.subject.cleanup()
        self.subject.record.write_text(json.dumps(owner))
        for field in ("wrong_image", "wrong_mount"):
            setattr(self, field, True)
            with self.assertRaises(scratch.ScratchError):
                self.subject.cleanup()
            self.assertTrue(self.attached)
            self.assertTrue(self.subject.image.exists())
            setattr(self, field, False)
        self.assertFalse(any(c[0] == "detach" for c in self.calls))
        self.subject.cleanup()
        self.assert_removed()

    def test_failed_detach_retains_image_and_reports_error(self):
        self.subject.prepare()
        self.fail_detach = True
        with self.assertRaisesRegex(scratch.ScratchError, "detach failed"):
            self.subject.cleanup()
        self.assertTrue(self.subject.image.exists())
        self.assertTrue(self.subject.record.exists())
        with self.assertRaisesRegex(scratch.ScratchError, "earlier scratch cleanup incomplete"):
            self.subject.cleanup()
        self.assertEqual(sum(c[0] == "detach" for c in self.calls), 1)

    def test_plain_folder_fails_mount_check(self):
        self.subject.directory.mkdir()
        self.subject.mount.mkdir()
        self.mount_guard.stop()
        with self.assertRaisesRegex(scratch.ScratchError, "separate mounted filesystem"):
            self.subject.mounted()

    def test_selection_is_read_only_and_matches_literal_and_glob_filters(self):
        tree = self.root / "conformance/scenarios"
        (tree / "package/backups").mkdir(parents=True)
        (tree / "package/backups/area.yaml").write_text("metadata")
        (tree / "package/backups/foreign-files.yaml").write_text("fixture")
        (tree / "literal[1].json").write_text("fixture")
        for f in ([], ["package"], ["/package/backups/foreign-files.yaml/"],
                  ["package/*/foreign-?iles.*"], ["literal[1].json"]):
            self.assertTrue(scratch.selected(self.root, ["conformance", "run", "--lane", "plan", *f]), f)
        for f in (["missing"], ["package/backups/area.yaml"], ["literal1.json"]):
            self.assertFalse(scratch.selected(self.root, ["conformance", "run", *f]), f)
        self.assertEqual(self.calls, [])
        self.assertFalse(self.subject.directory.exists())
