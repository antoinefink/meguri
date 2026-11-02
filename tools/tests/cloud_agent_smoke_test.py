#!/usr/bin/env python3
"""Generated parent/transport tests only. No signed, VM, SDK or smoke evidence."""
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("cloud_agent_smoke", Path(__file__).resolve().parents[1] /
                                              "scripts/cloud-agent-smoke.py")
s = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = s
spec.loader.exec_module(s)
RUN = "20261102T010200Z-103"
SHA = "a" * 40


def raw(value):
    return (json.dumps(value, sort_keys=True) + "\n").encode()


def header():
    return {"job": "cloudkit-signed", "run_id": RUN, "source_commit": SHA, "device": "mac1",
            "executable_sha256": "b" * 64, "container": "iCloud.example.generated", "environment": "development",
            "account_scope": "scope-generated", "account_labels": ["acct1"], "selection": "smoke", "device_kind": "mac"}


def private(header_bytes):
    h = json.loads(header_bytes)
    return raw({"protocol_version": 1, "header_sha256": s.v.digest(header_bytes),
                **{k: h[k] for k in ("job", "run_id", "source_commit", "device", "executable_sha256",
                                     "container", "environment", "account_scope")},
                "accounts": {"acct1": list(b"generated-opaque-choice")}})


def record(commands, command, seq, kind, **values):
    n = command
    return {"header_sha256": json.loads(commands[n-1])["header_sha256"], "record_seq": seq,
            "command_seq": n, "command_sha256": s.v.digest(commands[n-1][:-1]), "agent_instance": "instance1",
            "observed_at": "2026-11-02T01:02:00Z", "kind": kind, "facts": [], "checks": [], **values}


def pair(commands, n, seq):
    # A generated protocol double, not real operation receipts. These exercise
    # parser/ordering only; no output is registered as signed execution evidence.
    start = record(commands, n, seq, "started", action=s.PROGRAM[n-1][0])
    done = record(commands, n, seq+1, "done", checks=[
        {"name": "integrity", "outcome": "pass", "facts": [{"name": "commandCompleted"}]}])
    return raw(start) + raw(done)


class JournalTests(unittest.TestCase):
    def setUp(self):
        self.commands = s.command_bytes(raw(header()))
        self.journal = s.Journal(raw(header()), self.commands)

    def test_exact_ten_original_command_forms_have_no_requested_results(self):
        self.assertEqual(len(self.commands), 10)
        for n, command in enumerate(self.commands, 1):
            c = json.loads(command)
            self.assertEqual(set(c), {"header_sha256", "command_seq", "leg", "action", "arguments",
                                      "account_before", "account_after"})
            self.assertEqual((c["action"], c["arguments"]), s.PROGRAM[n-1])
            self.assertEqual(c["account_before"], c["account_after"])
            self.assertEqual(c["account_before"], "acct1")

    def test_prefix_truncation_changed_hash_duplicate_and_gap_refuse(self):
        first = pair(self.commands, 1, 1)
        complete, _ = self.journal.inspect(first, 1)
        self.journal.retain(complete)
        with self.assertRaises(s.v.Refused):
            self.journal.inspect(first[:-1], 1)
        with self.assertRaises(s.v.Refused):
            self.journal.inspect(first.replace(b'instance1', b'instance2'), 1)
        for key, value in (("command_sha256", "f"*64), ("record_seq", 9), ("command_seq", 2)):
            changed = json.loads(first.splitlines()[0])
            changed[key] = value
            with self.assertRaises(s.v.Refused):
                s.Journal(raw(header()), self.commands).inspect(raw(changed), 1)
        with self.assertRaises(s.v.Refused):
            self.journal.inspect(first + first, 1)

    def test_failed_blocks_successor_but_permits_late_original_observation(self):
        start = record(self.commands, 1, 1, "started", action="launch")
        failed = record(self.commands, 1, 2, "failed", failure="execution", error_domain="core", error_code=1)
        late = record(self.commands, 1, 3, "observation", facts=[{"name": "ownedWorkJoined"}])
        prefix = raw(start) + raw(failed) + raw(late)
        _, command_outcomes = self.journal.inspect(prefix, 1)
        self.assertEqual(command_outcomes, {1: "failed"})
        with self.assertRaises(s.v.Refused):
            self.journal.inspect(prefix + raw(record(self.commands, 2, 4, "started", action="connect")), 2)

    def test_incomplete_tail_has_no_terminal_and_requested_success_fields_refuse(self):
        first = pair(self.commands, 1, 1)
        complete, command_outcomes = self.journal.inspect(first[:-2], 1)
        self.assertEqual(command_outcomes, {})
        self.assertEqual(len(complete.splitlines()), 1)
        done = json.loads(first.splitlines()[1])
        done["checks"][0]["facts"][0]["requested_success"] = True
        with self.assertRaises(s.v.Refused):
            self.journal.inspect(first.splitlines()[0] + b"\n" + raw(done), 1)
        with self.assertRaises(s.v.Refused):
            self.journal.inspect(first.replace(b'"kind": "started"', b'"kind":"started","kind":"done"'), 1)
        _, command_outcomes = self.journal.inspect(first, 1)
        self.assertEqual(command_outcomes, {1: "done"})
        with self.assertRaises(s.Missing):
            s.completion(1, self.journal.done_checks[1])


class ParentTransport:
    def __init__(self, test):
        self.test, self.commands, self.journal, self.retained = test, [], b"", b""
        self.events = []

    def request(self, action, payload=b"", **metadata):
        self.events.append(action)
        if action == "prepare":
            self.test.assertNotIn("data", metadata["files"])
            self.test.assertEqual(set(metadata["files"]), set(self.test.roles))
            self.test.assertEqual(payload, b"".join(self.test.roles[n] for n in sorted(self.test.roles)) +
                                  self.test.projection.private_bytes())
            return {"prepared": True}, b"derived-private-plan"
        if action == "launch":
            return {"launched": True}, b""
        if action == "append":
            self.test.assertEqual((self.test.local_path / "commands.jsonl").read_bytes(),
                                  b"".join(self.commands) + payload)
            if self.commands:
                self.test.assertEqual((self.test.local_path / "steps.jsonl").read_bytes(), self.retained)
            self.commands.append(payload)
            return {"appended": True}, b""
        if action == "poll":
            n = len(self.commands)
            self.journal += pair(self.test.commands, n, 2*n-1)
            if n == self.test.fail_at:
                lines = self.journal.splitlines()
                failed = json.loads(lines[-1])
                failed.update(kind="failed", checks=[], failure="execution", error_domain="core", error_code=4)
                lines[-1] = raw(failed).rstrip(b"\n")
                self.journal = b"\n".join(lines) + b"\n"
            if n == self.test.change_at:
                self.journal = self.journal.replace(b'instance1', b'instance2')
            return {"alive": True}, self.journal
        if action == "retain":
            self.test.assertEqual((self.test.local_path / "steps.jsonl").read_bytes(), payload)
            self.retained = payload
            return {"retained": True}, b""
        if action == "status":
            return {}, b""
        if action == "finish":
            self.test.assertEqual(len(self.commands), 10)
            # Actual returned prefix AFTER original-process join, including a
            # late callback that the parent has not retained yet.
            final = record(self.test.commands, 10, 21, "observation", facts=[{"name": "ownedWorkJoined"}])
            self.journal += raw(final)
            return {"joined": True, "exit_code": 0, "passed": 1, "failed": 0, "skipped": 0}, self.journal
        raise AssertionError(action)

    def close(self):
        self.events.append("original-transport-joined")


class ParentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.local_path = self.root / "parent"
        self.local_path.mkdir(mode=0o700)
        self.local = s.h.Directory(str(self.local_path))
        self.addCleanup(self.local.close)
        self.support = self.root / "host-support"
        self.support.mkdir(mode=0o700)
        self.header = type("Header", (), {"raw": raw(header())})()
        self.commands = s.command_bytes(self.header.raw)
        self.expected = header()
        self.fail_at = self.change_at = None
        self.check_count = 0
        test = self
        class Projection:
            def private_bytes(self): return private(test.header.raw)
            def host_application_support(self): return str(test.support)
            def revalidate(self): test.check_count += 1
        self.projection = Projection()
        self.tree = type("Tree", (), {"files": {"manifest.json": type("F", (), {"raw": b"{}\n"})()},
                                     "check": lambda _: None})()
        self.fixture = type("Original", (), {"raw": b"{}\n", "check": lambda _: None})()
        self.frozen = b"{}\n"
        self.roles = {"fixture-input.json": self.frozen, "fixture-evidence/fixtures-result.json": b"{}\n",
                      "fixture-evidence/generation.json": b"{}\n", "fixture-evidence/parent/manifest.json": b"{}\n"}
        self.transport = ParentTransport(self)
        self.patchers = [patch.object(s, "Transport", return_value=self.transport),
                         # Transport-only progression double. Production's
                         # completion gap cannot yield signed acceptance.
                         patch.object(s, "completion", return_value=None),
                         patch.object(s, "smoke_plan", return_value=(b"original", b"derived-private-plan")),
                         patch.object(s.tempfile, "mkdtemp", side_effect=self.private_launch)]
        for p in self.patchers:
            p.start()
            self.addCleanup(p.stop)

    def private_launch(self, **_):
        path = self.root / "private-launch"
        path.mkdir(mode=0o700)
        return str(path)

    def run_parent(self, recheck=lambda: None):
        files = {"agent-build.json": type("Manifest", (), {"value": {"xctestrun": {"path": "original.xctestrun"}}})()}
        return s.execute(str(self.root), RUN, object(), self.expected, self.header, files,
            {"path": "/generated-private-app-root"}, {}, recheck, self.tree, self.fixture, self.fixture,
            self.frozen, self.projection, self.local)

    def test_successors_follow_both_durable_prefix_copies_and_original_join(self):
        self.run_parent()
        events = self.transport.events
        self.assertEqual(events[:2], ["prepare", "launch"])
        self.assertEqual(events[2:-2], ["append", "poll", "retain"] * 10)
        self.assertEqual(events[-2:], ["finish", "original-transport-joined"])
        self.assertEqual((self.local_path / "commands.jsonl").read_bytes(), b"".join(self.commands))
        self.assertEqual((self.local_path / "steps.jsonl").read_bytes(), self.transport.journal)
        self.assertEqual(len(self.transport.journal.splitlines()), 21)
        public = b"".join(p.read_bytes() for p in self.local_path.iterdir())
        self.assertNotIn(self.projection.private_bytes(), public)
        self.assertNotIn(b'"accounts"', public)
        self.assertNotIn(b"/generated-private-app-root", public)
        private_file = self.support / "Meguri/CloudKitSignedPrivate" / RUN / "mac1/expected-accounts.json"
        self.assertEqual(private_file.read_bytes(), self.projection.private_bytes())
        self.assertEqual(private_file.stat().st_mode & 0o777, 0o600)
        self.assertEqual(private_file.parent.stat().st_mode & 0o777, 0o700)

    def test_failed_terminal_and_changed_prefix_never_deliver_successor_and_join_original(self):
        self.fail_at = 3
        with self.assertRaises(s.v.Refused): self.run_parent()
        self.assertEqual(len(self.transport.commands), 3)
        self.assertNotIn("finish", self.transport.events)
        self.assertEqual(self.transport.events[-1], "original-transport-joined")
        self.assertEqual(json.loads((self.local_path / "steps.jsonl").read_bytes().splitlines()[-1])["kind"], "failed")

    def test_changed_prefix_keeps_the_original_durable_host_prefix(self):
        self.change_at = 3
        with self.assertRaises(s.v.Refused): self.run_parent()
        self.assertEqual(len(self.transport.commands), 3)
        self.assertEqual(len((self.local_path / "steps.jsonl").read_bytes().splitlines()), 4)
        self.assertEqual(self.transport.events[-1], "original-transport-joined")

    def test_changed_source_receipt_or_root_at_prelaunch_refuses_before_first_command(self):
        def recheck():
            if self.transport.events:
                raise s.v.Refused("changed-authority")
        with self.assertRaises(s.v.Refused): self.run_parent(recheck)
        self.assertEqual(self.transport.events, ["prepare", "original-transport-joined"])
        self.assertFalse((self.local_path / "commands.jsonl").exists())

    def test_private_preexisting_or_changed_bytes_refuse_without_replacing(self):
        original = s.materialize_private(str(self.support), self.header.raw, self.projection.private_bytes())
        self.addCleanup(original.close)
        with self.assertRaises(s.v.Refused):
            s.materialize_private(str(self.support), self.header.raw, self.projection.private_bytes())
        Path(original.path).write_bytes(private(self.header.raw).replace(b"103", b"104"))
        with self.assertRaises(s.v.Refused): original.check()


class GuestSupervisorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.support = self.root / "support"
        self.support.mkdir(mode=0o700)
        self.run = self.support / "Meguri/CloudKitSigned" / RUN / "mac1"
        self.fixture_files = {"manifest.json": b"{}\n", "collection.anki2": b"generated-DB"}
        self.fixture_files.update({"collection.media/generated-%d.bin" % n: bytes([n])*10240 for n in range(5)})
        fmap = {n: {"sha256": s.v.digest(b), "bytes": len(b)} for n, b in self.fixture_files.items()}
        self.frozen = raw({"protocol_version": 1, "run_id": RUN, "device": "mac1", "source_commit": SHA,
                           "base": "small", "parent_files": fmap, "prepared_source": "/generated-host-fixture", "files": fmap})
        h = header()
        h["fixture_delivery"] = {"source": "/generated-host-fixture", "frozen_input": {
            "sha256": s.v.digest(self.frozen), "bytes": len(self.frozen)}}
        self.header_raw = raw(h)
        evidence = {"run.json": self.header_raw, **{name: b"{}\n" for name in s.h.ROLES}}
        metadata = {n: {"sha256": s.v.digest(b), "bytes": len(b)} for n, b in evidence.items()}
        stream = io.BytesIO(raw(metadata) + b"".join(evidence[n] for n in ("run.json", *s.h.ROLES)))
        self.support_state = s.h.guest_support(root=str(self.support))
        copied = s.h.guest_evidence(str(self.support), RUN, "mac1", stream, True, self.support_state)
        self.roles = {"fixture-input.json": self.frozen, "fixture-evidence/fixtures-result.json": b"{}\n",
                      "fixture-evidence/generation.json": b"{}\n",
                      **{"fixture-evidence/parent/" + n: b for n, b in self.fixture_files.items()}}
        products = self.root / "products"
        products.mkdir(mode=0o700)
        (products / "original.xctestrun").write_bytes(b"original")
        self.prepare = {"action": "prepare", "support": self.support_state, "copied": copied,
            "header": self.header_raw.decode(), "manifest": {"products": {"path": str(products)},
                "xctestrun": {"path": "original.xctestrun"}},
            "files": {n: {"sha256": s.v.digest(b), "bytes": len(b)} for n, b in self.roles.items()}}
        self.payload = b"".join(self.roles[n] for n in sorted(self.roles)) + private(self.header_raw)
        self.commands = s.command_bytes(self.header_raw)
        self.journal = pair(self.commands, 1, 1)
        self.events = []
        test = self
        class OriginalChild:
            returncode = None
            def poll(self): return self.returncode
            def wait(self, **_):
                test.events.append("original-child-wait")
                self.returncode = 0
        self.process = OriginalChild()

    def launch(self, args, **kwargs):
        self.assertFalse((self.run / "data").exists())
        self.assertTrue((self.run / "fixture-input.json").exists())
        self.assertEqual((self.run / "fixture-input.json").read_bytes(), self.frozen)
        self.assertIn("-only-testing:MeguriSnapshotTests/CloudKitSoakTests", args)
        self.assertNotIn("MEGURI_CLOUD_AGENT_RUN_DIRECTORY", kwargs["env"])
        self.events.append("original-child-launch")
        (self.run / "steps.jsonl").write_bytes(self.journal)
        return self.process

    def exchange_frames(self, operations):
        wire, output = io.BytesIO(), io.BytesIO()
        s.frame_write(wire, self.prepare, self.payload)
        for action, payload in operations:
            s.frame_write(wire, {"action": action}, payload)
        wire.seek(0)
        stdin = type("Input", (), {"buffer": wire})()
        stdout = type("Output", (), {"buffer": output})()
        stage = self.root / "private-guest-stage"
        stage.mkdir(mode=0o700)
        with patch.object(s.sys, "stdin", stdin), patch.object(s.sys, "stdout", stdout), \
             patch.object(s, "smoke_plan", return_value=(b"original", b"derived")), \
             patch.object(s.tempfile, "mkdtemp", return_value=str(stage)), \
             patch.object(s.subprocess, "Popen", side_effect=self.launch), \
             patch.object(s.h, "setup_command", return_value=raw({"passedTests": 1, "failedTests": 0, "skippedTests": 0})), \
             patch.object(s.h, "setup_join", side_effect=lambda child: self.assertIs(child, self.process)) as join:
            try:
                s.guest_smoke()
            finally:
                self.join_count = join.call_count
        output.seek(0)
        frames = []
        while output.tell() < len(output.getvalue()):
            frames.append(s.frame_read(output))
        return frames

    def test_real_file_delivery_no_data_private_permissions_and_original_process_join(self):
        frames = self.exchange_frames([("launch", b""), ("append", self.commands[0]),
                                 ("retain", self.journal), ("poll", b""), ("finish", b"")])
        self.assertEqual(self.events, ["original-child-launch", "original-child-wait"])
        self.assertEqual(self.join_count, 0)
        self.assertEqual(frames[-1][1], self.journal)
        self.assertEqual((self.run / "commands.jsonl").read_bytes(), self.commands[0])
        self.assertEqual((self.run / "retained-steps.jsonl").read_bytes(), self.journal)
        self.assertFalse((self.run / "data").exists())
        for n, b in self.roles.items(): self.assertEqual((self.run / n).read_bytes(), b)
        expected = self.support / "Meguri/CloudKitSignedPrivate" / RUN / "mac1/expected-accounts.json"
        self.assertEqual(expected.read_bytes(), private(self.header_raw))
        self.assertEqual(expected.stat().st_mode & 0o777, 0o600)
        public = b"".join(p.read_bytes() for p in self.run.rglob("*") if p.is_file())
        self.assertNotIn(private(self.header_raw), public)
        self.assertNotIn(b'"accounts"', public)

    def test_protocol_failure_joins_the_original_child_without_second_launch(self):
        with self.assertRaises(s.v.Refused):
            self.exchange_frames([("launch", b""), ("launch", b"")])
        self.assertEqual(self.events, ["original-child-launch"])
        self.assertEqual(self.join_count, 1)

    def test_changed_root_and_partial_existing_inputs_refuse_before_launch(self):
        (self.run / "fixture-input.json").write_bytes(b"partial")
        with self.assertRaises(s.v.Refused): self.exchange_frames([])
        self.assertEqual(self.events, [])
        self.assertEqual((self.run / "fixture-input.json").read_bytes(), b"partial")

    def test_changed_root_identity_refuses_before_input_or_launch(self):
        self.prepare["support"]["chain"][-1][1] += 1
        with self.assertRaises(s.v.Refused): self.exchange_frames([])
        self.assertFalse((self.run / "fixture-input.json").exists())
        self.assertEqual(self.events, [])


if __name__ == "__main__":
    unittest.main()
