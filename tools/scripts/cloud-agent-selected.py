#!/usr/bin/env python3
"""Generated J3.2 Mac/VM-iPhone coordinator; never whole-lane evidence."""
import contextlib
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import signal
import sys
import time

_spec = importlib.util.spec_from_file_location("cloud_selected_smoke", Path(__file__).with_name("cloud-agent-smoke.py"))
s = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = s
_spec.loader.exec_module(s)
h, v, i, a = s.h, s.v, s.i, s.a
need, strict = s.need, s.strict
SCOPE = "SELECTED-GENERATED"
PROGRAMS = {
    "mac": (("launch", {}), ("connect", {"preview": "p1"}), ("publish", {"preview": "p1"}),
            ("edit", {"note": "n1", "field": "Q"}), ("sync_now", {}), ("close", {})),
    "iphone": (("launch", {}), ("connect", {"preview": "p1"}), ("adopt", {"preview": "p1"}),
               ("sync_now", {}), ("observe", {"note": "n1"}), ("close", {})),
}
FETCH = re.compile(r"sdk([1-9][0-9]*)-(fetch-start|fetch-zone-start|fetched|fetch-zone-end|fetch-end)-(delivered|committed|returned)")
ACK = re.compile(r"n1-sdk([1-9][0-9]*)-batch([1-9][0-9]*)-ack")
MEDIA = {"media-upload-pending", "media-download-pending", "media-missing", "media-unknown",
         "media-uploads-running", "media-downloads-running", "media-upload-bytes", "media-download-bytes",
         "media-remaining-download-ms", "media-root-acked", "media-zone-missing", "media-no-space"}



# Closed public vocabulary: no exception text or caller-selected labels escape.
PREFLIGHT_STAGES = {"entry", "source", "vm-context", "vm-observation", "lease-acquire", "lease-input",
    "lease-retain", "lease-release", "simulator-pins", "simulator-lease", "simulator-device", "simulator-runtime",
    "simulator-os", "simulator-receipt", "destination-freeze", "fixtures", "execution"}
PREFLIGHT_REFUSALS = {"unclassified", "filesystem-refused", "input-shape", "receipt-invalid", "process-refused",
    "source-dirty", "machine-observation", "child-failed", "child-binding", "child-outcome", "receipt-in-progress",
    "lease-changed", "lease-owner-kind", "lease-expired-or-restarted", "lease-target", "lease-receipt-changed",
    "simulator-kind", "simulator-target", "simulator-kind-state", "simulator-runtime", "vm-simctl", "vm-lease",
    "destination-selected-map", "destination-frozen-selected", "destination-frozen-map", "destination-envelope",
    "destination-entry", "private-authority", "private-changed", "receipt-source", "receipt-outcome",
    "artifact-declaration"}


class Diagnostic:
    def __init__(self): self.stage = "entry"

    def enter(self, stage):
        need(stage in PREFLIGHT_STAGES, "selected-diagnostic-stage")
        self.stage = stage

    def failure(self, error):
        code = "unclassified"
        if isinstance(error, v.Refused) and len(error.args) == 1 and type(error.args[0]) is str:
            if error.args[0] in PREFLIGHT_REFUSALS:
                code = error.args[0]
        elif isinstance(error, OSError): code = "filesystem-refused"
        elif isinstance(error, v.Invalid): code = "receipt-invalid"
        elif isinstance(error, (ValueError, TypeError, KeyError, IndexError, AttributeError, UnicodeError, RecursionError)):
            code = "input-shape"
        elif isinstance(error, s.subprocess.SubprocessError): code = "process-refused"
        return {"scope": SCOPE, "selection": "sync_now", "outcome": "failed_evidence",
                "category": "selected-refused", "signed_execution": False,
                "stage": self.stage if self.stage in PREFLIGHT_STAGES else "entry", "refusal": code}


def sanitized_failure(value, cleanup=None):
    generic = {"scope": SCOPE, "selection": "sync_now", "outcome": "failed_evidence",
               "category": "selected-refused", "signed_execution": False}
    if (isinstance(value, dict) and set(value) == {*generic, "stage", "refusal"} and
            all(type(value[key]) is type(wanted) and value[key] == wanted for key, wanted in generic.items()) and
            type(value["stage"]) is str and value["stage"] in PREFLIGHT_STAGES and
            type(value["refusal"]) is str and value["refusal"] in PREFLIGHT_REFUSALS):
        result = {**generic, "stage": value["stage"], "refusal": value["refusal"]}
    else:
        result = generic
    if cleanup is not None:
        result["cleanup"] = sanitized_cleanup(cleanup)
    return result



def sanitized_cleanup(value):
    unconfirmed = {"drain": "unconfirmed", "reservation": "retained-or-unconfirmed"}
    if not isinstance(value, dict): return unconfirmed
    if (set(value) == {"scope", "outcome"} and value.get("scope") == SCOPE and
            type(value.get("outcome")) is str and value["outcome"] in {"reservation-released", "no-reservation"}):
        return {"drain": "joined", "reservation": "released" if value["outcome"] == "reservation-released" else "none"}
    clean = sanitized_failure(value)
    if "stage" in clean:
        return {"drain": "joined", "reservation": "retained-or-unconfirmed",
                "stage": clean["stage"], "refusal": clean["refusal"]}
    return unconfirmed


def command_bytes(header_raw):
    header = strict(header_raw)
    need(header.get("selection") == "sync_now" and header.get("device_kind") in PROGRAMS, "selected-program")
    return [i.raw_json({"header_sha256": v.digest(header_raw), "command_seq": n, "leg": "sync_now",
                        "action": action, "arguments": arguments, "account_before": "acct1", "account_after": "acct1"})
            for n, (action, arguments) in enumerate(PROGRAMS[header["device_kind"]], 1)]


def exact(item, name, fields=(), **values):
    s.fact(item)
    need(set(item) == {"name", *fields, *values} and item["name"] == name and
         all(item[key] == value for key, value in values.items()), "selected-fact")


def unique(facts):
    values = {}
    for item in facts:
        label = item.get("operation")
        need(label is not None and label not in values, "selected-fact-duplicate")
        values[label] = item
    return values


class Evidence(s.SmokeEvidence):
    """The selected producer adds admission and original fetch callback phases."""
    def __init__(self):
        super().__init__()
        self.admissions = {}
        self.adoption = None

    @staticmethod
    def trace(item):
        operation = item.get("operation", "")
        return bool(s.TRACE.fullmatch(operation) or FETCH.fullmatch(operation) or operation in {
            "normal-adoption-generation-joined", "required-scan-before-native-sdk"})

    def ingest(self, item):
        operation = item.get("operation", "")
        if operation in {"normal-adoption-generation-joined", "required-scan-before-native-sdk"}:
            is_admission = operation == "required-scan-before-native-sdk"
            s.evidence_shape(item, "applied", engine=is_admission)
            self.tick += 1
            need(item["count"] == self.tick <= 4096 and self.last_batch is None, "selected-admission-order")
            if is_admission:
                need(item["engine"] not in self.admissions, "selected-admission-order")
                self.admissions[item["engine"]] = self.tick
            else:
                need(self.adoption is None and not self.admissions, "selected-adoption-order")
                self.adoption = self.tick
            return True
        if self.trace(item) and "engine" in item:
            need(item["engine"] in self.admissions, "selected-scan-before-sdk")
        found = FETCH.fullmatch(operation)
        if found is None:
            return super().ingest(item)
        sdk, kind, phase = found.groups()
        s.evidence_shape(item, "engineCallback", engine=True)
        self.tick += 1
        need(item["count"] == self.tick <= 4096 and self.last_batch is None, "selected-callback-order")
        self.ingest_callback(sdk, kind, None, phase, item["engine"])
        return True

    def acknowledged(self, item):
        found = ACK.fullmatch(item.get("operation", ""))
        need(found is not None, "selected-ack-operation")
        sdk, batch = found.groups()
        exact(item, "applied", ("count", "operation", "engine"))
        callback = self.sdk.get(sdk)
        need(callback is not None and callback[:2] == ["sent", batch] and len(callback[2]) == 3 and
             callback[3] == item["engine"] and callback[2][-1] == item["count"] and self.batches[batch]["saves"] > 0,
             "selected-ack-callback")
        need(self.has_later_state(item["engine"], item["count"]), "selected-ack-state")

    def fetched(self, item):
        exact(item, "engineCallback", ("count", "engine"), operation="n1-original-fetched-returned")
        returned = [(kind, ticks[-1]) for kind, _, ticks, engine in self.sdk.values()
                    if engine == item["engine"] and len(ticks) == 3]
        pages = [tick for kind, tick in returned if kind == "fetched"]
        need(item["count"] in pages, "selected-fetch-return")
        kinds = {kind: [tick for value, tick in returned if value == kind]
                 for kind in ("fetch-start", "fetch-zone-start", "fetch-zone-end", "fetch-end")}
        need(any(start < zone < item["count"] < end < finish for start in kinds["fetch-start"]
                 for zone in kinds["fetch-zone-start"] for end in kinds["fetch-zone-end"]
                 for finish in kinds["fetch-end"]), "selected-fetch-order")

    def joined(self, facts):
        values = unique(facts)
        need(set(values) == {"own-joined", "sdk-joined"}, "selected-join-shape")
        exact(values["own-joined"], "ownedWorkJoined", ("count",), operation="own-joined")
        self.require_joined_engines(values["sdk-joined"])
        need(values["own-joined"]["count"] == len(self.own) and self.last_batch is None and
             all(len(ticks) == 4 for _, ticks in self.own.values()) and
             all(len(ticks) == 3 for _, _, ticks, _ in self.sdk.values()), "selected-join-order")
        for kind, batch, ticks, engine in self.sdk.values():
            if kind == "sent" and self.batches[batch]["saves"]:
                need(self.has_later_state(engine, ticks[-1]), "selected-join-state")


class Journal(s.Journal):
    def __init__(self, header_raw):
        self.kind = strict(header_raw)["device_kind"]
        need(self.kind in PROGRAMS, "selected-program")
        super().__init__(header_raw, command_bytes(header_raw), program=PROGRAMS[self.kind])

    def inspect(self, raw, delivered):
        complete, terminal = super().inspect(raw, delivered)
        self.records = [strict(line) for line in complete.splitlines()]
        # Validate public vocabulary before copying even a partial command's
        # observations to exported evidence. Unknown strings remain private.
        labels = MEDIA | {"p1", "n1", "own-joined", "sdk-joined", "canonical-active", "canonical-complete-epoch",
            "binding-matches-epoch", "required-scan-native-sdk-admitted", "owned-adoption-backup-activation",
            "original-n1-preserved", "original-control-witness", "original-edit-witness", "original-exchange-issued",
            "original-exchange", "selected", "held-back", "newer-pending", "incoming-waiting",
            "n1-earlier-original-ack", "n1-original-ack-after-attempt-boundary-timing-unresolved",
            "n1-original-fetched-returned", "normal-adoption-generation-joined", "required-scan-before-native-sdk"}
        for record in self.records:
            for item in record["facts"] + [f for check in record["checks"] for f in check["facts"]]:
                label = item.get("operation")
                traced = s.TRACE.fullmatch(label or "")
                need(label is None or label in labels or ACK.fullmatch(label) or FETCH.fullmatch(label) or
                     (traced and (traced.group(2) is None or traced.group(2) in s.OWN_KINDS)),
                     "selected-public-fact")
        return complete, terminal

    def completion(self, through):
        need(type(through) is int and 1 <= through <= 6, "selected-command")
        ends = {r["command_seq"]: r["record_seq"] for r in self.records if r["kind"] == "done"}
        need(through in ends, "selected-terminal-context")
        completed = {}
        for number in range(1, through + 1):
            evidence = Evidence()
            for record in self.records:
                if record["kind"] != "observation" or record["record_seq"] > ends[number]:
                    continue
                for item in record["facts"]:
                    traced = evidence.ingest(item)
                    if record["command_seq"] in ends and record["record_seq"] > ends[record["command_seq"]]:
                        need(traced, "selected-late-operation-fact")
            checks = self.done_checks.get(number)
            wanted = {1: ["integrity"], 2: ["coordinatorResult"], 3: ["coordinatorResult"],
                      4: ["localDurability"] if self.kind == "mac" else ["coordinatorResult"],
                      5: ["coordinatorResult"] if self.kind == "mac" else ["localDurability"],
                      6: ["integrity", "coordinatorResult", "localDurability"]}[number]
            if self.kind == "iphone" and number == 3:
                wanted = ["integrity", "coordinatorResult"]
            need(isinstance(checks, list) and [c.get("name") for c in checks] == wanted, "selected-checks")
            facts = []
            for check in checks:
                need(set(check) == {"name", "outcome", "facts"} and check["outcome"] == "pass" and check["facts"],
                     "selected-checks")
                for item in check["facts"]:
                    s.fact(item)
                facts.extend(check["facts"])
            observations = self.done_observations[number]
            ordinary = [item for item in observations if not Evidence.trace(item)]
            need(ordinary and ordinary[0] == {"name": "accountVerified"}, "selected-account")
            expected = [{"name": "accountVerified"}]
            if number == 1:
                need(len(ordinary) == 2, "selected-open")
                exact(ordinary[1], "coreOpened", ("count",))
                exact(facts[0], "integrity", ("count",))
                need(len(facts) == 1 and ordinary[1]["count"] > 0, "selected-open")
                expected.append(ordinary[1])
            else:
                expected.extend(checks[1]["facts"] if self.kind == "iphone" and number == 3 else
                                facts[1:] if number == 6 else facts)
                if number == 3:
                    self.control(facts, evidence)
                elif self.kind == "mac" and number == 4:
                    need(len(facts) in (2, 3), "selected-edit")
                    exact(facts[0], "localCommitted", ("count", "digest"), operation="n1")
                    exact(facts[1], "localDurable", ("count", "digest"), operation="n1")
                    need(facts[0]["count"] > 0 and facts[1]["count"] > 0 and facts[0]["digest"] == facts[1]["digest"],
                         "selected-edit")
                    if len(facts) == 3:
                        exact(facts[2], "localDurable", ("count", "digest"), operation="original-edit-witness")
                        need(facts[2]["count"] > 0, "selected-edit")
                elif (self.kind == "mac" and number == 5) or (self.kind == "iphone" and number == 4):
                    self.exchange(facts, evidence, completed)
                    issued = [f for f in ordinary if f.get("operation") == "original-exchange-issued"]
                    if issued:
                        need(len(issued) == 1, "selected-exchange-issuance")
                        exact(issued[0], "syncResult", ("count", "digest"), operation="original-exchange-issued")
                        need(issued[0]["count"] == facts[0]["count"], "selected-exchange-issuance")
                        expected.insert(1, issued[0])
                elif self.kind == "iphone" and number == 5:
                    need(len(facts) == 2, "selected-peer")
                    exact(facts[0], "localDurable", ("count", "digest"), operation="n1")
                    need(facts[0]["count"] > 0, "selected-peer")
                    evidence.fetched(facts[1])
                elif number == 6:
                    need(len(facts) == 4, "selected-close")
                    exact(facts[0], "integrity", ("count",))
                    exact(facts[-1], "ownedWorkJoined")
                    evidence.joined(checks[1]["facts"])
                elif number == 2:
                    need(facts == [{"name": "applied", "count": 20, "operation": "p1"}], "selected-preview")
            need(ordinary == expected, "selected-observation-copy")
            need(evidence.last_batch is None, "selected-batch-count")
            completed[number] = facts
        return completed

    def control(self, facts, evidence):
        need(evidence.admissions and evidence.sdk and
             all(ticks[0] > evidence.admissions[engine] for _, _, ticks, engine in evidence.sdk.values()),
             "selected-scan-before-sdk")
        need((evidence.adoption is not None) == (self.kind == "iphone"), "selected-adoption-order")
        if self.kind == "iphone":
            exact(facts[0], "integrity", ("count",))
            facts = facts[1:]
        else:
            exact(facts[0], "stateSaved", ("count", "engine"), operation="p1")
            need(facts[0]["engine"] in evidence.admissions, "selected-scan-before-sdk")
            evidence.publication()
            facts = facts[1:]
        values = unique(facts)
        required = {"canonical-active", "canonical-complete-epoch", "binding-matches-epoch", "required-scan-native-sdk-admitted"}
        if self.kind == "iphone":
            required |= {"owned-adoption-backup-activation", "original-n1-preserved"}
        need(required <= set(values) <= required | {"original-control-witness"}, "selected-control")
        for label in ("canonical-active", "canonical-complete-epoch"):
            exact(values[label], "applied", ("digest",), operation=label)
        generation = values["binding-matches-epoch"].get("count")
        need(type(generation) is int and generation > 0, "selected-control")
        for label in ("binding-matches-epoch", "required-scan-native-sdk-admitted"):
            exact(values[label], "applied", count=generation, operation=label)
        if self.kind == "iphone":
            exact(values["owned-adoption-backup-activation"], "applied", count=generation, operation="owned-adoption-backup-activation")
            exact(values["original-n1-preserved"], "localDurable", ("digest",), operation="original-n1-preserved")
        if "original-control-witness" in values:
            exact(values["original-control-witness"], "applied", ("digest",), count=generation, operation="original-control-witness")

    def exchange(self, facts, evidence, completed):
        values = unique(facts)
        required = {"original-exchange", "selected", "held-back", "newer-pending", "incoming-waiting"}
        extra = set(values) - required - MEDIA
        if self.kind == "mac":
            ack = [name for name in extra if ACK.fullmatch(name)]
            timing = [name for name in extra if name in {"n1-earlier-original-ack", "n1-original-ack-after-attempt-boundary-timing-unresolved"}]
            need(len(ack) == len(timing) == 1 and extra == {ack[0], timing[0], "n1"}, "selected-source-ack")
            evidence.acknowledged(values[ack[0]])
            original = completed[4][1]
            exact(values[timing[0]], "stateSaved", count=original["count"], digest=original["digest"], operation=timing[0])
            exact(values["n1"], "localDurable", count=original["count"], digest=original["digest"], operation="n1")
        else:
            need(not extra, "selected-exchange")
        need(required <= set(values), "selected-exchange")
        for label in required | (set(values) & MEDIA):
            exact(values[label], "syncResult", ("count",), operation=label)
        need(values["original-exchange"]["count"] > 0, "selected-exchange")
        for label in {"media-root-acked", "media-zone-missing", "media-no-space"} & set(values):
            need(values[label]["count"] in (0, 1), "selected-media-status")


def compare_controls(mac, phone):
    source = unique(mac.completion(3)[3][1:])
    peer = unique(phone.completion(3)[3][1:])
    for label in ("canonical-active", "canonical-complete-epoch"):
        need(source[label]["digest"] == peer[label]["digest"], "selected-control-comparison")


def compare_note(mac, phone):
    source = mac.completion(5)[4][1]
    peer = phone.completion(5)[5][0]
    need(source["digest"] == peer["digest"], "selected-note-comparison")
    adopted = unique(phone.completion(3)[3][1:])["original-n1-preserved"]
    need(source["digest"] != adopted["digest"], "selected-original-change")


def coordinate(mac, phone):
    for session, numbers in ((mac, (1, 2, 3)), (phone, (1, 2, 3))):
        for number in numbers:
            session.send(number)
    compare_controls(mac.journal, phone.journal)
    for session in (mac, phone):
        for number in (4, 5):
            session.send(number)
    compare_note(mac.journal, phone.journal)
    mac.send(6)
    phone.send(6)
    mac.finish()
    phone.finish()
    return {"scope": SCOPE, "selection": "sync_now", "outcome": "selected_completed", "signed_execution": True,
            "devices": ["mac1", "phone1"], "commands": 12, "controls_equal": True, "note_equal": True,
            "closed_journals": True, "original_app_hosted_joined": True}


def status_outcome(raw):
    if not raw:
        return
    value = strict(raw)
    need(isinstance(value, dict) and set(value) ==
         {"scope", "signed_acceptance", "outcome", "failure", "user_steps", "dependencies"} and
         value["scope"] == "sync_now" and value["signed_acceptance"] is False and
         isinstance(value["user_steps"], list) and isinstance(value["dependencies"], list), "selected-status")
    if value["failure"] == "implementation_missing":
        raise s.Missing("selected-client-capability")
    if value["outcome"] == "awaiting_user" and value["failure"] == "awaiting_user":
        steps = set(value["user_steps"])
        need(steps and steps <= {"cloudkit-dev-container", "cloudkit-test-accounts"}, "selected-user-step")
        raise s.UserStep(sorted(steps)[0])
    need(value["outcome"] == "selected-program-completed" and value["failure"] is None and
         not value["user_steps"], "selected-agent-failed")


class Session:
    def __init__(self, vm, expected, header, files, support, copied, recheck, tree,
                 fixture_receipt, generation, frozen_raw, projection, local, destination):
        self.vm, self.expected, self.header, self.files = vm, expected, header, files
        self.support, self.copied, self.recheck = support, copied, recheck
        self.tree, self.fixture_receipt, self.generation = tree, fixture_receipt, generation
        self.frozen_raw, self.projection, self.local = frozen_raw, projection, local
        self.destination, self.transport, self.private = destination, None, None
        self.journal, self.sent, self.delivered, self.finished = Journal(header.raw), b"", 0, False

    def check(self):
        self.recheck()
        for original in (self.header, self.fixture_receipt, self.generation, self.tree, self.local):
            original.check()
        self.projection.revalidate()
        if self.private is not None:
            self.private.check()

    def __enter__(self):
        try:
            self.private = s.materialize_private(self.projection.host_application_support(), self.header.raw,
                                                self.projection.private_bytes())
            self.check()
            roles = {"fixture-input.json": self.frozen_raw,
                     "fixture-evidence/fixtures-result.json": self.fixture_receipt.raw,
                     "fixture-evidence/generation.json": self.generation.raw,
                     **{"fixture-evidence/parent/" + name: file.raw for name, file in self.tree.files.items()}}
            refs = {name: {"sha256": v.digest(raw), "bytes": len(raw)} for name, raw in roles.items()}
            self.transport = s.Transport(self.vm)
            metadata, derived = self.transport.request("prepare", b"".join(roles[name] for name in sorted(roles)) +
                self.projection.private_bytes(), support=self.support, copied=self.copied,
                manifest=self.files["agent-build.json"].value, header=self.header.raw.decode(), files=refs,
                selected=True, destination=self.destination)
            need(metadata == {"prepared": True}, "selected-prepare")
            run_path = self.support["path"] + "/Meguri/CloudKitSigned/" + self.expected["run_id"] + "/" + self.expected["device"]
            _, expected_plan = s.smoke_plan(self.files["agent-build.json"].value, run_path, selected=True)
            need(derived == expected_plan, "selected-plan")
            h.write_new(self.local.fd, "launch-selection.json", i.raw_json({
                "scope": SCOPE, "test": "MeguriSnapshotTests/CloudKitSoakTests",
                "environment_keys": ["MEGURI_CLOUD_AGENT_RUN_DIRECTORY"],
                "original_xctestrun": self.files["agent-build.json"].value["xctestrun"]}))
            os.fsync(self.local.fd)
            reply, raw = self.transport.request("launch")
            need(reply == {"launched": True} and not raw, "selected-launch")
            return self
        except BaseException:
            self.__exit__(*sys.exc_info())
            raise

    def send(self, number):
        need(number == self.delivered + 1 and number <= 6 and not self.finished and
             all(n in self.journal.done_checks for n in range(1, number)), "selected-command-order")
        self.check()
        command = self.journal.commands[number - 1]
        s.append_exact(self.local, "commands.jsonl", self.sent, command)
        reply, raw = self.transport.request("append", command)
        need(reply == {"appended": True} and not raw, "selected-append")
        self.sent += command
        self.delivered = number
        deadline = time.monotonic() + s.WAIT
        while True:
            self.check()
            metadata, returned = self.transport.request("poll")
            need(set(metadata) == {"alive"} and type(metadata["alive"]) is bool, "selected-poll")
            complete, terminal = self.journal.inspect(returned, number)
            if complete != self.journal.retained:
                s.append_exact(self.local, "steps.jsonl", self.journal.retained, complete[len(self.journal.retained):])
                self.journal.retain(complete)
                reply, raw = self.transport.request("retain", complete)
                need(reply == {"retained": True} and not raw, "selected-retain")
            if terminal.get(number) == "failed":
                _, raw = self.transport.request("status")
                status_outcome(raw)
                failure = next(strict(line).get("failure") for line in complete.splitlines()
                               if strict(line)["kind"] == "failed")
                if failure == "implementation_missing":
                    raise s.Missing("selected-client-capability")
                raise v.Refused("selected-command-failed")
            if terminal.get(number) == "done":
                self.journal.completion(number)
                return
            if not metadata["alive"]:
                _, raw = self.transport.request("status")
                status_outcome(raw)
                raise v.Refused("selected-agent-ended")
            need(time.monotonic() < deadline, "selected-command-timeout")
            select.select([], [], [], min(0.25, max(0, deadline - time.monotonic())))

    def finish(self):
        need(self.delivered == 6 and not self.finished, "selected-finish-order")
        self.check()
        reply, raw = self.transport.request("finish")
        need(reply == {"joined": True, "exit_code": 0, "passed": 1, "failed": 0, "skipped": 0}, "selected-process-result")
        final, terminal = self.journal.inspect(raw, 6)
        need(final == raw and terminal == dict.fromkeys(range(1, 7), "done"), "selected-final-journal")
        s.append_exact(self.local, "steps.jsonl", self.journal.retained, final[len(self.journal.retained):])
        self.journal.retain(final)
        self.journal.completion(6)
        end = next(r["record_seq"] for r in self.journal.records if r["kind"] == "done" and r["command_seq"] == 6)
        need(all(r["record_seq"] <= end for r in self.journal.records), "selected-work-after-close")
        self.transport.child.wait(timeout=s.WAIT)
        need(self.transport.child.returncode == 0, "selected-supervisor-result")
        self.finished = True
        self.check()

    def __exit__(self, *_):
        try:
            if self.transport is not None:
                self.transport.close()
        finally:
            if self.private is not None:
                self.private.close()
            s.close_appends(self.local)


def run_selected(root, parent_run, context, diagnostic=None):
    diagnostic = Diagnostic() if diagnostic is None else diagnostic
    need(v.MG_RUN_ID.fullmatch(parent_run), "selected-parent")
    diagnostic.enter("source")
    sha = v.source(root)
    directory = os.path.join(root, "build/mg/cloudkit-signed", parent_run)
    diagnostic.enter("vm-context")
    with s.owned_context(root, parent_run, context) as (vm, lease_check), contextlib.ExitStack() as held:
        local = h.Directory(directory)
        held.callback(local.close)
        diagnostic.enter("vm-observation")
        observed = i.machine(vm)
        diagnostic.enter("lease-acquire")
        lease_ref = s.child(root, "sim", ["acquire", "iphone", "--holder", "cloud-selected", "--ttl-min", "120"],
                            sha, context, context_key="MG_CLOUD_AGENT_SELECTED_CONTEXT_FD")
        diagnostic.enter("lease-input")
        lease = i.lease_input(root, {"device_kind": "iphone", "source_commit": sha}, lease_ref["run_id"])
        # Release remains under this same VM owner, after both supervisors have
        # joined and the shell's checked orphan drain has completed.
        diagnostic.enter("lease-retain")
        h.write_new(local.fd, "selected-lease.json", i.raw_json(lease_ref))
        os.fsync(local.fd)
        diagnostic.enter("simulator-pins")
        pins = v.json_input(root, root + "/tools/sim-devices.json")
        simulator_build = pins["runtime"]["build"]
        sim_expected = {"device_kind": "iphone", "source_commit": sha, "os_build": simulator_build}
        i.live_simulator(vm, root, sim_expected, lease, observed, checkpoint=diagnostic.enter)
        destinations = {"mac1": {"device_kind": "mac", "vm": vm.selection["name"],
                                 "vm_instance": observed["vm_instance"], "simulator": None},
                        "phone1": {"device_kind": "iphone", "vm": vm.selection["name"],
                                   "vm_instance": observed["vm_instance"], "simulator": {
                                       "udid": lease["udid"], "lease_receipt": lease["lease_receipt"]}}}
        diagnostic.enter("destination-freeze")
        with i.freeze_destinations(parent_run, sha, destinations) as map_check:
            diagnostic.enter("fixtures")
            fixtures = s.child(root, "fixtures", ["cloud-small", "--seed", "1"], sha, context,
                               context_key="MG_CLOUD_AGENT_SELECTED_CONTEXT_FD")
            diagnostic.enter("execution")
            receipt_path = os.path.join(root, "build/mg/fixtures", fixtures["run_id"], "result.json")
            receipt = h.Original(receipt_path)
            held.callback(receipt.close)
            need(receipt.value["placement"] == "docker" and receipt.value["service"] == "dev", "fixture-placement")
            generation = h.Original(os.path.dirname(receipt_path) + "/cloud-small.json")
            held.callback(generation.close)
            g = generation.value
            s.small(g, 1)
            source = g["dir"] if os.path.isabs(g["dir"]) else os.path.join(root, v.relative(g["dir"]))
            v.contained(root + "/build/fixtures", source)
            for path, kind in ((generation.reference["path"], "generation"), (source, "fixture"), (source + "/manifest.json", "manifest")):
                need(sum(item["path"] == path and item["kind"] == kind for item in receipt.value["artifacts"]) == 1,
                     "fixture-declaration")
            entries = [row for row in receipt.value["details"]["fixtures"] if row.get("target") == "cloud-small" and row.get("kind") == "cloud-small"]
            need(len(entries) == 1 and all(entries[0][key] == g[key] for key in ("request", "manifest", "observed")), "generation-binding")
            tree = s.FixtureTree(source)
            held.callback(tree.close)
            need(strict(tree.files["manifest.json"].raw) == g["manifest"], "fixture-manifest")
            file_map = {name: file.reference for name, file in tree.files.items()}
            builds = {}
            for platform in ("mac", "ios-sim"):
                builds[platform] = s.child(root, "build-app", ["--platform", platform, "--config", "Debug", "--test-plan", "CloudAgent"],
                    sha, context, context_key="MG_CLOUD_AGENT_SELECTED_CONTEXT_FD")
            templates, installed, frozen, locals_ = {}, {}, {}, {}
            for label, kind, platform, os_build in (("mac1", "mac", "mac", observed["os_build"]),
                                                   ("phone1", "iphone", "ios-sim", simulator_build)):
                os.mkdir(label, 0o700, dir_fd=local.fd)
                child_dir = h.Directory(directory + "/" + label)
                held.callback(child_dir.close)
                locals_[label] = child_dir
                build, build_hash = v.receipt(root, root + "/build/mg", builds[platform], sha)
                manifest_path = v.declared_reference(root, build, build["details"]["agent_build"])
                manifest = h.Original(manifest_path)
                held.callback(manifest.close)
                m = manifest.value
                need(m["signing"] == "team" and m["platform"] == platform and m["test_plan"] == "CloudAgent", "build-signing")
                _, info = v.bundle(m["products"]["path"], m["app"]["path"], platform)
                need(info.get("MeguriCloudEnvironment") == "development" and isinstance(info.get("MeguriCloudContainer"), str), "development-product")
                if info.get("MeguriCloudSchemaConfigured") is False:
                    raise s.UserStep("cloudkit-dev-container")
                need(info.get("MeguriCloudSchemaConfigured") is True, "schema-configuration")
                frozen[label] = i.raw_json({"protocol_version": 1, "run_id": parent_run, "device": label,
                    "source_commit": sha, "base": "small", "parent_files": file_map, "prepared_source": source, "files": file_map})
                h.write_new(child_dir.fd, "fixture-input.json", frozen[label])
                template = {"protocol_version": 1, "job": "cloudkit-signed", "run_id": parent_run, "lane": "cloud-signed",
                    "source_commit": sha, "tree_clean": True, "device": label, "device_kind": kind, "os_build": os_build,
                    "app_build": info["CFBundleVersion"], "executable_sha256": m["app"]["executable_sha256"],
                    "build_receipt": builds[platform], "build_receipt_sha256": build_hash, "container": info["MeguriCloudContainer"],
                    "environment": "development", "account_scope": "scope-" + parent_run, "account_labels": ["acct1"], "selection": "sync_now",
                    "fixture_delivery": {"base": "small", "fixtures_receipt": fixtures, "source": source,
                        "manifest": {"path": "manifest.json", **file_map["manifest.json"]},
                        "frozen_input": {"path": "fixture-input.json", "sha256": v.digest(frozen[label]), "bytes": len(frozen[label])}}}
                templates[label] = template
                template_path = directory + "/" + label + "/header-template.json"
                h.write_new(child_dir.fd, "header-template.json", i.raw_json(template))
                os.fsync(child_dir.fd)
                args = ["--parent-run", parent_run, "--header-template", template_path]
                if kind != "mac":
                    args += ["--lease-run", lease_ref["run_id"]]
                installed[label] = s.child(root, "cloudkit-agent-install", args, sha, context,
                    context_key="MG_CLOUD_AGENT_SELECTED_CONTEXT_FD")
            need(templates["mac1"]["container"] == templates["phone1"]["container"], "selected-container")

            def consume_mac(expected, header, files, support, copied, recheck):
                original_check = recheck
                def recheck():
                    lease_check(); map_check(); original_check()
                    i.live_simulator(vm, root, sim_expected, lease, i.machine(vm))
                need(expected == {**templates["mac1"], "deployment_receipt": installed["mac1"]}, "final-header-changed")
                with a.approved_account_input(header.raw, expected["container"]) as projection:
                    with Session(vm, expected, header, files, support, copied, recheck, tree, receipt, generation,
                                 frozen["mac1"], projection, locals_["mac1"], destinations["mac1"]) as mac:
                        def consume_phone(expected, header, files, support, copied, recheck):
                            original_check = recheck
                            def recheck():
                                lease_check(); map_check(); original_check()
                                i.live_simulator(vm, root, sim_expected, lease, i.machine(vm))
                            need(expected == {**templates["phone1"], "deployment_receipt": installed["phone1"]}, "final-header-changed")
                            with a.approved_account_input(header.raw, expected["container"]) as peer_projection:
                                need(strict(projection.private_bytes())["accounts"] ==
                                     strict(peer_projection.private_bytes())["accounts"], "selected-account-scope")
                                with Session(vm, expected, header, files, support, copied, recheck, tree, receipt, generation,
                                             frozen["phone1"], peer_projection, locals_["phone1"], destinations["phone1"]) as phone:
                                    return coordinate(mac, phone)
                        return h.handoff_product_evidence(root, parent_run, installed["phone1"]["run_id"], vm, lease_check, consume_phone)
            report = h.handoff_product_evidence(root, parent_run, installed["mac1"]["run_id"], vm, lease_check, consume_mac)
            if report.get("outcome") == "implementation_missing":
                raise s.Missing("selected-client-capability")
            return report



def release_lease(root, parent_run, context, diagnostic=None):
    """Called only after the parent's checked drain under the same VM lease."""
    diagnostic = Diagnostic() if diagnostic is None else diagnostic
    need(v.MG_RUN_ID.fullmatch(parent_run), "selected-parent")
    path = os.path.join(root, "build/mg/cloudkit-signed", parent_run, "selected-lease.json")
    if not os.path.lexists(path):
        return {"scope": SCOPE, "outcome": "no-reservation"}
    diagnostic.enter("vm-context")
    with s.owned_context(root, parent_run, context) as (vm, check), contextlib.ExitStack() as held:
        original = h.Original(path)
        held.callback(original.close)
        ref = original.value
        need(isinstance(ref, dict) and set(ref) == {"job", "run_id"} and ref["job"] == "sim", "selected-lease")
        sha = v.source(root)
        diagnostic.enter("lease-input")
        lease = i.lease_input(root, {"device_kind": "iphone", "source_commit": sha}, ref["run_id"])
        diagnostic.enter("vm-observation")
        observed = i.machine(vm)
        pins = v.json_input(root, root + "/tools/sim-devices.json")
        expected = {"device_kind": "iphone", "source_commit": sha, "os_build": pins["runtime"]["build"]}
        i.live_simulator(vm, root, expected, lease, observed, checkpoint=diagnostic.enter)
        original.check(); check()
        diagnostic.enter("lease-release")
        s.child(root, "sim", ["release", lease["udid"]], sha, context,
                context_key="MG_CLOUD_AGENT_SELECTED_CONTEXT_FD")
        original.check(); check()
    return {"scope": SCOPE, "outcome": "reservation-released"}


def main():
    parser = i.Parser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--parent-run", required=True)
    parser.add_argument("--install-child", action="store_true")
    parser.add_argument("--release-lease", action="store_true")
    parser.add_argument("--sanitize-failure", action="store_true")
    parser.add_argument("--run-id")
    parser.add_argument("--run-directory")
    parser.add_argument("--source-commit")
    parser.add_argument("--lease-run")
    args = parser.parse_args()
    diagnostic = Diagnostic()
    delivery = i.DeliveryDiagnostic() if args.install_child else None
    if args.sanitize_failure:
        value = None
        try:
            need(v.MG_RUN_ID.fullmatch(args.parent_run) and not any((args.install_child, args.release_lease,
                 args.run_id, args.run_directory, args.source_commit, args.lease_run)), "selected-arguments")
            path = os.path.join(args.root, "build/mg/cloudkit-signed", args.parent_run, "selected-report.json")
            original = h.Original(path)
            try: value = original.value
            finally: original.close()
        except (v.Refused, v.Invalid, OSError, ValueError, TypeError, KeyError, UnicodeError):
            pass
        cleanup = None
        try:
            with os.fdopen(os.dup(3), "rb") as private:
                cleanup = strict(private.read(s.CAP + 1))
        except (v.Refused, OSError, ValueError, TypeError, UnicodeError):
            cleanup = {}
        print(json.dumps(sanitized_failure(value, cleanup), sort_keys=True))
        return 0
    try:
        with os.fdopen(os.dup(3), "rb") as private:
            context = strict(private.read(s.CAP + 1))
        need(not (args.install_child and args.release_lease), "selected-arguments")
        if args.install_child:
            need(args.run_id and args.run_directory and args.source_commit == v.source(args.root), "install-source")
            need(args.run_directory == os.path.join(args.root, "build/mg/cloudkit-agent-install", args.run_id), "install-run")
            delivery.enter("vm-context")
            with s.owned_context(args.root, args.parent_run, context) as (vm, check):
                report = i.deliver(args.root, args.run_directory, args.parent_run, args.run_id, args.lease_run, vm,
                                   checkpoint=delivery.enter)
                check()
            print(json.dumps(report, sort_keys=True))
            return 0
        need(not any((args.run_id, args.run_directory, args.source_commit, args.lease_run)), "selected-arguments")
        report = release_lease(args.root, args.parent_run, context, diagnostic) if args.release_lease else run_selected(args.root, args.parent_run, context, diagnostic)
        code = 0
    except a.InputFailure as error:
        report, code = {"scope": SCOPE, "selection": "sync_now", "signed_execution": False, **error.public_outcome()}, 1
        if error.outcome == "awaiting_user": code = 76
        elif error.outcome == "implementation_missing": code = 78
    except s.Missing:
        report, code = {"scope": SCOPE, "outcome": "implementation_missing", "category": "selected-client-capability", "signed_execution": False}, 78
    except s.UserStep as error:
        report, code = {"scope": SCOPE, "outcome": "awaiting_user", "category": error.args[0], "signed_execution": False}, 76
    except i.Queued:
        report, code = {"scope": SCOPE, "outcome": "queued", "category": "vm-unavailable", "signed_execution": False}, 75
    except (v.Refused, v.Invalid, OSError, ValueError, TypeError, KeyError, IndexError, AttributeError,
            UnicodeError, RecursionError, s.subprocess.SubprocessError) as error:
        if delivery is None:
            report = diagnostic.failure(error)
        else:
            report = {"scope": "DELIVERY-ONLY", "outcome": "failed_evidence", "category": "selected-delivery-refused",
                      "signed_execution": False, **delivery.failure(error)}
        code = 1
    print(json.dumps(report, sort_keys=True))
    return code


if __name__ == "__main__":
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(number, h.setup_cancel)
    sys.exit(main())
