#!/usr/bin/env python3
"""Generated parent/transport tests only. No signed, VM, SDK or smoke evidence."""
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
import plistlib
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


def generated_checks():
    # Exact current Swift encodings with generated counts/payload digests.
    # No fixture here is an actual storage, SDK or signed execution receipt.
    media = b"".join(hashlib.sha256(
        ("MeguriCloudSmoke/%s/mac1/m1/%d" % (RUN, n)).encode()).digest() for n in range(320))
    rows = [
        ("integrity", [{"name": "integrity", "count": 5}]),
        ("coordinatorResult", [{"name": "applied", "count": 20, "operation": "p1"}]),
        ("coordinatorResult", [{"name": "stateSaved", "count": 7, "engine": "engine1", "operation": "p1"}]),
        ("localDurability", [{"name": "localCommitted", "count": 8, "operation": "n1"},
                             {"name": "localDurable", "count": 1, "digest": "1"*64, "operation": "n1"}]),
        ("localDurability", [{"name": "applied", "count": 1, "operation": "c1"}]),
        ("localDurability", [{"name": "localCommitted", "count": 10, "operation": "s1"},
                             {"name": "localDurable", "count": 2, "digest": "2"*64, "operation": "c1"},
                             {"name": "localDurable", "count": 1, "digest": "3"*64, "operation": "r1"}]),
        ("localDurability", [{"name": "applied", "count": 1, "operation": "s1"}]),
        ("localDurability", [{"name": "localCommitted", "count": 1, "operation": "m1"},
                             {"name": "localDurable", "count": 1, "digest": "4"*64, "operation": "m1"},
                             {"name": "localDurable", "count": 10240,
                              "digest": hashlib.sha256(media).hexdigest(), "operation": "m1"}]),
        ("coordinatorResult", [{"name": "stateSaved", "count": 1, "digest": "1"*64, "operation": "n1"},
                              {"name": "stateSaved", "count": 2, "digest": "2"*64, "operation": "c1"},
                              {"name": "stateSaved", "count": 1, "digest": "3"*64, "operation": "r1"},
                              {"name": "stateSaved", "count": 1, "digest": "4"*64, "operation": "m1"},
                              {"name": "syncResult", "count": 1, "operation": "exchange1"}]),
        ("integrity", [{"name": "integrity", "count": 12}])]
    checks = {n: [{"name": name, "outcome": "pass", "facts": facts}]
              for n, (name, facts) in enumerate(rows, 1)}
    checks[10].append({"name": "localDurability", "outcome": "pass", "facts": [{"name": "ownedWorkJoined"}]})
    return checks


def generated_observations(checks):
    observations = {n: [{"name": "accountVerified"}] + copy.deepcopy(c[0]["facts"])
                    for n, c in checks.items()}
    observations[1] = [{"name": "accountVerified"}, {"name": "coreOpened", "count": 2}]
    observations[9][1:1] = [{"name": "syncResult", "count": 0, "operation": label}
                           for label in ("media-pending", "media-missing", "media-unknown")]
    observations[10] = [{"name": "accountVerified"}, {"name": "ownedWorkJoined"}]
    return observations


def generated_journal(commands, count=10, checks=None, observations=None):
    checks = generated_checks() if checks is None else checks
    observations = generated_observations(checks) if observations is None else observations
    lines = []
    for n in range(1, count+1):
        lines.append(record(commands, n, len(lines)+1, "started", action=s.PROGRAM[n-1][0]))
        for item in observations[n]:
            lines.append(record(commands, n, len(lines)+1, "observation", facts=[item]))
        lines.append(record(commands, n, len(lines)+1, "done", checks=checks[n]))
    return b"".join(raw(line) for line in lines)


def current_evidence(*, held_batches=True, target_card=True):
    """Generated facts in the canonical Evidence/Runtime encoding, never SDK proof."""
    checks = generated_checks()
    observations = generated_observations(checks)
    def f(name, count, operation, engine=False, digest=None):
        item = {"name": name, "count": count, "operation": operation}
        if engine: item["engine"] = "engine1"
        if digest is not None: item["digest"] = digest
        return item
    tick = 0
    def event(name, operation, engine=False):
        nonlocal tick
        tick += 1
        return f(name, tick, operation, engine)
    publication = []
    for n, kind in enumerate(("createZones", "saveMediaRoot", "savePublishingEpoch",
                              "saveCompleteEpoch", "saveActive"), 1):
        for phase in ("prepared", "enqueued", "replied", "committed"):
            publication.append(event("stateSaved" if phase == "committed" else "applied",
                                     "own%d-%s-%s" % (n, kind, phase)))
    def callback(n, kind):
        return [event("stateSaved" if kind == "state" and phase == "committed" else "engineCallback",
                      "sdk%d-%s-%s" % (n, kind, phase), True)
                for phase in ("delivered", "committed", "returned")]
    publication += callback(1, "state")
    observations[3] += publication
    observations[5].insert(1, f("localDurable", 1, "c1-before-answer", digest="0"*64))
    observations[6][1:1] = [f("applied", 9, "c1-held-projection", digest="0"*64),
                              f("applied", 0, "r1-held-absent")]
    def batch(n, kind, saves):
        return [event("applied", "batch%d-%s-handoff" % (n, kind), True),
                f("applied", saves, "batch%d-save-count" % n, True)]
    # Held-period batches and the target card are optional actual observations;
    # the deterministic HeldView baseline/absence proof above is independent.
    if held_batches:
        observations[6] += batch(1, "collection", 1) + batch(2, "media", 1)
    collection_batch, media_batch = (3, 4) if held_batches else (1, 2)
    upload = batch(collection_batch, "collection", 3) + batch(media_batch, "media", 1)
    upload += callback(2, "sent-batch%d" % collection_batch)
    collection_returned = tick
    upload += callback(3, "sent-batch%d" % media_batch)
    media_returned = tick
    upload += callback(4, "state")
    acks = [f("applied", media_returned if label == "m1" else collection_returned,
              "%s-sdk%d-batch%d-ack" % (label, 3 if label == "m1" else 2,
                                       media_batch if label == "m1" else collection_batch), True)
            for label in ("n1", "c1", "r1", "m1")]
    checks[9][0]["facts"][4:4] = copy.deepcopy(acks)
    observations[9][-1:-1] = acks
    observations[9] += upload
    proof = []
    if held_batches:
        proof += [f("applied", 10, "batch2-s1-media-snapshot", True),
                  f("applied", 10, "batch1-s1-snapshot", True),
                  f("applied", 0, "batch1-r1-absent", True)]
        if target_card:
            proof.append(f("applied", 9, "batch1-c1-held", True, "0"*64))
    proof += [f("applied", 2 if held_batches else 0, "held-batches"),
             f("applied", 1 if held_batches else 0, "held-media-batches"),
             f("applied", 1 if held_batches else 0, "held-collection-batches"),
             f("applied", 1 if held_batches and target_card else 0, "c1-held-handed"),
             f("applied", 0, "r1-held-handed"),
             f("ownedWorkJoined", 5, "own-joined"),
             f("ownedWorkJoined", 4, "sdk-joined", True)]
    checks[10].insert(1, {"name": "coordinatorResult", "outcome": "pass", "facts": copy.deepcopy(proof)})
    observations[10][1:1] = proof
    return checks, observations


class CurrentEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.commands = s.command_bytes(raw(header()))
        self.checks, self.obs = current_evidence()

    def complete(self, n=10):
        journal = s.Journal(raw(header()), self.commands)
        journal.inspect(generated_journal(self.commands, n, self.checks, self.obs), n)
        return s.completion(n, journal.done_checks[n], journal)

    def item(self, label):
        return next(f for facts in self.obs.values() for f in facts if f.get("operation") == label)

    def proof(self, label, **changes):
        self.item(label).update(changes)
        next(f for f in self.checks[10][1]["facts"] if f.get("operation") == label).update(changes)

    def reject(self, label, **changes):
        self.item(label).update(changes)
        with self.assertRaises(s.v.Refused): self.complete()

    def test_exact_current_encoding_validates_every_command_and_check_copies_are_not_ticks(self):
        for n in range(1, 11):
            with self.subTest(command=n): self.assertIsNone(self.complete(n))

    def test_buffered_events_keep_original_aliases_across_command_flushes(self):
        # Own and SDK events are flushed after their initiating command; no
        # assignment of a batch's origin from its flushing command is permitted.
        moved = [f for f in self.obs[6] if s.TRACE.fullmatch(f.get("operation", ""))]
        self.obs[6] = [f for f in self.obs[6] if f not in moved]
        self.obs[7] += moved
        self.assertIsNone(self.complete())

    def test_own_order_kind_origin_and_tick_refuse(self):
        for label, changes in (("own1-createZones-enqueued", {"count": 1}),
                               ("own1-createZones-replied", {"operation": "own2-createZones-replied"}),
                               ("own1-createZones-committed", {"name": "applied"}),
                               ("own2-saveMediaRoot-prepared", {"operation": "own2-unrecognized-prepared"}),
                               ("own1-createZones-prepared", {"engine": "engine1"})):
            with self.subTest(label=label):
                self.checks, self.obs = current_evidence()
                self.reject(label, **changes)

    def test_missing_native_publication_is_missing_even_with_summary_success(self):
        self.obs[3] = [f for f in self.obs[3] if not s.TRACE.fullmatch(f.get("operation", ""))]
        with self.assertRaisesRegex(s.Missing, "smoke-publication-sdk-order"): self.complete(3)
        self.checks, self.obs = current_evidence()
        self.obs[3] = [f for f in self.obs[3] if not f.get("operation", "").startswith("sdk")]
        with self.assertRaisesRegex(s.Missing, "smoke-publication-sdk-order"): self.complete(3)

    def test_shared_tick_duplicate_gap_overflow_and_sdk_engine_refuse(self):
        for changes in ({"count": 20}, {"count": 22}, {"count": 4097}, {"engine": "engine2"},
                        {"name": "stateSaved"}, {"count": True}):
            with self.subTest(changes=changes):
                self.checks, self.obs = current_evidence()
                self.reject("sdk1-state-delivered", **changes)
        self.checks, self.obs = current_evidence()
        self.obs[3].append(copy.deepcopy(self.item("sdk1-state-returned")))
        with self.assertRaises(s.v.Refused): self.complete(3)

    def test_sdk_original_batch_stage_and_returned_ack_link_refuse(self):
        for label, changes in (("sdk2-sent-batch3-committed", {"operation": "sdk2-sent-batch4-committed"}),
                               ("sdk2-sent-batch3-delivered", {"operation": "sdk2-sent-batch3-returned"}),
                               ("sdk2-sent-batch3-returned", {"engine": "other"}),
                               ("batch3-collection-handoff", {"operation": "batch5-collection-handoff"})):
            with self.subTest(label=label):
                self.checks, self.obs = current_evidence()
                self.reject(label, **changes)
        for label in ("n1", "c1", "r1", "m1"):
            for field, value in (("count", 1), ("engine", "other"),
                                 ("operation", label + "-sdk1-batch3-ack")):
                self.checks, self.obs = current_evidence()
                ack = next(f for f in self.checks[9][0]["facts"] if f.get("operation", "").startswith(label+"-sdk"))
                operation = ack["operation"]
                ack[field] = value
                self.item(operation)[field] = value
                with self.subTest(label=label, field=field), self.assertRaises(s.v.Refused): self.complete()

    def test_each_ack_requires_later_native_state_not_entity_summaries(self):
        self.obs[9] = [f for f in self.obs[9] if not f.get("operation", "").startswith("sdk4-")]
        with self.assertRaisesRegex(s.Missing, "smoke-acknowledgement-sdk-order"): self.complete(9)

    def test_invalid_review_image_evidence_is_rejected(self):
        for label, changes in (("c1-before-answer", {"digest": "2"*64}),
                               ("c1-before-answer", {"count": 2}),
                               ("c1-held-projection", {"count": 10}),
                               ("c1-held-projection", {"digest": "2"*64}),
                               ("r1-held-absent", {"count": 1})):
            self.checks, self.obs = current_evidence()
            with self.subTest(label=label): self.reject(label, **changes)
        self.checks, self.obs = current_evidence()
        self.obs[6].remove(self.item("c1-held-projection"))
        self.obs[6].remove(self.item("r1-held-absent"))
        with self.assertRaisesRegex(s.Missing, "smoke-review-held-image"): self.complete(6)

    def test_held_batch_image_kind_counts_and_joins_refuse(self):
        for label, changes in (("batch1-c1-held", {"digest": "2"*64}),
                               ("batch1-c1-held", {"count": 10}),
                               ("batch1-r1-absent", {"count": 1}),
                               ("batch1-s1-snapshot", {"count": 9}),
                               ("batch2-s1-media-snapshot", {"operation": "batch2-s1-snapshot"}),
                               ("held-batches", {"count": 1}),
                               ("held-media-batches", {"count": 0}),
                               ("c1-held-handed", {"count": 0}),
                               ("r1-held-handed", {"count": 1}),
                               ("own-joined", {"count": 6}), ("sdk-joined", {"count": 5})):
            self.checks, self.obs = current_evidence()
            self.proof(label, **changes)
            with self.subTest(label=label), self.assertRaises(s.v.Refused): self.complete()

    def test_zero_observed_batches_with_deterministic_held_view_validates(self):
        self.checks, self.obs = current_evidence(held_batches=False)
        proof = self.checks[10][1]["facts"]
        self.assertFalse(any(f["operation"].startswith("batch") for f in proof))
        self.assertEqual({f["operation"]: f["count"] for f in proof if f["name"] == "applied"},
                         {"held-batches": 0, "held-collection-batches": 0, "held-media-batches": 0,
                          "c1-held-handed": 0, "r1-held-handed": 0})
        self.assertIsNone(self.complete())

    def test_collection_batches_without_target_card_validate(self):
        self.checks, self.obs = current_evidence(target_card=False)
        self.assertEqual(self.item("held-collection-batches")["count"], 1)
        self.assertEqual(self.item("c1-held-handed")["count"], 0)
        self.assertFalse(any(f.get("operation") == "batch1-c1-held" for f in self.obs[10]))
        self.assertIsNone(self.complete())

    def test_zero_counts_must_match_observed_batches_and_handoffs(self):
        for target_card, held_batches, label in (
                (True, False, "held-batches"), (True, False, "held-collection-batches"),
                (True, False, "held-media-batches"), (True, False, "c1-held-handed"),
                (True, False, "r1-held-handed"), (False, True, "c1-held-handed"),
                (False, True, "r1-held-handed")):
            for count in (-1, True, 1):
                with self.subTest(held_batches=held_batches, label=label, count=count):
                    self.checks, self.obs = current_evidence(held_batches=held_batches, target_card=target_card)
                    self.proof(label, count=count)
                    with self.assertRaises(s.v.Refused): self.complete()

    def test_zero_cases_still_require_every_count_and_deterministic_held_view_fact(self):
        for options in ({"held_batches": False}, {"target_card": False}):
            for label in ("held-batches", "held-collection-batches", "held-media-batches",
                          "c1-held-handed", "r1-held-handed", "c1-before-answer",
                          "c1-held-projection", "r1-held-absent"):
                with self.subTest(options=options, label=label):
                    self.checks, self.obs = current_evidence(**options)
                    self.checks[10][1]["facts"] = [f for f in self.checks[10][1]["facts"]
                                                 if f["operation"] != label]
                    for n in (5, 6, 10):
                        self.obs[n] = [f for f in self.obs[n] if f.get("operation") != label]
                    with self.assertRaises(s.v.Refused): self.complete()

    def test_zero_batches_refuse_invented_per_batch_or_card_handoff_proof(self):
        for operation, count, digest in (("batch3-s1-snapshot", 10, None),
                                         ("batch1-c1-held", 9, "0"*64)):
            with self.subTest(operation=operation):
                self.checks, self.obs = current_evidence(held_batches=False)
                invented = {"name": "applied", "count": count, "operation": operation, "engine": "engine1"}
                if digest is not None: invented["digest"] = digest
                self.checks[10][1]["facts"].append(copy.deepcopy(invented))
                self.obs[10].insert(-1, invented)
                with self.assertRaisesRegex(s.v.Refused, "smoke-held-accounting"): self.complete()

    def test_close_proof_needs_observed_copy_and_all_original_work_terminal(self):
        self.obs[10].pop(1)
        with self.assertRaises(s.v.Refused): self.complete()
        self.checks, self.obs = current_evidence()
        self.obs[10].append({"name": "engineCallback", "count": 37, "engine": "engine1",
                             "operation": "sdk5-state-delivered"})
        self.proof("sdk-joined", count=5)
        with self.assertRaisesRegex(s.v.Refused, "smoke-join-order"): self.complete()

    def test_late_callback_keeps_original_command_and_original_append_order(self):
        self.obs[9].append({"name": "engineCallback", "count": 37, "engine": "engine1",
                            "operation": "sdk5-state-delivered"})
        self.proof("sdk-joined", count=5)
        lines = [json.loads(line) for line in generated_journal(self.commands, 10, self.checks, self.obs).splitlines()]
        start_close = next(i for i, line in enumerate(lines) if line["kind"] == "started" and line["command_seq"] == 10)
        phases = [("stateSaved", "committed"), ("engineCallback", "returned")]
        lines[start_close + 1:start_close + 1] = [record(self.commands, 9, 0, "observation", facts=[
            {"name": name, "count": count, "engine": "engine1", "operation": "sdk5-state-" + stage}])
            for count, (name, stage) in enumerate(phases, 38)]
        for seq, line in enumerate(lines, 1):
            line["record_seq"] = seq
        journal = s.Journal(raw(header()), self.commands)
        journal.inspect(b"".join(raw(line) for line in lines), 10)
        self.assertEqual(journal.ordered_trace[journal.done_trace_offsets[9] - 1]["operation"], "sdk5-state-delivered")
        self.assertEqual(journal.ordered_trace[journal.done_trace_offsets[10] - 1]["operation"], "sdk5-state-returned")
        s.completion(9, journal.done_checks[9], journal)
        s.completion(10, journal.done_checks[10], journal)

    def test_generic_pass_cannot_clear_native_fence(self):
        self.checks[3][0]["facts"] = [{"name": "commandCompleted"}]
        with self.assertRaises(s.v.Refused): self.complete(3)
        self.checks, self.obs = current_evidence()
        self.item("sdk1-state-committed")["requested_success"] = True
        with self.assertRaises(s.v.Refused): self.complete(3)


class CompletionMappingTests(unittest.TestCase):
    def setUp(self):
        self.commands = s.command_bytes(raw(header()))
        self.checks = generated_checks()

    def completion(self, n, checks=None, observations=None):
        checks = self.checks if checks is None else checks
        journal = s.Journal(raw(header()), self.commands)
        journal.inspect(generated_journal(self.commands, n, checks, observations), n)
        return s.completion(n, journal.done_checks[n], journal)

    def refused(self, n, checks=None, observations=None):
        with self.assertRaises(s.v.Refused) as error:
            self.completion(n, checks, observations)
        self.assertNotIsInstance(error.exception, s.Missing)

    def test_all_exact_generated_encodings_validate_but_never_prove_smoke(self):
        for n in range(1, 11):
            with self.subTest(command=n):
                if n < 3:
                    self.assertIsNone(self.completion(n))
                else:
                    with self.assertRaisesRegex(s.Missing, "^smoke-publication-sdk-order$"):
                        self.completion(n)

    def test_core_opened_requires_only_typed_generation(self):
        for count in (0, 1, 2**64-1):
            s.fact({"name": "coreOpened", "count": count})
        for value in ({"name": "coreOpened"}, {"name": "coreOpened", "count": True},
                      {"name": "coreOpened", "count": -1}, {"name": "coreOpened", "count": 2**64},
                      {"name": "coreOpened", "count": 1.0}, {"name": "coreOpened", "count": None},
                      {"name": "coreOpened", "count": "1"},
                      {"name": "coreOpened", "count": 1, "digest": "a"*64},
                      {"name": "coreOpened", "count": 1, "operation": "p1"},
                      {"name": "coreOpened", "count": 1, "extra": 0}, {"name": []}):
            with self.subTest(value=value), self.assertRaises(s.v.Refused): s.fact(value)
        for name in ("accountVerified", "ownedWorkJoined"):
            s.fact({"name": name})
            with self.assertRaises(s.v.Refused): s.fact({"name": name, "count": 1})

    def test_each_step_rejects_wrong_check_outcome_kind_extra_and_duplicate(self):
        for n in range(1, 11):
            for key, value in (("name", "recovery"), ("name", []), ("outcome", "open"), ("outcome", "fail"),
                               ("facts", []), ("extra", True)):
                changed = copy.deepcopy(self.checks)
                changed[n][0][key] = value
                with self.subTest(command=n, field=key, value=value): self.refused(n, changed)
            changed = copy.deepcopy(self.checks)
            changed[n].append(copy.deepcopy(changed[n][0]))
            self.refused(n, changed)
            changed = copy.deepcopy(self.checks)
            changed[n][0]["facts"][0]["name"] = "commandCompleted"
            self.refused(n, changed)
            changed = copy.deepcopy(self.checks)
            changed[n][0]["facts"].append(copy.deepcopy(changed[n][0]["facts"][0]))
            self.refused(n, changed)

    def test_each_exact_fact_rejects_missing_extra_wrong_label_count_and_digest(self):
        for n, checks in self.checks.items():
            for ci, check in enumerate(checks):
                for fi, item in enumerate(check["facts"]):
                    mutations = [(key, None) for key in item]
                    mutations += [("engine", "other"), ("operation", "other"), ("error_domain", "core"),
                                  ("digest", "F"*64), ("count", True)]
                    for key, value in mutations:
                        changed = copy.deepcopy(self.checks)
                        target = changed[n][ci]["facts"][fi]
                        if value is None: del target[key]
                        else: target[key] = value
                        with self.subTest(command=n, check=ci, fact=fi, key=key, value=value):
                            self.refused(n, changed)
        for n, index, value in ((2, 0, 19), (5, 0, 2), (7, 0, 0), (8, 2, 10239),
                                (4, 1, 0), (6, 1, 0), (6, 2, 0), (8, 1, 0), (9, 4, 0)):
            changed = copy.deepcopy(self.checks)
            changed[n][0]["facts"][index]["count"] = value
            self.refused(n, changed)

    def test_retained_observations_require_account_and_exact_operation_facts(self):
        for n in range(1, 11):
            for change in ("account", "operation", "duplicate", "order"):
                obs = generated_observations(self.checks)
                if change == "account": obs[n].pop(0)
                elif change == "operation": obs[n].pop()
                elif change == "duplicate": obs[n].append(copy.deepcopy(obs[n][-1]))
                else: obs[n].reverse()
                with self.subTest(command=n, change=change): self.refused(n, observations=obs)
        obs = generated_observations(self.checks)
        obs[4][1]["count"] += 1
        self.refused(4, observations=obs)
        for n, baseline in ((3, 4), (4, 6), (4, 7), (6, 7), (6, 8), (10, 9)):
            changed = copy.deepcopy(self.checks)
            changed[n][0]["facts"][0]["count"] = baseline
            self.refused(n, changed)

    def test_acknowledgements_match_each_retained_revision_and_digest(self):
        for index in range(4):
            for key, value in (("count", 99), ("digest", "f"*64), ("operation", "n1")):
                if index == 0 and key == "operation": continue
                changed = copy.deepcopy(self.checks)
                changed[9][0]["facts"][index][key] = value
                with self.subTest(index=index, key=key): self.refused(9, changed)
        changed = copy.deepcopy(self.checks)
        changed[9][0]["facts"][:4] = list(reversed(changed[9][0]["facts"][:4]))
        with self.assertRaises(s.Missing): self.completion(9, changed)

    def test_media_digest_uses_original_header_and_observation_accounting_is_required(self):
        changed = copy.deepcopy(self.checks)
        changed[8][0]["facts"][2]["digest"] = "f"*64
        self.refused(8, changed)
        for index in range(1, 4):
            obs = generated_observations(self.checks)
            obs[9].pop(index)
            self.refused(9, observations=obs)
        obs = generated_observations(self.checks)
        obs[9][1]["operation"] = "media-unknown"
        self.refused(9, observations=obs)
        # Nonzero accounting is preserved, never silently rewritten to zero.
        obs = generated_observations(self.checks)
        obs[9][1]["count"] = 2
        with self.assertRaises(s.Missing): self.completion(9, observations=obs)

    def test_late_observation_cannot_supply_missing_preterminal_evidence(self):
        observations = generated_observations(self.checks)
        opened = observations[1].pop()
        prefix = generated_journal(self.commands, 1, self.checks, observations)
        late = record(self.commands, 1, len(prefix.splitlines())+1, "observation", facts=[opened])
        journal = s.Journal(raw(header()), self.commands)
        journal.inspect(prefix + raw(late), 1)
        with self.assertRaisesRegex(s.v.Refused, "^smoke-open-observation$"):
            s.completion(1, journal.done_checks[1], journal)

    def test_missing_context_or_changed_terminal_cannot_be_accepted(self):
        with self.assertRaisesRegex(s.Missing, "^smoke-journal-context$"):
            s.completion(1, self.checks[1])
        journal = s.Journal(raw(header()), self.commands)
        journal.inspect(generated_journal(self.commands, 2), 2)
        changed = copy.deepcopy(self.checks[1])
        changed[0]["facts"][0]["count"] += 1
        with self.assertRaisesRegex(s.v.Refused, "^smoke-terminal-context$"):
            s.completion(1, changed, journal)
        del journal.done_checks[1]
        with self.assertRaisesRegex(s.v.Refused, "^smoke-context$"):
            s.completion(2, self.checks[2], journal)

    def test_media_digest_cannot_be_reused_for_another_original_header(self):
        other = header()
        other["device"] = "mac2"
        commands = s.command_bytes(raw(other))
        journal = s.Journal(raw(other), commands)
        journal.inspect(generated_journal(commands, 8), 8)
        with self.assertRaisesRegex(s.v.Refused, "^smoke-media-digest$"):
            s.completion(8, journal.done_checks[8], journal)


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
        for reason in ("execution", "engine_order_not_shown"):
            with self.subTest(reason=reason):
                journal = s.Journal(raw(header()), self.commands)
                start = record(self.commands, 1, 1, "started", action="launch")
                failed = record(self.commands, 1, 2, "failed", failure=reason, error_domain="core", error_code=1)
                late = record(self.commands, 1, 3, "observation", facts=[{"name": "ownedWorkJoined"}])
                prefix = raw(start) + raw(failed) + raw(late)
                _, command_outcomes = journal.inspect(prefix, 1)
                self.assertEqual(command_outcomes, {1: "failed"})
                with self.assertRaises(s.v.Refused):
                    journal.inspect(prefix + raw(record(self.commands, 2, 4, "started", action="connect")), 2)

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
        with self.assertRaisesRegex(s.v.Refused, "^smoke-fact$"):
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
            if self.test.actual_wire:
                self.journal = generated_journal(self.test.commands, n, self.test.wire_checks,
                                                 self.test.wire_observations)
            else:
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
            final = record(self.test.commands, 10, len(self.journal.splitlines())+1,
                           "observation", facts=[{"name": "ownedWorkJoined"}])
            self.journal += raw(final)
            return {"joined": True, "exit_code": 0, "passed": 1, "failed": 0, "skipped": 0}, self.journal
        raise AssertionError(action)

    def close(self):
        self.events.append("original-transport-joined")


class InstallDiagnosticTests(unittest.TestCase):
    def invoke(self, failure):
        output = io.StringIO()
        def deliver(*args, checkpoint):
            checkpoint("recheck-build")
            raise failure
        argv = ["cloud-agent-smoke.py", "--root", "/generated", "--parent-run", RUN,
                "--install-child", "--run-id", RUN, "--source-commit", SHA,
                "--run-directory", "/generated/build/mg/cloudkit-agent-install/" + RUN]
        with patch.object(sys, "argv", argv), patch.object(s.os, "dup", return_value=900), \
                patch.object(s.os, "fdopen", return_value=io.BytesIO(raw({}))), \
                patch.object(s.v, "source", return_value=SHA), \
                patch.object(s, "owned_context", return_value=contextlib.nullcontext((object(), lambda: None))), \
                patch.object(s.i, "deliver", side_effect=deliver), contextlib.redirect_stdout(output):
            self.assertEqual(s.main(), 1)
        return json.loads(output.getvalue())

    def test_install_child_preserves_only_fixed_delivery_stage_and_refusal(self):
        report = self.invoke(s.v.Refused("profile-device"))
        self.assertEqual(report, {"scope": "SMOKE", "outcome": "failed_evidence",
            "category": "smoke-refused", "signed_execution": False,
            "stage": "recheck-build", "refusal": "profile-device"})

    def test_unknown_refusals_and_exception_payloads_never_enter_public_report(self):
        for failure, category in [(s.v.Refused("generated-private-path"), "unclassified"),
                                  (s.v.Refused("profile-device", "generated-private-value"), "unclassified"),
                                  (OSError("generated-private-path"), "filesystem-refused"),
                                  (ValueError("generated-private-value"), "input-shape")]:
            with self.subTest(kind=type(failure).__name__):
                report = self.invoke(failure)
                self.assertEqual(report["refusal"], category)
                self.assertEqual(report["stage"], "recheck-build")
                self.assertNotIn("generated-private", json.dumps(report))


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
        self.actual_wire, self.wire_checks = False, generated_checks()
        self.wire_observations = None
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

    def test_current_wire_accepts_after_original_join_and_keeps_both_prefixes(self):
        self.patchers[1].stop()
        self.actual_wire = True
        self.wire_checks, self.wire_observations = current_evidence()
        result = self.run_parent()
        self.assertEqual(result["outcome"], "smoke_completed")
        self.assertEqual(len(self.transport.commands), 10)
        self.assertEqual(self.transport.events[-2:], ["finish", "original-transport-joined"])
        self.assertEqual((self.local_path / "steps.jsonl").read_bytes(), self.transport.journal)

    def test_missing_held_projection_keeps_prefix_and_never_delivers_release(self):
        self.patchers[1].stop()
        self.actual_wire = True
        self.wire_checks, self.wire_observations = current_evidence()
        self.wire_observations[6] = [f for f in self.wire_observations[6]
                                   if f.get("operation") not in ("c1-held-projection", "r1-held-absent")]
        with self.assertRaisesRegex(s.Missing, "smoke-review-held-image"): self.run_parent()
        self.assertEqual(len(self.transport.commands), 6)
        self.assertEqual(self.transport.events[-1], "original-transport-joined")
        self.assertNotIn("finish", self.transport.events)
        self.assertEqual((self.local_path / "steps.jsonl").read_bytes(), self.transport.retained)

    def test_actual_wire_retains_publication_then_missing_joins_without_successor(self):
        self.patchers[1].stop()
        self.actual_wire = True
        with self.assertRaisesRegex(s.Missing, "^smoke-publication-sdk-order$"): self.run_parent()
        self.assertEqual(len(self.transport.commands), 3)
        self.assertEqual(self.transport.events[2:-1], ["append", "poll", "retain"] * 3)
        self.assertEqual(self.transport.events[-1], "original-transport-joined")
        self.assertNotIn("finish", self.transport.events)
        self.assertEqual((self.local_path / "steps.jsonl").read_bytes(), self.transport.retained)

    def test_actual_wire_wrong_preview_never_delivers_publication(self):
        self.patchers[1].stop()
        self.actual_wire = True
        self.wire_checks[2][0]["facts"][0]["count"] = 19
        with self.assertRaisesRegex(s.v.Refused, "^smoke-fact$"): self.run_parent()
        self.assertEqual(len(self.transport.commands), 2)
        self.assertEqual(self.transport.events[-1], "original-transport-joined")
        self.assertNotIn("finish", self.transport.events)

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


class SmokePlanSelectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.products = Path(self.temp.name).resolve()
        self.path = self.products / "original.xctestrun"

    def fixture(self, selection):
        target = {"BlueprintName": "MeguriSnapshotTests", "OnlyTestIdentifiers": copy.deepcopy(selection),
                  "TestHostPath": "__TESTROOT__/Meguri.app", "EnvironmentVariables": {}, "TestingEnvironmentVariables": {}}
        raw = plistlib.dumps({"TestConfigurations": [{"TestTargets": [target]}]})
        self.path.write_bytes(raw)
        manifest = {"products": {"path": str(self.products)}, "xctestrun": {
            "path": self.path.name, "sha256": s.v.digest(raw), "bytes": len(raw)}}
        return manifest, raw

    def test_actual_smoke_plan_accepts_both_full_orders_without_changing_original(self):
        for selection in (["CloudKitSoakTests", "CloudKitPathPreflightTests"],
                          ["CloudKitPathPreflightTests", "CloudKitSoakTests"]):
            for selected in (False, True):
                with self.subTest(selection=selection, selected=selected):
                    manifest, original = self.fixture(selection)
                    raw, private = s.smoke_plan(manifest, "/generated/private-run", selected=selected)
                    self.assertEqual(raw, original)
                    self.assertEqual(self.path.read_bytes(), original)
                    target = plistlib.loads(private)["TestConfigurations"][0]["TestTargets"][0]
                    self.assertEqual(target["OnlyTestIdentifiers"], ["CloudKitSoakTests"])
                    self.assertEqual(target["EnvironmentVariables"]["MEGURI_CLOUD_AGENT_RUN_DIRECTORY"], "/generated/private-run")
                    self.assertEqual(target["TestHostPath"], str(self.products / "Meguri.app"))

    def test_legacy_soak_only_is_selected_scope_only(self):
        manifest, original = self.fixture(["CloudKitSoakTests"])
        with self.assertRaisesRegex(s.v.Refused, "launch-selection"):
            s.smoke_plan(manifest, "/generated/private-run")
        raw, private = s.smoke_plan(manifest, "/generated/private-run", selected=True)
        self.assertEqual(raw, original)
        self.assertEqual(plistlib.loads(private)["TestConfigurations"][0]["TestTargets"][0]["OnlyTestIdentifiers"], ["CloudKitSoakTests"])
        self.assertEqual(self.path.read_bytes(), original)

    def test_actual_smoke_plan_refuses_extra_missing_duplicate_and_malformed_sets_in_both_scopes(self):
        for selection in (["CloudKitPathPreflightTests"], ["CloudKitSoakTests", "CloudKitSoakTests"],
                          ["CloudKitPathPreflightTests", "CloudKitPathPreflightTests"],
                          ["CloudKitSoakTests", "GeneratedOtherTests"],
                          ["CloudKitSoakTests", "CloudKitPathPreflightTests", "GeneratedOtherTests"],
                          [], "CloudKitSoakTests", ["CloudKitSoakTests", {}]):
            for selected in (False, True):
                with self.subTest(selection=selection, selected=selected):
                    manifest, original = self.fixture(selection)
                    with self.assertRaisesRegex(s.v.Refused, "launch-selection"):
                        s.smoke_plan(manifest, "/generated/private-run", selected=selected)
                    self.assertEqual(self.path.read_bytes(), original)

    def test_actual_smoke_plan_keeps_hash_and_environment_refusals(self):
        manifest, original = self.fixture(["CloudKitPathPreflightTests", "CloudKitSoakTests"])
        manifest["xctestrun"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(s.v.Refused, "launch-template"):
            s.smoke_plan(manifest, "/generated/private-run")
        for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
            plan = plistlib.loads(original)
            plan["TestConfigurations"][0]["TestTargets"][0][key]["TEST_RUNNER_GENERATED"] = "generated"
            raw = plistlib.dumps(plan); self.path.write_bytes(raw)
            manifest["xctestrun"].update(sha256=s.v.digest(raw), bytes=len(raw))
            with self.assertRaisesRegex(s.v.Refused, "launch-environment"):
                s.smoke_plan(manifest, "/generated/private-run")
            self.assertEqual(self.path.read_bytes(), raw)


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
             patch.object(s.h, "setup_process", side_effect=self.launch), \
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
