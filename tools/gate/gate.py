#!/usr/bin/env python3
"""Test tiers, merge gate, quarantine and scheduled runs (U19; R1950-R1953).

tools/mg gate, merge, nightly, milestone and quarantine call this; nothing
else should. Commands:

  load      tiers.yaml + quarantine.yaml + the job registry -> config.json.
            The only command that reads YAML (PyYAML): it runs in the dev
            container. Everything else is stdlib JSON, so macOS's
            /usr/bin/python3 (3.9) runs it on the host.
  plan      config.json + changed paths + mode -> plan.json (what runs, in
            which order, with which arguments, and why the rest does not).
  evaluate  plan.json + job results (and a merge's worktree receipt) ->
            receipt.json: one outcome per run, quarantine applied, the
            caller's receipt-level blockers, and the verdict.
  quarantine-entry  checks a new quarantine entry against two gate receipts
            of one commit (the gate-recorded evidence) and prints its YAML.

Exit status: 0 done (a failing verdict is still 0: read the receipt), 1 the
input is invalid (errors on stdout as JSON), 2 usage.
"""

import argparse
import datetime
import json
import os
import re
import sys

TIERS = ("T0", "T1", "T2", "T3", "T4")
STAGES = ("static", "canary", "main")
CLASSES = ("static", "unit", "conformance", "sync", "data-safety", "ui", "perf", "tooling", "similarity")
# R1953: the milestone gates fail while a test of these classes is quarantined.
PROTECTED = ("conformance", "sync", "data-safety")
NEEDS = ("oracle", "sync-server", "image-diff", "frame-extraction", "simulator", "device", "mac-ui",
         "refs", "private-data", "vm")
# R1952: what each need forces on the placement of the run's test work.
DOCKER_NEEDS = ("oracle", "sync-server", "image-diff", "frame-extraction")
HOST_NEEDS = ("simulator", "device", "mac-ui", "vm")
MODES = ("local", "full", "nightly", "weekly", "milestone", "release")
SCHEDULES = ("nightly", "weekly")
# worktree: runs where the gate runs. main: tools/mg merge runs merge_args from
# the main checkout against the branch's tree. tip: tools/mg merge runs args in
# a full checkout of the branch tip (the branch's code and oracle/, the main
# checkout's corpus).
WHERE = ("worktree", "main", "tip")
# Whose oracle/ this checkout's oracle-image runs, and the runs that read the
# oracle/ files (oracle_files), would use (gate.sh): its own, the main
# checkout's when that is exactly this HEAD's, or neither.
ORACLE_TREES = ("own", "same", "differs")
RECEIPT_VERSION = 2
RUN_KEYS = {
    "id", "job", "args", "merge_args", "tier", "stage", "placement", "class", "triggers", "filters",
    "budget_s", "local", "where", "oracle", "oracle_files", "needs", "schedule", "pending", "disabled",
    "blocking", "require", "release_only", "user_step", "why",
}
QUARANTINE_KEYS = {"test", "run", "class", "owner", "reason", "deadline", "approved_by", "evidence", "escalation"}
EVIDENCE_KEYS = {"commit", "failed", "passed"}
RUN_ID = re.compile(r"^[a-z][a-z0-9-]*$")
GATE_RUN = re.compile(r"^[0-9]{8}T[0-9]{6}Z-[0-9]+$")
SHA = re.compile(r"^[0-9a-f]{40}$")
PLACEMENT = re.compile(r"^(host|vm|docker:[a-z][a-z0-9-]*)$")
PLACEHOLDER = re.compile(r"\{([a-z]+)\}")
PLACEHOLDERS = ("base", "tip", "seed", "milestone")
BRACED = re.compile(r"\{([^{}]*)\}")
# A whole argument naming a file an earlier run of the same gate wrote (an
# artifact of its result); tools/mg resolves it when the run starts.
ARTIFACT = re.compile(r"^\{artifact:([a-z][a-z0-9-]*):([A-Za-z0-9._-]+)\}$")
# Outcomes that never block. `queued`: a run the machine's state kept from
# testing (warn with details.queued: no test VM, a locked host, macOS
# dropping XCUITest's input; U19b). Its receipt record names the user steps
# and the parts that did not run; tools/mg merge queues it for tools/mg
# catch-up, which runs it again once doctor reports the steps clear.
NONBLOCKING = ("pass", "warn", "queued", "quarantined", "pending", "disabled", "not-triggered",
               "skipped-local", "user-step")
# Outcomes that leave a receipt incomplete for a merge: the run still has to
# happen somewhere (the main checkout, or a full checkout of the branch tip).
INCOMPLETE = ("skipped-local", "deferred-main", "deferred-tip")
# Runs the gate did not start because an earlier failure decided the verdict:
# that failure blocks, these do not add to it. A receipt with one is never
# merge-ready: a required run did not run.
CONSEQUENCES = ("skipped-after-canary", "skipped-after-t0")
# Failure ids with which an oracle job refuses the oracle/ it would use
# (tools/mg.d/oracle.sh): `conformance --live` reports one in a failing
# result, `oracle test` in an error result. Either way the run needs a
# checkout that has the branch's own oracle/.
ORACLE_TREE_FAILURES = ("oracle-tree-differs", "no-oracle-tree")


class Invalid(Exception):
    pass


# ---------------------------------------------------------------------------
# Path globs: `**` any number of directories, `*` within one segment, `?` one
# character; a trailing `/` matches everything under the directory; a leading
# `!` excludes.

def glob_regex(pattern):
    if pattern.endswith("/"):
        pattern += "**"
    out, i = [], 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        elif pattern[i] == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    return re.compile("^" + "".join(out) + "$")


def matcher(patterns):
    include = [glob_regex(p) for p in patterns if not p.startswith("!")]
    exclude = [glob_regex(p[1:]) for p in patterns if p.startswith("!")]

    def matches(path):
        return any(r.match(path) for r in include) and not any(r.match(path) for r in exclude)

    return matches


# ---------------------------------------------------------------------------
# load


def read_yaml(path):
    import yaml  # the dev container has PyYAML; the host does not

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
        try:
            return yaml.load(f, Loader=UniqueKeyLoader)
        except yaml.YAMLError as e:
            raise Invalid("%s: not valid YAML: %s" % (path, e))


def read_registry(path):
    jobs = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line:
                continue
            parts = line.split("\t")
            if len(parts) < 2:
                raise Invalid("%s: not a `tools/mg --list` line: %r" % (path, line))
            jobs[parts[0]] = parts[1]
    return jobs


def is_str_list(value):
    return isinstance(value, list) and all(isinstance(v, str) and v for v in value)


def check_placeholders(where, args, errors, artifacts=True):
    """The {artifact:<run>:<file>} references among the args, as (run, file)."""
    refs = []
    for a in args:
        m = ARTIFACT.match(a)
        if m and artifacts:
            refs.append((m.group(1), m.group(2)))
            continue
        for name in BRACED.findall(a):
            if name not in PLACEHOLDERS:
                errors.append("%s: unknown placeholder {%s} (known: %s%s)" % (
                    where, name, ", ".join(PLACEHOLDERS),
                    ", and in args a whole argument {artifact:<run id>:<file name>}" if artifacts else ""))
    return refs


def check_run(run, index, tiers, registry, conformance, errors):
    where = "runs[%d]" % index
    if not isinstance(run, dict):
        errors.append("%s: not a mapping" % where)
        return None
    rid = run.get("id")
    if not isinstance(rid, str) or not RUN_ID.match(rid):
        errors.append("%s: id must be a lowercase slug, got %r" % (where, rid))
        return None
    where = "run %s" % rid
    extra = set(run) - RUN_KEYS
    if extra:
        errors.append("%s: unknown keys %s" % (where, ", ".join(sorted(extra))))
    job = run.get("job")
    if not isinstance(job, str) or not job:
        errors.append("%s: job is required" % where)
        return None
    tier = run.get("tier")
    if tier not in tiers:
        errors.append("%s: tier must be one of %s, got %r" % (where, ", ".join(tiers), tier))
        return None
    out = {
        "id": rid, "job": job, "tier": tier,
        "args": run.get("args", []), "merge_args": run.get("merge_args"),
        "stage": run.get("stage", "main"), "placement": run.get("placement"), "class": run.get("class"),
        "triggers": run.get("triggers"), "filters": run.get("filters"), "budget_s": run.get("budget_s"),
        "local": run.get("local", True), "where": run.get("where", "worktree"), "oracle": run.get("oracle", False),
        "oracle_files": run.get("oracle_files", False), "needs": run.get("needs", []), "schedule": run.get("schedule"), "pending": run.get("pending"),
        "disabled": run.get("disabled"), "blocking": run.get("blocking", True), "require": run.get("require", []),
        "release_only": run.get("release_only", False), "user_step": run.get("user_step"),
    }
    if not is_str_list(out["args"]) and out["args"] != []:
        errors.append("%s: args must be a list of strings" % where)
    out["artifacts"] = check_placeholders(where, out["args"] if is_str_list(out["args"]) else [], errors)
    if out["stage"] not in STAGES:
        errors.append("%s: stage must be one of %s" % (where, ", ".join(STAGES)))
    elif out["stage"] != "main" and tier != "T0":
        errors.append("%s: only T0 runs have a %s stage" % (where, out["stage"]))
    if out["class"] not in CLASSES:
        errors.append("%s: class must be one of %s, got %r" % (where, ", ".join(CLASSES), out["class"]))
    if not isinstance(out["budget_s"], int) or isinstance(out["budget_s"], bool) or out["budget_s"] <= 0:
        errors.append("%s: budget_s (seconds) must be a positive integer" % where)
    for flag in ("local", "oracle", "oracle_files", "blocking", "release_only"):
        if not isinstance(out[flag], bool):
            errors.append("%s: %s must be true or false" % (where, flag))
    if out["where"] not in WHERE:
        errors.append("%s: where must be %s" % (where, ", ".join(WHERE)))
    if out["where"] == "main":
        if not is_str_list(out["merge_args"]) and out["merge_args"] != []:
            errors.append("%s: a where: main run needs merge_args (the main checkout's arguments against the branch's tree)" % where)
        else:
            check_placeholders(where, out["merge_args"], errors, artifacts=False)
    elif out["merge_args"] is not None:
        errors.append("%s: merge_args only belong to where: main runs" % where)
    if out["where"] in ("main", "tip") and out["local"]:
        errors.append("%s: a where: %s run cannot run in gate --local (set local: false)" % (where, out["where"]))
    if out["oracle"] and out["local"]:
        errors.append("%s: an oracle-image run cannot run in gate --local (set local: false)" % where)
    if not is_str_list(out["needs"]) and out["needs"] != []:
        errors.append("%s: needs must be a list" % where)
    else:
        for n in out["needs"]:
            if n not in NEEDS:
                errors.append("%s: unknown need %r (one of %s)" % (where, n, ", ".join(NEEDS)))
    # Placement (R1952): a docker job's placement is its service; a host job
    # runs on the host, drives the test VM, drives containers, or several.
    placement = out["placement"]
    if isinstance(placement, str):
        placement = [placement]
    out["placement"] = placement
    if not is_str_list(placement) or not placement or not all(PLACEMENT.match(p) for p in placement):
        errors.append("%s: placement must be host, vm, docker:<service> or a list of them, got %r" % (where, placement))
    else:
        registered = registry.get(job)
        if registered and registered.startswith("docker:") and placement != [registered]:
            errors.append("%s: job %s runs in %s, not %s" % (where, job, registered, ", ".join(placement)))
        in_docker = any(p.startswith("docker:") for p in placement)
        on_host = any(not p.startswith("docker:") for p in placement)
        if any(n in DOCKER_NEEDS for n in out["needs"]) and not in_docker:
            errors.append("%s: needs %s, which run in Docker (R1952); placement is %s"
                          % (where, ", ".join(n for n in out["needs"] if n in DOCKER_NEEDS), ", ".join(placement)))
        if any(n in HOST_NEEDS for n in out["needs"]) and not on_host:
            errors.append("%s: needs %s, which run on the host or the test VM (R1952); placement is %s"
                          % (where, ", ".join(n for n in out["needs"] if n in HOST_NEEDS), ", ".join(placement)))
        if out["oracle"] and "oracle" not in out["needs"]:
            errors.append("%s: an oracle-image run lists oracle under needs" % where)
    # Selection
    if out["triggers"] is not None and out["filters"] is not None:
        errors.append("%s: give triggers or filters, not both" % where)
    if out["triggers"] is not None and not is_str_list(out["triggers"]):
        errors.append("%s: triggers must be a list of path globs" % where)
    if out["filters"] is not None:
        if not isinstance(out["filters"], dict) or not out["filters"]:
            errors.append("%s: filters must map path globs to conformance filters" % where)
        else:
            for glob, values in out["filters"].items():
                if not isinstance(glob, str) or not is_str_list(values):
                    errors.append("%s: filters[%r] must be a list of conformance filters" % (where, glob))
                    continue
                for v in values:
                    if v != "*" and conformance and not scenario_exists(conformance, v):
                        errors.append("%s: filter %r names no scenario or area under conformance/scenarios" % (where, v))
    if tier in ("T2", "T3", "T4") and (out["triggers"] is not None or out["filters"] is not None):
        errors.append("%s: %s runs are scheduled, not triggered by paths" % (where, tier))
    if tier == "T2":
        if out["schedule"] not in SCHEDULES:
            errors.append("%s: a T2 run needs schedule: nightly or weekly" % where)
    elif out["schedule"] is not None:
        errors.append("%s: only T2 runs have a schedule" % where)
    if out["release_only"] and tier != "T3":
        errors.append("%s: release_only belongs to T3 runs" % where)
    if out["user_step"] is not None and (tier != "T4" or not isinstance(out["user_step"], str)):
        errors.append("%s: user_step (a user-performed run) belongs to T4 runs" % where)
    if tier == "T4" and out["user_step"] is None and out["pending"] is None:
        errors.append("%s: a T4 run is user-performed: give user_step or pending" % where)
    # Wiring state
    if out["pending"] is not None and out["disabled"] is not None:
        errors.append("%s: pending (job not landed) and disabled (job landed, run not wired) exclude each other" % where)
    if out["pending"] is not None:
        if not isinstance(out["pending"], str) or not out["pending"]:
            errors.append("%s: pending names the unit or condition that lands the job" % where)
        elif job in registry:
            errors.append("%s: job %s exists now; remove `pending: %s` and wire the run" % (where, job, out["pending"]))
    else:
        # A user step is Antoine's (U256): its job names the procedure, which
        # need not be a tools/mg job.
        if job not in registry and out["user_step"] is None:
            errors.append("%s: job %s is not a tools/mg job (tools/mg --list); mark the run pending until it lands" % (where, job))
        if out["disabled"] is not None and (not isinstance(out["disabled"], str) or not out["disabled"]):
            errors.append("%s: disabled names why the run is not wired yet" % where)
    if not isinstance(out["require"], list):
        errors.append("%s: require must be a list" % where)
    else:
        for r in out["require"]:
            if not isinstance(r, dict) or not isinstance(r.get("id"), str):
                errors.append("%s: each require entry needs an id" % where)
            elif "summary" in r:
                if not isinstance(r.get("max"), int) or set(r) - {"id", "summary", "max", "release"}:
                    errors.append("%s: require %s: {id, summary, max[, release]}" % (where, r["id"]))
            elif "detail" in r:
                if not is_str_list(r.get("only_keys")) or set(r) - {"id", "detail", "only_keys", "release"}:
                    errors.append("%s: require %s: {id, detail, only_keys[, release]}" % (where, r["id"]))
            else:
                errors.append("%s: require %s needs summary or detail" % (where, r["id"]))
    return out


def scenario_exists(conformance, value):
    base = os.path.join(conformance, "scenarios", value.strip("/"))
    return os.path.isdir(base) or any(os.path.isfile(base + s) for s in (".yaml", ".yml", ".json"))


def load_tiers(data, registry, conformance):
    errors = []
    if not isinstance(data, dict):
        raise Invalid("tiers.yaml: the top level is not a mapping")
    extra = set(data) - {"version", "tiers", "runs", "utilities"}
    if extra:
        errors.append("tiers.yaml: unknown keys %s" % ", ".join(sorted(extra)))
    if data.get("version") != 1:
        errors.append("tiers.yaml: version must be 1")
    tiers = data.get("tiers")
    if not isinstance(tiers, dict) or list(tiers) != list(TIERS):
        errors.append("tiers.yaml: tiers must describe %s, in that order" % ", ".join(TIERS))
        tiers = {t: {} for t in TIERS}
    runs = []
    seen = set()
    for i, run in enumerate(data.get("runs") or []):
        out = check_run(run, i, TIERS, registry, conformance, errors)
        if out is None:
            continue
        if out["id"] in seen:
            errors.append("run %s: duplicate id" % out["id"])
        seen.add(out["id"])
        runs.append(out)
    # An {artifact:...} names a run that starts before this one (plan order:
    # stage, then tier, then this file's order).
    order = {r["id"]: (STAGE_ORDER.get(r["stage"], 0), TIER_ORDER[r["tier"]], i) for i, r in enumerate(runs)}
    for r in runs:
        for ref, name in r["artifacts"]:
            if ref not in order or order[ref] >= order[r["id"]]:
                errors.append("run %s: {artifact:%s:%s} names run %s, which does not run before it"
                              % (r["id"], ref, name, ref))
    utilities = {}
    for i, u in enumerate(data.get("utilities") or []):
        if not isinstance(u, dict) or set(u) != {"job", "why"} or not all(isinstance(u[k], str) and u[k] for k in u):
            errors.append("utilities[%d]: {job, why}" % i)
            continue
        if u["job"] not in registry:
            errors.append("utility %s: not a tools/mg job" % u["job"])
        if u["job"] in utilities:
            errors.append("utility %s: listed twice" % u["job"])
        utilities[u["job"]] = u["why"]
    # Every registered job has a tier or is a utility (U19 Verification).
    in_runs = {r["job"] for r in runs}
    for job in sorted(registry):
        if job not in in_runs and job not in utilities:
            errors.append("job %s has no tier: add a run for it, or list it under utilities with why it verifies nothing" % job)
    if errors:
        raise Invalid("\n".join(errors))
    return {"tiers": tiers, "runs": runs, "utilities": utilities}


def as_date(value):
    if isinstance(value, datetime.datetime):
        return None
    if isinstance(value, datetime.date):
        return value.isoformat()
    if isinstance(value, str) and re.match(r"^\d{4}-\d{2}-\d{2}$", value):
        try:
            datetime.date.fromisoformat(value)
            return value
        except ValueError:
            return None
    return None


def check_quarantine_entry(entry, index, runs):
    """(normalized entry, [errors])."""
    errors = []
    if not isinstance(entry, dict):
        return None, ["entries[%d]: not a mapping" % index]
    label = "entries[%d]" % index
    if isinstance(entry.get("run"), str) and isinstance(entry.get("test"), str):
        label = "%s:%s" % (entry["run"], entry["test"])
    extra = set(entry) - QUARANTINE_KEYS
    if extra:
        errors.append("%s: unknown keys %s" % (label, ", ".join(sorted(extra))))
    for key in ("test", "run", "owner", "reason"):
        if not isinstance(entry.get(key), str) or not entry[key].strip():
            errors.append("%s: %s is required" % (label, key))
    run = next((r for r in runs if r["id"] == entry.get("run")), None)
    if isinstance(entry.get("run"), str) and run is None:
        errors.append("%s: run %s is not in tiers.yaml" % (label, entry["run"]))
    cls = entry.get("class")
    if cls not in CLASSES:
        errors.append("%s: class must be one of %s" % (label, ", ".join(CLASSES)))
    elif run is not None and run["class"] in PROTECTED and cls != run["class"]:
        errors.append("%s: the tests of run %s are %s tests; class %s would hide that from the milestone rule"
                      % (label, run["id"], run["class"], cls))
    deadline = as_date(entry.get("deadline"))
    if deadline is None:
        errors.append("%s: deadline must be a date (YYYY-MM-DD)" % label)
    if entry.get("approved_by") != "antoine":
        errors.append("%s: approved_by: antoine is required, written only from Antoine's chat approval (R1953)" % label)
    ev = entry.get("evidence")
    evidence = None
    if not isinstance(ev, dict) or set(ev) != EVIDENCE_KEYS:
        errors.append("%s: evidence {commit, failed, passed} is required: two gate runs on one commit, the test failing in one and passing in the other" % label)
    else:
        if not isinstance(ev["commit"], str) or not SHA.match(ev["commit"]):
            errors.append("%s: evidence.commit must be a full commit id" % label)
        for k in ("failed", "passed"):
            if not isinstance(ev[k], str) or not GATE_RUN.match(ev[k]):
                errors.append("%s: evidence.%s must be a gate run id" % (label, k))
        if ev.get("failed") == ev.get("passed"):
            errors.append("%s: evidence.failed and evidence.passed must be two gate runs" % label)
        evidence = {k: ev[k] for k in EVIDENCE_KEYS}
    if entry.get("escalation") is not None and not isinstance(entry.get("escalation"), str):
        errors.append("%s: escalation must be an escalation id" % label)
    out = {
        "test": entry.get("test"), "run": entry.get("run"), "class": cls, "owner": entry.get("owner"),
        "reason": entry.get("reason"), "deadline": deadline, "approved_by": entry.get("approved_by"),
        "evidence": evidence, "escalation": entry.get("escalation"), "label": label,
    }
    return out, errors


def load_quarantine(data, runs):
    if data is None:
        data = {}
    if not isinstance(data, dict) or set(data) - {"entries"}:
        return {"entries": [], "invalid": [{"label": "quarantine.yaml", "errors": ["the top level must be a mapping with `entries`"]}]}
    entries, invalid, seen = [], [], set()
    for i, entry in enumerate(data.get("entries") or []):
        out, errors = check_quarantine_entry(entry, i, runs)
        if out is not None and (out["run"], out["test"]) in seen:
            errors.append("%s: listed twice" % out["label"])
        if out is not None:
            seen.add((out["run"], out["test"]))
        if errors:
            invalid.append({"label": out["label"] if out else "entries[%d]" % i, "errors": errors})
        else:
            entries.append(out)
    return {"entries": entries, "invalid": invalid}


def cmd_load(a):
    registry = read_registry(a.registry)
    try:
        tiers = load_tiers(read_yaml(a.tiers), registry, a.conformance)
    except Invalid as e:
        json.dump({"errors": str(e).split("\n")}, sys.stdout)
        print()
        return 1
    try:
        qdata = read_yaml(a.quarantine) if os.path.exists(a.quarantine) else {}
        quarantine = load_quarantine(qdata, tiers["runs"])
    except Invalid as e:
        quarantine = {"entries": [], "invalid": [{"label": "quarantine.yaml", "errors": [str(e)]}]}
    config = dict(tiers)
    config["quarantine"] = quarantine
    config["registry"] = registry
    write_json(a.out, config)
    return 0


# ---------------------------------------------------------------------------
# plan

STAGE_ORDER = {s: i for i, s in enumerate(STAGES)}
TIER_ORDER = {t: i for i, t in enumerate(TIERS)}


def substitute(args, values):
    out = []
    for a in args:
        def repl(m):
            v = values.get(m.group(1))
            if v is None:
                raise Invalid("argument %r needs {%s}, which this run does not set" % (a, m.group(1)))
            return v
        out.append(PLACEHOLDER.sub(repl, a))
    return out


def plan_runs(config, changed, mode, main_capable, values, all_paths=False, only=None, oracle_tree="own"):
    """Every run of the modes' tiers, in execution order, with action run or
    skip and the reason."""
    if mode in ("local", "full"):
        tiers = ("T0", "T1")
    elif mode in ("nightly", "weekly"):
        tiers = ("T2",)
    else:  # milestone, release: every per-change run in full, then T3 and T4
        tiers = ("T0", "T1", "T3", "T4")
        all_paths = True
    items = []
    for run in config["runs"]:
        if run["tier"] not in tiers:
            continue
        if run["tier"] == "T2" and mode == "nightly" and run["schedule"] != "nightly":
            continue
        if run["release_only"] and mode != "release":
            continue
        item = {k: run[k] for k in ("id", "job", "tier", "stage", "class", "where", "oracle", "blocking",
                                    "budget_s", "placement", "require", "user_step")}
        item["args"] = list(run["args"])
        item["merge_args"] = list(run["merge_args"]) if run["merge_args"] is not None else None
        item["action"], item["outcome"], item["reason"] = "run", None, ""
        if run["filters"] is not None:
            selected = set()
            for glob, values_ in run["filters"].items():
                m = matcher([glob])
                if all_paths or any(m(p) for p in changed):
                    selected.update(values_)
            if not selected:
                item["action"], item["outcome"] = "skip", "not-triggered"
                item["reason"] = "no changed path selects a scenario area"
            elif "*" not in selected:
                item["args"] += sorted(selected)
                item["reason"] = "areas: " + ", ".join(sorted(selected))
            else:
                item["reason"] = "every area"
        elif run["triggers"] is not None and not all_paths:
            m = matcher(run["triggers"])
            hits = [p for p in changed if m(p)]
            if not hits:
                item["action"], item["outcome"] = "skip", "not-triggered"
                item["reason"] = "no changed path matches its triggers"
            else:
                item["reason"] = "%s changed" % hits[0] + (" and %d more" % (len(hits) - 1) if len(hits) > 1 else "")
        if only is not None and run["id"] not in only:
            item["action"], item["outcome"], item["reason"] = "skip", "not-triggered", "not in --only"
        if item["action"] == "run":
            if run["pending"] is not None:
                item["action"], item["outcome"], item["reason"] = "skip", "pending", "job lands with %s" % run["pending"]
            elif run["disabled"] is not None:
                item["action"], item["outcome"], item["reason"] = "skip", "disabled", run["disabled"]
            elif run["user_step"] is not None:
                item["action"], item["outcome"], item["reason"] = "skip", "user-step", "user-performed: %s" % run["user_step"]
            elif mode == "local" and not run["local"]:
                item["action"], item["outcome"] = "skip", "skipped-local"
                item["reason"] = ("the similarity scan and oracle-image jobs need refs/ or oracle/, which an "
                                  "implementer worktree lacks; tools/mg merge runs them")
            elif run["where"] == "main" and not main_capable:
                item["action"], item["outcome"] = "skip", "deferred-main"
                item["reason"] = "runs from the main checkout (tools/mg merge)"
            elif run["where"] == "tip" and not main_capable:
                item["action"], item["outcome"] = "skip", "deferred-tip"
                item["reason"] = ("needs the branch's own code with the main checkout's corpus; "
                                  "tools/mg merge runs it in a full checkout of the tip")
            elif (run["oracle"] or run["oracle_files"]) and oracle_tree == "differs":
                # An oracle_files run would compare the branch with another
                # oracle/ (the main checkout's lock, its harvest inventory) and
                # fail as an ordinary run, which no merge replaces.
                item["action"], item["outcome"] = "skip", "deferred-tip"
                item["reason"] = ("this checkout has no oracle/ and the main checkout's is not this branch's; "
                                  "tools/mg merge runs it in a full checkout of the tip")
        try:
            item["args"] = substitute(item["args"], values)
            if item["merge_args"] is not None:
                item["merge_args"] = substitute(item["merge_args"], values)
        except Invalid as e:
            # Only a run that would run needs its values.
            if item["action"] == "run" or item["outcome"] in ("deferred-main", "deferred-tip"):
                item["action"], item["outcome"], item["reason"] = "skip", "unresolved", str(e)
                item["blocking"] = True
        items.append(item)
    items.sort(key=lambda i: (STAGE_ORDER[i["stage"]], TIER_ORDER[i["tier"]]))
    return items


def read_nul_list(path):
    if not path:
        return []
    with open(path, "rb") as f:
        data = f.read()
    return sorted({p.decode("utf-8", "surrogateescape") for p in data.split(b"\0") if p})


def cmd_plan(a):
    config = read_json(a.config)
    changed = read_nul_list(a.changed)
    values = {}
    for v in a.var or []:
        k, _, val = v.partition("=")
        if val:
            values[k] = val
    # --only "" selects no run (a milestone with none); without --only, all.
    only = {x for x in a.only.split(",") if x} if a.only is not None else None
    if only:
        unknown = only - {r["id"] for r in config["runs"]}
        if unknown:
            json.dump({"errors": ["unknown run id(s) for --only: %s" % ", ".join(sorted(unknown))]}, sys.stdout)
            print()
            return 1
    items = plan_runs(config, changed, a.mode, a.main_capable == "yes", values, a.all, only, a.oracle_tree)
    write_json(a.out, {"mode": a.mode, "values": values, "changed": changed, "runs": items})
    return 0


# ---------------------------------------------------------------------------
# evaluate


def today(now):
    return datetime.datetime.strptime(now, "%Y-%m-%dT%H:%M:%SZ").date().isoformat()


def get_path(obj, dotted):
    for part in dotted.split("."):
        if not isinstance(obj, dict) or part not in obj:
            return None
        obj = obj[part]
    return obj


def assess(item, entry, quarantine, day):
    """A run record from one execution: entry = {result, exit_code, outcome}."""
    rec = {"id": item["id"], "job": item["job"], "tier": item["tier"], "stage": item["stage"],
           "class": item["class"], "args": item["args"], "blocking": item["blocking"], "budget_s": item["budget_s"],
           "result": entry.get("result"), "status": None, "outcome": None, "message": "", "failures": [],
           "quarantined": [], "duration_ms": None, "over_budget": False, "repro": None, "queued": False,
           "user_steps": [], "queued_parts": []}
    if entry.get("outcome"):
        rec["outcome"] = entry["outcome"]
        rec["message"] = entry.get("message", "")
        return rec
    path = entry.get("result")
    try:
        res = read_json(path)
    except (OSError, ValueError, TypeError):
        rec["outcome"] = "error"
        rec["message"] = "tools/mg %s wrote no readable result (exit %s)" % (item["job"], entry.get("exit_code"))
        return rec
    rec["status"] = res.get("status")
    rec["message"] = res.get("message", "")
    rec["duration_ms"] = res.get("duration_ms")
    rec["failures"] = [{"id": f.get("id"), "message": (f.get("message") or "")[:500], "repro": f.get("repro")}
                       for f in res.get("failures") or []]
    rec["repro"] = "tools/mg %s%s" % (item["job"], "".join(" " + shell_quote(x) for x in item["args"]))
    if isinstance(rec["duration_ms"], int) and rec["duration_ms"] > item["budget_s"] * 1000:
        rec["over_budget"] = True
    details = res.get("details") or {}
    status = rec["status"]
    ids = [f["id"] for f in rec["failures"]]
    if status == "pass":
        rec["outcome"] = "pass"
    elif status == "warn":
        rec["queued"] = details.get("queued") is True
        rec["outcome"] = "queued" if rec["queued"] else "warn"
        if rec["queued"]:
            rec["queued_parts"], rec["user_steps"] = queued_for(res)
    elif any(i in ORACLE_TREE_FAILURES for i in ids):
        # Whatever else failed ran against another oracle/ than the branch's.
        rec["outcome"] = "needs-oracle-tree"
    elif status == "fail":
        active = {e["test"]: e for e in quarantine if e["run"] == item["id"] and e["deadline"] >= day}
        if ids and all(i in active for i in ids):
            rec["outcome"] = "quarantined"
            rec["quarantined"] = ids
        else:
            rec["outcome"] = "fail"
            rec["quarantined"] = [i for i in ids if i in active]
    else:
        rec["outcome"] = "error"
    # Pass rules on the result itself (R1880's harvest rules and the like).
    if rec["outcome"] in ("pass", "warn"):
        for r in item.get("require") or []:
            if r.get("release") and not entry.get("release"):
                continue
            if "summary" in r:
                value = (res.get("summary") or {}).get(r["summary"])
                if not isinstance(value, int) or value > r["max"]:
                    rec["outcome"] = "fail"
                    rec["failures"].append({"id": r["id"], "message": "%s is %s; at most %d is allowed"
                                            % (r["summary"], value, r["max"]), "repro": rec["repro"]})
            else:
                value = get_path(details, r["detail"])
                if value is None:
                    # No rows of that kind: nothing can be outside only_keys.
                    continue
                if not isinstance(value, dict):
                    bad = "%s is not a table of counts" % r["detail"]
                else:
                    bad = ", ".join("%s %s" % (n, k) for k, n in sorted(value.items()) if n and k not in r["only_keys"])
                if bad:
                    rec["outcome"] = "fail"
                    rec["failures"].append({"id": r["id"], "message": "%s: %s; only %s allowed"
                                            % (r["detail"], bad, ", ".join(r["only_keys"])), "repro": rec["repro"]})
    return rec


def queued_for(res):
    """(parts, user steps) of a queued result: details.queued_parts
    ({user_step, part, reason[, repro]}), and the steps they name; a result
    without parts (from before U19b) names its missing user steps only."""
    details = res.get("details") or {}
    parts = [p for p in details.get("queued_parts") or [] if isinstance(p, dict) and p.get("user_step")]
    steps = []
    for p in parts:
        if p["user_step"] not in steps:
            steps.append(p["user_step"])
    if not steps:
        steps = [u.get("id") for u in res.get("user_steps") or []
                 if isinstance(u, dict) and u.get("state") == "missing" and u.get("id")]
    return parts, steps


def shell_quote(s):
    if s and re.match(r"^[A-Za-z0-9_./:=@%+,-]+$", s):
        return s
    return "'" + s.replace("'", "'\\''") + "'"


def cmd_evaluate(a):
    config = read_json(a.config)
    plan = read_json(a.plan)
    day = today(a.now)
    quarantine = config["quarantine"]["entries"]
    records = {}
    # Receipt-level blockers: what the gate found beside its runs (a job that
    # changed the tested tree, a worktree a merge found changed). A merge
    # carries its worktree receipt's, so none is lost on re-evaluation.
    receipt_blocking = []
    # A merge's worktree receipt first, then this run's executions, which
    # replace the runs it records.
    if a.receipt:
        receipt = read_json(a.receipt)
        if receipt.get("receipt_version") != RECEIPT_VERSION:
            raise Invalid("%s: receipt_version %r; this gate reads version %d only"
                          % (a.receipt, receipt.get("receipt_version"), RECEIPT_VERSION))
        for rec in receipt.get("runs", []):
            records[rec["id"]] = rec
        receipt_blocking += receipt.get("receipt_blocking", [])
    for raw in a.blocker or []:
        try:
            b = json.loads(raw)
        except ValueError:
            b = None
        if not isinstance(b, dict) or not all(isinstance(b.get(k), str) and b[k] for k in ("id", "message")):
            raise Invalid("--blocker takes a JSON object with id and message, got %r" % raw)
        receipt_blocking.append({"id": b["id"], "message": b["message"], "repro": b.get("repro") or "git status"})
    entries = []
    if a.results:
        with open(a.results, encoding="utf-8") as f:
            entries = [json.loads(line) for line in f if line.strip()]
    items = {i["id"]: i for i in plan["runs"]}
    for entry in entries:
        item = items.get(entry["id"])
        if item is None:
            continue
        entry = dict(entry, release=(plan["mode"] == "release"))
        records[entry["id"]] = assess(item, entry, quarantine, day)
    runs = []
    for item in plan["runs"]:
        rec = records.get(item["id"])
        if rec is None:
            rec = {"id": item["id"], "job": item["job"], "tier": item["tier"], "stage": item["stage"],
                   "class": item["class"], "args": item["args"], "blocking": item["blocking"],
                   "budget_s": item["budget_s"], "result": None, "status": None,
                   "outcome": item["outcome"] or "not-run", "message": item["reason"], "failures": [],
                   "quarantined": [], "duration_ms": None, "over_budget": False, "repro": None, "queued": False,
                   "user_steps": [], "queued_parts": []}
            if item["outcome"] == "deferred-main":
                rec["merge_args"] = item["merge_args"]
        runs.append(rec)
    blocking = []
    for rec in runs:
        if rec["outcome"] in NONBLOCKING + INCOMPLETE + CONSEQUENCES:
            continue
        if not rec["blocking"] and rec["outcome"] in ("fail", "warn"):
            continue
        first = rec["failures"][0]["message"] if rec["failures"] else rec["message"]
        reason = {
            "needs-oracle-tree": "the oracle/ it used is not this branch's (%s); tools/mg merge runs it in a full checkout of the tip" % first,
            "not-run": "planned but not run",
        }.get(rec["outcome"], first)
        blocking.append({"id": "run:" + rec["id"], "message": "%s (%s): %s" % (rec["id"], rec["outcome"], reason),
                         "repro": rec.get("repro") or "tools/mg %s" % rec["job"]})
        for f in rec["failures"]:
            if f["id"] in rec["quarantined"]:
                continue
            blocking.append({"id": "%s:%s" % (rec["job"], f["id"]), "message": f["message"],
                             "repro": f.get("repro") or rec.get("repro") or "tools/mg %s" % rec["job"]})
    q = config["quarantine"]
    expired = [e for e in q["entries"] if e["deadline"] < day]
    for e in expired:
        blocking.append({"id": "quarantine-expired:%s:%s" % (e["run"], e["test"]),
                         "message": "the quarantine of %s:%s (owner %s) ended on %s: fix the test or remove it (R1953)"
                         % (e["run"], e["test"], e["owner"], e["deadline"]), "repro": "tools/mg quarantine check"})
    for inv in q["invalid"]:
        blocking.append({"id": "quarantine-invalid:%s" % inv["label"], "message": "; ".join(inv["errors"]),
                         "repro": "tools/mg quarantine check"})
    protected = []
    if plan["mode"] in ("milestone", "release"):
        protected = [e for e in q["entries"] if e["class"] in PROTECTED]
        for e in protected:
            blocking.append({"id": "quarantine-protected:%s:%s" % (e["run"], e["test"]),
                             "message": "%s:%s is a quarantined %s test; a milestone gate fails while any conformance, sync or data-safety test is quarantined (R1953)"
                             % (e["run"], e["test"], e["class"]), "repro": "tools/mg quarantine check"})
    blocking += receipt_blocking
    incomplete = [r["id"] for r in runs if r["outcome"] in INCOMPLETE + CONSEQUENCES]
    receipt = {
        "receipt_version": RECEIPT_VERSION,
        "mode": plan["mode"],
        "commit": a.commit or None,
        "dirty": a.dirty == "true",
        "base": plan["values"].get("base"),
        "values": plan["values"],
        "evaluated_at": a.now,
        "changed_paths": len(plan["changed"]),
        "runs": runs,
        "quarantine": {
            "active": [dict(e, failed_here=any(e["test"] in r["quarantined"] for r in runs if r["id"] == e["run"]))
                       for e in q["entries"] if e["deadline"] >= day],
            "expired": expired, "invalid": q["invalid"], "protected": protected,
        },
        "receipt_blocking": receipt_blocking,
        "blocking": blocking,
        "incomplete": incomplete,
        # Runs the machine's state kept from testing: non-blocking, and what
        # tools/mg merge queues for tools/mg catch-up.
        "queued": [{"id": r["id"], "job": r["job"], "args": r["args"], "result": r["result"],
                    "user_steps": r.get("user_steps") or [], "parts": r.get("queued_parts") or []}
                   for r in runs if r["outcome"] == "queued"],
        "pass": not blocking,
        "merge_ready": (not blocking and not incomplete and a.dirty != "true" and plan["mode"] == "full"),
    }
    write_json(a.out, receipt)
    return 0


# ---------------------------------------------------------------------------
# quarantine-entry: the evidence check of `tools/mg quarantine add`


def yaml_scalar(value):
    # A JSON string is a valid YAML double-quoted scalar.
    return json.dumps(value, ensure_ascii=False)


def cmd_quarantine_entry(a):
    config = read_json(a.config)
    errors = []
    receipts = {}
    for k in ("failed", "passed"):
        try:
            receipts[k] = read_json(getattr(a, k + "_receipt"))
        except (OSError, ValueError) as e:
            errors.append("cannot read the %s receipt: %s" % (k, e))
    if not errors:
        f, p = receipts["failed"], receipts["passed"]
        if f.get("commit") != p.get("commit") or not f.get("commit"):
            errors.append("the two receipts are for different commits (%s, %s); the evidence is one commit failing and passing"
                          % (f.get("commit"), p.get("commit")))
        if f.get("dirty") or p.get("dirty"):
            errors.append("a receipt from a dirty tree is no evidence: its run did not test a commit")

        def run_of(receipt):
            return next((r for r in receipt.get("runs", []) if r.get("id") == a.run and r.get("result")), None)

        failing, passing = run_of(f), run_of(p)
        if failing is None or not any(x.get("id") == a.test for x in failing.get("failures", [])):
            errors.append("the failed receipt has no %s run in which %s failed" % (a.run, a.test))
        if passing is None or passing.get("status") != "pass":
            errors.append("the passed receipt has no %s run that passed" % a.run)
    entry = {
        "test": a.test, "run": a.run, "class": a.cls, "owner": a.owner, "reason": a.reason,
        "deadline": a.deadline, "approved_by": a.approved_by,
        "evidence": {"commit": receipts.get("failed", {}).get("commit"), "failed": a.failed_run, "passed": a.passed_run},
    }
    if a.escalation:
        entry["escalation"] = a.escalation
    _, entry_errors = check_quarantine_entry(entry, 0, config["runs"])
    errors += entry_errors
    if as_date(a.deadline) and as_date(a.deadline) < today(a.now):
        errors.append("the deadline %s has passed" % a.deadline)
    if any(e["run"] == a.run and e["test"] == a.test for e in config["quarantine"]["entries"]):
        errors.append("%s:%s is already quarantined" % (a.run, a.test))
    if errors:
        json.dump({"errors": errors}, sys.stdout)
        print()
        return 1
    lines = ["  - test: " + yaml_scalar(a.test), "    run: " + yaml_scalar(a.run), "    class: " + yaml_scalar(a.cls),
             "    owner: " + yaml_scalar(a.owner), "    reason: " + yaml_scalar(a.reason),
             "    deadline: " + yaml_scalar(a.deadline), "    approved_by: antoine"]
    if a.escalation:
        lines.append("    escalation: " + yaml_scalar(a.escalation))
    lines += ["    evidence:", "      commit: " + yaml_scalar(entry["evidence"]["commit"]),
              "      failed: " + yaml_scalar(a.failed_run), "      passed: " + yaml_scalar(a.passed_run)]
    with open(a.out, "w", encoding="utf-8") as out:
        out.write("\n".join(lines) + "\n")
    return 0


# ---------------------------------------------------------------------------


def read_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def write_json(path, value):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(value, f, indent=1, sort_keys=False)
        f.write("\n")
    os.replace(tmp, path)


def main(argv):
    p = argparse.ArgumentParser(prog="gate.py")
    sub = p.add_subparsers(dest="cmd")
    s = sub.add_parser("load")
    s.add_argument("--tiers", required=True)
    s.add_argument("--quarantine", required=True)
    s.add_argument("--registry", required=True)
    s.add_argument("--conformance", default="")
    s.add_argument("--out", required=True)
    s = sub.add_parser("plan")
    s.add_argument("--config", required=True)
    s.add_argument("--changed", default="")
    s.add_argument("--mode", required=True, choices=MODES)
    s.add_argument("--main-capable", choices=("yes", "no"), default="no")
    s.add_argument("--oracle-tree", choices=ORACLE_TREES, default="own")
    s.add_argument("--var", action="append")
    s.add_argument("--all", action="store_true")
    s.add_argument("--only", default=None)
    s.add_argument("--out", required=True)
    s = sub.add_parser("evaluate")
    s.add_argument("--config", required=True)
    s.add_argument("--plan", required=True)
    s.add_argument("--results", default="")
    s.add_argument("--receipt", default="")
    s.add_argument("--blocker", action="append")
    s.add_argument("--now", required=True)
    s.add_argument("--commit", default="")
    s.add_argument("--dirty", default="false")
    s.add_argument("--out", required=True)
    s = sub.add_parser("quarantine-entry")
    s.add_argument("--config", required=True)
    s.add_argument("--run", required=True)
    s.add_argument("--test", required=True)
    s.add_argument("--class", dest="cls", required=True)
    s.add_argument("--owner", required=True)
    s.add_argument("--reason", required=True)
    s.add_argument("--deadline", required=True)
    s.add_argument("--approved-by", default="")
    s.add_argument("--escalation", default="")
    s.add_argument("--failed-run", required=True)
    s.add_argument("--passed-run", required=True)
    s.add_argument("--failed-receipt", required=True)
    s.add_argument("--passed-receipt", required=True)
    s.add_argument("--now", required=True)
    s.add_argument("--out", required=True)
    a = p.parse_args(argv)
    try:
        if a.cmd == "load":
            return cmd_load(a)
        if a.cmd == "plan":
            return cmd_plan(a)
        if a.cmd == "evaluate":
            return cmd_evaluate(a)
        if a.cmd == "quarantine-entry":
            return cmd_quarantine_entry(a)
    except Invalid as e:
        json.dump({"errors": str(e).split("\n")}, sys.stdout)
        print()
        return 1
    p.print_usage(sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
