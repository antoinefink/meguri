#!/usr/bin/env python3
"""The critic's packet and the checks on its report (U10, R1983-R1986), in
the image-tools container. tools/mg critic runs the session itself.

  critic.py packet --snapshots RUN_DIR --out DIR [--screen ID]... [--drive RUN_DIR]
                   [--checks FILE] [--host-root PATH] [--run ID]
      builds the packet: the screens' snapshot images with their variants,
      contact sheets, the rubric, the brief, reference images, calibration
      examples, the output schema and packet.json; exit 1 when a screen has
      no image
  critic.py validate --packet DIR --output FILE --report FILE
      reads the session's output (Claude's JSON result), checks it against
      the schema, the rubric and the packet, and drops issues a passing
      deterministic check contradicts (R1986); exit 1, and no report, when
      the output is invalid
  critic.py apply --report FILE --ledger FILE [--inventory FILE] [--evidence JSON]
                  [--session ID] [--commit SHA]
      records the report in the screen status ledger (screen_status.py);
      evidence is {screen: {snapshots|a11y|lint: {run, status, commit}}}
"""

import argparse
import glob
import json
import os
import re
import shutil
import struct
import sys

import jsonschema
import yaml

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "screen-status"))

import screen_status  # noqa: E402

DESIGN = "docs/design"
RUBRIC = os.path.join(DESIGN, "rubric.md")
BRIEF = os.path.join(DESIGN, "brief.md")
SCHEMA = os.path.join(DESIGN, "critic-issue.schema.json")
DECISIONS = os.path.join(DESIGN, "decisions.md")
CALIBRATION = os.path.join(DESIGN, "critic-calibration")
REFERENCES = os.path.join(DESIGN, "references")
ROLE = "docs/process/roles/critic.md"
INVENTORY = os.path.join(DESIGN, "screen-inventory.yaml")
# Owners whose result is the source of truth for a rule (rubric.md, R1986).
DETERMINISTIC_OWNERS = ("lint:", "test:", "audit", "touch-targets", "hierarchy", "contact-sheet", "states", "truncation check")


class PacketError(Exception):
    pass


# ---------------------------------------------------------------------------
# The rubric

def rubric(path=RUBRIC):
    """{"version": int, "rules": {id: {"owner": text, "deterministic": bool}}}.
    Retired rules are left out: an issue may not cite them."""
    with open(path) as handle:
        text = handle.read()
    match = re.search(r"^Version (\d+)", text, re.M)
    if not match:
        raise PacketError("%s has no 'Version N' line" % path)
    rules = {}
    section = text.split("\n## Rules", 1)[-1].split("\n## Lint rule index", 1)[0]
    for line in section.splitlines():
        row = re.match(r"^\| `([a-z][a-z0-9-]*)` \|(.*)$", line)
        if not row:
            continue
        cells = [c.strip() for c in row.group(2).split("|")]
        if any("retired" in c.lower() for c in cells):
            continue
        owner = cells[1] if len(cells) > 1 else ""
        rules[row.group(1)] = {"owner": owner, "deterministic": any(o in owner for o in DETERMINISTIC_OWNERS)}
    return {"version": int(match.group(1)), "rules": rules}


# ---------------------------------------------------------------------------
# The packet

def to_container(path, host_root):
    """A host path under the checkout as this container sees it."""
    if host_root and path.startswith(host_root.rstrip("/") + "/"):
        return os.path.join(os.getcwd(), path[len(host_root.rstrip("/")) + 1:])
    return path


def read_results(run_dir):
    path = os.path.join(run_dir, "results.jsonl")
    if not os.path.exists(path):
        raise PacketError("%s has no results.jsonl: not a tools/mg snapshots run" % run_dir)
    with open(path) as handle:
        return [json.loads(line) for line in handle if line.strip()]


def screen_images(results, screens, host_root):
    """Per screen, the latest image of every rendered cell: the baseline when
    it passed, the new render otherwise."""
    chosen = {}
    for entry in results:
        subject = entry.get("subject", "")
        # Components, the settings probe, test swatches and card faces (their
        # DOM and pixels are the rendering checks' business) are no screens.
        if subject.startswith(("designsystem.", "matrix.", "test.", "cardface.")) or entry.get("kind") == "text":
            continue
        if screens and subject not in screens:
            continue
        status = entry.get("status")
        if status == "pass":
            source, path = "baseline", entry.get("baseline")
        elif status in ("recorded", "new", "fail"):
            source, path = "render", entry.get("actual")
        else:
            continue
        if not path:
            continue
        path = to_container(path, host_root)
        if not os.path.exists(path):
            raise PacketError("%s: image %s is missing" % (entry["name"], path))
        chosen[entry["name"]] = (entry, source, path)
    return chosen


def build_packet(args):
    out = args.out
    if os.path.exists(out):
        shutil.rmtree(out)
    os.makedirs(out)
    inventory = yaml.safe_load(open(INVENTORY)) or {}
    by_id = {s["id"]: s for s in inventory.get("screens") or []}
    results = read_results(args.snapshots)
    chosen = screen_images(results, set(args.screen or []), args.host_root)
    screens = sorted({entry["subject"] for entry, _, _ in chosen.values()})
    missing = [s for s in args.screen or [] if s not in screens]
    if missing or not screens:
        raise PacketError("no snapshot image for %s in %s" % (", ".join(missing) or "any screen", args.snapshots))
    images = []
    for name in sorted(chosen):
        entry, source, path = chosen[name]
        rel = os.path.join("screens", name + ".png")
        os.makedirs(os.path.dirname(os.path.join(out, rel)), exist_ok=True)
        shutil.copyfile(path, os.path.join(out, rel))
        width, height = png_size(path)
        images.append({
            "path": rel, "screen": entry["subject"], "state": entry["state"], "variant": entry["cell"],
            "platform": entry.get("platform"), "axes": entry.get("axes", {}), "layout": entry.get("layout"),
            "width_px": width, "height_px": height, "scale": 2, "source": source,
            "notes": platform_notes(entry.get("platform")),
        })
    sheets = []
    if args.drive:
        for sheet in sorted(glob.glob(os.path.join(args.drive, "**", "*.png"), recursive=True)):
            if "/motion/" not in sheet and "contact" not in os.path.basename(sheet):
                continue
            rel = os.path.join("contact-sheets", os.path.basename(sheet))
            os.makedirs(os.path.join(out, "contact-sheets"), exist_ok=True)
            shutil.copyfile(sheet, os.path.join(out, rel))
            sheets.append(rel)
    references = []
    patterns = sorted({p for s in screens for p in (by_id.get(s, {}).get("references") or [])})
    os.makedirs(os.path.join(out, "references"))
    shutil.copyfile(os.path.join(REFERENCES, "index.md"), os.path.join(out, "references", "index.md"))
    for pattern in patterns:
        for ref in sorted(glob.glob(os.path.join(REFERENCES, pattern + "-*.png"))):
            shutil.copyfile(ref, os.path.join(out, "references", os.path.basename(ref)))
            references.append(os.path.join("references", os.path.basename(ref)))
    os.makedirs(os.path.join(out, "calibration"))
    calibration = []
    for source in [DECISIONS] + sorted(glob.glob(os.path.join(CALIBRATION, "*"))):
        if os.path.isfile(source):
            shutil.copyfile(source, os.path.join(out, "calibration", os.path.basename(source)))
            calibration.append(os.path.join("calibration", os.path.basename(source)))
    for source, name in ((RUBRIC, "rubric.md"), (BRIEF, "brief.md"), (SCHEMA, "schema.json"), (ROLE, "role.md")):
        shutil.copyfile(source, os.path.join(out, name))
    checks = []
    if args.checks:
        with open(args.checks) as handle:
            checks = json.load(handle).get("checks", [])
    manifest = {
        "run": args.run,
        "rubric_version": rubric()["version"],
        "snapshots_run": os.path.basename(os.path.normpath(args.snapshots)),
        "screens": [
            {"id": s, "title": by_id.get(s, {}).get("title", s), "signature": bool(by_id.get(s, {}).get("signature")),
             "purpose": by_id.get(s, {}).get("purpose", "a sample screen of the verification tooling"),
             "references": by_id.get(s, {}).get("references") or []}
            for s in screens
        ],
        "images": images,
        "contact_sheets": sheets,
        "references": references,
        "calibration": calibration,
        "checks": checks,
        "files": {"role": "role.md", "rubric": "rubric.md", "brief": "brief.md", "schema": "schema.json"},
    }
    with open(os.path.join(out, "packet.json"), "w") as handle:
        json.dump(manifest, handle, indent=2)
    return manifest


def png_size(path):
    """A PNG's width and height in pixels, from its IHDR chunk."""
    with open(path, "rb") as handle:
        head = handle.read(24)
    if len(head) < 24 or head[:8] != b"\x89PNG\r\n\x1a\n" or head[12:16] != b"IHDR":
        raise PacketError("%s is not a PNG" % path)
    return struct.unpack(">II", head[16:24])


def platform_notes(platform):
    if platform == "mac":
        return ("Mac snapshots render off-screen: Liquid Glass cannot be drawn there, so chrome is drawn flat on an opaque "
                "surface and the window looks inactive. Do not report missing glass or inactive styling on Mac images.")
    return "Rendered in the app's own window on the simulator, at 2x, with the status bar outside the image."


# ---------------------------------------------------------------------------
# The report

def parse_output(path):
    """The critic's JSON object from Claude's output (a result message, or
    the message list ending in one): `structured_output` when the session
    had a schema, else the result text (fences allowed)."""
    with open(path) as handle:
        raw = handle.read()
    try:
        outer = json.loads(raw)
    except ValueError as error:
        raise PacketError("the session output is not JSON: %s" % error)
    if isinstance(outer, list):
        # With a schema, the JSON output is the session's message list; the
        # answer is in its result message.
        results = [m for m in outer if isinstance(m, dict) and m.get("type") == "result"]
        if not results:
            raise PacketError("the session output has no result message")
        outer = results[-1]
    if isinstance(outer, dict) and outer.get("type") == "result":
        if outer.get("is_error"):
            raise PacketError("the critic session ended with an error: %s" % str(outer.get("result"))[:300])
        if isinstance(outer.get("structured_output"), dict):
            return outer["structured_output"]
        text = (outer.get("result") or "").strip()
        fenced = re.search(r"```(?:json)?\s*(\{.*\})\s*```", text, re.S)
        text = fenced.group(1) if fenced else text[text.find("{"): text.rfind("}") + 1]
        try:
            return json.loads(text)
        except ValueError as error:
            raise PacketError("the critic's answer is not a JSON object: %s" % error)
    if isinstance(outer, dict):
        return outer
    raise PacketError("the session output is not a JSON object")


def validate(packet_dir, output_path):
    """(report, errors): the standing and dropped issues, or why the output
    is not acceptable."""
    with open(os.path.join(packet_dir, "packet.json")) as handle:
        packet = json.load(handle)
    with open(os.path.join(packet_dir, "schema.json")) as handle:
        schema = json.load(handle)
    try:
        output = parse_output(output_path)
    except PacketError as error:
        return None, [str(error)]
    errors = ["schema: %s at %s" % (e.message, "/".join(str(p) for p in e.absolute_path) or "the top level")
              for e in sorted(jsonschema.Draft202012Validator(schema).iter_errors(output), key=lambda e: list(e.absolute_path))]
    if errors:
        return None, errors
    rules = rubric(os.path.join(packet_dir, "rubric.md"))
    if output["rubric_version"] != rules["version"]:
        errors.append("rubric_version %s; the packet's rubric is version %s" % (output["rubric_version"], rules["version"]))
    images = {i["path"]: i for i in packet["images"]}
    variants = {(i["screen"], i["variant"], i["state"]) for i in packet["images"]}
    screens = {s["id"] for s in packet["screens"]}
    for n, issue in enumerate(output["issues"], 1):
        where = "issue %d (%s, %s)" % (n, issue["screen"], issue["rule"])
        if issue["rule"] not in rules["rules"]:
            errors.append("%s: rule %s is not in rubric v%s" % (where, issue["rule"], rules["version"]))
        if issue["screen"] not in screens:
            errors.append("%s: screen %s is not in the packet" % (where, issue["screen"]))
        image = images.get(issue["image"])
        if image is None:
            errors.append("%s: image %s is not in the packet" % (where, issue["image"]))
            continue
        if (image["screen"], image["variant"], image["state"]) != (issue["screen"], issue["variant"], issue["state"]):
            errors.append("%s: image %s shows %s/%s/%s" % (where, issue["image"], image["screen"], image["state"], image["variant"]))
        if (issue["screen"], issue["variant"], issue["state"]) not in variants:
            errors.append("%s: no variant %s of state %s" % (where, issue["variant"], issue["state"]))
        region = issue["region"]
        if region["x"] + region["width"] > image["width_px"] + 1 or region["y"] + region["height"] > image["height_px"] + 1:
            errors.append("%s: region %s lies outside the %dx%d image" % (where, region, image["width_px"], image["height_px"]))
    graded = set(output.get("screens") or [])
    if not screens <= graded:
        errors.append("the report does not grade %s" % ", ".join(sorted(screens - graded)))
    if errors:
        return None, errors
    standing, dropped = [], []
    for issue in output["issues"]:
        check = deterministic_result(packet.get("checks") or [], issue)
        if rules["rules"][issue["rule"]]["deterministic"] and check == "pass":
            dropped.append(dict(issue, dropped="its deterministic check passed for this variant (R1986)"))
        else:
            standing.append(issue)
    return {
        "run": packet["run"],
        "rubric_version": rules["version"],
        "screens": sorted(screens),
        "issues": standing,
        "dropped": dropped,
        "notes": output.get("notes", ""),
    }, []


def deterministic_result(checks, issue):
    """The deterministic check's result for the issue's rule and variant:
    pass, fail, or None when no check covers that cell."""
    for check in checks:
        if check.get("rule") != issue["rule"] or check.get("screen") != issue["screen"]:
            continue
        if check.get("variant") in ("*", issue["variant"]):
            return check.get("status")
    return None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("packet")
    p.add_argument("--snapshots", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--screen", action="append")
    p.add_argument("--drive")
    p.add_argument("--checks")
    p.add_argument("--host-root", default="")
    p.add_argument("--run", default="")
    v = sub.add_parser("validate")
    v.add_argument("--packet", required=True)
    v.add_argument("--output", required=True)
    v.add_argument("--report", required=True)
    a = sub.add_parser("apply")
    a.add_argument("--report", required=True)
    a.add_argument("--ledger", default=screen_status.LEDGER)
    a.add_argument("--inventory", default=screen_status.INVENTORY)
    a.add_argument("--evidence", default="{}")
    a.add_argument("--session", default="")
    a.add_argument("--commit", default="")
    args = parser.parse_args(argv)
    try:
        if args.command == "packet":
            manifest = build_packet(args)
            print(json.dumps({"screens": [s["id"] for s in manifest["screens"]], "images": len(manifest["images"]),
                              "contact_sheets": len(manifest["contact_sheets"])}))
            return 0
        if args.command == "validate":
            report, errors = validate(args.packet, args.output)
            if errors:
                for error in errors:
                    print("critic output: %s" % error, file=sys.stderr)
                print(json.dumps({"valid": False, "errors": errors}))
                return 1
            with open(args.report, "w") as handle:
                json.dump(report, handle, indent=2)
            print(json.dumps({"valid": True, "issues": len(report["issues"]), "dropped": len(report["dropped"])}))
            return 0
        with open(args.report) as handle:
            report = json.load(handle)
        evidence = json.loads(args.evidence)
        report["session"] = args.session or None
        report["commit"] = args.commit or None
        inventory = screen_status.load_inventory(args.inventory)
        canonical = screen_status.is_canonical(args.ledger)
        ledger = screen_status.load(args.ledger)
        statuses = {}
        for screen in report["screens"]:
            screen_status.check_screen_id(screen, inventory, canonical)
            statuses[screen] = screen_status.record_critic(ledger, screen, dict(report, evidence=evidence.get(screen, {})))
        screen_status.dump(ledger, args.ledger)
        print(json.dumps({"statuses": statuses}))
        return 0
    except (PacketError, screen_status.LedgerError, OSError) as error:
        print("critic: %s" % error, file=sys.stderr)
        return 1 if isinstance(error, (PacketError, screen_status.LedgerError)) else 2


if __name__ == "__main__":
    sys.exit(main())
