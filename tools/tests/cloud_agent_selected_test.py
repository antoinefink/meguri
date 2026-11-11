#!/usr/bin/env python3
"""Generated selected doubles prove reducer refusals, never signed execution."""
import copy
import contextlib
import io
import os
import importlib.util
import itertools
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
    def callback(kind, saved_batch=False):
        nonlocal sdk
        sdk += 1
        facts = []
        for phase in ("delivered", "committed", "returned"):
            state_save = kind == "state" and phase == "committed"
            facts.append(event("stateSaved" if state_save else "engineCallback", "sdk%d-%s-%s" % (sdk, kind, phase), True))
            if state_save:
                facts.append(event("stateSaved", "sdk%d-state-batches" % sdk, True))
                if saved_batch:
                    facts.append(event("stateSaved", "sdk%d-entity1-read-absent" % sdk, True))
                    facts.append(event("stateSaved", "sdk%d-send1-batch1-entity1-reflected-absent" % sdk, True))
        return facts
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
        traces += [event("applied", "required-scan-before-native-sdk", engine=True)]
        traces += callback("state")
        check(3, [("coordinatorResult", [fact("stateSaved", "p1", 7, engine=True)] + control)])
    else:
        traces += [event("applied", "normal-adoption-generation-joined"),
                   event("applied", "required-scan-before-native-sdk", engine=True)]
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
        upload = [event("applied", "batch1-collection-handoff", True), fact("applied", "batch1-save-count", 1, engine=True),
                  event("applied", "send1-batch1-manual-build", True),
                  event("applied", "send1-batch1-entity1-member", True)]
        sdk += 1
        for phase in ("delivered", "committed", "returned"):
            upload.append(event("engineCallback", "sdk%d-sent-batch1-%s" % (sdk, phase), True))
            if phase == "committed":
                upload.append(event("engineCallback", "send1-batch1-entity1-saved", True))
        returned, sent_sdk = tick, sdk
        upload.append(event("engineCallback", "send1-batch1-batch-returned", True))
        upload.append(event("applied", "send1-manual-returned", True))
        upload += callback("state", saved_batch=True)
        exchange += [fact("applied", "n1-sdk%d-batch1-entity1-ack" % sent_sdk, returned, engine=True),
                     fact("stateSaved", "n1-earlier-original-ack", 2, "1" * 64),
                     fact("localDurable", "n1", 2, "1" * 64)]
        check(5, [("coordinatorResult", exchange)])
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
    number = 5 if kind == "mac" else 4
    observations[number][1:1] = [
        fact("syncResult", "original-exchange-issued", 1, "d" * 64),
        fact("syncResult", "exchange-capture", 1),
        fact("syncResult", "exchange-boundary", 9),
        fact("syncResult", "exchange-selected-entity1"),
        fact("syncResult", "terminal-exchange-succeeded", 1),
        fact("syncResult", "terminal-ended-ms", 30),
        *[{**item, "operation": "terminal-" + item["operation"]} for item in exchange
          if item["operation"] in c.EXCHANGE_COUNTS | c.MEDIA]]
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
    def group(item):
        label = item.get("operation", "")
        if label in c.TERMINAL_LABELS:
            return "terminal"
        if label in {"exchange-capture", "exchange-boundary"} or c.CAPTURE_MEMBER.fullmatch(label):
            return "capture"
        if label.endswith("-state-committed") or c.state_batch_fact(label):
            return label.split("-")[0]
        callback = c.SENT_PHASE.fullmatch(label)
        if callback:
            return callback.groups()
        send = c.SEND.fullmatch(label)
        if send:
            _, batch, _, phase = send.groups()
            if c.send_result(phase):
                return (batch, "committed")
            if phase == "batch-returned":
                return (batch, "returned")
        if c.pending_fact(label):
            return "pending"
        return None
    for n in range(1, through+1):
        add(n, "started", action=c.PROGRAMS[kind][n-1][0])
        for grouped, items in itertools.groupby(observations[n], key=group):
            if grouped:
                add(n, "observation", facts=list(items))
            else:
                for item in items: add(n, "observation", facts=[item])
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
    def test_replacement_engine_cannot_prove_original_save(self):
        evidence = c.Evidence()

        def trace(label, name="applied", engine="engine1"):
            evidence.position += 1
            evidence.ingest({"name": name, "count": evidence.tick + 1, "operation": label, "engine": engine})

        def callback(label, engine):
            for phase in ("delivered", "committed", "returned"):
                name = "stateSaved" if label.endswith("-state") and phase == "committed" else "engineCallback"
                operation = label + "-" + phase
                trace(operation, name, engine)
                if name == "stateSaved":
                    sdk = label.split("-")[0]
                    labels = [operation, sdk + "-state-batches"]
                    if engine == "engine1":
                        labels.extend([sdk + "-entity1-read-absent", sdk + "-send1-batch1-entity1-reflected-absent"])
                    group = [{"name": name, "operation": op, "engine": engine} for op in labels]
                    for item in group[1:]:
                        evidence.ingest({**item, "count": evidence.tick + 1})
                    evidence.check_state_batch_group(group)

        trace("required-scan-before-native-sdk")
        trace("batch1-collection-handoff")
        evidence.ingest({"name": "applied", "count": 1, "operation": "batch1-save-count", "engine": "engine1"})
        trace("send1-batch1-manual-build")
        trace("send1-batch1-entity1-member")
        trace("sdk1-sent-batch1-delivered", "engineCallback")
        trace("sdk1-sent-batch1-committed", "engineCallback")
        trace("send1-batch1-entity1-saved", "engineCallback")
        trace("sdk1-sent-batch1-returned", "engineCallback")
        ack = {"name": "applied", "count": evidence.tick, "operation": "n1-sdk1-batch1-entity1-ack", "engine": "engine1"}
        trace("send1-batch1-batch-returned", "engineCallback")
        with self.assertRaisesRegex(s.v.Refused, "selected-scan-before-sdk"):
            trace("sdk2-state-delivered", "engineCallback", "engine2")
        trace("required-scan-before-native-sdk", engine="engine2")
        callback("sdk2-state", "engine2")
        with self.assertRaisesRegex(s.v.Refused, "engine_order_not_shown"):
            evidence.acknowledged(ack)
        with self.assertRaisesRegex(s.v.Refused, "selected-ack-callback"):
            evidence.acknowledged({**ack, "engine": "engine2"})
        callback("sdk3-state", "engine1")
        evidence.acknowledged(ack)
        evidence.joined([fact("ownedWorkJoined", "own-joined", 0), fact("ownedWorkJoined", "sdk-joined", 3)])

    def test_delete_results_require_their_own_batch_and_state(self):
        for phase in ("delete-error-3", "deleted", "delete-error-11", "saved"):
            evidence = c.Evidence()

            def group(*labels, name="engineCallback"):
                evidence.position += 1
                items = []
                for label in labels:
                    item = fact(name, label, evidence.tick + 1, engine=True)
                    evidence.ingest(item)
                    items.append(item)
                evidence.check_sent_group(items)
                evidence.check_state_batch_group(items)

            group("required-scan-before-native-sdk", "batch1-media-handoff", name="applied")
            evidence.ingest(fact("applied", "batch1-save-count", 0, engine=True))
            evidence.ingest(fact("applied", "batch1-delete-count", 1, engine=True))
            group("send1-batch1-manual-build", "send1-batch1-entity1-delete-member", name="applied")
            group("sdk1-sent-batch1-delivered")
            result = "send1-batch1-entity1-" + phase
            if phase == "saved":
                with self.assertRaisesRegex(s.v.Refused, "selected-batch-result"):
                    group("sdk1-sent-batch1-committed", result)
                continue
            group("sdk1-sent-batch1-committed", result)
            group("sdk1-sent-batch1-returned", "send1-batch1-batch-returned")
            own = fact("ownedWorkJoined", "own-joined", 0)
            if phase != "delete-error-3":
                with self.assertRaisesRegex(s.v.Refused, "engine_order_not_shown"):
                    evidence.joined([own, fact("ownedWorkJoined", "sdk-joined", 1, engine=True)])
                group("sdk2-state-delivered")
                group("sdk2-state-committed", "sdk2-state-batches", "sdk2-entity1-read-absent",
                      "sdk2-send1-batch1-entity1-reflected-absent", name="stateSaved")
                group("sdk2-state-returned")
            evidence.joined([own, fact("ownedWorkJoined", "sdk-joined", len(evidence.sdk), engine=True)])

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

    def test_install_child_retains_only_allowlisted_delivery_failure(self):
        root = "/generated/checkout"
        run = "20261110T000000Z-1"
        parent = "20261110T000000Z-2"
        sha = "a" * 40
        args = ["cloud-agent-selected.py", "--install-child", "--root", root, "--parent-run", parent,
                "--run-id", run, "--run-directory", root + "/build/mg/cloudkit-agent-install/" + run,
                "--source-commit", sha]
        vm = object()
        for reason, expected in [("profile-device", "profile-device"), ("generated-private-value", "unclassified")]:
            with self.subTest(reason=reason), tempfile.TemporaryFile() as context:
                context.write(raw({}))
                context.seek(0)
                output = io.StringIO()
                dup = os.dup

                def refuse(*args, checkpoint):
                    self.assertIs(args[-1], vm)
                    checkpoint("recheck-build")
                    raise s.v.Refused(reason)

                with patch.object(sys, "argv", args), patch.object(os, "dup", side_effect=lambda fd: dup(context.fileno())), \
                        patch.object(s.v, "source", return_value=sha), patch.object(s, "owned_context",
                            return_value=contextlib.nullcontext((vm, lambda: None))), \
                        patch.object(c.i, "deliver", side_effect=refuse), contextlib.redirect_stdout(output):
                    self.assertEqual(c.main(), 1)
                self.assertEqual(json.loads(output.getvalue()), {"scope": "DELIVERY-ONLY", "outcome": "failed_evidence",
                    "category": "selected-delivery-refused", "signed_execution": False, "stage": "recheck-build",
                    "refusal": expected})

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
            index = next(i for i, f in enumerate(observations[4]) if f.get("operation") == "original-exchange")
            observations[4].insert(index, {**item, "operation": "terminal-media-upload-pending"})
        absent = admitted("iphone").completion(4)[4]
        present = admitted("iphone", mutate=add_media).completion(4)[4]
        self.assertNotIn("media-upload-pending", c.unique(absent))
        self.assertEqual(c.unique(present)["media-upload-pending"]["count"], 0)
        def bad(checks, observations):
            item = fact("syncResult", "media-no-space", 2)
            checks[4][0]["facts"].append(item); observations[4].append(copy.deepcopy(item))
        with self.assertRaises(s.v.Refused): admitted("iphone", mutate=bad)

    def test_exchange_summary_needs_matching_original_terminal_accounting(self):
        for mutation in ("missing", "changed", "split"):
            def alter(checks, observations):
                group = observations[5]
                idx = next(i for i, item in enumerate(group) if item.get("operation") == "terminal-selected")
                if mutation == "missing":
                    group[:] = [f for f in group if f.get("operation") not in c.TERMINAL_LABELS]
                elif mutation == "changed":
                    group[idx]["count"] += 1
                else:
                    group.insert(idx, group.pop(next(i for i, f in enumerate(group)
                                                     if f.get("operation") == "original-exchange")))
            with self.subTest(mutation=mutation), self.assertRaisesRegex(s.v.Refused, "selected-exchange-terminal"):
                admitted("mac", mutate=alter)

    def test_missing_ack_revision_and_callback_state_refuse(self):
        for label in ("n1-sdk2-batch1-entity1-ack", "n1-earlier-original-ack", "sdk3-state-committed",
                      "sdk3-state-batches"):
            def omit(checks, observations):
                for check in checks[5]: check["facts"][:] = [f for f in check["facts"] if f.get("operation") != label]
                observations[5][:] = [f for f in observations[5] if f.get("operation") != label]
            with self.subTest(label=label), self.assertRaises(s.v.Refused): admitted("mac", mutate=omit)
        def mismatch(checks, observations):
            for item in checks[5][0]["facts"] + observations[5]:
                if item.get("operation") == "n1-earlier-original-ack": item["count"] = 99
        with self.assertRaises(s.v.Refused): admitted("mac", mutate=mismatch)

    def test_exchange_needs_the_original_capture_and_matching_count(self):
        def omit(checks, observations):
            observations[5] = [item for item in observations[5] if item.get("operation") != "exchange-capture"]

        def lose_member(checks, observations):
            observations[5] = [item for item in observations[5] if item.get("operation") != "exchange-selected-entity1"]

        for mutate in (omit, lose_member):
            with self.assertRaisesRegex(s.v.Refused, "selected-exchange-capture"):
                admitted("mac", mutate=mutate)

    def test_acknowledgement_requires_its_actual_saved_batch_member(self):
        def substitute(checks, observations):
            for item in checks[5][0]["facts"] + observations[5]:
                if item.get("operation") == "n1-sdk2-batch1-entity1-ack":
                    item["operation"] = "n1-sdk2-batch1-entity2-ack"
        with self.assertRaisesRegex(s.v.Refused, "selected-ack-member"):
            admitted("mac", mutate=substitute)

    def test_live_pending_read_excludes_adapter_removals(self):
        def replace_with_held_read(checks, observations):
            for item in observations[5]:
                if item.get("operation") == "sdk3-entity1-read-absent":
                    item["operation"] = "sdk3-entity1-read-held"
        with self.assertRaisesRegex(s.v.Refused, "selected-state-batches"):
            admitted("mac", mutate=replace_with_held_read)

        def remove_before_state(checks, observations):
            items = observations[5]
            first_state = next(i for i, item in enumerate(items) if item.get("operation") == "sdk3-state-delivered")
            tick = items[first_state]["count"]
            for item in items[first_state:]:
                item["count"] += 2
            items[first_state:first_state] = [
                fact("applied", "pending-update", tick, engine=True),
                fact("applied", "entity1-removed-held", tick + 1, engine=True)]
        with self.assertRaisesRegex(s.v.Refused, "selected-state-batches"):
            admitted("mac", mutate=remove_before_state)

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
