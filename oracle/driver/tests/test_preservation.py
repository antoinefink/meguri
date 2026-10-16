# SPDX-License-Identifier: AGPL-3.0-or-later
"""adopt and the raw db_dump (specs/conformance/normalized-dump.md N4, N5)
through real workers. The fixture is made here with Anki and sqlite3, with ids
the test assigns; expected digests come from the stored bytes as SQLite's hex()
returns them, never from the driver's dump code."""

from __future__ import annotations

import hashlib
import os
import shutil
import sqlite3
import tempfile
import unittest

from anki.collection import Collection

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
# Note ids in the order the notes were added: A, B, C. Card ids per note.
NOTE_IDS = {"A": 3000, "B": 1000, "C": 2000}
CARD_IDS = {"A": 10, "B": 30, "C": 20}
CLOZE = {
    "name": "Cz",
    "kind": "cloze",
    "fields": ["Text", "Extra"],
    "templates": [{"name": "Cloze", "front": "{{cloze:Text}}", "back": "{{cloze:Text}}<br>{{Extra}}"}],
    "css": ".card {}",
}


def make_fixture(root: str) -> str:
    """preservation/three.anki2: three Basic notes A, B and C added in that
    order, then given ids out of that order (NOTE_IDS, CARD_IDS)."""
    os.makedirs(os.path.join(root, "preservation"))
    work = tempfile.mkdtemp(dir=root)
    col = Collection(os.path.join(work, "collection.anki2"))
    basic = col.models.by_name("Basic")
    for front in "ABC":
        note = col.new_note(basic)
        note["Front"] = front
        col.add_note(note, 1)
    col.close(downgrade=False)
    path = os.path.join(root, "preservation", "three.anki2")
    shutil.copyfile(os.path.join(work, "collection.anki2"), path)
    shutil.rmtree(work)
    con = sqlite3.connect(path)
    # Only integer columns change, so any comparison serves for unicase.
    con.create_collation("unicase", lambda a, b: (a > b) - (a < b))
    for front in "ABC":
        nid = con.execute("select id from notes where sfld = ?", (front,)).fetchone()[0]
        con.execute("update cards set id = ?, nid = ? where nid = ?", (CARD_IDS[front], NOTE_IDS[front], nid))
        con.execute("update notes set id = ? where id = ?", (NOTE_IDS[front], nid))
    con.commit()
    con.close()
    return "preservation/three.anki2"


def sha_hex(text: str) -> dict:
    return {"sha256": hashlib.sha256(bytes.fromhex(text)).hexdigest()}


class Adopt(unittest.TestCase):
    def setUp(self):
        # The TAP runner runs each test on its own, so no class fixtures.
        self.root = tempfile.mkdtemp(prefix="fixtures-", dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        self.fixture = make_fixture(self.root)
        self.worker = WorkerProcess("UTC", extra_env={"ORACLE_FIXTURES": self.root})

    def tearDown(self):
        self.worker.close()
        shutil.rmtree(self.root, ignore_errors=True)

    def opened(self, steps):
        sc = {"id": "t/adopt", "clock": {"tz": "UTC", "start": START}, "steps": [{"open": {"fixture": self.fixture}}] + steps}
        resp = self.worker.request({"cmd": "run", "scenario": sc})
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"]

    def test_notes_then_cards_get_logical_ids_in_ascending_id_order(self):
        out = self.opened([
            {"observe": ["ids"]},
            {"adopt": {}},
            {"observe": ["ids", "card(#c1, #c2, #c3)"]},
        ])
        self.assertEqual(out[1], {"ids": {}})
        self.assertEqual(out[2], {"adopt": {"notes": 3, "cards": 3}})
        self.assertEqual(out[3]["ids"], {
            "#n1": 1000, "#n2": 2000, "#n3": 3000,
            "#c1": 10, "#c2": 20, "#c3": 30,
        })
        # Card 10 is A's (note 3000), card 30 B's (note 1000).
        self.assertEqual({ref: card["nid"] for ref, card in out[3]["card"].items()}, {"#c1": "#n3", "#c2": "#n2", "#c3": "#n1"})

    def test_adopt_changes_nothing_and_numbers_only_what_has_no_id(self):
        raw = {"db_dump": {"raw": True}}
        out = self.opened([
            {"observe": [raw, "schema_objects"]},
            {"adopt": {}},
            {"adopt": {}},
            {"observe": [raw, "schema_objects"]},
            {"reopen": {}},
            {"observe": [raw, "schema_objects"]},
        ])
        self.assertEqual(out[3], {"adopt": {"notes": 0, "cards": 0}})
        self.assertEqual(out[4], out[1])
        # Nothing was waiting to be written at close either.
        self.assertEqual(out[6], out[1])

    def test_a_card_an_edit_adds_gets_the_next_card_id(self):
        setup = {"notetypes": [CLOZE], "notes": [{"notetype": "Cz", "deck": "A", "fields": ["{{c1::a}}", ""]}]}
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/adopt", "UTC", START, [
                {"adopt": {}},
                {"edit_note": {"note": "#n1", "fields": {"Text": "{{c1::a}} {{c2::b}}"}}},
                {"observe": ["ids"]},
                {"adopt": {}},
                {"observe": ["ids", "card(#c2)"]},
                {"db_exec": {"sql": "SELECT id FROM cards WHERE ord = 1"}},
            ], setup=setup))
        self.assertEqual(out[0], {"adopt": {"notes": 0, "cards": 0}})
        self.assertEqual(sorted(out[2]["ids"]), ["#c1", "#n1"])
        self.assertEqual(out[3], {"adopt": {"notes": 0, "cards": 1}})
        self.assertEqual(out[4]["ids"]["#c2"], out[5]["db_exec"]["rows"][0][0])
        self.assertEqual((out[4]["card"]["#c2"]["nid"], out[4]["card"]["#c2"]["ord"]), ("#n1", 1))

    def test_adopt_takes_an_empty_mapping(self):
        for args in (None, [], {"notes": True}, "all"):
            resp = self.worker.request({"cmd": "run", "scenario": {
                "id": "t/adopt", "clock": {"tz": "UTC", "start": START},
                "steps": [{"open": {"fixture": self.fixture}}, {"adopt": args}],
            }})
            self.assertFalse(resp["ok"], args)
            self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), (args, resp))

    def test_actor_scenarios_have_no_adopt(self):
        resp = self.worker.request({"cmd": "run", "scenario": {
            "id": "t/adopt", "clock": {"tz": "UTC", "start": START},
            "actors": ["client_a"], "steps": [{"actor": "client_a"}, {"adopt": {}}],
        }})
        self.assertFalse(resp["ok"], resp)
        self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)


class RawDump(unittest.TestCase):
    def test_raw_keeps_guids_and_digests_every_stored_byte(self):
        tables = ["notes", "fields", "templates"]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/raw", "UTC", START, [
                {"observe": [{"db_dump": {"tables": tables, "raw": True}}]},
                {"observe": [{"db_dump": {"tables": tables}}]},
                {"db_exec": {"sql": "SELECT id, guid FROM notes"}},
                {"db_exec": {"sql": "SELECT ntid || ',' || ord, hex(config) FROM fields"}},
                {"db_exec": {"sql": "SELECT ntid || ',' || ord, hex(config) FROM templates"}},
            ], notes=2))
        raw, plain = out[0]["db_dump"], out[1]["db_dump"]
        self.assertEqual(sorted(raw), sorted(tables))
        guids = {str(nid): guid for nid, guid in out[2]["db_exec"]["rows"]}
        self.assertEqual({key: row["guid"] for key, row in raw["notes"].items()}, guids)
        self.assertEqual({row["guid"] for row in plain["notes"].values()}, {"#random"})
        for step, table in ((3, "fields"), (4, "templates")):
            stored = {key: sha_hex(hexed) for key, hexed in out[step]["db_exec"]["rows"]}
            self.assertEqual({key: row["config"] for key, row in raw[table].items()}, stored, table)
            # The plain digest leaves out the random field or template id.
            for key, row in plain[table].items():
                self.assertNotEqual(row["config"], stored[key], (table, key))
        # Everything but the random values reads the same in both forms.
        for table in tables:
            for key, row in raw[table].items():
                same = {c: v for c, v in row.items() if (table, c) not in (("notes", "guid"), ("fields", "config"), ("templates", "config"))}
                self.assertEqual({c: plain[table][key][c] for c in same}, same, (table, key))

    def test_raw_without_tables_dumps_all_twelve_and_raw_false_is_the_plain_dump(self):
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/raw", "UTC", START, [
                {"observe": [{"db_dump": {"raw": True}}]},
                {"observe": ["db_dump"]},
                {"observe": [{"db_dump": {"raw": False}}]},
                {"observe": [{"db_dump": {}}]},
                {"observe": [{"db_dump": {"tables": ["notes"], "raw": False}}]},
            ], notes=1))
        self.assertEqual(sorted(out[0]["db_dump"]), sorted([
            "col", "notes", "cards", "revlog", "graves", "decks", "deck_config",
            "notetypes", "fields", "templates", "tags", "config",
        ]))
        self.assertEqual(out[2], out[1])
        self.assertEqual(out[3], out[1])
        self.assertEqual(out[4]["db_dump"], {"notes": out[1]["db_dump"]["notes"]})
        self.assertNotEqual(out[0]["db_dump"]["notes"], out[1]["db_dump"]["notes"])

    def test_raw_is_a_boolean(self):
        with WorkerProcess("UTC") as w:
            for args in ({"raw": 1}, {"raw": "true"}, {"raw": None}, {"tables": ["notes"], "raw": True, "x": 1}):
                resp = w.request({"cmd": "run", "scenario": scenario("t/raw", "UTC", START, [{"observe": [{"db_dump": args}]}])})
                self.assertFalse(resp["ok"], args)
                self.assertEqual(resp["error"]["kind"], "invalid_scenario", (args, resp))
