#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Bind only the sync-web Node TAP and Playwright JSON runner reports."""
import json
import re
import sys
from pathlib import Path


def node_tap(text):
    lines = text.splitlines()
    if lines.count("TAP version 13") != 1:
        raise ValueError("Missing or repeated Node TAP header")
    summaries = {}
    for key in ("tests", "suites", "pass", "fail", "cancelled", "skipped", "todo"):
        values = re.findall(rf"^# {key} (\d+)$", text, re.M)
        if len(values) != 1:
            raise ValueError(f"Missing or repeated Node TAP {key} summary")
        summaries[key] = int(values[0])
    tests = re.findall(r"^(not ok|ok) (\d+) - (.*)$", text, re.M)
    plans = re.findall(r"^1\.\.(\d+)$", text, re.M)
    if (len(plans) != 1 or int(plans[0]) != summaries["tests"] or
            len(tests) != summaries["tests"] or
            [int(test[1]) for test in tests] != list(range(1, len(tests) + 1)) or
            summaries["suites"] != 0 or
            re.search(r"^\s+(?:not )?ok \d+ -", text, re.M)):
        raise ValueError("Inconsistent flat Node TAP tests/plan/summary")
    skipped = sum(bool(re.search(r" # SKIP\b", title, re.I)) for _, _, title in tests)
    todo = sum(bool(re.search(r" # TODO\b", title, re.I)) for _, _, title in tests)
    plain = [status for status, _, title in tests if not re.search(r" # (?:SKIP|TODO)\b", title, re.I)]
    if (skipped != summaries["skipped"] or todo != summaries["todo"] or
            plain.count("ok") != summaries["pass"] or
            plain.count("not ok") != summaries["fail"] + summaries["cancelled"] or
            sum(summaries[key] for key in ("pass", "fail", "cancelled", "skipped", "todo")) != len(tests)):
        raise ValueError("Node TAP outcomes differ from summary")
    counts = dict(total=len(tests), selected=len(tests), completed=summaries["pass"] + summaries["fail"],
                  passed=summaries["pass"], failed=summaries["fail"], skipped=skipped,
                  cancelled=summaries["cancelled"], todo=todo, flaky=0)
    errors = []
    if not tests or counts["passed"] != len(tests):
        errors.append("Node unit report is empty or contains nonpassing/cancelled/skipped/todo tests")
    return counts, errors


def playwright(report):
    tests = []

    def visit(suite):
        for spec in suite.get("specs", []):
            tests.extend(spec["tests"])
        for child in suite.get("suites", []):
            visit(child)

    for suite in report["suites"]:
        visit(suite)
    counts = dict(total=len(tests), selected=len(tests), completed=0, passed=0, failed=0, skipped=0, flaky=0)
    errors = []
    statuses = {key: 0 for key in ("expected", "unexpected", "flaky", "skipped")}
    for test in tests:
        status = test["status"]
        statuses[status] += 1
        results = test["results"]
        if status == "flaky":
            counts["flaky"] += 1
        last = results[-1]["status"] if results else None
        if status == "skipped" or last == "skipped":
            counts["skipped"] += 1
        elif last in ("passed", "failed", "timedOut", "interrupted"):
            counts["completed"] += 1
            counts["passed" if last == "passed" else "failed"] += 1
        if (test["expectedStatus"] != "passed" or status != "expected" or len(results) != 1 or
                any(result.get("retry") != 0 or result.get("status") != "passed" or result.get("errors") for result in results)):
            errors.append("Playwright requires one passed attempt, retry zero and expected passed status per selected test")
    stats = report["stats"]
    if any(type(stats.get(key)) is not int or stats[key] != value for key, value in statuses.items()):
        errors.append("Playwright nested test outcomes differ from retained stats")
    if report.get("errors"):
        errors.append("Playwright report contains runner errors")
    if not tests or counts["completed"] != len(tests) or counts["passed"] != len(tests) or counts["skipped"] or counts["flaky"]:
        errors.append("Playwright report is empty, incomplete or nonpassing")
    return counts, errors


mode, source, output = sys.argv[1:]
try:
    text = Path(source).read_text()
    if mode == "unit":
        counts, errors = node_tap(text)
    elif mode == "browser":
        counts, errors = playwright(json.loads(text))
    else:
        raise ValueError("Unknown sync-web report format")
except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
    counts, errors = None, [f"Invalid or missing {mode} report: {error}"]
Path(output).write_text(json.dumps(dict(counts=counts, errors=errors), indent=2) + "\n")
if errors:
    print("; ".join(errors), file=sys.stderr)
    sys.exit(1)
