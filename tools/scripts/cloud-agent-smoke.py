#!/usr/bin/env python3
"""Internal single-Mac J3.1 parent. FD 3 is MG's retained private VM context.

No public route/account/product-verification switches. The original MG children
produce the inputs; one duplex, owned SSH supervisor retains the app directory
through delivery, launch, polling and original xcodebuild termination. Diagnostics
and private input never become artifacts. This supplies SMOKE scope only.
"""
import contextlib
import datetime
import importlib.util
import inspect
import json
import os
from pathlib import Path
import plistlib
import re
import select
import shlex
import signal
import stat
import subprocess
import sys
import tempfile
import time

_spec = importlib.util.spec_from_file_location(
    "cloud_agent_smoke_accounts", Path(__file__).with_name("cloud-agent-account-input.py"))
a = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = a
_spec.loader.exec_module(a)
h, v, i = a.h, a.v, a.h.i
need, strict, CAP = h.need, h.strict, h.CAP
# Transport bounds, not a new sync-policy deadline or another execution attempt.
WAIT = 600
TREE_CAP = 128 * 1024 * 1024
PROGRAM = (
    ("launch", {}), ("connect", {"preview": "p1"}), ("publish", {"preview": "p1"}),
    ("add_note", {"note": "n1", "deck": "D2"}),
    ("start_session", {"session": "s1", "deck": "D2", "card": "c1"}),
    ("answer", {"session": "s1", "card": "c1", "rating": "good"}),
    ("end_session", {"session": "s1"}), ("add_media", {"media": "m1"}),
    ("sync_now", {}), ("close", {}))
FACTS = {"accountVerified", "coreOpened", "localCommitted", "localDurable", "syncResult",
         "integrity", "ownedWorkJoined", "engineCallback", "stateSaved", "applied", "commandCompleted"}
FAILURES = {"json", "shape", "version", "binding", "prefix", "sequence", "journal", "busy",
            "wrong-container", "unsupported", "execution", "recovery", "storage",
            "implementation_missing", "awaiting_user"}
LABEL = re.compile(r"[A-Za-z][A-Za-z0-9_-]{0,63}")
DIGEST = re.compile(r"[0-9a-f]{64}")


class Missing(v.Refused):
    pass


class UserStep(v.Refused):
    pass


class FrozenFile:
    """Original binary or JSON bytes, with retained no-follow authority."""
    def __init__(self, path):
        self.parent, self.fd = h.Directory(os.path.dirname(path)), None
        self.path, self.name = path, os.path.basename(path)
        try:
            self.fd = os.open(self.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                              dir_fd=self.parent.fd)
            info = os.fstat(self.fd)
            need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == os.getuid()
                 and not info.st_mode & 0o022 and 0 <= info.st_size <= 64 * 1024 * 1024, "input-file")
            self.pin = h.file_pin(info)
            self.raw = os.pread(self.fd, info.st_size + 1, 0)
            need(len(self.raw) == info.st_size, "input-size")
            self.reference = {"sha256": v.digest(self.raw), "bytes": len(self.raw)}
            self.check()
        except BaseException:
            self.close()
            raise

    def check(self):
        self.parent.check()
        need(h.file_pin(os.fstat(self.fd)) == self.pin and h.file_pin(os.stat(
            self.name, dir_fd=self.parent.fd, follow_symlinks=False)) == self.pin and
            os.pread(self.fd, len(self.raw) + 1, 0) == self.raw, "input-changed")

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None
        self.parent.close()


class FixtureTree:
    def __init__(self, source):
        self.root, self.media, self.files = h.Directory(source), None, {}
        try:
            self.media = h.Directory(source + "/collection.media")
            names = set(os.listdir(self.root.fd))
            need(names in ({"collection.anki2", "manifest.json", "collection.media"},
                           {"collection.anki2", "manifest.json", "collection.media", "collection.anki2-wal"}),
                 "fixture-files")
            media = os.listdir(self.media.fd)
            need(len(media) == 5 and all(v.relative(name) == name and "/" not in name for name in media),
                 "fixture-media")
            names.remove("collection.media")
            self.names = sorted(names | {"collection.media/" + name for name in media})
            for name in self.names:
                self.files[name] = FrozenFile(source + "/" + name)
            need(sum(len(f.raw) for f in self.files.values()) <= TREE_CAP and
                 all(len(self.files["collection.media/" + name].raw) == 10240 for name in media), "fixture-size")
            self.check()
        except BaseException:
            self.close()
            raise

    def check(self):
        self.root.check()
        self.media.check()
        need(set(os.listdir(self.root.fd)) == {name for name in self.names if "/" not in name} |
             {"collection.media"} and set(os.listdir(self.media.fd)) ==
             {name.split("/")[1] for name in self.names if "/" in name}, "fixture-changed")
        for f in self.files.values():
            f.check()

    def close(self):
        for f in self.files.values():
            f.close()
        if self.media is not None:
            self.media.close()
        self.root.close()


def small(g, seed):
    request = {"seed": seed, "now_ms": 1735689600000, "utc_offset_secs": 0, "rollover_hour": 4,
               "notes": 20, "cards": 20, "media": "files", "media_files": 5, "media_file_bytes": 10240,
               "reviews_per_card": 3, "review_days_before_pinned": [3, 2, 1],
               "answer_seconds_after_day_start": 28800, "answer_ordinal_seconds": True}
    need(g["request"] == request and all(type(g["request"][key]) is type(value)
                                        for key, value in request.items()), "fixture-request")
    m, observed = g["manifest"], g["observed"]
    counts = dict(notes=20, cards=20, revlog=60, notetypes=1, decks=2, presets=1, tags=20,
                  media_refs=5, media_files=5, media_bytes=51200)
    need(m["generator_version"] == 5 and m["kind"] == m["name"] == "cloud-small" and
         m["seed"] == seed and m["now_ms"] == request["now_ms"] and m["utc_offset_secs"] == 0 and
         m["cards_requested"] == 20 and m["media"] == "files" and m["profile_sha256"] is None and
         m["worst_case_sha256"] is None and m["counts"] == counts and observed["counts"] == counts and
         DIGEST.fullmatch(m["dump_sha256"]) and observed["dump_sha256"] == m["dump_sha256"], "fixture-readback")


@contextlib.contextmanager
def owned_context(root, parent, context):
    need(isinstance(context, dict) and set(context) == {"selection", "lease_slot"}, "private-context")
    holder = h.Original(os.path.join(context["lease_slot"], "holder.json"))
    try:
        need(holder.value.get("job") == "cloudkit-signed" and holder.value.get("run_id") == parent and
             holder.value.get("worktree") == root and type(holder.value.get("pid")) is int and
             holder.value["pid"] > 1, "vm-lease-owner")

        def check():
            holder.check()
            os.kill(holder.value["pid"], 0)
            raw = v.command(["/bin/ps", "-A", "-o", "pid=", "-o", "ppid="], "vm-lease-owner").stdout
            parents = dict(tuple(map(int, line.split())) for line in raw.splitlines())
            pid = os.getpid()
            for _ in range(200):
                if pid == holder.value["pid"]:
                    return
                pid = parents.get(pid, 0)
            raise v.Refused("vm-lease-owner")

        check()
        yield h.VM(root, context["selection"], check), check
        check()
    finally:
        holder.close()


def child(root, job, args, sha, context=None, *, context_key="MG_CLOUD_AGENT_SMOKE_CONTEXT_FD"):
    """Fresh MG child, original returned result, synchronous exit, no latest lookup."""
    env = dict(os.environ, MEGURI_APP_SIGNING="team")
    need(context_key in {"MG_CLOUD_AGENT_SMOKE_CONTEXT_FD", "MG_CLOUD_AGENT_SELECTED_CONTEXT_FD"}, "child-context")
    env.pop("MG_CLOUD_AGENT_SMOKE_CONTEXT_FD", None)
    env.pop("MG_CLOUD_AGENT_SELECTED_CONTEXT_FD", None)
    with contextlib.ExitStack() as held:
        descriptors = ()
        if context is not None:
            private = held.enter_context(tempfile.TemporaryFile())
            private.write(i.raw_json(context))
            private.seek(0)
            descriptors = (private.fileno(),)
            env[context_key] = str(private.fileno())
            if context_key == "MG_CLOUD_AGENT_SELECTED_CONTEXT_FD":
                # All selected children use the already retained VM. This file
                # stays private and alive until the original child has exited.
                selection = context["selection"]
                env["MEGURI_SIM_ALLOW_HOST"] = "0"
                env["MEGURI_MAC_UI_ALLOW_HOST"] = "0"
                config = held.enter_context(tempfile.NamedTemporaryFile(mode="w+b"))
                config.write(i.raw_json({"vms": [{key: selection[key] for key in ("name", "tart", "ssh")}]}))
                config.flush()
                os.fsync(config.fileno())
                env["MEGURI_MAC_VM_CONFIG"] = config.name
        out, err = held.enter_context(tempfile.TemporaryFile()), held.enter_context(tempfile.TemporaryFile())
        process = None
        try:
            with h.setup_launch():
                process = h.setup_process([os.path.join(root, "tools/mg"), job, *args], env=env,
                                           stdout=out, stderr=err, pass_fds=descriptors, start_new_session=True)
            process.wait(timeout=3600)
        finally:
            if process is not None and process.poll() is None:
                h.setup_join(process)
        need(out.tell() <= CAP, "child-output")
        out.seek(0)
        path = out.read(CAP + 1).decode().strip()
        directory = os.path.join(root, "build/mg", job)
        v.contained(directory, path)
        need(os.path.basename(path) == "result.json", "child-result")
        original = h.Original(path)
        held.callback(original.close)
        r = original.value
        v._first_sync_envelope(r)
        need(r["job"] == job and r["root"] == root and r["commit"] == {"sha": sha, "dirty": False}
             and r["args"] == args and not r["dry_run"] and r["exit_code"] == process.returncode and
             os.path.dirname(path) == os.path.join(directory, r["run_id"]), "child-binding")
        h.exited(r["run_id"], os.path.dirname(path))
        if r["status"] != "pass" or process.returncode != 0:
            # Only observed existing, user-owned prerequisites can be assisted.
            steps = {step.get("id") for step in r["user_steps"]}
            if r["status"] == "warn" and r["details"].get("outcome") == "awaiting_user" and steps and \
                    steps <= {"cloudkit-dev-container", "cloudkit-test-accounts"} and not r["failures"]:
                raise UserStep(sorted(steps)[0])
            if job in {"cloudkit-agent-install", "sim"} and r["status"] == "warn" and not r["failures"] and \
                    r["details"].get("outcome") == "queued" and r["details"].get("queued") is True and \
                    (job != "sim" or steps == {"mac-vm"}):
                raise i.Queued("vm-unavailable")
            raise v.Refused("child-failed")
        need(not r["failures"] and not r["user_steps"] and r["summary"]["failed"] == 0 and
             v.source(root) == sha, "child-outcome")
        return {"job": job, "run_id": r["run_id"]}


def materialize_private(support, header, raw):
    """Same fixed J2.2 path on host and guest, exclusive publication/readback."""
    value = strict(raw)
    fields = ("job", "run_id", "source_commit", "device", "executable_sha256", "container",
              "environment", "account_scope")
    need(set(value) == {"protocol_version", "header_sha256", "accounts", *fields} and
         type(value["protocol_version"]) is int and value["protocol_version"] == 1 and
         value["header_sha256"] == v.digest(header) and
         all(value[k] == strict(header)[k] for k in fields), "private-binding")
    accounts = value["accounts"]
    need(isinstance(accounts, dict) and set(accounts) == {"acct1"} and
         isinstance(accounts["acct1"], list) and accounts["acct1"] and
         all(type(b) is int and 0 <= b <= 255 for b in accounts["acct1"]), "private-labels")
    need(len(raw) <= 1048576, "private-size")
    root = h.Directory(support)
    with contextlib.ExitStack() as held:
        held.callback(root.close)
        checks, fd = [root.check], root.fd
        for name in ("Meguri", "CloudKitSignedPrivate", value["run_id"], value["device"]):
            try:
                os.mkdir(name, 0o700, dir_fd=fd)
                os.fsync(fd)
            except FileExistsError:
                pass
            fd, check = h.private_child(fd, name, held)
            checks.append(check)
            need(stat.S_IMODE(os.fstat(fd).st_mode) == 0o700, "private-mode")
        need(not os.listdir(fd), "private-preexisting")
        h.write_new(fd, "expected-accounts.json", raw)
        os.fsync(fd)
        original = FrozenFile(support + "/Meguri/CloudKitSignedPrivate/" + value["run_id"] + "/" +
                              value["device"] + "/expected-accounts.json")
        try:
            need(original.raw == raw and stat.S_IMODE(os.fstat(original.fd).st_mode) == 0o600,
                 "private-readback")
            for check in checks:
                check()
            return original
        except BaseException:
            original.close()
            raise


def command_bytes(header_raw):
    return [i.raw_json({"header_sha256": v.digest(header_raw), "command_seq": n, "leg": "smoke",
                        "action": action, "arguments": arguments,
                        "account_before": "acct1", "account_after": "acct1"})
            for n, (action, arguments) in enumerate(PROGRAM, 1)]


def fact(value):
    need(isinstance(value, dict) and {"name"} <= set(value) <=
         {"name", "count", "digest", "engine", "operation", "error_domain", "error_code"} and
         isinstance(value["name"], str) and value["name"] in FACTS, "journal-fact")
    if value["name"] == "accountVerified":
        need(set(value) == {"name"}, "journal-private-fact")
    if value["name"] == "ownedWorkJoined":
        need(set(value) == {"name"} or
             (set(value) == {"name", "count", "operation"} and value["operation"] == "own-joined") or
             (set(value) == {"name", "count", "operation", "engine"} and
              value["operation"] == "sdk-joined" and value["engine"] == "engine1"), "journal-private-fact")
    if value["name"] == "coreOpened":
        need(set(value) == {"name", "count"}, "journal-core-opened")
    if "count" in value:
        need(type(value["count"]) is int and 0 <= value["count"] < 2**64, "journal-count")
    if "digest" in value:
        need(isinstance(value["digest"], str) and DIGEST.fullmatch(value["digest"]), "journal-digest")
    for key in ("engine", "operation"):
        if key in value:
            need(isinstance(value[key], str) and LABEL.fullmatch(value[key]), "journal-label")
    need(("error_domain" in value) == ("error_code" in value), "journal-error")
    if "error_domain" in value:
        need(value["error_domain"] in ("agent", "core", "cloudKit", "cancellation") and
             type(value["error_code"]) is int and -2**63 <= value["error_code"] < 2**63, "journal-error")


def _completion_facts(number, checks):
    """Exact Runtime checks; chronological evidence is validated separately."""
    need(type(number) is int and 1 <= number <= len(PROGRAM), "smoke-command")
    names = {1: ["integrity"], 2: ["coordinatorResult"], 3: ["coordinatorResult"],
             4: ["localDurability"], 5: ["localDurability"], 6: ["localDurability"],
             7: ["localDurability"], 8: ["localDurability"], 9: ["coordinatorResult"],
             10: ["integrity", "localDurability"]}[number]
    if number == 10 and isinstance(checks, list) and len(checks) == 3:
        names = ["integrity", "coordinatorResult", "localDurability"]
    need(isinstance(checks, list) and len(checks) == len(names), "smoke-checks")
    facts = []
    for check, name in zip(checks, names):
        need(isinstance(check, dict) and set(check) == {"name", "outcome", "facts"} and
             check["name"] == name and check["outcome"] == "pass" and
             isinstance(check["facts"], list) and check["facts"], "smoke-checks")
        for item in check["facts"]:
            fact(item)
        facts.extend(check["facts"])

    def exact(item, name, fields=(), **values):
        need(set(item) == {"name", *fields, *values} and item["name"] == name and
             all(item[key] == value for key, value in values.items()), "smoke-fact")

    def revision(item, label):
        exact(item, "localDurable", ("count", "digest"), operation=label)
        need(item["count"] > 0, "smoke-revision")

    sizes = {1: 1, 2: 1, 3: 1, 4: 2, 5: 1, 6: 3, 7: 1, 8: 3, 9: 5, 10: 2}
    need(len(facts) == sizes[number] or number == 9 and len(facts) == 9 or
         number == 10 and len(checks) == 3 and len(facts) >= 9, "smoke-facts")
    if number in (1, 10):
        exact(facts[0], "integrity", ("count",))
        if number == 10:
            need(len(checks[0]["facts"]) == len(checks[-1]["facts"]) == 1, "smoke-close-checks")
            exact(facts[-1], "ownedWorkJoined")
    elif number == 2:
        exact(facts[0], "applied", count=20, operation="p1")
    elif number == 3:
        exact(facts[0], "stateSaved", ("count",), engine="engine1", operation="p1")
    elif number in (4, 6, 8):
        label = {4: "n1", 6: "s1", 8: "m1"}[number]
        exact(facts[0], "localCommitted", ("count",), operation=label)
        need(facts[0]["count"] > 0, "smoke-commit")
        revision(facts[1], {4: "n1", 6: "c1", 8: "m1"}[number])
        if number == 6:
            revision(facts[2], "r1")
        elif number == 8:
            exact(facts[2], "localDurable", ("digest",), count=10240, operation="m1")
    elif number in (5, 7):
        exact(facts[0], "applied", count=1, operation={5: "c1", 7: "s1"}[number])
    elif number == 9:
        # Core's entity enumeration order is not a wire guarantee.
        need({f.get("operation") for f in facts[:4]} == {"n1", "c1", "r1", "m1"},
             "smoke-ack-labels")
        for item in facts[:4]:
            exact(item, "stateSaved", ("count", "digest"), operation=item["operation"])
            need(item["count"] > 0, "smoke-revision")
        if len(facts) == 9:
            need({f.get("operation", "").split("-")[0] for f in facts[4:8]} ==
                 {"n1", "c1", "r1", "m1"}, "smoke-ack-labels")
            for item in facts[4:8]:
                exact(item, "applied", ("count", "operation"), engine="engine1")
                need(re.fullmatch(r"[ncrm]1-sdk[1-9][0-9]*-batch[1-9][0-9]*-ack", item["operation"]),
                     "smoke-ack-labels")
        exact(facts[-1], "syncResult", ("count",), operation="exchange1")
        need(facts[-1]["count"] > 0, "smoke-exchange")
    return facts


# These are aliases and fields emitted by CloudKitSmokeEvidence, never native
# identities, envelopes or state. Check copies are deliberately not ingested.
OWN_KINDS = {"createZones", "saveMediaRoot", "savePublishingEpoch", "saveCompleteEpoch",
             "saveActive", "readOwnEpoch", "readResetMediaRoot"}
TRACE = re.compile(r"(?:own([1-9][0-9]*)-([A-Za-z]+)-(prepared|enqueued|replied|committed)|"
                   r"sdk([1-9][0-9]*)-(state|sent)(?:-batch([1-9][0-9]*))?-(delivered|committed|returned)|"
                   r"batch([1-9][0-9]*)-(collection-handoff|media-handoff|save-count))")


def evidence_shape(item, name, *, engine=False, digest=False):
    need(set(item) == {"name", "count", "operation"} |
         ({"engine"} if engine else set()) | ({"digest"} if digest else set()) and
         item["name"] == name and (not engine or item["engine"] == "engine1"), "smoke-evidence-shape")


class SmokeEvidence:
    """One shared chronological ledger across all command drains."""
    def __init__(self):
        self.tick, self.own, self.sdk, self.batches = 0, {}, {}, {}
        self.last_batch = None

    def ingest(self, item):
        match = TRACE.fullmatch(item.get("operation", ""))
        if match is None:
            return False
        own, kind, phase, sdk, event, batch, stage, handed, handoff = match.groups()
        if handoff == "save-count":
            evidence_shape(item, "applied", engine=True)
            need(handed == self.last_batch and self.batches[handed]["saves"] is None,
                 "smoke-batch-save-count")
            self.batches[handed]["saves"] = item["count"]
            self.last_batch = None
            return True
        need(self.last_batch is None, "smoke-batch-save-count")
        self.tick += 1
        need(item.get("count") == self.tick <= 4096, "smoke-evidence-tick")
        if own:
            need(kind in OWN_KINDS and int(own) <= 256, "smoke-own-kind")
            evidence_shape(item, "stateSaved" if phase == "committed" else "applied")
            if phase == "prepared":
                need(int(own) == len(self.own) + 1, "smoke-own-origin")
                self.own[own] = [kind, []]
            need(own in self.own and self.own[own][0] == kind, "smoke-own-origin")
            phases = self.own[own][1]
            need(len(phases) < 4 and phase == ("prepared", "enqueued", "replied", "committed")[len(phases)],
                 "smoke-own-order")
            phases.append(self.tick)
        elif sdk:
            evidence_shape(item, "stateSaved" if event == "state" and stage == "committed" else "engineCallback",
                           engine=True)
            need((event == "sent") == (batch is not None) and int(sdk) <= 1024, "smoke-sdk-origin")
            if stage == "delivered":
                need(int(sdk) == len(self.sdk) + 1 and all(len(c[2]) == 3 for c in self.sdk.values()),
                     "smoke-sdk-origin")
                need(batch is None or batch in self.batches, "smoke-sdk-handoff")
                self.sdk[sdk] = [event, batch, []]
            need(sdk in self.sdk and self.sdk[sdk][:2] == [event, batch], "smoke-sdk-origin")
            phases = self.sdk[sdk][2]
            need(len(phases) < 3 and stage == ("delivered", "committed", "returned")[len(phases)],
                 "smoke-sdk-order")
            phases.append(self.tick)
        else:
            evidence_shape(item, "applied", engine=True)
            need(int(handed) == len(self.batches) + 1 and int(handed) <= 256, "smoke-batch-origin")
            self.batches[handed] = {"kind": handoff, "tick": self.tick, "saves": None}
            self.last_batch = handed
        return True

    def publication(self):
        required = OWN_KINDS - {"readOwnEpoch", "readResetMediaRoot"}
        if not all(len(ticks) == 4 for _, ticks in self.own.values()) or not required <= {
                k for k, ticks in self.own.values() if len(ticks) == 4} or not any(
                kind == "state" and len(ticks) == 3 for kind, _, ticks in self.sdk.values()):
            raise Missing("smoke-publication-sdk-order")

    def acknowledgements(self, facts):
        if len(facts) != 9:
            raise Missing("smoke-acknowledgement-sdk-order")
        acknowledged = {}
        for item in facts[4:8]:
            entity, sdk, batch, _ = item["operation"].split("-")
            acknowledged.setdefault(batch[5:], set()).add(entity)
            callback = self.sdk.get(sdk[3:])
            need(callback is not None and callback[:2] == ["sent", batch[5:]] and len(callback[2]) == 3 and
                 item["count"] == callback[2][-1], "smoke-ack-callback")
            need(self.batches[batch[5:]]["saves"] > 0, "smoke-ack-handoff")
            if not any(kind == "state" and len(ticks) == 3 and ticks[1] > item["count"]
                       for kind, _, ticks in self.sdk.values()):
                raise Missing("smoke-acknowledgement-sdk-order")
        need(all(len(entities) <= self.batches[batch]["saves"] for batch, entities in acknowledged.items()),
             "smoke-ack-handoff-count")

    def joined(self, proof, baseline, answer):
        if not proof:
            raise Missing("smoke-owned-work-join")
        labels = {}
        for item in proof:
            label = item.get("operation")
            need(label not in labels, "smoke-held-duplicate")
            labels[label] = item
            if item["name"] == "ownedWorkJoined":
                fact(item)
            else:
                evidence_shape(item, "applied", engine=True, digest=bool(re.fullmatch(
                    r"batch[1-9][0-9]*-c1-held", label or "")))
        def take(label):
            if label not in labels:
                raise Missing("smoke-review-held-image")
            return labels.pop(label)
        for label, count in (("own-joined", len(self.own)), ("sdk-joined", len(self.sdk))):
            item = take(label)
            evidence_shape(item, "ownedWorkJoined", engine=label == "sdk-joined")
            need(item["count"] == count and count > 0, "smoke-join-count")
        need(self.last_batch is None and all(len(t) == 4 for _, t in self.own.values()) and
             all(len(t) == 3 for _, _, t in self.sdk.values()), "smoke-join-order")
        counts = {label: take(label)["count"] for label in
                  ("held-batches", "held-collection-batches", "held-media-batches",
                   "c1-held-handed", "r1-held-handed")}
        collection, media, cards = set(), set(), 0
        for batch, info in self.batches.items():
            prefix = "batch" + batch
            snapshot = labels.pop(prefix + "-s1-snapshot", None)
            media_snapshot = labels.pop(prefix + "-s1-media-snapshot", None)
            need(not (snapshot and media_snapshot), "smoke-held-batch-kind")
            if snapshot is not None or media_snapshot is not None:
                held = snapshot if snapshot is not None else media_snapshot
                need(held["count"] >= answer["count"] and info["saves"] is not None and
                     info["kind"] == ("collection-handoff" if snapshot is not None else "media-handoff"),
                     "smoke-held-snapshot")
                (collection if snapshot is not None else media).add(batch)
            if snapshot is not None:
                absent = take(prefix + "-r1-absent")
                need(absent["count"] == 0, "smoke-held-review")
                card = labels.pop(prefix + "-c1-held", None)
                if card is not None:
                    cards += 1
                    need(info["saves"] > 0 and card["digest"] == baseline["digest"] and
                         card["count"] < answer["count"], "smoke-held-card")
        need(not labels and counts == {"held-batches": len(collection) + len(media),
             "held-collection-batches": len(collection), "held-media-batches": len(media),
             "c1-held-handed": cards, "r1-held-handed": 0}, "smoke-held-accounting")


def completion(command_number, checks, journal=None):
    _completion_facts(command_number, checks)
    if journal is None:
        raise Missing("smoke-journal-context")
    need(journal.done_checks.get(command_number) == checks, "smoke-terminal-context")
    previous, evidence = {}, SmokeEvidence()
    baseline, held_projection, missing = None, None, []
    for number in range(1, command_number + 1):
        need(number in journal.done_checks and number in journal.done_observations, "smoke-context")
        facts = _completion_facts(number, journal.done_checks[number])
        observations = []
        need(journal.done_observations[number] and
             journal.done_observations[number][0] == {"name": "accountVerified"}, "smoke-operation-observations")
        for item in journal.done_observations[number]:
            fact(item)
            if not evidence.ingest(item):
                observations.append(item)
        expected = [{"name": "accountVerified"}]
        if number == 1:
            need(len(observations) == 2, "smoke-open-observation")
            opened = observations[1]
            need(opened["name"] == "coreOpened", "smoke-open-observation")
            expected.append(opened)
        elif number == 10:
            proof = journal.done_checks[10][1]["facts"] if len(journal.done_checks[10]) == 3 else []
            expected.extend(proof + [facts[-1]])
        elif number == 9:
            need(len(observations) == len(facts) + 4, "smoke-media-accounting")
            for item, label in zip(observations[1:4], ("media-pending", "media-missing", "media-unknown")):
                need(set(item) == {"name", "count", "operation"} and item["name"] == "syncResult"
                     and item["operation"] == label, "smoke-media-accounting")
            expected.extend(observations[1:4] + facts)
            retained = {"n1": previous[4][1], "c1": previous[6][1],
                        "r1": previous[6][2], "m1": previous[8][1]}
            for item in facts[:4]:
                original = retained[item["operation"]]
                need((item["count"], item["digest"]) == (original["count"], original["digest"]),
                     "smoke-ack-revision")
        else:
            if number == 5:
                if len(observations) == 2:
                    missing.append("smoke-review-held-image")
                else:
                    need(len(observations) >= 3, "smoke-held-baseline")
                    baseline = observations[1]
                    evidence_shape(baseline, "localDurable", digest=True)
                    need(baseline["operation"] == "c1-before-answer" and baseline["count"] > 0, "smoke-held-baseline")
                    expected.append(baseline)
            elif number == 6:
                if len(observations) == 4:
                    missing.append("smoke-review-held-image")
                else:
                    need(baseline is not None and len(observations) >= 6, "smoke-held-baseline")
                    held_projection, absent = observations[1:3]
                    evidence_shape(held_projection, "applied", engine=True, digest=True)
                    evidence_shape(absent, "applied", engine=True)
                    need(held_projection["operation"] == "c1-held-projection" and
                         held_projection["digest"] == baseline["digest"] and
                         held_projection["count"] < facts[0]["count"] and absent["operation"] == "r1-held-absent" and
                         absent["count"] == 0 and facts[1]["count"] > baseline["count"] and
                         facts[1]["digest"] != baseline["digest"], "smoke-held-projection")
                    expected.extend([held_projection, absent])
            expected.extend(facts)
        need(observations == expected, "smoke-operation-observations")
        if number in (3, 4, 6, 10):
            prior_seq = previous[{3: 1, 4: 3, 6: 4, 10: 6}[number]][0]["count"]
            need(facts[0]["count"] > prior_seq if number in (4, 6) else
                 facts[0]["count"] >= prior_seq, "smoke-sequence")
        if number == 8:
            header = journal.header_value
            need(isinstance(header.get("run_id"), str) and isinstance(header.get("device"), str), "smoke-media-context")
            media = b"".join(bytes.fromhex(v.digest(
                ("MeguriCloudSmoke/%s/%s/m1/%d" % (header["run_id"], header["device"], n)).encode()))
                for n in range(320))
            need(facts[2]["digest"] == v.digest(media), "smoke-media-digest")
        previous[number] = facts
    # Buffered traces belong to their original aliases, regardless of which
    # command drains them. Summary stateSaved/entity facts never stand in for SDK.
    need(evidence.last_batch is None, "smoke-batch-save-count")
    if command_number >= 3:
        evidence.publication()
    if missing:
        raise Missing(missing[0])
    if command_number >= 7 and held_projection is None:
        raise Missing("smoke-review-release-order")
    if command_number >= 9:
        evidence.acknowledgements(previous[9])
    if command_number == 10:
        evidence.joined(proof, baseline, previous[6][0])


class Journal:
    """Original-prefix and fixed content-free wire validation, no recovery."""
    def __init__(self, header_raw, commands, *, program=None):
        self.program = PROGRAM if program is None else program
        self.header, self.commands, self.retained = v.digest(header_raw), commands, b""
        self.header_value = strict(header_raw)
        self.done_checks, self.done_observations, self.late_observations = {}, {}, []

    def inspect(self, raw, delivered):
        need(type(raw) is bytes and len(raw) <= CAP and raw.startswith(self.retained), "journal-prefix")
        complete = raw[:raw.rfind(b"\n") + 1]
        started, terminal, instance, done_checks = set(), {}, None, {}
        observations, done_observations, late_observations = {}, {}, []
        for seq, line in enumerate(complete.splitlines(), 1):
            r = strict(line.decode("utf-8"))
            required = {"header_sha256", "record_seq", "command_seq", "command_sha256", "agent_instance",
                        "observed_at", "kind", "facts", "checks"}
            need(isinstance(r, dict) and required <= set(r) <= required |
                 {"action", "account", "failure", "error_domain", "error_code"}, "journal-fields")
            n = r["command_seq"]
            need(type(n) is int and 1 <= n <= delivered and type(r["record_seq"]) is int and
                 r["record_seq"] == seq and r["header_sha256"] == self.header and
                 r["command_sha256"] == v.digest(self.commands[n-1][:-1]), "journal-binding")
            need(isinstance(r["agent_instance"], str) and LABEL.fullmatch(r["agent_instance"]), "journal-instance")
            if instance is None:
                instance = r["agent_instance"]
            need(r["agent_instance"] == instance, "smoke-no-relaunch")
            need(isinstance(r["observed_at"], str) and r["observed_at"].endswith("Z"), "journal-time")
            datetime.datetime.fromisoformat(r["observed_at"].replace("Z", "+00:00"))
            need(isinstance(r["facts"], list) and isinstance(r["checks"], list), "journal-shape")
            for f in r["facts"]:
                fact(f)
            kind = r["kind"]
            if kind == "started":
                need(set(r) == required | {"action"} and r["action"] == self.program[n-1][0] and
                     not r["facts"] and not r["checks"] and n not in started and
                     started == set(range(1, n)) and all(terminal.get(p) == "done" for p in range(1, n)),
                     "journal-start")
                started.add(n)
                observations[n] = []
            elif kind == "observation":
                need(set(r) == required and n in started and r["facts"] and not r["checks"], "journal-observation")
                if n in terminal:
                    late_observations.extend(r["facts"])
                observations[n].extend(r["facts"])
            elif kind in ("done", "failed"):
                need(n in started and n not in terminal and n == len(started), "journal-terminal")
                if kind == "failed":
                    need(set(r) == required | {"failure", "error_domain", "error_code"} and
                         r["failure"] in FAILURES and r["error_domain"] in
                         ("agent", "core", "cloudKit", "cancellation") and type(r["error_code"]) is int,
                         "journal-failed")
                else:
                    need(set(r) == required and not r["facts"], "journal-done")
                names = []
                for c in r["checks"]:
                    need(isinstance(c, dict) and set(c) == {"name", "outcome", "facts"} and
                         isinstance(c["name"], str) and
                         c["name"] in {"integrity", "localDurability", "coordinatorResult", "recovery"} and
                         c["outcome"] in ("pass", "fail", "open") and isinstance(c["facts"], list), "journal-check")
                    names.append(c["name"])
                    for f in c["facts"]:
                        fact(f)
                if kind == "done":
                    need(names and len(names) == len(set(names)) and
                         all(c["outcome"] == "pass" and c["facts"] for c in r["checks"]), "journal-required-checks")
                    done_checks[n] = r["checks"]
                    done_observations[n] = list(observations[n])
                terminal[n] = kind
            else:
                raise v.Refused("journal-kind")
        self.done_checks = done_checks
        self.done_observations = done_observations
        self.late_observations = late_observations
        return complete, terminal

    def retain(self, complete):
        need(complete.endswith(b"\n") and complete.startswith(self.retained), "journal-prefix")
        self.retained = complete


def frame_write(stream, metadata, raw=b""):
    need(len(raw) <= TREE_CAP + CAP, "transport-size")
    stream.write(i.raw_json({**metadata, "bytes": len(raw)}))
    stream.write(raw)
    stream.flush()


def frame_read(stream):
    line = stream.readline(CAP + 1)
    need(line.endswith(b"\n"), "transport-frame")
    value = strict(line)
    size = value.pop("bytes")
    need(type(size) is int and 0 <= size <= TREE_CAP + CAP, "transport-size")
    chunks, left = [], size
    while left:
        raw = stream.read(min(left, 1024 * 1024))
        need(raw, "transport-truncated")
        chunks.append(raw)
        left -= len(raw)
    return value, b"".join(chunks)


def append_exact(authority, name, before, raw):
    """Coordinator-owned append only, with exact prefix/readback and fsync."""
    authority.check()
    logs = getattr(authority, "_smoke_logs", None)
    if logs is None:
        logs = authority._smoke_logs = {}
    if name not in logs:
        need(not before, "append-prefix")
        fd = os.open(name, os.O_RDWR | os.O_APPEND | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_NONBLOCK,
                     0o600, dir_fd=authority.fd)
        logs[name] = (fd, h.directory_pin(os.fstat(fd)))
    fd, pin = logs[name]
    try:
        info = os.fstat(fd)
        need(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1 and
             stat.S_IMODE(info.st_mode) == 0o600 and h.directory_pin(info) == pin and
             h.directory_pin(os.stat(name, dir_fd=authority.fd, follow_symlinks=False)) == pin and
             os.pread(fd, len(before) + 1, 0) == before, "append-prefix")
        with os.fdopen(os.dup(fd), "ab") as out:
            out.write(raw)
            out.flush()
            os.fsync(out.fileno())
        need(os.pread(fd, len(before) + len(raw) + 1, 0) == before + raw and
             h.file_pin(os.stat(name, dir_fd=authority.fd, follow_symlinks=False)) == h.file_pin(os.fstat(fd)),
             "append-changed")
        os.fsync(authority.fd)
        authority.check()
    except BaseException:
        # The retained descriptor remains owned until the caller's finally;
        # failure never reopens a replacement or retries a partial append.
        raise


def close_appends(authority):
    for fd, _ in getattr(authority, "_smoke_logs", {}).values():
        os.close(fd)
    authority._smoke_logs = {}


def smoke_plan(manifest, run_path, *, selected=False):
    products = manifest["products"]["path"]
    xpath = products + "/" + manifest["xctestrun"]["path"]
    raw = h.checked_raw(xpath)
    need({"sha256": v.digest(raw), "bytes": len(raw)} ==
         {k: manifest["xctestrun"][k] for k in ("sha256", "bytes")}, "launch-template")
    plan = plistlib.loads(raw)
    targets = plan["TestConfigurations"][0]["TestTargets"]
    need(len(targets) == 1, "launch-selection")
    target = targets[0]
    selection = target["OnlyTestIdentifiers"]
    complete = (isinstance(selection, list) and len(selection) == 2 and
                all(isinstance(name, str) for name in selection) and
                set(selection) == {"CloudKitSoakTests", "CloudKitPathPreflightTests"})
    need(complete or (selected and selection == ["CloudKitSoakTests"]), "launch-selection")
    target["OnlyTestIdentifiers"] = ["CloudKitSoakTests"]
    for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
        need(not any(k.startswith(("MEGURI_CLOUD_AGENT_", "TEST_RUNNER_")) for k in target.get(key, {})),
             "launch-environment")
    environment = target.setdefault("EnvironmentVariables", {})
    environment["MEGURI_CLOUD_AGENT_RUN_DIRECTORY"] = run_path

    def expand(value):
        if isinstance(value, str):
            return value.replace("__TESTROOT__", os.path.dirname(xpath))
        if isinstance(value, list):
            return [expand(item) for item in value]
        if isinstance(value, dict):
            return {key: expand(item) for key, item in value.items()}
        return value
    return raw, plistlib.dumps(expand(plan))


def guest_smoke():
    """A single original supervisor holds all no-follow bindings until join."""
    request, payload = frame_read(sys.stdin.buffer)
    need(request.pop("action") == "prepare", "transport-order")
    support, copied, manifest = request["support"], request["copied"], request["manifest"]
    header = request["header"].encode("utf-8")
    expected = strict(header)
    selected = request.get("selected", False)
    need(type(selected) is bool, "launch-selection")
    if selected:
        destination = request.get("destination")
        need(expected["selection"] == "sync_now" and expected["device_kind"] in {"mac", "iphone"} and
             isinstance(destination, dict) and set(destination) == {"device_kind", "vm", "vm_instance", "simulator"} and
             destination["device_kind"] == expected["device_kind"], "selected-destination")
        simulator = destination["simulator"]
        if expected["device_kind"] == "mac":
            need(simulator is None and manifest["platform"] == "mac", "selected-destination")
            launch_destination = "platform=macOS"
        else:
            need(manifest["platform"] == "ios-sim" and isinstance(simulator, dict) and
                 set(simulator) == {"udid", "lease_receipt"} and isinstance(simulator["udid"], str) and
                 re.fullmatch(r"[0-9A-Fa-f-]{36}", simulator["udid"]), "selected-destination")
            launch_destination = "platform=iOS Simulator,id=" + simulator["udid"]
    else:
        need(expected["selection"] == "smoke" and expected["device_kind"] == "mac" and
             "destination" not in request, "smoke-selection")
        launch_destination = "platform=macOS"
    need(expected["account_labels"] == ["acct1"] and LABEL.fullmatch(expected["device"]) and
         v.MG_RUN_ID.fullmatch(expected["run_id"]), "smoke-selection")
    run_path = support["path"] + "/Meguri/CloudKitSigned/" + expected["run_id"] + "/" + expected["device"]
    child, launched, finished = None, False, False
    with contextlib.ExitStack() as held:
        base, run = h.Directory(support["path"]), h.Directory(run_path)
        held.callback(base.close)
        held.callback(run.close)
        need(h.root_state(base, support["path"]) == support and
             list(h.directory_pin(os.fstat(run.fd))) == copied["run"] and
             set(os.listdir(run.fd)) == {"run.json", "product-evidence"}, "run-not-prelaunch")
        originals = []
        for role in ("run.json", *h.ROLES):
            f = FrozenFile(run_path + "/" + (role if role == "run.json" else "product-evidence/" + role))
            held.callback(f.close)
            originals.append(f)
            need(f.reference == copied["files"][role] and list(f.pin) == copied["pins"][role], "evidence-changed")
        need(originals[0].raw == header, "header-changed")
        evidence = h.Directory(run_path + "/product-evidence")
        held.callback(evidence.close)
        need(list(h.directory_pin(os.fstat(evidence.fd))) == copied["evidence"] and
             set(os.listdir(evidence.fd)) == set(h.ROLES), "evidence-changed")
        # All complete input bytes arrive before any creation or launch.
        refs, blobs, offset = request["files"], {}, 0
        need(set(refs) >= {"fixture-input.json", "fixture-evidence/fixtures-result.json",
                           "fixture-evidence/generation.json", "fixture-evidence/parent/manifest.json",
                           "fixture-evidence/parent/collection.anki2"}, "fixture-roles")
        for name, ref in sorted(refs.items()):
            need(v.relative(name) == name and (name in {"fixture-input.json",
                 "fixture-evidence/fixtures-result.json", "fixture-evidence/generation.json"} or
                 name.startswith("fixture-evidence/parent/")), "fixture-role")
            need(set(ref) == {"sha256", "bytes"} and type(ref["bytes"]) is int and
                 0 <= ref["bytes"] <= 64 * 1024 * 1024 and isinstance(ref["sha256"], str) and
                 DIGEST.fullmatch(ref["sha256"]), "fixture-reference")
            raw = payload[offset:offset + ref["bytes"]]
            need(len(raw) == ref["bytes"] and v.digest(raw) == ref["sha256"], "fixture-bytes")
            blobs[name], offset = raw, offset + len(raw)
        private = payload[offset:]
        frozen = strict(blobs["fixture-input.json"])
        need(set(frozen) == {"protocol_version", "run_id", "device", "source_commit", "base", "parent_files",
                             "prepared_source", "files"} and frozen["protocol_version"] == 1 and
             all(frozen[k] == expected[k] for k in ("run_id", "device", "source_commit")) and
             frozen["base"] == "small" and frozen["prepared_source"] == expected["fixture_delivery"]["source"] and
             frozen["files"] == frozen["parent_files"] and
             {name.removeprefix("fixture-evidence/parent/"): ref for name, ref in refs.items()
              if name.startswith("fixture-evidence/parent/")} == frozen["files"] and
             refs["fixture-input.json"] == {k: expected["fixture_delivery"]["frozen_input"][k]
                                            for k in ("sha256", "bytes")}, "fixture-binding")
        parent_names = set(frozen["files"])
        media_names = {name for name in parent_names if name.startswith("collection.media/")}
        need(len(media_names) == 5 and all(len(name.split("/")) == 2 and
             frozen["files"][name]["bytes"] == 10240 for name in media_names) and
             parent_names in ({"collection.anki2", "manifest.json"} | media_names,
                              {"collection.anki2", "collection.anki2-wal", "manifest.json"} | media_names), "fixture-files")
        # Exclusive fixed roles: partial prior input refuses, never gets erased.
        os.mkdir("fixture-evidence", 0o700, dir_fd=run.fd)
        fdir = h.Directory(run_path + "/fixture-evidence")
        held.callback(fdir.close)
        os.mkdir("parent", 0o700, dir_fd=fdir.fd)
        parent = h.Directory(run_path + "/fixture-evidence/parent")
        held.callback(parent.close)
        os.mkdir("collection.media", 0o700, dir_fd=parent.fd)
        media = h.Directory(run_path + "/fixture-evidence/parent/collection.media")
        held.callback(media.close)
        for name, raw in blobs.items():
            directory = run if name == "fixture-input.json" else (
                media if name.startswith("fixture-evidence/parent/collection.media/") else
                parent if name.startswith("fixture-evidence/parent/") else fdir)
            h.write_new(directory.fd, name.rsplit("/", 1)[-1], raw)
            f = FrozenFile(run_path + "/" + name)
            held.callback(f.close)
            originals.append(f)
            need(f.raw == raw, "fixture-readback")
        private_file = materialize_private(support["path"], header, private)
        held.callback(private_file.close)
        for directory in (media, parent, fdir, run):
            os.fsync(directory.fd)
        tree = FixtureTree(run_path + "/fixture-evidence/parent")
        held.callback(tree.close)
        original_plan, plan = smoke_plan(manifest, run_path, selected=selected)
        # Derived bytes contain a private OS root: retain outside public output.
        stage = tempfile.mkdtemp(prefix="meguri-cloud-smoke-", dir=os.path.realpath(tempfile.gettempdir()))
        os.chmod(stage, 0o700)
        launch_dir = h.Directory(stage)
        held.callback(launch_dir.close)
        h.write_new(launch_dir.fd, "smoke.xctestrun", plan)
        os.fsync(launch_dir.fd)
        plan_file = FrozenFile(stage + "/smoke.xctestrun")
        held.callback(plan_file.close)

        def check():
            base.check()
            run.check()
            evidence.check()
            fdir.check()
            tree.check()
            private_file.check()
            plan_file.check()
            need(h.root_state(base, support["path"]) == support and set(os.listdir(evidence.fd)) == set(h.ROLES)
                 and set(os.listdir(fdir.fd)) == {"fixtures-result.json", "generation.json", "parent"}, "input-changed")
            for f in originals:
                f.check()
            need(h.checked_raw(manifest["products"]["path"] + "/" + manifest["xctestrun"]["path"]) ==
                 original_plan, "launch-template-changed")

        check()
        frame_write(sys.stdout.buffer, {"prepared": True}, plan)
        commands, retained, journal_fd, journal_pin = b"", b"", None, None
        out, err = held.enter_context(tempfile.TemporaryFile()), held.enter_context(tempfile.TemporaryFile())
        try:
            while True:
                request, raw = frame_read(sys.stdin.buffer)
                check()
                action = request["action"]
                if action == "launch":
                    need(not launched and not raw and not commands and not os.path.lexists(run_path + "/data"),
                         "transport-order")
                    environment = {k: val for k, val in os.environ.items()
                                   if not k.startswith(("TEST_RUNNER_", "MEGURI_CLOUD_AGENT_"))}
                    with h.setup_launch():
                        child = h.setup_process(["/usr/bin/xcodebuild", "test-without-building", "-xctestrun",
                            stage + "/smoke.xctestrun", "-destination", launch_destination,
                            "-only-testing:MeguriSnapshotTests/CloudKitSoakTests", "-parallel-testing-enabled", "NO",
                            "-collect-test-diagnostics", "never", "-resultBundlePath", stage + "/smoke.xcresult"],
                            env=environment, stdout=out, stderr=err, start_new_session=True)
                    launched = True
                    frame_write(sys.stdout.buffer, {"launched": True})
                elif action == "append":
                    need(launched and child.poll() is None and raw.endswith(b"\n") and raw.count(b"\n") == 1,
                         "transport-order")
                    append_exact(run, "commands.jsonl", commands, raw)
                    commands += raw
                    frame_write(sys.stdout.buffer, {"appended": True})
                elif action == "retain":
                    need(launched and raw.startswith(retained) and raw.endswith(b"\n"), "transport-order")
                    append_exact(run, "retained-steps.jsonl", retained, raw[len(retained):])
                    retained = raw
                    frame_write(sys.stdout.buffer, {"retained": True})
                elif action == "poll":
                    need(launched and not raw, "transport-order")
                    journal = b""
                    if "steps.jsonl" in os.listdir(run.fd):
                        # The agent may append concurrently. Read a bounded
                        # original prefix through one descriptor, never a mirror.
                        if journal_fd is None:
                            journal_fd = os.open("steps.jsonl", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                                                 dir_fd=run.fd)
                            journal_pin = h.directory_pin(os.fstat(journal_fd))
                        fd = journal_fd
                        try:
                            info = os.fstat(fd)
                            need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == os.getuid()
                                 and not info.st_mode & 0o022 and info.st_size <= CAP and
                                 h.directory_pin(info) == journal_pin, "journal-authority")
                            journal = os.pread(fd, CAP + 1, 0)
                            need(len(journal) <= CAP and h.directory_pin(os.stat("steps.jsonl", dir_fd=run.fd,
                                 follow_symlinks=False)) == journal_pin, "journal-changed")
                        finally:
                            run.check()
                    frame_write(sys.stdout.buffer, {"alive": child.poll() is None}, journal)
                elif action == "status":
                    need(launched and not raw, "transport-order")
                    status = b""
                    if "agent-status.json" in os.listdir(run.fd):
                        status = h.checked_raw(run_path + "/agent-status.json")
                    frame_write(sys.stdout.buffer, {}, status)
                elif action == "finish":
                    need(launched and not raw and retained, "transport-order")
                    child.wait(timeout=WAIT)
                    need(child.returncode == 0, "smoke-test-failed")
                    summary = strict(h.setup_command(["/usr/bin/xcrun", "xcresulttool", "get", "test-results",
                                                      "summary", "--path", stage + "/smoke.xcresult"]))
                    need(summary.get("passedTests") == 1 and summary.get("failedTests") == 0 and
                         summary.get("skippedTests") == 0, "smoke-test-result")
                    check()
                    final = FrozenFile(run_path + "/steps.jsonl")
                    held.callback(final.close)
                    need(final.raw.startswith(retained) and final.raw.endswith(b"\n"), "journal-final-prefix")
                    need(journal_pin is not None and h.directory_pin(os.fstat(final.fd)) == journal_pin,
                         "journal-changed")
                    finished = True
                    frame_write(sys.stdout.buffer, {"joined": True, "exit_code": child.returncode,
                                                    "passed": 1, "failed": 0, "skipped": 0}, final.raw)
                    return
                elif action == "cancel":
                    need(not raw, "transport-order")
                    if child is not None:
                        h.setup_join(child)
                    frame_write(sys.stdout.buffer, {"joined": True})
                    return
                else:
                    raise v.Refused("transport-action")
        finally:
            if child is not None and not finished:
                h.setup_join(child)
            if journal_fd is not None:
                os.close(journal_fd)
            close_appends(run)


def guest_source():
    definitions = (v.Refused, v.need, v.digest, v.identity, i.strict, i.raw_json,
        h.directory_pin, h.file_pin, h.Directory, h.Original, h.private_child, h.write_new,
        h.root_state, h.checked_raw, h.setup_cancel, h.setup_launch, h.SetupSigInfo, h.SetupBSDInfo, h.SetupKernel, h.SetupChild, h.setup_process,
        h.setup_join, h.setup_command,
        FrozenFile, FixtureTree, materialize_private, frame_write, frame_read, append_exact, close_appends,
        smoke_plan, guest_smoke)
    preamble = ("import os,sys,stat,json,re,hashlib,contextlib,signal,subprocess,tempfile,plistlib,ctypes,errno,threading,time\n"
                "from types import SimpleNamespace\nCAP=16*1024*1024\nTREE_CAP=128*1024*1024\nWAIT=600\n"
                "LABEL=re.compile(" + repr(LABEL.pattern) + ")\nDIGEST=re.compile(" + repr(DIGEST.pattern) + ")\n")
    names = ("directory_pin", "file_pin", "Directory", "Original", "private_child", "write_new", "root_state",
             "checked_raw", "setup_cancel", "setup_launch", "setup_join", "setup_command", "setup_process")
    return (preamble + "\n".join(inspect.getsource(f) for f in definitions) +
        "\nv=SimpleNamespace(need=need,digest=digest,identity=identity,Refused=Refused,relative=lambda p: "
        "p if isinstance(p,str) and p and not p.startswith('/') and all(x not in ('','.','..') for x in p.split('/')) "
        "and '\\x00' not in p else (_ for _ in ()).throw(Refused('path')),MG_RUN_ID=re.compile(" + repr(v.MG_RUN_ID.pattern) + "))\n" +
        "h=SimpleNamespace(**{k:globals()[k] for k in " + repr(names) + "},ROLES=" + repr(h.ROLES) + ")\n" +
        "i=SimpleNamespace(raw_json=raw_json)\n"
        "for n in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP): signal.signal(n,setup_cancel)\n"
        "try: guest_smoke()\nexcept Exception: sys.exit(1)\n").encode()


class Transport:
    """One owned original bridge/SSH process, never detached or rediscovered."""
    def source(self):
        return guest_source()

    def __init__(self, vm):
        self.vm, self.child, self.buffer = vm, None, bytearray()
        self.out = tempfile.TemporaryFile()
        self.err = tempfile.TemporaryFile()
        self.closed = False
        cmd = " ".join(shlex.quote(w) for w in ("/usr/bin/python3", "-B", "-I", "-c", self.source().decode(),
                                               "xcodebuild test-without-building"))
        environment = dict(os.environ, MG_ROOT=vm.root, CLOUD_AGENT_VM=json.dumps(vm.selection))
        try:
            vm.lease_check()
            with h.setup_launch():
                self.child = h.setup_process(["/bin/bash", "-c", i.BRIDGE, "cloud-agent-smoke",
                    os.path.join(vm.root, "tools"), "copy", cmd], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                    stderr=self.err, env=environment, start_new_session=True, bufsize=0)
            os.set_blocking(self.child.stdin.fileno(), False)
            os.set_blocking(self.child.stdout.fileno(), False)
        except BaseException:
            self.close()
            raise

    def request(self, action, raw=b"", **metadata):
        self.vm.lease_check()
        need(not self.closed and self.child.poll() is None, "transport-ended")
        # A bounded writer/reader handles a broken or non-reading guest without
        # leaving an owned SSH child blocked on a large fixture pipe.
        wire = i.raw_json({"action": action, **metadata, "bytes": len(raw)}) + raw
        offset, deadline = 0, time.monotonic() + WAIT
        while offset < len(wire):
            remaining = deadline - time.monotonic()
            need(remaining > 0 and select.select([], [self.child.stdin], [], remaining)[1], "transport-timeout")
            try:
                written = os.write(self.child.stdin.fileno(), wire[offset:offset + 65536])
            except BlockingIOError:
                continue
            need(written > 0, "transport-ended")
            offset += written
        def receive(size=None):
            while (size is None and b"\n" not in self.buffer) or (size is not None and len(self.buffer) < size):
                need(len(self.buffer) <= TREE_CAP + CAP, "transport-size")
                remaining = deadline - time.monotonic()
                need(remaining > 0 and select.select([self.child.stdout], [], [], remaining)[0], "transport-timeout")
                chunk = os.read(self.child.stdout.fileno(), 65536)
                need(chunk, "transport-ended")
                self.buffer.extend(chunk)
            length = self.buffer.index(10) + 1 if size is None else size
            data = bytes(self.buffer[:length])
            del self.buffer[:length]
            return data
        response = strict(receive())
        count = response.pop("bytes")
        need(type(count) is int and 0 <= count <= CAP, "transport-size")
        returned = receive(count)
        self.vm.lease_check()
        return response, returned

    def close(self):
        if self.closed:
            return
        self.closed = True
        try:
            if self.child is not None:
                if self.child.stdin is not None:
                    self.child.stdin.close()
                h.setup_join(self.child)
                self.child.stdout.close()
        finally:
            self.out.close()
            self.err.close()


def status_outcome(raw):
    """Only fixed app-reported setup/missing categories, never free diagnostics."""
    if not raw:
        return
    value = strict(raw)
    need(isinstance(value, dict) and set(value) ==
         {"scope", "signed_acceptance", "outcome", "failure", "user_steps", "dependencies"} and
         value["scope"] == "smoke" and value["signed_acceptance"] is False and
         value["outcome"] in {"failed", "awaiting_user", "smoke-completed"} and
         isinstance(value["user_steps"], list) and isinstance(value["dependencies"], list), "agent-status")
    if value["failure"] == "implementation_missing":
        raise Missing("client-capability")
    if value["outcome"] == "awaiting_user" and value["failure"] == "awaiting_user":
        steps = set(value["user_steps"])
        need(steps and steps <= {"cloudkit-dev-container", "cloudkit-test-accounts"}, "agent-user-step")
        raise UserStep(sorted(steps)[0])
    need(value["outcome"] == "smoke-completed" and value["failure"] is None and
         not value["user_steps"], "agent-failed")


def execute(root, parent, vm, expected, header, files, support, copied, recheck, tree, fixture_receipt,
            generation, frozen_raw, projection, local):
    need(expected["selection"] == "smoke" and expected["device_kind"] == "mac", "smoke-selection")
    roles = {"fixture-input.json": frozen_raw,
             "fixture-evidence/fixtures-result.json": fixture_receipt.raw,
             "fixture-evidence/generation.json": generation.raw,
             **{"fixture-evidence/parent/" + name: f.raw for name, f in tree.files.items()}}
    refs = {name: {"sha256": v.digest(raw), "bytes": len(raw)} for name, raw in roles.items()}
    commands = command_bytes(header.raw)
    journal, sent = Journal(header.raw, commands), b""
    host_private = materialize_private(projection.host_application_support(), header.raw, projection.private_bytes())
    try:
        def check():
            recheck()
            fixture_receipt.check()
            generation.check()
            tree.check()
            host_private.check()
            projection.revalidate()
            local.check()
        check()
        with contextlib.closing(Transport(vm)) as transport:
            metadata, derived = transport.request("prepare", b"".join(roles[n] for n in sorted(roles)) +
                projection.private_bytes(), support=support, copied=copied,
                manifest=files["agent-build.json"].value, header=header.raw.decode(), files=refs)
            need(metadata == {"prepared": True}, "fixture-delivery")
            run_path = support["path"] + "/Meguri/CloudKitSigned/" + parent + "/" + expected["device"]
            _, wanted_plan = smoke_plan(files["agent-build.json"].value, run_path)
            need(derived == wanted_plan, "launch-derived-bytes")
            # Private original derived bytes survive this run, without placing
            # the OS app root in MG artifacts. Public selection is independent.
            private_launch = tempfile.mkdtemp(prefix="meguri-cloud-launch-")
            os.chmod(private_launch, 0o700)
            with contextlib.closing(h.Directory(private_launch)) as launch_dir:
                h.write_new(launch_dir.fd, "smoke.xctestrun", derived)
                os.fsync(launch_dir.fd)
            h.write_new(local.fd, "launch-selection.json", i.raw_json({
                "scope": "SMOKE", "test": "MeguriSnapshotTests/CloudKitSoakTests",
                "environment_keys": ["MEGURI_CLOUD_AGENT_RUN_DIRECTORY"],
                "original_xctestrun": files["agent-build.json"].value["xctestrun"]}))
            check()
            reply, raw = transport.request("launch")
            need(reply == {"launched": True} and not raw, "launch-observation")
            for n, command in enumerate(commands, 1):
                check()
                # Keep exact originals before their delivery; never recreate a
                # command from a returned dictionary or alter an earlier line.
                append_exact(local, "commands.jsonl", sent, command)
                reply, raw = transport.request("append", command)
                need(reply == {"appended": True} and not raw, "command-readback")
                sent += command
                deadline = time.monotonic() + WAIT
                while True:
                    check()
                    metadata, returned = transport.request("poll")
                    need(set(metadata) == {"alive"} and type(metadata["alive"]) is bool, "poll-observation")
                    complete, terminal = journal.inspect(returned, n)
                    if complete != journal.retained:
                        append_exact(local, "steps.jsonl", journal.retained, complete[len(journal.retained):])
                        journal.retain(complete)
                        # Durable host retention precedes the agent prefix file,
                        # and both precede every successor command.
                        reply, raw = transport.request("retain", complete)
                        need(reply == {"retained": True} and not raw, "journal-retained-readback")
                    if terminal.get(n) == "failed":
                        # Preserve the failed journal and inspect only the
                        # original app's fixed status. No successor is sent.
                        _, status = transport.request("status")
                        if status:
                            status_outcome(status)
                        failure = next(strict(line).get("failure") for line in complete.splitlines()
                                       if strict(line)["kind"] == "failed")
                        if failure == "implementation_missing":
                            raise Missing("client-capability")
                        raise v.Refused("command-failed")
                    if terminal.get(n) == "done":
                        completion(n, journal.done_checks[n], journal)
                        break
                    if not metadata["alive"]:
                        _, status = transport.request("status")
                        status_outcome(status)
                        raise v.Refused("agent-ended-before-terminal")
                    need(time.monotonic() < deadline, "command-timeout")
                    # Host polling only; no device sync timer, repeated command
                    # or second execution. This wait is interruptible by signals.
                    select.select([], [], [], min(0.25, max(0, deadline - time.monotonic())))
            check()
            reply, raw = transport.request("finish")
            need(reply == {"joined": True, "exit_code": 0, "passed": 1, "failed": 0, "skipped": 0}
                 and raw, "original-process-result")
            # Read after original app-hosted termination as well. A late
            # callback may extend but cannot rewrite the retained prefix.
            # The guest finish joined first, so it cannot accept more commands.
            final, command_outcomes = journal.inspect(raw, 10)
            need(final == raw and command_outcomes == dict.fromkeys(range(1, 11), "done"), "closed-journal")
            append_exact(local, "steps.jsonl", journal.retained, final[len(journal.retained):])
            journal.retain(final)
            completion(10, journal.done_checks[10], journal)
            need(all(not TRACE.fullmatch(item.get("operation", "")) for item in journal.late_observations),
                 "smoke-work-after-terminal")
            check()
        return {"scope": "SMOKE", "outcome": "smoke_completed", "signed_execution": True,
                "commands": 10, "closed_journal": True, "original_app_hosted_joined": True}
    finally:
        host_private.close()
        close_appends(local)


def run_mac_smoke(root, parent_run, context):
    """Real parent composition. MG retains this one private VM context to drain."""
    need(v.MG_RUN_ID.fullmatch(parent_run), "parent-run")
    sha = v.source(root)
    directory = os.path.join(root, "build/mg/cloudkit-signed", parent_run)
    with owned_context(root, parent_run, context) as (vm, lease_check), contextlib.ExitStack() as held:
        local = h.Directory(directory)
        held.callback(local.close)
        observed = i.machine(vm)
        fixtures = child(root, "fixtures", ["cloud-small", "--seed", "1"], sha)
        receipt_path = os.path.join(root, "build/mg/fixtures", fixtures["run_id"], "result.json")
        receipt = h.Original(receipt_path)
        held.callback(receipt.close)
        need(receipt.value["placement"] == "docker" and receipt.value["service"] == "dev", "fixture-placement")
        generation_path = os.path.dirname(receipt_path) + "/cloud-small.json"
        generation = h.Original(generation_path)
        held.callback(generation.close)
        g = generation.value
        small(g, 1)
        source = g["dir"] if os.path.isabs(g["dir"]) else os.path.join(root, v.relative(g["dir"]))
        v.contained(root + "/build/fixtures", source)
        for path, kind in ((generation_path, "generation"), (source, "fixture"), (source + "/manifest.json", "manifest")):
            need(sum(x["path"] == path and x["kind"] == kind for x in receipt.value["artifacts"]) == 1,
                 "fixture-declaration")
        entries = [x for x in receipt.value["details"]["fixtures"]
                   if x.get("target") == "cloud-small" and x.get("kind") == "cloud-small"]
        need(len(entries) == 1 and all(entries[0][k] == g[k] for k in ("request", "manifest", "observed")),
             "generation-binding")
        tree = FixtureTree(source)
        held.callback(tree.close)
        need(strict(tree.files["manifest.json"].raw) == g["manifest"], "fixture-manifest")
        file_map = {name: f.reference for name, f in tree.files.items()}
        frozen_raw = i.raw_json({"protocol_version": 1, "run_id": parent_run, "device": "mac1",
            "source_commit": sha, "base": "small", "parent_files": file_map, "prepared_source": source, "files": file_map})
        h.write_new(local.fd, "fixture-input.json", frozen_raw)
        os.fsync(local.fd)
        build = child(root, "build-app", ["--platform", "mac", "--config", "Debug", "--test-plan", "CloudAgent"], sha)
        build_value, build_hash = v.receipt(root, root + "/build/mg", build, sha)
        manifest_path = v.declared_reference(root, build_value, build_value["details"]["agent_build"])
        manifest = h.Original(manifest_path)
        held.callback(manifest.close)
        m = manifest.value
        need(m["signing"] == "team" and m["platform"] == "mac" and m["test_plan"] == "CloudAgent", "build-signing")
        _, info = v.bundle(m["products"]["path"], m["app"]["path"], "mac")
        need(info.get("MeguriCloudEnvironment") == "development" and
             isinstance(info.get("MeguriCloudContainer"), str), "development-product")
        if info.get("MeguriCloudSchemaConfigured") is False:
            raise UserStep("cloudkit-dev-container")
        need(info.get("MeguriCloudSchemaConfigured") is True, "schema-configuration")
        template = {"protocol_version": 1, "job": "cloudkit-signed", "run_id": parent_run, "lane": "cloud-signed",
            "source_commit": sha, "tree_clean": True, "device": "mac1", "device_kind": "mac",
            "os_build": observed["os_build"], "app_build": info["CFBundleVersion"],
            "executable_sha256": m["app"]["executable_sha256"], "build_receipt": build,
            "build_receipt_sha256": build_hash, "container": info["MeguriCloudContainer"], "environment": "development",
            "account_scope": "scope-" + parent_run, "account_labels": ["acct1"], "selection": "smoke",
            "fixture_delivery": {"base": "small", "fixtures_receipt": fixtures, "source": source,
                "manifest": {"path": "manifest.json", **file_map["manifest.json"]},
                "frozen_input": {"path": "fixture-input.json", "sha256": v.digest(frozen_raw), "bytes": len(frozen_raw)}}}
        template_path = directory + "/header-template.json"
        h.write_new(local.fd, "header-template.json", i.raw_json(template))
        os.fsync(local.fd)
        need(i.machine(vm) == observed and v.source(root) == sha, "destination-changed")
        installed = child(root, "cloudkit-agent-install", ["--parent-run", parent_run, "--header-template", template_path],
                          sha, context)
        need(i.machine(vm) == observed, "destination-changed")

        def consume(expected, header, files, support, copied, recheck):
            need(expected == {**template, "deployment_receipt": installed}, "final-header-changed")
            with a.approved_account_input(header.raw, template["container"]) as projection:
                return execute(root, parent_run, vm, expected, header, files, support, copied, recheck,
                               tree, receipt, generation, frozen_raw, projection, local)
        report = h.handoff_product_evidence(root, parent_run, installed["run_id"], vm, lease_check, consume)
        if report.get("outcome") == "implementation_missing":
            raise Missing("client-path-preflight")
        return report


def main():
    parser = i.Parser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--parent-run", required=True)
    parser.add_argument("--install-child", action="store_true")
    parser.add_argument("--run-id")
    parser.add_argument("--run-directory")
    parser.add_argument("--source-commit")
    args = parser.parse_args()
    diagnostic = i.DeliveryDiagnostic() if args.install_child else None
    try:
        with os.fdopen(os.dup(3), "rb") as private:
            context = strict(private.read(CAP + 1))
        if args.install_child:
            need(args.run_id and args.run_directory and args.source_commit == v.source(args.root), "install-source")
            need(args.run_directory == os.path.join(args.root, "build/mg/cloudkit-agent-install", args.run_id), "install-run")
            diagnostic.enter("vm-context")
            with owned_context(args.root, args.parent_run, context) as (vm, lease_check):
                report = i.deliver(args.root, args.run_directory, args.parent_run, args.run_id, None, vm,
                                   checkpoint=diagnostic.enter)
                lease_check()
            print(json.dumps(report, sort_keys=True))
            return 0
        need(not any((args.run_id, args.run_directory, args.source_commit)), "parent-arguments")
        report = run_mac_smoke(args.root, args.parent_run, context)
        code = 0
    except a.InputFailure as error:
        report, code = {"scope": "SMOKE", "signed_execution": False, **error.public_outcome()}, 1
        if error.outcome == "awaiting_user":
            code = 76
        elif error.outcome == "implementation_missing":
            code = 78
    except Missing as error:
        category = "smoke-observation-wire" if error.args == ("smoke-observation-wire",) else "client-capability"
        report, code = {"scope": "SMOKE", "outcome": "implementation_missing", "category": category,
                        "signed_execution": False}, 78
    except UserStep as error:
        report, code = {"scope": "SMOKE", "outcome": "awaiting_user", "category": error.args[0],
                        "signed_execution": False}, 76
    except i.Queued:
        report, code = {"scope": "SMOKE", "outcome": "queued", "category": "vm-unavailable",
                        "signed_execution": False}, 75
    except (v.Refused, v.Invalid, OSError, ValueError, TypeError, KeyError, IndexError, AttributeError,
            UnicodeError, RecursionError, subprocess.SubprocessError) as error:
        report, code = {"scope": "SMOKE", "outcome": "failed_evidence", "category": "smoke-refused",
                        "signed_execution": False}, 1
        if diagnostic is not None:
            report.update(diagnostic.failure(error))
    print(json.dumps(report, sort_keys=True))
    return code


if __name__ == "__main__":
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(number, h.setup_cancel)
    sys.exit(main())
