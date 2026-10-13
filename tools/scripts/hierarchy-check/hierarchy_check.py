#!/usr/bin/env python3
"""Deterministic layout and identifier checks over hierarchy exports (U8).

Reads the JSON that AgentDriverTests writes before and after every drive step
(app/MeguriUITests/AgentDriver/HierarchyExport.swift) and reports:

- missing-identifier: an interactive control or a list row with no
  accessibility identifier (R1964);
- bad-identifier: an identifier that is not area.screen.element[.id], each of
  area, screen and element lowerCamel (R1964);
- duplicate-identifier: one identifier on several elements;
- text-overlap: two text elements of one window whose frames overlap (R1965);
- text-outside-window: a text element that is not wholly inside its window or
  sheet and not inside a scrolling container (R1965).

Ignored: system UI (keyboard, status bar, menu bar, Touch Bar), the buttons of
system alerts (which drop identifiers; app/AGENTS.md), identifiers the system
sets (a leading '_', BackButton, Toolbar, PopoverDismissRegion), elements with an empty frame, and transient
elements (activity and progress indicators) with everything inside them. Wrong
colors, broken alignment and unreadable text are judgments for the critic
(U10), not checks here.

usage: hierarchy_check.py [--report <report.json>] <hierarchy.json>...
Exit code: 0 no issue, 1 issues, 2 unreadable input. Issues also go to stderr.
"""

import argparse
import json
import re
import sys

INTERACTIVE = {
    "button", "radioButton", "checkBox", "disclosureTriangle", "popUpButton", "comboBox", "menuButton",
    "toolbarButton", "link", "slider", "segmentedControl", "picker", "pickerWheel", "switch", "toggle",
    "textField", "secureTextField", "searchField", "textView", "datePicker", "stepper", "colorWell",
    "incrementArrow", "decrementArrow", "tab",
}
ROWS = {"cell", "tableRow", "outlineRow"}
TEXT = {"staticText", "textField", "secureTextField", "searchField", "textView"}
TRANSIENT = {"activityIndicator", "progressIndicator"}
SYSTEM_UI = {"keyboard", "statusBar", "menuBar", "touchBar"}
SCROLLING = {"scrollView", "table", "collectionView", "outline", "browser", "webView"}
WINDOWS = {"window", "sheet", "popover", "alert"}
SYSTEM_IDENTIFIERS = {"BackButton", "Toolbar", "PopoverDismissRegion"}

IDENTIFIER = re.compile(r"^[a-z][a-zA-Z0-9]*\.[a-z][a-zA-Z0-9]*\.[a-z][a-zA-Z0-9]*(\.[A-Za-z0-9_-]+)?$")
# Frames that overlap by at most this much (points) in either direction only
# touch: text line boxes often share a fraction of a point.
OVERLAP_TOLERANCE = 1.0
EDGE_TOLERANCE = 0.5


class InputError(Exception):
    pass


class Element:
    def __init__(self, raw, parent, index):
        if not isinstance(raw, dict) or not isinstance(raw.get("type"), str) or not isinstance(raw.get("frame"), dict):
            raise InputError("an element needs a type and a frame")
        self.raw = raw
        self.parent = parent
        self.type = raw["type"]
        self.identifier = raw.get("identifier") or ""
        self.label = raw.get("label") or ""
        f = raw["frame"]
        try:
            self.x, self.y, self.w, self.h = (float(f[k]) for k in ("x", "y", "width", "height"))
        except (KeyError, TypeError, ValueError):
            raise InputError("a frame needs numeric x, y, width and height")
        self.path = (parent.path + "/" if parent else "") + "%s[%d]" % (self.type, index)
        self.ancestors = (parent.ancestors + [parent]) if parent else []
        children = raw.get("children", [])
        if not isinstance(children, list):
            raise InputError("children must be a list")
        self.children = [Element(c, self, i) for i, c in enumerate(children)]

    def walk(self):
        yield self
        for child in self.children:
            yield from child.walk()

    @property
    def frame(self):
        return self.raw["frame"]

    @property
    def visible(self):
        return self.w > 0 and self.h > 0

    def under(self, types):
        return self.type in types or any(a.type in types for a in self.ancestors)

    def window(self):
        """The nearest window-like ancestor (window, sheet, popover, alert)."""
        for a in reversed(self.ancestors):
            if a.type in WINDOWS:
                return a
        return None

    def in_system_alert(self):
        """Inside a system alert: an alert, or (Mac) a sheet with no identifier anywhere in it."""
        for a in self.ancestors:
            if a.type == "alert":
                return True
            if a.type == "sheet" and not any(e.identifier for e in a.walk()):
                return True
        return False


def is_system_identifier(identifier):
    return identifier.startswith("_") or identifier in SYSTEM_IDENTIFIERS


def describe(e):
    return {"type": e.type, "identifier": e.identifier, "label": e.label, "frame": e.frame, "path": e.path}


def check(document):
    """The issues in one hierarchy export, as a list of dicts with a `kind`."""
    if not isinstance(document, dict) or document.get("schema") != 1 or not isinstance(document.get("root"), dict):
        raise InputError("not a hierarchy export (schema 1 with a root element)")
    root = Element(document["root"], None, 0)
    elements = [e for e in root.walk() if not e.under(SYSTEM_UI) and not e.under(TRANSIENT)]
    issues = []

    for e in elements:
        if (e.type in INTERACTIVE or e.type in ROWS) and not e.identifier and e.visible and not e.in_system_alert():
            issues.append(dict(kind="missing-identifier", message="%s '%s' has no accessibility identifier" % (e.type, e.label),
                               **describe(e)))

    by_identifier = {}
    for e in elements:
        if not e.identifier or is_system_identifier(e.identifier):
            continue
        if not IDENTIFIER.match(e.identifier):
            issues.append(dict(kind="bad-identifier",
                               message="'%s' is not area.screen.element[.id] (lowerCamel parts)" % e.identifier, **describe(e)))
        by_identifier.setdefault(e.identifier, []).append(e)
    for identifier, group in by_identifier.items():
        if len(group) > 1:
            issues.append({
                "kind": "duplicate-identifier",
                "identifier": identifier,
                "message": "'%s' is on %d elements" % (identifier, len(group)),
                "frames": [g.frame for g in group],
                "types": [g.type for g in group],
                "paths": [g.path for g in group],
            })

    texts = [e for e in elements if e.type in TEXT and e.visible]
    for i, a in enumerate(texts):
        for b in texts[i + 1:]:
            if a in b.ancestors or b in a.ancestors or a.window() is not b.window():
                continue
            dx = min(a.x + a.w, b.x + b.w) - max(a.x, b.x)
            dy = min(a.y + a.h, b.y + b.h) - max(a.y, b.y)
            if dx > OVERLAP_TOLERANCE and dy > OVERLAP_TOLERANCE:
                issues.append({
                    "kind": "text-overlap",
                    "message": "'%s' and '%s' overlap by %gx%g pt" % (a.label, b.label, round(dx, 2), round(dy, 2)),
                    "labels": [a.label, b.label],
                    "identifiers": [a.identifier, b.identifier],
                    "frames": [a.frame, b.frame],
                    "paths": [a.path, b.path],
                })

    for e in texts:
        window = e.window()
        if window is None or e.under(SCROLLING):
            continue
        t = EDGE_TOLERANCE
        inside = (e.x >= window.x - t and e.y >= window.y - t and
                  e.x + e.w <= window.x + window.w + t and e.y + e.h <= window.y + window.h + t)
        if not inside:
            issues.append(dict(kind="text-outside-window", window=window.frame,
                               message="'%s' is not wholly inside its %s" % (e.label, window.type), **describe(e)))
    return issues


def main(argv):
    parser = argparse.ArgumentParser(description="Layout and identifier checks over hierarchy exports (U8).")
    parser.add_argument("--report", help="write the JSON report here")
    parser.add_argument("files", nargs="+")
    args = parser.parse_args(argv)
    files, counts = [], {"files": 0, "issues": 0}
    for path in args.files:
        try:
            with open(path) as f:
                issues = check(json.load(f))
        except (OSError, ValueError, InputError) as error:
            print("hierarchy-check: %s: %s" % (path, error), file=sys.stderr)
            return 2
        files.append({"path": path, "issues": issues})
        counts["files"] += 1
        for issue in issues:
            counts["issues"] += 1
            counts[issue["kind"]] = counts.get(issue["kind"], 0) + 1
            print("%s: %s: %s" % (path, issue["kind"], issue["message"]), file=sys.stderr)
    if args.report:
        with open(args.report, "w") as f:
            json.dump({"files": files, "counts": counts}, f, indent=2, sort_keys=True)
    return 1 if counts["issues"] else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
