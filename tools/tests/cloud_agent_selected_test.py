#!/usr/bin/env python3
"""Generated selected doubles prove reducer refusals, never signed execution."""
import copy
import contextlib
import os
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("cloud_selected", Path(__file__).resolve().parents[1] /
                                              "scripts/cloud-agent-selected.py")
c = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = c
spec.loader.exec_module(c)
s = c.s


def raw(value):
    return (json.dumps(value, sort_keys=True) + "\n").encode()


def header(kind):
    return {"selection": "sync_now", "device_kind": kind}


def fact(name, operation=None, count=None, digest=None, engine=False):
    item = {"name": name}
    if operation is not None: item["operation"] = operation
    if count is not None: item["count"] = count
    if digest is not None: item["digest"] = digest
    if engine: item["engine"] = "engine1"
    return item


def generated(kind):
    """Current AgentFact wire shapes and original callback ordering, all synthetic."""
    checks, observations = {}, {}
    tick, sdk = 0, 0
    def event(name, label, engine=False):
        nonlocal tick
        tick += 1
        return fact(name, label, tick, engine=engine)
    def callback(kind):
        nonlocal sdk
        sdk += 1
        return [event("stateSaved" if kind == "state" and phase == "committed" else "engineCallback",
                      "sdk%d-%s-%s" % (sdk, kind, phase), True)
                for phase in ("delivered", "committed", "returned")]
    def check(n, names):
        checks[n] = [{"name": name, "outcome": "pass", "facts": rows} for name, rows in names]
        observations[n] = [fact("accountVerified")] + copy.deepcopy([row for _, rows in names for row in rows])
    check(1, [("integrity", [fact("integrity", count=5)])])
    observations[1] = [fact("accountVerified"), fact("coreOpened", count=2)]
    check(2, [("coordinatorResult", [fact("applied", "p1", 20)])])
    control = [fact("applied", "canonical-active", digest="a" * 64),
               fact("applied", "canonical-complete-epoch", digest="b" * 64),
               fact("applied", "binding-matches-epoch", 1),
               fact("applied", "required-scan-native-sdk-admitted", 1),
               fact("applied", "original-control-witness", 1, "c" * 64)]
    traces = []
    if kind == "mac":
        for own, label in enumerate(("createZones", "saveMediaRoot", "savePublishingEpoch", "saveCompleteEpoch", "saveActive"), 1):
            traces += [event("stateSaved" if phase == "committed" else "applied", "own%d-%s-%s" % (own, label, phase))
                       for phase in ("prepared", "enqueued", "replied", "committed")]
        traces += [event("applied", "required-scan-before-native-sdk")]
        traces += callback("state")
        check(3, [("coordinatorResult", [fact("stateSaved", "p1", 7, engine=True)] + control)])
    else:
        traces += [event("applied", label) for label in
                   ("normal-adoption-generation-joined", "required-scan-before-native-sdk")]
        traces += callback("state")
        control += [fact("applied", "owned-adoption-backup-activation", 1),
                    fact("localDurable", "original-n1-preserved", digest="0" * 64)]
        check(3, [("integrity", [fact("integrity", count=5)]), ("coordinatorResult", control)])
        observations[3] = [fact("accountVerified")] + copy.deepcopy(control)
    observations[3] += traces
    exchange = [fact("syncResult", label, count) for label, count in
                (("original-exchange", 1), ("selected", 1), ("held-back", 0), ("newer-pending", 0), ("incoming-waiting", 0))]
    if kind == "mac":
        check(4, [("localDurability", [fact("localCommitted", "n1", 8, "1" * 64),
                                       fact("localDurable", "n1", 2, "1" * 64),
                                       fact("localDurable", "original-edit-witness", 1, "e" * 64)])])
        upload = [event("applied", "batch1-collection-handoff", True), fact("applied", "batch1-save-count", 1, engine=True)]
        upload += callback("sent-batch1")
        returned, sent_sdk = tick, sdk
        upload += callback("state")
        exchange += [fact("applied", "n1-sdk%d-batch1-ack" % sent_sdk, returned, engine=True),
                     fact("stateSaved", "n1-earlier-original-ack", 2, "1" * 64),
                     fact("localDurable", "n1", 2, "1" * 64)]
        check(5, [("coordinatorResult", exchange)])
        observations[5].insert(1, fact("syncResult", "original-exchange-issued", 1, "d" * 64))
        observations[5] += upload
    else:
        check(4, [("coordinatorResult", exchange)])
        fetch = []
        for label in ("fetch-start", "fetch-zone-start", "fetched", "fetch-zone-end", "fetch-end"):
            fetch += callback(label)
            if label == "fetched": returned = tick
        observations[4] += fetch
        check(5, [("localDurability", [fact("localDurable", "n1", 9, "1" * 64),
                                       fact("engineCallback", "n1-original-fetched-returned", returned, engine=True)])])
    join = [fact("ownedWorkJoined", "own-joined", 5 if kind == "mac" else 0),
            fact("ownedWorkJoined", "sdk-joined", sdk, engine=True)]
    check(6, [("integrity", [fact("integrity", count=5)]), ("coordinatorResult", join),
              ("localDurability", [fact("ownedWorkJoined")])])
    observations[6] = [fact("accountVerified")] + copy.deepcopy(join) + [fact("ownedWorkJoined")]
    return checks, observations


def journal_bytes(kind, checks, observations, through=6, header_value=None):
    commands = c.command_bytes(raw(header(kind) if header_value is None else header_value))
    records = []
    def add(n, record_kind, **extra):
        records.append({"header_sha256": json.loads(commands[n-1])["header_sha256"],
                        "record_seq": len(records)+1, "command_seq": n,
                        "command_sha256": s.v.digest(commands[n-1][:-1]), "agent_instance": "original1",
                        "observed_at": "2026-11-10T12:00:00Z", "kind": record_kind, "facts": [], "checks": [], **extra})
    for n in range(1, through+1):
        add(n, "started", action=c.PROGRAMS[kind][n-1][0])
        for item in observations[n]: add(n, "observation", facts=[item])
        add(n, "done", checks=checks[n])
    return b"".join(raw(r) for r in records)


def admitted(kind, through=6, mutate=None):
    checks, observations = generated(kind)
    if mutate is not None: mutate(checks, observations)
    j = c.Journal(raw(header(kind)))
    j.inspect(journal_bytes(kind, checks, observations, through), through)
    j.completion(through)
    return j


class SelectedTests(unittest.TestCase):
    def test_preflight_diagnostics_export_only_closed_stage_and_refusal_values(self):
        diagnostic = c.Diagnostic()
        diagnostic.enter("simulator-os")
        known = diagnostic.failure(s.v.Refused("vm-simctl"))
        self.assertEqual(known["stage"], "simulator-os")
        self.assertEqual(known["refusal"], "vm-simctl")
        self.assertFalse(known["signed_execution"])
        for error in (s.v.Refused("generated-private-path-value"), KeyError("generated-private-field"),
                      OSError("generated-private-filesystem-value")):
            exported = diagnostic.failure(error)
            self.assertNotIn("generated-private", json.dumps(exported))
            self.assertIn(exported["refusal"], c.PREFLIGHT_REFUSALS)
        for mutation in ({"stage": "generated-private-stage"}, {"refusal": "generated-private-code"},
                         {"extra": "generated-private-context"}, {"signed_execution": True}):
            self.assertEqual(c.sanitized_failure({**known, **mutation}), c.sanitized_failure(None))
        self.assertEqual(c.sanitized_failure(known), known)
        with self.assertRaises(s.v.Refused): diagnostic.enter("generated-private-stage")

    def test_cleanup_diagnostics_do_not_hide_primary_refusal_or_unknown_ownership(self):
        diagnostic = c.Diagnostic(); diagnostic.enter("simulator-os")
        primary = diagnostic.failure(s.v.Refused("vm-simctl"))
        cleanup = diagnostic.failure(s.v.Refused("lease-changed"))
        exported = c.sanitized_failure(primary, cleanup)
        self.assertEqual(exported["refusal"], "vm-simctl")
        self.assertEqual(exported["cleanup"], {"drain": "joined", "reservation": "retained-or-unconfirmed",
                                              "stage": "simulator-os", "refusal": "lease-changed"})
        self.assertEqual(c.sanitized_cleanup({"scope": c.SCOPE, "outcome": "reservation-released"}),
                         {"drain": "joined", "reservation": "released"})
        self.assertEqual(c.sanitized_cleanup({"scope": c.SCOPE, "outcome": "no-reservation"}),
                         {"drain": "joined", "reservation": "none"})
        self.assertEqual(c.sanitized_cleanup({**cleanup, "extra": "generated-private-value"}),
                         {"drain": "unconfirmed", "reservation": "retained-or-unconfirmed"})
        self.assertNotIn("generated-private", json.dumps(c.sanitized_failure(primary, {"extra": "generated-private-value"})))
        self.assertEqual(c.sanitized_cleanup({"scope": c.SCOPE, "outcome": ["generated-private-value"]}),
                         {"drain": "unconfirmed", "reservation": "retained-or-unconfirmed"})

    def test_exact_six_commands_have_no_payload_or_recovery_expectations(self):
        for kind in c.PROGRAMS:
            commands = [json.loads(line) for line in c.command_bytes(raw(header(kind)))]
            self.assertEqual(len(commands), 6)
            self.assertEqual([x["action"] for x in commands], [x[0] for x in c.PROGRAMS[kind]])
            self.assertTrue(all(set(x) == {"header_sha256", "command_seq", "leg", "action", "arguments", "account_before", "account_after"}
                                and x["leg"] == "sync_now" for x in commands))

    def test_current_encoding_passes_each_boundary_and_independent_comparisons(self):
        for kind in c.PROGRAMS:
            for n in range(1, 7): admitted(kind, n)
        c.compare_controls(admitted("mac"), admitted("iphone"))
        c.compare_note(admitted("mac"), admitted("iphone"))

    def test_optional_media_absent_and_zero_are_distinct_and_boolean_is_bounded(self):
        def add_media(checks, observations):
            item = fact("syncResult", "media-upload-pending", 0)
            checks[4][0]["facts"].append(item); observations[4].append(copy.deepcopy(item))
        absent = admitted("iphone").completion(4)[4]
        present = admitted("iphone", mutate=add_media).completion(4)[4]
        self.assertNotIn("media-upload-pending", c.unique(absent))
        self.assertEqual(c.unique(present)["media-upload-pending"]["count"], 0)
        def bad(checks, observations):
            item = fact("syncResult", "media-no-space", 2)
            checks[4][0]["facts"].append(item); observations[4].append(copy.deepcopy(item))
        with self.assertRaises(s.v.Refused): admitted("iphone", mutate=bad)

    def test_missing_ack_revision_and_callback_state_refuse(self):
        for label in ("n1-sdk2-batch1-ack", "n1-earlier-original-ack", "sdk3-state-committed"):
            def omit(checks, observations):
                for check in checks[5]: check["facts"][:] = [f for f in check["facts"] if f.get("operation") != label]
                observations[5][:] = [f for f in observations[5] if f.get("operation") != label]
            with self.subTest(label=label), self.assertRaises(s.v.Refused): admitted("mac", mutate=omit)
        def mismatch(checks, observations):
            for item in checks[5][0]["facts"] + observations[5]:
                if item.get("operation") == "n1-earlier-original-ack": item["count"] = 99
        with self.assertRaises(s.v.Refused): admitted("mac", mutate=mismatch)

    def test_peer_apply_fetch_return_and_control_comparison_are_required(self):
        def bad_peer(checks, observations):
            for item in checks[5][0]["facts"] + observations[5]:
                if item.get("operation") == "n1-original-fetched-returned": item["count"] = 1
        with self.assertRaises(s.v.Refused): admitted("iphone", mutate=bad_peer)
        def wrong_digest(checks, observations):
            for item in checks[3][1]["facts"] + observations[3]:
                if item.get("operation") == "canonical-active": item["digest"] = "f" * 64
        with self.assertRaises(s.v.Refused): c.compare_controls(admitted("mac"), admitted("iphone", mutate=wrong_digest))
        def wrong_note(checks, observations):
            for item in checks[5][0]["facts"] + observations[5]:
                if item.get("operation") == "n1": item["digest"] = "f" * 64
        with self.assertRaises(s.v.Refused): c.compare_note(admitted("mac"), admitted("iphone", mutate=wrong_note))

    def test_partial_prefix_rewrite_instance_and_unknown_private_fields_refuse(self):
        checks, observations = generated("mac")
        original = journal_bytes("mac", checks, observations, 3)
        j = c.Journal(raw(header("mac")))
        complete, _ = j.inspect(original + b'{"partial":', 3)
        self.assertEqual(complete, original); j.retain(complete)
        with self.assertRaises(s.v.Refused): j.inspect(b" " + original, 3)
        for change in ({"agent_instance": "new2"}, {"private_route": "/generated-private"}, {"recovered_from": 1}):
            rows = [json.loads(line) for line in original.splitlines()]; rows[-1].update(change)
            with self.subTest(change=change), self.assertRaises(s.v.Refused):
                c.Journal(raw(header("mac"))).inspect(b"".join(raw(r) for r in rows), 3)

    def test_unknown_operation_and_engine_strings_refuse_before_prefix_publication(self):
        checks, observations = generated("mac")
        original = journal_bytes("mac", checks, observations, 3)
        for mutation in ({"operation": "generated-private-value"}, {"engine": "generatedPrivateEngine"},
                         {"operation": "own1-generatedPrivate-prepared"}):
            rows = [json.loads(line) for line in original.splitlines()]
            row = next(row for row in rows if row["facts"] and "operation" in row["facts"][0])
            row["facts"][0].update(mutation)
            journal = c.Journal(raw(header("mac")))
            with self.subTest(mutation=mutation), self.assertRaises(s.v.Refused):
                journal.inspect(b"".join(raw(row) for row in rows), 3)
            self.assertEqual(journal.retained, b"")

    def test_late_callback_preserves_origin_and_nontrace_late_operation_refuses(self):
        checks, obs = generated("iphone")
        rows = [json.loads(line) for line in journal_bytes("iphone", checks, obs).splitlines()]
        moved = next(r for r in rows if any(f.get("operation") == "sdk1-state-returned" for f in r["facts"]))
        rows.remove(moved)
        index = next(n for n, r in enumerate(rows) if r["command_seq"] == 4 and r["kind"] == "started")
        rows.insert(index, moved)
        for n, row in enumerate(rows, 1): row["record_seq"] = n
        j = c.Journal(raw(header("iphone"))); j.inspect(b"".join(raw(r) for r in rows), 6); j.completion(6)
        moved["facts"] = [fact("localDurable", "n1", 99, "f" * 64)]
        j = c.Journal(raw(header("iphone"))); j.inspect(b"".join(raw(r) for r in rows), 6)
        with self.assertRaises(s.v.Refused): j.completion(6)

    def test_barriers_original_sessions_and_failure_cutoff(self):
        events = []
        class Session:
            def __init__(self, kind): self.kind, self.journal = kind, admitted(kind)
            def send(self, n): events.append((self.kind, n))
            def finish(self): events.append((self.kind, "join"))
        mac, phone = Session("mac"), Session("iphone")
        report = c.coordinate(mac, phone)
        self.assertEqual(events, [("mac", n) for n in (1, 2, 3)] + [("iphone", n) for n in (1, 2, 3)] +
                         [("mac", 4), ("mac", 5), ("iphone", 4), ("iphone", 5), ("mac", 6), ("iphone", 6)] +
                         [("mac", "join"), ("iphone", "join")])
        self.assertEqual(report["scope"], "SELECTED-GENERATED")
        self.assertNotIn("scenarios", report); self.assertNotIn("full_lane", report)
        events.clear(); mac.send = Mock(side_effect=s.v.Refused("failed"))
        with self.assertRaises(s.v.Refused): c.coordinate(mac, phone)
        self.assertEqual(events, [])

    def test_selected_children_retain_private_vm_context_and_disable_host_fallback(self):
        context = {"selection": {"name": "generated-vm", "ssh": "/generated-private-ssh",
                                  "tart": "/generated-private-tart", "state": "running"},
                   "lease_slot": "/generated-private-lease"}
        captured = []
        def stop_before_launch(argv, **kwargs):
            environment = kwargs["env"]
            descriptor = int(environment["MG_CLOUD_AGENT_SELECTED_CONTEXT_FD"])
            self.assertEqual(json.loads(os.pread(descriptor, 10000, 0)), context)
            self.assertNotIn("MG_CLOUD_AGENT_SMOKE_CONTEXT_FD", environment)
            self.assertEqual(environment["MEGURI_SIM_ALLOW_HOST"], "0")
            self.assertEqual(environment["MEGURI_MAC_UI_ALLOW_HOST"], "0")
            private = Path(environment["MEGURI_MAC_VM_CONFIG"])
            self.assertEqual(private.stat().st_mode & 0o777, 0o600)
            self.assertEqual(json.loads(private.read_bytes()), {"vms": [{key: context["selection"][key]
                                                                      for key in ("name", "ssh", "tart")}]})
            self.assertEqual(kwargs["pass_fds"], (descriptor,))
            self.assertNotIn("generated-private", " ".join(argv))
            captured.append(private)
            raise s.v.Refused("generated-stop-before-launch")
        with patch.object(s.h, "setup_process", side_effect=stop_before_launch), \
             patch.object(s.h, "setup_launch", return_value=contextlib.nullcontext()), \
             patch.dict(s.os.environ, {"MEGURI_SIM_ALLOW_HOST": "1", "MEGURI_MAC_UI_ALLOW_HOST": "1", "MG_CLOUD_AGENT_SMOKE_CONTEXT_FD": "99"}):
            with self.assertRaisesRegex(s.v.Refused, "generated-stop-before-launch"):
                s.child("/generated", "sim", ["acquire", "iphone"], "a" * 40, context,
                        context_key="MG_CLOUD_AGENT_SELECTED_CONTEXT_FD")
        self.assertEqual(len(captured), 1)
        self.assertFalse(captured[0].exists())

    def test_two_original_transports_retain_prefixes_and_join_without_relaunch(self):
        events = []
        transports = []
        class Transport:
            def __init__(self, vm):
                self.kind, self.sent = vm.kind, 0
                self.child = Mock(returncode=0)
                self.checks, self.observations = generated(self.kind)
                transports.append(self); events.append((self.kind, "supervisor-open"))
            def request(self, action, data=b"", **kwargs):
                events.append((self.kind, action))
                if action == "prepare":
                    self.header = json.loads(kwargs["header"])
                    self.commands = c.command_bytes(raw(self.header))
                    self.test.assertTrue(kwargs["selected"])
                    return {"prepared": True}, b"derived"
                if action == "launch": return {"launched": True}, b""
                if action == "append":
                    self.test.assertEqual(data, self.commands[self.sent]); self.sent += 1
                    return {"appended": True}, b""
                wire = journal_bytes(self.kind, self.checks, self.observations, self.sent, self.header)
                if action == "poll": return {"alive": True}, wire
                if action == "retain":
                    self.test.assertEqual(data, wire)
                    return {"retained": True}, b""
                if action == "finish":
                    return {"joined": True, "exit_code": 0, "passed": 1, "failed": 0, "skipped": 0}, wire
                raise AssertionError(action)
            def close(self): events.append((self.kind, "supervisor-joined"))
        test = self
        Transport.test = test
        with tempfile.TemporaryDirectory() as directory, patch.object(s, "Transport", Transport), \
             patch.object(s, "materialize_private", side_effect=lambda *args: Mock()), \
             patch.object(s, "smoke_plan", return_value=(b"original", b"derived")):
            sessions, locals_ = [], []
            try:
                for kind in ("mac", "iphone"):
                    path = Path(directory, kind); path.mkdir(mode=0o700)
                    local = s.h.Directory(str(path)); locals_.append(local)
                    value = dict(header(kind), run_id="20261110T010200Z-103", device=kind + "1")
                    original = Mock(raw=raw(value))
                    tree = Mock(files={"manifest.json": Mock(raw=b"{}\n")})
                    projection = Mock(); projection.private_bytes.return_value = b"generated-private-account"
                    files = {"agent-build.json": Mock(value={"xctestrun": {"path": "original.xctestrun"}})}
                    sessions.append(c.Session(Mock(kind=kind), value, original, files,
                        {"path": "/generated-private-support"}, {}, Mock(), tree, Mock(raw=b"{}\n"),
                        Mock(raw=b"{}\n"), b"{}\n", projection, local, {"device_kind": kind}))
                with sessions[0] as mac, sessions[1] as phone:
                    c.coordinate(mac, phone)
                    self.assertEqual(len(transports), 2)
                    self.assertNotIn(("mac", "supervisor-joined"), events)
                    self.assertNotIn(("iphone", "supervisor-joined"), events)
                    for local in locals_:
                        local.check()
                for transport in transports:
                    self.assertEqual(transport.sent, 6)
                    transport.child.wait.assert_called_once_with(timeout=s.WAIT)
                    self.assertEqual(events.count((transport.kind, "launch")), 1)
                    self.assertEqual(events.count((transport.kind, "supervisor-joined")), 1)
                public = b"".join(file.read_bytes() for file in Path(directory).glob("*/*"))
                self.assertNotIn(b"generated-private-account", public)
                self.assertNotIn(b"generated-private-support", public)
            finally:
                for local in locals_: local.close()

    def test_process_skip_failure_and_nonjoined_refuse(self):
        for reply in ({"joined": True, "exit_code": 0, "passed": 0, "failed": 0, "skipped": 1},
                      {"joined": False, "exit_code": 0, "passed": 1, "failed": 0, "skipped": 0},
                      {"joined": True, "exit_code": 1, "passed": 1, "failed": 1, "skipped": 0}):
            obj = object.__new__(c.Session); obj.delivered = 6; obj.finished = False
            obj.check = Mock(); obj.transport = Mock(); obj.transport.request.return_value = reply, b""
            with self.subTest(reply=reply), self.assertRaises(s.v.Refused): obj.finish()

    def test_one_pending_command_and_lease_loss_refuse_before_delivery(self):
        obj = object.__new__(c.Session); obj.delivered = 1; obj.finished = False
        obj.journal = Mock(); obj.journal.done_checks = {}
        with self.assertRaises(s.v.Refused): obj.send(2)
        obj.delivered = 0; obj.check = Mock(side_effect=s.v.Refused("lease-lost")); obj.transport = Mock()
        with self.assertRaises(s.v.Refused): obj.send(1)
        obj.transport.request.assert_not_called()


if __name__ == "__main__": unittest.main()
