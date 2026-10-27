# SPDX-License-Identifier: AGPL-3.0-or-later
"""The add_defaults and notetype_last_deck observations (specs/notes/add-defaults.md
D1, D2, D7): the review deck, the last deck's readable forms, the two error
kinds, request checks, and that the collection never changes. Expected values
come from add-defaults D1/D2 and editor/config-keys K4.1, never from the
driver."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.session import Session

CARD = [{"name": "Card", "front": "{{Front}}", "back": "{{Back}}"}]


def nt(name):
    return {"name": name, "fields": ["Front", "Back"], "templates": CARD}


CRAM = {"db_exec": {"sql": "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES "
                           "(900, 'Cram', 0, 0, X'08011001', X'12130801120a0a066465636b3a78106428d804383c')"}}


def last_deck_key(notetype, value_sql):
    return {"db_exec": {"sql": "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) SELECT "
                               f"'_nt_' || id || '_lastDeck', 0, 0, {value_sql} FROM notetypes WHERE name = '{notetype}'"}}


def defaults(**args):
    return {"observe": [{"add_defaults": args}]}


def last(**args):
    return {"observe": [{"notetype_last_deck": args}]}


class AddDefaults(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, steps, setup=None):
        sc = {"id": "t/add-defaults", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        if setup is not None:
            sc["setup"] = setup
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def run_steps(self, steps, setup=None):
        s = self.session(steps, setup)
        try:
            return s.run_all()["steps"]
        finally:
            s.close()

    def test_review_deck_counts_when_the_current_deck_is_filtered(self):
        setup = {
            "decks": ["Spanish", "French"],
            "notetypes": [nt("Vocab"), nt("Idiom"), nt("Grammar")],
            "notes": [{"notetype": "Vocab", "deck": "Spanish", "fields": ["a"]},
                      {"notetype": "Idiom", "deck": "French", "fields": ["b"]},
                      {"notetype": "Idiom", "deck": "French", "fields": ["c"]}],
        }
        steps = [
            CRAM,
            {"db_exec": {"sql": "UPDATE cards SET odid = did, did = 900 WHERE id = (SELECT min(id) FROM cards)"}},
            {"db_exec": {"sql": "UPDATE config SET val = CAST((SELECT id FROM notetypes WHERE name = 'Grammar') AS BLOB) "
                                "WHERE KEY = 'curModel'"}},
            {"delete_notes": {"notes": ["#n3"]}},
            {"set_config": {"key": "addToCur", "value": True}},
            {"set_config": {"key": "curDeck", "value": 900}},
            defaults(review_card="#c1"), defaults(review_card="#c2"), defaults(review_card="#c3"), defaults(),
            {"set_config": {"key": "curDeck", "value": 1}},
            defaults(review_card="#c1"),
        ]
        out = [o["add_defaults"] for o in self.run_steps(steps, setup)[6:] if o]
        self.assertEqual(out, [
            {"deck": "Spanish", "notetype": "Vocab"},    # odid, and Spanish's last note type
            {"deck": "French", "notetype": "Idiom"},
            {"deck": "Default", "notetype": "Grammar"},  # deleted card: no review deck
            {"deck": "Default", "notetype": "Grammar"},
            {"deck": "Default", "notetype": "Grammar"},  # curDeck normal: the card is not read
        ])

    def test_last_deck_reads_only_an_integer_naming_a_normal_deck(self):
        setup = {"decks": ["Target"], "notetypes": [nt(n) for n in ("Absent", "Int", "Filtered", "Str", "Float", "Text")]}
        steps = [
            CRAM,
            {"db_exec": {"sql": "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) "
                                "VALUES (5, 'Five', 0, 0, X'08011001', X'0a020801')"}},
            last_deck_key("Int", "CAST(5 AS BLOB)"),
            last_deck_key("Filtered", "CAST(900 AS BLOB)"),
            last_deck_key("Str", "CAST('\"5\"' AS BLOB)"),
            last_deck_key("Float", "CAST('5.0' AS BLOB)"),
            last_deck_key("Text", "'5'"),
            {"db_exec": {"sql": "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('_nt_424242_lastDeck', 0, 0, CAST(5 AS BLOB))"}},
            {"set_config": {"key": "addToCur", "value": True}},
            last(notetype="Absent"), last(notetype="Int"), last(notetype="Filtered"),
            last(notetype="Str"), last(notetype="Float"), last(notetype="Text"), last(notetype_id=424242),
        ]
        out = [o["notetype_last_deck"]["deck"] for o in self.run_steps(steps, setup)[9:]]
        # D2: addToCur true is not read; storage/config-table C3's typed read.
        self.assertEqual(out, [None, "Five", None, None, None, None, "Five"])

    def test_error_kinds(self):
        setup = {"notetypes": [nt("Vocab")]}
        no_notetype = [{"db_exec": {"sql": "DELETE FROM notetypes"}}, {"reopen": {}}, defaults()]
        self.assertEqual(self.run_steps(no_notetype, setup)[-1]["add_defaults"], {"error": "invalid_argument"})
        no_deck = [CRAM, {"db_exec": {"sql": "DELETE FROM decks WHERE id = 1"}}, {"reopen": {}},
                   {"set_config": {"key": "curDeck", "value": 900}}, {"set_config": {"key": "addToCur", "value": True}},
                   defaults()]
        self.assertEqual(self.run_steps(no_deck, setup)[-1]["add_defaults"], {"error": "not_found"})

    def test_the_collection_never_changes(self):
        setup = {"decks": ["Spanish"], "notetypes": [nt("Vocab")],
                 "notes": [{"notetype": "Vocab", "deck": "Spanish", "fields": ["a"]}]}
        steps = [{"observe": ["db_dump"]}, defaults(review_card="#c1"), last(notetype="Vocab"), {"observe": ["db_dump"]}]
        out = self.run_steps(steps, setup)
        self.assertEqual(out[1]["add_defaults"], {"deck": "Default", "notetype": "Vocab"})  # addToCur true, curDeck 1
        self.assertEqual(out[2]["notetype_last_deck"], {"deck": "Spanish"})
        self.assertEqual(out[3]["db_dump"], out[0]["db_dump"])

    def test_requests_outside_d7_are_invalid_before_any_collection(self):
        bad = [
            {"add_defaults": None}, {"add_defaults": []}, {"add_defaults": {"review_card": "#n1"}},
            {"add_defaults": {"review_card": 1}}, {"add_defaults": {"preset_deck": "A"}},
            {"notetype_last_deck": None}, {"notetype_last_deck": {}}, {"notetype_last_deck": {"notetype": ""}},
            {"notetype_last_deck": {"notetype": "A", "notetype_id": 1}}, {"notetype_last_deck": {"notetype_id": True}},
            {"notetype_last_deck": {"notetype_id": "1"}}, {"notetype_last_deck": {"notetype_id": 2**63}},
            {"notetype_last_deck": {"deck": "A"}},
        ]
        for req in bad:
            with self.subTest(req=req), self.assertRaises(DriverError) as ctx:
                self.session([{"observe": ["ids"]}, {"observe": [req]}])
            self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("invalid_scenario", 1))
        for twice in ([{"add_defaults": {}}, {"add_defaults": {}}],
                      [{"notetype_last_deck": {"notetype": "A"}}, {"notetype_last_deck": {"notetype": "B"}}]):
            with self.subTest(twice=twice), self.assertRaises(DriverError) as ctx:
                self.session([{"observe": twice}])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")

    def test_names_and_ids_are_checked_when_the_step_runs(self):
        setup = {"notetypes": [nt("Vocab")]}
        for steps, kind in (([last(notetype="vocab")], "unknown_ref"), ([defaults(review_card="#c1")], "unknown_ref")):
            with self.subTest(steps=steps), self.assertRaises(DriverError) as ctx:
                self.run_steps(steps, setup)
            self.assertEqual(ctx.exception.kind, kind)
        s = self.session([], setup)
        try:
            s.run_all()
            ntid = s.col.db.scalar("select id from notetypes where name = 'Vocab'")
            with self.assertRaises(DriverError) as ctx:
                registry.OBSERVATIONS["notetype_last_deck"](s, {"notetype_id": ntid})
            self.assertEqual(ctx.exception.kind, "invalid_scenario")
        finally:
            s.close()

if __name__ == "__main__":
    unittest.main()
