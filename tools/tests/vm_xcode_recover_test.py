#!/usr/bin/env python3
"""Synthetic Meguri evidence and generated wrappers only; no real VM commands.

The admitted original helper is parsed/hash-checked, never imported/executed.
Integrator runs this suite through MG; filtered authors only write it.
"""
import contextlib
import ctypes
import errno
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import struct
import subprocess
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("recovery", Path(__file__).parents[1] / "scripts/vm-xcode-recover.py")
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)
CANARY = "generated-private-canary-94582"


def write(path, value):
    path = Path(path)
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_bytes(value if isinstance(value, bytes) else m.encoded(value))
    path.chmod(0o600)


def snapshot(path):
    return {str(p.relative_to(path)): p.read_bytes() for p in Path(path).rglob("*") if p.is_file()}


class FakeSystem(m.System):
    def __init__(self, fixture):
        self.fixture, self.events = fixture, []
        self.running = True
        self.process_table = {}
        self.fail_at = None
        self.hook = lambda _: None

    def event(self, name):
        self.events.append(name)
        self.hook(name)
        if self.fail_at == name:
            raise m.Refused()

    def processes(self):
        self.event("host")
        return self.process_table

    def guest(self, selection):
        self.fixture.assertEqual(selection, self.fixture.vm)
        self.fixture.assertTrue(self.running)
        self.event("guest")
        return {"guest_managed": 0, "simulator_leases": 0, "inspection_complete": True}

    def state(self, selection):
        self.fixture.assertEqual(selection, self.fixture.vm)
        self.event("running-state" if self.running else "stopped-state")
        return ("running" if self.running else "stopped", {"Name": self.fixture.vm["name"], "Synthetic": CANARY})

    def stop(self, selection):
        self.fixture.assertEqual(selection, self.fixture.vm)
        self.fixture.assertTrue(self.fixture.guard.is_dir())
        self.fixture.assertTrue((self.fixture.guard / "recovery/claim.json").is_file())
        self.fixture.assertFalse((Path(self.fixture.recovery.evidence) / "recovery-proof.json").exists())
        self.event("stop")
        self.running = False

    def archive(self, source_fd, source_name, target_fd):
        proof = Path(self.fixture.recovery.evidence) / "recovery-proof.json"
        self.fixture.assertTrue(proof.is_file())
        self.fixture.assertTrue(self.fixture.recovery.proved)
        self.fixture.assertFalse(self.running)
        self.event("archive")
        # Exercise the real no-replace filesystem primitive on synthetic dirs.
        return super().archive(source_fd, source_name, target_fd)


class RecoveryCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.root = self.base / "checkout"
        self.lease_base = self.root / "build/mg"
        self.temp_root = self.base / "temp"
        self.temp_root.mkdir(mode=0o700)
        self.config = self.base / "config.json"
        self.vm = {"name": "MeguriGeneratedOnly", "ssh": str(self.base / "fake-ssh"), "tart": str(self.base / "fake-tart")}
        for key in ("ssh", "tart"):
            write(self.vm[key], b"#!/bin/sh\nexit 93\n")
            Path(self.vm[key]).chmod(0o700)
        write(self.config, {"vms": [self.vm]})
        self.lease = self.lease_base / ("mac-vm." + self.vm["name"] + ".lock")
        self.guard = self.lease / "vm-xcode-pending"
        self.guard.mkdir(mode=0o700, parents=True)
        write(str(self.lease) + ".mutex", b"")
        self.holder = {"job": "vm-xcode-stage", "run_id": m.RUN, "pid": 17001,
                       "worktree": str(self.root), "started_at": "2026-11-02T20:00:56Z"}
        write(self.lease / "holder.json", self.holder)
        self.private = self.temp_root / "meguri-vm-xcode.ABC123"
        self.private.mkdir(mode=0o700)
        (self.private / "state").mkdir(mode=0o700)
        self.context = {"selection": dict(self.vm, state="running"), "lease_slot": str(self.lease),
                        "root": str(self.root), "run": m.RUN, "private": str(self.private)}
        write(self.private / "context.json", self.context)
        write(self.private / "worker.pid", b"17002\n")
        write(self.private / "helpers.log", (CANARY + " normal private shell message\n").encode() + m.encoded(m.DIAGNOSTIC))
        self.receipt_dir = self.root / "build/mg/vm-xcode-stage" / m.RUN
        self.result = {"job": "vm-xcode-stage", "run_id": m.RUN, "root": str(self.root), "status": "error",
                       "dry_run": False, "args": [], "commit": {"sha": m.COMMIT, "dirty": False},
                       "finished_at": "2026-11-02T20:02:40Z", "summary": dict.fromkeys(("total", "passed", "failed", "skipped"), 0),
                       "artifacts": [{"kind": "json", "path": str(self.receipt_dir / "stage-report.json")}]}
        write(self.receipt_dir / "result.json", self.result)
        write(self.receipt_dir / "stage-report.json", {"scope": m.SCOPE, "outcome": "refused", "checks": {}})
        self.route = {"root": str(self.root), "config": str(self.config), "selection": self.vm,
                      "lease_base": str(self.lease_base), "temp_root": str(self.temp_root), "recovery_run": "20261103T010203Z-12345"}
        self.system = FakeSystem(self)
        self.recovery = m.Recovery(self.route, self.system)
        self.addCleanup(self.recovery.pins.close)
        dead = patch.object(m, "dead", return_value=True)
        dead.start()
        self.addCleanup(dead.stop)
        self.original_private = snapshot(self.private)
        self.original_receipt = snapshot(self.receipt_dir)
        self.original_holder = (self.lease / "holder.json").read_bytes()

    def run_recovery(self, mode="--stop-and-archive"):
        return self.recovery.execute(mode)

    def refuses(self):
        with self.assertRaises((m.Refused, OSError, ValueError, KeyError, TypeError, m.Cancelled)):
            self.run_recovery()
        self.assertTrue(self.guard.is_dir())
        self.assertNotIn("archive", self.system.events)
        return self.system.events

    def test_positive_order_and_original_bytes(self):
        real_write = m.write_new
        def journal(fd, name, value):
            self.system.events.append("write:" + name)
            return real_write(fd, name, value)
        with patch.object(m, "write_new", side_effect=journal):
            report = self.run_recovery()
        events = self.system.events
        self.assertEqual(events.count("stop"), 1)
        stop = events.index("stop")
        proof = events.index("write:recovery-proof.json")
        archive = events.index("archive")
        self.assertLess(events.index("guest"), stop)
        self.assertLess(stop, events.index("stopped-state"))
        self.assertLess(events.index("stopped-state"), proof)
        self.assertLess(proof, len(events) - 1)
        self.assertLess(max(i for i, event in enumerate(events) if event == "host"), archive)
        self.assertEqual(events[-1], "archive")  # no later VM operation
        self.assertEqual(report["outcome"], "archived")
        self.assertTrue(all(report["checks"].values()))
        archive_path = Path(self.recovery.evidence) / "archived-lease"
        self.assertFalse(self.lease.exists())
        self.assertEqual((archive_path / "holder.json").read_bytes(), self.original_holder)
        self.assertTrue((archive_path / "vm-xcode-pending/recovery/claim.json").is_file())
        self.assertEqual(snapshot(self.private), self.original_private)
        self.assertEqual(snapshot(self.receipt_dir), self.original_receipt)
        self.assertNotIn(CANARY, json.dumps(report))
        self.assertNotIn(str(self.base), json.dumps(report))
        self.assertFalse((self.private / "lease-safe.json").exists())
        proof_value = json.loads((Path(self.recovery.evidence) / "recovery-proof.json").read_bytes())
        self.assertEqual(proof_value["original_query_completion"], "unknown")
        self.assertIsNone(proof_value["historical_identity"])
        self.assertEqual(Path(self.recovery.evidence).stat().st_mode & 0o777, 0o700)

    def test_inspect_does_not_modify_original_or_claim(self):
        before = snapshot(self.lease)
        report = self.run_recovery("--inspect")
        self.assertEqual(report["outcome"], "inspected")
        self.assertEqual(snapshot(self.lease), before)
        self.assertEqual(list(self.guard.iterdir()), [])
        self.assertEqual(snapshot(self.private), self.original_private)
        self.assertNotIn("stop", self.system.events)
        self.assertNotIn("archive", self.system.events)

    def test_exact_source_static_admission(self):
        raw = (Path(__file__).parent / "fixtures/vm_xcode_94582_original_helper.py").read_bytes()
        self.assertTrue(m.source_admission(raw)["first_ioreg_before_prepare"])
        with self.assertRaises(m.Refused):
            m.source_admission(raw.replace(b'IOPlatformExpertDevice', b'IOPlatformOtherDevice', 1))
        for bad in (raw[:-1], raw + b"\n"):
            with self.assertRaises(m.Refused):
                m.source_admission(bad)

    def test_wrong_receipt_job_run_commit_status_or_checks(self):
        for key, value in (("job", "other"), ("run_id", "other"), ("status", "pass"),
                           ("commit", {"sha": "0" * 40}), ("args", ["--check-source"]),
                           ("summary", {"total": 1}), ("finished_at", "2026-11-03T00:00:00Z")):
            with self.subTest(key=key):
                changed = dict(self.result, **{key: value})
                write(self.receipt_dir / "result.json", changed)
                self.refuses()
                self.assertNotIn("stop", self.system.events)
                # New Recovery pins each independent refusal scenario.
                self.recovery.pins.close()
                self.recovery = m.Recovery(self.route, self.system)
                self.addCleanup(self.recovery.pins.close)
        write(self.receipt_dir / "result.json", self.result)

    def test_wrong_diagnostic_refuses_before_stop(self):
        value = dict(m.DIAGNOSTIC, exception="Uncertain")
        write(self.private / "helpers.log", m.encoded(value))
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_duplicate_malformed_and_substring_diagnostics(self):
        for value in (m.encoded(m.DIAGNOSTIC) * 2, b"Refused\n", b'{"exception":\n',
                      b'normal\n', b'{"exception":"Refused","exception":"Refused"}\n',
                      m.encoded(dict(m.DIAGNOSTIC, stack=m.DIAGNOSTIC["stack"][:-1]))):
            with self.subTest(value=value):
                with self.assertRaises((m.Refused, ValueError)):
                    m.diagnostic_admission(value)

    def test_wrong_context(self):
        write(self.private / "context.json", dict(self.context, selection=dict(self.vm, name="Foreign", state="running")))
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_ambiguous_context(self):
        other = self.temp_root / "meguri-vm-xcode.DEF456"
        write(other / "context.json", dict(self.context, private=str(other)))
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def historical_stage(self, suffix="OLD001"):
        directory = self.temp_root / ("meguri-vm-xcode." + suffix)
        context = dict(self.context, run="20261101T010203Z-10000", private=str(directory))
        write(directory / "context.json", context)
        return directory, context

    def assert_discovery_archived(self):
        report = self.run_recovery()
        self.assertEqual(report["outcome"], "archived")
        self.assertTrue(all(report["checks"].values()))
        self.assertEqual(self.system.events.count("stop"), 1)
        self.assertEqual(self.system.events[-1], "archive")
        self.assertGreaterEqual(self.system.events.count("guest"), 2)
        self.assertGreaterEqual(self.system.events.count("host"), 3)
        proof = json.loads((Path(self.recovery.evidence) / "recovery-proof.json").read_bytes())
        self.assertTrue(proof["original_stop_completed_and_joined"])
        self.assertEqual(proof["independent_state"], "stopped")
        self.assertEqual(proof["original_query_completion"], "unknown")
        self.assertTrue(all(value == 0 for value in proof["no_active_work"].values()
                            if type(value) is int))
        context_pin = next(pin for pin in proof["pins"] if pin["path"] == str(self.private / "context.json"))
        self.assertEqual(context_pin["pin"], list(m.file_identity((self.private / "context.json").stat())))
        self.assertEqual(context_pin["sha256"], m.sha(m.encoded(self.context)))
        self.assertEqual(snapshot(self.private), self.original_private)
        self.assertEqual(snapshot(self.receipt_dir), self.original_receipt)
        self.assertNotIn(str(self.temp_root), self.recovery.pins.inventories)
        return proof

    def test_more_than_8192_unrelated_names_admit_with_original_proofs(self):
        for number in range(8193):
            (self.temp_root / ("generated-unrelated-%05d" % number)).touch(mode=0o600)
        # Near matches are unrelated too; none may be opened as stage dirs.
        for name in ("meguri-vm-xcode.ABC12", "meguri-vm-xcode.ABC1234", "meguri-vm-xcode.ABC_12"):
            write(self.temp_root / name, b"generated unrelated entry")
        self.assert_discovery_archived()
        self.assertEqual(self.recovery.pins.checks[str(self.temp_root)], {self.private.name})
        self.assertFalse(any(name.startswith("generated-unrelated-") for _, name, *_ in self.recovery.pins.entries))

    def test_unrelated_name_churn_through_stop_and_proof_is_tolerated(self):
        unrelated = self.temp_root / "generated-unrelated-changing"
        def churn(event):
            if unrelated.exists():
                unrelated.unlink()
            else:
                write(unrelated, b"generated unrelated entry")
        self.system.hook = churn
        self.assert_discovery_archived()
        self.assertEqual(self.recovery.pins.checks[str(self.temp_root)], {self.private.name})
        self.assertFalse(any(name == unrelated.name for _, name, *_ in self.recovery.pins.entries))

    def test_contextless_stage_is_only_ignored_as_candidate(self):
        other = self.temp_root / "meguri-vm-xcode.EMPTY1"
        other.mkdir(mode=0o700)
        self.assert_discovery_archived()
        self.assertEqual(self.recovery.pins.checks[str(self.temp_root)], {self.private.name, other.name})
        self.assertIn((self.recovery.pins.directories[str(other)], "context.json"), self.recovery.pins.absences)

    def test_seven_different_runs_sharing_lease_admit_with_context_pins(self):
        historical = [self.historical_stage("OLD%03d" % n) for n in range(7)]
        proof = self.assert_discovery_archived()
        self.assertEqual(len(self.recovery.pins.checks[str(self.temp_root)]), 8)
        for directory, context in historical:
            pin = next(pin for pin in proof["pins"] if pin["path"] == str(directory / "context.json"))
            self.assertEqual(pin["sha256"], m.sha(m.encoded(context)))
            self.assertEqual(pin["pin"], list(m.file_identity((directory / "context.json").stat())))

    def test_exact_run_requires_every_full_binding_field(self):
        self.historical_stage()
        for key, value in (("lease_slot", str(self.base / "other-lease")),
                           ("root", str(self.base / "other-root")),
                           ("private", str(self.base / "other-private")),
                           ("selection", dict(self.vm, state="stopped"))):
            with self.subTest(key=key):
                write(self.private / "context.json", dict(self.context, **{key: value}))
                self.refuses()
                self.assertNotIn("stop", self.system.events)
                self.recovery.pins.close()
                self.recovery = m.Recovery(self.route, self.system)
                self.addCleanup(self.recovery.pins.close)

    def test_present_unrelated_context_must_be_readable_valid_run_object(self):
        other, context = self.historical_stage()
        for raw in (b'{"run":', b'{"run":"one","run":"two"}', b'[]', b'{}',
                    m.encoded(dict(context, run=None)), b"x" * 65537):
            with self.subTest(raw_size=len(raw)):
                write(other / "context.json", raw)
                self.refuses()
                self.assertNotIn("stop", self.system.events)
                self.recovery.pins.close()
                self.recovery = m.Recovery(self.route, self.system)
                self.addCleanup(self.recovery.pins.close)

    def test_present_unrelated_context_permission_error_is_not_absence(self):
        other, _ = self.historical_stage()
        original = m.os.open
        def unreadable(name, flags, *args, **kwargs):
            if name == "context.json" and kwargs.get("dir_fd") == self.recovery.pins.directories.get(str(other)):
                raise PermissionError(errno.EACCES, "generated unreadable context")
            return original(name, flags, *args, **kwargs)
        with patch.object(m.os, "open", side_effect=unreadable):
            self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_present_unrelated_context_symlink_is_not_absence(self):
        other, _ = self.historical_stage()
        (other / "context.json").unlink()
        (other / "context.json").symlink_to("generated-missing")
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_present_unrelated_context_nonregular_refuses(self):
        other, _ = self.historical_stage()
        (other / "context.json").unlink()
        (other / "context.json").mkdir(mode=0o700)
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_relevant_name_overflow_refuses_without_control(self):
        for number in range(64):
            (self.temp_root / ("meguri-vm-xcode.N%05d" % number)).mkdir(mode=0o700)
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_relevant_enumeration_time_is_bounded_even_for_unrelated_names(self):
        write(self.temp_root / "generated-unrelated", b"generated entry")
        original = m.os.scandir
        clock = [0.0]
        @contextlib.contextmanager
        def slow(fd):
            with original(fd) as entries:
                def delayed():
                    for entry in entries:
                        if fd == self.recovery.pins.directories.get(str(self.temp_root)) and \
                                entry.name == "generated-unrelated":
                            clock[0] += m.DISCOVERY_SECONDS
                        yield entry
                yield delayed()
        with patch.object(m.os, "scandir", side_effect=slow), \
                patch.object(m.time, "monotonic", side_effect=lambda: clock[0]):
            self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_relevant_enumeration_completion_must_meet_time_bound(self):
        original = m.os.scandir
        clock = [0.0]
        @contextlib.contextmanager
        def slow_close(fd):
            with original(fd) as entries:
                yield entries
            if fd == self.recovery.pins.directories.get(str(self.temp_root)):
                clock[0] += m.DISCOVERY_SECONDS
        with patch.object(m.os, "scandir", side_effect=slow_close), \
                patch.object(m.time, "monotonic", side_effect=lambda: clock[0]):
            self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_relevant_enumeration_timeout_on_pin_recheck_refuses(self):
        self.recovery.admit()
        clock = [0.0]
        def elapsed():
            clock[0] += m.DISCOVERY_SECONDS
            return clock[0]
        with patch.object(m.time, "monotonic", side_effect=elapsed), self.assertRaises(m.Refused):
            self.recovery.pins.check()
        self.assertNotIn("stop", self.system.events)
        self.assertNotIn("archive", self.system.events)
        self.assertTrue(self.guard.is_dir())

    def test_relevant_name_added_between_discovery_and_first_file_pin_refuses(self):
        original = self.recovery.pins.freeze_stages
        def change(path):
            names = original(path)
            (self.temp_root / "meguri-vm-xcode.NEW001").mkdir(mode=0o700)
            return names
        with patch.object(self.recovery.pins, "freeze_stages", side_effect=change):
            self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_relevant_name_removed_before_stop_refuses(self):
        other, _ = self.historical_stage()
        def change(event):
            if event == "guest":
                other.rename(self.temp_root / "generated-retained")
        self.system.hook = change
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_context_absence_cannot_turn_into_present_context_before_stop(self):
        other = self.temp_root / "meguri-vm-xcode.EMPTY1"
        other.mkdir(mode=0o700)
        def change(event):
            if event == "guest":
                write(other / "context.json", dict(self.context, private=str(other)))
        self.system.hook = change
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_historical_context_cannot_change_run_after_discovery(self):
        other, context = self.historical_stage()
        def change(event):
            if event == "guest":
                write(other / "context.json", dict(context, run=m.RUN))
        self.system.hook = change
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_present_historical_context_cannot_disappear_after_discovery(self):
        other, _ = self.historical_stage()
        def change(event):
            if event == "guest":
                (other / "context.json").unlink()
        self.system.hook = change
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_contextless_and_historical_metadata_do_not_prove_workers_dead(self):
        (self.temp_root / "meguri-vm-xcode.EMPTY1").mkdir(mode=0o700)
        self.historical_stage()
        for live_pid in (self.holder["pid"], 17002):
            with self.subTest(live_pid=live_pid), patch.object(m, "dead", side_effect=lambda pid: pid != live_pid):
                self.refuses()
                self.assertNotIn("stop", self.system.events)
                self.recovery.pins.close()
                self.recovery = m.Recovery(self.route, self.system)
                self.addCleanup(self.recovery.pins.close)

    def test_contextless_and_historical_metadata_do_not_hide_managed_activity(self):
        (self.temp_root / "meguri-vm-xcode.EMPTY1").mkdir(mode=0o700)
        self.historical_stage()
        self.system.process_table = {29001: "python vm-xcode-stage.py"}
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_contextless_and_historical_metadata_do_not_hide_unknown_guest_activity(self):
        (self.temp_root / "meguri-vm-xcode.EMPTY1").mkdir(mode=0o700)
        self.historical_stage()
        self.system.fail_at = "guest"
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_receiver_or_unknown_entries(self):
        write(self.private / "receiver-pending.json", {"pending": True})
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_nested_or_recording_lease(self):
        write(self.lease / "inner/holder.json", self.holder)
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_existing_claim_prevents_second_stop(self):
        (self.guard / "recovery").mkdir()
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_live_reused_unknown_worker(self):
        for result in (False, PermissionError()):
            with self.subTest(result=result):
                kw = {"side_effect": result} if isinstance(result, Exception) else {"return_value": result}
                with patch.object(m, "dead", **kw):
                    self.refuses()
                self.assertNotIn("stop", self.system.events)

    def test_host_activity_or_failed_inspection(self):
        self.system.process_table = {20001: "/generated/tools/mg sim acquire iphone"}
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_guest_activity_failure_keeps_claim(self):
        self.system.fail_at = "guest"
        self.refuses()
        self.assertTrue((self.guard / "recovery/claim.json").is_file())
        self.assertNotIn("stop", self.system.events)

    def test_stop_failure_keeps_intent_claim_and_fence(self):
        self.system.fail_at = "stop"
        self.refuses()
        self.assertEqual(self.system.events.count("stop"), 1)
        self.assertTrue((Path(self.recovery.evidence) / "stop-intent.json").exists())
        self.assertTrue((self.guard / "recovery/claim.json").is_file())

    def test_independent_state_failure(self):
        self.system.fail_at = "stopped-state"
        self.refuses()
        self.assertEqual(self.system.events.count("stop"), 1)

    def test_changed_state_namespace_refuses(self):
        original = self.system.state
        def state(selection):
            current, metadata = original(selection)
            if current == "stopped":
                metadata["Synthetic"] = "changed"
            return current, metadata
        with patch.object(self.system, "state", side_effect=state):
            self.refuses()

    def test_failed_proof_write_keeps_fence(self):
        original = m.write_new
        def fail(fd, name, value):
            if name == "recovery-proof.json":
                raise OSError("generated durable-write failure")
            return original(fd, name, value)
        with patch.object(m, "write_new", side_effect=fail):
            self.refuses()
        self.assertTrue(self.recovery.stopped)
        self.assertFalse(self.recovery.proved)

    def test_failed_proof_directory_fsync_keeps_fence(self):
        original = os.fsync
        def fail(fd):
            if self.recovery.evidence and fd == self.recovery.evidence_fd and \
                    (Path(self.recovery.evidence) / "recovery-proof.json").exists():
                raise OSError("generated fsync failure")
            return original(fd)
        with patch.object(m.os, "fsync", side_effect=fail):
            self.refuses()
        self.assertFalse(self.recovery.proved)

    def test_host_activity_at_actual_stop_boundary_retains_intent_and_fence(self):
        def activate(event):
            if event == "host" and (Path(self.recovery.evidence) / "stop-intent.json").exists():
                self.system.process_table = {29000: "tools/mg sim acquire iphone"}
        self.system.hook = activate
        self.refuses()
        self.assertNotIn("stop", self.system.events)
        self.assertTrue((Path(self.recovery.evidence) / "stop-intent.json").is_file())
        self.assertTrue((self.guard / "recovery/claim.json").is_file())

    def test_final_host_activity_refuses_after_durable_proof(self):
        def activate(event):
            if event == "host" and self.recovery.proved:
                self.system.process_table = {29000: "python vnc-holder.py"}
        self.system.hook = activate
        self.refuses()
        self.assertTrue(self.recovery.proved)

    def test_no_replace_archive_failure_keeps_proof_and_fence(self):
        collision_stats = []
        def collision(event):
            if event == "archive":
                destination = Path(self.recovery.evidence) / "archived-lease"
                destination.mkdir()
                collision_stats.append(destination.stat())
        self.system.hook = collision
        with self.assertRaises(m.Refused):
            self.run_recovery()
        self.assertEqual(self.system.events.count("archive"), 1)
        self.assertTrue(self.lease.is_dir())
        self.assertTrue(self.guard.is_dir())
        self.assertTrue(self.recovery.claimed)
        self.assertTrue((self.guard / "recovery/claim.json").is_file())
        self.assertEqual((self.lease / "holder.json").read_bytes(), self.original_holder)
        self.assertTrue(self.recovery.proved)
        proof = Path(self.recovery.evidence) / "recovery-proof.json"
        self.assertTrue(proof.is_file())
        self.assertNotIn(self.lease, proof.parents)
        self.assertFalse(self.recovery.archived)
        destination = Path(self.recovery.evidence) / "archived-lease"
        self.assertEqual(destination.stat(), collision_stats[0])
        self.assertEqual(list(destination.iterdir()), [])

    def test_cancellation_keeps_claim_and_original_evidence(self):
        def cancel(event):
            if event == "stop":
                raise m.Cancelled()
        self.system.hook = cancel
        self.refuses()
        self.assertEqual(snapshot(self.private), self.original_private)
        self.assertTrue((self.guard / "recovery/claim.json").is_file())

    def test_final_report_error_after_rename_propagates(self):
        report = self.run_recovery()
        with patch.object(m.sys, "platform", "darwin"), \
                patch.object(m.sys, "argv", ["helper", "--stage-run", m.RUN, "--stop-and-archive"]), \
                patch.object(m.os, "dup", return_value=999), \
                patch.object(m.os, "fdopen", return_value=io.BytesIO(m.encoded(self.route))), \
                patch.object(m, "Recovery", return_value=self.recovery), \
                patch.object(self.recovery, "execute", return_value=report), \
                patch("builtins.print", side_effect=OSError("generated publication failure")):
            with self.assertRaises(OSError):
                m.main()
        self.assertTrue(self.recovery.archived)
        self.assertTrue((Path(self.recovery.evidence) / "archived-lease").is_dir())


    def test_hard_linked_context(self):
        os.link(self.private / "context.json", self.base / "link")
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_symlinked_context(self):
        target = self.private / "context.json"
        target.rename(self.private / "saved")
        target.symlink_to("saved")
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_oversized_diagnostic(self):
        write(self.private / "helpers.log", b"x" * (m.LIMIT + 1))
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_changed_evidence_before_stop(self):
        def change(event):
            if event == "guest":
                write(self.private / "context.json", self.context)
        self.system.hook = change
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_changed_claim_after_stop_retains_fence(self):
        def change(event):
            if event == "stopped-state":
                write(self.guard / "recovery/claim.json", {"unknown": True})
        self.system.hook = change
        self.refuses()

    def test_changed_wrapper_at_stop_boundary(self):
        def change(event):
            if event == "running-state" and "guest" in self.system.events:
                write(self.vm["tart"], b"changed generated wrapper")
        self.system.hook = change
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_parent_substitution(self):
        def change(event):
            if event == "guest":
                self.private.rename(self.private.with_name("retained"))
                self.private.mkdir()
        self.system.hook = change
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_fold_metadata_with_live_or_unknown_holder(self):
        fold = self.root / "build/vm-fold"
        fold.mkdir()
        os.mkfifo(fold / "commands")
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_recording_metadata_unknown(self):
        write(self.root / "build/mg/record/active/00000000-0000-0000-0000-000000000000.json", {"unknown": True})
        self.refuses()
        self.assertNotIn("stop", self.system.events)

    def test_second_recovery_cannot_interleave(self):
        other = m.Recovery(dict(self.route, recovery_run="20261103T010204Z-54321"), self.system)
        self.addCleanup(other.pins.close)
        self.recovery.admit()
        other.admit()
        self.recovery.claim()
        with self.assertRaises(m.Refused):
            other.claim()
        self.assertNotIn("stop", self.system.events)

    def test_guest_metadata_and_processes_are_read_only(self):
        pool = self.base / "generated-pool"
        leases = pool / "leases"
        leases.mkdir(mode=0o700, parents=True)
        own = str(os.getpid()) + " 0 generated-reader\n"
        cases = ((own, True), (own + "29001 1 xcodebuild test-without-building\n", False),
                 (own + "29002 1 /generated/xctest test\n", False),
                 (own + "29003 1 simctl io generated recordVideo file\n", False),
                 ("malformed process listing\n", False))
        def guest_read(rows):
            output = io.StringIO()
            def ps(args, **kwargs):
                self.assertEqual(args[0], "/bin/ps")
                kwargs["stdout"].write(rows.encode())
                kwargs["stdout"].flush()
            with patch.object(subprocess, "run", side_effect=ps), \
                    patch.object(os.path, "expanduser", return_value=str(pool)), \
                    contextlib.redirect_stdout(output):
                try:
                    exec(compile(m.GUEST_READ, "generated-guest-read", "exec"), {})
                except SystemExit as error:
                    self.assertEqual(error.code, 1)
            return json.loads(output.getvalue())["inspection_complete"]
        for rows, expected in cases:
            self.assertEqual(guest_read(rows), expected)
        udid = "00000000-0000-0000-0000-000000000000"
        lease = {"udid": udid, "kind": "iphone", "holder": "generated", "worktree": "/generated",
                 "cwd": "/generated", "pid": 29000, "ttl_min": 60, "acquired_at": "generated",
                 "expires_at": "generated", "acquired_epoch": 1, "expires_epoch": 2**40}
        write(leases / udid / "lease.json", lease)
        before = snapshot(pool)
        self.assertFalse(guest_read(own))  # active simulator lease
        self.assertEqual(snapshot(pool), before)
        lease["expires_epoch"] = 2  # positive source-owned expiry; no GC/expire call
        write(leases / udid / "lease.json", lease)
        self.assertTrue(guest_read(own))
        (pool / "pool.lock").mkdir()
        self.assertFalse(guest_read(own))


class PrivateFailureCase(unittest.TestCase):
    def diagnostic(self, error):
        public, private = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(public), contextlib.redirect_stderr(private):
            m.private_failure(error)
        self.assertEqual(public.getvalue(), "")
        self.assertEqual(len(private.getvalue().splitlines()), 1)
        self.assertLessEqual(len(private.getvalue()), 2048)
        record = json.loads(private.getvalue())
        self.assertIn(set(record), ({"exception", "stack"}, {"exception", "stack", "errno"}))
        self.assertIs(type(record["exception"]), str)
        self.assertIs(type(record["stack"]), list)
        self.assertLessEqual(len(record["stack"]), 8)
        for frame in record["stack"]:
            self.assertEqual(set(frame), {"function", "line"})
            self.assertIs(type(frame["function"]), str)
            self.assertRegex(frame["function"], r"\A(?:[A-Za-z_][A-Za-z0-9_]{0,63}|<(?:module|lambda|listcomp|dictcomp|setcomp|genexpr)>)\Z")
            self.assertIs(type(frame["line"]), int)
            self.assertTrue(1 <= frame["line"] <= 1000000)
        if "errno" in record:
            self.assertIs(type(record["errno"]), int)
            self.assertTrue(0 <= record["errno"] <= 4095)
        self.assertNotIn(CANARY, private.getvalue())
        return record

    def test_finite_exact_class_mapping_and_static_fallback(self):
        cases = ((m.Refused(), "Refused"), (m.Cancelled(), "Cancelled"),
                 (OSError(), "OSError"), (PermissionError(), "PermissionError"),
                 (FileNotFoundError(), "FileNotFoundError"),
                 (ProcessLookupError(), "ProcessLookupError"),
                 (ChildProcessError(), "ChildProcessError"), (TimeoutError(), "TimeoutError"),
                 (subprocess.TimeoutExpired(CANARY, 1), "TimeoutExpired"),
                 (ValueError(), "ValueError"), (TypeError(), "TypeError"),
                 (KeyError(), "KeyError"), (RuntimeError(), "RuntimeError"),
                 (json.JSONDecodeError(CANARY, CANARY, 0), "JSONDecodeError"),
                 (type(CANARY, (m.Refused,), {})(CANARY), "Other"),
                 (Exception(CANARY), "Other"))
        for error, expected in cases:
            with self.subTest(expected=expected):
                self.assertEqual(self.diagnostic(error), {"exception": expected, "stack": []})

    def test_errno_requires_exact_bounded_integer(self):
        for number in (0, 1, 4095, -1, 4096, 2**100, True, 1.0, "1", None,
                       type("GeneratedInt", (int,), {})(1)):
            error = OSError(CANARY)
            error.errno = number
            record = self.diagnostic(error)
            expected = type(number) is int and 0 <= number <= 4095
            self.assertEqual("errno" in record, expected)
            if expected:
                self.assertEqual(record["errno"], number)
        error = ValueError(CANARY)
        error.errno = 1
        self.assertNotIn("errno", self.diagnostic(error))

    @staticmethod
    def generated_failure(filename, depth=0, name="generated_leaf"):
        # Compiler frames only: no file access or real helper operations.
        source = "def %s():\n    raise error\n" % name
        previous = name
        for number in range(depth):
            current = "generated_frame_%d" % number
            source += "def %s():\n    %s()\n" % (current, previous)
            previous = current
        namespace = {"error": ValueError(CANARY + "/generated/private/path")}
        exec(compile(source, filename, "exec"), namespace)
        try:
            namespace[previous]()
        except ValueError as error:
            return error

    def test_fixed_source_filter_and_innermost_frame_cap(self):
        for filename in ("vm-xcode-recover.py", "/generated/vm-xcode-recover.py",
                         m.PRIVATE_SOURCE + ".other", "/generated/" + m.PRIVATE_SOURCE,
                         CANARY + "/vm-xcode-recover.py"):
            self.assertEqual(self.diagnostic(self.generated_failure(filename))["stack"], [])
        record = self.diagnostic(self.generated_failure(m.PRIVATE_SOURCE, depth=12))
        self.assertEqual(len(record["stack"]), 8)
        self.assertEqual([frame["function"] for frame in record["stack"]],
                         ["generated_frame_%d" % number for number in range(6, -1, -1)] + ["generated_leaf"])
        self.assertEqual(record["stack"][-1], {"function": "generated_leaf", "line": 2})
        self.assertEqual(self.diagnostic(self.generated_failure(m.PRIVATE_SOURCE, name="g" * 65))["stack"], [])
        try:
            m.need(False)
        except m.Refused as error:
            self.assertEqual(self.diagnostic(error)["stack"],
                             [{"function": "need", "line": m.need.__code__.co_firstlineno + 2}])

    def test_message_path_and_exception_rendering_are_never_used(self):
        class GeneratedError(OSError):
            def __str__(self):
                raise AssertionError("exception string accessed")

            def __repr__(self):
                raise AssertionError("exception repr accessed")
        error = GeneratedError(1, CANARY, "/generated/" + CANARY)
        self.assertEqual(self.diagnostic(error), {"exception": "Other", "errno": 1, "stack": []})

    def invoke(self, failure=None, phase="execute", sink_failure=None, close_failure=None,
               publish_failure=False):
        recovery = SimpleNamespace(execute=unittest.mock.Mock(return_value={"outcome": "inspected"}),
                                   report=unittest.mock.Mock(return_value={"outcome": "refused"}),
                                   pins=SimpleNamespace(close=unittest.mock.Mock(side_effect=close_failure)))
        constructor = unittest.mock.Mock(return_value=recovery)
        if phase == "execute":
            recovery.execute.side_effect = failure
        elif phase == "constructor":
            constructor.side_effect = failure
        public, private = io.StringIO(), io.StringIO()
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(m.sys, "platform", "generated-other" if phase == "platform" else "darwin"))
            stack.enter_context(patch.object(m.sys, "argv", ["helper", "--stage-run", m.RUN, "--inspect"]))
            stack.enter_context(patch.object(m.signal, "signal"))
            dup = stack.enter_context(patch.object(m.os, "dup", return_value=999,
                                                  side_effect=failure if phase == "context" else None))
            stack.enter_context(patch.object(m.os, "fdopen", return_value=io.BytesIO(b"{}")))
            stack.enter_context(patch.object(m, "Recovery", constructor))
            # A missed seam must never inspect a real process or start a child.
            stack.enter_context(patch.object(m, "host_processes", side_effect=AssertionError("real processes")))
            stack.enter_context(patch.object(m.subprocess, "Popen", side_effect=AssertionError("real child")))
            stack.enter_context(contextlib.redirect_stdout(public))
            stack.enter_context(contextlib.redirect_stderr(private))
            if sink_failure:
                stack.enter_context(patch.object(private, "write", side_effect=sink_failure))
            if publish_failure:
                stack.enter_context(patch("builtins.print", side_effect=OSError(CANARY)))
            status = m.run()
        self.assertNotIn(CANARY, public.getvalue() + private.getvalue())
        return status, public.getvalue(), private.getvalue(), recovery, constructor, dup

    def test_inner_refusal_and_cancellation_keep_public_report_and_status(self):
        for error in (m.Refused(CANARY), m.Cancelled(CANARY), OSError(1, CANARY)):
            status, public, private, recovery, _, _ = self.invoke(error)
            self.assertEqual(status, 1)
            self.assertEqual(public, json.dumps({"outcome": "refused"}) + "\n")
            self.assertEqual(len(private.splitlines()), 1)
            self.assertEqual(json.loads(private)["exception"],
                             "Cancelled" if type(error) is m.Cancelled else "Refused" if type(error) is m.Refused else "PermissionError")
            recovery.report.assert_called_once_with("refused")
            recovery.pins.close.assert_called_once_with()

    def test_outer_context_and_constructor_errors_have_no_public_report(self):
        for phase in ("context", "constructor"):
            for error in (m.Refused(CANARY), m.Cancelled(CANARY)):
                status, public, private, recovery, constructor, _ = self.invoke(error, phase)
                self.assertEqual((status, public), (1, ""))
                self.assertEqual(len(private.splitlines()), 1)
                self.assertEqual(json.loads(private)["exception"],
                                 "Cancelled" if type(error) is m.Cancelled else "Refused")
                recovery.execute.assert_not_called()
                recovery.pins.close.assert_not_called()
                if phase == "context":
                    constructor.assert_not_called()

    def test_initial_admission_refusal_retains_own_predicate_location(self):
        status, public, private, recovery, constructor, dup = self.invoke(phase="platform")
        self.assertEqual((status, public), (1, ""))
        self.assertEqual(len(private.splitlines()), 1)
        record = json.loads(private)
        self.assertEqual(record["exception"], "Refused")
        self.assertEqual(record["stack"][-1],
                         {"function": "need", "line": m.need.__code__.co_firstlineno + 2})
        dup.assert_not_called()
        constructor.assert_not_called()
        recovery.pins.close.assert_not_called()

    def test_success_preserves_public_bytes_without_diagnostics(self):
        status, public, private, recovery, _, _ = self.invoke()
        self.assertEqual((status, public, private), (0, json.dumps({"outcome": "inspected"}) + "\n", ""))
        recovery.execute.assert_called_once_with("--inspect")
        recovery.report.assert_not_called()
        recovery.pins.close.assert_called_once_with()

    def test_private_sink_failure_cannot_change_inner_or_outer_refusal(self):
        for phase in ("execute", "context", "constructor"):
            for sink_error in (OSError(CANARY), m.Cancelled(CANARY), SystemExit(0)):
                status, public, private, recovery, _, _ = self.invoke(
                    m.Refused(CANARY), phase, sink_failure=sink_error)
                self.assertEqual(status, 1)
                self.assertEqual(private, "")
                self.assertEqual(public, json.dumps({"outcome": "refused"}) + "\n" if phase == "execute" else "")
                self.assertEqual(recovery.pins.close.call_count, int(phase == "execute"))

    def test_render_and_flush_failures_are_suppressed(self):
        for operation in ("render", "flush"):
            private = io.StringIO()
            with contextlib.redirect_stderr(private), contextlib.ExitStack() as stack:
                if operation == "render":
                    stack.enter_context(patch.object(m.json, "dumps", side_effect=m.Cancelled()))
                else:
                    stack.enter_context(patch.object(private, "flush", side_effect=SystemExit(0)))
                m.private_failure(m.Refused(CANARY))
            self.assertEqual(len(private.getvalue().splitlines()), int(operation == "flush"))

    def test_outer_catch_does_not_duplicate_first_inner_diagnostic(self):
        for extra in ({"close_failure": OSError(CANARY)}, {"publish_failure": True}):
            status, _, private, _, _, _ = self.invoke(m.Refused(CANARY), **extra)
            self.assertEqual(status, 1)
            self.assertEqual(len(private.splitlines()), 1)
            self.assertEqual(json.loads(private)["exception"], "Refused")
        status, _, private, _, _, _ = self.invoke(close_failure=OSError(CANARY))
        self.assertEqual(status, 1)
        self.assertEqual(len(private.splitlines()), 1)


class IsolatedFunctions(unittest.TestCase):
    def test_managed_predicates_and_host_only_exclusions(self):
        selection = {"name": "generated", "ssh": "/generated/ssh", "tart": "/generated/tart"}
        for args in ("tools/mg sim acquire iphone", "python vm-xcode-stage.py", "bash sim-record.sh",
                     "python vnc-holder.py", "/generated/ssh read", "/generated/tart stop generated"):
            with self.subTest(args=args):
                with self.assertRaises(m.Refused):
                    m.host_activity({123456: args}, selection)
        self.assertEqual(sum(m.host_activity({123456: "tools/mg build-app --platform mac"}, selection).values()), 0)
        self.assertEqual(sum(m.host_activity({123456: "tools/mg xcode-check --physical-device generated"}, selection).values()), 0)

    def test_dead_requires_positive_esrch(self):
        with patch.object(m.os, "kill", side_effect=ProcessLookupError):
            self.assertTrue(m.dead(123456))
        with patch.object(m.os, "kill", return_value=None):
            self.assertFalse(m.dead(123456))
        with patch.object(m.os, "kill", side_effect=PermissionError):
            with self.assertRaises(PermissionError):
                m.dead(123456)

    def test_generated_wrapper_exact_arguments_and_no_other_control(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            events = base / "events"
            wrapper = base / "wrapper"
            # This generated wrapper never reaches a VM or executes guest code.
            wrapper.write_text("#!/usr/bin/env python3\nimport json,sys\nfrom pathlib import Path\n"
                               + "p=Path(" + repr(str(events)) + ")\n"
                               + "with p.open('a') as f: f.write(json.dumps(sys.argv[1:])+'\\n')\n"
                               + "a=sys.argv[1:]\n"
                               + "if a==['stop','GeneratedOnly','--timeout','60']: pass\n"
                               + "elif a==['list','--format','json']: print('[{\"Name\":\"GeneratedOnly\",\"State\":\"stopped\"}]')\n"
                               + "elif len(a)==1 and a[0].startswith('/usr/bin/python3 -B -I -c '): print('{\"guest_managed\":0,\"simulator_leases\":0,\"inspection_complete\":true}')\n"
                               + "else: raise SystemExit(94)\n")
            wrapper.chmod(0o700)
            vm = {"name": "GeneratedOnly", "tart": str(wrapper), "ssh": str(wrapper)}
            system = m.System()
            system.guest(vm)
            system.stop(vm)
            self.assertEqual(system.state(vm), ("stopped", {"Name": "GeneratedOnly"}))
            calls = [json.loads(line) for line in events.read_text().splitlines()]
            self.assertEqual(calls[1:], [["stop", "GeneratedOnly", "--timeout", "60"], ["list", "--format", "json"]])


class GeneratedKernel:
    """Only generated C buffers cross the real reader's ctypes call boundaries."""
    owner, own, worker, foreign = 501, 31000, 31001, 31002

    @classmethod
    def record(cls, pid, uid=None, ruid=None, svuid=None, start=100, usec=1):
        uid = cls.owner if uid is None else uid
        ruid = uid if ruid is None else ruid
        svuid = uid if svuid is None else svuid
        # Independent wire-format fixture, including the documented ABI offsets.
        return struct.pack("=12I16s32s6I2Q", 0, 2, 0, pid, 1, uid, 20, ruid, 20, svuid, 20, 0,
                           b"generated", b"generated", 3, pid, 0, 0, 0, 0, start, usec)

    @staticmethod
    def arguments(*words):
        return struct.pack("=i", len(words)) + b"/generated/executable\0\0" + b"\0".join(words) + b"\0K=V\0"

    def __init__(self):
        self.records = {self.own: self.record(self.own), self.worker: self.record(self.worker),
                        self.foreign: self.record(self.foreign, uid=0)}
        self.args = {pid: self.arguments(b"generated-idle") for pid in self.records}
        self.info_errors, self.args_errors = {}, {self.foreign: errno.EPERM}
        self.info_sizes, self.args_sizes = {}, {}
        self.info_calls, self.argv_calls, self.enumerations = {}, [], 0
        self.on_info = lambda pid, count: None
        self.on_argv = lambda pid: None
        self.on_list = lambda count: None
        self.list_size = None
        self.list_rows = None
        self.lib = SimpleNamespace(proc_listpids=unittest.mock.Mock(side_effect=self.listpids),
                                   proc_pidinfo=unittest.mock.Mock(side_effect=self.pidinfo))
        self.libc = SimpleNamespace(sysctl=unittest.mock.Mock(side_effect=self.sysctl))

    def listpids(self, kind, uid, buffer, capacity):
        assert (kind, uid) == (1, 0)
        self.enumerations += 1
        self.on_list(self.enumerations)
        rows = self.list_rows if self.list_rows is not None else list(self.records)
        data = struct.pack("=" + "i" * len(rows), *rows)
        ctypes.memmove(buffer, data, min(len(data), capacity))
        return self.list_size if self.list_size is not None else len(data)

    def pidinfo(self, pid, flavor, arg, buffer, capacity):
        assert (flavor, arg, capacity) == (3, 0, 136)
        self.info_calls[pid] = self.info_calls.get(pid, 0) + 1
        self.on_info(pid, self.info_calls[pid])
        if pid in self.info_errors or pid not in self.records:
            ctypes.set_errno(self.info_errors.get(pid, errno.ESRCH))
            return 0
        data = self.records[pid]
        ctypes.memmove(buffer, data, min(len(data), capacity))
        return self.info_sizes.get(pid, len(data))

    def sysctl(self, mib, count, buffer, size, new, newsize):
        assert (mib[0], mib[1], count, new, newsize) == (1, 49, 3, None, 0)
        pid = mib[2]
        self.argv_calls.append(pid)
        self.on_argv(pid)
        if pid in self.args_errors:
            ctypes.set_errno(self.args_errors[pid])
            return -1
        length = ctypes.cast(size, ctypes.POINTER(ctypes.c_size_t))
        data = self.args[pid]
        ctypes.memmove(buffer, data, min(len(data), length.contents.value))
        length.contents.value = self.args_sizes.get(pid, len(data))
        return 0

    def read(self):
        # No CDLL loads, host process queries, or top-level System.processes fake.
        with patch.object(m.sys, "platform", "darwin"), \
                patch.object(m.os, "getpid", return_value=self.own), \
                patch.object(m.os, "getuid", return_value=self.owner), \
                patch.object(m.os, "geteuid", return_value=self.owner), \
                patch.object(m.ctypes, "CDLL", side_effect=AssertionError("real kernel access")):
            return m.host_processes(self.lib, self.libc)


class KernelReaderCase(unittest.TestCase):
    def test_protected_foreign_uid_skips_argv(self):
        kernel = GeneratedKernel()
        rows = kernel.read()
        self.assertEqual(set(rows), {kernel.own, kernel.worker})
        self.assertNotIn(kernel.foreign, kernel.argv_calls)
        self.assertGreaterEqual(kernel.info_calls[kernel.foreign], 2)

    def test_real_effective_and_saved_owner_uids_are_relevant(self):
        for credentials in ((501, 0, 0), (0, 501, 0), (0, 0, 501)):
            with self.subTest(credentials=credentials):
                kernel = GeneratedKernel()
                kernel.records[kernel.worker] = kernel.record(kernel.worker, *credentials)
                kernel.args_errors[kernel.worker] = errno.EPERM
                with self.assertRaises(m.Refused):
                    kernel.read()
                self.assertIn(kernel.worker, kernel.argv_calls)

    def test_relevant_permission_and_arbitrary_errors_refuse(self):
        for error in (errno.EPERM, errno.EACCES, errno.EIO, errno.EINVAL, 0):
            with self.subTest(error=error):
                kernel = GeneratedKernel()
                kernel.args_errors[kernel.worker] = error
                with self.assertRaises(m.Refused):
                    kernel.read()

    def test_unknown_ownership_even_of_foreign_process_refuses(self):
        for pid in (GeneratedKernel.worker, GeneratedKernel.foreign):
            for error in (errno.EPERM, errno.EIO, errno.EINVAL, 0):
                kernel = GeneratedKernel()
                kernel.info_errors[pid] = error
                with self.subTest(pid=pid, error=error), self.assertRaises(m.Refused):
                    kernel.read()

    def test_positive_kernel_esrch_exit_can_be_skipped(self):
        kernel = GeneratedKernel()
        kernel.info_errors[kernel.worker] = errno.ESRCH
        self.assertEqual(set(kernel.read()), {kernel.own})
        self.assertNotIn(kernel.worker, kernel.argv_calls)

    def test_argv_esrch_requires_positive_exit_confirmation(self):
        kernel = GeneratedKernel()
        kernel.args_errors[kernel.worker] = errno.ESRCH
        with self.assertRaises(m.Refused):
            kernel.read()  # Still live according to kernel identity.
        kernel = GeneratedKernel()
        kernel.args_errors[kernel.worker] = errno.ESRCH
        def exit_at_argv(pid):
            if pid == kernel.worker:
                kernel.info_errors[pid] = errno.ESRCH
        kernel.on_argv = exit_at_argv
        self.assertEqual(set(kernel.read()), {kernel.own})

    def test_unrelated_uid_creation_and_exit_are_tolerated(self):
        kernel = GeneratedKernel()
        def churn(count):
            if count == 2:
                del kernel.records[kernel.foreign]
                kernel.records[32000] = kernel.record(32000, uid=80)
                kernel.args_errors[32000] = errno.EPERM
        kernel.on_list = churn
        self.assertEqual(set(kernel.read()), {kernel.own, kernel.worker})
        self.assertNotIn(32000, kernel.argv_calls)
        self.assertNotIn(kernel.foreign, kernel.argv_calls)

    def test_relevant_pid_reuse_credentials_and_exec_changes_refuse(self):
        for changed in ({"start": 101}, {"usec": 2}, {"uid": 0, "ruid": 501}, {"svuid": 0}):
            with self.subTest(changed=changed):
                kernel = GeneratedKernel()
                def replace(pid):
                    if pid == kernel.worker:
                        kernel.records[pid] = kernel.record(pid, **changed)
                kernel.on_argv = replace
                with self.assertRaises(m.Refused):
                    kernel.read()
        kernel = GeneratedKernel()
        def exec_at_last_scan(count):
            if count == 2:
                kernel.args[kernel.worker] = kernel.arguments(b"tools/mg", b"sim", b"acquire", b"iphone")
        kernel.on_list = exec_at_last_scan
        with self.assertRaises(m.Refused):
            kernel.read()

    def test_relevant_new_arrival_or_final_pin_change_refuses(self):
        for kind in ("arrival", "reuse", "foreign-to-owner"):
            kernel = GeneratedKernel()
            def change(count):
                if count == 2:
                    if kind == "arrival":
                        kernel.records[32000] = kernel.record(32000)
                    elif kind == "reuse":
                        kernel.records[kernel.worker] = kernel.record(kernel.worker, start=102)
                    else:
                        kernel.records[kernel.foreign] = kernel.record(kernel.foreign)
            kernel.on_list = change
            with self.subTest(kind=kind), self.assertRaises(m.Refused):
                kernel.read()

    def test_relevant_exit_after_argv_is_positive_and_reuse_still_refuses(self):
        for reuse in (False, True):
            kernel = GeneratedKernel()
            def exit_after_read(pid, count):
                if pid == kernel.worker and count == 2:
                    del kernel.records[pid]
            def reappear(count):
                if count == 2 and reuse:
                    kernel.records[kernel.worker] = kernel.record(kernel.worker, start=102)
            kernel.on_info, kernel.on_list = exit_after_read, reappear
            if reuse:
                with self.assertRaises(m.Refused):
                    kernel.read()
            else:
                self.assertEqual(set(kernel.read()), {kernel.own})

    def test_relevant_active_mg_is_recognized_through_reader(self):
        kernel = GeneratedKernel()
        kernel.args[kernel.worker] = kernel.arguments(b"/generated/tools/mg", b"sim", b"acquire", b"iphone")
        rows = kernel.read()
        self.assertEqual(rows[kernel.worker], "/generated/tools/mg sim acquire iphone")
        with patch.object(m.os, "getpid", return_value=kernel.own), self.assertRaises(m.Refused):
            m.host_activity(rows, {"name": "generated", "ssh": "/generated/ssh", "tart": "/generated/tart"})

    def test_partial_oversized_and_malformed_kernel_data_refuse(self):
        for size in (0, -1, 3, 32768 * 4, 32768 * 4 + 4):
            kernel = GeneratedKernel()
            kernel.list_size = size
            with self.subTest(list_size=size), self.assertRaises(m.Refused):
                kernel.read()
        for rows in ([31000, 31001, 31001], [31000, -5], [31001]):
            kernel = GeneratedKernel()
            kernel.list_rows = rows
            with self.subTest(rows=rows), self.assertRaises(m.Refused):
                kernel.read()
        for size in (0, -1, 135, 137):
            kernel = GeneratedKernel()
            kernel.info_sizes[kernel.worker] = size
            with self.subTest(info_size=size), self.assertRaises(m.Refused):
                kernel.read()
        for raw in (GeneratedKernel.record(31999), GeneratedKernel.record(31001, start=0),
                    GeneratedKernel.record(31001, usec=1000000), b"\0" * 136):
            kernel = GeneratedKernel()
            kernel.records[kernel.worker] = raw
            with self.subTest(raw=raw), self.assertRaises(m.Refused):
                kernel.read()

    def test_partial_oversized_malformed_and_changed_argv_refuse(self):
        for size in (0, 4, 262144, 262145):
            kernel = GeneratedKernel()
            kernel.args_sizes[kernel.worker] = size
            with self.subTest(size=size), self.assertRaises(m.Refused):
                kernel.read()
        for raw in (struct.pack("=i", 0) + b"/x\0x\0", struct.pack("=i", 16384) + b"/x\0x\0",
                    struct.pack("=i", 1) + b"/unterminated", struct.pack("=i", 1) + b"relative\0x\0",
                    struct.pack("=i", 2) + b"/x\0one\0partial", struct.pack("=i", 1) + b"/x\0partial",
                    struct.pack("=i", 1) + b"/x\0\xff\0", struct.pack("=i", 1) + b"/x\0\0"):
            kernel = GeneratedKernel()
            kernel.args[kernel.worker] = raw
            with self.subTest(raw=raw), self.assertRaises((m.Refused, UnicodeError)):
                kernel.read()

    def test_relevant_omission_and_self_identity_mismatch_refuse(self):
        kernel = GeneratedKernel()
        def omit(count):
            if count == 2:
                kernel.list_rows = [kernel.own, kernel.foreign]
                kernel.records[kernel.worker] = kernel.record(kernel.worker, start=102)
        kernel.on_list = omit
        with self.assertRaises(m.Refused):
            kernel.read()
        kernel = GeneratedKernel()
        kernel.records[kernel.own] = kernel.record(kernel.own, uid=0)
        with self.assertRaises(m.Refused):
            kernel.read()


class CommandLifetimeCase(unittest.TestCase):
    def test_missing_nonreaping_primitive_or_auto_reap_refuses_before_spawn(self):
        for missing in (False, True):
            with patch.object(m.signal, "getsignal", return_value=signal.SIG_IGN), \
                    patch.object(m.os, "waitid", None if missing else unittest.mock.Mock()), \
                    patch.object(m.subprocess, "Popen") as spawn, \
                    patch.object(m.os, "killpg") as group_signal, \
                    patch.object(m.os, "kill") as pid_signal:
                with self.assertRaises(m.Refused):
                    m.command(["generated-only"], 1)
                spawn.assert_not_called()
                group_signal.assert_not_called()
                pid_signal.assert_not_called()

    def run_generated(self, observation, code=0, signal_error=None, join_error=None):
        """No real child, wait, signal, sleep, or clock. Track held lifetime."""
        events, state = [], {"reaped": False, "complete": False, "clock": 0}
        child = unittest.mock.Mock(pid=123456, returncode=None)
        def spawn(*args, **kwargs):
            self.assertTrue(kwargs["close_fds"])
            self.assertTrue(kwargs["start_new_session"])
            self.assertNotIn("pass_fds", kwargs)
            kwargs["stdout"].write(b"generated-output")
            kwargs["stdout"].flush()
            events.append("spawn")
            return child
        def waitid(kind, pid, flags):
            self.assertEqual((kind, pid), (m.os.P_PID, child.pid))
            self.assertEqual(flags, m.os.WEXITED | m.os.WNOHANG | m.os.WNOWAIT)
            self.assertFalse(state["reaped"])
            events.append("observe")
            result = observation(events, state)
            if isinstance(result, BaseException):
                raise result
            if result:
                state["complete"] = True
                return SimpleNamespace(si_pid=child.pid, si_code=m.os.CLD_EXITED)
            return None
        def killpg(pid, sig):
            self.assertEqual(pid, child.pid)
            self.assertFalse(state["reaped"])
            self.assertFalse(state["complete"])
            self.assertIsNone(child.returncode)
            events.append("TERM" if sig == signal.SIGTERM else "KILL")
            if signal_error:
                raise signal_error
        def join(timeout):
            self.assertGreaterEqual(timeout, 0)
            self.assertLessEqual(timeout, 10)
            self.assertTrue(state["complete"])
            self.assertFalse(state["reaped"])
            events.append("join")
            if join_error:
                raise join_error
            state["reaped"] = True
            child.returncode = code
        def clock():
            state["clock"] += 1
            return state["clock"]
        child.wait.side_effect = join
        child.poll.side_effect = AssertionError("reaping poll forbidden")
        with patch.object(m.subprocess, "Popen", side_effect=spawn), \
                patch.object(m.os, "waitid", side_effect=waitid), \
                patch.object(m.os, "killpg", side_effect=killpg), \
                patch.object(m.os, "kill", side_effect=AssertionError("PID signalling forbidden")), \
                patch.object(m.time, "monotonic", side_effect=clock), \
                patch.object(m.time, "sleep"):
            try:
                result = m.command(["generated-only"], 1)
            except BaseException as error:
                result = error
        return result, events, state

    def test_normal_success_and_nonzero_completion_never_signal(self):
        for code in (0, 9):
            result, events, state = self.run_generated(lambda *_: True, code=code)
            self.assertEqual(events, ["spawn", "observe", "join"])
            self.assertTrue(state["reaped"])
            if code:
                self.assertIsInstance(result, m.Refused)
            else:
                self.assertEqual(result, b"generated-output")

    def test_original_lifetime_held_through_timeout_and_cancellation_signals(self):
        for cancellation in (False, True):
            def observation(events, state):
                if cancellation and events.count("observe") == 1:
                    return m.Cancelled()
                return "KILL" in events
            result, events, state = self.run_generated(observation)
            self.assertIsInstance(result, m.Cancelled if cancellation else subprocess.TimeoutExpired)
            self.assertLess(events.index("TERM"), events.index("KILL"))
            self.assertLess(events.index("KILL"), events.index("join"))
            self.assertEqual(events[-1], "join")
            self.assertTrue(state["reaped"])

    def test_completion_observed_during_cleanup_prevents_further_signals(self):
        for after_term in (False, True):
            def observation(events, state):
                if events.count("observe") == 1:
                    return m.Cancelled()
                return "TERM" in events if after_term else True
            result, events, state = self.run_generated(observation)
            self.assertIsInstance(result, m.Cancelled)
            self.assertEqual(events.count("TERM"), int(after_term))
            self.assertNotIn("KILL", events)
            self.assertEqual(events[-1], "join")
            self.assertTrue(state["reaped"])

    def test_failed_join_after_known_completion_cannot_signal(self):
        result, events, state = self.run_generated(lambda *_: True, join_error=ChildProcessError())
        self.assertIsInstance(result, ChildProcessError)
        self.assertEqual(events.count("join"), 1)
        self.assertNotIn("TERM", events)
        self.assertNotIn("KILL", events)

    def test_positive_exit_after_missing_group_still_requires_join(self):
        result, events, state = self.run_generated(lambda events, _: "TERM" in events,
                                                   signal_error=ProcessLookupError())
        self.assertIsInstance(result, subprocess.TimeoutExpired)
        self.assertEqual(events.count("TERM"), 1)
        self.assertNotIn("KILL", events)
        self.assertTrue(state["reaped"])

    def test_unconfirmed_exit_is_bounded_and_refuses_without_early_reap(self):
        result, events, state = self.run_generated(lambda *_: False)
        self.assertIsInstance(result, m.Refused)
        self.assertEqual(events.count("TERM"), 1)
        self.assertEqual(events.count("KILL"), 1)
        self.assertNotIn("join", events)
        self.assertFalse(state["reaped"])
        self.assertLess(events.count("observe"), 30)

    def test_unknown_ownership_or_signal_failure_refuses(self):
        result, events, state = self.run_generated(lambda *_: ChildProcessError())
        self.assertIsInstance(result, m.Refused)
        self.assertNotIn("TERM", events)
        self.assertNotIn("KILL", events)
        self.assertFalse(state["reaped"])
        result, events, state = self.run_generated(lambda *_: False, signal_error=PermissionError())
        self.assertIsInstance(result, PermissionError)
        self.assertEqual(events.count("TERM"), 1)
        self.assertNotIn("KILL", events)
        self.assertFalse(state["reaped"])


if __name__ == "__main__":
    unittest.main()
