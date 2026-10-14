#!/usr/bin/env python3
"""The screen status ledger, docs/design/screen-status.yaml (U10; U254
operates it): the only place a screen's status lives (R1985, R1987).

Statuses: sketch -> built -> verified -> approved.
  built     the screen exists and has snapshots
  verified  its latest snapshot and accessibility runs passed and the latest
            critic check left no high-severity issue (R1985)
  approved  verified, plus Antoine's logged pick (docs/design/decisions.md)
            and gallery approval of its baselines (signature screens, R1988)

A high-severity critic issue moves a verified or approved screen back to
built; medium issues become tasks; low ones stay listed for the gallery.
Only this script writes the ledger (tools/mg critic and gallery call it).

  screen_status.py check [--ledger L] [--inventory I]
  screen_status.py init [--ledger L] [--inventory I]       add inventory screens missing from the ledger
  screen_status.py inventory-json [--inventory I] [--out F] the inventory as JSON
  screen_status.py record-critic --screen S --report R [--ledger L] [--inventory I]
  screen_status.py transition --screen S --to STATUS --by WHO [--ledger L] [--inventory I]

A ledger other than the canonical path may hold sample screens (`sample.*`,
test-hook samples, never in the inventory); the canonical ledger may not.
"""

import argparse
import datetime
import json
import os
import re
import sys

import yaml

LEDGER = "docs/design/screen-status.yaml"
INVENTORY = "docs/design/screen-inventory.yaml"
DECISIONS = "docs/design/decisions.md"
STATUSES = ["sketch", "built", "verified", "approved"]
SEVERITIES = ["high", "medium", "low"]
SCREEN_ID = re.compile(r"^[a-z][A-Za-z0-9]*\.[a-z][A-Za-z0-9]*$")

HEADER = """\
# Screen status ledger (U10 created it, U254 operates it; R1985, R1987).
# The only place a screen's status lives: sketch, built, verified, approved.
# Written only by tools/scripts/screen-status/screen_status.py, through
# tools/mg critic and tools/mg gallery; never edit it by hand.
#
# Per screen (ids from screen-inventory.yaml):
#   status      sketch | built | verified | approved
#   fix_rounds  agent render-and-fix rounds since the last merge (R1987, U254)
#   evidence    the runs behind the status: snapshots, a11y, lint, critic
#               (run id, status, commit)
#   issues      open critic issues from the latest check (screen, variant,
#               rubric rule, severity, region)
#   tasks       medium critic issues to work on (R1985)
#   history     status changes: date, from, to, by, reason
"""


class LedgerError(Exception):
    pass


def now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def load_inventory(path=INVENTORY):
    with open(path) as handle:
        data = yaml.safe_load(handle) or {}
    screens = {}
    for entry in data.get("screens") or []:
        screens[entry["id"]] = {"states": list(entry.get("states") or []), "signature": bool(entry.get("signature"))}
    return screens


def load(path=LEDGER):
    if not os.path.exists(path):
        return {"version": 1, "screens": {}}
    with open(path) as handle:
        data = yaml.safe_load(handle) or {}
    data.setdefault("version", 1)
    data["screens"] = data.get("screens") or {}
    return data


def dump(ledger, path=LEDGER):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    body = yaml.safe_dump(ledger, sort_keys=False, allow_unicode=True, width=100, default_flow_style=False)
    tmp = path + ".tmp"
    with open(tmp, "w") as handle:
        handle.write(HEADER + "\n" + body)
    os.replace(tmp, path)


def is_canonical(path):
    """Whether `path` is the checkout's own ledger (run from the checkout's root)."""
    return os.path.realpath(path) == os.path.realpath(LEDGER)


def entry_for(ledger, screen):
    entry = ledger["screens"].setdefault(screen, {"status": "sketch", "fix_rounds": 0})
    entry.setdefault("status", "sketch")
    entry.setdefault("fix_rounds", 0)
    return entry


def check_screen_id(screen, inventory, canonical):
    if screen.startswith("sample."):
        if canonical:
            raise LedgerError("%s is a sample screen: it never goes in %s (pass --ledger)" % (screen, LEDGER))
        return
    if screen not in inventory:
        raise LedgerError("%s is not in screen-inventory.yaml" % screen)


def problems(ledger, inventory, canonical):
    """Everything wrong with a ledger, one line each."""
    found = []
    if ledger.get("version") != 1:
        found.append("version must be 1")
    if canonical:
        for screen in inventory:
            if screen not in ledger["screens"]:
                found.append("%s is in screen-inventory.yaml but not in %s (tools/mg design-docs --write adds it)" % (screen, LEDGER))
    for screen, entry in ledger["screens"].items():
        try:
            check_screen_id(screen, inventory, canonical)
        except LedgerError as error:
            found.append(str(error))
        if not isinstance(entry, dict):
            found.append("%s: not a mapping" % screen)
            continue
        status = entry.get("status")
        if status not in STATUSES:
            found.append("%s: status %r is not one of %s" % (screen, status, ", ".join(STATUSES)))
        if not isinstance(entry.get("fix_rounds", 0), int) or entry.get("fix_rounds", 0) < 0:
            found.append("%s: fix_rounds must be a whole number" % screen)
        highs = [i for i in entry.get("issues") or [] if i.get("severity") == "high"]
        if status in ("verified", "approved") and highs:
            found.append("%s: %s with %d open high-severity critic issue(s)" % (screen, status, len(highs)))
        if status in ("verified", "approved"):
            missing = [g for g in ("snapshots", "a11y", "critic") if (entry.get("evidence") or {}).get(g, {}).get("status") != "pass"]
            if missing:
                found.append("%s: %s without passing %s evidence" % (screen, status, ", ".join(missing)))
    return found


def history(entry, to, by, reason):
    if entry["status"] == to:
        return
    entry.setdefault("history", []).append({"date": now(), "from": entry["status"], "to": to, "by": by, "reason": reason})
    entry["status"] = to


def verified_ready(entry):
    """Why the screen cannot be verified yet; empty when it can."""
    reasons = []
    evidence = entry.get("evidence") or {}
    for gate in ("snapshots", "a11y"):
        status = (evidence.get(gate) or {}).get("status")
        if status != "pass":
            reasons.append("%s: %s" % (gate, status or "no run"))
    highs = [i for i in entry.get("issues") or [] if i.get("severity") == "high"]
    if highs:
        reasons.append("%d high-severity critic issue(s) open" % len(highs))
    if (evidence.get("critic") or {}).get("status") != "pass":
        reasons.append("no passing critic check")
    return reasons


def record_critic(ledger, screen, report, by="tools/mg critic"):
    """Records one critic check of `screen` (report: run, session, commit,
    evidence{snapshots, a11y, lint}, issues[standing], dropped[]) and moves
    the status. Returns the new status."""
    entry = entry_for(ledger, screen)
    issues = [i for i in report.get("issues") or [] if i["screen"] == screen]
    highs = [i for i in issues if i["severity"] == "high"]
    evidence = entry.setdefault("evidence", {})
    for gate, value in (report.get("evidence") or {}).items():
        if value:
            evidence[gate] = value
    evidence["critic"] = {
        "run": report["run"],
        "session": report.get("session"),
        "commit": report.get("commit"),
        "status": "fail" if highs else "pass",
        "high": len(highs),
        "medium": sum(1 for i in issues if i["severity"] == "medium"),
        "low": sum(1 for i in issues if i["severity"] == "low"),
        "dropped": len([d for d in report.get("dropped") or [] if d["screen"] == screen]),
        "date": now(),
    }
    entry["issues"] = [
        {k: i[k] for k in ("state", "variant", "rule", "severity", "region", "summary", "image") if k in i} | {"run": report["run"]}
        for i in issues
    ]
    if not entry["issues"]:
        entry.pop("issues")
    # One task per rule and image: a re-check words the same problem
    # differently, so the latest wording replaces the earlier one.
    tasks = entry.setdefault("tasks", [])
    for issue in issues:
        if issue["severity"] != "medium":
            continue
        key = (issue["rule"], issue.get("state"), issue.get("variant"))
        task = {"rule": issue["rule"], "state": issue.get("state"), "variant": issue.get("variant"),
                "summary": issue["summary"], "from": report["run"], "open": True}
        for n, existing in enumerate(tasks):
            if (existing["rule"], existing.get("state"), existing.get("variant")) == key:
                tasks[n] = dict(task, since=existing.get("since", existing.get("from")))
                break
        else:
            tasks.append(dict(task, since=report["run"]))
    if not tasks:
        entry.pop("tasks")
    if highs:
        if entry["status"] in ("verified", "approved", "sketch"):
            history(entry, "built", by, "critic run %s: %d high-severity issue(s): %s" % (
                report["run"], len(highs), "; ".join("%s (%s)" % (i["rule"], i["variant"]) for i in highs)))
    else:
        if entry["status"] == "sketch":
            history(entry, "built", by, "critic run %s graded its snapshots" % report["run"])
        if entry["status"] == "built" and not verified_ready(entry):
            history(entry, "verified", by, "critic run %s: no high-severity issue; snapshots and accessibility passed" % report["run"])
    return entry["status"]


def logged_pick(screen, decisions=DECISIONS):
    if not os.path.exists(decisions):
        return False
    with open(decisions) as handle:
        return any(line.startswith("|") and ("| %s |" % screen) in line for line in handle)


def transition(ledger, screen, to, by, approved_images=0, decisions=DECISIONS):
    """An explicit transition, checked against the rules above."""
    if to not in STATUSES:
        raise LedgerError("unknown status %r" % to)
    entry = entry_for(ledger, screen)
    if to == "verified":
        reasons = verified_ready(entry)
        if reasons:
            raise LedgerError("%s cannot be verified: %s" % (screen, "; ".join(reasons)))
    if to == "approved":
        if entry["status"] != "verified":
            raise LedgerError("%s is %s; only a verified screen can be approved" % (screen, entry["status"]))
        if not logged_pick(screen, decisions):
            raise LedgerError("%s has no pick in decisions.md (R1988)" % screen)
        if approved_images < 1:
            raise LedgerError("%s has no gallery-approved baseline" % screen)
    history(entry, to, by, "explicit transition")
    return entry["status"]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["check", "init", "inventory-json", "record-critic", "transition"])
    parser.add_argument("--ledger", default=LEDGER)
    parser.add_argument("--inventory", default=INVENTORY)
    parser.add_argument("--screen")
    parser.add_argument("--report")
    parser.add_argument("--to")
    parser.add_argument("--by", default="tools/mg")
    parser.add_argument("--approved-images", type=int, default=0)
    parser.add_argument("--out", help="inventory-json: write here instead of stdout")
    args = parser.parse_args(argv)
    inventory = load_inventory(args.inventory)
    if args.command == "inventory-json":
        text = json.dumps({"screens": [{"id": k, **v} for k, v in inventory.items()]}, indent=2) + "\n"
        if args.out:
            with open(args.out, "w") as handle:
                handle.write(text)
        else:
            sys.stdout.write(text)
        return 0
    canonical = is_canonical(args.ledger)
    ledger = load(args.ledger)
    try:
        if args.command == "check":
            found = problems(ledger, inventory, canonical)
            for line in found:
                print(line)
            return 1 if found else 0
        if args.command == "init":
            for screen in inventory:
                entry_for(ledger, screen)
            dump(ledger, args.ledger)
            return 0
        if not args.screen:
            raise LedgerError("--screen is required")
        check_screen_id(args.screen, inventory, canonical)
        if args.command == "record-critic":
            with open(args.report) as handle:
                report = json.load(handle)
            status = record_critic(ledger, args.screen, report)
        else:
            status = transition(ledger, args.screen, args.to, args.by, args.approved_images)
        dump(ledger, args.ledger)
        print(json.dumps({"screen": args.screen, "status": status}))
        return 0
    except LedgerError as error:
        print("screen-status: %s" % error, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
