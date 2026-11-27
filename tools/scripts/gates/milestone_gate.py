#!/usr/bin/env python3
"""Milestone gate evaluator (U240; docs/process/milestones.yaml, tiers.md).

tools/mg milestone-gate calls this; tools/mg milestone (U19) runs the suites
whose results it reads. Commands:

  load      milestones.yaml -> JSON, including exact first-sync historical
            definitions with --root. The only command that reads YAML
            (PyYAML: the dev container, or a python3 that has it).
  check     the manifest against the tiers config (`gate.py load` output) and
            the plan's Delivery Milestones table and Unit Index.
  evaluate  one milestone's gate: suite results (build/mg/<job>/<run>/
            result.json), git history, the escalation ledger, the quarantine
            and the user-step evidence at HEAD -> an evaluation, and on a pass
            the milestone record.
  runs      the tiers runs `tools/mg milestone <M>` runs: what M's gate
            reads and the runs those take files from, nothing of a later
            milestone (milestone_runs).

Everything but `load` is stdlib only, for macOS's /usr/bin/python3 (3.9).

The milestones and their order are the manifest's `milestones` list (which
check keeps equal to the plan's table); nothing here knows their names.

Exit status: load and check: 0 valid, 1 invalid (errors as JSON on stdout),
2 usage. evaluate: 0 pass, 1 fail, 3 awaiting_user, 2 an invalid manifest or
input, an unknown milestone (errors as JSON on stdout) or usage. runs: 0, or
2 as evaluate.
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import stat
import subprocess
import sys

# A milestone id: a word of the plan's table (its row starts **<id> <name>**).
# `release` is tools/mg milestone's release run, never a milestone.
MILESTONE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
NOT_A_MILESTONE = ("release",)
UNIT = re.compile(r"^U[0-9]+[a-z]?$")
SLUG = re.compile(r"^[a-z0-9][a-z0-9.-]*$")
RID = re.compile(r"^R[0-9]{3,4}$")
SHA = re.compile(r"^[0-9a-f]{40}$")
DATE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$")
DEVICES = ("iphone", "ipad", "mac")
# The tiers `tools/mg milestone` runs: a suite must be one of their runs.
SUITE_TIERS = ("T0", "T1", "T3")
PLACEHOLDER = re.compile(r"\{([a-z]+)\}")
ARTIFACT = re.compile(r"^\{artifact:([a-z][a-z0-9-]*):([A-Za-z0-9._-]+)\}$")
# U19 (R1953): the milestone gates fail while a test of these classes is quarantined.
PROTECTED = ("conformance", "sync", "data-safety")
# The job that evaluates a milestone gate: no milestone's suites may run it
# for that milestone or a later one.
GATE_JOB = "milestone-gate"
GLOBAL_KINDS = ("quarantine", "suite")
# Roles whose commits do not merge a unit: their branches land through it.
NOT_MERGING = ("spec-writer", "oracle-maintainer")
LEDGER = "docs/process/provenance/escalations.jsonl"
RECORDS = "docs/process/milestone-records"
RECORD_VERSION = 1

VALIDATOR_KEYS = {
    "setup": {"step"},
    "device-run": {"device"},
    "variant-pick": {"screens"},
    "trial": {"days"},
    "device-session": {"days"},
    "capture": {"index", "cases", "cases_file"},
    "first-sync": {"results"},
    "report": set(),
}
TIMESTAMP = "%Y-%m-%dT%H:%M:%SZ"

# U241's budget catalog (tools/perf/budgets.yaml, as `perf_gate.py load`
# writes it) and its device-run report contract, which v_device_run checks.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import perf_gate  # noqa: E402  (stdlib only, like this file)

# The fixed list of external dependencies a deferral may name, each with the
# only condition that keeps it deferred and the warn it covers: while an
# approved deferral holds, a result of its cells' run that is warn with
# `covers` is deferred instead of failing.
DEPENDENCIES = {
    "xcode-27.1-foldable-simulator": {
        "why": "Xcode 27.0 has no foldable iPhone simulator; Apple is expected to ship one with Xcode 27.1 "
               "(plan Dependencies / Assumptions)",
        "condition": {"job": "sim-setup", "args": [], "detail": "foldable",
                      "equals": {"status": "unavailable", "reason": "toolchain"}},
        "covers": {"detail": "lane", "equals": {"kind": "foldable", "status": "unavailable", "reason": "toolchain"}},
    },
}

# Plan excerpts: the table text a suite, global condition or user step
# implements (check_coverage).
PLAN_CELLS = {"gate": "closing gate", "user_steps": "user steps"}
# Words that may stay unclaimed between excerpts.
CONNECTIVES = frozenset(("a", "an", "and", "as", "at", "by", "for", "in", "of", "on", "or", "plus", "the", "to",
                         "with"))
UNIT_TOKEN = re.compile(r"(?<![A-Za-z0-9])U[0-9]+[a-z]?(?![A-Za-z0-9])")
UNIT_RANGE = re.compile(r"(?<![A-Za-z0-9])U([0-9]+)[\u2013-]U([0-9]+)(?![A-Za-z0-9])")  # en dash or hyphen
DEVICE_NAMES = {"iphone": "iPhone", "ipad": "iPad", "mac": "Mac"}
# How the plan's table words each kind of user step: (pattern, the cells it
# applies in, the words). Plan text that matches is claimed only by user steps
# of that kind (a trial's "<N>-day" by a trial of N days), and a user step of
# that kind claims text that matches. Text that names no kind is a report.
STEP_WORDING = {
    "trial": (re.compile(r"(?<![0-9])([0-9]+)-day\b|\btrial\b|\bdaily-driver\b", re.I), ("user_steps",),
              '"<N>-day", "trial" or "daily-driver"'),
    "device-run": (re.compile(r"\bdevice runs?\b", re.I), ("user_steps", "gate"), '"device run"'),
    "device-session": (re.compile(r"\bdevice sessions?\b", re.I), ("user_steps",), '"device session"'),
    "variant-pick": (re.compile(r"\bvariant picks?\b|\bsign-off\b", re.I), ("user_steps",),
                     '"variant picks" or "sign-off"'),
    "capture": (re.compile(r"capture\b", re.I), ("user_steps",), '"capture"'),
    "setup": (re.compile(r"\bR1969\b"), ("user_steps",), "R1969 (the one-time setup steps)"),
    "first-sync": (re.compile(r"\bfirst real sync\b", re.I), ("user_steps",), '"first real sync"'),
}
# Wording a step of the key's kind may hold besides its own: the first real
# sync starts the trial it mentions, so its "trial" is not a trial step.
OUTRANKS = {"first-sync": ("trial",)}


class Invalid(Exception):
    pass


def read_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def write_json(path, value):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(value, f, indent=2, ensure_ascii=False)
        f.write("\n")
    os.replace(tmp, path)


def get_path(obj, dotted):
    for part in dotted.split("."):
        if not isinstance(obj, dict) or part not in obj:
            return None
        obj = obj[part]
    return obj


def as_date(value):
    if isinstance(value, str) and DATE.match(value):
        try:
            return datetime.date.fromisoformat(value)
        except ValueError:
            return None
    return None


def as_time(value):
    if isinstance(value, str):
        try:
            return datetime.datetime.strptime(value, TIMESTAMP)
        except ValueError:
            return None
    return None


# ---------------------------------------------------------------------------
# load


def cmd_load(a):
    import yaml  # the dev container has PyYAML; macOS's python3 does not

    class UniqueKeyLoader(yaml.SafeLoader):
        pass

    def construct_mapping(loader, node, deep=False):
        seen = set()
        for key_node, _ in node.value:
            key = loader.construct_object(key_node, deep=deep)
            if key in seen:
                raise Invalid("%s: duplicate key %r (line %d)" % (a.manifest, key, key_node.start_mark.line + 1))
            seen.add(key)
        return yaml.SafeLoader.construct_mapping(loader, node, deep)

    UniqueKeyLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, construct_mapping)

    def plain(value):
        if isinstance(value, (datetime.date, datetime.datetime)):
            return value.isoformat()
        raise Invalid("%s: %r is not a JSON value" % (a.manifest, value))

    try:
        with open(a.manifest, encoding="utf-8") as f:
            data = yaml.load(f, Loader=UniqueKeyLoader)
        if a.root and isinstance(data, dict):
            # Compile only the definitions named by committed first-sync
            # evidence, while the existing load phase has PyYAML available.
            # Evaluation verifies these source bindings using stdlib alone.
            data["_first_sync_definitions"] = _load_first_sync_definitions(
                data, Git(a.root), lambda source: yaml.load(source, Loader=UniqueKeyLoader))
        text = json.dumps(data, default=plain, ensure_ascii=False)
    except (Invalid, OSError) as e:
        print(json.dumps({"errors": [str(e)]}))
        return 1
    except yaml.YAMLError as e:
        print(json.dumps({"errors": ["%s: not valid YAML: %s" % (a.manifest, e)]}))
        return 1
    with open(a.out, "w", encoding="utf-8") as f:
        f.write(text + "\n")
    return 0


def _load_first_sync_definitions(manifest, git, decode):
    head = git.commit("HEAD")
    compiled = {"head": head, "builds": {}}
    milestones = manifest.get("milestones")
    for milestone in milestones if isinstance(milestones, list) else []:
        steps = milestone.get("user_steps") if isinstance(milestone, dict) else None
        for step in steps if isinstance(steps, list) else []:
            validator = step.get("validator") if isinstance(step, dict) else None
            if (not isinstance(validator, dict) or validator.get("type") != "first-sync"
                    or not is_rel_path(step.get("evidence"))):
                continue
            raw = git.show(head, step["evidence"])
            try:
                evidence = json.loads(raw) if raw is not None else {}
                commit = evidence.get("build", {}).get("commit")
            except (ValueError, UnicodeError, AttributeError):
                continue  # the evidence validator reports its own shape error
            if not isinstance(commit, str) or not SHA.fullmatch(commit) or git.commit(commit) != commit:
                continue
            if commit in compiled["builds"]:
                continue
            source = git.show(commit, "tools/gate/tiers.yaml")
            entry = {}
            compiled["builds"][commit] = entry
            if source is None:
                entry["error"] = "historical-definition-missing"
                continue
            entry["sha256"] = hashlib.sha256(source).hexdigest()
            try:
                definition = decode(source)
                if not isinstance(definition, dict) or not isinstance(definition.get("runs"), list):
                    raise Invalid("historical-definition-malformed")
                runs = {}
                selected = {r[0] for r in FIRST_SYNC_RESULTS.values()}
                for run in definition["runs"]:
                    if not isinstance(run, dict) or not isinstance(run.get("id"), str) or run["id"] not in selected:
                        continue
                    if (not SLUG.fullmatch(run["id"]) or run["id"] in runs
                            or not isinstance(run.get("job"), str) or not SLUG.fullmatch(run["job"])
                            or not is_str_list(run.get("args", []), nonempty=False)):
                        raise Invalid("historical-definition-malformed")
                    runs[run["id"]] = {"job": run["job"], "args": run.get("args", []),
                                       "pending": bool(run.get("pending")), "disabled": bool(run.get("disabled"))}
                entry["runs"] = runs
            except Exception:
                # YAML diagnostics and values must not escape this boundary.
                entry["error"] = "historical-definition-malformed"
    return compiled


# ---------------------------------------------------------------------------
# the manifest model


def is_rel_path(value):
    return (isinstance(value, str) and value and not value.startswith("/")
            and ".." not in value.split("/") and "\\" not in value)


def is_str_list(value, nonempty=True):
    return (isinstance(value, list) and (value or not nonempty)
            and all(isinstance(v, str) and v for v in value))


def check_rules(where, rules, errors):
    if rules is None:
        return []
    if not isinstance(rules, list):
        errors.append("%s: require must be a list" % where)
        return []
    out = []
    for r in rules:
        if not isinstance(r, dict) or not isinstance(r.get("id"), str):
            errors.append("%s: a require rule needs an id" % where)
        elif set(r) == {"id", "summary", "max"} and isinstance(r["summary"], str) and isinstance(r["max"], int):
            out.append(r)
        elif set(r) == {"id", "detail", "only_keys"} and isinstance(r["detail"], str) and is_str_list(r["only_keys"]):
            out.append(r)
        else:
            errors.append("%s: require rule %s must be {id, summary, max} or {id, detail, only_keys}" % (where, r.get("id")))
    return out


def check_scope(where, scope, errors):
    if scope is None:
        return None
    if (not isinstance(scope, dict) or set(scope) != {"detail", "items"} or not isinstance(scope["detail"], str)
            or not scope["detail"] or not is_str_list(scope["items"])):
        errors.append("%s: scope must be {detail: <dotted path>, items: [...]}" % where)
        return None
    return {"detail": scope["detail"], "items": list(scope["items"])}


def check_bindings(where, value, user_step, errors):
    """A definition's plan excerpts as [{text, items, cell}]. A suite or global
    condition quotes the closing gate text; a user step quotes the user steps
    text, or the closing gate text with cell: gate."""
    if value is None:
        return []
    if not isinstance(value, list) or not value:
        errors.append("%s: plan must be a list of excerpts of the plan's table text" % where)
        return []
    out = []
    for b in value:
        b = {"text": b} if isinstance(b, str) else b
        keys = {"text", "items", "cell"} if user_step else {"text", "items"}
        if not isinstance(b, dict) or set(b) - keys or not isinstance(b.get("text"), str) or not b["text"].strip():
            errors.append("%s: a plan excerpt is a string or {text, items%s}" % (where, ", cell" if user_step else ""))
            continue
        items = b.get("items", [])
        if not is_str_list(items, nonempty=False):
            errors.append("%s: plan excerpt %r: items must be a list of names" % (where, b["text"]))
            items = []
        cell = b.get("cell", "user_steps" if user_step else "gate")
        if cell not in PLAN_CELLS:
            errors.append("%s: plan excerpt %r: cell must be gate or user_steps" % (where, b["text"]))
            continue
        span = UNIT_RANGE.search(b["text"])
        if span:
            errors.append("%s: plan excerpt %r spans the unit range %s; quote each unit of it on its own (U%s)"
                          % (where, b["text"], span.group(0), span.group(2)))
        out.append({"text": b["text"], "items": list(items), "cell": cell})
    return out


def check_items(where, bindings, have, what, errors):
    """Every item an excerpt binds must be one of the definition's `have`, its
    `what` (scope items, screens, cases); have None: `what` names the kind of
    definition, which binds no items."""
    for b in bindings:
        if not b["items"]:
            continue
        if have is None:
            errors.append("%s: %s binds no items (plan excerpt %r)" % (where, what, b["text"]))
            continue
        lack = [i for i in b["items"] if i not in have]
        if lack:
            errors.append("%s: plan excerpt %r binds %s, which its %s lacks (%s)"
                          % (where, b["text"], ", ".join(lack), what, ", ".join(have) or "none"))


def plan_units(text):
    """The unit ids a plan text names, ranges (U252–U254) expanded."""
    units = set(UNIT_TOKEN.findall(text))
    for m in UNIT_RANGE.finditer(text):
        a, b = int(m.group(1)), int(m.group(2))
        if a < b <= a + 100:
            units.update("U%d" % n for n in range(a, b + 1))
    return units


def excerpt_spans(text, excerpt):
    """Where excerpt occurs in text, not inside a longer word or id."""
    spans, i = [], text.find(excerpt)
    while i >= 0:
        j = i + len(excerpt)
        if not (i > 0 and text[i - 1].isalnum() and excerpt[0].isalnum()) and \
                not (j < len(text) and text[j].isalnum() and excerpt[-1].isalnum()):
            spans.append((i, j))
        i = text.find(excerpt, i + 1)
    return spans


def unclaimed(text, covered):
    """The runs of text no excerpt covers that hold more than connectives."""
    runs, start = [], None
    for i, c in enumerate(covered + [True]):
        if not c and start is None:
            start = i
        elif c and start is not None:
            runs.append(text[start:i])
            start = None
    return [r.strip(" .,;:") for r in runs
            if any(w.lower() not in CONNECTIVES for w in re.findall(r"[^\W_]+", r))]


def milestone_claims(model, m):
    """A milestone's plan excerpts by cell: [{key, who, b, step, spans}], key
    telling the definitions apart, step the user step (else None), spans
    where the excerpt occurs in the cell's text. A global condition applies
    from its `from` milestone on: its excerpts must occur in that milestone's
    text, and claim a later milestone's text where they occur there too (a
    later row that repeats the clause)."""
    mid = m["id"]
    order = model["order"]
    claims = {cell: [] for cell in PLAN_CELLS}

    def add(cell, key, who, b, step=None):
        claims[cell].append({"key": key, "who": who, "b": b, "step": step,
                             "spans": excerpt_spans(m["plan"][cell], b["text"])})

    for s in m["suites"]:
        for b in s["plan"]:
            add("gate", ("suite", s["id"]), "%s %s" % (mid, s["id"]), b)
    for b in m["cumulative"]:
        add("gate", ("cumulative", mid), "%s cumulative" % mid, b)
    for g in model["global"]:
        if g["from"] in order and order.index(g["from"]) <= order.index(mid):
            for b in g["plan"]:
                if g["from"] == mid or excerpt_spans(m["plan"]["gate"], b["text"]):
                    add("gate", ("global", g["id"]), "global %s" % g["id"], b)
    for st in m["user_steps"]:
        for b in st["plan"]:
            add(b["cell"], ("step", st["id"]), "%s %s" % (mid, st["id"]), b, st)
    return claims


def device_runs(claims):
    """{key: (user step, [excerpt texts])} of the device-run steps that claim plan text."""
    runs = {}
    for cell in PLAN_CELLS:
        for c in claims[cell]:
            if c["step"] is not None and c["step"]["validator"].get("type") == "device-run":
                runs.setdefault(c["key"], (c["step"], []))[1].append(c["b"]["text"])
    return runs


def run_devices(runs):
    """The devices device runs must cover: those their excerpts name, or all three."""
    texts = [t for _, ts in runs.values() for t in ts]
    return [d for d in DEVICES if any(re.search(r"(?<![A-Za-z0-9])%s(?![A-Za-z0-9])" % DEVICE_NAMES[d], t)
                                      for t in texts)] or list(DEVICES)


def gaps(m, claims):
    """What of a milestone's texts the claims leave unclaimed: runs of text,
    the units the text names, the devices its device runs need."""
    out = []
    for cell, label in PLAN_CELLS.items():
        text = m["plan"][cell]
        covered, units, claimed = [False] * len(text), plan_units(text), set()
        for c in claims[cell]:
            excerpt, spans = c["b"]["text"], c["spans"]
            if UNIT_TOKEN.fullmatch(excerpt):
                claimed.add(excerpt)
            elif len(spans) == 1:
                claimed.update(UNIT_TOKEN.findall(excerpt))
            else:
                continue  # not in the text, or not unique (check_coverage reports it)
            for i, j in spans:
                covered[i:j] = [True] * (j - i)
        for run in unclaimed(text, covered):
            out.append("%s: the %s text %r is claimed by no suite, global condition or user step; give the one that "
                       "implements it a plan excerpt (or restore it if it was removed)" % (m["id"], label, run))
        for u in sorted(units - claimed, key=lambda x: int(re.sub(r"[^0-9]", "", x))):
            out.append("%s: the %s text names %s, but no plan excerpt claims it (a unit inside a range needs its "
                       "own: plan: [%s])" % (m["id"], label, u, u))
    runs = device_runs(claims)
    if runs:
        required = run_devices(runs)
        have = [st["validator"].get("device") for st, _ in runs.values()]
        for d in required:
            if d not in have:
                out.append("%s: the plan asks for device runs on %s; there is no device-run user step on %s that "
                           "claims it" % (m["id"], ", ".join(DEVICE_NAMES[x] for x in required), d))
    return out


def overlaps(a, b):
    return any(i < z and x < j for i, j in a for x, z in b)


def check_excerpts(m, claims, errors):
    """Each excerpt is a unit the text names, or occurs once in its text."""
    for cell, label in PLAN_CELLS.items():
        units = plan_units(m["plan"][cell])
        for c in claims[cell]:
            excerpt, n = c["b"]["text"], len(c["spans"])
            if UNIT_TOKEN.fullmatch(excerpt):
                if excerpt not in units:
                    errors.append("%s: plan excerpt %r is not a unit the %s text of %s names"
                                  % (c["who"], excerpt, label, m["id"]))
            elif n == 0:
                errors.append("%s: plan excerpt %r is not in the %s text of %s" % (c["who"], excerpt, label, m["id"]))
            elif n > 1:
                errors.append("%s: plan excerpt %r occurs %d times in the %s text of %s; quote enough of it to make it "
                              "unique" % (c["who"], excerpt, n, label, m["id"]))


def check_shared(claims, errors):
    """Definitions share plan text only by quoting the same excerpt with the
    same items (bare unit ids bind none), so text bound to scope items is
    never also claimed without them."""
    for cell in PLAN_CELLS:
        quoted = [c for c in claims[cell] if not UNIT_TOKEN.fullmatch(c["b"]["text"]) and len(c["spans"]) == 1]
        for x, c in enumerate(quoted):
            for o in quoted[x + 1:]:
                if o["key"] == c["key"] or not overlaps(c["spans"], o["spans"]):
                    continue
                if o["spans"] != c["spans"]:
                    errors.append("%s: plan excerpt %r overlaps %r of %s; definitions share plan text only by quoting "
                                  "the same excerpt" % (o["who"], o["b"]["text"], c["b"]["text"], c["who"]))
                elif sorted(o["b"]["items"]) != sorted(c["b"]["items"]):
                    errors.append("%s: plan excerpt %r binds %s, and %s quotes it binding %s; definitions that claim "
                                  "the same plan text must bind the same items (a suite that only supports another "
                                  "quotes no plan text)" % (o["who"], o["b"]["text"], ", ".join(o["b"]["items"]) or
                                                            "no items", c["who"], ", ".join(c["b"]["items"]) or "no items"))


def check_wording(m, claims, errors):
    """The plan's wording names the kind of user step (STEP_WORDING): text
    that names a kind is claimed only by user steps of that kind (an "<N>-day"
    by a trial of N days), and a step claims text that names its kind."""
    seen = set()
    for cell, label in PLAN_CELLS.items():
        for kind, (pattern, cells, _) in STEP_WORDING.items():
            if cell not in cells:
                continue
            for hit in pattern.finditer(m["plan"][cell]):
                days = int(hit.group(1)) if kind == "trial" and hit.group(1) else None
                for c in claims[cell]:
                    if not overlaps([hit.span()], c["spans"]):
                        continue
                    v = c["step"]["validator"] if c["step"] else None
                    if v is None:
                        got = {"suite": "a suite", "cumulative": "the cumulative rule"}.get(c["key"][0],
                                                                                         "a global condition")
                    elif (kind in OUTRANKS.get(v.get("type"), ())
                          and STEP_WORDING[v["type"]][0].search(c["b"]["text"])):
                        continue  # the step's own wording names it; the other kind's is part of its clause
                    elif v.get("type") != kind:
                        got = "a %s step" % v.get("type")
                    elif days is not None and v.get("days") != days:
                        got = "a trial of %s days" % v.get("days")
                    else:
                        continue
                    msg = "%s: the %s text of %s says %r, which asks for a %s step%s; this is %s" % (
                        c["who"], label, m["id"], hit.group(0), kind, " of %d days" % days if days else "", got)
                    if msg not in seen:
                        seen.add(msg)
                        errors.append(msg)
    for st in m["user_steps"]:
        v = st["validator"]
        wording = STEP_WORDING.get(v.get("type"))
        if st["plan"] and wording and not any(b["cell"] in wording[1] and wording[0].search(b["text"])
                                              for b in st["plan"]):
            errors.append("%s %s: a %s step, but none of its plan excerpts says %s"
                          % (m["id"], st["id"], v.get("type"), wording[2]))
        if v.get("type") == "trial" and st["plan"] and not any(
                re.search(r"(?<![0-9])%s-day" % v.get("days"), b["text"]) for b in st["plan"]):
            errors.append("%s %s: a trial of %s days, but its plan excerpts do not say %s-day"
                          % (m["id"], st["id"], v.get("days"), v.get("days")))
    runs = device_runs(claims)
    if runs:
        required = run_devices(runs)
        for st, _ in runs.values():
            if st["validator"].get("device") not in required:
                errors.append("%s %s: a device run on %s, but its plan excerpts name only %s"
                              % (m["id"], st["id"], st["validator"].get("device"),
                                 ", ".join(DEVICE_NAMES[x] for x in required)))


def check_needed(m, claims, errors):
    """Every definition that quotes plan text is needed: without its claims
    some text, unit or device goes unclaimed (gaps), so removing it fails the
    check. Call it only when the claims leave no gap."""
    keys = {}
    for cell in PLAN_CELLS:
        for c in claims[cell]:
            keys.setdefault(c["key"], c["who"])
    for key, who in keys.items():
        if gaps(m, {cell: [c for c in cs if c["key"] != key] for cell, cs in claims.items()}):
            continue
        alike = sorted({o["who"] for cs in claims.values() for c in cs if c["key"] == key for o in cs
                        if o["key"] != key and (overlaps(c["spans"], o["spans"]) or c["b"]["text"] == o["b"]["text"])})
        errors.append("%s: removing it would go unnoticed: %s claim%s all the plan text it quotes; quote what only it "
                      "implements, or nothing if it supports another definition"
                      % (who, ", ".join(alike) or "other definitions", "s" if len(alike) == 1 else ""))


def check_coverage(model, errors):
    """The plan's table text against the definitions that run: every part of
    a milestone's closing gate and user steps texts is claimed by the plan
    excerpt of a suite, global condition (in its `from` milestone) or user
    step; each unit the text names, inside a range too, by its own excerpt;
    device runs cover the devices their excerpts name (all three when they
    name none). Then check_needed, check_shared and check_wording keep a
    single edit from weakening a milestone unnoticed."""
    for m in model["milestones"]:
        claims = milestone_claims(model, m)
        check_excerpts(m, claims, errors)
        check_shared(claims, errors)
        check_wording(m, claims, errors)
        missing = gaps(m, claims)
        errors += missing
        if not missing:
            check_needed(m, claims, errors)


def runnable(run):
    """Why a tiers run cannot be a milestone suite, or None."""
    if run["tier"] not in SUITE_TIERS:
        return "is a %s run; tools/mg milestone runs only %s" % (run["tier"], ", ".join(SUITE_TIERS))
    if run.get("release_only"):
        return "runs only in release runs"
    if run.get("user_step"):
        return "is a user step (%s)" % run["user_step"]
    return None


def gate_milestones(job, args, order):
    """The milestones a run of `job args` requires the gate of."""
    if job != GATE_JOB:
        return []
    out = []
    for a in args:
        if "{milestone}" in a:
            out.append("{milestone}")
        elif a in order:
            out.append(a)
    return out


def check_suite(where, s, runs, order, mi, errors):
    if not isinstance(s, dict):
        errors.append("%s: a suite must be a mapping" % where)
        return None
    extra = set(s) - {"id", "run", "scope", "require", "pending", "plan", "why"}
    if extra:
        errors.append("%s: unknown keys %s" % (where, ", ".join(sorted(extra))))
    if not isinstance(s.get("id"), str) or not SLUG.match(s["id"]):
        errors.append("%s: id must be a slug" % where)
        return None
    where = "%s %s" % (where.split(" suites")[0], s["id"])
    if not isinstance(s.get("run"), str) or not SLUG.match(s["run"]):
        errors.append("%s: run must name a tools/gate/tiers.yaml run" % where)
        return None
    pending = s.get("pending")
    if pending is not None and (not isinstance(pending, str) or not UNIT.match(pending)):
        errors.append("%s: pending must be a unit id" % where)
    run = runs.get(s["run"])
    if run is None and pending is None:
        errors.append("%s: run %s is not in tools/gate/tiers.yaml (list it there, or mark the suite pending: <unit>)"
                      % (where, s["run"]))
    if run is not None:
        why_not = runnable(run)
        if why_not:
            errors.append("%s: run %s %s" % (where, s["run"], why_not))
        for m in gate_milestones(run["job"], run["args"], order):
            if m == "{milestone}" or order.index(m) >= mi:
                errors.append("%s: run %s runs `tools/mg %s %s`, which requires the %s gate; no milestone's suites may require its own gate"
                              % (where, s["run"], run["job"], " ".join(run["args"]), order[mi] if m == "{milestone}" else m))
    scope = check_scope(where, s.get("scope"), errors)
    plan = check_bindings(where, s.get("plan"), False, errors)
    if scope:
        check_items(where, plan, scope["items"], "scope", errors)
    else:
        check_items(where, plan, None, "a suite without a scope", errors)
    return {
        "id": s["id"], "run": s["run"], "scope": scope,
        "require": check_rules(where, s.get("require"), errors), "pending": pending, "plan": plan,
        "why": s.get("why") if isinstance(s.get("why"), str) else "",
    }


def check_step(where, st, errors):
    if not isinstance(st, dict):
        errors.append("%s: a user step must be a mapping" % where)
        return None
    extra = set(st) - {"id", "description", "evidence", "validator", "plan"}
    if extra:
        errors.append("%s: unknown keys %s" % (where, ", ".join(sorted(extra))))
    if not isinstance(st.get("id"), str) or not SLUG.match(st["id"]):
        errors.append("%s: id must be a slug" % where)
        return None
    where = "%s %s" % (where.split(" user_steps")[0], st["id"])
    if not isinstance(st.get("description"), str) or not st["description"].strip():
        errors.append("%s: description is required" % where)
    if not is_rel_path(st.get("evidence")):
        errors.append("%s: evidence must be a path relative to the checkout" % where)
    v = st.get("validator")
    if not isinstance(v, dict) or v.get("type") not in VALIDATOR_KEYS:
        errors.append("%s: validator.type must be one of %s" % (where, ", ".join(sorted(VALIDATOR_KEYS))))
        return None
    t = v["type"]
    extra = set(v) - {"type"} - VALIDATOR_KEYS[t]
    if extra:
        errors.append("%s: a %s validator takes no %s" % (where, t, ", ".join(sorted(extra))))
    if t == "setup" and (not isinstance(v.get("step"), str) or not SLUG.match(v["step"])):
        errors.append("%s: setup needs step: <docs/process/one-time-setup.md step id>" % where)
    if t == "device-run" and v.get("device") not in DEVICES:
        errors.append("%s: device-run needs device: %s" % (where, "|".join(DEVICES)))
    if t == "variant-pick" and not is_str_list(v.get("screens")):
        errors.append("%s: variant-pick needs screens" % where)
    if t in ("trial", "device-session") and (not isinstance(v.get("days"), int) or isinstance(v["days"], bool) or v["days"] < 1):
        errors.append("%s: %s needs days: <whole number>" % (where, t))
    if t == "capture":
        if not is_rel_path(v.get("index")):
            errors.append("%s: capture needs index: <path>" % where)
        if ("cases" in v) == ("cases_file" in v):
            errors.append("%s: capture needs exactly one of cases and cases_file" % where)
        elif "cases" in v and not is_str_list(v["cases"]):
            errors.append("%s: cases must be a list of case ids" % where)
        elif "cases_file" in v and not is_rel_path(v["cases_file"]):
            errors.append("%s: cases_file must be a path" % where)
    if t == "first-sync" and (not is_str_list(v.get("results")) or not all(SLUG.match(r) for r in v["results"])
                              or len(set(v["results"])) != len(v["results"])):
        errors.append("%s: first-sync needs results: [<id of each result the sync relied on>, ...]" % where)
    plan = check_bindings(where, st.get("plan"), True, errors)
    if t == "variant-pick" and is_str_list(v.get("screens")):
        check_items(where, plan, v["screens"], "screens", errors)
    elif t == "capture" and is_str_list(v.get("cases")):
        check_items(where, plan, v["cases"], "cases", errors)
    elif t == "first-sync" and is_str_list(v.get("results")):
        check_items(where, plan, v["results"], "results", errors)
    else:
        check_items(where, plan, None, "a %s step" % t, errors)
    return {"id": st["id"], "description": st.get("description"), "evidence": st.get("evidence"), "validator": dict(v),
            "plan": plan}


def check_deferral(where, d, runs):
    """(entry, problems): the gate rejects an entry with problems."""
    problems = []
    if not isinstance(d, dict) or not isinstance(d.get("id"), str) or not SLUG.match(d["id"]):
        return None, ["%s: a deferral needs an id (slug)" % where]
    where = "%s %s" % (where.split(" deferred")[0], d["id"])
    extra = set(d) - {"id", "dependency", "condition", "cells", "approved_by", "why"}
    if extra:
        problems.append("%s: unknown keys %s" % (where, ", ".join(sorted(extra))))
    dep = DEPENDENCIES.get(d.get("dependency"))
    if dep is None:
        problems.append("%s: dependency must be one of %s" % (where, ", ".join(sorted(DEPENDENCIES))))
    elif d.get("condition") != dep["condition"]:
        problems.append("%s: the condition of %s is %s" % (where, d["dependency"], json.dumps(dep["condition"])))
    if d.get("approved_by") != "antoine":
        problems.append("%s: approved_by: antoine is required, written only from Antoine's chat approval" % where)
    cells = d.get("cells")
    entry = {"id": d["id"], "dependency": d.get("dependency"), "condition": d.get("condition"),
             "approved_by": d.get("approved_by"), "why": d.get("why") if isinstance(d.get("why"), str) else "",
             "cells": None}
    # The cells are named: a deferral waives them and nothing else of its run.
    if not isinstance(cells, dict) or set(cells) != {"run", "scope"} or not isinstance(cells.get("run"), str):
        problems.append("%s: cells must be {run: <tiers run>, scope: {detail, items}}: the deferred cells" % where)
    else:
        errs = []
        scope = check_scope(where, cells.get("scope"), errs)
        problems += errs or ([] if scope else ["%s: cells.scope must name the deferred cells" % where])
        run = runs.get(cells["run"])
        if run is None:
            problems.append("%s: cells run %s is not in tools/gate/tiers.yaml" % (where, cells["run"]))
        elif runnable(run):
            problems.append("%s: cells run %s %s" % (where, cells["run"], runnable(run)))
        entry["cells"] = {"id": "deferred:" + d["id"], "run": cells["run"], "scope": scope, "require": [],
                          "pending": None, "why": entry["why"]}
    return entry, problems


def build_model(manifest, config):
    """(model, errors, problems): errors make the manifest invalid; problems
    are deferral entries the gate rejects."""
    errors, problems = [], []
    runs = {r["id"]: r for r in config.get("runs", [])}
    if not isinstance(manifest, dict):
        return None, ["the manifest must be a mapping"], []
    extra = set(manifest) - {"version", "plan", "evidence_paths", "budget_devices", "global", "milestones",
                             "_first_sync_definitions"}
    if extra:
        errors.append("unknown top-level keys %s" % ", ".join(sorted(extra)))
    if manifest.get("version") != 1:
        errors.append("version must be 1")
    if not is_rel_path(manifest.get("plan")):
        errors.append("plan must name the plan file")
    evidence = manifest.get("evidence_paths") or []
    if not isinstance(evidence, list) or not all(is_rel_path(p) for p in evidence):
        errors.append("evidence_paths must be relative paths")
        evidence = []
    devices = manifest.get("budget_devices") or {}
    if not isinstance(devices, dict) or not all(
            RID.match(k) and isinstance(v, list) and v and all(d in DEVICES for d in v) for k, v in devices.items()):
        errors.append("budget_devices maps budget R-IDs to lists of %s" % ", ".join(DEVICES))
        devices = {}
    ms = manifest.get("milestones")
    order = [m.get("id") if isinstance(m, dict) else None for m in ms] if isinstance(ms, list) else []
    if (not order or not all(isinstance(i, str) and MILESTONE_ID.match(i) and i not in NOT_A_MILESTONE for i in order)
            or len(set(order)) != len(order)):
        errors.append("milestones must be a list of milestones in delivery order, each with its own id (a word, "
                      "not %s), as the plan's Delivery Milestones table lists them; got %s"
                      % (" or ".join(NOT_A_MILESTONE), json.dumps(order)))
        return None, errors, problems
    # The one milestone list and its order: everything else reads it from here.
    model = {"plan": manifest.get("plan"), "evidence_paths": list(evidence), "budget_devices": devices,
             "global": [], "milestones": [], "order": list(order),
             "_first_sync_definitions": manifest.get("_first_sync_definitions")}
    seen_units, suite_runs, suite_details = {}, {}, {}
    for mi, m in enumerate(ms):
        mid = m["id"]
        extra = set(m) - {"id", "name", "plan", "units", "cumulative", "suites", "budgets", "user_steps", "deferred"}
        if extra:
            errors.append("%s: unknown keys %s" % (mid, ", ".join(sorted(extra))))
        plan = m.get("plan")
        if not isinstance(m.get("name"), str) or not m["name"]:
            errors.append("%s: name is required" % mid)
        if (not isinstance(plan, dict) or set(plan) != {"gate", "user_steps"}
                or not all(isinstance(v, str) for v in plan.values())):
            errors.append("%s: plan must hold the table's gate and user_steps texts" % mid)
            plan = {"gate": "", "user_steps": ""}
        # The closing gate text that states the cumulative rule, which the
        # evaluator applies to every milestone: each excerpt names an earlier
        # milestone, whose gate this one includes.
        cumulative = check_bindings("%s cumulative" % mid, m.get("cumulative"), False, errors)
        check_items("%s cumulative" % mid, cumulative, None, "the cumulative rule", errors)
        for b in cumulative:
            if not any(re.search(r"(?<![A-Za-z0-9])%s(?![A-Za-z0-9])" % re.escape(e), b["text"]) for e in order[:mi]):
                errors.append("%s cumulative: plan excerpt %r names no earlier milestone (%s)"
                              % (mid, b["text"], ", ".join(order[:mi]) or "none"))
        units = m.get("units")
        if not is_str_list(units, nonempty=False) or not all(UNIT.match(u) for u in units):
            errors.append("%s: units must be unit ids" % mid)
            units = []
        for u in units:
            if u in seen_units:
                errors.append("%s: unit %s is already in %s; every unit belongs to exactly one milestone"
                              % (mid, u, seen_units[u]))
            seen_units[u] = mid
        suites, ids = [], set()
        for i, s in enumerate(m.get("suites") or []):
            out = check_suite("%s suites[%d]" % (mid, i), s, runs, order, mi, errors)
            if out is None:
                continue
            if out["id"] in ids:
                errors.append("%s: suite %s is listed twice" % (mid, out["id"]))
            ids.add(out["id"])
            if out["id"] in suite_runs and suite_runs[out["id"]] != out["run"]:
                errors.append("%s: suite %s runs %s here and %s in an earlier milestone"
                              % (mid, out["id"], out["run"], suite_runs[out["id"]]))
            suite_runs.setdefault(out["id"], out["run"])
            if out["scope"]:
                prev = suite_details.setdefault(out["id"], out["scope"]["detail"])
                if prev != out["scope"]["detail"]:
                    errors.append("%s: suite %s scopes details.%s here and details.%s earlier"
                                  % (mid, out["id"], out["scope"]["detail"], prev))
            out["since"] = mid
            suites.append(out)
        budgets = m.get("budgets") or {}
        if not isinstance(budgets, dict) or not all(
                SLUG.match(k) and is_str_list(v) and all(RID.match(r) for r in v) for k, v in budgets.items()):
            errors.append("%s: budgets maps group names to budget R-IDs" % mid)
            budgets = {}
        steps, sids = [], set()
        for i, st in enumerate(m.get("user_steps") or []):
            out = check_step("%s user_steps[%d]" % (mid, i), st, errors)
            if out is None:
                continue
            if out["id"] in sids:
                errors.append("%s: user step %s is listed twice" % (mid, out["id"]))
            sids.add(out["id"])
            steps.append(out)
        deferred = []
        for i, d in enumerate(m.get("deferred") or []):
            entry, probs = check_deferral("%s deferred[%d]" % (mid, i), d, runs)
            problems += probs
            if entry is not None:
                entry["since"] = mid
                entry["problems"] = probs
                deferred.append(entry)
        model["milestones"].append({"id": mid, "name": m.get("name"), "plan": plan, "units": list(units),
                                    "cumulative": cumulative, "suites": suites, "budgets": budgets,
                                    "user_steps": steps, "deferred": deferred})
    gids = set()
    for i, g in enumerate(manifest.get("global") or []):
        where = "global[%d]" % i
        if not isinstance(g, dict) or not isinstance(g.get("id"), str) or not SLUG.match(g["id"]):
            errors.append("%s: a global condition needs an id" % where)
            continue
        where = "global %s" % g["id"]
        extra = set(g) - {"id", "kind", "run", "pending", "from", "plan", "why"}
        if extra:
            errors.append("%s: unknown keys %s" % (where, ", ".join(sorted(extra))))
        plan = check_bindings(where, g.get("plan"), False, errors)
        check_items(where, plan, None, "a global condition", errors)
        if g["id"] in gids:
            errors.append("%s: listed twice" % where)
        gids.add(g["id"])
        frm = g.get("from", order[0])
        if frm not in order:
            errors.append("%s: from must be a milestone (%s)" % (where, ", ".join(order)))
            frm = order[0]
        kind, pending = g.get("kind"), g.get("pending")
        if (kind is None) == (pending is None):
            errors.append("%s: give either kind (%s) or pending: <unit>" % (where, ", ".join(GLOBAL_KINDS)))
            continue
        if pending is not None and (not isinstance(pending, str) or not UNIT.match(pending)):
            errors.append("%s: pending must be a unit id" % where)
        if kind is not None and kind not in GLOBAL_KINDS:
            errors.append("%s: kind must be one of %s" % (where, ", ".join(GLOBAL_KINDS)))
        entry = {"id": g["id"], "kind": kind, "pending": pending, "from": frm, "plan": plan,
                 "why": g.get("why") if isinstance(g.get("why"), str) else "", "suite": None}
        if kind == "suite":
            run = runs.get(g.get("run"))
            if run is None:
                errors.append("%s: run must name a tools/gate/tiers.yaml run" % where)
            elif runnable(run):
                errors.append("%s: run %s %s" % (where, g["run"], runnable(run)))
            entry["suite"] = {"id": "global:" + g["id"], "run": g.get("run"), "scope": None, "require": [],
                              "pending": None, "why": entry["why"]}
        elif "run" in g:
            errors.append("%s: only a suite condition names a run" % where)
        model["global"].append(entry)
    check_coverage(model, errors)
    return model, errors, problems


def cumulative(model, mi):
    """A milestone's suites: its own and every earlier milestone's, merged by
    id (scope items and pass rules add up; any pending entry keeps the suite
    pending)."""
    merged, order = {}, []
    for m in model["milestones"][:mi + 1]:
        for s in m["suites"]:
            cur = merged.get(s["id"])
            if cur is None:
                cur = merged[s["id"]] = {"id": s["id"], "run": s["run"], "scope": None, "require": [],
                                         "pending": [], "why": [], "since": s["since"]}
                order.append(s["id"])
            if s["scope"]:
                if cur["scope"] is None:
                    cur["scope"] = {"detail": s["scope"]["detail"], "items": []}
                cur["scope"]["items"] += [i for i in s["scope"]["items"] if i not in cur["scope"]["items"]]
            cur["require"] += [r for r in s["require"] if r["id"] not in [x["id"] for x in cur["require"]]]
            if s["pending"] and s["pending"] not in cur["pending"]:
                cur["pending"].append(s["pending"])
            if s["why"]:
                cur["why"].append("%s: %s" % (s["since"], s["why"]))
    return [merged[i] for i in order]


# ---------------------------------------------------------------------------
# check: the manifest against the plan


def plan_tables(text):
    """({milestone: {name, units, gate, user_steps}} in the table's order,
    unit index ids, unit heading ids, rows of the table that do not parse)."""
    rows, index, headings, bad, section = {}, [], [], [], None
    for line in text.splitlines():
        if line.startswith("### "):
            section = line[4:].strip()
            m = re.match(r"^### (U[0-9]+[a-z]?)\. ", line)
            if m:
                headings.append(m.group(1))
            continue
        if section == "Delivery Milestones" and line.startswith("| **"):
            # A milestone row: | **<id> <name>** | units | gate | user steps |
            cells = [c.strip() for c in re.split(r"(?<!\\)\|", line)[1:-1]]
            m = re.match(r"^\*\*(\S+) (.+)\*\*$", cells[0]) if len(cells) == 4 else None
            if m is None or m.group(1) in rows:
                bad.append(line[:80])
                continue
            rows[m.group(1)] = {"name": m.group(2), "units": [u.strip() for u in cells[1].split(",")],
                                "gate": cells[2], "user_steps": cells[3]}
        elif section == "Unit Index":
            m = re.match(r"^\| (U[0-9]+[a-z]?) \|", line)
            if m:
                index.append(m.group(1))
    return rows, index, headings, bad


def check_plan(model, text):
    errors = []
    rows, index, headings, bad = plan_tables(text)
    if not rows:
        return ["the plan has no Delivery Milestones table"]
    if not index:
        errors.append("the plan has no Unit Index")
    for line in bad:
        errors.append("the plan's Delivery Milestones table has a row that is not | **<id> <name>** | units | "
                      "closing gate | user steps | of a new id: %s" % line)
    listed_order = [i for i in model["order"] if i in rows]
    table_order = [i for i in rows if i in model["order"]]
    if listed_order != table_order:
        errors.append("the manifest lists the milestones as %s; the plan's table orders them %s"
                      % (", ".join(listed_order), ", ".join(table_order)))
    for m in model["milestones"]:
        row = rows.get(m["id"])
        if row is None:
            errors.append("%s: the plan's Delivery Milestones table has no row for it" % m["id"])
            continue
        if row["name"] != m["name"]:
            errors.append("%s: name %r; the plan says %r" % (m["id"], m["name"], row["name"]))
        if row["units"] != m["units"]:
            missing = [u for u in row["units"] if u not in m["units"]]
            extra = [u for u in m["units"] if u not in row["units"]]
            errors.append("%s: units differ from the plan's table (missing %s; not in the plan %s%s)"
                          % (m["id"], ", ".join(missing) or "none", ", ".join(extra) or "none",
                             "; order differs" if not missing and not extra else ""))
        for key, label in (("gate", "closing gate"), ("user_steps", "user steps")):
            if row[key] != m["plan"][key]:
                errors.append("%s: plan.%s is not the table's %s text; copy the cell and update the %s"
                              % (m["id"], key, label, "suites" if key == "gate" else "user steps"))
    for extra in [i for i in rows if i not in model["order"]]:
        errors.append("the plan's table has a row %s that the manifest lacks" % extra)
    listed = {u for m in model["milestones"] for u in m["units"]}
    for u in index:
        if u not in listed:
            errors.append("unit %s of the Unit Index is in no milestone" % u)
    for u in sorted(listed - set(index)):
        errors.append("unit %s is not in the plan's Unit Index" % u)
    for u in headings:
        if u not in listed and re.sub(r"[a-z]$", "", u) not in listed:
            errors.append("unit %s has a section in the plan but neither it nor %s is in a milestone"
                          % (u, re.sub(r"[a-z]$", "", u)))
    return errors


def cmd_check(a):
    try:
        manifest = read_json(a.manifest)
        config = read_json(a.config)
    except (OSError, ValueError) as e:
        print(json.dumps({"errors": ["cannot read the input: %s" % e]}))
        return 1
    model, errors, problems = build_model(manifest, config)
    errors += problems
    if model is not None:
        try:
            with open(os.path.join(a.root, model["plan"]), encoding="utf-8") as f:
                errors += check_plan(model, f.read())
        except OSError as e:
            errors.append("cannot read the plan: %s" % e)
    print(json.dumps({"errors": errors}, ensure_ascii=False))
    return 1 if errors else 0


# ---------------------------------------------------------------------------
# runs: what `tools/mg milestone <M>` runs


def milestone_runs(model, config, milestone):
    """The tiers run ids `tools/mg milestone <milestone>` plans, sorted: the
    runs its gate reads (its cumulative suites, the global suite conditions
    from their `from` milestone, the deferrals' cells), every run whose file
    one of them takes as an {artifact:<run>:<file>} argument, and the T4 runs
    whose `user_step` is the kind of one of its user steps (listed as awaiting
    Antoine, never run). No later milestone's suite: M1R never runs M1's."""
    order = model["order"]
    mi = order.index(milestone)
    runs = {r["id"]: r for r in config.get("runs", [])}
    want = [s["run"] for s in cumulative(model, mi)]
    want += [g["suite"]["run"] for g in model["global"] if g["kind"] == "suite" and order.index(g["from"]) <= mi]
    want += [d["cells"]["run"] for m in model["milestones"][:mi + 1] for d in m["deferred"] if d["cells"]]
    selected = set()
    while want:
        rid = want.pop()
        if rid in selected or rid not in runs:
            continue  # a pending suite's run that tiers.yaml does not list yet: the gate reports it pending
        selected.add(rid)
        want += [m.group(1) for m in (ARTIFACT.match(x) for x in runs[rid]["args"]) if m]
    kinds = {st["validator"]["type"] for st in model["milestones"][mi]["user_steps"]}
    selected.update(r["id"] for r in runs.values() if r["tier"] == "T4" and r.get("user_step") in kinds)
    return sorted(selected)


def cmd_runs(a):
    try:
        manifest = read_json(a.manifest)
        config = read_json(a.config)
    except (OSError, ValueError) as e:
        print(json.dumps({"errors": ["cannot read the input: %s" % e]}))
        return 2
    model, errors, _ = build_model(manifest, config)
    if errors:
        print(json.dumps({"errors": errors}, ensure_ascii=False))
        return 2
    if a.milestone not in model["order"]:
        print(json.dumps({"errors": ["unknown milestone %r; the milestones of the manifest are %s"
                                     % (a.milestone, ", ".join(model["order"]))]}, ensure_ascii=False))
        return 2
    print(json.dumps({"milestone": a.milestone, "runs": milestone_runs(model, config, a.milestone)}))
    return 0


# ---------------------------------------------------------------------------
# git


class Git:
    def __init__(self, root):
        self.root = root

    def run(self, *args):
        p = subprocess.run(["git", "-C", self.root] + list(args), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return p.returncode, p.stdout

    def out(self, *args):
        rc, out = self.run(*args)
        if rc != 0:
            raise Invalid("git %s failed" % " ".join(args))
        return out.decode("utf-8", "replace")

    def commit(self, rev):
        rc, out = self.run("rev-parse", "-q", "--verify", rev + "^{commit}")
        return out.decode().strip() if rc == 0 else None

    def is_ancestor(self, a, b):
        return self.run("merge-base", "--is-ancestor", a, b)[0] == 0

    def changed(self, a, b, *paths):
        out = self.out("diff", "-z", "--name-only", "--no-renames", a, b, "--", *paths)
        return [p for p in out.split("\0") if p]

    def commits(self, a, b):
        return [c for c in self.out("rev-list", "--reverse", "%s..%s" % (a, b)).split() if c]

    def show(self, rev, path):
        """The file's bytes at rev, or None when rev has no such file."""
        rc, out = self.run("cat-file", "blob", "%s:%s" % (rev, path))
        return out if rc == 0 else None

    def ls(self, rev, directory):
        rc, out = self.run("ls-tree", "-z", "--name-only", "%s:%s" % (rev, directory.rstrip("/")))
        return [n for n in out.decode("utf-8", "replace").split("\0") if n] if rc == 0 else []

    def uncommitted(self, path):
        rc, out = self.run("status", "--porcelain", "--untracked-files=all", "--", path)
        return rc == 0 and bool(out.strip())

    def unit_roles(self, rev):
        """{unit: set of roles} from the `Unit:` and `Role:` trailers of every
        commit reachable from rev."""
        out = self.out("log", "--format=%x1e%(trailers:key=Unit,valueonly,separator=%x1f)%x1d"
                              "%(trailers:key=Role,valueonly,separator=%x1f)", rev)
        roles = {}
        for rec in out.split("\x1e"):
            if "\x1d" not in rec:
                continue
            units, rs = rec.split("\x1d", 1)
            rset = {r.strip() for r in rs.split("\x1f") if r.strip()} or {""}
            for chunk in units.split("\x1f"):
                for u in re.split(r"[,\s]+", chunk.strip()):
                    if UNIT.match(u):
                        roles.setdefault(u, set()).update(rset)
        return roles


# ---------------------------------------------------------------------------
# results


class Results:
    """The job results under build/mg, and the one result the gate reads for
    each tiers run: the latest non-dry-run result of its job whose args are
    the run's with the milestone values. {base} and {tip} are the result's own
    commit, {milestone} the milestone, and {artifact:<run>:<file>} exactly
    that file among the artifacts of the result the gate reads for <run>,
    which must be clean and on the result's own commit."""

    def __init__(self, directory, runs, milestone):
        self.dir, self.runs, self.milestone = directory, runs, milestone
        self.selected = {}

    def for_run(self, run_id, resolving=()):
        """(path, result) the gate reads for a tiers run, or None."""
        if run_id in resolving or run_id not in self.runs:
            return None
        if run_id not in self.selected:
            run = self.runs[run_id]
            self.selected[run_id] = self.latest(run["job"], run["args"], resolving + (run_id,))
        return self.selected[run_id]

    def latest(self, job, pattern, resolving=()):
        """(path, result) of the latest non-dry-run result of `job` whose args
        match the pattern, or None."""
        best = None
        directory = os.path.join(self.dir, job)
        try:
            names = os.listdir(directory)
        except OSError:
            return None
        for name in names:
            if name == "latest":
                continue
            path = os.path.join(directory, name, "result.json")
            try:
                r = read_json(path)
            except (OSError, ValueError):
                continue
            if not isinstance(r, dict) or r.get("dry_run") is not False or r.get("job") != job:
                continue
            sha = (r.get("commit") or {}).get("sha") if isinstance(r.get("commit"), dict) else None
            if not self.args_match(pattern, r.get("args"), sha, resolving):
                continue
            key = (str(r.get("started_at") or ""), str(r.get("run_id") or name))
            if best is None or key > best[0]:
                best = (key, path, r)
        return (best[1], best[2]) if best else None

    def artifact(self, run_id, name, resolving):
        """(path, commit) of the file `name` among the artifacts of the clean
        result the gate reads for run_id, or None."""
        found = self.for_run(run_id, resolving)
        if found is None:
            return None
        r = found[1]
        commit = r.get("commit") if isinstance(r.get("commit"), dict) else {}
        if commit.get("dirty") is not False:
            return None
        paths = [x["path"] for x in r.get("artifacts") or []
                 if isinstance(x, dict) and isinstance(x.get("path"), str) and x["path"].rsplit("/", 1)[-1] == name]
        return (paths[0], commit.get("sha")) if paths else None

    def args_match(self, pattern, args, sha, resolving):
        if not isinstance(args, list) or len(pattern) != len(args):
            return False
        values = {"base": sha, "tip": sha, "milestone": self.milestone}
        for p, a in zip(pattern, args):
            m = ARTIFACT.match(p)
            if m:
                ref = self.artifact(m.group(1), m.group(2), resolving)
                if ref is None or a != ref[0] or not sha or ref[1] != sha:
                    return False
                continue
            try:
                expected = PLACEHOLDER.sub(lambda x: values[x.group(1)], p)
            except (KeyError, TypeError):
                return False
            if expected != a:
                return False
        return True


def cell_status(value):
    """A scope table entry's status: the string, or a mapping's status."""
    return value if isinstance(value, str) else (value.get("status") if isinstance(value, dict) else None)


def queued_steps(r):
    """The user steps a queued result waits for, and the parts it did not
    run (U19b): details.queued_parts, or without parts its missing user
    steps."""
    details = r.get("details") if isinstance(r.get("details"), dict) else {}
    parts = [p for p in details.get("queued_parts") or [] if isinstance(p, dict) and p.get("user_step")]
    steps = []
    for p in parts:
        if p["user_step"] not in steps:
            steps.append(p["user_step"])
    if not steps:
        steps = [u.get("id") for u in r.get("user_steps") or []
                 if isinstance(u, dict) and u.get("state") == "missing" and u.get("id")]
    return steps, [p.get("part") for p in parts if p.get("part")]


def judge(r, rules, scope, waiver=None):
    """(outcome, message) of one result under the pass rules: pass,
    awaiting_user, deferred, or a failing unavailable, warn, dirty or fail.
    A queued result (the machine's state kept it from testing, U19b) never
    passes: it counts as missing, awaiting the user step it names.

    waiver: the deferrals that hold for the result's run (Waiver), or None.
    They waive their cells and nothing else. A warn of the unavailable lane
    they cover is deferred when the result records no failure, the pass rules
    hold and every other cell the suite needs passes: its scope items but the
    waived ones, or without a scope every cell the result reports beside them
    (the deferral's detail table must be there). A pass whose scope lacks only
    waived cells is deferred too."""
    commit = r.get("commit") if isinstance(r.get("commit"), dict) else {}
    if not isinstance(commit.get("sha"), str) or not SHA.match(commit["sha"]):
        return "fail", "the result names no commit"
    if commit.get("dirty") is not False:
        return "dirty", "the result is from a dirty tree (uncommitted or untracked files); a gate result must be for a commit"
    status = r.get("status")
    details = r.get("details") if isinstance(r.get("details"), dict) else {}
    message = r.get("message") or ""
    covered = status == "warn" and waiver is not None and waiver.covers
    if status == "warn" and details.get("queued") is True:
        steps, parts = queued_steps(r)
        return "awaiting_user", "queued until user step %s clears (tools/mg catch-up runs it again); not tested: %s; %s" % (
            ", ".join(steps) or "(none named)", ", ".join(parts) or "the run", message)
    if status == "warn" and details.get("outcome") == "awaiting_user":
        return "awaiting_user", message or "awaiting Antoine"
    if status == "warn" and not covered:
        # Any other warn: the job did not run, or ran with something to look
        # at. Neither closes a milestone.
        lane = details.get("lane") if isinstance(details.get("lane"), dict) else {}
        if lane.get("status") == "unavailable":
            return "unavailable", "unavailable: %s (lane %s); it did not run, and no approved deferral covers it%s" % (
                lane.get("reason"), lane.get("kind"), (": " + message) if message else "")
        return "warn", "warn%s; a milestone suite must pass (only a queued run or one awaiting Antoine is told apart)" % (
            (": " + message) if message else "")
    if status not in ("pass", "warn"):
        fails = [f.get("id", "?") for f in r.get("failures") or [] if isinstance(f, dict)]
        return "fail", "%s%s" % (status, (": " + message) if message else "") + (
            " (%s)" % ", ".join(fails[:5]) if fails else "")
    waived = waiver.cells if waiver else {}
    used = covered  # whether a waived cell stands in for a pass
    bad = []
    if covered:
        fails = [f.get("id", "?") for f in r.get("failures") or [] if isinstance(f, dict)]
        if fails:
            bad.append("the result records failures (%s)" % ", ".join(fails[:5]))
        if not scope:
            # The suite needs its whole run: every cell beside the waived ones.
            for detail, items in sorted(waived.items()):
                table = get_path(details, detail)
                if not isinstance(table, dict):
                    bad.append("details.%s is missing, so the cells beside the deferred %s cannot be checked"
                               % (detail, ", ".join(items)))
                    continue
                for item, v in sorted(table.items()):
                    if item not in items and cell_status(v) != "pass":
                        bad.append("%s: %s" % (item, cell_status(v) or "not reported"))
    for rule in rules:
        if "summary" in rule:
            value = (r.get("summary") or {}).get(rule["summary"])
            if not isinstance(value, int) or value > rule["max"]:
                bad.append("%s: %s is %s; at most %d is allowed" % (rule["id"], rule["summary"], value, rule["max"]))
        else:
            value = get_path(details, rule["detail"])
            if value is None:
                continue  # no rows of that kind: nothing outside only_keys
            if not isinstance(value, dict):
                bad.append("%s: %s is not a table of counts" % (rule["id"], rule["detail"]))
                continue
            off = ", ".join("%s %s" % (n, k) for k, n in sorted(value.items()) if n and k not in rule["only_keys"])
            if off:
                bad.append("%s: %s: %s; only %s allowed" % (rule["id"], rule["detail"], off, ", ".join(rule["only_keys"])))
    if scope:
        skip = waived.get(scope["detail"], [])
        need = [i for i in scope["items"] if i not in skip]
        table = get_path(details, scope["detail"])
        if not isinstance(table, dict):
            if need:
                bad.append("details.%s is missing, so its scope (%s) cannot be checked"
                           % (scope["detail"], ", ".join(need)))
            used = used or bool(skip and set(skip) & set(scope["items"]))
        else:
            for item in scope["items"]:
                st = cell_status(table.get(item))
                if item in skip:
                    used = used or st != "pass"
                elif st != "pass":
                    bad.append("%s: %s" % (item, st or "not reported"))
    if bad:
        if covered or used:
            return "fail", "%s; the deferred cells (%s) are waived, not the rest" % ("; ".join(bad), waiver.summary())
        return "fail", "; ".join(bad)
    if used:
        return "deferred", "%s waived by %s%s" % (waiver.summary(), waiver.by, (": " + message) if message else "")
    return "pass", message


class Waiver:
    """The cells the deferrals that hold for one run waive ({detail: [items]}),
    whether its result's warn is the unavailable lane they cover, and which
    deferrals they are."""

    def __init__(self):
        self.cells, self.covers, self.names = {}, False, []

    @property
    def by(self):
        return "; ".join(self.names)

    def summary(self):
        return "; ".join("%s %s" % (d, ", ".join(items)) for d, items in sorted(self.cells.items()))


# ---------------------------------------------------------------------------
# user-step validators: each returns (outcome, message, details) with outcome
# pass, awaiting_user or fail


def evidence_json(ctx, path):
    """(value, None) from the file at HEAD; (None, outcome tuple) when it is
    absent or unreadable."""
    raw = ctx["git"].show(ctx["head"], path)
    if raw is None:
        return None, ("awaiting_user", "no evidence at %s%s" % (path, ctx["hint"](path)), {})
    try:
        return json.loads(raw.decode("utf-8")), None
    except (UnicodeDecodeError, ValueError):
        return None, ("fail", "%s is not valid JSON" % path, {})


def v_setup(ctx, step, v):
    doctor = ctx["doctor"]
    if not isinstance(doctor, dict):
        return "fail", "tools/mg doctor gave no readable result", {}
    found = [s for s in doctor.get("user_steps") or [] if isinstance(s, dict) and s.get("id") == v["step"]]
    if not found:
        return "pass", "done (tools/mg doctor lists no %s step)" % v["step"], {"state": "done"}
    state = found[0].get("state")
    if state == "unverified":
        return "pass", "tools/mg doctor cannot check %s; recorded as unverified" % v["step"], {"state": "unverified"}
    return "awaiting_user", "Antoine: %s (docs/process/one-time-setup.md#%s)" % (
        found[0].get("title") or v["step"], v["step"]), {"state": state}


def required_budgets(ctx, device):
    out = []
    for m in ctx["model"]["milestones"][:ctx["mi"] + 1]:
        for group in m["budgets"].values():
            for rid in group:
                if rid not in out and device in ctx["model"]["budget_devices"].get(rid, DEVICES):
                    out.append(rid)
    return out


def v_device_run(ctx, step, v):
    data, missing = evidence_json(ctx, step["evidence"])
    if missing:
        return missing
    path, mid, device = step["evidence"], ctx["milestone"], v["device"]
    if not isinstance(data, dict) or not isinstance(data.get("budgets"), list):
        return "fail", "%s: expected {milestone, device, build_commit, budgets: [...]}" % path, {}
    if data.get("milestone") != mid:
        return "fail", "%s names milestone %r, not %s" % (path, data.get("milestone"), mid), {}
    if data.get("device") != device:
        return "fail", "%s names device %r, not %s" % (path, data.get("device"), device), {}
    build = data.get("build_commit")
    ref = ctx["gate"] or ctx["head"]
    if not isinstance(build, str) or not SHA.match(build) or ctx["git"].commit(build) != build:
        return "fail", "%s: build_commit must be a full commit id of this repository" % path, {}
    if not ctx["git"].is_ancestor(build, ref):
        return "fail", "%s: build commit %s is not an ancestor of the gate commit %s" % (path, build[:12], ref[:12]), {}
    required = required_budgets(ctx, device)
    info = {"build_commit": build, "budgets": required, "changed_since_build": ctx["git"].changed(build, ref, "app", "core")}
    if ctx["catalog"] is None:
        return "fail", "%s: no budget catalog to check it against (%s)" % (path, ctx["catalog_error"]), info
    # U241's report contract: schema, Profile configuration, the device as
    # runner, and every device case of the catalog recorded with its
    # dimensions and judged again from its values.
    outcome, message, details = perf_gate.verify_device_report(data, ctx["catalog"], device, required)
    info.update(details)
    return outcome, "%s %s" % (path, message), info


def decision_rows(text):
    rows = []
    for line in text.splitlines():
        cells = [c.strip() for c in re.split(r"(?<!\\)\|", line)[1:-1]] if line.startswith("|") else []
        if len(cells) >= 3 and as_date(cells[0]):
            rows.append({"date": as_date(cells[0]), "screen": cells[1], "decision": cells[2]})
    return rows


def window_text(ctx):
    return "since %s" % ctx["start"].isoformat() if ctx["start"] else "within the milestone"


def in_window(ctx, day):
    return day <= ctx["today"] and (ctx["start"] is None or day >= ctx["start"])


def v_variant_pick(ctx, step, v):
    raw = ctx["git"].show(ctx["head"], step["evidence"])
    rows = decision_rows(raw.decode("utf-8", "replace")) if raw is not None else []
    picked, waiting = {}, []
    for screen in v["screens"]:
        hits = [r for r in rows if r["screen"] == screen and r["decision"].lower().startswith("picked")
                and in_window(ctx, r["date"])]
        if hits:
            picked[screen] = {"date": hits[-1]["date"].isoformat(), "decision": hits[-1]["decision"]}
        else:
            waiting.append(screen)
    if waiting:
        return "awaiting_user", "no pick logged in %s %s for %s%s" % (
            step["evidence"], window_text(ctx), ", ".join(waiting), ctx["hint"](step["evidence"])), {"picked": picked}
    return "pass", "picks logged for %s" % ", ".join(v["screens"]), {"picked": picked}


def is_none(value):
    return value is None or value == "none"


def v_trial(ctx, step, v):
    raw = ctx["git"].show(ctx["head"], step["evidence"])
    if raw is None:
        return "awaiting_user", "no daily-driver log at %s%s" % (step["evidence"], ctx["hint"](step["evidence"])), {}
    days = {}
    for n, line in enumerate(raw.decode("utf-8", "replace").splitlines(), 1):
        if not line.strip():
            continue
        try:
            e = json.loads(line)
        except ValueError:
            return "fail", "%s line %d is not JSON" % (step["evidence"], n), {}
        day = as_date(e.get("date")) if isinstance(e, dict) else None
        if day is None:
            return "fail", "%s line %d has no date (YYYY-MM-DD)" % (step["evidence"], n), {}
        # A day counts only when its entry says what happened: an absent
        # fallback or incident is not "none".
        lacking = [k for k in ("devices", "fallback", "data_incident") if k not in e]
        if lacking:
            return "fail", ("%s line %d lacks %s: every entry records, from Antoine's statement, the devices used, "
                            "the fallback (none or {task, in_scope}) and the data incident (none or what happened)"
                            % (step["evidence"], n, ", ".join(lacking))), {}
        devices = e["devices"]
        if (not isinstance(devices, list) or not devices or not all(isinstance(d, str) and d in DEVICES for d in devices)
                or len(set(devices)) != len(devices)):
            return "fail", "%s line %d: devices must list the devices used, each once (%s)" % (
                step["evidence"], n, ", ".join(DEVICES)), {}
        fb, inc = e["fallback"], e["data_incident"]
        if not is_none(fb) and not (isinstance(fb, dict) and isinstance(fb.get("task"), str)
                                    and isinstance(fb.get("in_scope"), bool)):
            return "fail", "%s line %d: fallback must be none or {task, in_scope}" % (step["evidence"], n), {}
        if day > ctx["today"]:
            return "fail", "%s line %d is dated after today" % (step["evidence"], n), {}
        if not in_window(ctx, day):
            continue
        bad = days.setdefault(day, [])
        if not is_none(inc) and inc != "":
            bad.append("data incident: %s" % (inc if isinstance(inc, str) else json.dumps(inc)))
        if isinstance(fb, dict) and fb["in_scope"]:
            bad.append("in-scope fallback: %s" % fb["task"])
    if not days:
        return "awaiting_user", "the log has no day %s" % window_text(ctx), {}
    last = max(days)
    streak, day = 0, last
    while day in days and not days[day]:
        streak += 1
        day -= datetime.timedelta(days=1)
    info = {"streak": streak, "days": v["days"], "last": last.isoformat()}
    if streak >= v["days"]:
        info["first"] = (last - datetime.timedelta(days=streak - 1)).isoformat()
        return "pass", "%d consecutive logged days ending %s, no in-scope fallback or data incident" % (
            streak, last.isoformat()), info
    if day in days:
        return "fail", "the trial broke on %s (%s); %d of %d days since" % (
            day.isoformat(), "; ".join(days[day]), streak, v["days"]), info
    return "awaiting_user", "%d of %d consecutive days logged, ending %s" % (streak, v["days"], last.isoformat()), info


def v_device_session(ctx, step, v):
    directory = step["evidence"]
    latest, found = None, None
    for name in sorted(ctx["git"].ls(ctx["head"], directory)):
        if not name.endswith(".json"):
            continue
        path = directory.rstrip("/") + "/" + name
        data, problem = evidence_json(ctx, path)
        if problem:
            return problem
        day = as_date(data.get("date")) if isinstance(data, dict) else None
        if day is None or data.get("reported_by") != "antoine":
            return "fail", "%s needs a date (YYYY-MM-DD) and reported_by: antoine" % path, {}
        latest = max(latest, day) if latest else day
        if ctx["today"] - datetime.timedelta(days=v["days"]) <= day <= ctx["today"]:
            found = (path, day)
    if found:
        return "pass", "device session %s (%s)" % (found[1].isoformat(), found[0]), {
            "file": found[0], "date": found[1].isoformat()}
    return "awaiting_user", "no device session in the last %d days (latest: %s)%s" % (
        v["days"], latest.isoformat() if latest else "none", ctx["hint"](directory)), {}


def case_ids(data):
    if not isinstance(data, dict) or not isinstance(data.get("cases"), list):
        return None
    ids = [c.get("id") if isinstance(c, dict) else None for c in data["cases"]]
    return ids if all(isinstance(i, str) and i for i in ids) else None


def v_capture(ctx, step, v):
    if "cases_file" in v:
        data, problem = evidence_json(ctx, v["cases_file"])
        required = case_ids(data) if not problem else None
        if required is None:
            return "fail", "the case list %s is missing or not {cases: [{id}, ...]}; its owning area writes it" % (
                v["cases_file"]), {}
    else:
        required = list(v["cases"])
    data, problem = evidence_json(ctx, v["index"])
    if problem:
        return problem
    listed = case_ids(data)
    if listed is None:
        return "fail", "%s must be {cases: [{id}, ...]}" % v["index"], {}
    absent = [c for c in required if c not in listed]
    if absent:
        return "awaiting_user", "%s lacks %s" % (v["index"], ", ".join(absent)), {"absent": absent}
    return "pass", "%s lists all %d cases" % (v["index"], len(required)), {"cases": len(required)}


def v_report(ctx, step, v):
    data, problem = evidence_json(ctx, step["evidence"])
    if problem:
        return problem
    if (not isinstance(data, dict) or as_date(data.get("date")) is None or data.get("result") not in ("pass", "fail")
            or data.get("reported_by") != "antoine"):
        return "fail", "%s needs {date, result: pass|fail, reported_by: antoine}" % step["evidence"], {}
    if data["result"] == "fail":
        return "fail", "Antoine reported a failure on %s (%s)" % (data["date"], step["evidence"]), {}
    return "pass", "Antoine reported a pass on %s" % data["date"], {"date": data["date"]}


# Existing supported jobs. General conformance and crash/fault proof remain
# mandatory; bootstrap checks do not stand in for the pending convergence soak.
FIRST_SYNC_RESULTS = {
    "u246-sync-area": ("u246-conformance", "conformance-aggregate", "areas", ("sync",), None),
    "sync-server-test": ("sync-server-test", "sync-server-test", None, (), None),
    "sync-server-package-test": ("sync-server-package-test", "sync-server-package-test", None, (), None),
    "meguri-provider-real-server": ("meguri-provider-real-server", "meguri-provider-real-server", None, (), None),
    "u250-sync-classes": ("u250-fault", "fault", "classes", ("review", "sync", "backup"), None),
}
# Same phase contract as mg_meguri_provider__result. These are synthetic
# collection facts, never account identities or a real user's collection.
FIRST_SYNC_PHASES = {
    "upload": ["fresh-upload-client", "explicit-collection-and-media", "retained-safety-artifacts",
               "all-media-accounted", "ready-automatic", "exact-fixture", "closed"],
    "download": ["initial-no-collection", "explicit-collection-and-media", "initial-no-backup",
                 "all-media-accounted", "ready-automatic", "exact-fixture", "closed"],
    "reopen": ["cold-ready", "persisted-binding-epoch", "exact-fixture", "closed"],
}
MG_RUN_ID = re.compile(r"^[0-9]{8}T[0-9]{6}Z-[0-9]+$")


def _first_sync_bytes(boundary, path):
    """Read one bounded file via nofollow directory descriptors."""
    if ".." in path.split(os.path.sep) or "\x00" in path:
        raise Invalid("path-escape")
    boundary, path = os.path.abspath(boundary), os.path.abspath(path)
    if os.path.commonpath((boundary, path)) != boundary or path == boundary:
        raise Invalid("path-escape")
    fd = None
    try:
        fd = os.open(os.path.sep, os.O_RDONLY | os.O_DIRECTORY)
        parts = path.split(os.path.sep)[1:]
        for part in parts[:-1]:
            next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = next_fd
        file_fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        with os.fdopen(file_fd, "rb") as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise Invalid("not-regular")
            raw = stream.read(16 * 1024 * 1024 + 1)
            if len(raw) > 16 * 1024 * 1024:
                raise Invalid("oversize")
    except OSError:
        raise Invalid("missing-unreadable-or-symlink") from None
    finally:
        if fd is not None:
            os.close(fd)
    return raw


def _first_sync_json(boundary, path, *, witness=None):
    raw = _first_sync_bytes(boundary, path)
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise ValueError()
            out[key] = value
        return out

    def constant(_):
        raise ValueError()

    try:
        value = json.loads(raw, object_pairs_hook=pairs, parse_constant=constant)
    except (ValueError, UnicodeError, RecursionError):
        raise Invalid("malformed-json") from None
    if witness is not None:
        witness["sha256"] = hashlib.sha256(raw).hexdigest()
    return value


def _first_sync_envelope(r):
    """Complete fixed v1 MG envelope, stdlib equivalent of mg-result.schema.json.

    Keep this deliberately specific to MG; it is not a JSON Schema engine.
    Return bounded field categories, never JSON values or schema exceptions.
    """
    fields = {"schema_version", "job", "run_id", "status", "exit_code", "placement", "service", "dry_run",
              "args", "root", "started_at", "finished_at", "duration_ms", "commit", "message", "summary",
              "failures", "artifacts", "user_steps", "details"}
    if not isinstance(r, dict) or set(r) != fields:
        raise Invalid("envelope-schema-fields")
    integer = lambda n: type(n) is int and n >= 0
    text = lambda s: isinstance(s, str)
    nonempty = lambda s: text(s) and bool(s)
    checks = {
        "schema_version": type(r["schema_version"]) is int and r["schema_version"] == 1,
        "job": text(r["job"]), "run_id": text(r["run_id"]) and bool(MG_RUN_ID.fullmatch(r["run_id"])),
        "status": r["status"] in ("pass", "warn", "fail", "error"),
        "exit_code": type(r["exit_code"]) is int and r["exit_code"] in (0, 1, 2),
        "placement": r["placement"] in ("host", "docker", "none"),
        "service": r["service"] is None or text(r["service"]), "dry_run": type(r["dry_run"]) is bool,
        "args": isinstance(r["args"], list) and all(text(x) for x in r["args"]),
        "root": text(r["root"]), "message": text(r["message"]), "duration_ms": integer(r["duration_ms"]),
        "started_at": as_time(r["started_at"]) is not None,
        "finished_at": as_time(r["finished_at"]) is not None, "details": isinstance(r["details"], dict),
    }
    c, s = r["commit"], r["summary"]
    checks["commit"] = (isinstance(c, dict) and set(c) == {"sha", "dirty"}
                        and (c["sha"] is None or (text(c["sha"]) and bool(SHA.fullmatch(c["sha"]))))
                        and (c["dirty"] is None or type(c["dirty"]) is bool))
    checks["summary"] = (isinstance(s, dict) and {"total", "passed", "failed", "skipped"} <= set(s)
                         and all(integer(n) for n in s.values()))
    checks["failures"] = isinstance(r["failures"], list) and all(
        isinstance(f, dict) and set(f) == {"id", "message", "repro"}
        and nonempty(f["id"]) and text(f["message"]) and nonempty(f["repro"]) for f in r["failures"])
    checks["artifacts"] = isinstance(r["artifacts"], list) and all(
        isinstance(a, dict) and {"kind", "path"} <= set(a) <= {"kind", "path", "label"}
        and nonempty(a["kind"]) and text(a["path"]) and a["path"].startswith("/")
        and ("label" not in a or text(a["label"])) for a in r["artifacts"])
    checks["user_steps"] = isinstance(r["user_steps"], list) and all(
        isinstance(u, dict) and set(u) == {"id", "state", "required", "title", "how", "doc"}
        and text(u["id"]) and re.fullmatch(r"[a-z0-9-]+", u["id"])
        and u["state"] in ("missing", "unverified") and type(u["required"]) is bool
        and all(text(u[k]) for k in ("title", "how", "doc")) for u in r["user_steps"])
    for field, valid in checks.items():
        if not valid:
            raise Invalid("envelope-schema-" + field)


def _first_sync_run(ctx, commit, run_id):
    """Use load's exact historical definition, bound to its source bytes."""
    run = ctx["results"].runs.get(run_id)
    if not run or run.get("pending") or run.get("disabled"):
        raise Invalid("run-missing-pending-or-disabled")
    source = ctx["git"].show(commit, "tools/gate/tiers.yaml")
    if source is None:
        raise Invalid("historical-definition-missing")
    compiled = ctx["model"].get("_first_sync_definitions")
    if not isinstance(compiled, dict) or compiled.get("head") != ctx["head"]:
        raise Invalid("historical-definition-unavailable")
    builds = compiled.get("builds")
    entry = builds.get(commit) if isinstance(builds, dict) else None
    if not isinstance(entry, dict):
        raise Invalid("historical-definition-unavailable")
    if entry.get("sha256") != hashlib.sha256(source).hexdigest():
        raise Invalid("historical-definition-mismatch")
    if entry.get("error"):
        raise Invalid("historical-definition-malformed")
    runs = entry.get("runs")
    old = runs.get(run_id) if isinstance(runs, dict) else None
    if not isinstance(old, dict) or old.get("pending") or old.get("disabled"):
        raise Invalid("historical-definition-unavailable")
    if old.get("job") != run["job"] or not is_str_list(old.get("args", []), nonempty=False):
        raise Invalid("historical-definition-mismatch")
    return dict(run, args=old.get("args", []))


def _first_sync_artifacts(ctx, rid, receipt):
    """Validate the existing native bootstrap producer's required phase files."""
    if rid != "meguri-provider-real-server":
        return [], []
    loaded, problems = [], []
    for phase, checks in FIRST_SYNC_PHASES.items():
        declared = [a for a in receipt["artifacts"] if os.path.basename(a["path"]) == phase + ".json"]
        if len(declared) != 1 or declared[0]["kind"] != "provider-phase":
            problems.append("phase-artifact-declaration")
            continue
        path, witness = declared[0]["path"], {}
        try:
            value = _first_sync_json(os.path.join(ctx["git"].root, "build"), path, witness=witness)
            loaded.append(path)
            ctx.setdefault("_first_sync_artifact_hashes", {})[path] = witness["sha256"]
            expected = {"format": 1, "phase": phase, "status": "pass", "checks": checks,
                        "counts": {"notes": 2, "cards": 2, "media": 2, "revlogs": 0, "reps": 0}}
            if value != expected or type(value.get("format")) is not int or any(
                    type(n) is not int for n in value["counts"].values()):
                problems.append("phase-artifact-substitution")
        except Invalid as error:
            problems.append("artifact-" + str(error))
    return sorted(set(problems)), loaded


def _first_sync_reference(ctx, rid, reference, commit, started, resolving=()):
    run_id, job, detail, items, lane = FIRST_SYNC_RESULTS[rid]
    if rid in resolving:
        raise Invalid("receipt-argument-reference-cycle")
    if not isinstance(reference, dict):
        raise Invalid("result-reference-missing")
    if set(reference) - {"job", "run_id", "commit", "finished_at", "outcome"}:
        raise Invalid("result-reference-fields")
    if reference.get("job") != job:
        raise Invalid("reference-job-mismatch")
    selected = reference.get("run_id")
    if not isinstance(selected, str) or not MG_RUN_ID.fullmatch(selected):
        raise Invalid("reference-run-id-invalid")
    path = os.path.join(ctx["results"].dir, job, selected, "result.json")
    try:
        receipt = _first_sync_json(ctx["results"].dir, path)
    except Invalid as e:
        raise Invalid("receipt-" + str(e)) from None
    _first_sync_envelope(receipt)
    if receipt["job"] != job or receipt["run_id"] != selected:
        raise Invalid("receipt-job-or-run-id-mismatch")
    if receipt["root"] != os.path.abspath(ctx["git"].root):
        raise Invalid("receipt-root-checkout-mismatch")
    if receipt["commit"] != {"sha": commit, "dirty": False}:
        raise Invalid("receipt-build-commit-or-dirty-mismatch")
    if receipt["dry_run"]:
        raise Invalid("receipt-dry-run")
    if receipt["status"] != "pass" or receipt["exit_code"] != 0:
        raise Invalid("receipt-status-or-exit-not-pass")
    if receipt["summary"]["failed"] != 0 or receipt["failures"]:
        raise Invalid("receipt-summary-failed-or-failures")
    if receipt["details"].get("queued") or receipt["details"].get("outcome") == "awaiting_user" or receipt["user_steps"]:
        raise Invalid("receipt-queued-or-awaiting-user")
    if as_time(receipt["started_at"]) > as_time(receipt["finished_at"]) or as_time(receipt["finished_at"]) >= started:
        raise Invalid("receipt-time-order-or-finish-not-before-sync")
    for key, actual in (("commit", commit), ("finished_at", receipt["finished_at"]), ("outcome", "pass")):
        if key in reference and reference[key] != actual:
            raise Invalid("copied-display-" + key + "-disagrees-with-receipt")
    run = _first_sync_run(ctx, commit, run_id)
    exact = Results(ctx["results"].dir, ctx["results"].runs, ctx["milestone"])
    # This path must never cause args_match's artifact resolution to discover
    # a latest run. Unreferenced artifact dependencies remain unresolved.
    exact.selected = {key: None for key in exact.runs}
    for pattern in run["args"]:
        match = ARTIFACT.fullmatch(pattern)
        if not match:
            continue
        aliases = [key for key, mapped in FIRST_SYNC_RESULTS.items() if mapped[0] == match.group(1)]
        if len(aliases) != 1:
            raise Invalid("receipt-argument-reference-missing")
        dep = aliases[0]
        referenced, metadata = _first_sync_reference(
            ctx, dep, ctx["references"].get(dep), commit, started, resolving + (rid,))
        paths = [a["path"] for a in referenced["artifacts"] if os.path.basename(a["path"]) == match.group(2)]
        if len(paths) != 1:
            raise Invalid("receipt-argument-artifact-declaration")
        try:
            _first_sync_bytes(os.path.join(ctx["git"].root, "build"), paths[0])
        except Invalid as e:
            raise Invalid("receipt-argument-artifact-" + str(e)) from None
        exact.selected[match.group(1)] = (metadata["receipt"], referenced)
    if run["job"] != job or not exact.args_match(run["args"], receipt["args"], commit, (run_id,)):
        raise Invalid("receipt-arguments-mismatch")
    if any(a == "--leg" or a.startswith("--leg=") for a in receipt["args"]):
        raise Invalid("receipt-filtered-leg")
    if lane and receipt["details"].get("lane") != lane:
        raise Invalid("receipt-lane-mismatch")
    scope = {"detail": detail, "items": list(items)} if detail else None
    if judge(receipt, [], scope)[0] != "pass":
        raise Invalid("receipt-required-" + (detail or "result") + "-scope-not-pass")
    suites = [s for m in ctx["model"]["milestones"][:ctx["mi"] + 1] for s in m["suites"]
              if s["run"] == run_id]
    if not suites or any(s.get("pending") for s in suites):
        raise Invalid("mapped-suite-missing-or-pending")
    for owner in [run] + suites:
        for rule in owner.get("require", []):
            if judge(receipt, [rule], None)[0] != "pass":
                # Rule identifiers come from approved configuration, but keep
                # even those out of diagnostics if they are not bounded slugs.
                key = rule["id"] if SLUG.fullmatch(rule["id"]) else "rule"
                raise Invalid("receipt-pass-rule-" + key)
    # Safe metadata is available even when the lane's evidence cannot pass.
    return receipt, {"job": job, "run_id": selected, "commit": commit, "finished_at": receipt["finished_at"],
                     "receipt": path, "receipt_validated": True, "evidence_validated": False}


def v_first_sync(ctx, step, v):
    """Check the first sync's exact historical receipts, never latest runs."""
    data, missing = evidence_json(ctx, step["evidence"])
    if missing:
        return missing
    path = step["evidence"]
    shape = ("{build: {commit, dirty}, sync_started_at, results: {<id>: {job, run_id}}, "
             "backup: {file, taken_at}} (times YYYY-MM-DDTHH:MM:SSZ)")
    if not isinstance(data, dict) or not all(isinstance(data.get(k), dict) for k in ("build", "results", "backup")):
        return "fail", "%s: expected %s" % (path, shape), {}
    build, results, backup = data["build"], data["results"], data["backup"]
    ctx = dict(ctx, references=results, _first_sync_artifact_hashes={})
    started = as_time(data.get("sync_started_at"))
    if started is None:
        return "fail", "%s: sync_started_at must be YYYY-MM-DDTHH:MM:SSZ" % path, {}
    commit = build.get("commit")
    if not isinstance(commit, str) or not SHA.match(commit) or ctx["git"].commit(commit) != commit:
        return "fail", "%s: build.commit must be a full commit id of this repository" % path, {}
    bad, verified = [], {}
    if build.get("dirty") is not False:
        bad.append("the build is not from a clean tree")
    if set(v["results"]) != set(FIRST_SYNC_RESULTS):
        bad.append("required-result-list-incomplete")
    for rid in FIRST_SYNC_RESULTS:
        try:
            receipt, metadata = _first_sync_reference(ctx, rid, results.get(rid), commit, started)
            problems, artifacts = _first_sync_artifacts(ctx, rid, receipt)
            metadata["evidence_validated"] = not problems
            metadata["artifacts_checked"] = artifacts
            metadata["artifact_sha256"] = {p: ctx["_first_sync_artifact_hashes"][p] for p in artifacts}
            if build.get("dirty") is False:
                verified[rid] = metadata
            if problems:
                bad.append("%s: %s" % (rid, ", ".join(problems)))
        except (Invalid, ValueError, UnicodeError) as error:
            # Invalid contains only fixed categories from these helpers.
            # Decoder errors from historical config contain source values.
            category = str(error) if isinstance(error, Invalid) else "historical-definition-malformed"
            bad.append("%s: %s" % (rid, category))
    taken = as_time(backup.get("taken_at"))
    if not isinstance(backup.get("file"), str) or not backup["file"] or taken is None:
        bad.append("the backup needs its file and taken_at")
    elif taken >= started:
        bad.append("backup-time-not-before-sync")
    details = {"build_commit": commit, "sync_started_at": data["sync_started_at"], "results": verified}
    if bad:
        return "fail", "%s: %s" % (path, "; ".join(bad)), details
    return "pass", "first sync at %s, on clean build %s, after passing %s and backup %s" % (
        data["sync_started_at"], commit[:12], ", ".join(v["results"]), backup["file"]), {
        "build_commit": commit, "sync_started_at": data["sync_started_at"], "backup": backup,
        "results": verified}


VALIDATORS = {
    "setup": v_setup, "device-run": v_device_run, "variant-pick": v_variant_pick, "trial": v_trial,
    "device-session": v_device_session, "capture": v_capture, "first-sync": v_first_sync, "report": v_report,
}


# ---------------------------------------------------------------------------
# evaluate


def parse_ledger(text):
    """{id: open escalation}; ValueError when the ledger does not parse."""
    lines = []
    for line in text.splitlines():
        if line.strip():
            rec = json.loads(line)
            if not isinstance(rec, dict) or not isinstance(rec.get("id"), str):
                raise ValueError("an escalation line has no id")
            lines.append(rec)
    settled = {r["id"] for r in lines if r.get("state") != "open"}
    out = {}
    for r in lines:
        if r.get("state") == "open" and r["id"] not in settled:
            out.setdefault(r["id"], r)
    return out


def evaluate(model, config, a):
    git = Git(a.root)
    order = model["order"]
    mi = order.index(a.milestone)
    milestone = model["milestones"][mi]
    now = a.now
    today = datetime.datetime.strptime(now, "%Y-%m-%dT%H:%M:%SZ").date()
    runs = {r["id"]: r for r in config.get("runs", [])}
    head = git.commit("HEAD")
    if head is None:
        raise Invalid("%s has no HEAD commit" % a.root)
    failures, awaiting = [], []
    seen = []  # (what, commit) of every result read

    def fail(fid, message):
        failures.append({"id": fid, "message": message})

    results = Results(a.results, runs, a.milestone)
    in_effect = []  # deferrals that hold: (model entry, record)

    def waiver_for(run_id, r):
        """The cells the deferrals in effect waive for this result of run_id
        (a Waiver), or None when none is for its run."""
        details = r.get("details") if isinstance(r.get("details"), dict) else {}
        waiver = None
        for d, rec in in_effect:
            if d["cells"]["run"] != run_id:
                continue
            waiver = waiver or Waiver()
            scope = d["cells"]["scope"]
            have = waiver.cells.setdefault(scope["detail"], [])
            have += [i for i in scope["items"] if i not in have]
            cover = DEPENDENCIES[d["dependency"]]["covers"]
            got = get_path(details, cover["detail"])
            if isinstance(got, dict) and all(got.get(k) == v for k, v in cover["equals"].items()):
                waiver.covers = True
            waiver.names.append("%s (%s, approved by %s; %s)" % (d["id"], d["dependency"], d["approved_by"],
                                                                 rec["message"]))
        return waiver

    def assess(s):
        """One suite record: its run's result judged under its pass rules."""
        rec = {"id": s["id"], "run": s["run"], "since": s.get("since"), "scope": s["scope"],
               "job": None, "args": None, "result": None, "run_id": None, "commit": None, "status": None,
               "summary": None, "outcome": None, "message": ""}
        run = runs.get(s["run"])
        pending = list(s["pending"]) if isinstance(s["pending"], list) else ([s["pending"]] if s["pending"] else [])
        if run is not None:
            rec["job"], rec["args"] = run["job"], list(run["args"])
            if run.get("pending"):
                pending.append(run["pending"])
            elif run.get("disabled"):
                rec["outcome"], rec["message"] = "pending", "its tiers run is disabled: %s" % run["disabled"]
                return rec
        if pending:
            rec["outcome"], rec["message"] = "pending", "lands with %s" % ", ".join(sorted(set(pending)))
            return rec
        if run is None:
            rec["outcome"], rec["message"] = "missing", "run %s is not in tools/gate/tiers.yaml" % s["run"]
            return rec
        found = results.for_run(s["run"])
        if found is None:
            refs = [m for m in (ARTIFACT.match(x) for x in run["args"]) if m]
            rec["outcome"] = "missing"
            rec["message"] = "no result of `tools/mg %s` (run %s; `tools/mg milestone %s` runs it)%s" % (
                " ".join([run["job"]] + run["args"]), s["run"], a.milestone, "".join(
                    " whose argument is the %s of the %s result the gate reads, made on its commit" % (
                        m.group(2), m.group(1)) for m in refs))
            return rec
        path, r = found
        rec["result"], rec["run_id"], rec["status"] = path, r.get("run_id"), r.get("status")
        rec["commit"] = (r.get("commit") or {}).get("sha")
        rec["summary"] = r.get("summary")
        rec["args"] = r.get("args")
        rules = [x for x in run.get("require") or [] if not x.get("release")] + s["require"]
        rec["outcome"], rec["message"] = judge(r, rules, s["scope"], waiver_for(s["run"], r))
        if rec["commit"]:
            seen.append((s["id"], rec["commit"]))
        return rec

    # Deferrals first: the condition's result, and the cells once it no
    # longer holds. A deferral that holds covers its cells' run below.
    deferred = []
    for m in model["milestones"][:mi + 1]:
        for d in m["deferred"]:
            rec = {"id": d["id"], "since": d["since"], "dependency": d["dependency"], "condition": d["condition"],
                   "approved_by": d["approved_by"], "why": d["why"], "outcome": None, "message": ""}
            deferred.append(rec)
            if d["problems"]:
                rec["outcome"], rec["message"] = "rejected", "; ".join(d["problems"])
                continue
            cond = DEPENDENCIES[d["dependency"]]["condition"]
            found = results.latest(cond["job"], cond["args"])
            if found is None:
                rec["outcome"] = "unchecked"
                rec["message"] = "no `tools/mg %s` result; run it on the gate commit" % cond["job"]
                continue
            path, r = found
            commit = r.get("commit") if isinstance(r.get("commit"), dict) else {}
            if commit.get("dirty") is not False or not isinstance(commit.get("sha"), str):
                rec["outcome"], rec["message"] = "unchecked", "the %s result is not from a clean commit" % cond["job"]
                continue
            seen.append(("deferred %s (%s)" % (d["id"], cond["job"]), commit["sha"]))
            rec["condition_result"] = {"result": path, "run_id": r.get("run_id")}
            detail = get_path(r.get("details") or {}, cond["detail"])
            if isinstance(detail, dict) and all(detail.get(k) == v for k, v in cond["equals"].items()):
                rec["outcome"], rec["message"] = "deferred", "%s still reports %s" % (cond["job"], json.dumps(detail))
                in_effect.append((d, rec))
                continue
            cells = assess(d["cells"])
            rec["cells"] = cells
            if cells["outcome"] == "pass":
                rec["outcome"], rec["message"] = "resolved", "the condition no longer holds and its cells pass"
            else:
                rec["outcome"] = "fail"
                rec["message"] = "the condition no longer holds (%s reports %s) and its cells are not green: %s %s" % (
                    cond["job"], json.dumps(detail), cells["outcome"], cells["message"])

    suites = [assess(s) for s in cumulative(model, mi)]

    # Global suite conditions read results too: assess them before fixing the
    # gate commit.
    globals_ = []
    for g in model["global"]:
        rec = {"id": g["id"], "kind": g["kind"], "from": g["from"], "outcome": None, "message": "", "why": g["why"]}
        if mi < order.index(g["from"]):
            rec["outcome"], rec["message"] = "not-applicable", "applies from %s" % g["from"]
        elif g["pending"]:
            rec["outcome"], rec["message"] = "pending", "its check lands with %s" % g["pending"]
        elif g["kind"] == "suite":
            s = assess(g["suite"])
            rec["outcome"], rec["message"], rec["attached"] = s["outcome"], s["message"], {
                k: s[k] for k in ("run", "job", "args", "result", "run_id", "commit", "status", "summary")}
        globals_.append(rec)

    # One gate commit for every result.
    commits = sorted({c for _, c in seen})
    gate = commits[0] if len(commits) == 1 else None
    if len(commits) > 1:
        by = {}
        for what, c in seen:
            by.setdefault(c, []).append(what)
        fail("results-commit", "results not from one commit: " + "; ".join(
            "%s on %s" % (", ".join(by[c]), c[:12]) for c in commits))
    evidence_commits = []
    evidence_paths = list(model["evidence_paths"]) + [st["evidence"] for m in model["milestones"] for st in m["user_steps"]]

    def is_evidence(p):
        return any(p.startswith(e) if e.endswith("/") else p == e for e in evidence_paths)

    if gate:
        if not git.is_ancestor(gate, head):
            fail("gate-commit", "the gate commit %s is not HEAD (%s) or an ancestor of it" % (gate[:12], head[:12]))
        else:
            other = [p for p in git.changed(gate, head) if not is_evidence(p)]
            if other:
                fail("after-gate", "commits after the gate commit %s change %d paths outside the evidence paths: %s"
                     % (gate[:12], len(other), ", ".join(other[:10]) + (" ..." if len(other) > 10 else "")))
            evidence_commits = git.commits(gate, head)

    for s in suites:
        if s["outcome"] == "awaiting_user":
            awaiting.append({"id": "suite:" + s["id"], "message": s["message"]})
        elif s["outcome"] not in ("pass", "deferred"):
            fail("suite:" + s["id"], "%s (%s): %s" % (s["id"], s["outcome"], s["message"]))
    for g in globals_:
        if g["outcome"] in ("pass", "deferred", "not-applicable", None):
            continue
        if g["outcome"] == "awaiting_user":
            awaiting.append({"id": "global:" + g["id"], "message": g["message"]})
        else:
            fail("global:" + g["id"], "%s (%s): %s" % (g["id"], g["outcome"], g["message"]))
    for d in deferred:
        if d["outcome"] not in ("deferred", "resolved"):
            fail("deferred:" + d["id"], "%s (%s): %s" % (d["id"], d["outcome"], d["message"]))

    # The quarantine (U19, R1953).
    day = today.isoformat()
    q = config.get("quarantine") or {"entries": [], "invalid": []}
    open_quarantine = [{k: e.get(k) for k in ("run", "test", "class", "owner", "deadline")}
                       for e in q.get("entries", []) if e.get("deadline", "") >= day]
    for g in globals_:
        if g["kind"] != "quarantine" or g["outcome"] is not None:
            continue
        problems_q = []
        for e in q.get("entries", []):
            if e.get("class") in PROTECTED:
                problems_q.append(("quarantine-protected:%s:%s" % (e["run"], e["test"]),
                                   "%s:%s is a quarantined %s test (owner %s); a milestone gate fails while any conformance, sync or data-safety test is quarantined (U19)"
                                   % (e["run"], e["test"], e["class"], e.get("owner"))))
            if e.get("deadline", "") < day:
                problems_q.append(("quarantine-expired:%s:%s" % (e["run"], e["test"]),
                                   "the quarantine of %s:%s (owner %s) ended on %s (R1953)"
                                   % (e["run"], e["test"], e.get("owner"), e.get("deadline"))))
        for inv in q.get("invalid", []):
            problems_q.append(("quarantine-invalid:%s" % inv.get("label"),
                               "%s: %s (R1953)" % (inv.get("label"), "; ".join(inv.get("errors") or []))))
        for fid, msg in problems_q:
            fail(fid, msg)
        g["outcome"] = "fail" if problems_q else "pass"
        g["message"] = "%d problems" % len(problems_q) if problems_q else "%d open entries, none protected" % len(open_quarantine)

    # Units: merged at or before the gate commit, no open escalation.
    ref = gate or head
    roles = git.unit_roles(ref)
    ledger_raw = git.show(head, LEDGER)
    open_escalations = {}
    try:
        open_escalations = parse_ledger(ledger_raw.decode("utf-8")) if ledger_raw is not None else {}
    except (UnicodeDecodeError, ValueError) as e:
        fail("escalations", "%s does not parse at HEAD (%s); the gate cannot tell which units are escalated" % (LEDGER, e))
    units = []
    for m in model["milestones"][:mi + 1]:
        for u in m["units"]:
            merged = bool(roles.get(u, set()) - set(NOT_MERGING))
            esc = sorted(e["id"] for e in open_escalations.values() if e.get("unit") == u)
            units.append({"id": u, "milestone": m["id"], "merged": merged, "open_escalations": esc})
            if not merged:
                fail("unit:" + u, "%s (%s) is not merged at or before %s: no commit with `Unit: %s` by an implementing role"
                     % (u, m["id"], "the gate commit" if gate else "HEAD", u))
            for eid in esc:
                e = open_escalations[eid]
                fail("escalation:%s:%s" % (u, eid), "%s has an open escalation %s (%s): %s"
                     % (u, eid, e.get("kind"), e.get("summary")))

    # User steps of this milestone.
    start = None
    if mi > 0:
        prev = git.show(head, "%s/%s.json" % (RECORDS, order[mi - 1]))
        try:
            closed = json.loads(prev.decode("utf-8")).get("closed_at") if prev is not None else None
            start = datetime.datetime.strptime(closed, "%Y-%m-%dT%H:%M:%SZ").date() if closed else None
        except (UnicodeDecodeError, ValueError, AttributeError, TypeError):
            fail("previous-record", "%s/%s.json does not parse" % (RECORDS, order[mi - 1]))
    doctor = None
    if a.doctor:
        try:
            doctor = read_json(a.doctor)
        except (OSError, ValueError):
            doctor = None

    def hint(path):
        return " (the working tree has an uncommitted change there; evidence counts once committed)" \
            if git.uncommitted(path) else ""

    catalog, catalog_error = None, "milestone_gate.py evaluate --catalog: perf_gate.py load's output"
    if a.catalog:
        try:
            catalog, _ = perf_gate.load_catalog(a.catalog)
        except (perf_gate.Invalid, OSError, ValueError) as e:
            catalog_error = "%s: %s" % (a.catalog, e)

    ctx = {"git": git, "head": head, "gate": gate, "milestone": a.milestone, "mi": mi, "model": model,
           "today": today, "start": start, "doctor": doctor, "hint": hint, "catalog": catalog,
           "catalog_error": catalog_error, "results": results}
    steps = []
    for st in milestone["user_steps"]:
        outcome, message, details = VALIDATORS[st["validator"]["type"]](ctx, st, st["validator"])
        steps.append({"id": st["id"], "type": st["validator"]["type"], "description": st["description"],
                      "evidence": st["evidence"], "outcome": outcome, "message": message, "details": details})
        if outcome == "awaiting_user":
            awaiting.append({"id": "user-step:" + st["id"], "message": "%s: %s" % (st["id"], message)})
        elif outcome != "pass":
            fail("user-step:" + st["id"], "%s: %s" % (st["id"], message))

    if a.outside_main:
        # A unit branch carries its own `Unit:` trailers: its units look
        # merged there. Only the integration branch closes a milestone.
        fail("checkout", "a milestone closes only in the main checkout, on the integration branch (where "
                         "tools/mg milestone runs); in this checkout the branch's own commits count as merged")
    outcome = "fail" if failures else ("awaiting_user" if awaiting else "pass")
    evaluation = {
        "milestone": a.milestone, "name": milestone["name"], "outcome": outcome, "evaluated_at": now,
        "gate_commit": gate, "head": head, "evidence_commits": evidence_commits,
        "window_start": start.isoformat() if start else None,
        "suites": suites, "units": units, "user_steps": steps, "deferred": deferred, "global": globals_,
        "quarantine": open_quarantine, "failures": failures, "awaiting": awaiting, "record": None,
    }
    if outcome == "pass" and a.record:
        if os.path.exists(a.record):
            evaluation["record"] = {"path": a.record, "written": False, "note": "a record exists; left unchanged"}
        else:
            write_json(a.record, {
                "record_version": RECORD_VERSION, "milestone": a.milestone, "name": milestone["name"],
                "closed_at": now, "gate_commit": gate, "head": head, "evidence_commits": evidence_commits,
                "suites": [{k: s[k] for k in ("id", "run", "job", "args", "since", "run_id", "status", "summary",
                                              "outcome")} for s in suites],
                "units": [u["id"] for u in units],
                "user_steps": [{k: s[k] for k in ("id", "type", "evidence", "outcome", "message", "details")}
                               for s in steps],
                "quarantine": open_quarantine,
                "deferred": [{k: d.get(k) for k in ("id", "since", "dependency", "condition", "approved_by",
                                                     "why", "outcome", "message")} for d in deferred],
                "global": [{k: g.get(k) for k in ("id", "kind", "outcome", "message", "attached") if k in g}
                           for g in globals_],
            })
            evaluation["record"] = {"path": a.record, "written": True}
    return evaluation


def cmd_evaluate(a):
    try:
        datetime.datetime.strptime(a.now, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        print(json.dumps({"errors": ["--now must be YYYY-MM-DDTHH:MM:SSZ"]}))
        return 2
    try:
        manifest = read_json(a.manifest)
        config = read_json(a.config)
    except (OSError, ValueError) as e:
        print(json.dumps({"errors": ["cannot read the input: %s" % e]}))
        return 2
    # Deferral problems fail the gate (evaluate), not the manifest.
    model, errors, _ = build_model(manifest, config)
    if errors:
        print(json.dumps({"errors": errors}, ensure_ascii=False))
        return 2
    if a.milestone not in model["order"]:
        print(json.dumps({"errors": ["unknown milestone %r; the milestones of the manifest are %s"
                                     % (a.milestone, ", ".join(model["order"]))]}, ensure_ascii=False))
        return 2
    try:
        evaluation = evaluate(model, config, a)
    except Invalid as e:
        print(json.dumps({"errors": [str(e)]}))
        return 2
    write_json(a.out, evaluation)
    return {"pass": 0, "fail": 1, "awaiting_user": 3}[evaluation["outcome"]]


def main(argv):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="command", required=True)
    s = sub.add_parser("load")
    s.add_argument("--manifest", required=True)
    s.add_argument("--root", help="compile first-sync definitions from committed evidence in this checkout")
    s.add_argument("--out", required=True)
    s = sub.add_parser("check")
    s.add_argument("--manifest", required=True, help="the manifest as JSON (load's output)")
    s.add_argument("--config", required=True, help="gate.py load's output")
    s.add_argument("--root", required=True, help="the checkout (the plan is read from it)")
    s = sub.add_parser("evaluate")
    s.add_argument("--manifest", required=True, help="the manifest as JSON (load's output)")
    s.add_argument("--config", required=True, help="gate.py load's output: tiers runs and the quarantine")
    s.add_argument("--milestone", required=True)
    s.add_argument("--root", required=True, help="the checkout: git history and evidence at HEAD")
    s.add_argument("--results", required=True, help="the job results directory (build/mg)")
    s.add_argument("--now", required=True, help="YYYY-MM-DDTHH:MM:SSZ")
    s.add_argument("--doctor", help="a tools/mg doctor result.json, for setup steps")
    s.add_argument("--catalog", help="perf_gate.py load's output (tools/perf/budgets.yaml), for device runs")
    s.add_argument("--record", help="where a pass writes the milestone record (unless one exists)")
    s.add_argument("--outside-main", action="store_true",
                   help="the checkout is not the main checkout: evaluate, but the gate fails")
    s.add_argument("--out", required=True)
    s = sub.add_parser("runs")
    s.add_argument("--manifest", required=True, help="the manifest as JSON (load's output)")
    s.add_argument("--config", required=True, help="gate.py load's output: the tiers runs")
    s.add_argument("--milestone", required=True)
    a = p.parse_args(argv)
    return {"load": cmd_load, "check": cmd_check, "evaluate": cmd_evaluate, "runs": cmd_runs}[a.command](a)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
