#!/usr/bin/env python3
"""Screen inventory checks and the generated screens.md (U9, R2001).

docs/design/screen-inventory.yaml is the only screen inventory; screens.md is
its readable view and is never edited by hand. Status (sketch, built,
verified, approved) lives only in docs/design/screen-status.yaml (U10, U254),
so the inventory may not carry one.

usage: design-screens.py check|write [--root DIR]
  check   validate the inventory and fail when screens.md is not what it generates
  write   validate the inventory and rewrite screens.md

Exit status: 0 ok, 1 invalid inventory or stale screens.md, 2 usage.
"""

import argparse
import re
import sys
from pathlib import Path

import yaml

STATES = ["empty", "typical", "overflow", "error", "loading"]  # U10 PreviewStates
VARIANTS = {"full": "full covering set", "pairwise": "pairwise set"}  # U252
FIELDS = {"id", "title", "units", "signature", "platforms", "states", "variants", "references", "purpose"}
PLATFORMS = ["iphone", "ipad", "mac", "widget"]
ID = re.compile(r"[a-z][a-zA-Z0-9]*\.[a-z][a-zA-Z0-9]*")
UNIT = re.compile(r"U[0-9]+")
REFERENCE_ROW = re.compile(r"^\|\s*`([a-z0-9-]+)`\s*\|", re.M)


def problems_in(inventory, references):
    out = []
    if not isinstance(inventory, dict) or inventory.get("version") != 1 or not isinstance(inventory.get("screens"), list):
        return ["the inventory must be a mapping with version: 1 and a screens list"]
    extra = set(inventory) - {"version", "screens"}
    if extra:
        out.append(f"unknown top-level keys {sorted(extra)}")
    seen = set()
    for n, screen in enumerate(inventory["screens"], 1):
        where = f"screen {n}"
        if not isinstance(screen, dict):
            out.append(f"{where}: not a mapping")
            continue
        sid = screen.get("id")
        if isinstance(sid, str):
            where = sid
        if "status" in screen:
            out.append(f"{where}: status lives only in docs/design/screen-status.yaml")
        missing, unknown = FIELDS - set(screen), set(screen) - FIELDS - {"status"}
        if missing:
            out.append(f"{where}: missing {sorted(missing)}")
        if unknown:
            out.append(f"{where}: unknown keys {sorted(unknown)}")
        if not isinstance(sid, str) or not ID.fullmatch(sid):
            out.append(f"{where}: id must be area.screen, lowerCamel parts (R1964)")
        elif sid in seen:
            out.append(f"{where}: duplicate id")
        else:
            seen.add(sid)
        for key in ("title", "purpose"):
            if key in screen and not (isinstance(screen[key], str) and screen[key].strip()):
                out.append(f"{where}: {key} must be non-empty text")
        if "signature" in screen and not isinstance(screen["signature"], bool):
            out.append(f"{where}: signature must be true or false")
        units = screen.get("units", [])
        if not (isinstance(units, list) and units and all(isinstance(u, str) and UNIT.fullmatch(u) for u in units)):
            out.append(f"{where}: units must be a non-empty list of unit ids (U152)")
        platforms = screen.get("platforms", [])
        if not (isinstance(platforms, list) and platforms and set(platforms) <= set(PLATFORMS)):
            out.append(f"{where}: platforms must be a non-empty list from {PLATFORMS}")
        states = screen.get("states", [])
        if not (isinstance(states, list) and states and all(s in STATES for s in states) and len(set(states)) == len(states)):
            out.append(f"{where}: states must be a non-empty list from {STATES} (U10)")
        variants = screen.get("variants")
        if "variants" in screen and variants not in VARIANTS:
            out.append(f"{where}: variants must be one of {sorted(VARIANTS)}")
        if screen.get("signature") is True and variants != "full":
            out.append(f"{where}: a signature screen uses the full covering set (R1972)")
        refs = screen.get("references", [])
        if not isinstance(refs, list):
            out.append(f"{where}: references must be a list")
        else:
            for ref in refs:
                if ref not in references:
                    out.append(f"{where}: reference '{ref}' is not in docs/design/references/index.md")
    return out


def render(inventory):
    screens = inventory["screens"]
    lines = [
        "# Screens",
        "",
        "<!-- Generated from screen-inventory.yaml by `tools/mg design-docs --write`. Do not edit. -->",
        "",
        "The readable view of `screen-inventory.yaml`, the only screen inventory (R2001).",
        "Status (sketch, built, verified, approved) lives only in `screen-status.yaml`.",
        f"States come from the five-state vocabulary: {', '.join(STATES)}.",
        "Signature screens use the full snapshot covering set and get a variant pick (R1972, R1988);",
        "the others use the pairwise set (U252).",
        "",
        f"{len(screens)} screens, {sum(1 for s in screens if s['signature'])} signature.",
    ]
    areas = []
    for screen in screens:
        area = screen["id"].split(".")[0]
        if area not in areas:
            areas.append(area)
    for area in areas:
        lines += ["", f"## {area}", "", "| Screen | Id | Units | Platforms | States | Variants | References |", "|---|---|---|---|---|---|---|"]
        for s in (s for s in screens if s["id"].split(".")[0] == area):
            title = s["title"] + (" (signature)" if s["signature"] else "")
            refs = ", ".join(f"`{r}`" for r in s["references"]) or "none"
            lines.append(
                f"| {title} | `{s['id']}` | {', '.join(s['units'])} | {', '.join(s['platforms'])} | "
                f"{', '.join(s['states'])} | {VARIANTS[s['variants']]} | {refs} |"
            )
        lines.append("")
        for s in (s for s in screens if s["id"].split(".")[0] == area):
            lines.append(f"- `{s['id']}`: {' '.join(s['purpose'].split())}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description="Screen inventory checks and screens.md")
    parser.add_argument("mode", choices=["check", "write"])
    parser.add_argument("--root", default=".")
    args = parser.parse_args()
    design = Path(args.root) / "docs" / "design"
    try:
        inventory = yaml.safe_load((design / "screen-inventory.yaml").read_text(encoding="utf-8"))
    except (OSError, yaml.YAMLError) as error:
        print(f"screen-inventory.yaml: {error}", file=sys.stderr)
        return 1
    try:
        references = set(REFERENCE_ROW.findall((design / "references" / "index.md").read_text(encoding="utf-8")))
    except OSError as error:
        print(f"references/index.md: {error}", file=sys.stderr)
        return 1
    problems = problems_in(inventory, references)
    for problem in problems:
        print(f"screen-inventory.yaml: {problem}")
    if problems:
        return 1
    generated = render(inventory)
    target = design / "screens.md"
    if args.mode == "write":
        target.write_text(generated, encoding="utf-8")
        return 0
    current = target.read_text(encoding="utf-8") if target.exists() else ""
    if current != generated:
        print("screens.md: not what screen-inventory.yaml generates; run tools/mg design-docs --write")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
