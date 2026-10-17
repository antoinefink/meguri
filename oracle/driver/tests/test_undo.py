# SPDX-License-Identifier: AGPL-3.0-or-later
"""Undo ops (specs/conformance/undo-ops.md V1-V4) through real workers: undo,
redo, set_config's undoable, remove_config and the undo_status observation.
Expected ids come from the spec's V4 table and the step each scenario op makes
there; expected rows from the scenario's own values."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.harness import WorkerProcess
from driver.session import Session
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
# A statement that changes nothing and empties both histories (V5).
EMPTY_HISTORY = {"db_exec": {"sql": "UPDATE notes SET usn = usn WHERE 0"}}
NONE = {"undo_status": {"undo": None, "redo": None}}

# undo-ops.md V4, in its order.
V4_IDS = [
    "add_deck", "add_note", "add_note_type", "answer_card", "build_deck", "bury", "change_deck",
    "change_note_type", "clear_unused_tags", "custom_study", "deck_options", "delete_deck", "delete_note",
    "empty", "empty_cards", "find_and_replace", "grade_now", "image_occlusion", "import", "preferences",
    "remove_note_type", "remove_tag", "rename_deck", "rename_tag", "reposition", "reset_card", "select_deck",
    "set_due_date", "set_flag", "suspend", "toggle_load_balancer", "unbury_unsuspend", "update_card",
    "update_config", "update_deck", "update_note", "update_note_type", "update_tag",
]


def status(undo, redo):
    return {"undo_status": {"undo": undo, "redo": redo}}


def run(worker, steps, notes=0):
    return worker.request({"cmd": "run", "scenario": scenario("t/undo", "UTC", START, steps, notes=notes)})


class UndoRedo(unittest.TestCase):
    def test_an_edit_is_undone_and_redone_and_empty_histories_are_refused(self):
        steps = [
            {"observe": ["undo_status"]},
            EMPTY_HISTORY,
            {"observe": ["undo_status"]},
            {"undo": {}},
            {"redo": {}},
            {"advance": "1h"},
            {"edit_note": {"note": "#n1", "fields": ["changed"]}},
            {"observe": ["undo_status"]},
            {"undo": {}},
            {"db_exec": {"sql": "SELECT flds FROM notes"}},
            {"observe": [{"undo_status": None}]},
            {"redo": {}},
            {"db_exec": {"sql": "SELECT flds FROM notes"}},
            {"observe": ["undo_status"]},
            {"redo": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/undo", "UTC", START, steps, notes=1))
        # The setup's note addition is an Add Note step on the oracle (V5).
        self.assertEqual(out[0], status("add_note", None))
        self.assertEqual(out[2], NONE)
        self.assertEqual(out[3:5], [{"undo": {"error": "nothing_to_undo"}}, {"redo": {"error": "nothing_to_redo"}}])
        self.assertEqual(out[7], status("update_note", None))
        self.assertEqual(out[8], {})
        self.assertEqual(out[9]["db_exec"]["rows"], [["q1\x1fa1"]])
        self.assertEqual(out[10], status(None, "update_note"))
        self.assertEqual(out[11], {})
        self.assertEqual(out[12]["db_exec"]["rows"], [["changed\x1fa1"]])
        self.assertEqual(out[13], status("update_note", None))
        self.assertEqual(out[14], {"redo": {"error": "nothing_to_redo"}})

    def test_ops_name_their_steps(self):
        steps = [
            EMPTY_HISTORY,
            {"delete_notes": {"notes": ["#n1"]}},
            {"observe": ["undo_status"]},
            {"advance": "1h"},
            {"add_note": {"notetype": "QA", "deck": "A", "fields": ["q", "a"]}},
            {"observe": ["undo_status"]},
            {"advance": "1h"},
            {"answer": {"card": "#c2", "rating": "good"}},
            {"observe": ["undo_status"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/undo", "UTC", START, steps, notes=1))
        self.assertEqual(
            [out[2], out[5], out[8]],
            [status("delete_note", None), status("add_note", None), status("answer_card", None)],
        )


class ConfigWrites(unittest.TestCase):
    def test_undoable_writes_and_removals_are_update_config_steps(self):
        steps = [
            EMPTY_HISTORY,
            {"set_config": {"key": "plain", "value": 1, "undoable": False}},
            {"observe": ["undo_status"]},
            {"set_config": {"key": "probe", "value": [1], "undoable": True}},
            {"observe": ["undo_status"]},
            {"remove_config": {"key": "probe"}},
            {"observe": ["undo_status"]},
            {"db_exec": {"sql": "SELECT count() FROM config WHERE KEY = 'probe'"}},
            {"undo": {}},
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT) FROM config WHERE KEY = 'probe'"}},
            {"undo": {}},
            {"db_exec": {"sql": "SELECT count() FROM config WHERE KEY = 'probe'"}},
            {"undo": {}},
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT) FROM config WHERE KEY = 'plain'"}},
            EMPTY_HISTORY,
            {"remove_config": {"key": "absent"}},
            {"observe": ["undo_status"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/undo", "UTC", START, steps))
        self.assertEqual(out[2], NONE)
        self.assertEqual(out[4], status("update_config", None))
        self.assertEqual(out[6], status("update_config", None))
        self.assertEqual(out[7]["db_exec"]["rows"], [[0]])
        self.assertEqual(out[9]["db_exec"]["rows"], [["[1]"]])
        self.assertEqual(out[11]["db_exec"]["rows"], [[0]])
        self.assertEqual(out[12], {"undo": {"error": "nothing_to_undo"}})
        # The write without an undo step stays.
        self.assertEqual(out[13]["db_exec"]["rows"], [["1"]])
        self.assertEqual(out[16], NONE)

    def test_bad_arguments(self):
        bad = [
            {"undo": []},
            {"undo": None},
            {"undo": {"steps": 1}},
            {"redo": {"x": 1}},
            {"remove_config": "k"},
            {"remove_config": {}},
            {"remove_config": {"key": ""}},
            {"remove_config": {"key": 1}},
            {"remove_config": {"key": "k", "undoable": True}},
            {"set_config": {"key": "k", "value": 1, "undoable": 1}},
            {"set_config": {"key": "k", "value": 1, "undoable": "true"}},
            {"set_config": {"key": "k", "value": 1, "undoable": None}},
            {"observe": [{"undo_status": {}}]},
            {"observe": ["undo_status(#n1)"]},
        ]
        with WorkerProcess("UTC") as w:
            for step in bad:
                resp = run(w, [step], notes=1)
                with self.subTest(step=step):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class StepNames(unittest.TestCase):
    """The name mapping through one in-process session: Anki's translations
    and a custom step are not reachable through the protocol."""

    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        sc = scenario("t/undo", "UTC", START, [], notes=1)
        self.s = Session(sc, "UTC", self.tmp.name, Clock(str(Path(self.tmp.name) / "clock")))

    def tearDown(self):
        self.s.close()
        self.tmp.cleanup()

    def step(self, step):
        return self.s.run_step(len(self.s.outputs), step)

    def test_every_v4_id_has_a_name_of_its_own(self):
        from driver.ops import undo

        by_name = undo.step_ids(self.s.col.tr)
        self.assertEqual(sorted(by_name.values()), sorted(V4_IDS))
        self.assertEqual(len(by_name), len(V4_IDS))

    def test_a_step_v4_does_not_list_is_a_run_error_without_its_name(self):
        name = "Probe Step 7f3a"
        self.s.col.add_custom_undo_entry(name)
        with self.assertRaises(DriverError) as caught:
            self.step({"observe": ["undo_status"]})
        err = caught.exception.to_json()
        self.assertEqual((err["kind"], err["step"]), ("internal_error", 0))
        self.assertNotIn(name, repr(err))


if __name__ == "__main__":
    unittest.main()
