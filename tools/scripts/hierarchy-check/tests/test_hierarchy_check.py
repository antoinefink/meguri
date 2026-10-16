"""Tests for hierarchy_check (U8, R1964, R1965).

Synthetic trees pin each rule; the fixtures under fixtures/ are real exports
from AgentDriverTests (see fixtures/README.md) and pin the rules against what
XCUITest actually reports.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(os.path.dirname(HERE), "hierarchy_check.py")
FIXTURES = os.path.join(HERE, "fixtures")
sys.path.insert(0, os.path.dirname(HERE))

import hierarchy_check  # noqa: E402


def frame(x, y, w, h):
    return {"x": x, "y": y, "width": w, "height": h}


def el(type_, ident="", label="", f=(0, 0, 10, 10), children=()):
    return {
        "type": type_,
        "identifier": ident,
        "label": label,
        "value": None,
        "frame": frame(*f),
        "hit_point": None,
        "enabled": True,
        "selected": False,
        "focused": False,
        "children": list(children),
    }


def doc(*children, window=(0, 0, 400, 800)):
    return {
        "schema": 1,
        "platform": "ios",
        "captured_at": 0,
        "window": frame(*window),
        "screenshot": {"origin": {"x": 0, "y": 0}, "scale": 3},
        "root": el("application", label="Meguri", f=window, children=[el("window", f=window, children=children)]),
    }


def kinds(issues):
    return sorted(i["kind"] for i in issues)


class MissingIdentifier(unittest.TestCase):
    def test_a_button_without_identifier_is_reported_with_its_frame_and_label(self):
        issues = hierarchy_check.check(doc(el("button", label="Save", f=(10, 20, 80, 44))))
        self.assertEqual(kinds(issues), ["missing-identifier"])
        self.assertEqual(issues[0]["type"], "button")
        self.assertEqual(issues[0]["label"], "Save")
        self.assertEqual(issues[0]["frame"], frame(10, 20, 80, 44))

    def test_every_interactive_type_and_list_row_needs_one(self):
        for t in ["textField", "switch", "toggle", "slider", "link", "segmentedControl", "cell", "tableRow", "outlineRow"]:
            with self.subTest(type=t):
                self.assertEqual(kinds(hierarchy_check.check(doc(el(t, f=(0, 0, 50, 20))))), ["missing-identifier"])

    def test_a_list_row_is_identified_by_its_content(self):
        # Seen in real exports of SwiftUI lists: the Mac's outline row and
        # cell, and the iPad's cell, wrap the row's content, which carries
        # the identifier the row was given.
        mac_row = el("outlineRow", f=(0, 82, 144, 32), children=[el("cell", f=(10, 82, 124, 32), children=[
            el("staticText", "shell.root.sidebar.home", f=(19, 89, 64, 17))])])
        ipad_row = el("cell", f=(16, 486, 288, 52), children=[el("other", f=(16, 486, 288, 52), children=[
            el("staticText", "shell.root.sidebar.browse", "Browse", f=(16, 486, 288, 52)), el("image", "magnifyingglass", f=(33, 500, 21, 21))])])
        self.assertEqual(hierarchy_check.check(doc(el("outline", f=(0, 82, 144, 300), children=[mac_row]), ipad_row)), [])

    def test_a_row_whose_content_has_no_identifier_of_the_apps_is_reported(self):
        # A symbol's name on the row's image is the system's, not the row's.
        row = el("cell", f=(16, 86, 288, 52), children=[
            el("staticText", label="Home", f=(16, 86, 288, 52)), el("image", "house", f=(31, 100, 26, 23))])
        issues = hierarchy_check.check(doc(row))
        self.assertEqual([(i["kind"], i["type"]) for i in issues], [("missing-identifier", "cell")])

    def test_static_text_images_and_containers_need_none(self):
        tree = doc(el("other", children=[el("staticText", label="Hello", f=(0, 0, 50, 20)), el("image", f=(0, 30, 20, 20))]))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_system_alert_buttons_are_accepted(self):
        # System alerts drop their buttons' identifiers (app/AGENTS.md).
        alert = el("alert", label="Sample error", f=(50, 300, 300, 200), children=[
            el("staticText", label="Sample error", f=(60, 310, 280, 20)),
            el("button", label="Try Again", f=(60, 440, 130, 44)),
            el("button", label="Cancel", f=(200, 440, 130, 44)),
        ])
        self.assertEqual(hierarchy_check.check(doc(alert)), [])

    def test_an_app_sheet_without_any_identifier_is_not_a_system_alert(self):
        # No identifier at all is no evidence of a system alert: an app sheet
        # whose controls and rows all lack identifiers reports each of them.
        sheet = el("sheet", f=(100, 100, 400, 300), children=[
            el("staticText", label="Pick a deck", f=(120, 120, 200, 20)),
            el("button", f=(120, 340, 100, 24)),
            el("outline", f=(120, 150, 360, 180), children=[
                el("outlineRow", f=(120, 150, 360, 24)),
                el("outlineRow", f=(120, 174, 360, 24)),
            ]),
        ])
        issues = hierarchy_check.check(doc(sheet, window=(0, 0, 800, 600)))
        self.assertEqual([(i["kind"], i["type"], i["frame"]) for i in issues], [
            ("missing-identifier", "button", frame(120, 340, 100, 24)),
            ("missing-identifier", "outlineRow", frame(120, 150, 360, 24)),
            ("missing-identifier", "outlineRow", frame(120, 174, 360, 24)),
        ])

    def test_an_app_sheet_whose_only_identifier_is_an_appkit_one_is_not_a_system_alert(self):
        sheet = el("sheet", f=(100, 100, 400, 300), children=[
            el("textField", "_NS:12", f=(120, 120, 200, 22)),
            el("button", label="Add", f=(120, 340, 100, 24)),
        ])
        issues = hierarchy_check.check(doc(sheet, window=(0, 0, 800, 600)))
        self.assertEqual([(i["kind"], i["label"]) for i in issues], [("missing-identifier", "Add")])

    def test_a_mac_alert_sheet_whose_buttons_dropped_their_identifiers_is_a_system_alert(self):
        # AppKit's id on the sheet itself marks the alert (app/AGENTS.md).
        alert = el("sheet", "_NS:87", "alert", f=(100, 100, 260, 160), children=[
            el("staticText", label="Sample error", f=(120, 120, 200, 20)),
            el("button", label="Try Again", f=(120, 220, 100, 24)),
        ])
        self.assertEqual(hierarchy_check.check(doc(alert)), [])

    def test_a_mac_alert_sheet_with_only_system_identifiers_is_a_system_alert(self):
        # As the Mac reports a SwiftUI alert: AppKit ids on the sheet and its
        # texts, action-button-<n> on the buttons.
        alert = el("sheet", "_NS:87", "alert", f=(100, 100, 260, 122), children=[
            el("staticText", "_NS:74", f=(120, 120, 218, 18)),
            el("button", "action-button-2", "Cancel", f=(115, 177, 112, 30)),
            el("button", "action-button-1", "Try Again", f=(233, 177, 112, 30)),
        ])
        self.assertEqual(hierarchy_check.check(doc(alert)), [])

    def test_an_app_sheet_with_identifiers_still_needs_them_on_every_control(self):
        sheet = el("sheet", f=(100, 100, 400, 300), children=[
            el("button", "sample.sheet.done", "Done", f=(120, 120, 60, 24)),
            el("button", label="Close", f=(200, 120, 60, 24)),
        ])
        issues = hierarchy_check.check(doc(sheet, window=(0, 0, 800, 600)))
        self.assertEqual(kinds(issues), ["missing-identifier"])
        self.assertEqual(issues[0]["label"], "Close")

    def test_keyboard_status_bar_and_menu_bar_are_system_ui(self):
        tree = doc(
            el("keyboard", f=(0, 500, 400, 300), children=[el("key", label="q", f=(0, 500, 40, 40)), el("button", label="return", f=(300, 700, 90, 40))]),
            el("menuBar", f=(0, 0, 400, 24), children=[el("menuBarItem", label="File", f=(0, 0, 40, 24))]),
        )
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_the_split_views_sidebar_toggle_is_the_systems(self):
        # Seen in real exports: the Mac toolbar's toggle and the iPad
        # sidebar's, in its navigation bar; neither takes an identifier.
        tree = doc(el("toolbar", f=(0, 30, 800, 52), children=[el("button", label="Hide Sidebar", f=(92, 30, 44, 52))]),
                   el("navigationBar", f=(0, 32, 320, 54), children=[el("button", label="Show Sidebar", f=(270, 36, 36, 36))]))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_a_sidebar_label_outside_a_bar_or_another_bar_button_still_needs_an_identifier(self):
        tree = doc(el("button", label="Hide Sidebar", f=(0, 100, 44, 44)),
                   el("toolbar", f=(0, 30, 800, 52), children=[el("button", label="Add", f=(700, 30, 44, 52))]),
                   el("navigationBar", f=(0, 32, 320, 54), children=[el("other", f=(0, 32, 320, 54), children=[
                       el("button", label="Hide Sidebar", f=(270, 36, 36, 36))])]))
        self.assertEqual([i["label"] for i in hierarchy_check.check(tree)], ["Hide Sidebar", "Add", "Hide Sidebar"])

    def test_zero_size_controls_are_not_on_screen_and_skipped(self):
        self.assertEqual(hierarchy_check.check(doc(el("button", label="hidden", f=(0, 0, 0, 0)))), [])


class IdentifierFormat(unittest.TestCase):
    def test_identifiers_follow_area_screen_element_and_an_optional_id(self):
        good = ["home.deckList.row", "home.deckList.row.1700000000000", "review.answer.again", "sample.sheet.timeZone",
                "browser.table.row.note-12_a"]
        for ident in good:
            with self.subTest(ident=ident):
                self.assertEqual(hierarchy_check.check(doc(el("button", ident, f=(0, 0, 40, 40)))), [])

    def test_other_identifiers_are_reported(self):
        for ident in ["saveButton", "home.save", "Home.list.row", "home.list.Row", "home.list.row.", "home..row", "home list row"]:
            with self.subTest(ident=ident):
                issues = hierarchy_check.check(doc(el("button", ident, f=(0, 0, 40, 40))))
                self.assertEqual(kinds(issues), ["bad-identifier"])
                self.assertEqual(issues[0]["identifier"], ident)

    def test_system_identifiers_are_not_the_apps(self):
        # Seen in real exports: Mac window buttons, SwiftUI's navigation bar,
        # back button and toolbar, and the iPad form sheet's dismiss region.
        tree = doc(el("button", "_XCUI:CloseWindow", f=(5, 5, 14, 14)), el("button", "BackButton", "Back", f=(0, 50, 60, 44)),
                   el("navigationBar", "_TtGC7SwiftUI32NavigationStackHosting", f=(0, 50, 400, 54)),
                   el("toolbar", "Toolbar", f=(0, 0, 400, 800)), el("other", "PopoverDismissRegion", f=(-400, -800, 1200, 2400)))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_a_duplicated_identifier_is_reported_once_with_every_frame(self):
        tree = doc(el("button", "home.list.add", f=(0, 0, 40, 40)), el("button", "home.list.add", f=(0, 100, 40, 40)),
                   el("staticText", "home.list.add", "Add", f=(0, 200, 40, 20)))
        issues = hierarchy_check.check(tree)
        self.assertEqual(kinds(issues), ["duplicate-identifier"])
        self.assertEqual(issues[0]["identifier"], "home.list.add")
        self.assertEqual(issues[0]["frames"], [frame(0, 0, 40, 40), frame(0, 100, 40, 40), frame(0, 200, 40, 20)])

    def test_mac_scene_ids_and_the_navigation_back_button_are_not_the_apps(self):
        # A window carries its SwiftUI scene id ("main"); the Mac back button
        # of a NavigationStack carries its symbol name.
        tree = {
            "schema": 1, "platform": "mac", "captured_at": 0, "window": frame(0, 30, 800, 600),
            "screenshot": {"origin": {"x": 0, "y": 30}, "scale": 2},
            "root": el("application", f=(0, 30, 800, 600), children=[el("window", "main", f=(0, 30, 800, 600), children=[
                el("toolbar", f=(0, 30, 800, 52), children=[el("button", "chevron.backward", "Back", f=(92, 30, 44, 52))]),
            ])]),
        }
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_ios_names_navigation_bars_after_their_title_and_symbol_images_after_the_symbol(self):
        # Seen in real exports: UIKit's navigation bar takes its title as its
        # identifier, an SF Symbol's image its name, on every image of it.
        tree = doc(el("navigationBar", "Spanish::Mining", f=(0, 62, 400, 106), children=[
                       el("staticText", label="Spanish::Mining", f=(16, 119, 200, 40))]),
                   el("image", "house", "Home", f=(10, 200, 40, 40)), el("image", "gearshape.fill", "Gear", f=(60, 200, 40, 40)),
                   el("image", "rectangle.stack", f=(10, 300, 20, 20)), el("image", "rectangle.stack", f=(10, 340, 20, 20)))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_a_navigation_bar_named_otherwise_and_an_images_own_identifier_are_still_checked(self):
        tree = doc(el("navigationBar", "Home", f=(0, 62, 400, 106), children=[el("staticText", label="Decks", f=(16, 119, 200, 40))]),
                   el("image", "home.deckIcon", f=(10, 200, 40, 40)), el("button", "gearshape", "Settings", f=(60, 200, 40, 40)))
        issues = hierarchy_check.check(tree)
        self.assertEqual([(i["kind"], i["identifier"]) for i in issues],
                         [("bad-identifier", "Home"), ("bad-identifier", "home.deckIcon"), ("bad-identifier", "gearshape")])

    def test_a_mac_split_view_and_its_list_column_are_named_by_swiftui(self):
        # Seen in real exports: NavigationSplitView's split group after the
        # window's scene id, and the one column of a SwiftUI List.
        tree = doc(el("splitGroup", "main, SidebarNavigationSplitView", f=(0, 30, 800, 600), children=[
            el("outline", label="Sidebar", f=(0, 82, 144, 548), children=[el("tableColumn", "ListColumn", f=(10, 82, 124, 548))])]))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_those_names_on_other_elements_are_still_reported(self):
        tree = doc(el("group", "main, SidebarNavigationSplitView", f=(0, 30, 800, 600)), el("button", "ListColumn", f=(0, 0, 40, 40)))
        self.assertEqual(kinds(hierarchy_check.check(tree)), ["bad-identifier", "bad-identifier"])

    def test_alert_button_ids_outside_a_sheet_and_symbol_ids_elsewhere_are_still_reported(self):
        tree = doc(el("button", "action-button-1", "Save", f=(0, 0, 40, 40)), el("button", "chevron.backward", "Back", f=(0, 50, 40, 40)))
        self.assertEqual(kinds(hierarchy_check.check(tree)), ["bad-identifier", "bad-identifier"])


class TextOverlap(unittest.TestCase):
    def test_two_overlapping_labels_are_reported_with_both_frames(self):
        tree = doc(el("staticText", "sample.defects.first", "First", f=(16, 100, 140, 20)),
                   el("staticText", "sample.defects.second", "Second", f=(56, 106, 150, 20)))
        issues = hierarchy_check.check(tree)
        self.assertEqual(kinds(issues), ["text-overlap"])
        self.assertEqual(issues[0]["frames"], [frame(16, 100, 140, 20), frame(56, 106, 150, 20)])
        self.assertEqual(issues[0]["labels"], ["First", "Second"])

    def test_touching_or_barely_overlapping_frames_are_not_an_overlap(self):
        tree = doc(el("staticText", label="A", f=(0, 0, 100, 20)), el("staticText", label="B", f=(0, 20, 100, 20)),
                   el("staticText", label="C", f=(0, 39.5, 100, 20)))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_a_spinner_over_a_label_is_transient_and_ignored(self):
        tree = doc(el("staticText", "sample.defects.waiting", "Waiting", f=(16, 200, 100, 20)),
                   el("activityIndicator", "sample.defects.spinner", "In progress", f=(40, 196, 28, 28),
                      children=[el("staticText", label="Loading", f=(30, 205, 60, 16))]))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_text_inside_text_is_one_element_not_an_overlap(self):
        tree = doc(el("staticText", label="Title", f=(0, 0, 100, 20), children=[el("staticText", label="Title", f=(0, 0, 100, 20))]))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_text_behind_a_mac_sheet_belongs_to_another_window(self):
        sheet = el("sheet", f=(100, 50, 400, 300), children=[el("staticText", "sample.sheet.title", "Sample sheet", f=(120, 70, 120, 20))])
        tree = doc(el("staticText", "home.placeholder.title", "Meguri", f=(110, 60, 200, 40)), sheet, window=(0, 0, 800, 600))
        self.assertEqual(hierarchy_check.check(tree), [])


class TextOutsideWindow(unittest.TestCase):
    def test_text_cut_by_the_window_edge_is_reported(self):
        issues = hierarchy_check.check(doc(el("staticText", "home.header.title", "A very long title", f=(300, 10, 200, 20))))
        self.assertEqual(kinds(issues), ["text-outside-window"])
        self.assertEqual(issues[0]["frame"], frame(300, 10, 200, 20))
        self.assertEqual(issues[0]["window"], frame(0, 0, 400, 800))

    def test_text_scrolled_out_of_a_scroll_view_is_not(self):
        tree = doc(el("scrollView", f=(0, 0, 400, 800), children=[el("staticText", label="Row 40", f=(16, 1600, 100, 20))]))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_text_inside_a_sheet_is_measured_against_the_sheet(self):
        sheet = el("sheet", f=(100, 50, 400, 300), children=[el("staticText", label="Spills", f=(450, 70, 120, 20))])
        issues = hierarchy_check.check(doc(sheet, window=(0, 0, 800, 600)))
        self.assertEqual(kinds(issues), ["text-outside-window"])
        self.assertEqual(issues[0]["window"], frame(100, 50, 400, 300))


class Input(unittest.TestCase):
    def test_a_document_that_is_not_a_hierarchy_export_is_an_error(self):
        for bad in [{}, {"schema": 2, "root": {}}, {"schema": 1, "root": {"type": "application"}}]:
            with self.subTest(bad=bad):
                with self.assertRaises(hierarchy_check.InputError):
                    hierarchy_check.check(bad)


class RealExports(unittest.TestCase):
    """Exports captured by AgentDriverTests from the test-hook routes."""

    def load(self, name):
        with open(os.path.join(FIXTURES, name)) as f:
            return json.load(f)

    def test_the_sample_sheet_on_iphone_has_no_issue(self):
        self.assertEqual(hierarchy_check.check(self.load("ios-sample-sheet.json")), [])

    def test_the_sample_routes_on_the_mac_have_no_issue(self):
        for name in ["mac-sample-screen.json", "mac-sample-sheet.json", "mac-sample-alert.json"]:
            with self.subTest(name=name):
                self.assertEqual(hierarchy_check.check(self.load(name)), [])

    def test_the_shell_on_iphone_and_ipad_has_no_issue(self):
        # The tab bar with its buttons' identifiers and symbol images, the
        # iPad sidebar's rows, toggle and navigation bars named by UIKit.
        for name in ["ios-shell-typical.json", "ipad-shell-typical.json"]:
            with self.subTest(name=name):
                self.assertEqual(hierarchy_check.check(self.load(name)), [])

    def test_the_defects_route_on_iphone_reports_its_two_defects_and_not_the_spinner(self):
        issues = hierarchy_check.check(self.load("ios-sample-defects.json"))
        self.assertEqual(kinds(issues), ["missing-identifier", "text-overlap"])
        missing = next(i for i in issues if i["kind"] == "missing-identifier")
        self.assertEqual((missing["type"], missing["label"]), ("button", "Unidentified action"))
        overlap = next(i for i in issues if i["kind"] == "text-overlap")
        self.assertEqual(overlap["identifiers"], ["sample.defects.first", "sample.defects.second"])


class CommandLine(unittest.TestCase):
    def run_cli(self, *args):
        return subprocess.run([sys.executable, SCRIPT, *args], capture_output=True, text=True)

    def test_it_writes_a_report_and_exits_1_when_there_are_issues(self):
        with tempfile.TemporaryDirectory() as tmp:
            clean = os.path.join(tmp, "clean.json")
            bad = os.path.join(tmp, "bad.json")
            report = os.path.join(tmp, "report.json")
            with open(clean, "w") as f:
                json.dump(doc(el("button", "home.list.add", f=(0, 0, 40, 40))), f)
            with open(bad, "w") as f:
                json.dump(doc(el("button", label="Add", f=(0, 0, 40, 40))), f)
            result = self.run_cli("--report", report, clean, bad)
            self.assertEqual(result.returncode, 1, result.stderr)
            with open(report) as f:
                data = json.load(f)
            self.assertEqual([(e["path"], kinds(e["issues"])) for e in data["files"]], [(clean, []), (bad, ["missing-identifier"])])
            self.assertEqual(data["counts"], {"files": 2, "issues": 1, "missing-identifier": 1})
            self.assertIn("missing-identifier", result.stderr)

            result = self.run_cli("--report", report, clean)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_unreadable_input_exits_2(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "x.json")
            with open(path, "w") as f:
                f.write("not json")
            self.assertEqual(self.run_cli(path).returncode, 2)


if __name__ == "__main__":
    unittest.main()
