# SPDX-License-Identifier: AGPL-3.0-or-later
"""Record ops (specs/conformance/record-ops.md W1-W4) through real workers:
db_exec, edit_note, delete_notes and set_config. Expected values come from the
statements and arguments themselves (SQLite's typeof, hashlib, the scenario
clock), never from the driver."""

from __future__ import annotations

import hashlib
import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.harness import WorkerProcess
from driver.session import Session
from driver.tests.test_worker import epoch, run_ok, scenario

START = "2030-01-15T10:00:00"


def sha(data: bytes) -> dict:
    return {"sha256": hashlib.sha256(data).hexdigest()}


def run(worker, steps, notes=0, setup=None):
    return worker.request({"cmd": "run", "scenario": scenario("t/rec", "UTC", START, steps, notes=notes, setup=setup)})


class DbExec(unittest.TestCase):
    def test_values_follow_the_storage_class_of_each_returned_value(self):
        steps = [
            {"db_exec": {"sql": "SELECT 1, 1.0, 2.5, 1e16, 'é', X'00ff', NULL, CAST(X'61ff62' AS TEXT), X''"}},
            {"db_exec": {"sql": "SELECT ?, typeof(?), ?, typeof(?), ?, typeof(?), ?, typeof(?)",
                         "params": ["x", "x", 3, 3, 2.0, 2.0, None, None]}},
            {"db_exec": {"sql": "SELECT typeof(CAST(? AS BLOB)), CAST(? AS BLOB)", "params": ["é", "é"]}},
            {"db_exec": {"sql": "SELECT 1 WHERE 0"}},
            {"db_exec": {"sql": "UPDATE notes SET usn = usn"}},
            {"db_exec": {"sql": "SELECT 7;"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/db", "UTC", START, steps, notes=1))
        rows = [step["db_exec"]["rows"] for step in out]
        self.assertEqual(rows[0], [[1, 1.0, 2.5, 1e16, "é", sha(b"\x00\xff"), None, "a\ufffdb", sha(b"")]])
        # REAL values stay floats on the wire (1.0, not 1), INTEGER values ints.
        self.assertEqual([type(v) for v in rows[0][0][:4]], [int, float, float, float])
        self.assertEqual(rows[1], [["x", "text", 3, "integer", 2.0, "real", None, "null"]])
        self.assertIsInstance(rows[1][0][4], float)
        self.assertEqual(rows[2], [["blob", sha("é".encode("utf-8"))]])
        self.assertEqual(rows[3:], [[], [], [[7]]])

    def test_a_write_commits_at_once_and_stamps_nothing(self):
        steps = [
            {"db_exec": {"sql": "SELECT mod FROM col"}},
            {"advance": "1h"},
            {"db_exec": {"sql": "UPDATE notes SET usn = 7"}},
            {"handoff": "meguri"},  # closes and reopens the collection
            {"db_exec": {"sql": "SELECT (SELECT mod FROM col), usn, mod FROM notes"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/db", "UTC", START, steps, notes=1))
        col_mod = out[0]["db_exec"]["rows"][0][0]
        t = epoch(START, "UTC")
        self.assertEqual(out[2], {"db_exec": {"rows": []}})
        # col.mod and the note's own mod stay where setup left them.
        self.assertEqual(out[4]["db_exec"]["rows"], [[col_mod, 7, t]])

    def test_arguments_are_checked_and_sql_errors_carry_no_message(self):
        bad = [
            "SELECT 1",
            {"sql": ""},
            {"sql": 1},
            {"params": []},
            {"sql": "SELECT ?", "params": [True]},
            {"sql": "SELECT ?", "params": [[1]]},
            {"sql": "SELECT ?", "params": [{"a": 1}]},
            {"sql": "SELECT ?", "params": [2**63]},
            {"sql": "SELECT ?", "params": [-(2**63) - 1]},
            {"sql": "SELECT ?", "params": None},
            {"sql": "SELECT 1", "bind": []},
        ]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, [{"observe": ["ids"]}, {"db_exec": args}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)
            limits = run_ok(self, w, scenario("t/db", "UTC", START, [
                {"db_exec": {"sql": "SELECT ?, ?", "params": [2**63 - 1, -(2**63)]}}]))
            error = run(w, [{"db_exec": {"sql": "SELECT nope FROM nowhere"}}])
        self.assertEqual(limits[0]["db_exec"]["rows"], [[2**63 - 1, -(2**63)]])
        self.assertEqual(error["error"], {"kind": "anki_error", "op": "db_exec", "exception": "DBError", "step": 0})


class EditNote(unittest.TestCase):
    def test_fields_and_tags_replace_what_they_name(self):
        steps = [
            {"edit_note": {"note": "#n1", "fields": ["Q1"]}},
            {"db_exec": {"sql": "SELECT flds FROM notes"}},
            {"edit_note": {"note": "#n1", "fields": {"A": "A2"}, "tags": ["b", "a"]}},
            {"db_exec": {"sql": "SELECT flds FROM notes"}},
            {"edit_note": {"note": "#n1", "fields": ["Q3", "A3"]}},
            {"db_exec": {"sql": "SELECT flds FROM notes"}},
            {"edit_note": {"note": "#n1", "tags": []}},
            {"db_exec": {"sql": "SELECT flds, trim(tags) FROM notes"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/edit", "UTC", START, steps, notes=1))
        # notes.flds joins the fields with U+001F (the schema-18 column format).
        self.assertEqual([out[i]["db_exec"]["rows"][0][0] for i in (1, 3, 5)], ["Q1\x1fa1", "Q1\x1fA2", "Q3\x1fA3"])
        self.assertEqual(out[7]["db_exec"]["rows"], [["Q3\x1fA3", ""]])
        self.assertEqual([out[i] for i in (0, 2, 4, 6)], [{}, {}, {}, {}])

    def test_bad_arguments_and_missing_notes(self):
        bad = [
            "#n1",
            {"fields": ["x"]},
            {"note": "#c1", "fields": ["x"]},
            {"note": "#n1", "fields": ["x", "y", "z"]},
            {"note": "#n1", "fields": {"Nope": "x"}},
            {"note": "#n1", "fields": [1]},
            {"note": "#n1", "fields": "x"},
            {"note": "#n1", "fields": None},
            {"note": "#n1", "tags": "x"},
            {"note": "#n1", "tags": [True]},
            {"note": "#n1", "deck": "A"},
        ]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, [{"edit_note": args}], notes=1)
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)
            unknown = run(w, [{"edit_note": {"note": "#n2", "fields": ["x"]}}], notes=1)
            gone = run(w, [{"delete_notes": {"notes": ["#n1"]}}, {"edit_note": {"note": "#n1", "fields": ["x"]}}], notes=1)
        self.assertEqual(unknown["error"], {"kind": "unknown_ref", "ref": "#n2", "step": 0})
        self.assertEqual((gone["error"]["kind"], gone["error"]["op"], gone["error"]["step"]), ("anki_error", "edit_note", 1))
        self.assertEqual(set(gone["error"]), {"kind", "op", "exception", "step"})


class DeleteNotes(unittest.TestCase):
    def test_existing_notes_go_and_missing_ones_are_skipped(self):
        t = epoch(START, "UTC") * 1000
        steps = [
            {"delete_notes": {"notes": ["#n1"]}},
            {"db_exec": {"sql": "SELECT id FROM notes ORDER BY id"}},
            {"delete_notes": {"notes": ["#n1", "#n2", "#n2"]}},
            {"db_exec": {"sql": "SELECT count(*) FROM notes"}},
            {"delete_notes": {"notes": ["#n3", "#n1"]}},
            {"observe": ["ids", "card(#c1, #c2)"]},
            {"db_exec": {"sql": "SELECT id FROM notes"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/del", "UTC", START, steps, notes=3))
        self.assertEqual([out[i] for i in (0, 2, 4)], [{}, {}, {}])
        self.assertEqual(out[1]["db_exec"]["rows"], [[t + 1], [t + 2]])
        self.assertEqual(out[3]["db_exec"]["rows"], [[1]])
        # Logical ids of deleted notes and cards stay valid.
        self.assertEqual(out[5]["ids"], {"#n1": t, "#n2": t + 1, "#n3": t + 2, "#c1": t, "#c2": t + 1, "#c3": t + 2})
        self.assertEqual(out[5]["card"], {"#c1": None, "#c2": None})
        self.assertEqual(out[6]["db_exec"]["rows"], [])

    def test_bad_arguments(self):
        bad = [["#n1"], {"notes": []}, {"notes": "#n1"}, {"notes": ["#c1"]}, {"notes": ["#n1"], "force": True}, {}]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, [{"delete_notes": args}], notes=1)
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)
            unknown = run(w, [{"delete_notes": {"notes": ["#n1", "#n5"]}}], notes=1)
        self.assertEqual(unknown["error"], {"kind": "unknown_ref", "ref": "#n5", "step": 0})


class SetConfig(unittest.TestCase):
    def test_values_keep_their_json_kinds(self):
        value = {"b": 1.0, "a": [1, -0.0, None, True, "x"], "big": 2**64 - 1, "low": -(2**63), "e": 1e16}
        steps = [
            {"set_config": {"key": "probe", "value": value}},
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT), typeof(val), usn FROM config WHERE KEY = 'probe'"}},
            {"set_config": {"key": "nothing", "value": None}},
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT) FROM config WHERE KEY = 'nothing'"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/cfg", "UTC", START, steps))
        text, storage, usn = out[1]["db_exec"]["rows"][0]
        self.assertEqual((storage, usn), ("blob", -1))
        # Floats stay floats (1.0, -0.0), integers stay integers.
        for part in ('"b":1.0', "[1,-0.0,null,true,", f'"big":{2**64 - 1}', f'"low":{-(2**63)}'):
            self.assertIn(part, text)
        self.assertNotIn('"b":1,', text)
        self.assertEqual(out[3]["db_exec"]["rows"], [["null"]])

    def test_bad_arguments(self):
        bad = [
            ["k", 1],
            {"key": "k"},
            {"value": 1},
            {"key": "", "value": 1},
            {"key": 1, "value": 1},
            {"key": "k", "value": 1, "undoable": True},
            {"key": "k", "value": 2**64},
            {"key": "k", "value": -(2**63) - 1},
            {"key": "k", "value": {"a": [1, {"b": 2**70}]}},
        ]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, [{"set_config": args}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class UndoHistory(unittest.TestCase):
    """W1, W2, W4 through one in-process session (the undo queue is not
    observable through the protocol)."""

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

    def test_edits_are_undoable_config_writes_keep_the_history_and_raw_writes_clear_it(self):
        col = self.s.col
        added = col.undo_status().undo  # setup's note
        self.step({"edit_note": {"note": "#n1", "fields": ["changed"]}})
        edit = col.undo_status().undo
        self.assertNotIn(edit, ("", added))
        self.step({"set_config": {"key": "probe", "value": 1}})
        self.assertEqual(col.undo_status().undo, edit)
        self.step({"db_exec": {"sql": "  select count(*) FROM notes"}})
        self.assertEqual(col.undo_status().undo, edit)
        self.step({"db_exec": {"sql": "UPDATE notes SET usn = 3"}})
        self.assertEqual(col.undo_status().undo, "")

    def test_deleting_notes_is_undoable(self):
        added = self.s.col.undo_status().undo  # setup's note
        self.step({"delete_notes": {"notes": ["#n1"]}})
        self.assertNotIn(self.s.col.undo_status().undo, ("", added))


if __name__ == "__main__":
    unittest.main()
