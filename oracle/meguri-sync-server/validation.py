# SPDX-License-Identifier: AGPL-3.0-or-later
"""Validate the neutral boundary, including retained evidence, before credit."""
from __future__ import annotations

import hashlib
import json
import math
import pathlib
import re

from jsonschema import Draft202012Validator

DIGEST = re.compile(r"[0-9a-f]{64}\Z")
COMPLETED = {"matched", "different", "failed", "sealed"}
SUCCESS = {"matched", "sealed"}
SOURCE_FIELDS = ("source_commit", "batch_base", "source_sha256", "lock_sha256", "mg_job", "arguments")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def same_value(left, right):
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(same_value(left[key], right[key]) for key in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(same_value(a, b) for a, b in zip(left, right))
    return left == right


def unique_named(values):
    result = {}
    for entry in values:
        require(entry["name"] not in result, "duplicate observation name")
        result[entry["name"]] = entry["value"]
    return result


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON member")
        result[key] = value
    return result


def _numbers(value):
    if type(value) is int:
        require(abs(value) <= 9007199254740991, "wide integer requires an exact decimal string")
    elif type(value) is float:
        require(math.isfinite(value), 'nonfinite number')
    elif isinstance(value, dict):
        for child in value.values():
            _numbers(child)
    elif isinstance(value, list):
        for child in value:
            _numbers(child)


def decode(data):
    value = json.loads(data, object_pairs_hook=_pairs, parse_constant=lambda _: require(False, "nonfinite number"))
    _numbers(value)
    return value


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True, allow_nan=False).encode()


def sha(data):
    return hashlib.sha256(data).hexdigest()


class Artifacts:
    def __init__(self, root):
        self.root = pathlib.Path(root)

    def put(self, data):
        digest = sha(data)
        self.root.mkdir(parents=True, exist_ok=True)
        path = self.root / digest
        if path.exists():
            require(self.read(digest) == data, "artifact collision")
        else:
            with path.open("xb") as stream:
                stream.write(data)
        return digest

    def put_json(self, value):
        return self.put(encoded(value))

    def read(self, digest, length=None):
        require(isinstance(digest, str) and DIGEST.fullmatch(digest), "invalid artifact digest")
        require(not self.root.is_symlink(), 'linked artifact directory')
        path = self.root / digest
        require(not path.is_symlink() and path.is_file(), "missing or linked artifact")
        require(path.stat().st_nlink == 1, "multiply linked artifact")
        data = path.read_bytes()
        require(sha(data) == digest, "artifact hash mismatch")
        require(length is None or len(data) == length, "artifact length mismatch")
        return data

    def json(self, digest):
        return decode(self.read(digest))


def binding(manifest, case):
    return {**{key: manifest[key] for key in SOURCE_FIELDS},
            "case_id": case["id"], "lane": case["lane"],
            "fixture_sha256": case["fixture_sha256"], "variants": case["variants"],
            "expected": case["expected"], "observed": case.get("observed"),
            "status": "matched" if case["status"] == "sealed" else case["status"]}


def _schema(manifest, schema):
    errors = list(Draft202012Validator(schema).iter_errors(manifest))
    require(not errors, "JSON schema violation")


def _identity(case):
    lane = case["lane"]
    client, server = lane["client"], lane["server"]
    family = case["family"]
    require(case["id"].split(":", 1)[0] == family, "case ID/family mismatch")
    if family.startswith("P-"):
        require(case["classification"] in {"baseline", "current_probe"}, "probe classification mismatch")
        require(lane["kind"] in {"reference", "candidate"}, "probe lane mismatch")
        require(client["kind"] in {"desktop", "mobile"}, "probe client mismatch")
    else:
        require(case["classification"] == "meguri_policy", "policy classification mismatch")
    wanted = {"reference": (None, "reference_runtime"), "candidate": (None, "meguri_sync"),
              "retained_peer": ("retained_meguri", "meguri_sync"), "policy": ("service", "meguri_sync")}[lane["kind"]]
    require(wanted[0] is None or client["kind"] == wanted[0], "client/lane mismatch")
    require(server["kind"] == wanted[1], "peer/lane mismatch")
    if family == "M-I01":
        require(lane["kind"] == "retained_peer", "retained family mismatch")
    if family in {"P-R01", "P-R02", "P-R03"}:
        require(lane["kind"] == "candidate", "recovery requires candidate")
        if family != "P-R03":
            require(client["kind"] == {"P-R01": "desktop", "P-R02": "mobile"}[family], "recovery target mismatch")


def _artifacts(manifest, case, store, invocation, trusted_runtime):
    roles = {}
    for artifact in case["artifacts"]:
        store.read(artifact["sha256"], artifact.get("bytes"))
        require(artifact["role"] not in roles, "duplicate evidence role")
        roles[artifact["role"]] = artifact["sha256"]
    fixture = store.json(case["fixture_sha256"])
    require(roles.get("synthetic_input") == case["fixture_sha256"], "fixture manifest identity mismatch")
    require(fixture.get("format") == "meguri-sync-fixtures-v1", "invalid fixture manifest")
    entries = fixture.get("entries", [])
    digests = set()
    for entry in entries:
        require(entry["sha256"] not in digests, "duplicate fixture input")
        digests.add(entry["sha256"])
        store.read(entry["sha256"], entry["bytes"])
    require(bool(digests), "empty fixture manifest")
    for action in case["actions"]:
        require("fixture_sha256" in action and action["fixture_sha256"] in digests, "unbound action fixture")
    wanted = binding(manifest, case)
    for role in ("neutral_observation", "mg_receipt"):
        receipt = store.json(roles[role])
        require(same_value(receipt.get("binding"), wanted), "case receipt binding mismatch")
        require(receipt.get("format") == "meguri-sync-" + role + "-v1", "evidence format mismatch")
    receipt = store.json(roles["mg_receipt"])
    require(receipt.get("run_id") == invocation["run_id"], "MG run mismatch")
    require(receipt.get("selection") == invocation["selection"], "MG selection mismatch")
    require(same_value(receipt.get('completed_ids'), [row['id'] for row in manifest['cases'] if row['status'] in COMPLETED]), 'MG completed selection mismatch')
    runtime = store.json(receipt["runtime_receipt_sha256"])
    require(runtime.get("format") == "meguri-sync-runtime-v1", "invalid runtime receipt")
    require(same_value(runtime, trusted_runtime), 'retained runtime receipt differs from the pinned runtime')
    require(runtime.get('lock_sha256') == manifest['lock_sha256'], 'pinned runtime lock mismatch')
    lane = case["lane"]
    if lane["client"]["kind"] == "desktop":
        require(same_value(lane["client"], runtime["client"]), "Desktop runtime identity mismatch")
    if lane["server"]["kind"] == "reference_runtime":
        require(same_value(lane["server"], runtime["server"]), "reference runtime identity mismatch")
    else:
        peer = lane["server"]
        require(all(peer[key] == manifest[key] for key in ("source_commit", "source_sha256", "lock_sha256")), "candidate source mismatch")
        require(peer["executable_sha256"] == manifest.get("executable_sha256"), "candidate executable mismatch")
        package = store.json(roles["package"])
        require(same_value(package.get("server"), peer), "candidate package receipt mismatch")
        require(package.get("format") == "meguri-sync-package-v1", "invalid package receipt")
        store.read(package["executable_artifact_sha256"])
        require(package["executable_artifact_sha256"] == peer["executable_sha256"], "candidate executable artifact mismatch")
    if lane["client"]["kind"] == "mobile":
        device = store.json(roles["device_evidence"])
        require(device.get("format") == "meguri-sync-device-v1", "invalid device receipt")
        require(same_value(device.get("binding"), wanted), "device execution/build binding mismatch")
        require(device.get("actual_device") is True, "actual device receipt required")
    if case["status"] == "sealed":
        result = store.json(receipt['final_result_sha256'])
        require(result.get('job') == manifest['mg_job'] and result.get('run_id') == invocation['run_id'], 'final MG job/run mismatch')
        require(result.get('status') == 'pass' and result.get('exit_code') == 0 and result.get('dry_run') is False, 'final MG result did not pass')
        require(same_value(result.get('args'), manifest['arguments']), 'final MG arguments mismatch')
        require(result.get('commit', {}).get('sha') == manifest['source_commit'] and result['commit'].get('dirty') is False, 'final MG source is stale or dirty')
        require(same_value(result.get('details', {}).get('reference_invocation'), invocation), 'final MG invocation binding mismatch')
        review = store.json(roles["independent_review"])
        require(review.get("format") == "meguri-sync-review-v1", "invalid review receipt")
        require(same_value(review.get("binding"), wanted), "review source/case binding mismatch")
        require(review.get("outcome") == "accepted", "review did not accept evidence")
        require(review.get("reviewer_session") and review["reviewer_session"] != invocation["author_session"], "review is not independent")
        batch = store.json(review['review_batch_sha256'])
        require(batch.get('format') == 'meguri-sync-review-batch-v1' and batch.get('outcome') == 'accepted' and batch.get('role') == 'oracle-maintainer', 'invalid review batch')
        require(batch.get('reviewer_session') == review['reviewer_session'], 'reviewer identity mismatch')
        require(sum(same_value(entry, wanted) for entry in batch.get('bindings', [])) == 1, 'review batch case/source mismatch')


def validate(manifest, contract, schema, store, invocation, trusted_runtime, require_credit=True):
    _schema(manifest, schema)
    require(manifest["document_kind"] == "evidence", "execution evidence required")
    require(contract.get("format") == "meguri-sync-selection-v1", "invalid selection contract")
    expected = contract["cases"]
    require(expected and len({case["id"] for case in expected}) == len(expected), "empty or duplicate expected selection")
    cases = manifest["cases"]
    require(len({case["id"] for case in cases}) == len(cases), "duplicate case IDs")
    require({case["id"] for case in cases} == {case["id"] for case in expected}, "incomplete or unexpected selection")
    expected_by_id = {case["id"]: case for case in expected}
    require(manifest["expected_case_count"] == len(expected), "expected count mismatch")
    require(manifest["selected_case_count"] == len(cases), "selected count mismatch")
    require(manifest["completed_case_count"] == sum(case["status"] in COMPLETED for case in cases), "completed count mismatch")
    require(invocation["selection"] == [case["id"] for case in expected], "invocation selection differs from contract")
    require(all(same_value(manifest[key], invocation[key]) for key in SOURCE_FIELDS), "MG source/arguments mismatch")
    for case in cases:
        _identity(case)
        selected = expected_by_id[case["id"]]
        for key in ("family", "classification", "variants", "spec_ids", "expected"):
            require(same_value(case[key], selected[key]), "case differs from reviewed contract")
        require(case["lane"]["kind"] == contract["lane_kind"], "selection lane mismatch")
        require(case["lane"]["client"]["kind"] == selected["client_kind"], "selection target mismatch")
        expected_values = unique_named(case["expected"])
        if case["status"] in {"matched", "different", "sealed"}:
            observed = unique_named(case["observed"])
            matches = same_value(expected_values, observed)
            require(matches == (case["status"] != "different"), "claimed outcome differs from observations")
            _artifacts(manifest, case, store, invocation, trusted_runtime)
    credit = all(case["status"] in SUCCESS for case in cases)
    require(not require_credit or credit, "selection remains incomplete or failed")
    return {"expected": len(expected), "selected": len(cases), "completed": manifest["completed_case_count"], "credit": credit}
