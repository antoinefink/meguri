# SPDX-License-Identifier: AGPL-3.0-or-later
"""browser_config_set and the browser_config observation
(specs/conformance/browser-config-ops.md B1, B2) through real workers.
Expected values come from the spec's tables (specs/browser/config-keys.md K1,
K2), from the scenario's own arguments (their compact JSON text, sorted as
storage/config-table C5 says) and from the scenario clock, never from the
driver."""

from __future__ import annotations

import json
import math
import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.harness import WorkerProcess
from driver.session import Session
from driver.tests.test_worker import epoch, run_ok, scenario

START = "2030-01-15T10:00:00"
T0 = epoch(START, "UTC")

# browser-config-ops.md B2 with the K1 defaults: what Anki's reads give when
# none of the 23 keys is stored.
DEFAULTS = {
    "browserTableShowNotesMode": False,
    "collapseSavedSearches": False,
    "collapseToday": False,
    "collapseFlags": False,
    "collapseCardState": False,
    "collapseDecks": False,
    "collapseNotetypes": False,
    "collapseTags": False,
    "previewBothSides": False,
    "ignoreAccentsInSearch": False,
    "restorePositionBrowser": True,
    "resetCountsBrowser": False,
    "normalize_note_text": True,
    "setDueBrowser": "0",
    "defaultSearchText": "",
    "activeCols": ["noteFld", "template", "cardDue", "deck"],
    "activeNoteCols": ["noteFld", "note", "template", "noteTags"],
    "sortType": None,
    "noteSortType": None,
    "sortBackwards": False,
    "browserNoteSortBackwards": False,
    "savedFilters": {},
    "flagLabels": {},
}
KEYS_SQL = ", ".join(f"'{k}'" for k in DEFAULTS)

# One value of its B1 shape for each of the 22 keys browser_config_set takes.
VALUES = {
    "browserTableShowNotesMode": True,
    "collapseSavedSearches": True,
    "collapseToday": True,
    "collapseFlags": True,
    "collapseCardState": True,
    "collapseDecks": True,
    "collapseNotetypes": True,
    "collapseTags": True,
    "previewBothSides": True,
    "ignoreAccentsInSearch": True,
    "restorePositionBrowser": False,
    "resetCountsBrowser": True,
    "sortBackwards": True,
    "browserNoteSortBackwards": True,
    "setDueBrowser": "1-7!",
    "defaultSearchText": " deck:Inbox\tis:due ",
    "sortType": "cardDue",
    "noteSortType": "",
    "activeCols": ["question", "addonX", "", "question"],
    "activeNoteCols": [],
    "savedFilters": {"Zed": "tag:leech", "Ünï": 'deck:"A B"', "apple": "a\\b", "Apple": "x\ty"},
    "flagLabels": {"3": "Important ✓", "1": ""},
}


def compact(value) -> str:
    """storage/config-table C5: no spaces, members sorted by code point,
    non-ASCII kept as UTF-8."""
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def run(worker, steps, setup=None):
    return worker.request({"cmd": "run", "scenario": scenario("t/browser", "UTC", START, steps, setup=setup)})


def delete_all():
    return {"db_exec": {"sql": f"DELETE FROM config WHERE KEY IN ({KEYS_SQL})"}}


class BrowserConfigSet(unittest.TestCase):
    def test_each_key_is_stored_whole_in_compact_form_and_read_back(self):
        steps = [delete_all(), {"advance": "1h"}]
        steps += [{"browser_config_set": {"key": k, "value": v}} for k, v in VALUES.items()]
        steps += [
            {"db_exec": {"sql": "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT), typeof(val) FROM config "
                                f"WHERE KEY IN ({KEYS_SQL}) ORDER BY KEY"}},
            {"observe": ["browser_config"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/browser", "UTC", START, steps))
        written = out[2:-2]
        self.assertEqual(written, [{}] * len(VALUES))
        now = T0 + 3600
        expected = [[k, -1, now, compact(VALUES[k]), "blob"] for k in sorted(VALUES)]
        self.assertEqual(out[-2]["db_exec"]["rows"], expected)
        self.assertEqual(out[-1]["browser_config"], {**DEFAULTS, **VALUES})

    def test_the_same_value_writes_nothing(self):
        steps = [
            {"browser_config_set": {"key": "savedFilters", "value": {"b": "2", "a": "1"}}},
            {"browser_config_set": {"key": "collapseTags", "value": True}},
            {"browser_config_set": {"key": "setDueBrowser", "value": "3"}},
            {"browser_config_set": {"key": "activeCols", "value": ["deck"]}},
            {"db_exec": {"sql": "SELECT mod FROM col"}},
            {"advance": "1h"},
            {"browser_config_set": {"key": "savedFilters", "value": {"a": "1", "b": "2"}}},
            {"browser_config_set": {"key": "collapseTags", "value": True}},
            {"browser_config_set": {"key": "setDueBrowser", "value": "3"}},
            {"browser_config_set": {"key": "activeCols", "value": ["deck"]}},
            {"db_exec": {"sql": "SELECT (SELECT mod FROM col), "
                                f"(SELECT count(*) FROM config WHERE mtime_secs >= {T0 + 3600})"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/browser", "UTC", START, steps))
        col_mod = out[4]["db_exec"]["rows"][0][0]
        self.assertEqual(out[-1]["db_exec"]["rows"], [[col_mod, 0]])

    def test_keys_and_values_outside_b1_are_invalid(self):
        bad = [
            ["activeCols", ["deck"]],
            {"key": "collapseTags"},
            {"value": True},
            {"key": "collapseTags", "value": True, "undoable": True},
            {"key": "normalize_note_text", "value": False},
            {"key": "restorePositionReviewer", "value": True},
            {"key": "", "value": True},
            {"key": 1, "value": True},
            {"key": None, "value": True},
            {"key": ["collapseTags"], "value": True},
            {"key": "collapseTags", "value": 1},
            {"key": "collapseTags", "value": 0},
            {"key": "collapseTags", "value": 1.0},
            {"key": "collapseTags", "value": "true"},
            {"key": "collapseTags", "value": None},
            {"key": "sortBackwards", "value": 1},
            {"key": "sortBackwards", "value": "false"},
            {"key": "setDueBrowser", "value": 5},
            {"key": "setDueBrowser", "value": None},
            {"key": "defaultSearchText", "value": True},
            {"key": "defaultSearchText", "value": ["deck:x"]},
            {"key": "sortType", "value": None},
            {"key": "noteSortType", "value": 1.5},
            {"key": "activeCols", "value": "noteFld"},
            {"key": "activeCols", "value": ["noteFld", 1]},
            {"key": "activeNoteCols", "value": [True]},
            {"key": "activeNoteCols", "value": [None]},
            {"key": "activeNoteCols", "value": [["noteFld"]]},
            {"key": "activeCols", "value": {}},
            {"key": "savedFilters", "value": []},
            {"key": "savedFilters", "value": {"a": 1}},
            {"key": "savedFilters", "value": {"a": None}},
            {"key": "flagLabels", "value": {"1": True}},
            {"key": "flagLabels", "value": {"1": ["x"]}},
            {"key": "flagLabels", "value": "x"},
        ]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, [{"observe": ["ids"]}, {"browser_config_set": args}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)


class BrowserConfigObservation(unittest.TestCase):
    def test_absent_keys_read_as_their_defaults_and_reading_writes_nothing(self):
        steps = [
            delete_all(),
            {"db_exec": {"sql": "SELECT mod FROM col"}},
            {"advance": "1h"},
            {"observe": ["browser_config"]},
            {"db_exec": {"sql": f"SELECT (SELECT mod FROM col), (SELECT count(*) FROM config WHERE KEY IN ({KEYS_SQL})), "
                                f"(SELECT count(*) FROM config WHERE mtime_secs >= {T0 + 3600})"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/browser", "UTC", START, steps))
        value = out[3]["browser_config"]
        self.assertEqual(value, DEFAULTS)
        self.assertEqual(len(value), 23)
        self.assertEqual(out[4]["db_exec"]["rows"], [[out[1]["db_exec"]["rows"][0][0], 0, 0]])

    def test_typed_reads_give_the_default_for_any_other_shape(self):
        # K2.1, K2.2: only a JSON boolean (string) is read; anything else,
        # unreadable bytes and a TEXT storage class included, is the default.
        stored = [
            ("browserTableShowNotesMode", "1", "blob"),
            ("collapseSavedSearches", '"true"', "blob"),
            ("collapseToday", "null", "blob"),
            ("collapseFlags", "{", "blob"),
            ("collapseCardState", "true", "text"),
            ("collapseDecks", " true ", "blob"),
            ("collapseNotetypes", "[true]", "blob"),
            ("collapseTags", "1.0", "blob"),
            ("previewBothSides", "true", "blob"),
            ("restorePositionBrowser", "0", "blob"),
            ("normalize_note_text", "false", "blob"),
            ("setDueBrowser", "5", "blob"),
            ("defaultSearchText", '"  deck:x "', "blob"),
        ]
        expected = dict(DEFAULTS)
        expected.update(collapseDecks=True, previewBothSides=True, normalize_note_text=False,
                        defaultSearchText="  deck:x ")
        self.assertEqual(self._observe_stored(stored), expected)

    def test_untyped_keys_report_the_stored_json_whatever_its_shape(self):
        stored = [
            ("activeCols", '[1.0, -0.0, 2, 1e16, "x"]', "blob"),
            ("activeNoteCols", '"note"', "blob"),
            ("sortType", "7", "blob"),
            ("noteSortType", '"noteCrt', "blob"),
            ("sortBackwards", '"yes"', "text"),
            ("browserNoteSortBackwards", "true", "blob"),
            ("savedFilters", '{"a":"x","a":"y","b":[null]}', "blob"),
            ("flagLabels", "[]", "blob"),
        ]
        value = self._observe_stored(stored)
        expected = dict(DEFAULTS)
        expected.update(activeCols=[1.0, -0.0, 2, 1e16, "x"], activeNoteCols="note", sortType=7,
                        browserNoteSortBackwards=True, savedFilters={"a": "y", "b": [None]}, flagLabels=[])
        self.assertEqual(value, expected)
        # Number kinds survive (observations.md O6): floats stay floats, the
        # sign of -0.0 included, integers stay integers.
        cols = value["activeCols"]
        self.assertEqual([type(v) for v in cols[:4]], [float, float, int, float])
        self.assertEqual(math.copysign(1.0, cols[1]), -1.0)
        self.assertIs(type(value["sortType"]), int)

    def _observe_stored(self, stored):
        rows = ", ".join(
            "(?, 0, 1700000000, CAST(? AS BLOB))" if kind == "blob" else "(?, 0, 1700000000, ?)"
            for _, _, kind in stored
        )
        params = [p for key, text, _ in stored for p in (key, text)]
        check = {"db_exec": {"sql": "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT), typeof(val) FROM config "
                                    "WHERE mtime_secs = 1700000000 ORDER BY KEY"}}
        steps = [
            delete_all(),
            {"db_exec": {"sql": f"INSERT INTO config (KEY, usn, mtime_secs, val) VALUES {rows}", "params": params}},
            {"advance": "1h"},
            {"observe": ["browser_config"]},
            check,
            {"db_exec": {"sql": f"SELECT count(*) FROM config WHERE mtime_secs >= {T0 + 3600}"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/browser", "UTC", START, steps))
        # Reading leaves every stored row as it was.
        self.assertEqual(out[4]["db_exec"]["rows"],
                         sorted([k, 0, 1700000000, text, kind] for k, text, kind in stored))
        self.assertEqual(out[5]["db_exec"]["rows"], [[0]])
        return out[3]["browser_config"]

    def test_takes_no_arguments(self):
        with WorkerProcess("UTC") as w:
            for spec in ({"browser_config": {}}, {"browser_config": []}, "browser_config(#c1)"):
                resp = run(w, [{"observe": ["ids"]}, {"observe": [spec]}])
                with self.subTest(spec=spec):
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)


class UndoHistory(unittest.TestCase):
    """B1: no undo entry and the history kept, for each of the three setters
    (the undo queue is not observable through the protocol)."""

    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        sc = scenario("t/browser-undo", "UTC", START, [], notes=1)
        self.s = Session(sc, "UTC", self.tmp.name, Clock(str(Path(self.tmp.name) / "clock")))

    def tearDown(self):
        self.s.close()
        self.tmp.cleanup()

    def test_writes_add_no_undo_entry_and_keep_the_history(self):
        col = self.s.col
        added = col.undo_status().undo
        self.s.run_step(0, {"edit_note": {"note": "#n1", "fields": ["changed"]}})
        edit = col.undo_status().undo
        self.assertNotIn(edit, ("", added))
        for key, value in (("collapseDecks", True), ("defaultSearchText", "is:due"),
                           ("savedFilters", {"x": "y"}), ("sortBackwards", True)):
            self.s.run_step(len(self.s.outputs), {"browser_config_set": {"key": key, "value": value}})
            with self.subTest(key=key):
                self.assertEqual(col.undo_status().undo, edit)


if __name__ == "__main__":
    unittest.main()
