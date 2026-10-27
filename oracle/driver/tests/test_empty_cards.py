# SPDX-License-Identifier: AGPL-3.0-or-later
"""Empty Cards ops (specs/conformance/empty-cards-ops.md Y1-Y3) through real
workers. Expected values come from the scenarios (note types, fields, edits)
and the spec texts (specs/notes/empty-cards.md E1, E2, E4 and E5: what is
empty, the report order, the screen's keep rule, Anki deleting what it
listed), never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
OPTIONAL = {
    "name": "Beta Optional",
    "fields": ["Front", "Back", "Add Reverse"],
    "templates": [
        {"name": "Card 1", "front": "{{Front}}", "back": "{{Back}}"},
        {"name": "Card 2", "front": "{{#Add Reverse}}{{Back}}{{/Add Reverse}}", "back": "{{Front}}"},
    ],
}
CLOZE = {
    "name": "alpha Cloze",
    "kind": "cloze",
    "fields": ["Text", "Back Extra"],
    "templates": [{"name": "Cloze", "front": "{{cloze:Text}}", "back": "{{cloze:Text}}"}],
}
PLAIN = {"name": "Plain", "fields": ["Front"], "templates": [{"name": "Card 1", "front": "{{Front}}"}]}
SETUP = {
    "notetypes": [OPTIONAL, CLOZE, PLAIN],
    "notes": [
        {"notetype": "Beta Optional", "fields": ["a", "A", "y"]},  # n1: c1, c2
        {"notetype": "Beta Optional", "fields": ["", "B", "y"]},  # n2: c3 (card 2 only)
        {"notetype": "Beta Optional", "fields": ["c", "C", "y"]},  # n3: c4, c5
        {"notetype": "alpha Cloze", "fields": ["{{c1::x}} {{c2::y}}", ""]},  # n4: c6, c7
        {"notetype": "Plain", "fields": ["t"]},  # n5: c8
    ],
}
# A note and its card written raw: no logical id; its front renders nothing.
RAW_NOTE = (
    "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data) "
    "SELECT 1000, 'raw', id, 0, 0, '', '', '', 0, 0, '' FROM notetypes WHERE name = 'Plain'"
)
RAW_CARD = (
    "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) "
    "VALUES (2000, 1000, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, '')"
)
EDITS = [
    {"edit_note": {"note": "#n1", "fields": {"Add Reverse": ""}}},
    {"edit_note": {"note": "#n2", "fields": {"Add Reverse": ""}}},
    {"edit_note": {"note": "#n3", "fields": {"Front": "", "Add Reverse": ""}}},
    {"edit_note": {"note": "#n4", "fields": {"Text": "no cloze left"}}},
]
GRAVES = {"db_exec": {"sql": "SELECT type, count() FROM graves GROUP BY type ORDER BY type"}}


def sc(steps):
    return scenario("t/empty", "UTC", START, steps, setup=SETUP)


def rows(step):
    return step["db_exec"]["rows"]


def note(ref, ords, cards, all_empty):
    return {"note": ref, "ords": ords, "cards": cards, "all_empty": all_empty}


class EmptyCards(unittest.TestCase):
    def test_scan_order_flags_and_the_screen_delete_from_the_kept_report(self):
        steps = [{"empty_cards": {}}] + EDITS + [
            {"db_exec": {"sql": RAW_NOTE}},
            {"db_exec": {"sql": RAW_CARD}},
            {"empty_cards": {}},
            {"delete_empty_cards": {"keep_notes": True}},
            {"observe": ["card(#c2, #c3, #c4, #c5, #c6, #c7)"]},
            GRAVES,
            # The same report again, without keep: what is left of it.
            {"delete_empty_cards": {"keep_notes": False}},
            GRAVES,
            {"db_exec": {"sql": "SELECT sfld FROM notes ORDER BY id"}},
            {"empty_cards": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[0], {"empty_cards": []})
        # E2: note types by name under unicase, notes by id; E1: no G6 card.
        self.assertEqual(out[7], {"empty_cards": [
            {"notetype": "alpha Cloze", "notes": [note("#n4", [0, 1], 2, True)]},
            {"notetype": "Beta Optional", "notes": [
                note("#n1", [1], 2, False), note("#n2", [1], 1, True), note("#n3", [0, 1], 2, True),
            ]},
            {"notetype": "Plain", "notes": [note(1000, [0], 1, True)]},
        ]})
        # E4 with keep: every listed card but each all-empty note's first.
        self.assertEqual(out[8], {"delete_empty_cards": {"deleted": 3, "kept_valid": 0}})
        cards = out[9]["card"]
        self.assertEqual({ref: cards[ref] is None for ref in cards},
                         {"#c2": True, "#c3": False, "#c4": False, "#c5": True, "#c6": False, "#c7": True})
        self.assertEqual(rows(out[10]), [[0, 3]])
        # The listed cards still there (c3, c4, c6 and the raw one) go, then their notes.
        self.assertEqual(out[11], {"delete_empty_cards": {"deleted": 4, "kept_valid": 0}})
        self.assertEqual(rows(out[12]), [[0, 7], [1, 4]])
        self.assertEqual(rows(out[13]), [["a"], ["t"]])
        self.assertEqual(out[14], {"empty_cards": []})

    def test_the_delete_removes_listed_cards_that_became_valid(self):
        steps = [
            EDITS[0],
            {"empty_cards": {}},
            {"edit_note": {"note": "#n1", "fields": {"Add Reverse": "y"}}},
            {"delete_empty_cards": {"keep_notes": False}},
            {"observe": ["card(#c2)"]},
            {"empty_cards": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[1], {"empty_cards": [{"notetype": "Beta Optional", "notes": [note("#n1", [1], 2, False)]}]})
        # E5: Anki does not look again.
        self.assertEqual(out[3], {"delete_empty_cards": {"deleted": 1, "kept_valid": 0}})
        self.assertEqual(out[4], {"card": {"#c2": None}})
        self.assertEqual(out[5], {"empty_cards": []})

    def test_the_scan_reads_note_type_rows_a_db_exec_wrote(self):
        # A db_exec leaves Anki's note type cache alone; Meguri reads the rows.
        # Plain's template shows {{Front}}, which no field is called any more.
        steps = [
            {"empty_cards": {}},
            {"db_exec": {"sql": "UPDATE fields SET name = 'Upper' WHERE ntid = (SELECT id FROM notetypes WHERE name = 'Plain')"}},
            {"empty_cards": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[0], {"empty_cards": []})
        self.assertEqual(out[2], {"empty_cards": [{"notetype": "Plain", "notes": [note("#n5", [0], 1, True)]}]})

    def test_bad_arguments_and_a_delete_without_a_scan_are_invalid(self):
        bad = [
            [{"delete_empty_cards": {"keep_notes": True}}],
            [{"empty_cards": {}}, {"delete_empty_cards": {}}],
            # The scan of another segment (Y1, D9.2): refused before any collection.
            [{"empty_cards": {}}, {"handoff": "oracle"}, {"delete_empty_cards": {"keep_notes": True}}],
            [{"empty_cards": {}}, {"delete_empty_cards": {"keep_notes": 1}}],
            [{"empty_cards": {}}, {"delete_empty_cards": {"keep_notes": False, "x": 1}}],
            [{"empty_cards": {"x": 1}}],
            [{"empty_cards": None}],
        ]
        with WorkerProcess("UTC") as w:
            resps = [w.request({"cmd": "run", "scenario": sc(steps)}) for steps in bad]
        for steps, resp in zip(bad, resps):
            with self.subTest(steps=steps):
                self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", len(steps) - 1), resp)


if __name__ == "__main__":
    unittest.main()
