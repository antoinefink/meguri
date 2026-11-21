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


class NativeReplayIdentity(unittest.TestCase):
    def fixture(self):
        link = el("link", label="Play audio", f=(20, 30, 44, 44))
        link.update(native_ax=True, ax_identifier="", ax_identifier_error=0, ax_identifier_wrong_type=False,
                    ax_dom_identifier="review.card.replay.q1", ax_dom_identifier_error=0,
                    ax_dom_identifier_wrong_type=False)
        page = el("webView", "review.card.page.a", f=(0, 0, 400, 800), children=[link])
        page["native_ax"] = True
        value = doc(page)
        value["platform"] = "mac"
        value["xcui_root"] = doc(el("webView", "review.card.page.a", f=(0, 0, 400, 800),
                                    children=[el("link", label="Play audio", f=(20, 30, 44, 44))]))["root"]
        binding = dict(page="review.card.page.a", document="synthetic-document", card=1,
                       generation=2, side="question", root="meguri-media://media/",
                       generated_ids=["review.card.replay.q1"])
        value.update(native_web_rule="render/display-transform#D4", native_web=binding,
                     native_web_binding_at_end=dict(binding))
        return value, page, link

    def test_exact_native_dom_identity_with_empty_raw_identifier(self):
        value, _, link = self.fixture()
        self.assertEqual(hierarchy_check.check(value), [])
        self.assertEqual(link["identifier"], "")
        self.assertEqual(link["ax_identifier"], "")
        self.assertEqual(value["xcui_root"]["children"][0]["children"][0]["children"][0]["identifier"], "")

    def test_missing_unsupported_wrong_type_and_duplicate_fail(self):
        mutations = [lambda p, e: e.pop("ax_dom_identifier"),
                     lambda p, e: e.update(ax_dom_identifier_error=-25205),
                     lambda p, e: e.update(ax_dom_identifier_wrong_type=True),
                     lambda p, e: e.update(ax_dom_identifier=42),
                     lambda p, e: p["children"].append(dict(e))]
        for mutation in mutations:
            value, page, link = self.fixture()
            mutation(page, link)
            self.assertIn("native-web-identity", kinds(hierarchy_check.check(value)))

    def test_stale_binding_and_untrusted_prefix_fail(self):
        for key, changed in [("document", "old"), ("card", 2), ("generation", 3), ("side", "answer"), ("page", "review.card.page.b")]:
            value, _, _ = self.fixture()
            value["native_web_binding_at_end"][key] = changed
            self.assertIn("native-web-identity", kinds(hierarchy_check.check(value)))
        value, _, _ = self.fixture()
        value["native_web"]["generated_ids"] = []
        value["native_web_binding_at_end"]["generated_ids"] = []
        self.assertIn("missing-identifier", kinds(hierarchy_check.check(value)))

    def test_no_field_alias_or_mobile_or_name_geometry_fallback(self):
        value, _, link = self.fixture()
        link["identifier"] = link["ax_dom_identifier"]
        self.assertIn("native-web-identity", kinds(hierarchy_check.check(value)))
        for platform in ["ios", "ipad"]:
            value, _, _ = self.fixture()
            value["platform"] = platform
            self.assertIn("native-web-identity", kinds(hierarchy_check.check(value)))
        value, _, link = self.fixture()
        del link["ax_dom_identifier"]
        self.assertIn("missing-identifier", kinds(hierarchy_check.check(value)))

    def test_other_identifier_and_geometry_rules_remain_active(self):
        value, page, _ = self.fixture()
        page["children"] += [el("button", label="Other", f=(100, 50, 44, 44)),
                             el("staticText", label="One", f=(0, 100, 80, 20)),
                             el("staticText", label="Two", f=(0, 100, 80, 20))]
        self.assertEqual(kinds(hierarchy_check.check(value)), ["missing-identifier", "text-overlap"])
        # An authored lookalike has no generated ownership, even with a DOM ID.
        authored = dict(page["children"][0])
        authored.update(ax_dom_identifier="review.card.replay.q0", label="Author & 猫")
        page["children"] = [page["children"][0], authored]
        self.assertEqual(kinds(hierarchy_check.check(value)), ["missing-identifier"])

    def test_original_xcui_overlap_and_raw_id_findings_survive_different_ax_tree(self):
        value, _, _ = self.fixture()
        original_page = value["xcui_root"]["children"][0]["children"][0]
        original_page["children"] += [
            el("staticText", "bad ID", "Original one", f=(10, 100, 80, 20)),
            el("staticText", "review.card.duplicate", "Original two", f=(10, 100, 80, 20)),
            el("button", "review.card.duplicate", "Original action", f=(200, 100, 40, 40)),
        ]
        issues = hierarchy_check.check(value)
        self.assertEqual(kinds(issues), ["bad-identifier", "duplicate-identifier", "text-overlap"])
        self.assertTrue(all(i["source"] == "xcui_root" for i in issues))
        overlap = next(i for i in issues if i["kind"] == "text-overlap")
        self.assertEqual(overlap["labels"], ["Original one", "Original two"])
        self.assertEqual(overlap["frames"], [frame(10, 100, 80, 20)] * 2)
        self.assertEqual(original_page["children"][0]["identifier"], "")

    def test_ax_and_xcui_diagnostics_stay_separate_without_leaf_mapping(self):
        value, page, link = self.fixture()
        original_page = value["xcui_root"]["children"][0]["children"][0]
        original_page["children"] += [el("staticText", label="Raw one", f=(10, 100, 80, 20)),
                                      el("staticText", label="Raw two", f=(10, 100, 80, 20))]
        page["children"] += [el("staticText", label="AX one", f=(10, 200, 80, 20)),
                             el("staticText", label="AX two", f=(10, 200, 80, 20)),
                             el("button", label="Still requires identity", f=(100, 300, 40, 40))]
        page["children"].append(el("link", label=link["label"], f=(20, 30, 44, 44)))
        issues = hierarchy_check.check(value)
        self.assertEqual(kinds(issues), ["missing-identifier", "missing-identifier", "text-overlap", "text-overlap"])
        self.assertEqual([i["source"] for i in issues if i["kind"] == "text-overlap"], ["root", "xcui_root"])
        self.assertTrue(all(i["source"] == "root" for i in issues if i["kind"] == "missing-identifier"))

    def test_raw_other_controls_and_unowned_pages_keep_missing_id_checks(self):
        value, _, _ = self.fixture()
        original_window = value["xcui_root"]["children"][0]
        original_window["children"][0]["children"].append(el("button", label="Raw unidentified action"))
        original_window["children"].append(el("webView", "review.card.otherPage", children=[
            el("link", label="Play audio", f=(20, 30, 44, 44))]))
        issues = hierarchy_check.check(value)
        self.assertEqual(kinds(issues), ["missing-identifier", "missing-identifier"])
        self.assertEqual({i["label"] for i in issues}, {"Raw unidentified action", "Play audio"})
        self.assertTrue(all(i["source"] == "xcui_root" for i in issues))


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

    def test_a_mac_scroll_bars_parts_are_the_systems(self):
        # Seen in a real export of a Mac scroll view (the shell's deck page):
        # AppKit's scroller reports its knob, arrows and page regions, the
        # page region below the knob as an unlabelled button, none with an
        # identifier, and none can take one.
        bar = el("scrollBar", f=(1162, 81, 19, 722.5), children=[
            el("valueIndicator", f=(1166, 85, 11, 260.5)),
            el("incrementArrow", f=(1163, 82, 0, 0)),
            el("decrementArrow", f=(1163, 82, 0, 0)),
            el("button", f=(1166, 346, 11, 453.5)),
            el("button", f=(1166, 85, 11, 0)),
        ])
        tree = doc(el("scrollView", f=(300, 81, 881, 722.5), children=[
            el("staticText", "testhooks.shell.deckPageRow.1", "Row 1", f=(320, 100, 600, 20)), bar]), window=(0, 0, 1180, 800))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_a_button_beside_a_scroll_bar_still_needs_an_identifier(self):
        tree = doc(el("scrollView", f=(0, 0, 400, 800), children=[
            el("scrollBar", f=(390, 0, 10, 800)), el("button", label="Load more", f=(16, 700, 200, 44))]))
        self.assertEqual([(i["kind"], i["label"]) for i in hierarchy_check.check(tree)], [("missing-identifier", "Load more")])

    def test_a_mac_steppers_arrows_are_appkits(self):
        def tree(ident="deckOptions.options.stepper.desiredRetention", platform="mac", arrow_id=""):
            stepper = el("stepper", ident, "Desired Retention", f=(100, 100, 22, 26), children=[
                el("incrementArrow", arrow_id, f=(101, 101, 20, 12)), el("decrementArrow", f=(101, 113, 20, 12))])
            d = doc(stepper)
            d["platform"] = platform
            return d
        self.assertEqual(hierarchy_check.check(tree()), [])
        # An unnamed stepper, another platform, or an arrow the app named keeps the arrows' checks.
        self.assertIn("missing-identifier", kinds(hierarchy_check.check(tree(ident=""))))
        self.assertIn("missing-identifier", kinds(hierarchy_check.check(tree(platform="ios"))))
        self.assertTrue(hierarchy_check.check(tree(arrow_id="Up")))

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


class NativeToggleParts(unittest.TestCase):
    """Synthetic normalized iOS trees for one standard SwiftUI Toggle."""

    def tree(self, state="1", identifier="work.preferences.option", label="Example option", y=100):
        text = el("staticText", label=label, f=(40, y + 20, 140, 20))
        inner = el("switch", f=(300, y + 16, 60, 28))
        inner["value"] = state
        outer = el("switch", identifier, label, f=(20, y, 360, 60), children=[text, inner])
        outer["value"] = state
        return doc(outer), outer, text, inner

    def assert_missing_controls(self, tree, *controls):
        missing = [i for i in hierarchy_check.check(tree) if i["kind"] == "missing-identifier"]
        self.assertEqual([(i["type"], i["label"], i["frame"]) for i in missing],
                         [(c["type"], c["label"], c["frame"]) for c in controls])

    def test_01(self):
        for state in ["0", "1"]:
            for identifier, label, y in [("work.preferences.option", "Example option", 100),
                                         ("app.settings.sample", "Window chime", 300)]:
                with self.subTest(state=state, identifier=identifier):
                    tree, _, _, _ = self.tree(state, identifier, label, y)
                    self.assertEqual(hierarchy_check.check(tree), [])

    def test_outer_requires_a_valid_app_identifier(self):
        for identifier in ["", None, "bad ID", "work.preferences", "_UIKitGenerated"]:
            with self.subTest(identifier=identifier):
                tree, outer, _, inner = self.tree()
                if identifier is None:
                    del outer["identifier"]
                else:
                    outer["identifier"] = identifier
                self.assert_missing_controls(tree, *([outer, inner] if not identifier else [inner]))
                bad = [i["identifier"] for i in hierarchy_check.check(tree) if i["kind"] == "bad-identifier"]
                self.assertEqual(bad, [identifier] if identifier in ["bad ID", "work.preferences"] else [])

    def test_outer_requires_its_label_and_binary_string_value(self):
        for key, values in [("label", [None, ""]), ("value", [None, "", "2", 1, True])]:
            for value in values:
                with self.subTest(key=key, value=value):
                    tree, outer, _, inner = self.tree()
                    outer[key] = value
                    self.assert_missing_controls(tree, inner)
            with self.subTest(key=key, absent=True):
                tree, outer, _, inner = self.tree()
                del outer[key]
                self.assert_missing_controls(tree, inner)

    def test_different_state_or_labelled_inner_switch_remains_a_control(self):
        for key, value in [("value", "0"), ("value", None), ("value", 1), ("label", "Another option")]:
            with self.subTest(key=key, value=value):
                tree, _, _, inner = self.tree()
                inner[key] = value
                self.assert_missing_controls(tree, inner)

    def test_app_identified_inner_switch_keeps_its_own_identity_checks(self):
        for identifier, expected in [("work.preferences.inner", []), ("bad ID", ["bad-identifier"]),
                                     ("work.preferences.option", ["duplicate-identifier"])]:
            with self.subTest(identifier=identifier):
                tree, outer, _, inner = self.tree()
                inner["identifier"] = identifier
                issues = hierarchy_check.check(tree)
                self.assertEqual(kinds(issues), expected)
                if expected:
                    self.assertEqual(issues[0]["identifier"], identifier)
                if expected == ["duplicate-identifier"]:
                    self.assertEqual(issues[0]["frames"], [outer["frame"], inner["frame"]])

    def test_label_and_leaf_structure_must_describe_the_same_single_toggle(self):
        for change in ["different-text", "text-value", "missing-text", "wrong-outer-role",
                       "wrong-inner-role", "nested-inner", "inner-child"]:
            with self.subTest(change=change):
                tree, outer, text, inner = self.tree()
                extra = None
                if change == "different-text":
                    text["label"] = "Another option"
                elif change == "text-value":
                    text["value"] = "1"
                elif change == "missing-text":
                    outer["children"].remove(text)
                elif change == "wrong-outer-role":
                    outer["type"] = "other"
                elif change == "wrong-inner-role":
                    inner["type"] = "button"
                elif change == "nested-inner":
                    outer["children"][1] = el("other", f=(300, 116, 60, 28), children=[inner])
                else:
                    extra = el("button", label="Nested action", f=(310, 120, 20, 20))
                    inner["children"].append(extra)
                self.assert_missing_controls(tree, *([inner, extra] if extra else [inner]))

    def test_another_interactive_sibling_is_not_hidden_with_the_inner_switch(self):
        tree, outer, _, inner = self.tree()
        button = el("button", label="Separate action", f=(200, 120, 44, 28))
        outer["children"].append(button)
        self.assert_missing_controls(tree, inner, button)

    def test_owner_and_parts_require_finite_contained_geometry(self):
        for part, x in [("outer", 50), ("text", 5), ("inner", 370)]:
            with self.subTest(part=part, outside=True):
                tree, outer, text, inner = self.tree()
                {"outer": outer, "text": text, "inner": inner}[part]["frame"]["x"] = x
                self.assert_missing_controls(tree, inner)
        for part in ["outer", "text", "inner"]:
            for key, value in [("x", float("nan")), ("y", float("inf")), ("width", float("inf"))]:
                with self.subTest(part=part, key=key, value=value):
                    tree, outer, text, inner = self.tree()
                    {"outer": outer, "text": text, "inner": inner}[part]["frame"][key] = value
                    self.assert_missing_controls(tree, inner)

    def test_standalone_and_non_ios_switches_still_need_identifiers(self):
        _, _, _, inner = self.tree()
        self.assert_missing_controls(doc(inner), inner)
        for platform in ["mac", "ipad", "iOS", None]:
            with self.subTest(platform=platform):
                tree, _, _, inner = self.tree()
                tree["platform"] = platform
                self.assert_missing_controls(tree, inner)

    def test_invalid_identifiers_on_every_original_toggle_node_remain_reported(self):
        for index in range(3):
            with self.subTest(node=index):
                tree, *nodes = self.tree()
                nodes[index]["identifier"] = "bad ID"
                bad = [i for i in hierarchy_check.check(tree) if i["kind"] == "bad-identifier"]
                self.assertEqual([(i["identifier"], i["frame"]) for i in bad],
                                 [("bad ID", nodes[index]["frame"])])

    def test_duplicates_on_every_original_toggle_node_remain_reported(self):
        for index in range(3):
            with self.subTest(node=index):
                tree, *nodes = self.tree()
                nodes[index]["identifier"] = "work.preferences.option"
                other = el("staticText", "work.preferences.option", "Other text", f=(20, 300, 120, 20))
                tree["root"]["children"][0]["children"].append(other)
                duplicates = [i for i in hierarchy_check.check(tree) if i["kind"] == "duplicate-identifier"]
                self.assertEqual(len(duplicates), 1)
                self.assertEqual(duplicates[0]["identifier"], "work.preferences.option")
                expected = [nodes[0]["frame"]]
                if index:
                    expected.append(nodes[index]["frame"])
                self.assertEqual(duplicates[0]["frames"], expected + [other["frame"]])

    def test_positive_shape_does_not_suppress_independent_findings(self):
        tree, outer, _, _ = self.tree()
        window = tree["root"]["children"][0]
        window["children"].extend([
            el("button", label="Separate action", f=(20, 300, 100, 44)),
            el("staticText", "bad ID", "Invalid identity", f=(20, 400, 140, 20)),
            el("staticText", outer["identifier"], "Repeated identity", f=(20, 500, 140, 20)),
        ])
        issues = hierarchy_check.check(tree)
        self.assertEqual(kinds(issues), ["bad-identifier", "duplicate-identifier", "missing-identifier"])
        self.assertEqual([i["label"] for i in issues if i["kind"] == "missing-identifier"], ["Separate action"])


class NativeSheetGrabber(unittest.TestCase):
    """Invented trees exercise the public checker, never captured fixtures."""

    def grabber(self):
        button = el("button", label="Sheet Grabber", f=(302, 410, 36, 20))
        button["value"] = "Half screen"
        return button

    def tree(self):
        grabber = self.grabber()
        form = el("collectionView", label="Form", f=(80, 460, 480, 400), children=[
            el("button", "work.sheet.apply", "Apply choice", f=(100, 480, 120, 44)),
        ])
        inner = el("other", f=(80, 450, 480, 420), children=[form])
        content = el("other", f=(60, 440, 520, 440), children=[inner])
        presentation = el("other", f=(60, 420, 520, 460), children=[content, grabber])
        dismiss = el("other", "PopoverDismissRegion", f=(0, 0, 640, 960))
        wrapper = el("other", f=(0, 0, 640, 960), children=[dismiss, presentation])
        tree = doc(wrapper, window=(0, 0, 640, 960))
        return tree, dict(grabber=grabber, form=form, inner=inner, content=content,
                          presentation=presentation, dismiss=dismiss, wrapper=wrapper,
                          window=tree["root"]["children"][0])

    def assert_missing(self, tree, count=1, label="Sheet Grabber"):
        issues = hierarchy_check.check(tree)
        self.assertEqual(len([i for i in issues if i["kind"] == "missing-identifier" and i["label"] == label]), count)
        return issues

    def current_content_tree(self):
        tree, p = self.tree()
        p["form"]["label"] = ""
        boundary = el("other", "sync.status.popover", f=(80, 455, 480, 410), children=[p["form"]])
        p["inner"]["children"] = [boundary]
        p["boundary"] = boundary
        return tree, p

    def panel_content_tree(self):
        tree, p = self.tree()
        boundary = el("other", "sync.status.popover", f=(80, 455, 480, 300), children=[
            el("button", "sync.status.settings", "Sync Settings", f=(300, 700, 120, 44)),
        ])
        content = el("other", f=(80, 455, 480, 300), children=[boundary])
        scroll = el("scrollView", f=(80, 450, 480, 420), children=[content])
        p["inner"]["children"] = [scroll]
        p.update(boundary=boundary, scroll=scroll, scrolled=content)
        return tree, p

    def test_panel_in_an_unmarked_scroll_view_is_supported(self):
        tree, _ = self.panel_content_tree()
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_panel_scroll_view_requires_the_exact_boundary_and_unmarked_wrappers(self):
        for target, key, value in [("boundary", "identifier", "sync.status.popoverExtra"), ("boundary", "label", "Panel"),
                                   ("scroll", "identifier", "work.scroll"), ("scroll", "label", "Scroll"),
                                   ("scrolled", "identifier", "work.content")]:
            with self.subTest(target=target, key=key, value=value):
                tree, p = self.panel_content_tree()
                p[target][key] = value
                self.assert_missing(tree)

    def test_exact_named_content_boundary_with_unlabeled_collection_is_supported(self):
        tree, p = self.current_content_tree()
        self.assertEqual(hierarchy_check.check(tree), [])
        p["form"]["children"].append(el("button", label="Choose pattern", f=(100, 550, 120, 44)))
        self.assertEqual([(i["kind"], i["label"]) for i in hierarchy_check.check(tree)],
                         [("missing-identifier", "Choose pattern")])

    def test_current_content_requires_the_exact_boundary(self):
        for key, value in [("identifier", "work.sheet.content"), ("identifier", "sync.status.popoverExtra"),
                           ("identifier", ""), ("type", "group"), ("label", "Invented content"),
                           ("value", "Invented value")]:
            with self.subTest(key=key, value=value):
                tree, p = self.current_content_tree()
                p["boundary"][key] = value
                self.assert_missing(tree)

    def test_current_boundary_requires_its_unlabeled_unidentified_collection(self):
        for change in ["missing", "wrong-role", "named", "identified", "absent-label", "absent-identifier",
                       "null-label", "null-identifier", "unrelated-descendant"]:
            with self.subTest(change=change):
                tree, p = self.current_content_tree()
                if change == "missing":
                    p["boundary"]["children"] = []
                elif change == "wrong-role":
                    p["form"]["type"] = "other"
                elif change == "named":
                    p["form"]["label"] = "Invented collection"
                elif change == "identified":
                    p["form"]["identifier"] = "work.sheet.collection"
                elif change.startswith("absent-"):
                    del p["form"][change.removeprefix("absent-")]
                elif change.startswith("null-"):
                    p["form"][change.removeprefix("null-")] = None
                else:
                    p["boundary"]["children"] = [el("other", "work.sheet.unrelated", f=(80, 455, 480, 410),
                                                           children=[p["form"]])]
                self.assert_missing(tree)

    def test_current_boundary_requires_finite_positive_typed_geometry(self):
        for key, value in [("x", float("nan")), ("y", float("inf")), ("width", 0), ("height", -1),
                           ("x", "80"), ("x", True)]:
            with self.subTest(key=key, value=value):
                tree, p = self.current_content_tree()
                p["boundary"]["frame"][key] = value
                self.assert_missing(tree)

    def test_matching_shape_avoids_only_the_grabbers_missing_identifier(self):
        tree, p = self.tree()
        self.assertEqual(hierarchy_check.check(tree), [])
        p["form"]["children"].append(el("button", label="Choose tint", f=(100, 550, 120, 44)))
        issues = hierarchy_check.check(tree)
        self.assertEqual([(i["kind"], i["label"]) for i in issues], [("missing-identifier", "Choose tint")])
        p["grabber"]["value"] = "Another detent"
        self.assertEqual([(i["kind"], i["label"]) for i in hierarchy_check.check(tree)],
                         [("missing-identifier", "Choose tint"), ("missing-identifier", "Sheet Grabber")])

    def test_same_control_in_form_content_still_needs_an_identifier(self):
        tree, p = self.tree()
        p["presentation"]["children"].remove(p["grabber"])
        p["form"]["children"].append(p["grabber"])
        self.assert_missing(tree)

    def test_scrolling_rows_and_bars_anywhere_in_ancestry_fail_closed(self):
        for role in ["scrollView", "table", "collectionView", "outline", "browser", "webView",
                     "cell", "tableRow", "outlineRow", "navigationBar", "toolbar"]:
            with self.subTest(role=role):
                tree, p = self.tree()
                p["window"]["children"] = [el(role, f=(0, 0, 640, 960), children=[p["wrapper"]])]
                self.assert_missing(tree)
        tree, p = self.tree()
        p["window"]["children"] = [el("other", label="Form", f=(0, 0, 640, 960), children=[p["wrapper"]])]
        self.assert_missing(tree)

    def test_ordinary_button_outside_presentation_is_reported(self):
        self.assertEqual(kinds(self.assert_missing(doc(self.grabber()))), ["missing-identifier"])

    def test_form_must_be_in_a_distinct_wrapped_content_branch(self):
        for change in ["missing", "wrong-role", "wrong-label", "unwrapped", "grabber-in-branch"]:
            with self.subTest(change=change):
                tree, p = self.tree()
                if change == "missing":
                    p["inner"]["children"] = []
                elif change == "wrong-role":
                    p["form"]["type"] = "other"
                elif change == "wrong-label":
                    p["form"]["label"] = "Invented content"
                elif change == "unwrapped":
                    p["presentation"]["children"] = [p["form"], p["grabber"]]
                else:
                    p["presentation"]["children"].remove(p["grabber"])
                    p["content"]["children"].append(p["grabber"])
                self.assert_missing(tree)

    def test_dismiss_region_must_be_the_direct_sibling_of_the_presentation(self):
        for change in ["missing", "nested", "inside-presentation", "inside-content", "wrong-role", "similar-name"]:
            with self.subTest(change=change):
                tree, p = self.tree()
                if change in ("missing", "nested", "inside-presentation", "inside-content"):
                    p["wrapper"]["children"].remove(p["dismiss"])
                if change == "nested":
                    p["wrapper"]["children"].append(el("other", f=(0, 0, 640, 960), children=[p["dismiss"]]))
                elif change == "inside-presentation":
                    p["presentation"]["children"].append(p["dismiss"])
                elif change == "inside-content":
                    p["content"]["children"].append(p["dismiss"])
                elif change == "wrong-role":
                    p["dismiss"]["type"] = "group"
                elif change == "similar-name":
                    p["dismiss"]["identifier"] = "PopoverDismissRegionExtra"
                self.assert_missing(tree)

    def test_surrounding_containers_require_other_role_valid_frames_and_empty_fields(self):
        for part in ["presentation", "wrapper", "content", "inner"]:
            for key in ["identifier", "label", "value"]:
                for value in ["work.sheet.named", False, 0, [], {}]:
                    with self.subTest(part=part, key=key, value=value):
                        tree, p = self.tree()
                        p[part][key] = value
                        self.assert_missing(tree)
                with self.subTest(part=part, key=key, absent=True):
                    tree, p = self.tree()
                    del p[part][key]
                    self.assert_missing(tree)
            with self.subTest(part=part, role="group"):
                tree, p = self.tree()
                p[part]["type"] = "group"
                self.assert_missing(tree)

    def test_grabber_must_straddle_top_be_narrower_and_share_the_centre(self):
        for f in [(302, 420, 36, 20), (302, 390, 36, 20), (302, 430, 36, 20),
                  (302, 400, 36, 20), (60, 410, 520, 20), (50, 410, 540, 20),
                  (302.51, 410, 36, 20), (301.49, 410, 36, 20)]:
            with self.subTest(frame=f):
                tree, p = self.tree()
                p["grabber"]["frame"] = frame(*f)
                self.assert_missing(tree)
        for x in [301.5, 302.5]:
            with self.subTest(centre_boundary=x):
                tree, p = self.tree()
                p["grabber"]["frame"]["x"] = x
                self.assertEqual(hierarchy_check.check(tree), [])

    def test_full_grabber_frame_must_remain_inside_the_window(self):
        for f in [(303, 0, 337, 960), (0, 411, 640, 549), (0, 0, 337, 960), (0, 0, 640, 429),
                  (302.1, 0, 337.9, 960), (0, 0, 337.9, 960)]:
            with self.subTest(window=f):
                tree, p = self.tree()
                p["window"]["frame"] = tree["window"] = frame(*f)
                self.assert_missing(tree)
        tree, p = self.tree()
        p["window"]["type"] = "other"
        self.assert_missing(tree)

    def test_all_evidence_requires_finite_positive_numeric_geometry(self):
        for part in ["grabber", "presentation", "wrapper", "content", "inner", "form", "dismiss", "window"]:
            for key, value in [("x", float("nan")), ("y", float("inf")), ("x", float("-inf")),
                               ("width", float("inf")), ("height", float("inf")), ("x", "302"), ("x", True)]:
                with self.subTest(part=part, key=key, value=value):
                    tree, p = self.tree()
                    p[part]["frame"][key] = value
                    self.assert_missing(tree)
            if part != "grabber":
                for key in ["width", "height"]:
                    for value in [0, -1, float("nan")]:
                        with self.subTest(part=part, key=key, value=value):
                            tree, p = self.tree()
                            p[part]["frame"][key] = value
                            self.assert_missing(tree)

    def test_empty_grabber_frames_keep_the_existing_invisible_control_behavior(self):
        for key in ["width", "height"]:
            for value in [0, -1, float("nan")]:
                with self.subTest(key=key, value=value):
                    tree, p = self.tree()
                    p["grabber"]["frame"][key] = value
                    self.assertEqual(hierarchy_check.check(tree), [])

    def test_malformed_frames_keep_the_existing_input_error(self):
        for part in ["grabber", "presentation", "wrapper", "form", "dismiss", "window"]:
            for value in [None, "broken", []]:
                with self.subTest(part=part, value=value):
                    tree, p = self.tree()
                    p[part]["frame"]["x"] = value
                    with self.assertRaises(hierarchy_check.InputError):
                        hierarchy_check.check(tree)

    def test_platform_and_schema_are_explicit_and_supported(self):
        for platform in [None, "mac", "iOS", "ipados", "", False, []]:
            with self.subTest(platform=platform):
                tree, _ = self.tree()
                tree["platform"] = platform
                self.assert_missing(tree)
        tree, _ = self.tree()
        del tree["platform"]
        self.assert_missing(tree)
        for schema in [True, 1.0]:
            with self.subTest(schema=schema):
                tree, _ = self.tree()
                tree["schema"] = schema
                self.assert_missing(tree)
        for schema in [None, 0, 2, "1"]:
            with self.subTest(schema=schema):
                tree, _ = self.tree()
                tree["schema"] = schema
                with self.assertRaises(hierarchy_check.InputError):
                    hierarchy_check.check(tree)
        tree, _ = self.tree()
        del tree["schema"]
        with self.assertRaises(hierarchy_check.InputError):
            hierarchy_check.check(tree)

    def test_other_detents_locales_and_malformed_or_absent_values_fail_closed(self):
        for value in [None, "", "Full screen", "Large", "half screen", "Half screen ", 1, True, [], {}, ["Half screen"]]:
            with self.subTest(value=value):
                tree, p = self.tree()
                p["grabber"]["value"] = value
                self.assert_missing(tree)
        tree, p = self.tree()
        del p["grabber"]["value"]
        self.assert_missing(tree)
        for label in ["Poignée de feuille", "sheet grabber", "Sheet Grabber "]:
            with self.subTest(label=label):
                tree, p = self.tree()
                p["grabber"]["label"] = label
                self.assert_missing(tree, label=label)

    def test_multiple_candidates_including_content_are_ambiguous(self):
        for branch in ["presentation", "form"]:
            with self.subTest(branch=branch):
                tree, p = self.tree()
                second = self.grabber()
                second["frame"] = frame(100, 600, 80, 30)
                p[branch]["children"].append(second)
                self.assert_missing(tree, count=2)

    def test_nonempty_app_identifiers_still_receive_format_and_duplicate_checks(self):
        tree, p = self.tree()
        p["grabber"]["identifier"] = "BadIdentifier"
        self.assertEqual([(i["kind"], i["identifier"]) for i in hierarchy_check.check(tree)],
                         [("bad-identifier", "BadIdentifier")])
        tree, p = self.tree()
        p["grabber"]["identifier"] = "work.sheet.apply"
        p["content"]["children"].append(el("button", "work.sheet.apply", "Extra choice", f=(250, 550, 120, 44)))
        issues = hierarchy_check.check(tree)
        self.assertEqual(kinds(issues), ["duplicate-identifier"])
        self.assertEqual(issues[0]["identifier"], "work.sheet.apply")
        self.assertEqual(len(issues[0]["paths"]), 3)

    def test_no_subtree_is_pruned_from_rows_overlap_or_outside_window_checks(self):
        tree, p = self.tree()
        p["form"]["children"].extend([
            el("cell", label="Row without ID", f=(100, 600, 120, 44)),
            el("button", "work.sheet.apply", "Duplicate action", f=(250, 600, 120, 44)),
            el("staticText", "work.sheet.first", "First text", f=(100, 700, 100, 20)),
            el("staticText", "work.sheet.second", "Second text", f=(120, 705, 100, 20)),
        ])
        p["grabber"]["children"] = [el("button", label="Child button", f=(305, 413, 10, 10)),
                                   el("staticText", "work.sheet.outside", "Outside text", f=(630, 900, 40, 20))]
        issues = self.assert_missing(tree, count=0)
        self.assertEqual(kinds(issues), ["duplicate-identifier", "missing-identifier", "missing-identifier",
                                        "text-outside-window", "text-overlap"])
        self.assertEqual(sorted(i["label"] for i in issues if i["kind"] == "missing-identifier"),
                         ["Child button", "Row without ID"])


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

    def test_a_mac_table_names_its_columns_and_headers(self):
        # Seen in a SwiftUI Table's export (U281): generated column ids, and
        # unlabelled header buttons at each column's x and width.
        columns = [("8F4D7F23-581A-4144-BABE-CA39D6A01371", 431, 153.5), ("741A6E0A-75AE-44AA-BCF2-13CA347718F1", 584.5, 97.5)]
        tree = doc(el("outline", "noteTypes.fields.table", f=(420, 81, 581, 701), children=[
            *[el("tableColumn", ident, f=(x, 110, w, 671)) for ident, x, w in columns],
            el("group", f=(421, 82, 579, 28), children=[el("button", f=(x, 82, w, 28)) for _, x, w in columns])]))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_swiftui_segments_a_searchable_field_and_a_titleless_sidebar_bar(self):
        # Seen on iPad (U281): a segmented Picker's segments, the field `searchable`
        # puts in a navigation bar, and the sidebar's bar after its removed title.
        tree = doc(el("navigationBar", "Meguri", f=(0, 32, 320, 54), children=[el("button", label="Hide Sidebar", f=(10, 40, 40, 40))]),
                   el("navigationBar", f=(320, 32, 900, 54), children=[
                       el("segmentedControl", "noteTypes.list.tabs", f=(700, 40, 120, 40), children=[
                           el("button", label="Fields", f=(700, 40, 60, 40)), el("button", label="Cards", f=(760, 40, 60, 40))]),
                       el("searchField", label="Filter", f=(1000, 40, 200, 40))]), window=(0, 0, 1366, 1024))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_lookalikes_of_those_parts_are_still_reported(self):
        tree = doc(el("navigationBar", "Meguri", f=(0, 32, 320, 54), children=[el("button", label="Edit", f=(10, 40, 40, 40))]),
                   el("segmentedControl", f=(0, 100, 120, 40), children=[el("button", label="Fields", f=(0, 100, 60, 40))]),
                   el("searchField", label="Filter", f=(0, 200, 200, 40)))
        self.assertEqual(kinds(hierarchy_check.check(tree)),
                         ["bad-identifier", "missing-identifier", "missing-identifier", "missing-identifier", "missing-identifier"])

    def test_table_header_lookalikes_are_still_reported(self):
        column = "8F4D7F23-581A-4144-BABE-CA39D6A01371"
        # A button that matches no column, a labelled one, a body button at a column's x and width,
        # and a column id outside a table.
        tree = doc(el("outline", "sample.screen.table", f=(0, 0, 400, 400), children=[
            el("tableColumn", column, f=(10, 30, 100, 300)),
            el("group", f=(0, 100, 400, 28), children=[el("button", f=(10, 100, 100, 28))]),
            el("group", f=(0, 0, 400, 28), children=[el("button", f=(10, 0, 100, 28)), el("button", f=(110, 0, 90, 28))]),
            el("group", f=(0, 0, 400, 28), children=[el("button", label="Name", f=(10, 0, 100, 28))])]),
            el("tableColumn", column, f=(0, 500, 100, 100)))
        self.assertEqual(kinds(hierarchy_check.check(tree)),
                         ["bad-identifier", "missing-identifier", "missing-identifier", "missing-identifier", "missing-identifier"])

    def test_those_names_on_other_elements_are_still_reported(self):
        tree = doc(el("group", "main, SidebarNavigationSplitView", f=(0, 30, 800, 600)), el("button", "ListColumn", f=(0, 0, 40, 40)))
        self.assertEqual(kinds(hierarchy_check.check(tree)), ["bad-identifier", "bad-identifier"])

    def test_alert_button_ids_outside_a_sheet_and_symbol_ids_elsewhere_are_still_reported(self):
        tree = doc(el("button", "action-button-1", "Save", f=(0, 0, 40, 40)), el("button", "chevron.backward", "Back", f=(0, 50, 40, 40)))
        self.assertEqual(kinds(hierarchy_check.check(tree)), ["bad-identifier", "bad-identifier"])


class NativeOutlineIdentifiers(unittest.TestCase):
    marker = "NSOutlineViewDisclosureButtonKey"

    def tree(self, *rows):
        tree = doc(el("outline", f=(0, 0, 400, 400), children=rows))
        tree["platform"] = "mac"
        return tree

    def row(self, y, own_id, marker=None, type_="disclosureTriangle", extra=()):
        return el("outlineRow", f=(0, y, 400, 30), children=[
            el("cell", f=(0, y, 400, 30), children=[
                el(type_, self.marker if marker is None else marker, f=(0, y, 14, 14)),
                el("button", own_id, "Deck", f=(20, y, 200, 30)), *extra,
            ]),
        ])

    def test_repeated_exact_marker_in_native_outline_cells_is_system_owned(self):
        tree = self.tree(self.row(0, "home.deck.row.1"), self.row(40, "home.deck.row.2"))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_exact_marker_on_other_roles_still_fails(self):
        for type_ in ["button", "staticText", "group", "cell"]:
            with self.subTest(type=type_):
                issues = hierarchy_check.check(self.tree(self.row(0, "home.deck.row.1", type_=type_)))
                self.assertEqual([(i["kind"], i["identifier"]) for i in issues],
                                 [("bad-identifier", self.marker)])

    def test_native_role_requires_cell_and_ordered_outline_row_outline_context(self):
        for containers in [(), ("cell",), ("outlineRow", "cell"), ("outline", "cell"),
                           ("outline", "outlineRow"), ("table", "tableRow", "cell"),
                           ("outlineRow", "outline", "cell"), ("outline", "outlineRow", "cell", "group")]:
            with self.subTest(containers=containers):
                children = [el("disclosureTriangle", self.marker), el("button", "home.deck.row.1", "Deck")]
                for type_ in reversed(containers):
                    children = [el(type_, children=children)]
                tree = doc(*children)
                tree["platform"] = "mac"
                self.assertEqual([(i["kind"], i["identifier"]) for i in hierarchy_check.check(tree)],
                                 [("bad-identifier", self.marker)])

    def test_similar_marker_names_are_not_exempted(self):
        for marker in [self.marker + ".1", self.marker + "Extra", "NSOutlineViewOtherKey"]:
            with self.subTest(marker=marker):
                issues = hierarchy_check.check(self.tree(self.row(0, "home.deck.row.1", marker=marker)))
                self.assertEqual([(i["kind"], i["identifier"]) for i in issues], [("bad-identifier", marker)])

    def test_app_owned_disclosure_and_row_duplicates_in_native_context_still_fail(self):
        tree = self.tree(self.row(0, "home.deck.row.1", marker="home.deck.disclosure.1"),
                         self.row(40, "home.deck.row.1", marker="home.deck.disclosure.1"))
        self.assertEqual([(i["kind"], i["identifier"]) for i in hierarchy_check.check(tree)],
                         [("duplicate-identifier", "home.deck.disclosure.1"),
                          ("duplicate-identifier", "home.deck.row.1")])

    def test_missing_ids_remain_reported(self):
        marker_only = el("outlineRow", children=[el("cell", children=[el("disclosureTriangle", self.marker)])])
        issues = hierarchy_check.check(self.tree(marker_only))
        self.assertEqual([(i["kind"], i["type"]) for i in issues],
                         [("missing-identifier", "outlineRow"), ("missing-identifier", "cell")])
        issues = hierarchy_check.check(self.tree(self.row(0, "home.deck.row.1", extra=[el("button", label="Unnamed")])))
        self.assertEqual([(i["kind"], i["label"]) for i in issues], [("missing-identifier", "Unnamed")])

    def test_system_marker_does_not_suppress_adjacent_text_geometry_findings(self):
        texts = [el("staticText", "home.deck.name.1", "First", f=(20, 0, 100, 20)),
                 el("staticText", "home.deck.name.2", "Second", f=(30, 0, 100, 20))]
        issues = hierarchy_check.check(self.tree(self.row(0, "home.deck.row.1", extra=texts)))
        self.assertEqual(kinds(issues), ["text-overlap"])


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

    def test_rows_a_scroll_view_clips_do_not_overlap_the_text_below_it(self):
        # Seen in a real export (the shell's deck page on the iPad, its
        # state probe below the scroll view): a lazy stack's next rows hang
        # past the scroll view's edge, clipped; only what shows counts.
        tree = doc(el("scrollView", f=(0, 0, 1032, 1334.5), children=[
            el("staticText", "testhooks.shell.deckPageRow.20", "Row 20", f=(372, 1312.5, 608, 44.5)),
            el("staticText", "testhooks.shell.deckPageRow.21", "Row 21", f=(372, 1369, 608, 44.5))]),
            el("staticText", "testhooks.shell.state", "layout=sidebar", f=(0, 1334.5, 1032, 41.5)), window=(0, 0, 1032, 1376))
        self.assertEqual(hierarchy_check.check(tree), [])

    def test_the_visible_part_of_a_clipped_row_still_overlaps(self):
        # A label floating over the scroll view's last visible points.
        tree = doc(el("scrollView", f=(0, 0, 400, 400), children=[
            el("staticText", "home.list.row.9", "Row 9", f=(16, 380, 300, 44))]),
            el("staticText", "home.list.badge", "New", f=(16, 390, 100, 20)))
        issues = hierarchy_check.check(tree)
        self.assertEqual(kinds(issues), ["text-overlap"])
        self.assertEqual(issues[0]["labels"], ["Row 9", "New"])
        self.assertIn("overlap by 100x10 pt", issues[0]["message"])

    def test_a_table_clips_its_rows_to_the_scroll_view_around_it(self):
        # On the Mac a table's frame is its whole content; the scroll view
        # around it is what shows.
        tree = doc(el("scrollView", f=(0, 100, 400, 300), children=[el("table", f=(0, 100, 400, 2000), children=[
            el("staticText", "home.list.row.30", "Row 30", f=(16, 410, 300, 20))])]),
            el("staticText", "home.footer.total", "40 decks", f=(16, 410, 120, 20)))
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

    def test_the_shell_deck_page_on_the_mac_has_no_issue(self):
        # A scroll view with AppKit's scroller, its lazy rows hanging past
        # its bottom edge into the state probe's strip below it.
        tree = self.load("mac-shell-deck-page.json")
        self.assertEqual(hierarchy_check.check(tree), [])
        root = hierarchy_check.Element(tree["root"], None, 0)
        self.assertTrue(any(e.type == "scrollBar" for e in root.walk()))
        probe = next(e for e in root.walk() if e.identifier == "testhooks.shell.state")
        self.assertTrue(any(e.identifier.startswith("testhooks.shell.deckPageRow.") and e.y < probe.y + probe.h and e.y + e.h > probe.y
                            for e in root.walk()), "no row's full frame reaches the probe")

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


class NativeSidebarDisclosureRowParts(unittest.TestCase):
    """iOS: a DisclosureGroup row's system button copies its label's identifier
    and adds a chevron; invented trees, never captured fixtures."""

    def row(self, label_id="home.sidebar.deck.1", value="6 new", chevron=(277, 263, 10, 14)):
        text = el("staticText", label_id, "Languages", f=(32, 259, 237, 20))
        text["value"] = value
        children = [text]
        if chevron:
            children.append(el("button", f=chevron))
        button = el("button", "home.sidebar.deck.1", "Languages", f=(16, 244, 288, 52), children=children)
        button["value"] = "6 new"
        return doc(el("cell", f=(16, 244, 288, 52), children=[el("other", f=(16, 244, 288, 52), children=[button])]))

    def test_the_rows_own_label_and_the_system_chevron_are_not_issues(self):
        self.assertEqual(hierarchy_check.check(self.row()), [])

    def test_a_different_label_or_value_is_still_a_duplicate(self):
        self.assertIn("duplicate-identifier", kinds(hierarchy_check.check(self.row(value="other"))))

    def test_a_chevron_away_from_the_trailing_end_still_needs_an_identifier(self):
        self.assertIn("missing-identifier", kinds(hierarchy_check.check(self.row(chevron=(40, 263, 10, 14)))))

    def test_a_chevron_outside_the_row_vertically_still_needs_an_identifier(self):
        self.assertIn("missing-identifier", kinds(hierarchy_check.check(self.row(chevron=(277, 320, 10, 14)))))

    def test_only_on_ios(self):
        tree = self.row()
        tree["platform"] = "mac"
        self.assertIn("duplicate-identifier", kinds(hierarchy_check.check(tree)))


class NativeMacToolbarTitlePair(unittest.TestCase):
    def tree(self, second_frame=(224, 30, 952, 52), label=""):
        title = el("staticText", "", label, f=(224, 30, 952, 52))
        title["value"] = "Today"
        subtitle = el("staticText", "", "", f=second_frame)
        subtitle["value"] = "35 new · 5 learning · 342 to review"
        tree = doc(title, subtitle, window=(0, 0, 1200, 800))
        tree["platform"] = "mac"
        return tree

    def test_the_title_and_subtitle_at_one_frame_do_not_overlap(self):
        self.assertEqual(hierarchy_check.check(self.tree()), [])

    def test_other_overlapping_texts_still_overlap(self):
        self.assertIn("text-overlap", kinds(hierarchy_check.check(self.tree(second_frame=(230, 40, 900, 40)))))
        self.assertIn("text-overlap", kinds(hierarchy_check.check(self.tree(label="Today"))))

    def test_three_texts_at_one_frame_still_overlap(self):
        tree = self.tree()
        third = el("staticText", "", "", f=(224, 30, 952, 52))
        third["value"] = "Another"
        tree["root"]["children"][0]["children"].append(third)
        self.assertIn("text-overlap", kinds(hierarchy_check.check(tree)))
