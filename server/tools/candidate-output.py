# SPDX-License-Identifier: MIT
"""Finite output binding for the owned Desktop candidate invocation.

This checks finite counts, identities and retained bytes. The pinned test tool's
separate validate invocation owns artifact JSON semantics. This helper never
executes a fixture, chooses a target, or grants release/broad-catalog acceptance.
"""
import hashlib
import json
import pathlib
import re
import stat
import sys


SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COUNT_KEYS = ("total", "passed", "failed", "expected_case_count",
              "selected_case_count", "completed_case_count")
INVOCATION_KEYS = {"source_commit", "batch_base", "source_sha256", "lock_sha256",
                   "executable_sha256", "mg_job", "arguments", "run_id", "author_session"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def regular(path):
    require(stat.S_ISREG(path.lstat().st_mode), "missing regular retained file")
    return path.read_bytes()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def load(path):
    return json.loads(regular(path), object_pairs_hook=unique_object,
                      parse_constant=lambda _: require(False, "nonfinite JSON value"))


def count(value):
    return type(value) is int and 0 <= value <= 2147483647


def same(left, right):
    # Preserve JSON boolean/numeric distinctions and array order.
    return json.dumps(left, sort_keys=True) == json.dumps(right, sort_keys=True)


def observations(items):
    require(isinstance(items, list) and items, "missing observations")
    result = {}
    for item in items:
        require(set(item) == {"name", "value"} and isinstance(item["name"], str)
                and item["name"] and item["name"] not in result,
                "invalid or duplicate observation name")
        result[item["name"]] = item["value"]
    return result


def validate(output, expected, peer, selection, summary, invocation):
    require(set(expected) == INVOCATION_KEYS, "invalid owned invocation inputs")
    require(set(invocation) == INVOCATION_KEYS | {"selection"}, "invalid final invocation fields")
    require(all(same(invocation[key], expected[key]) for key in INVOCATION_KEYS),
            "stale or mismatched final invocation identity")
    require(all(same(item["executable_sha256"], peer["server"]["executable_sha256"])
                for item in (expected, invocation)), "invocation executable differs from owned candidate")
    require(all(same(item[key], peer["server"][key])
                for item in (expected, invocation)
                for key in ("source_commit", "source_sha256", "lock_sha256")),
            "invocation source or lock differs from owned candidate")
    phase = expected['arguments'][3]
    require(phase in {'full', 'normal', 'media'}, 'unsupported owned candidate phase')
    required = {'full':11, 'normal':10, 'media':4}[phase]
    require(selection["format"] == "meguri-sync-selection-v1"
            and selection["lane_kind"] == "candidate" and selection["phase"] == phase,
            "invalid fixed selection")
    contracts = selection["cases"]
    ids = [case["id"] for case in contracts]
    require(len(ids) == required and len(set(ids)) == required, "invalid selected phase expansion")
    selected = invocation["selection"]
    require(isinstance(selected, list) and len(selected) == required
            and len(set(selected)) == required and set(selected) == set(ids),
            "final invocation selection differs from selected phase contract")
    require(all(count(summary[key]) for key in COUNT_KEYS), "invalid actual summary counts")
    require(type(summary["selection_credit"]) is bool
            and type(summary["broad_catalog_credit"]) is bool, "invalid actual credit flags")
    require(summary.get("skipped", 0) == 0 and type(summary.get("skipped", 0)) is int,
            "hidden skipped output")
    require(summary["expected_case_count"] == summary["selected_case_count"] == required,
            "missing or zero phase selection")
    evidence = load(output / "evidence.json")
    evidence_keys = {"format", "document_kind", "baseline_release", "source_commit",
                     "source_sha256", "lock_sha256", "batch_base", "mg_job", "arguments",
                     "expected_case_count", "selected_case_count", "completed_case_count", "cases"}
    require(evidence_keys <= set(evidence) <= evidence_keys | {"executable_sha256"},
            "invalid evidence fields")
    if "executable_sha256" in evidence:
        require(same(evidence["executable_sha256"], peer["server"]["executable_sha256"]),
                "evidence executable differs from owned candidate")
    require(evidence["format"] == "meguri-sync-server-v1"
            and evidence["document_kind"] == "evidence" and evidence["baseline_release"] == "26.09.3",
            "invalid evidence format")
    for key in ("source_commit", "source_sha256", "lock_sha256", "batch_base", "mg_job", "arguments"):
        require(same(evidence[key], expected[key]), "stale or mismatched evidence identity")
    cases = evidence["cases"]
    require(isinstance(cases, list) and len(cases) == required
            and len({case["id"] for case in cases}) == required
            and {case["id"] for case in cases} == set(ids), "missing or duplicate output cases")
    completed = sum(case["status"] in ("matched", "different", "failed", "sealed") for case in cases)
    passed = sum(case["status"] in ("matched", "sealed") for case in cases)
    for key, actual in (("expected_case_count", required), ("selected_case_count", len(cases)),
                        ("completed_case_count", completed)):
        require(count(evidence[key]) and evidence[key] == actual == summary[key],
                "mismatched actual execution counts")
    require(summary["total"] == len(cases) and summary["passed"] == passed
            and summary["failed"] == completed - passed, "mismatched actual outcome totals")
    require(completed == passed == required and summary["failed"] == 0, "incomplete or failed actual execution")
    require(summary["selection_credit"] and not summary["broad_catalog_credit"],
            "invalid finite coverage claim")
    require(not (output / "artifacts").is_symlink()
            and (output / "artifacts").is_dir(), "missing retained artifacts directory")
    contracts = {case["id"]: case for case in contracts}
    client_identity = None
    for case in cases:
        contract = contracts[case["id"]]
        require(case["id"].split(":")[0] == case["family"], "case family differs from ID")
        for key in ("family", "classification", "spec_ids", "variants", "expected"):
            require(same(case[key], contract[key]), "case differs from fixed selected contract")
        # Only the recorder's optional fixture binding is dynamic.
        actions = case["actions"]
        require(isinstance(actions, list) and all(isinstance(action, dict) for action in actions)
                and same([{key: value for key, value in action.items() if key != "fixture_sha256"}
                          for action in actions], contract["actions"]),
                "case differs from fixed selected contract")
        # Intermediate output cannot assert an independently reviewed seal.
        require(case["status"] == "matched", "candidate output has no independent seal")
        lane = case["lane"]
        require(lane["kind"] == "candidate" and same(lane["server"], peer["server"]),
                "case is not bound to this owned candidate")
        client = lane["client"]
        require(client["kind"] == "desktop" and client["version"] == "26.09.4"
                and client["build"] == "c06370ff" and SHA256.fullmatch(client["runtime_sha256"]),
                "missing actual Desktop identity")
        require(client_identity is None or same(client_identity, client), "inconsistent Desktop identity")
        client_identity = client
        expected_values = observations(case["expected"])
        observed = observations(case["observed"])
        require(all(key in observed and same(value, observed[key])
                    for key, value in expected_values.items()), "matched output has mismatched observations")
        roles = set()
        fixtures = set()
        for artifact in case["artifacts"]:
            digest = artifact["sha256"]
            require(isinstance(digest, str) and SHA256.fullmatch(digest), "invalid artifact digest")
            require(type(artifact["bytes"]) is int and artifact["bytes"] >= 0, "invalid artifact byte count")
            data = regular(output / "artifacts" / digest)
            require(len(data) == artifact["bytes"] and hashlib.sha256(data).hexdigest() == digest,
                    "missing or mismatched retained artifact bytes")
            roles.add(artifact["role"])
            if artifact["role"] == "synthetic_input":
                fixtures.add(digest)
        require({"synthetic_input", "neutral_observation", "mg_receipt"} <= roles,
                "missing required case artifact roles")
        require(case["fixture_sha256"] in fixtures, "fixture is not a retained synthetic input")
        require(all(action.get("fixture_sha256", case["fixture_sha256"]) in fixtures
                    for action in case["actions"]), "action fixture is not retained")


def main():
    output, expected_path, peer_path, selection_path = map(pathlib.Path, sys.argv[1:])
    report = {"summary": None, "invocation": None, "errors": []}
    # Keep independently parsed actual outputs available even on validation failure.
    for field, filename in (("summary", "summary.json"), ("invocation", "invocation.json")):
        try:
            value = load(output / filename)
            require(isinstance(value, dict), "runtime output must be a JSON object")
            report[field] = value
        except (OSError, ValueError, TypeError):
            report["errors"].append("missing or invalid " + filename)
    if not report["errors"]:
        try:
            validate(output, load(expected_path), load(peer_path), load(selection_path),
                     report["summary"], report["invocation"])
        except ValueError as error:
            report["errors"].append(str(error))
        except (OSError, KeyError, TypeError, AttributeError):
            report["errors"].append("missing or malformed required runtime evidence")
    print(json.dumps(report, sort_keys=True))
    return bool(report["errors"])


if __name__ == "__main__":
    sys.exit(main())
