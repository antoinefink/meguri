#!/usr/bin/env python3
"""Deterministic layout and identifier checks over hierarchy exports (U8).

Reads the JSON that AgentDriverTests writes before and after every drive step
(app/MeguriUITests/AgentDriver/HierarchyExport.swift) and reports:

- missing-identifier: an interactive control or a list row with no
  accessibility identifier (R1964). A row counts as identified when an
  element inside it has one: SwiftUI's List puts a row's modifiers on its
  content, inside the row and cell views AppKit and UIKit wrap it in, and no
  API names those;
- bad-identifier: an identifier that is not area.screen.element[.id], each of
  area, screen and element lowerCamel (R1964);
- duplicate-identifier: one identifier on several elements;
- text-overlap: two text elements of one window whose visible frames overlap
  (R1965); a text inside a scrolling container shows only where that
  container does, so a row scrolled past its edge overlaps nothing there;
- text-outside-window: a text element that is not wholly inside its window or
  sheet and not inside a scrolling container (R1965).

Ignored: system UI (keyboard, status bar, menu bar, Touch Bar, and a Mac
scroll bar with its knob, arrows and page regions), the buttons of
system alerts (which drop identifiers, or on the Mac carry action-button-<n>;
app/AGENTS.md; a Mac alert sheet is told by AppKit's id on the sheet or an
action-button-<n>, never by a lack of identifiers), the sidebar toggle a
split view puts in the toolbar or navigation bar (no identifier; labelled
Show or Hide Sidebar, so only in English, the driver's usual locale),
identifiers the system sets (a leading '_', BackButton, Toolbar,
PopoverDismissRegion, the Mac back button's chevron.backward in a toolbar, a
window's SwiftUI scene id, a navigation bar named after its title, an SF
Symbol's name on its image, and on the Mac a SwiftUI List's ListColumn and a
NavigationSplitView's '<scene id>, SidebarNavigationSplitView'), elements
with an empty frame, and transient elements (activity and progress
indicators) with everything inside them. Wrong colors, broken alignment and
unreadable text are judgments for the critic (U10), not checks here.

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
# A Mac scroll view's AppKit scroller reports its parts (the page regions
# as unlabelled buttons), none of which can take an identifier.
SYSTEM_UI = {"keyboard", "statusBar", "menuBar", "touchBar", "scrollBar"}
SCROLLING = {"scrollView", "table", "collectionView", "outline", "browser", "webView"}
WINDOWS = {"window", "sheet", "popover", "alert"}
SYSTEM_IDENTIFIERS = {"BackButton", "Toolbar", "PopoverDismissRegion"}
# The Mac's alert buttons, inside a sheet or alert.
ALERT_BUTTON = re.compile(r"^action-button-[0-9]+$")
# The name iOS gives the image of an SF Symbol: lowercase words and digits
# joined by dots (house, gearshape.fill, chart.bar.xaxis).
SYMBOL_NAME = re.compile(r"^[a-z0-9]+(\.[a-z0-9]+)*$")
# SwiftUI's NavigationSplitView on the Mac, after the window's scene id.
SPLIT_VIEW = re.compile(r"^(?:.+, )?SidebarNavigationSplitView$")
SIDEBAR_TOGGLE_LABELS = {"Show Sidebar", "Hide Sidebar"}
BARS = {"toolbar", "navigationBar"}

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

    def shown(self):
        """The part of the frame its scrolling containers show, as (x, y, w,
        h), or None when they show none of it."""
        x0, y0, x1, y1 = self.x, self.y, self.x + self.w, self.y + self.h
        for a in self.ancestors:
            if a.type in SCROLLING:
                x0, y0, x1, y1 = max(x0, a.x), max(y0, a.y), min(x1, a.x + a.w), min(y1, a.y + a.h)
        if x1 <= x0 or y1 <= y0:
            return None
        return x0, y0, x1 - x0, y1 - y0

    def under(self, types):
        return self.type in types or any(a.type in types for a in self.ancestors)

    def window(self):
        """The nearest window-like ancestor (window, sheet, popover, alert)."""
        for a in reversed(self.ancestors):
            if a.type in WINDOWS:
                return a
        return None

    def in_system_alert(self):
        """Inside a system alert: an alert, or (Mac) an alert sheet."""
        return any(a.type == "alert" or (a.type == "sheet" and is_alert_sheet(a)) for a in self.ancestors)

    def identified_inside(self):
        """An element inside this one carries an identifier the app set."""
        return any(d.identifier and not is_system_identifier(d) for c in self.children for d in c.walk())

    def is_sidebar_toggle(self):
        """The toggle a split view adds to its bar (a Mac toolbar, an iPad
        navigation bar), which has no identifier and none can be set."""
        return (self.type == "button" and not self.identifier and self.label in SIDEBAR_TOGGLE_LABELS
                and self.parent is not None and self.parent.type in BARS)


def is_alert_sheet(sheet):
    """A Mac system alert shown as a sheet: positive evidence (AppKit's id on
    the sheet itself, or an action-button-<n> button) and no identifier the
    app set. An app sheet has neither mark, even when none of its elements
    has an identifier."""
    marked = (sheet.identifier and is_system_identifier(sheet)) or any(
        e.type == "button" and ALERT_BUTTON.match(e.identifier) for e in sheet.walk())
    return bool(marked) and all(is_system_identifier(e) for e in sheet.walk() if e.identifier)


def is_system_identifier(e):
    """An identifier the system set, not the app."""
    ident = e.identifier
    if ident.startswith("_") or ident in SYSTEM_IDENTIFIERS:
        return True
    if e.type == "window":
        return True
    if ALERT_BUTTON.match(ident):
        return e.type == "button" and any(a.type in ("sheet", "alert") for a in e.ancestors)
    if ident == "chevron.backward":
        return e.type == "button" and e.parent is not None and e.parent.type == "toolbar"
    if e.type == "navigationBar":
        # UIKit names a navigation bar after its title.
        return any(c.type == "staticText" and c.label == ident for c in e.children)
    if e.type == "image":
        return bool(SYMBOL_NAME.match(ident))
    if e.type == "tableColumn":
        return ident == "ListColumn"
    if e.type == "splitGroup":
        return bool(SPLIT_VIEW.match(ident))
    return False


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
        if ((e.type in INTERACTIVE or e.type in ROWS) and not e.identifier and e.visible and not e.in_system_alert()
                and not e.is_sidebar_toggle() and not (e.type in ROWS and e.identified_inside())):
            issues.append(dict(kind="missing-identifier", message="%s '%s' has no accessibility identifier" % (e.type, e.label),
                               **describe(e)))

    by_identifier = {}
    for e in elements:
        if not e.identifier or is_system_identifier(e):
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
    shown = [(e, e.shown()) for e in texts]
    shown = [(e, r) for e, r in shown if r]
    for i, (a, (ax, ay, aw, ah)) in enumerate(shown):
        for b, (bx, by, bw, bh) in shown[i + 1:]:
            if a in b.ancestors or b in a.ancestors or a.window() is not b.window():
                continue
            dx = min(ax + aw, bx + bw) - max(ax, bx)
            dy = min(ay + ah, by + bh) - max(ay, by)
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
