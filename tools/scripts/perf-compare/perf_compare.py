#!/usr/bin/env python3
"""Perf results (U12, R1863, R1864): turns what the performance suites
produce into tools/perf/perf-result.schema.json, which U241's perf-gate
reads, and validates perf results.

  perf_compare.py xcresult <metrics.json> --suite <class> --configuration <config>
                  --runner <runner.json> --out <result.json>
      `xcrun xcresulttool get test-results metrics` output of a UI
      performance run: the suite's tests' XCTest metrics, one row per
      percentile (p50, p95, p99, max) of each metric's per-iteration
      measurements. Signpost metrics are named after their interval
      (IntervalNames); XCTest's own metrics are xctest.<metric>.
  perf_compare.py gungraun <summaries.jsonl> --suite <bench target> --out <result.json>
      gungraun's JSON lines (--output-format=json, summary schema v7): one
      `instructions` row (callgrind Ir) per benchmark, named
      `<group>.<function>`; a bench id `cards_<size>` names the fixture.
  perf_compare.py signposts <os-signpost.xml> --out <summary.json>
      `xcrun xctrace export` of a trace's os-signpost table (tools/mg
      metrics): the intervals in Meguri's subsystem with how often each
      began, and those whose name is not in the registry
      (app/Packages/MeguriCore/Sources/Instrumentation/IntervalNames.swift).
  perf_compare.py validate <result.json>...
      each file against the schema (needs python3-jsonschema: the dev and
      image-tools containers have it).

Percentiles use the rank rule of meguri-testkit's bench::timing: pN is the
smallest measurement with fewer than (100 - N)% of the measurements above
it, so a percentile never hides the slow tail.

Exit 0 on success, 1 on invalid input or results, 2 on usage errors.
"""

import argparse
import json
import os
import platform
import re
import sys
import xml.etree.ElementTree as ElementTree

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..")
SCHEMA = os.path.join(ROOT, "tools", "perf", "perf-result.schema.json")
REGISTRY = os.path.join(ROOT, "app", "Packages", "MeguriCore", "Sources", "Instrumentation", "IntervalNames.swift")
SUBSYSTEM = "com.antoinefink.meguri"

PERCENTILES = (("p50", 50), ("p95", 95), ("p99", 99), ("max", 100))


class InputError(Exception):
    pass


def percentile(values, per_hundred):
    """The rank rule: the smallest value with fewer than (100 - N)% above."""
    ordered = sorted(values)
    rank = min(len(ordered) * per_hundred // 100 + 1, len(ordered))
    return ordered[rank - 1]


# ---------------------------------------------------------------------------
# XCTest metrics

SIGNPOST = re.compile(r"XCTMetric_OSSignpost-(?P<name>.+)\.(?P<metric>[A-Za-z_]+)$")

# Units XCTest reports, and what a perf result uses: (unit, factor).
UNITS = {
    "s": ("ms", 1000.0),
    "ms": ("ms", 1.0),
    "kB": ("MB", 1.0 / 1024.0),
    "KB": ("MB", 1.0 / 1024.0),
    "MB": ("MB", 1.0),
    "ms per s": ("ms/s", 1.0),
    "ms/s": ("ms/s", 1.0),
    "%": ("%", 1.0),
    "frames": ("frames", 1.0),
    "count": ("count", 1.0),
    "hitches": ("count", 1.0),
}


def kebab(text):
    text = re.sub(r"([a-z0-9])([A-Z])", r"\1-\2", text)
    text = re.sub(r"[^A-Za-z0-9]+", "-", text)
    return text.strip("-").lower()


def xctest_metric(metric):
    """(name, metric) of an XCTest metric. A signpost metric is named after
    its interval (com.apple.dt.XCTMetric_OSSignpost-<interval>.duration).
    XCTest's own metrics are xctest.<kind> (the identifier's
    XCTMetric_<Kind>, e.g. Memory, Hitch, ApplicationLaunch), and their
    metric is the display name without its target in parentheses: the
    identifier's target may hold dots (a bundle id), so its measure cannot
    be split off reliably."""
    identifier = metric.get("identifier", "")
    found = SIGNPOST.search(identifier)
    if found:
        return found.group("name"), kebab(found.group("metric"))
    kind = re.split(r"[-.]", identifier.rsplit("XCTMetric_", 1)[-1], maxsplit=1)[0] if "XCTMetric_" in identifier else ""
    display = re.sub(r"\s*\([^)]*\)\s*$", "", metric.get("displayName", ""))
    return "xctest." + (kebab(kind) or "metric"), kebab(display) or "value"


def from_xcresult(metrics_doc, suite, configuration, runner):
    if not isinstance(metrics_doc, list):
        raise InputError("xcresulttool metrics output is a list of tests")
    rows = []
    for test in metrics_doc:
        identifier = test.get("testIdentifier", "")
        if identifier.split("/", 1)[0] != suite:
            continue
        for run in test.get("testRuns", []):
            for metric in run.get("metrics", []):
                values = metric.get("measurements") or []
                if not values:
                    continue
                unit = metric.get("unitOfMeasurement", "")
                if unit not in UNITS:
                    raise InputError(f"{identifier}: unit {unit!r} of {metric.get('identifier')} is not one perf results use")
                result_unit, factor = UNITS[unit]
                measured = [v * factor for v in values]
                name, kind = xctest_metric(metric)
                for label, per_hundred in PERCENTILES:
                    rows.append({
                        "name": name,
                        "test": identifier,
                        "metric": kind,
                        "unit": result_unit,
                        "percentile": label,
                        "value": percentile(measured, per_hundred),
                        "samples": len(measured),
                        "measurements": measured,
                    })
    if not rows:
        raise InputError(f"no metrics for suite {suite}")
    return {
        "schema_version": 1,
        "suite": suite,
        "kind": "ui",
        "configuration": configuration,
        "runner": runner,
        "results": rows,
    }


# ---------------------------------------------------------------------------
# gungraun

FIXTURE_ID = re.compile(r"^cards_(?P<size>\d+[km]?)$")


def from_gungraun(lines, suite, runner):
    rows = []
    for number, line in enumerate(lines, 1):
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            summary = json.loads(line)
        except json.JSONDecodeError as e:
            raise InputError(f"line {number}: {e}")
        if str(summary.get("version")) != "7":
            raise InputError(f"line {number}: gungraun summary version {summary.get('version')!r}, this reads 7")
        callgrind = [p for p in summary.get("profiles", []) if p.get("tool") == "Callgrind"]
        if len(callgrind) != 1:
            raise InputError(f"line {number}: {len(callgrind)} Callgrind profiles")
        try:
            instructions = callgrind[0]["data"]["total"]["metrics"]["Ir"]["values"]["new"]
        except (KeyError, TypeError):
            raise InputError(f"line {number}: no total Ir in the Callgrind profile")
        row = {
            "name": f"{summary['group']}.{summary['function_name']}",
            "test": summary.get("module_path", "") + (f" {summary['id']}" if summary.get("id") else ""),
            "metric": "instructions",
            "unit": "count",
            "value": instructions,
            "samples": 1,
        }
        found = FIXTURE_ID.match(summary.get("id") or "")
        if found:
            row["fixture"] = found.group("size")
        rows.append(row)
    if not rows:
        raise InputError("no gungraun summary")
    return {
        "schema_version": 1,
        "suite": suite,
        "kind": "instruction-count",
        "configuration": "bench",
        "runner": runner,
        "results": rows,
    }


def docker_runner():
    machine = platform.machine()
    return {
        "kind": "docker",
        "id": f"docker/{platform.system().lower()}-{machine}",
        "os": platform.system().lower(),
        "arch": machine,
    }


# ---------------------------------------------------------------------------
# Instruments signposts

INTERVAL_DECL = re.compile(r'\b(?:IntervalName|BridgeCall)\(\s*"(?P<name>[^"]+)"')


def registry_names(path=REGISTRY):
    """The names IntervalNames.swift registers (intervals and bridge calls)."""
    with open(path) as f:
        return sorted({m.group("name") for m in INTERVAL_DECL.finditer(f.read())})


def signpost_rows(xml_text):
    """The os-signpost table's rows as {mnemonic: text}. xctrace writes the
    schema and the rows in separate <node>s, and each value once: an
    element with an id (anywhere, nested ones too) holds it, later cells
    point at it with ref."""
    root = ElementTree.fromstring(xml_text)
    values = {}
    for element in root.iter():
        ident = element.get("id")
        if ident is not None:
            values[ident] = element.get("fmt") or (element.text or "")
    columns = None
    rows = []
    for node in root.iter("node"):
        schema = node.find("schema")
        if schema is not None:
            columns = [c.findtext("mnemonic") for c in schema.findall("col")]
        for row in node.findall("row"):
            if columns is None:
                raise InputError("a row before the table's schema")
            record = {}
            for column, cell in zip(columns, list(row)):
                ref = cell.get("ref")
                record[column] = values.get(ref, "") if ref is not None else (cell.get("fmt") or (cell.text or ""))
            rows.append(record)
    return rows


def summarize_signposts(xml_text, registered):
    counts = {}
    for row in signpost_rows(xml_text):
        if row.get("subsystem") != SUBSYSTEM:
            continue
        key = (row.get("category", ""), row.get("name", ""))
        entry = counts.setdefault(key, {"begins": 0, "events": 0})
        kind = row.get("event-type", "")
        if kind == "Begin":
            entry["begins"] += 1
        entry["events"] += 1
    intervals = [{"name": name, "category": category, "count": c["begins"] or c["events"]}
                 for (category, name), c in sorted(counts.items(), key=lambda kv: (kv[0][1], kv[0][0]))]
    return {
        "subsystem": SUBSYSTEM,
        "intervals": intervals,
        "unregistered": sorted({i["name"] for i in intervals if i["name"] not in registered}),
    }


# ---------------------------------------------------------------------------
# Validation

def validate(paths):
    from jsonschema import Draft202012Validator

    with open(SCHEMA) as f:
        schema = json.load(f)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    problems = []
    for path in paths:
        with open(path) as f:
            doc = json.load(f)
        for error in sorted(validator.iter_errors(doc), key=lambda e: list(e.path)):
            where = "/".join(str(p) for p in error.path) or "(root)"
            problems.append(f"{path}: {where}: {error.message}")
    return problems


def write(doc, path):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w") as f:
        json.dump(doc, f, indent=2, sort_keys=True)
        f.write("\n")


def main(argv):
    parser = argparse.ArgumentParser(prog="perf_compare.py")
    commands = parser.add_subparsers(dest="command", required=True)
    xc = commands.add_parser("xcresult")
    xc.add_argument("metrics")
    xc.add_argument("--suite", required=True)
    xc.add_argument("--configuration", required=True)
    xc.add_argument("--runner", required=True)
    xc.add_argument("--out", required=True)
    gg = commands.add_parser("gungraun")
    gg.add_argument("summaries")
    gg.add_argument("--suite", required=True)
    gg.add_argument("--out", required=True)
    sp = commands.add_parser("signposts")
    sp.add_argument("table")
    sp.add_argument("--out", required=True)
    sp.add_argument("--registry", default=REGISTRY)
    va = commands.add_parser("validate")
    va.add_argument("results", nargs="+")
    args = parser.parse_args(argv)

    try:
        if args.command == "xcresult":
            with open(args.metrics) as f:
                metrics = json.load(f)
            with open(args.runner) as f:
                runner = json.load(f)
            write(from_xcresult(metrics, args.suite, args.configuration, runner), args.out)
        elif args.command == "signposts":
            with open(args.table) as f:
                write(summarize_signposts(f.read(), set(registry_names(args.registry))), args.out)
        elif args.command == "gungraun":
            with open(args.summaries) as f:
                write(from_gungraun(f.readlines(), args.suite, docker_runner()), args.out)
        else:
            problems = validate(args.results)
            for problem in problems:
                print(problem)
            return 1 if problems else 0
    except (InputError, OSError, json.JSONDecodeError, ElementTree.ParseError) as e:
        print(f"perf_compare.py {args.command}: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
