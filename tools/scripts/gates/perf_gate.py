#!/usr/bin/env python3
"""Performance budget gate (U241; tools/perf/budgets.yaml, R1802, R1803,
R1861-R1864, R1866).

tools/mg perf-gate calls this. The catalog maps every performance budget
(R1807-R1855 and R1583) to the intervals and benchmarks that measure it; the
gate judges perf results (tools/perf/perf-result.schema.json, written by the
U12 jobs) against it and against the stored baselines. Commands:

  load        budgets.yaml (and the budget groups of milestones.yaml) ->
              JSON. The only command that reads YAML (PyYAML: the dev
              container, or a python3 that has it).
  check       the catalog against the plan's requirement list, the interval
              registry (IntervalNames.swift), the UI performance tests and
              milestones.yaml.
  evaluate    perf results (given, or discovered among the perf jobs' results
              on one commit) -> an evaluation: one judgement per measured
              case, instruction-count regressions, the refused inputs, per
              group and per budget statuses.
  rebaseline  baselines from an evaluation's inputs, with a reason.
  device-run  a device run's report (U256's evidence, which the milestone
              gate checks with verify_device_report) from an evaluation.

Rules (budgets.yaml `rules`):
  - Budgets are absolute on devices (iPhone, iPad, the Mac host) and in the
    Mac core benchmarks. On simulators, and for the Mac in the test VM, only
    frame, hitch-ratio, memory and count budgets are absolute; ms budgets
    there get the regression rule alone (simulator timing is not device
    timing), as do cases marked `simulator: regression` (KTD19's R1807,
    R1808, R1815 exception).
  - UI metrics fail above +10% against their baseline (R1863); instruction
    counts above +5% (R1864). Wall-clock benches are tracked (p50, p95, p99)
    and fail only on an absolute breach.
  - R1803: a budget stated at 150k (or less) holds at every size up to
    150k, time budgets may be 2x at up to 500k, and above 500k a result only
    has to exist (the feature completed). Every size is judged, but only a
    measurement at a case's own fixture proves it: a 1M completion or a 500k
    pass leaves a case stated at 150k missing (budget and group statuses,
    device-run reports). A row without a fixture fits only a case stated
    without one: it is never taken at a case's fixture.
  - R1861: every result of a suite must measure each case the catalog gates
    on that suite (pending cases aside, at any fixture the case takes), or
    the evaluation fails; `check` requires the suite to declare the
    interval and to call measure(app:), which measures the declared ones.
  - R1866: a UI result not built in Profile, or whose tests kept attachments
    or screenshots, is refused before anything is judged.

Everything but `load` is stdlib only and runs under macOS's /usr/bin/python3
(3.9): milestone_gate.py imports verify_device_report.

Exit status: load and check: 0 valid, 1 invalid (errors as JSON on stdout),
2 usage. evaluate: 0 pass or warn, 1 fail, 2 error. rebaseline and
device-run: 0 written, 1 refused, 2 error.
"""

import argparse
import fnmatch
import json
import math
import os
import re
import sys

SCHEMA_VERSION = 1
PLACEMENTS = ("core-docker", "core-mac", "simulator", "device")
UI_PLACEMENTS = ("simulator", "device")
DEVICES = ("iphone", "ipad", "mac")
UNITS = ("ms", "s", "count", "MB", "kB", "ms/s", "%", "frames")
PERCENTILES = ("p50", "p95", "p99", "max")
KINDS = ("ui", "wall-clock", "instruction-count")
# The configuration each kind of perf result must come from: UI tests only
# from Profile (Release optimization plus the test hooks, R1866), core
# benches from cargo's bench profile.
CONFIGURATIONS = {"ui": "Profile", "wall-clock": "bench", "instruction-count": "bench"}
RID = re.compile(r"^R[0-9]{3,4}$")
UNIT_ID = re.compile(r"^U[0-9]+[a-z]?$")
SLUG = re.compile(r"^[a-z0-9][a-z0-9-]*$")
METRIC = re.compile(r"^[a-z][a-z0-9-]*$")
SIZE = re.compile(r"^([0-9]+(?:\.[0-9]+)?)([km])$")
SHA = re.compile(r"^[0-9a-f]{40}$")
MILESTONE = re.compile(r"^M[0-9]$")
# The 500k and 1M thresholds of R1803.
LOOSE_SIZE = 500000
BUDGET_SIZE = 150000

CATALOG_KEYS = {"version", "plan", "budget_range", "extra_budgets", "registry", "perf_tests", "rules", "defaults",
                "groups", "budgets", "probes"}
RULE_KEYS = {"ui_regression_percent", "icount_regression_percent", "simulator_absolute_units", "time_units"}
DEFAULT_KEYS = {"fixture", "percentile", "multiplier_500k", "rule_1m"}
ENTRY_KEYS = {"id", "title", "group", "devices", "cases", "note"}
CASE_KEYS = {"id", "suite", "test", "name", "relative_to", "metric", "unit", "percentile", "limit", "strict",
             "fixture", "multiplier_500k", "rule_1m", "where", "simulator", "pending", "note"}


class Invalid(Exception):
    pass


def read_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def write_json(path, value):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(value, f, indent=2, sort_keys=True, ensure_ascii=False)
        f.write("\n")
    os.replace(tmp, path)


def is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def is_str_list(value):
    return isinstance(value, list) and all(isinstance(v, str) and v for v in value)


def fmt(value):
    """A number as the messages show it: at most three decimals, no trailing zeros."""
    if not is_number(value):
        return str(value)
    text = "%.3f" % value
    return text.rstrip("0").rstrip(".") if "." in text else text


def fmt_percent(pct):
    if math.isinf(pct):
        return "+inf%"
    text = "%+.1f" % round(pct, 1)
    return (text[:-2] if text.endswith(".0") else text) + "%"


def size_of(fixture):
    """The card count a size fixture names (10k, 150k, 1m), or None."""
    m = SIZE.match(fixture or "")
    if not m:
        return None
    return int(round(float(m.group(1)) * (1000 if m.group(2) == "k" else 1000000)))


def yaml_available():
    try:
        import yaml  # noqa: F401
    except ImportError:
        return False
    return True


# ---------------------------------------------------------------------------
# load


def read_yaml(path):
    import yaml  # the dev container has PyYAML; macOS's python3 does not

    class UniqueKeyLoader(yaml.SafeLoader):
        pass

    def construct_mapping(loader, node, deep=False):
        seen = set()
        for key_node, _ in node.value:
            key = loader.construct_object(key_node, deep=deep)
            if key in seen:
                raise Invalid("%s: duplicate key %r (line %d)" % (path, key, key_node.start_mark.line + 1))
            seen.add(key)
        return yaml.SafeLoader.construct_mapping(loader, node, deep)

    UniqueKeyLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, construct_mapping)
    with open(path, encoding="utf-8") as f:
        return yaml.load(f, Loader=UniqueKeyLoader)


def milestone_budgets(manifest):
    """What the catalog must agree with in milestones.yaml: the budgets that
    apply to some devices only, the device-run budget groups and the groups
    the perf-gate suites scope."""
    out = {"budget_devices": {}, "budget_groups": {}, "perf_gate_groups": {}}
    if not isinstance(manifest, dict):
        raise Invalid("milestones.yaml is not a mapping")
    out["budget_devices"] = manifest.get("budget_devices") or {}
    for m in manifest.get("milestones") or []:
        if not isinstance(m, dict):
            continue
        out["budget_groups"][m.get("id")] = m.get("budgets") or {}
        items = []
        for s in m.get("suites") or []:
            if isinstance(s, dict) and s.get("id") == "perf-gate" and isinstance(s.get("scope"), dict):
                items += [i for i in s["scope"].get("items") or [] if i not in items]
        if items:
            out["perf_gate_groups"][m.get("id")] = items
    return out


def load(catalog_path, milestones_path=None):
    raw = read_yaml(catalog_path)
    return {"catalog": raw, "milestones": milestone_budgets(read_yaml(milestones_path)) if milestones_path else None}


# ---------------------------------------------------------------------------
# normalize: the catalog with every case's defaults filled in


def normalize(raw):
    """(catalog, errors). The catalog has `cases`: every case of every budget
    and probe with its defaults filled in, keyed `<entry>/<case>`."""
    errors = []
    if not isinstance(raw, dict):
        return None, ["the catalog is not a mapping"]
    extra = set(raw) - CATALOG_KEYS
    if extra:
        errors.append("unknown keys %s" % ", ".join(sorted(extra)))
    if raw.get("version") != 1:
        errors.append("version must be 1")

    rules = raw.get("rules") or {}
    if not isinstance(rules, dict) or set(rules) != RULE_KEYS:
        errors.append("rules has exactly %s" % ", ".join(sorted(RULE_KEYS)))
        rules = {"ui_regression_percent": 10, "icount_regression_percent": 5,
                 "simulator_absolute_units": [], "time_units": ["ms", "s"]}
    for key in ("ui_regression_percent", "icount_regression_percent"):
        if not is_number(rules.get(key)) or rules[key] <= 0:
            errors.append("rules.%s is a positive number" % key)
    for key in ("simulator_absolute_units", "time_units"):
        if not is_str_list(rules.get(key)) or not set(rules[key]) <= set(UNITS):
            errors.append("rules.%s lists units of %s" % (key, ", ".join(UNITS)))
            rules[key] = []

    defaults = raw.get("defaults") or {}
    if not isinstance(defaults, dict) or set(defaults) != DEFAULT_KEYS:
        errors.append("defaults has exactly %s" % ", ".join(sorted(DEFAULT_KEYS)))
        defaults = {"fixture": None, "percentile": "p95", "multiplier_500k": 1, "rule_1m": "completes"}
    if defaults.get("percentile") not in PERCENTILES:
        errors.append("defaults.percentile is one of %s" % ", ".join(PERCENTILES))
    if not is_number(defaults.get("multiplier_500k")) or defaults["multiplier_500k"] < 1:
        errors.append("defaults.multiplier_500k is a number of at least 1")
    if defaults.get("rule_1m") != "completes":
        errors.append("defaults.rule_1m is completes (R1803)")

    groups = raw.get("groups") or {}
    if not isinstance(groups, dict) or not groups or not all(
            isinstance(k, str) and SLUG.match(k) and isinstance(v, str) and v for k, v in groups.items()):
        errors.append("groups maps group names to what they hold")
        groups = {}

    for key in ("plan", "registry", "perf_tests"):
        if key in raw and not (isinstance(raw[key], str) and raw[key]):
            errors.append("%s is a path in the repository" % key)
    rng = raw.get("budget_range")
    if rng is not None and not (is_str_list(rng) and len(rng) == 2 and all(RID.match(r) for r in rng)):
        errors.append("budget_range is [first R-ID, last R-ID]")
    if "extra_budgets" in raw and not (is_str_list(raw["extra_budgets"]) and all(RID.match(r) for r in raw["extra_budgets"])):
        errors.append("extra_budgets lists R-IDs")

    catalog = {"rules": rules, "defaults": defaults, "groups": groups, "budgets": [], "probes": [], "cases": [],
               "budget_range": rng, "extra_budgets": raw.get("extra_budgets") or [],
               "paths": {k: raw.get(k) for k in ("plan", "registry", "perf_tests")}}
    seen = set()
    for kind in ("budgets", "probes"):
        entries = raw.get(kind) or []
        if not isinstance(entries, list) or (kind == "budgets" and not entries):
            errors.append("%s is a list of entries" % kind)
            continue
        for i, entry in enumerate(entries):
            out = normalize_entry(entry, kind, i, groups, defaults, errors)
            if out is None:
                continue
            if out["id"] in seen:
                errors.append("%s is listed twice: one catalog entry per budget" % out["id"])
                continue
            seen.add(out["id"])
            catalog[kind].append(out)
            catalog["cases"] += out["cases"]
    return catalog, errors


def normalize_entry(entry, kind, index, groups, defaults, errors):
    where = "%s[%d]" % (kind, index)
    if not isinstance(entry, dict):
        errors.append("%s is not a mapping" % where)
        return None
    eid = entry.get("id")
    pattern = RID if kind == "budgets" else SLUG
    if not isinstance(eid, str) or not pattern.match(eid):
        errors.append("%s: id must be %s" % (where, "an R-ID" if kind == "budgets" else "a slug"))
        return None
    where = eid
    extra = set(entry) - ENTRY_KEYS
    if extra:
        errors.append("%s: unknown keys %s" % (where, ", ".join(sorted(extra))))
    if not isinstance(entry.get("title"), str) or not entry["title"]:
        errors.append("%s: title says what the budget is" % where)
    group = entry.get("group")
    if group not in groups:
        errors.append("%s: group %r is not one of groups" % (where, group))
    devices = entry.get("devices", list(DEVICES))
    if not is_str_list(devices) or not devices or not set(devices) <= set(DEVICES):
        errors.append("%s: devices lists %s" % (where, ", ".join(DEVICES)))
        devices = list(DEVICES)
    cases = entry.get("cases")
    if not isinstance(cases, list) or not cases:
        errors.append("%s: cases is a non-empty list" % where)
        return None
    out = {"id": eid, "kind": "budget" if kind == "budgets" else "probe", "title": entry.get("title"),
           "group": group, "devices": [d for d in DEVICES if d in devices],
           "restricted": set(devices) != set(DEVICES), "cases": []}
    ids = set()
    for j, c in enumerate(cases):
        nc = normalize_case(out, c, j, defaults, errors)
        if nc is None:
            continue
        if nc["id"] in ids:
            errors.append("%s: case %s is listed twice" % (where, nc["id"]))
            continue
        ids.add(nc["id"])
        out["cases"].append(nc)
    if out["kind"] == "budget" and not any("device" in c["where"] for c in out["cases"]):
        errors.append("%s: no case is measured on a device: U256's device runs cover every budget" % where)
    return out


def normalize_case(entry, c, index, defaults, errors):
    where = "%s/cases[%d]" % (entry["id"], index)
    if not isinstance(c, dict):
        errors.append("%s is not a mapping" % where)
        return None
    cid = c.get("id")
    if not isinstance(cid, str) or not SLUG.match(cid):
        errors.append("%s: id is a slug" % where)
        return None
    where = "%s/%s" % (entry["id"], cid)
    extra = set(c) - CASE_KEYS
    if extra:
        errors.append("%s: unknown keys %s" % (where, ", ".join(sorted(extra))))
    out = {
        "key": where, "id": cid, "budget": entry["id"], "kind": entry["kind"], "group": entry["group"],
        "devices": entry["devices"], "suite": c.get("suite"), "test": c.get("test"), "name": c.get("name"),
        "relative_to": c.get("relative_to"), "metric": c.get("metric"), "unit": c.get("unit"),
        "percentile": c["percentile"] if "percentile" in c else defaults.get("percentile"),
        "limit": c.get("limit"), "strict": c.get("strict", False),
        "fixture": c["fixture"] if "fixture" in c else defaults.get("fixture"),
        "multiplier_500k": c.get("multiplier_500k", defaults.get("multiplier_500k")),
        "rule_1m": c.get("rule_1m", defaults.get("rule_1m")),
        "where": c.get("where"), "simulator": c.get("simulator"), "pending": c.get("pending"), "note": c.get("note"),
    }
    for key in ("suite", "name"):
        if not isinstance(out[key], str) or not out[key]:
            errors.append("%s: %s is required" % (where, key))
    for key in ("test", "relative_to", "fixture", "note"):
        if out[key] is not None and not (isinstance(out[key], str) and out[key]):
            errors.append("%s: %s is a non-empty string" % (where, key))
    if not isinstance(out["metric"], str) or not METRIC.match(out["metric"]):
        errors.append("%s: metric is kebab case (duration, frames, memory-peak-physical, ...)" % where)
    if out["unit"] not in UNITS:
        errors.append("%s: unit is one of %s" % (where, ", ".join(UNITS)))
    if out["percentile"] is not None and out["percentile"] not in PERCENTILES:
        errors.append("%s: percentile is one of %s" % (where, ", ".join(PERCENTILES)))
    if not isinstance(out["strict"], bool):
        errors.append("%s: strict is true or false" % where)
    if not is_number(out["multiplier_500k"]) or out["multiplier_500k"] < 1:
        errors.append("%s: multiplier_500k is at least 1" % where)
    if out["rule_1m"] != "completes":
        errors.append("%s: rule_1m is completes (R1803)" % where)
    if not is_str_list(out["where"]) or not out["where"] or not set(out["where"]) <= set(PLACEMENTS):
        errors.append("%s: where lists %s" % (where, ", ".join(PLACEMENTS)))
        out["where"] = []
    if out["simulator"] is not None:
        if out["simulator"] not in ("absolute", "regression"):
            errors.append("%s: simulator is absolute or regression" % where)
        elif "simulator" not in out["where"]:
            errors.append("%s: simulator applies only to a case measured on simulators" % where)
    if out["pending"] is not None and not (isinstance(out["pending"], str) and UNIT_ID.match(out["pending"])):
        errors.append("%s: pending names the unit that lands the measurement" % where)
    limit = out["limit"]
    if limit is None:
        if entry["kind"] == "budget" and out["pending"] is None:
            errors.append("%s: limit is required (only a pending case may wait for its number)" % where)
    elif not is_number(limit) or limit < 0:
        errors.append("%s: limit is a non-negative number" % where)
    return out


# ---------------------------------------------------------------------------
# check: the catalog against the repository


INTERVAL_DECL = re.compile(
    r"static\s+let\s+(\w+)\s*=\s*IntervalName\(\s*\"([^\"]+)\"\s*,\s*\.(\w+)\s*"
    r"(?:,\s*fieldReported:\s*(true|false)\s*)?\)")
BRIDGE_DECL = re.compile(r"static\s+let\s+(\w+)\s*=\s*BridgeCall\(\s*\"([^\"]+)\"\s*\)")
CLASS_DECL = re.compile(r"\bclass\s+(\w+)\s*:\s*(\w+)[^{]*\{")
EXTENSION_DECL = re.compile(r"\bextension\s+(\w+)\b[^{]*\{")
INTERVALS_VAR = re.compile(r"\bclass\s+var\s+intervalNames\s*:\s*\[String\]\s*\{")
# PerfTestCase.measure(app:...): the only call that measures the declared
# intervals (measureLaunch measures the launch alone).
MEASURE_CALL = re.compile(r"\bmeasure\s*\(\s*app\s*:")


def read_registry(path):
    """The interval registry: {name: {ident, category, field_reported}} and
    {ident: name}."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    names, idents = {}, {}
    for m in INTERVAL_DECL.finditer(text):
        names[m.group(2)] = {"ident": m.group(1), "category": m.group(3), "field_reported": m.group(4) == "true"}
        idents[m.group(1)] = m.group(2)
    for m in BRIDGE_DECL.finditer(text):
        names[m.group(2)] = {"ident": m.group(1), "category": "bridge", "field_reported": False}
    return {"names": names, "idents": idents}


def braced(text, start):
    """The text between the brace at `start` and its match (string literals
    skipped), and the index after the match."""
    depth, i, n = 0, start, len(text)
    while i < n:
        ch = text[i]
        if ch == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return text[start + 1:i], i + 1
        i += 1
    return text[start + 1:], n


def calls_measure(text):
    """Whether Swift `text` calls measure(app:...): not its definition, not a
    line comment."""
    for m in MEASURE_CALL.finditer(text):
        line = text[text.rfind("\n", 0, m.start()) + 1:m.start()]
        if "//" not in line and not text[:m.start()].rstrip().endswith("func"):
            return True
    return False


def read_perf_tests(directory, registry):
    """{class: {declared, measures}} of the PerfTestCase subclasses under
    `directory`: the interval names each declares in `intervalNames`
    (IntervalNames.<x>.name or a literal), inherited when a subclass does
    not override it, and whether it or a base class (their bodies or
    extensions) calls measure(app:), which measures them."""
    classes, extended = {}, {}
    for dirpath, _, files in os.walk(directory):
        for name in sorted(files):
            if not name.endswith(".swift"):
                continue
            with open(os.path.join(dirpath, name), encoding="utf-8") as f:
                text = f.read()
            for m in CLASS_DECL.finditer(text):
                body, _ = braced(text, m.end() - 1)
                declared = None
                v = INTERVALS_VAR.search(body)
                if v:
                    inner, _ = braced(body, v.end() - 1)
                    declared = [registry["idents"].get(i, "IntervalNames." + i)
                                for i in re.findall(r"IntervalNames\.(\w+)\.name", inner)]
                    declared += re.findall(r"\"([^\"\\]+)\"", inner)
                classes[m.group(1)] = {"base": m.group(2), "declared": declared, "measures": calls_measure(body)}
            for m in EXTENSION_DECL.finditer(text):
                body, _ = braced(text, m.end() - 1)
                extended[m.group(1)] = extended.get(m.group(1), False) or calls_measure(body)

    def resolve(cls, seen=()):
        """(declared, measures) of `cls`, or None when it is no PerfTestCase."""
        info = classes.get(cls)
        if info is None or cls in seen:
            return None
        base = None
        if info["base"] != "PerfTestCase":
            base = resolve(info["base"], seen + (cls,))
            if base is None:
                return None
        declared = info["declared"] if info["declared"] is not None else (base[0] if base else [])
        return declared, info["measures"] or extended.get(cls, False) or bool(base and base[1])

    out = {}
    for cls in classes:
        found = resolve(cls)
        if found is not None:
            out[cls] = {"declared": found[0], "measures": found[1]}
    return out


def plan_requirements(path):
    with open(path, encoding="utf-8") as f:
        return set(re.findall(r"^- (R[0-9]+)\. ", f.read(), re.M))


def check(catalog, root, milestones=None):
    """Errors of the catalog against the repository: one entry per budget of
    the plan's range (and the extra budgets), registered intervals measured
    by the suites the cases name, every R1861 interval catalogued, and the
    devices and groups milestones.yaml uses."""
    errors = []
    paths = catalog["paths"]
    for key in ("plan", "registry", "perf_tests"):
        if not paths.get(key):
            errors.append("the catalog names no %s" % key)
    if errors:
        return errors

    # One entry per budget R-ID of the range, plus the extra budgets.
    try:
        defined = plan_requirements(os.path.join(root, paths["plan"]))
    except OSError as e:
        return ["cannot read the plan: %s" % e]
    rng = catalog["budget_range"]
    if not rng:
        return ["the catalog names no budget_range"]
    lo, hi = int(rng[0][1:]), int(rng[1][1:])
    expected = {r for r in defined if lo <= int(r[1:]) <= hi}
    for r in catalog["extra_budgets"]:
        if r not in defined:
            errors.append("extra budget %s is not a requirement of the plan" % r)
        expected.add(r)
    ids = {b["id"] for b in catalog["budgets"]}
    span = "%s-%s%s" % (rng[0], rng[1], "".join(", " + r for r in catalog["extra_budgets"]))
    for r in sorted(expected - ids, key=lambda x: int(x[1:])):
        errors.append("%s has no catalog entry: every budget of %s has exactly one" % (r, span))
    for r in sorted(ids - expected, key=lambda x: int(x[1:])):
        errors.append("%s is not a budget of %s in the plan" % (r, span))

    # Registered intervals, measured by the suites the cases name.
    try:
        registry = read_registry(os.path.join(root, paths["registry"]))
        tests = read_perf_tests(os.path.join(root, paths["perf_tests"]), registry)
    except OSError as e:
        return errors + ["cannot read the interval registry or the UI performance tests: %s" % e]
    for c in catalog["cases"]:
        if c["pending"] or not set(c["where"]) & set(UI_PLACEMENTS):
            continue
        names = [c["name"]] + ([c["relative_to"]] if c["relative_to"] else [])
        registered = []
        for n in names:
            if n.startswith("xctest."):
                continue
            if n not in registry["names"]:
                errors.append("%s: %s is not in IntervalNames (%s): UI performance tests measure only registered "
                              "intervals (R1861)" % (c["key"], n, paths["registry"]))
            else:
                registered.append(n)
        if c["suite"] not in tests:
            errors.append("%s: no UI performance test emits %s: %s is not a PerfTestCase subclass under %s"
                          % (c["key"], c["name"], c["suite"], paths["perf_tests"]))
            continue
        suite = tests[c["suite"]]
        for n in registered:
            if n not in suite["declared"]:
                errors.append("%s: %s does not declare %s in its intervalNames, so no UI performance test emits it"
                              % (c["key"], c["suite"], n))
        if registered and not suite["measures"]:
            errors.append("%s: no test of %s calls measure(app:), so none emits %s: a declaration alone measures "
                          "nothing (R1861)" % (c["key"], c["suite"], ", ".join(registered)))

    # R1861: every user-facing interval has a catalog entry.
    named = {c["name"] for c in catalog["cases"]}
    for n, info in sorted(registry["names"].items()):
        if info["field_reported"] and n not in named:
            errors.append("R1861 interval %s has no catalog entry" % n)

    if milestones:
        errors += check_milestones(catalog, milestones)
    return errors


def check_milestones(catalog, milestones):
    errors = []
    budgets = {b["id"]: b for b in catalog["budgets"]}
    manifest_devices = milestones.get("budget_devices") or {}
    for rid, b in sorted(budgets.items()):
        mine = b["devices"] if b["restricted"] else None
        theirs = manifest_devices.get(rid)
        theirs = [d for d in DEVICES if d in theirs] if isinstance(theirs, list) else None
        if mine != theirs:
            errors.append("%s: devices %s here, budget_devices %s in milestones.yaml"
                          % (rid, ", ".join(mine or DEVICES), ", ".join(theirs or DEVICES)))
    for rid in sorted(set(manifest_devices) - set(budgets)):
        errors.append("milestones.yaml budget_devices names %s, which has no catalog entry" % rid)
    for mid, groups in sorted((milestones.get("budget_groups") or {}).items()):
        for group, rids in sorted((groups or {}).items()):
            for rid in rids or []:
                if rid not in budgets:
                    errors.append("milestones.yaml %s budgets.%s names %s, which has no catalog entry" % (mid, group, rid))
    for mid, items in sorted((milestones.get("perf_gate_groups") or {}).items()):
        for g in items:
            if g not in catalog["groups"]:
                errors.append("milestones.yaml %s: the perf-gate suite scopes group %s, which is not a catalog group"
                              % (mid, g))
    return errors


# ---------------------------------------------------------------------------
# perf results


def result_problems(doc):
    """What makes `doc` unreadable as a perf result (the fields the gate
    uses; the jobs validate the full schema)."""
    if not isinstance(doc, dict):
        return ["not a JSON object"]
    problems = []
    if doc.get("schema_version") != SCHEMA_VERSION:
        problems.append("schema_version is not %d" % SCHEMA_VERSION)
    if not isinstance(doc.get("suite"), str) or not doc["suite"]:
        problems.append("no suite")
    if doc.get("kind") not in KINDS:
        problems.append("kind is not one of %s" % ", ".join(KINDS))
    if not isinstance(doc.get("configuration"), str):
        problems.append("no configuration")
    runner = doc.get("runner")
    if not isinstance(runner, dict) or not isinstance(runner.get("id"), str) or not runner["id"] \
            or runner.get("kind") not in ("simulator", "device", "mac", "docker"):
        problems.append("no runner kind and id")
    rows = doc.get("results")
    if not isinstance(rows, list):
        problems.append("no results")
        return problems
    for i, r in enumerate(rows):
        if not isinstance(r, dict) or not isinstance(r.get("name"), str) or not isinstance(r.get("metric"), str) \
                or r.get("unit") not in UNITS or not is_number(r.get("value")) \
                or ("percentile" in r and r["percentile"] not in PERCENTILES) \
                or ("fixture" in r and not isinstance(r["fixture"], str)) \
                or ("test" in r and not isinstance(r["test"], str)):
            problems.append("results[%d] is not a perf result row" % i)
    if "attachments" in doc and (not isinstance(doc["attachments"], int) or isinstance(doc["attachments"], bool)
                                 or doc["attachments"] < 0):
        problems.append("attachments is not a count")
    return problems


def load_result(path):
    try:
        doc = read_json(path)
    except (OSError, ValueError) as e:
        return None, ["cannot read it: %s" % e]
    problems = result_problems(doc)
    return (None if problems else doc), problems


def model_family(model):
    m = (model or "").lower()
    if m.startswith("iphone"):
        return "iphone"
    if m.startswith("ipad"):
        return "ipad"
    return None


def placement_of(doc):
    """(placement, device family) of a perf result: core-docker or core-mac
    for core benches; simulator for simulators and the Mac in the test VM
    (virtualized timing); device for real iPhones and iPads and the Mac host."""
    runner = doc.get("runner") or {}
    kind = runner.get("kind")
    if doc.get("kind") in ("wall-clock", "instruction-count"):
        # A budget's devices are the app's device families; core benches have none.
        return {"docker": "core-docker", "mac": "core-mac"}.get(kind), None
    if kind == "simulator":
        return "simulator", model_family(runner.get("model"))
    if kind == "device":
        return "device", model_family(runner.get("model"))
    if kind == "mac":
        return ("simulator" if str(runner.get("id", "")).startswith("vm/") else "device"), "mac"
    return None, None


def refusals(doc):
    """Why a perf result may not be judged (R1866), before anything is."""
    out = []
    kind = doc["kind"]
    expected = CONFIGURATIONS[kind]
    if doc.get("configuration") != expected:
        if kind == "ui":
            out.append("built in %s: performance results come only from the Profile configuration (R1866: Release "
                       "optimization with the test hooks; Debug and Release bundles are refused)" % doc.get("configuration"))
        else:
            out.append("built in %r: core benchmarks run in cargo's bench profile" % doc.get("configuration"))
    if kind == "ui":
        count = doc.get("attachments")
        if not isinstance(count, int) or isinstance(count, bool):
            out.append("records no attachment count, so nothing shows its tests kept no screenshots or attachments "
                       "(R1866)")
        elif count > 0:
            out.append("its tests kept %d attachments or screenshots: performance tests capture none (R1866)" % count)
    return out


def row_key(row):
    return (row.get("name"), row.get("metric"), row.get("unit"), row.get("percentile") or "", row.get("fixture") or "",
            row.get("test") or "")


def baseline_rows(doc):
    return {row_key(r): r["value"] for r in doc.get("rows") or []}


def fixture_rule(case, row_fixture, rules):
    """How R1803 applies to a row: ("scale", factor) for its limit,
    ("completes", None) above 500k, or None when the row's fixture is not the
    case's. A row without a fixture fits only a case without one: it is never
    taken at the case's, so it neither proves nor is judged against a case
    stated for a collection."""
    cf = case["fixture"]
    rf = row_fixture
    if cf is None:
        return ("scale", 1) if rf is None else None
    cs, rs = size_of(cf), size_of(rf)
    if cs is None or rs is None:
        return ("scale", 1) if rf == cf else None
    if rs > LOOSE_SIZE:
        return ("completes", None)
    if case["unit"] in rules["time_units"] and cs <= BUDGET_SIZE < rs:
        return ("scale", case["multiplier_500k"])
    return ("scale", 1)


def covers(case, row_fixture):
    """Whether a measurement at `row_fixture` (None: no fixture) is at the
    collection the case's limit is stated for. Other sizes are judged by
    R1803's rules, but only this one proves the case."""
    cf = case["fixture"]
    rf = row_fixture
    return rf == cf or (size_of(cf) is not None and size_of(cf) == size_of(rf))


def regression_percent(value, base):
    if base == 0:
        return 0.0 if value <= 0 else math.inf
    return (value - base) / abs(base) * 100.0


def over(pct, threshold):
    return round(pct, 6) > threshold


def case_applies(case, doc, place, family):
    """Whether `case` is measured by results of `doc`'s suite, placement and
    device family."""
    if place not in case["where"] or case["suite"] != doc["suite"]:
        return False
    if family is not None and family not in case["devices"]:
        return False
    if family is None and place in UI_PLACEMENTS and len(case["devices"]) < len(DEVICES):
        return False
    return True


def case_matches(case, doc, row, place, family):
    """Whether `row` of `doc` measures `case` (its unit aside: a unit that
    differs is a failure, not another case)."""
    if not case_applies(case, doc, place, family):
        return False
    if case["test"] and not fnmatch.fnmatchcase(row.get("test", ""), case["test"]):
        return False
    return (row["name"] == case["name"] and row["metric"] == case["metric"]
            and row.get("percentile") == case["percentile"])


def describe(doc):
    runner = doc["runner"]
    return runner.get("model") or runner.get("cpu") or runner["id"]


def judge(case, row, doc, place, family, base, rules):
    """One evaluation of `case` on `row`: status pass, fail or unchecked."""
    fixture = row.get("fixture")
    ev = {"case": case["key"], "budget": case["budget"], "kind": case["kind"], "group": case["group"],
          "suite": doc["suite"], "test": row.get("test"), "name": row["name"], "metric": row["metric"],
          "unit": row["unit"], "percentile": row.get("percentile"), "fixture": fixture, "runner": doc["runner"]["id"],
          "placement": place, "family": family, "raw": row["value"], "value": row["value"], "limit": None,
          "rule": None, "baseline": None, "regression_percent": None, "pending": case["pending"],
          "status": "pass", "message": ""}
    where = "%s, %s%s" % (place, describe(doc), (", " + fixture) if fixture else "")
    label = "%s %s %s" % (case["budget"], row["name"], row.get("percentile") or row["metric"])
    if row["unit"] != case["unit"]:
        ev.update(status="fail", message="%s: measured in %s, the catalog's %s is in %s (%s)"
                  % (label, row["unit"], case["key"], case["unit"], where))
        return ev
    rule = fixture_rule(case, row.get("fixture"), rules)
    if rule[0] == "completes":
        ev.update(rule="completes", message="%s: completed at %s (R1803: above 500k only completion counts)"
                  % (label, fixture))
        return ev
    factor = rule[1]
    if case["relative_to"]:
        refs = [r for r in doc["results"] if r["name"] == case["relative_to"] and r["metric"] == row["metric"]
                and r["unit"] == row["unit"] and r.get("percentile") == row.get("percentile")]
        if not refs:
            ev.update(status="fail", message="%s: no %s %s measurement in %s to compare with (%s)"
                      % (label, case["relative_to"], row.get("percentile") or "", doc["suite"], where))
            return ev
        ev["reference"] = refs[0]["value"]
        ev["value"] = row["value"] - refs[0]["value"]
    problems, checks = [], []
    absolute = place != "simulator" or case["simulator"] == "absolute" or (
        case["simulator"] is None and case["unit"] in rules["simulator_absolute_units"])
    if absolute and case["limit"] is not None:
        limit = case["limit"] * factor
        ev["limit"] = limit
        checks.append("absolute")
        breach = ev["value"] >= limit if case["strict"] else ev["value"] > limit
        if breach:
            against = (" against %s" % case["relative_to"]) if case["relative_to"] else ""
            scaled = (" (%sx the %s budget at %s, R1803)" % (fmt(factor), case["fixture"], fixture)) if factor != 1 else ""
            problems.append("%s %s %s%s %s %s %s%s (%s)" % (label, fmt(ev["value"]), row["unit"], against,
                                                            ">=" if case["strict"] else ">", fmt(limit), row["unit"],
                                                            scaled, where))
    if doc["kind"] == "ui" and base is not None:
        b = base.get(row_key(row))
        if b is not None:
            pct = regression_percent(row["value"], b)
            ev.update(baseline=b, regression_percent=pct)
            checks.append("regression")
            if over(pct, rules["ui_regression_percent"]):
                problems.append("%s: regression %s > %s%% against the baseline (%s -> %s %s; %s)" % (
                    label, fmt_percent(pct), fmt(rules["ui_regression_percent"]), fmt(b), fmt(row["value"]),
                    row["unit"], where))
    ev["rule"] = "+".join(checks) or None
    if problems:
        ev.update(status="fail", message="; ".join(problems))
    elif not checks:
        ev.update(status="unchecked", message="%s: no baseline for %s on %s: ms on simulators is gated by the "
                  "+%s%% rule alone (R1863); record one with tools/mg perf-gate --rebaseline"
                  % (label, doc["suite"], doc["runner"]["id"], fmt(rules["ui_regression_percent"])))
    return ev


def judge_icount(row, doc, base, rules):
    ev = {"case": None, "budget": None, "kind": "instruction-count", "group": None, "suite": doc["suite"],
          "test": row.get("test"), "name": row["name"], "metric": row["metric"], "unit": row["unit"],
          "percentile": row.get("percentile"), "fixture": row.get("fixture"), "runner": doc["runner"]["id"],
          "placement": placement_of(doc)[0], "family": None, "raw": row["value"], "value": row["value"],
          "limit": None, "rule": "regression", "baseline": None, "regression_percent": None, "pending": None,
          "status": "pass", "message": ""}
    b = (base or {}).get(row_key(row))
    label = "%s %s%s" % (doc["suite"], row["name"], (" at " + row["fixture"]) if row.get("fixture") else "")
    if b is None:
        ev.update(rule=None, status="unchecked", message="%s: no instruction-count baseline on %s; record one with "
                  "tools/mg perf-gate --rebaseline" % (label, doc["runner"]["id"]))
        return ev
    pct = regression_percent(row["value"], b)
    ev.update(baseline=b, regression_percent=pct)
    if over(pct, rules["icount_regression_percent"]):
        ev.update(status="fail", message="%s: instruction count regression %s > %s%% (%s -> %s; R1864)" % (
            label, fmt_percent(pct), fmt(rules["icount_regression_percent"]), fmt(b), fmt(row["value"])))
    return ev


STATUS_ORDER = ("fail", "missing", "pending", "unchecked", "pass")


def case_status(case, evaluations):
    """fail when any evaluation of `case` fails; otherwise pass (or
    unchecked) from those at its own fixture; None when none is there."""
    if any(e["status"] == "fail" for e in evaluations):
        return "fail"
    at = [e["status"] for e in evaluations if covers(case, e["fixture"])]
    if "pass" in at:
        return "pass"
    return "unchecked" if at else None


def rollup(catalog, evaluations, entries):
    """{entry or group: {status, cases: {status: n}, pending: [units]}} over
    the cases the perf jobs measure (a case measured only on devices is left
    to the device runs, unless a device result is evaluated here). A case
    judged only at other sizes than its fixture is missing (or pending)."""
    per_case = {}
    for e in evaluations:
        if e.get("case"):
            per_case.setdefault(e["case"], []).append(e)
    out = {}
    for name, cases in entries:
        counts, pending = {}, []
        for c in cases:
            evs = per_case.get(c["key"], [])
            s = case_status(c, evs)
            if s is None:
                if not evs and set(c["where"]) <= {"device"}:
                    continue
                s = "pending" if c["pending"] else "missing"
                if c["pending"] and c["pending"] not in pending:
                    pending.append(c["pending"])
            counts[s] = counts.get(s, 0) + 1
        status = next((s for s in STATUS_ORDER if counts.get(s)), "device-only")
        out[name] = {"status": status, "cases": counts, "pending": sorted(pending)}
    return out


def evaluate(catalog, inputs, baselines, require=(), problems=()):
    """The evaluation of `inputs` ([{path, job, run_id, result}]) against the
    catalog and `baselines` ({(runner id, suite): baseline document})."""
    rules = catalog["rules"]
    evaluations, tracked, failures, warnings, recorded, unmeasured = [], [], [], [], [], []
    untracked = 0
    for p in problems:
        failures.append({"id": "input", "message": p})
    for i, item in enumerate(inputs):
        doc = item["result"]
        place, family = placement_of(doc)
        refused = refusals(doc)
        recorded.append({"index": i, "path": item.get("path"), "job": item.get("job"), "run_id": item.get("run_id"),
                         "suite": doc["suite"], "kind": doc["kind"], "configuration": doc.get("configuration"),
                         "runner": doc["runner"], "placement": place, "family": family, "refused": refused,
                         "result": doc})
        if refused:
            failures.append({"id": "refused/%s" % doc["suite"],
                             "message": "%s on %s (%s) is refused before evaluating: %s" % (
                                 doc["suite"], doc["runner"]["id"], item.get("path"), "; ".join(refused))})
            continue
        if place is None:
            failures.append({"id": "input/%s" % doc["suite"],
                             "message": "%s: runner kind %r does not fit a %s result" % (
                                 item.get("path"), doc["runner"].get("kind"), doc["kind"])})
            continue
        base = baselines.get((doc["runner"]["id"], doc["suite"]))
        base = baseline_rows(base) if base is not None else None
        measured, elsewhere = set(), {}
        for row in doc["results"]:
            if doc["kind"] == "instruction-count":
                evaluations.append(dict(judge_icount(row, doc, base, rules), input=i))
                continue
            if doc["kind"] == "wall-clock" and row.get("percentile") in ("p50", "p95", "p99"):
                tracked.append({"suite": doc["suite"], "name": row["name"], "metric": row["metric"],
                                "unit": row["unit"], "percentile": row["percentile"], "value": row["value"],
                                "fixture": row.get("fixture"), "runner": doc["runner"]["id"]})
            matched = False
            for case in catalog["cases"]:
                if not case_matches(case, doc, row, place, family):
                    continue
                if row["unit"] == case["unit"] and fixture_rule(case, row.get("fixture"), rules) is None:
                    elsewhere.setdefault(case["key"], []).append(row.get("fixture"))
                    continue
                matched = True
                measured.add(case["key"])
                evaluations.append(dict(judge(case, row, doc, place, family, base, rules), input=i))
            untracked += 0 if matched else 1
        # R1861: the suite's result measures every case the catalog gates on
        # it; a declared interval no test measures has no row.
        if doc["kind"] != "instruction-count":
            for case in catalog["cases"]:
                if case["pending"] or case["key"] in measured or not case_applies(case, doc, place, family):
                    continue
                at = sorted({f or "no fixture" for f in elsewhere.get(case["key"], [])})
                unmeasured.append({"id": "missing/%s" % case["key"], "message": (
                    "%s on %s (%s) measured no %s %s for %s: a suite's results measure every case the catalog "
                    "gates on it (R1861)%s" % (
                        doc["suite"], doc["runner"]["id"], item.get("path"), case["name"], case["percentile"] or "",
                        case["key"], ("; its rows are at %s, the case is at %s" % (", ".join(at), case["fixture"]))
                        if at else ""))})
    for job in require:
        if not any(r["job"] == job for r in recorded):
            failures.append({"id": "require/%s" % job,
                             "message": "no %s result with perf results on this commit; run tools/mg %s first"
                             % (job, job)})
    for e in evaluations:
        target = e["case"] or "icount/%s/%s" % (e["suite"], e["name"])
        if e["status"] == "fail":
            failures.append({"id": "budget/%s" % target if e["case"] else target, "message": e["message"]})
        elif e["status"] == "unchecked":
            warnings.append(e["message"])
    failures += unmeasured
    if not evaluations:
        warnings.append("nothing the catalog gates was measured: %d perf results, %d untracked rows"
                        % (len(recorded), untracked))
    budgets = rollup(catalog, evaluations, [(b["id"], b["cases"]) for b in catalog["budgets"] + catalog["probes"]])
    groups = rollup(catalog, evaluations,
                    [(g, [c for c in catalog["cases"] if c["group"] == g]) for g in catalog["groups"]])
    status = "fail" if failures else ("warn" if warnings else "pass")
    return {"schema_version": SCHEMA_VERSION, "status": status, "inputs": recorded, "evaluations": evaluations,
            "tracked": tracked, "untracked": untracked, "failures": failures, "warnings": warnings,
            "groups": groups, "budgets": {k: v["status"] for k, v in budgets.items()}}


# ---------------------------------------------------------------------------
# inputs: the perf jobs' results on one commit


def discover(results_root, jobs, commit, dirty):
    """([{path, job, run_id, result}], problems): the perf results of `jobs`'
    non-dry-run results made on `commit` with the same tree state, the
    latest for each suite, runner and kind."""
    latest, problems = {}, []
    for job in jobs:
        directory = os.path.join(results_root, job)
        if not os.path.isdir(directory):
            continue
        for run in sorted(os.listdir(directory)):
            path = os.path.join(directory, run, "result.json")
            if run == "latest" or not os.path.isfile(path):
                continue
            try:
                result = read_json(path)
            except (OSError, ValueError):
                continue
            meta = result.get("commit") or {}
            if result.get("dry_run") or meta.get("sha") != commit or bool(meta.get("dirty")) != bool(dirty):
                continue
            stamp = (result.get("finished_at") or "", run)
            for artifact in result.get("artifacts") or []:
                if artifact.get("kind") != "perf-result":
                    continue
                doc, why = load_result(artifact.get("path", ""))
                if doc is None:
                    problems.append("%s (%s %s): not a perf result: %s" % (artifact.get("path"), job, run, "; ".join(why)))
                    continue
                key = (doc["suite"], doc["runner"]["id"], doc["kind"])
                if key not in latest or stamp > latest[key][0]:
                    latest[key] = (stamp, {"path": artifact["path"], "job": job, "run_id": run, "result": doc})
    found = [v[1] for _, v in sorted(latest.items(), key=lambda kv: (kv[1][1]["job"], kv[0]))]
    return found, problems


def given(paths):
    """([{path, job, run_id, result}], problems) of perf result files and
    directories of them."""
    found, problems = [], []
    files = []
    for p in paths:
        if os.path.isdir(p):
            files += [os.path.join(p, n) for n in sorted(os.listdir(p)) if n.endswith(".json")]
        else:
            files.append(p)
    for f in files:
        doc, why = load_result(f)
        if doc is None:
            problems.append("%s: not a perf result: %s" % (f, "; ".join(why)))
        else:
            found.append({"path": f, "job": None, "run_id": None, "result": doc})
    return found, problems


# ---------------------------------------------------------------------------
# baselines


def runner_slug(runner_id):
    return re.sub(r"[^a-z0-9.]+", "-", runner_id.lower()).strip("-.") or "runner"


def baseline_problems(doc, relpath):
    problems = []
    if not isinstance(doc, dict):
        return ["not a JSON object"]
    if doc.get("schema_version") != SCHEMA_VERSION:
        problems.append("schema_version is not %d" % SCHEMA_VERSION)
    runner = doc.get("runner")
    if not isinstance(runner, dict) or not isinstance(runner.get("id"), str) or not runner["id"]:
        problems.append("no runner id")
    elif os.path.dirname(relpath) != runner_slug(runner["id"]):
        problems.append("lies under %s/, but its runner %s belongs under %s/"
                        % (os.path.dirname(relpath), runner["id"], runner_slug(runner["id"])))
    if not isinstance(doc.get("suite"), str) or os.path.basename(relpath) != "%s.json" % doc.get("suite"):
        problems.append("its file is not named after its suite")
    if doc.get("kind") not in ("ui", "instruction-count"):
        problems.append("kind is ui or instruction-count (wall-clock benches have no baseline)")
    reason = doc.get("reason")
    if not isinstance(reason, str) or not reason.strip():
        problems.append("no reason: a baseline changes only through tools/mg perf-gate --rebaseline --reason")
    if not isinstance(doc.get("measured_commit"), str) or not SHA.match(doc["measured_commit"]):
        problems.append("measured_commit is not a commit id")
    rows = doc.get("rows")
    if not isinstance(rows, list) or not all(
            isinstance(r, dict) and isinstance(r.get("name"), str) and isinstance(r.get("metric"), str)
            and r.get("unit") in UNITS and is_number(r.get("value")) for r in rows):
        problems.append("rows are {name, metric, unit, [percentile, fixture, test], value}")
    return problems


def read_baselines(directory):
    """({(runner id, suite): document}, errors) under `directory`
    (<runner slug>/<suite>.json)."""
    out, errors = {}, []
    if not os.path.isdir(directory):
        return out, errors
    for slug in sorted(os.listdir(directory)):
        sub = os.path.join(directory, slug)
        if not os.path.isdir(sub):
            continue
        for name in sorted(os.listdir(sub)):
            if not name.endswith(".json"):
                continue
            rel = os.path.join(slug, name)
            try:
                doc = read_json(os.path.join(sub, name))
            except (OSError, ValueError) as e:
                errors.append("baseline %s: %s" % (rel, e))
                continue
            problems = baseline_problems(doc, rel)
            if problems:
                errors.append("baseline %s: %s" % (rel, "; ".join(problems)))
                continue
            out[(doc["runner"]["id"], doc["suite"])] = doc
    return out, errors


def rebaseline_docs(catalog, evaluation, reason, commit):
    """{<runner slug>/<suite>.json: baseline} from the evaluation's accepted
    inputs: UI results keep the rows a catalog case measures (below 1M),
    instruction counts every row; wall-clock benches have none."""
    if not isinstance(reason, str) or not reason.strip() or "\n" in reason.strip():
        raise Invalid("a rebaseline needs a one-line reason")
    if not SHA.match(commit or ""):
        raise Invalid("a rebaseline names the commit its results were measured on")
    rules = catalog["rules"]
    out = {}
    for item in evaluation["inputs"]:
        doc = item["result"]
        if item["refused"] or item["placement"] is None or doc["kind"] == "wall-clock":
            continue
        rows = []
        for row in doc["results"]:
            if doc["kind"] == "ui":
                keep = False
                for case in catalog["cases"]:
                    if case_matches(case, doc, row, item["placement"], item["family"]):
                        rule = fixture_rule(case, row.get("fixture"), rules)
                        keep = keep or (rule is not None and rule[0] == "scale")
                if not keep:
                    continue
            kept = {k: row[k] for k in ("name", "metric", "unit", "percentile", "fixture", "test", "value") if k in row}
            rows.append(kept)
        if not rows:
            continue
        rows.sort(key=lambda r: row_key(r))
        runner = {k: doc["runner"][k] for k in ("id", "kind", "model", "os", "os_build", "cpu") if k in doc["runner"]}
        out[os.path.join(runner_slug(runner["id"]), "%s.json" % doc["suite"])] = {
            "schema_version": SCHEMA_VERSION, "runner": runner, "suite": doc["suite"], "kind": doc["kind"],
            "measured_commit": commit, "reason": reason.strip(), "rows": rows}
    return out


def write_baselines(directory, docs):
    for rel, doc in sorted(docs.items()):
        write_json(os.path.join(directory, rel), doc)


# ---------------------------------------------------------------------------
# device runs (U256's evidence; the milestone gate verifies it)


def device_cases(catalog, rid, device):
    entry = [b for b in catalog["budgets"] if b["id"] == rid]
    if not entry or device not in entry[0]["devices"]:
        return []
    return [c for c in entry[0]["cases"] if "device" in c["where"]]


CASE_FIELDS = ("suite", "test", "name", "metric", "unit", "percentile", "fixture", "value", "raw", "reference",
               "limit", "rule", "baseline", "regression_percent", "status", "message")


def device_report(catalog, evaluation, milestone, device, build_commit):
    """(report, problems): the device run's budget report from the
    evaluation of that device's perf results."""
    problems = []
    evs = [e for e in evaluation["evaluations"] if e.get("case") and e["placement"] == "device"
           and e["family"] == device]
    runners = sorted({e["runner"] for e in evs})
    if not evs:
        problems.append("no %s device results among the evaluated perf results" % device)
    elif len(runners) > 1:
        problems.append("results from several %s devices: %s" % (device, ", ".join(runners)))
    if not MILESTONE.match(milestone or ""):
        problems.append("milestone is M0..M9")
    if not SHA.match(build_commit or ""):
        problems.append("build_commit is a full commit id")
    if problems:
        return None, problems
    runner = [i["runner"] for i in evaluation["inputs"] if i["runner"]["id"] == runners[0]][0]
    budgets = []
    for b in catalog["budgets"]:
        cases = device_cases(catalog, b["id"], device)
        if not cases:
            continue
        rows = []
        for c in cases:
            mine = [e for e in evs if e["case"] == c["key"]]
            for e in mine:
                rows.append(dict({"case": c["key"]}, **{k: e.get(k) for k in CASE_FIELDS}))
            if not any(covers(c, e["fixture"]) for e in mine):
                # Another size is judged, but proves nothing at the case's own (R1803).
                rows.append({"case": c["key"], "status": "pending" if c["pending"] else "missing",
                             "pending": c["pending"], "fixture": c["fixture"]})
        statuses = {r["status"] for r in rows}
        status = "fail" if "fail" in statuses else ("pass" if statuses == {"pass"} else "incomplete")
        budgets.append({"id": b["id"], "status": status, "cases": rows})
    report = {"schema_version": SCHEMA_VERSION, "kind": "device-run", "milestone": milestone, "device": device,
              "build_commit": build_commit, "configuration": CONFIGURATIONS["ui"], "runner": runner,
              "budgets": budgets}
    return report, []


def recheck(case, rec, rules):
    """(status, problem) of one recorded case of a device run, recomputed
    from its values with the catalog's rule for devices (absolute, plus the
    +10% rule when it records a baseline)."""
    label = "%s %s" % (case["key"], case["name"])
    for key in ("suite", "name", "metric", "unit", "percentile"):
        if rec.get(key) != case[key]:
            return "invalid", "%s: records %s %r, the catalog %r" % (label, key, rec.get(key), case[key])
    if not is_number(rec.get("value")):
        return "invalid", "%s: records no value" % label
    if case["relative_to"] and (not is_number(rec.get("raw")) or not is_number(rec.get("reference"))
                                or abs(rec["raw"] - rec["reference"] - rec["value"]) > 1e-9):
        return "invalid", "%s: its value is not its measurement minus %s's" % (label, case["relative_to"])
    rule = fixture_rule(case, rec.get("fixture"), rules)
    if rule is None:
        return "invalid", "%s: fixture %r is not the catalog's %r" % (label, rec.get("fixture"), case["fixture"])
    if rule[0] == "completes":
        return "pass", None
    checked, failed = False, []
    if case["limit"] is not None:
        limit = case["limit"] * rule[1]
        checked = True
        if (rec["value"] >= limit) if case["strict"] else (rec["value"] > limit):
            failed.append("%s %s %s > %s %s" % (label, rec.get("percentile") or "", fmt(rec["value"]), fmt(limit),
                                                case["unit"]))
    if is_number(rec.get("baseline")) and is_number(rec.get("raw")):
        checked = True
        pct = regression_percent(rec["raw"], rec["baseline"])
        if over(pct, rules["ui_regression_percent"]):
            failed.append("%s regression %s > %s%%" % (label, fmt_percent(pct), fmt(rules["ui_regression_percent"])))
    if not checked:
        return "unchecked", "%s: no limit and no baseline" % label
    return ("fail", "; ".join(failed)) if failed else ("pass", None)


def verify_device_report(report, catalog, device, required):
    """(outcome, message, details) of a device run's report against the
    catalog: pass, fail, or pending while a required case still waits on the
    unit that lands its measurement."""
    if not isinstance(report, dict):
        return "fail", "the report is not a JSON object", {}
    problems = []
    if report.get("schema_version") != SCHEMA_VERSION:
        problems.append("schema_version %r is not %d (U241's device-run report)"
                        % (report.get("schema_version"), SCHEMA_VERSION))
    if report.get("kind") != "device-run":
        problems.append("kind is not device-run")
    if report.get("configuration") != CONFIGURATIONS["ui"]:
        problems.append("configuration %r: device runs measure the Profile build (R1866)" % report.get("configuration"))
    runner = report.get("runner")
    place, family = placement_of({"kind": "ui", "runner": runner if isinstance(runner, dict) else {}})
    if place != "device" or family != device:
        problems.append("runner %r is not an %s device" % ((runner or {}).get("id") if isinstance(runner, dict)
                                                           else runner, device))
    if not isinstance(report.get("budgets"), list):
        return "fail", "; ".join(problems + ["no budgets"]), {}
    if problems:
        return "fail", "; ".join(problems), {}
    entries = {}
    for b in report["budgets"]:
        if not isinstance(b, dict) or not isinstance(b.get("id"), str) or not isinstance(b.get("cases"), list):
            return "fail", "every budget is {id, status, cases}", {}
        if b["id"] in entries:
            problems.append("%s is recorded twice" % b["id"])
        entries[b["id"]] = b
    rules = catalog["rules"]
    failed, absent, pending = [], [], {}
    for rid in required:
        b = entries.get(rid)
        if b is None:
            absent.append(rid)
            continue
        cases = device_cases(catalog, rid, device)
        if not cases:
            problems.append("the catalog has no %s device case for %s" % (device, rid))
            continue
        recorded = {}
        for r in b["cases"]:
            if isinstance(r, dict):
                recorded.setdefault(r.get("case"), []).append(r)
        statuses, why = [], []
        for c in cases:
            measured = [r for r in recorded.get(c["key"], []) if r.get("status") not in ("missing", "pending")]
            # Only a record at the case's own fixture proves it; one at
            # another size (a 1M completion) is judged too (R1803).
            if not any(covers(c, r.get("fixture")) for r in measured):
                if c["pending"]:
                    pending.setdefault(c["pending"], []).append(c["key"])
                    statuses.append("pending")
                else:
                    problems.append("%s records no result for %s%s" % (
                        rid, c["key"], (" at " + c["fixture"]) if c["fixture"] else ""))
                    statuses.append("missing")
            for r in measured:
                status, problem = recheck(c, r, rules)
                if status == "invalid":
                    problems.append(problem)
                    statuses.append("missing")
                    continue
                if status == "unchecked":
                    if c["pending"]:
                        pending.setdefault(c["pending"], []).append(c["key"])
                        statuses.append("pending")
                    else:
                        problems.append(problem)
                        statuses.append("missing")
                    continue
                if r.get("status") != status:
                    problems.append("%s records %s for %s, but its values give %s%s" % (
                        rid, r.get("status"), c["key"], status, (": " + problem) if problem else ""))
                statuses.append(status)
                if status == "fail":
                    why.append(problem)
        expected = "fail" if "fail" in statuses else ("pass" if set(statuses) == {"pass"} else "incomplete")
        if expected == "fail":
            failed.append("%s (%s)" % (rid, "; ".join(why)))
        if b.get("status") != expected:
            problems.append("%s records status %r, its cases give %s" % (rid, b.get("status"), expected))
    details = {"failed": [f.split(" ")[0] for f in failed], "absent": absent, "pending": sorted(pending)}
    if failed:
        return "fail", "records the failure of %s" % ", ".join(failed), details
    if absent:
        return "fail", "records no result for %s" % ", ".join(absent), details
    if problems:
        return "fail", "; ".join(problems), details
    if pending:
        return "pending", "the catalog still waits on %s for %s" % (
            ", ".join(sorted(pending)), ", ".join(k for u in sorted(pending) for k in pending[u])), details
    return "pass", "%d budgets pass on %s" % (len(required), runner.get("id")), details


# ---------------------------------------------------------------------------
# command line


def load_catalog(path):
    """The normalized catalog and the milestones excerpt from `load`'s output."""
    doc = read_json(path)
    if not isinstance(doc, dict) or "catalog" not in doc:
        raise Invalid("%s is not perf_gate.py load output" % path)
    catalog, errors = normalize(doc["catalog"])
    if errors:
        raise Invalid("; ".join(errors))
    return catalog, doc.get("milestones")


def cmd_load(a):
    try:
        write_json(a.out, load(a.catalog, a.milestones))
    except (Invalid, OSError) as e:
        print(json.dumps({"errors": [str(e)]}))
        return 1
    except Exception as e:  # a YAML syntax error
        print(json.dumps({"errors": ["%s: %s" % (a.catalog, e)]}))
        return 1
    _, errors = normalize(read_json(a.out)["catalog"])
    print(json.dumps({"errors": errors}))
    return 1 if errors else 0


def cmd_check(a):
    doc = read_json(a.catalog)
    catalog, errors = normalize(doc.get("catalog"))
    if not errors:
        errors = check(catalog, a.root, doc.get("milestones"))
    print(json.dumps({"errors": errors}))
    return 1 if errors else 0


def cmd_evaluate(a):
    try:
        catalog, _ = load_catalog(a.catalog)
    except (Invalid, OSError, ValueError) as e:
        write_json(a.out, {"status": "error", "errors": ["catalog: %s" % e]})
        return 2
    baselines, errors = read_baselines(a.baselines)
    if errors:
        write_json(a.out, {"status": "error", "errors": errors})
        return 2
    if a.results:
        inputs, problems = given(a.results)
    elif a.discover:
        inputs, problems = discover(a.discover, a.jobs.split(), a.commit, a.dirty == "true")
    else:
        inputs, problems = [], []
    evaluation = evaluate(catalog, inputs, baselines, a.require or [], problems)
    write_json(a.out, evaluation)
    return 1 if evaluation["status"] == "fail" else 0


def cmd_rebaseline(a):
    catalog, _ = load_catalog(a.catalog)
    evaluation = read_json(a.evaluation)
    try:
        docs = rebaseline_docs(catalog, evaluation, a.reason, a.commit)
    except Invalid as e:
        print(json.dumps({"errors": [str(e)], "written": []}))
        return 1
    write_baselines(a.baselines, docs)
    print(json.dumps({"errors": [], "written": sorted(os.path.join(a.baselines, p) for p in docs)}))
    return 0


def cmd_device_run(a):
    catalog, _ = load_catalog(a.catalog)
    evaluation = read_json(a.evaluation)
    report, problems = device_report(catalog, evaluation, a.milestone, a.device, a.build_commit)
    if problems:
        print(json.dumps({"errors": problems}))
        return 1
    write_json(a.out, report)
    print(json.dumps({"errors": []}))
    return 0


def main(argv):
    parser = argparse.ArgumentParser(prog="perf_gate.py")
    commands = parser.add_subparsers(dest="command")
    p = commands.add_parser("load")
    p.add_argument("--catalog", required=True)
    p.add_argument("--milestones")
    p.add_argument("--out", required=True)
    p = commands.add_parser("check")
    p.add_argument("--catalog", required=True)
    p.add_argument("--root", required=True)
    p = commands.add_parser("evaluate")
    p.add_argument("--catalog", required=True)
    p.add_argument("--baselines", required=True)
    p.add_argument("--results", action="append")
    p.add_argument("--discover")
    p.add_argument("--jobs", default="")
    p.add_argument("--commit")
    p.add_argument("--dirty", choices=("true", "false"), default="false")
    p.add_argument("--require", action="append")
    p.add_argument("--out", required=True)
    p = commands.add_parser("rebaseline")
    p.add_argument("--catalog", required=True)
    p.add_argument("--evaluation", required=True)
    p.add_argument("--baselines", required=True)
    p.add_argument("--reason", required=True)
    p.add_argument("--commit", required=True)
    p = commands.add_parser("device-run")
    p.add_argument("--catalog", required=True)
    p.add_argument("--evaluation", required=True)
    p.add_argument("--milestone", required=True)
    p.add_argument("--device", required=True, choices=DEVICES)
    p.add_argument("--build-commit", required=True)
    p.add_argument("--out", required=True)
    a = parser.parse_args(argv)
    handlers = {"load": cmd_load, "check": cmd_check, "evaluate": cmd_evaluate, "rebaseline": cmd_rebaseline,
                "device-run": cmd_device_run}
    if a.command not in handlers:
        parser.print_usage(sys.stderr)
        return 2
    try:
        return handlers[a.command](a)
    except (Invalid, OSError, ValueError) as e:
        print("perf_gate.py %s: %s" % (a.command, e), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
