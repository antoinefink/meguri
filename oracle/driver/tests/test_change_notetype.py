# SPDX-License-Identifier: AGPL-3.0-or-later
"""Change Note Type ops (specs/conformance/change-notetype-ops.md H1-H3)
through real workers. Expected values come from the scenarios (names, notes,
clock) and the spec texts (specs/notetypes/change-notetype.md M2 to M9: the
default-map tables, the selection rules, the preview and count definitions,
the stamps), never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok, scenario

START = "2030-01-15T10:00:00"
NOW = epoch(START, "UTC")
PAIR = {"name": "Pair", "fields": ["Front", "Back"], "templates": [{"name": "Card 1", "front": "{{Front}}", "back": "{{Back}}"}]}
PAIR_REVERSE = {
    "name": "Pair Reverse",
    "fields": ["Front", "Back", "Extra"],
    "templates": [
        {"name": "Card 1", "front": "{{Front}}", "back": "{{Back}}"},
        {"name": "Card 2", "front": "{{Back}}", "back": "{{Front}}"},
    ],
}
VOCAB = {
    "name": "Vocab",
    "fields": ["Back", "Notes", "Front", "More"],
    "templates": [
        {"name": "Recognition", "front": "{{Front}}", "back": "{{Back}}"},
        {"name": "Production", "front": "{{Back}}", "back": "{{Front}}"},
    ],
}
CASED = {"name": "Cased", "fields": ["Back", "front", "Extra"], "templates": [{"name": "Card 1", "front": "{{front}}"}]}
FLIP = {"name": "Flip", "fields": ["Extra", "Front"], "templates": [{"name": "Card 1", "front": "{{Front}}"}]}
CLOZE = {
    "name": "Cloze Text",
    "kind": "cloze",
    "fields": ["Text", "Back Extra"],
    "templates": [{"name": "Cloze", "front": "{{cloze:Text}}", "back": "{{cloze:Text}}"}],
}
SINGLE = {"name": "Single", "fields": ["Text"], "templates": [{"name": "Card 1", "front": "{{Text}}"}]}
SETUP = {
    "decks": ["Words"],
    "notetypes": [PAIR, PAIR_REVERSE, VOCAB, CASED, FLIP, CLOZE, SINGLE],
    "notes": [
        {"notetype": "Pair", "deck": "Words", "fields": ["one", ""]},  # n1
        {"notetype": "Pair", "deck": "Words", "fields": ["two", "deux"]},  # n2
        {"notetype": "Pair Reverse", "deck": "Words", "fields": ["tri", "trio", "x"]},  # n3: 2 cards
        {"notetype": "Cased", "deck": "Words", "fields": ["b", "f", "e"]},  # n4
        {"notetype": "Cloze Text", "deck": "Words", "fields": ["{{c1::a}} {{c3::b}}", ""]},  # n5: ords 0 and 2
        {"notetype": "Single", "deck": "Words", "fields": ["t"]},  # n6
    ],
}
NOTES_ROWS = "SELECT n.sfld, t.name, replace(n.flds, char(31), '|') FROM notes n JOIN notetypes t ON t.id = n.mid ORDER BY n.id"


def sc(steps, setup=SETUP):
    return scenario("t/change", "UTC", START, steps, setup=setup)


def info(notes, notetype):
    return {"change_notetype_info": {"notes": notes, "notetype": notetype}}


def rows(step):
    return step["db_exec"]["rows"]


def preview(out):
    keys = ("fields", "templates", "discarded_fields", "dropped_templates", "removed_cards")
    return tuple(out["change_notetype_info"][k] for k in keys)


class Info(unittest.TestCase):
    def test_names_default_maps_preview_counts_and_stamp(self):
        steps = [
            info(["#n1", "#n2"], "Pair Reverse"),
            info(["#n3"], "Vocab"),
            info(["#n3"], "Pair"),
            info(["#n4"], "Flip"),
            info(["#n5"], "Pair Reverse"),
            info(["#n6"], "Cloze Text"),
            {"db_exec": {"sql": "SELECT scm FROM col"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        first = out[0]["change_notetype_info"]
        self.assertEqual(
            {k: first[k] for k in ("old_notetype", "new_notetype", "old_fields", "new_fields", "old_templates", "new_templates")},
            {
                "old_notetype": "Pair",
                "new_notetype": "Pair Reverse",
                "old_fields": ["Front", "Back"],
                "new_fields": ["Front", "Back", "Extra"],
                "old_templates": ["Card 1"],
                "new_templates": ["Card 1", "Card 2"],
            },
        )
        # M5: n1's Back is empty; both notes have one card of ordinal 0; the stamp is col.scm.
        self.assertEqual((first["field_content"], first["card_counts"]), ([2, 1], [[0, 2]]))
        self.assertEqual([first["schema"]], rows(out[6])[0])
        # M2 and M3 tables, M6 for the default maps.
        self.assertEqual(preview(out[0]), ([0, 1, None], [0, None], [], [], 0))
        self.assertEqual(preview(out[1]), ([1, 2, 0, None], [0, 1], [], [], 0))
        self.assertEqual(preview(out[2]), ([0, 1], [0], [2], [1], 1))
        self.assertEqual(preview(out[3]), ([2, 0], [0], [1], [], 0))
        # M1: no template map when either side is cloze; cloze to standard
        # removes the cards at or past the new template count (n5's ordinal 2).
        self.assertEqual(preview(out[4]), ([0, 1, None], None, [], None, 1))
        self.assertEqual(out[4]["change_notetype_info"]["card_counts"], [[0, 1], [2, 1]])
        self.assertEqual(out[4]["change_notetype_info"]["old_templates"], ["Cloze"])
        self.assertEqual(preview(out[5]), ([0, None], None, [], None, 0))
        self.assertEqual(out[1]["change_notetype_info"]["card_counts"], [[0, 1], [1, 1]])
        self.assertEqual(out[1]["change_notetype_info"]["field_content"], [1, 1, 1])

    def test_field_content_counts_any_stored_text_and_missing_fields_as_empty(self):
        steps = [
            # A damaged note: one stored field for a two-field note type.
            {"db_exec": {"sql": "UPDATE notes SET flds = 'solo' WHERE sfld = 'two'"}},
            {"edit_note": {"note": "#n1", "fields": {"Back": " "}}},
            info(["#n1", "#n2"], "Single"),
            {"edit_note": {"note": "#n1", "fields": {"Back": "<br>"}}},
            info(["#n1"], "Single"),
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[2]["change_notetype_info"]["field_content"], [2, 1])
        self.assertEqual(out[4]["change_notetype_info"]["field_content"], [1, 1])

    def test_the_info_reads_note_type_rows_a_db_exec_wrote(self):
        # A db_exec leaves Anki's note type cache alone; Meguri reads the rows.
        steps = [
            info(["#n1"], "Pair Reverse"),
            {"db_exec": {"sql": "UPDATE notetypes SET name = 'Duo' WHERE name = 'Pair'"}},
            {"db_exec": {"sql": "UPDATE fields SET name = 'Upper' WHERE ord = 0 AND ntid = (SELECT id FROM notetypes WHERE name = 'Duo')"}},
            info(["#n1"], "Pair Reverse"),
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        second = out[3]["change_notetype_info"]
        self.assertEqual((second["old_notetype"], second["old_fields"]), ("Duo", ["Upper", "Back"]))

    def test_the_selection_check_refuses_as_browse_does(self):
        cases = [
            ([info(["#n1", "#n3"], "Pair")], "multiple_notetypes"),
            ([{"delete_notes": {"notes": ["#n2"]}}, info(["#n2", "#n1"], "Pair Reverse")], "not_found"),
            ([{"delete_notes": {"notes": ["#n2"]}}, info(["#n1", "#n2"], "Pair Reverse")], "multiple_notetypes"),
            # M11: Anki's check counts a note listed twice twice.
            ([info(["#n1", "#n1"], "Pair Reverse")], "multiple_notetypes"),
        ]
        with WorkerProcess("UTC") as w:
            outs = [run_ok(self, w, sc(steps)) for steps, _ in cases]
        for (steps, kind), out in zip(cases, outs):
            with self.subTest(steps=steps):
                self.assertEqual(out[-1], {"change_notetype_info": {"error": kind}})


class Change(unittest.TestCase):
    def test_the_change_rewrites_notes_creates_cards_without_logical_ids_and_stamps_the_schema(self):
        steps = [
            {"advance": "1h"},
            info(["#n1", "#n2"], "Pair Reverse"),
            {"change_notetype": {}},
            {"db_exec": {"sql": NOTES_ROWS + " LIMIT 3"}},
            {"db_exec": {"sql": "SELECT n.sfld, c.ord FROM cards c JOIN notes n ON n.id = c.nid WHERE n.sfld IN ('one', 'two') ORDER BY n.id, c.ord"}},
            {"observe": ["ids"]},
            {"db_exec": {"sql": "SELECT scm, mod FROM col"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[2], {"change_notetype": {"notes": 2}})
        self.assertEqual(rows(out[3]), [["one", "Pair Reverse", "one||"], ["two", "Pair Reverse", "two|deux|"], ["tri", "Pair Reverse", "tri|trio|x"]])
        # M9 step 3: card 2 only for the note whose Back has text.
        self.assertEqual(rows(out[4]), [["one", 0], ["two", 0], ["two", 1]])
        # The eight setup cards keep #c1..#c8; the new card has no logical id.
        self.assertEqual(sorted(k for k in out[5]["ids"] if k.startswith("#c")), sorted(f"#c{i}" for i in range(1, 9)))
        later = (NOW + 3600) * 1000
        self.assertEqual(rows(out[6]), [[later, later]])

    def test_the_change_generates_cards_from_the_stored_templates_and_fields(self):
        # Pair Reverse's Card 2 shows {{Back}}: once the field is renamed in
        # the rows after the info, the change makes no second card (M9).
        steps = [
            info(["#n2"], "Pair Reverse"),
            {"db_exec": {"sql": "UPDATE fields SET name = 'Verso' WHERE ord = 1 AND ntid = (SELECT id FROM notetypes WHERE name = 'Pair Reverse')"}},
            {"change_notetype": {}},
            {"db_exec": {"sql": "SELECT c.ord FROM cards c JOIN notes n ON n.id = c.nid WHERE n.sfld = 'two' ORDER BY c.ord"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[2], {"change_notetype": {"notes": 1}})
        self.assertEqual(rows(out[3]), [[0]])

    def test_a_handoff_forgets_the_kept_info(self):
        steps = [info(["#n1"], "Pair Reverse"), {"handoff": "meguri"}, info(["#n1"], "Pair Reverse"), {"change_notetype": {}}]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[3], {"change_notetype": {"notes": 1}})

    def test_template_maps_renumber_the_same_card_and_generate_the_rest(self):
        steps = [
            info(["#n1"], "Pair Reverse"),
            {"change_notetype": {"fields": [0, 0, None], "templates": [None, 0]}},
            {"observe": ["card(#c1)"]},
            {"db_exec": {"sql": "SELECT c.ord, replace(n.flds, char(31), '|') FROM cards c JOIN notes n ON n.id = c.nid WHERE n.sfld = 'one' ORDER BY c.ord"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual(out[1], {"change_notetype": {"notes": 1}})
        # M9.1: the old card becomes ordinal 1; the new ordinal 0 card is generated (Front has text).
        self.assertEqual(out[2]["card"]["#c1"]["ord"], 1)
        self.assertEqual(rows(out[3]), [[0, "one|one|"], [1, "one|one|"]])

    def test_refusals_change_nothing_and_keep_the_info(self):
        steps = [
            info(["#n3"], "Pair Reverse"),
            {"change_notetype": {}},  # M7
            {"change_notetype": {"fields": [0, 1, 2], "templates": [0, 1]}},  # M7: explicit identity
            {"advance": "1m"},
            {"db_exec": {"sql": "UPDATE col SET scm = scm + 1"}},
            {"db_exec": {"sql": "SELECT scm, mod FROM col"}},
            {"change_notetype": {"fields": [1, 0, 2]}},  # M8 check 3
            {"db_exec": {"sql": "SELECT scm, mod FROM col"}},
            {"db_exec": {"sql": "SELECT replace(flds, char(31), '|') FROM notes WHERE sfld = 'tri'"}},
            info(["#n1", "#n2"], "Pair Reverse"),
            {"delete_notes": {"notes": ["#n2"]}},
            {"advance": "1m"},
            {"db_exec": {"sql": "SELECT scm, mod FROM col"}},
            {"change_notetype": {}},  # M8 check 4
            {"db_exec": {"sql": "SELECT scm, mod FROM col"}},
            {"db_exec": {"sql": "SELECT t.name, (SELECT count() FROM cards WHERE nid = n.id) FROM notes n JOIN notetypes t ON t.id = n.mid WHERE n.sfld = 'one'"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc(steps))
        self.assertEqual([out[i] for i in (1, 2, 6, 13)], [
            {"change_notetype": {"error": "no_changes"}},
            {"change_notetype": {"error": "no_changes"}},
            {"change_notetype": {"error": "schema_changed"}},
            {"change_notetype": {"error": "not_found"}},
        ])
        self.assertEqual(rows(out[7]), rows(out[5]))
        self.assertEqual(rows(out[8]), [["tri|trio|x"]])
        self.assertEqual(rows(out[14]), rows(out[12]))
        self.assertEqual(rows(out[15]), [["Pair", 1]])

    def test_a_change_without_a_kept_info_or_with_a_bad_map_is_invalid(self):
        pair = [info(["#n1"], "Pair Reverse")]
        bad = [
            [{"change_notetype": {}}],
            pair + [{"change_notetype": {"fields": [0, 1]}}],
            pair + [{"change_notetype": {"fields": [0, 1, 2]}}],
            pair + [{"change_notetype": {"fields": [0, 1, True]}}],
            pair + [{"change_notetype": {"fields": [0, 1, -1]}}],
            pair + [{"change_notetype": {"templates": [0, 0]}}],
            pair + [{"change_notetype": {"templates": [0]}}],
            pair + [{"change_notetype": {"templates": [0, 1]}}],
            pair + [{"change_notetype": {"other": 1}}],
            pair + [{"change_notetype": None}],
            [info(["#n5"], "Pair Reverse"), {"change_notetype": {"templates": [0, None]}}],
            # A refused info forgets the kept one (H1).
            pair + [info(["#n1", "#n3"], "Pair"), {"change_notetype": {}}],
            # The info of another segment (H1, D9.2): refused before any collection.
            pair + [{"handoff": "meguri"}, {"change_notetype": {}}],
            [{"change_notetype_info": {"notes": [], "notetype": "Pair"}}],
            [{"change_notetype_info": {"notes": ["#c1"], "notetype": "Pair"}}],
            [{"change_notetype_info": {"notes": ["#n1"]}}],
        ]
        with WorkerProcess("UTC") as w:
            resps = [w.request({"cmd": "run", "scenario": sc(steps)}) for steps in bad]
            unknown = w.request({"cmd": "run", "scenario": sc([info(["#n1"], "Nope")])})
        for steps, resp in zip(bad, resps):
            with self.subTest(steps=steps):
                self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", len(steps) - 1), resp)
        self.assertEqual(unknown["error"]["kind"], "unknown_ref", unknown)


if __name__ == "__main__":
    unittest.main()
