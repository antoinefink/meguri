#!/usr/bin/env python3
"""Synthetic reader/projection evidence only; no setup or signed execution."""

import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("cloud_agent_account_input", Path(__file__).resolve().parents[1] /
                                              "scripts/cloud-agent-account-input.py")
a = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = a
spec.loader.exec_module(a)
CONTAINER = "iCloud.example.synthetic"


class AccountInputTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.support = self.base / "OS-observed-support"
        self.directory = self.support / "Meguri/CloudKitSignedPrivate"
        self.directory.mkdir(parents=True, mode=0o700)
        for path in (self.support, self.directory.parent, self.directory):
            path.chmod(0o700)
        self.source = self.directory / "approved-accounts.json"
        self.approved = {"protocol_version": 1, "container": CONTAINER, "environment": "development",
                         "accounts": {"acct1": [37, 0, 129, 255], "acct2": [201, 7, 42]}}
        self.header = {
            "protocol_version": 1, "job": "cloudkit-signed", "run_id": "20261102T010200Z-103",
            "lane": "cloud-signed", "source_commit": "a" * 40, "tree_clean": True,
            "device": "slot1", "device_kind": "mac", "os_build": "synthetic-os-build", "app_build": "42",
            "executable_sha256": "b" * 64, "build_receipt_sha256": "c" * 64,
            "build_receipt": {"job": "build-app", "run_id": "20261102T010000Z-101"},
            "deployment_receipt": {"job": "cloudkit-agent-install", "run_id": "20261102T010100Z-102"},
            "container": CONTAINER, "environment": "development", "account_scope": "synthetic-scope",
            "account_labels": ["acct1"], "selection": "smoke", "fixture_delivery": {},
        }
        self.save()
        # Explicit private platform-query fake. No production env/path override.
        self.query_patch = patch.object(a, "_platform_application_support_root", return_value=str(self.support))
        self.query = self.query_patch.start()
        self.addCleanup(patch.stopall)

    def save(self, raw=None):
        self.source.write_bytes(raw if raw is not None else json.dumps(self.approved).encode())
        self.source.chmod(0o600)

    def header_bytes(self):
        # Deliberate formatting: header hash must bind original bytes exactly.
        return (json.dumps(self.header, indent=2) + "\n\n").encode()

    def open(self, raw=None, container=CONTAINER):
        return a.approved_account_input(self.header_bytes() if raw is None else raw, container)

    def refused(self, kind="failed_evidence", raw=None, container=CONTAINER):
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            with self.assertRaises(a.InputFailure) as failure:
                with self.open(raw, container) as context:
                    context.private_bytes()
        public = failure.exception.public_outcome()
        self.assertEqual(public["outcome"], kind)
        self.assertEqual(set(public), {"outcome", "category"})
        if kind == "awaiting_user":
            self.assertEqual(public["category"], "cloudkit-test-accounts")
        elif kind == "implementation_missing":
            self.assertEqual(public["category"], "account-input-platform-query")
        else:
            self.assertEqual(public["category"], "account-input-refused")
        report = json.dumps(public) + str(failure.exception) + repr(failure.exception)
        self.assertEqual(stdout.getvalue() + stderr.getvalue(), "")
        for raw_private in (self.source.read_bytes() if self.source.is_file() else b"synthetic-absent",
                            bytes(self.approved["accounts"].get("acct1", [37])) if
                            all(type(n) is int and 0 <= n <= 255 for n in
                                self.approved["accounts"].get("acct1", [37])) else b"synthetic-identity"):
            self.assertNotIn(hashlib.sha256(raw_private).hexdigest(), report)
        self.assertNotIn(str(self.support), report)
        self.assertNotIn(str(self.approved["accounts"]), report)
        return failure.exception

    def test_exact_single_and_two_account_projection(self):
        for labels in (["acct1"], ["acct1", "acct2"], ["acct2", "acct1"]):
            with self.subTest(labels=labels):
                self.header["account_labels"] = labels
                header_raw = self.header_bytes()
                wanted = {
                    "protocol_version": 1, "header_sha256": hashlib.sha256(header_raw).hexdigest(),
                    "job": self.header["job"], "run_id": self.header["run_id"],
                    "source_commit": self.header["source_commit"], "device": self.header["device"],
                    "executable_sha256": self.header["executable_sha256"], "container": CONTAINER,
                    "environment": "development", "account_scope": self.header["account_scope"],
                    "accounts": {label: self.approved["accounts"][label] for label in labels},
                }
                with self.open(header_raw) as context:
                    actual = context.private_bytes()
                    self.assertEqual(actual, (json.dumps(wanted, sort_keys=True) + "\n").encode())
                    self.assertEqual(json.loads(actual), wanted)
                    self.assertNotIn("accounts", repr(context))
                    context.revalidate()
                with self.assertRaises(a.InputFailure):
                    context.private_bytes()

    def test_source_with_only_acct1_is_sufficient_for_single_account(self):
        del self.approved["accounts"]["acct2"]
        self.save()
        with self.open() as context:
            self.assertEqual(json.loads(context.private_bytes())["accounts"], self.approved["accounts"])
        self.header["account_labels"] = ["acct1", "acct2"]
        self.refused("awaiting_user")

    def test_missing_source_and_private_directories_await_setup_without_creation(self):
        self.source.unlink()
        self.refused("awaiting_user")
        self.assertFalse(self.source.exists())
        self.directory.rmdir()
        self.refused("awaiting_user")
        self.assertFalse(self.directory.exists())
        self.directory.parent.rmdir()
        self.refused("awaiting_user")
        self.assertFalse(self.directory.parent.exists())
        self.support.rmdir()
        self.refused("awaiting_user")
        self.assertFalse(self.support.exists())

    def test_binding_schema_and_labels_are_strict(self):
        original = copy.deepcopy(self.approved)
        cases = [dict(original, protocol_version=v) for v in (True, 1.0, 2, "1", None)]
        cases += [dict(original, environment=v) for v in ("production", "Development", "", None)]
        cases += [dict(original, container=v) for v in ("iCloud.example.other", "", None)]
        cases += [dict(original, unknown=1), {k: v for k, v in original.items() if k != "environment"}]
        cases += [dict(original, accounts=v) for v in ({}, [], None, {"acct2": [1]},
                  {"acct1": [1], "other": [2]}, {"acct1": [1], "acct2": [1]})]
        for value in cases:
            with self.subTest(value=value):
                self.save(json.dumps(value).encode())
                self.refused()

    def test_identity_arrays_reject_empty_bool_float_range_and_nested_values(self):
        for identity in ([], True, "bytes", {}, [True], [False], [1.0], [1e2], [-1], [256],
                         [None], [[1]], [{"byte": 1}], [1, "2"]):
            with self.subTest(identity=identity):
                value = copy.deepcopy(self.approved)
                value["accounts"]["acct1"] = identity
                self.save(json.dumps(value).encode())
                self.refused()

    def test_duplicate_keys_and_empty_malformed_or_oversized_sources_refuse(self):
        raw = json.dumps(self.approved).encode()
        values = (b"", b" ", b"null", b"[]", b"{}", b"\xff", b"{", b"NaN", raw.decode().encode("utf-16"),
                  b" " * (a.LIMIT + 1), raw.replace(b'"protocol_version": 1',
                  b'"protocol_version": 1, "protocol_version": 1'),
                  raw.replace(b'"acct1":', b'"acct1": [1], "acct1":'),
                  raw.replace(b'"acct1":', b'"acct\\u0031": [1], "acct1":'))
        for value in values:
            with self.subTest(size=len(value)):
                self.save(value)
                self.refused()

    def test_projection_checks_header_and_product_container(self):
        self.refused(container="iCloud.example.other")
        original = copy.deepcopy(self.header)
        for key, value in (("protocol_version", True), ("protocol_version", 1.0),
                           ("protocol_version", 2), ("environment", "production"),
                           ("container", "iCloud.example.other"), ("account_labels", []),
                           ("account_labels", ["acct2"]), ("account_labels", ["acct1", "acct1"]),
                           ("account_labels", ["acct1", "unknown"]), ("device", "../elsewhere"),
                           ("run_id", "../elsewhere")):
            with self.subTest(key=key, value=value):
                self.header = {**original, key: value}
                self.refused()
        self.header = original
        self.refused(raw=self.header_bytes().replace(b'"protocol_version": 1',
                         b'"protocol_version": 1, "protocol_version": 1'))

    def test_source_bytes_replacement_mode_and_link_changes_invalidate_stickily(self):
        for mutation in ("bytes", "replace", "mode", "hardlink", "unlink", "symlink"):
            with self.subTest(mutation=mutation):
                self.save()
                with self.assertRaises(a.InputFailure):
                    with self.open() as context:
                        original = context.private_bytes()
                        if mutation == "bytes":
                            changed = copy.deepcopy(self.approved)
                            changed["accounts"]["acct1"] = [99]
                            self.save(json.dumps(changed).encode())
                        elif mutation == "replace":
                            replacement = self.directory / "replacement"
                            replacement.write_bytes(self.source.read_bytes())
                            replacement.chmod(0o600)
                            replacement.replace(self.source)
                        elif mutation == "mode":
                            self.source.chmod(0o644)
                        elif mutation == "hardlink":
                            os.link(self.source, self.directory / "extra-link")
                        elif mutation == "unlink":
                            self.source.unlink()
                        else:
                            self.source.rename(self.directory / "target")
                            self.source.symlink_to("target")
                        with self.assertRaises(a.InputFailure):
                            context.private_bytes()
                        self.assertIsInstance(original, bytes)
                        context.revalidate()  # Cannot reuse or retarget this header.
                if self.source.is_symlink() or self.source.exists():
                    self.source.unlink()
                for name in ("extra-link", "target"):
                    (self.directory / name).unlink(missing_ok=True)

    def test_initial_links_modes_and_owner_refuse(self):
        self.source.chmod(0o644)
        self.refused()
        self.source.chmod(0o600)
        os.link(self.source, self.directory / "alias")
        self.refused()
        (self.directory / "alias").unlink()
        self.source.rename(self.directory / "target")
        self.source.symlink_to("target")
        self.refused()
        self.source.unlink()
        (self.directory / "target").rename(self.source)
        for path in (self.support, self.directory.parent, self.directory):
            path.chmod(0o777)
            self.refused()
            path.chmod(0o700)
        with patch.object(a.os, "getuid", return_value=os.getuid() + 1):
            self.refused()
        for path in (self.directory, self.directory.parent):
            moved = path.with_name(path.name + "-physical")
            path.rename(moved)
            path.symlink_to(moved.name, target_is_directory=True)
            self.refused()
            path.unlink()
            moved.rename(path)

    def test_ancestor_replacement_and_mode_changes_refuse_at_context_exit(self):
        for path in (self.support, self.directory.parent, self.directory):
            with self.subTest(path=path.name):
                moved = path.with_name(path.name + "-original")
                with self.assertRaises(a.InputFailure):
                    with self.open() as context:
                        context.private_bytes()
                        path.rename(moved)
                        path.mkdir(mode=0o700)
                path.rmdir()
                moved.rename(path)
                with self.assertRaises(a.InputFailure):
                    with self.open():
                        path.chmod(0o755 if path != self.support else 0o777)
                path.chmod(0o700)

    def test_changes_during_initial_read_are_rejected(self):
        read = a.os.pread
        mutated = False
        def replacing(fd, size, offset):
            nonlocal mutated
            result = read(fd, size, offset)
            if not mutated:
                mutated = True
                self.source.rename(self.directory / "original")
                self.save()
            return result
        with patch.object(a.os, "pread", side_effect=replacing):
            self.refused()

    def test_reader_is_read_only_and_creates_no_enrollment_or_per_run_files(self):
        before = self.source.stat()
        raw = self.source.read_bytes()
        entries = sorted(str(p.relative_to(self.support)) for p in self.support.rglob("*"))
        real_open = a.os.open
        def read_only(path, flags, *args, **kwargs):
            self.assertFalse(flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND))
            return real_open(path, flags, *args, **kwargs)
        with patch.object(a.os, "open", side_effect=read_only), \
                patch.object(a.os, "mkdir", side_effect=AssertionError("no creation")), \
                patch.object(a.subprocess, "run", side_effect=AssertionError("no account operation")):
            with self.open() as context:
                context.private_bytes()
        after = self.source.stat()
        self.assertEqual((before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns),
                         (after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns))
        self.assertEqual(raw, self.source.read_bytes())
        self.assertEqual(entries, sorted(str(p.relative_to(self.support)) for p in self.support.rglob("*")))

    def test_query_unavailability_stays_implementation_missing(self):
        self.query.side_effect = a.InputFailure("missing")
        self.refused("implementation_missing")

    def test_fixed_platform_query_captures_diagnostics_and_has_no_caller_path(self):
        # Exercise ONLY a subprocess double, never Foundation or host operations.
        self.query_patch.stop()
        with patch.object(a.sys, "platform", "darwin"), patch.object(a.subprocess, "run") as command:
            command.return_value = subprocess.CompletedProcess([], 0, json.dumps(str(self.support)).encode(),
                                                               b"synthetic-private-diagnostic")
            self.assertEqual(a._platform_application_support_root(), str(self.support))
            args, kwargs = command.call_args
            self.assertEqual(args[0][:4], ["/usr/bin/osascript", "-l", "JavaScript", "-e"])
            self.assertIn("NSApplicationSupportDirectory", args[0][4])
            self.assertIn("NSUserDomainMask", args[0][4])
            self.assertNotIn("HOME", args[0][4])
            self.assertTrue(kwargs["capture_output"])
            self.assertEqual(kwargs["env"], {})
            command.side_effect = OSError(str(self.support))
            self.refused("implementation_missing")
        with patch.object(a.sys, "platform", "linux"), patch.object(a.subprocess, "run") as command:
            self.refused("implementation_missing")
            command.assert_not_called()


if __name__ == "__main__":
    unittest.main()
