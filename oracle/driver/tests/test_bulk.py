# SPDX-License-Identifier: AGPL-3.0-or-later
"""find_replace, change_deck and find_replace_fields
(specs/conformance/bulk-ops.md X1-X3) through real workers. Expected values
come from the arguments and the behavior specs (specs/browser/find-replace.md
F3-F9, specs/browser/bulk-actions.md B7.2, the step ids of undo-ops.md V4),
never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok

START = "2030-01-15T10:00:00"
VOCAB = {"name": "Vocab", "fields": ["Word", "Meaning"], "templates": [{"name": "Recall", "front": "{{Word}}", "back": "{{Meaning}}"}]}
PLAIN = {"name": "Plain", "fields": ["Text", "Extra"], "templates": [{"name": "Card", "front": "{{Text}}", "back": "{{Extra}}"}]}
GAPS = {"name": "Gaps", "fields": ["Text", "Back Extra"], "templates": [{"name": "Card", "front": "{{Text}}", "back": "{{Back Extra}}"}]}
FLDS = {"db_exec": {"sql": "SELECT flds FROM notes ORDER BY id"}}
TAGS = {"db_exec": {"sql": "SELECT tags FROM notes ORDER BY id"}}
DECKS = {"db_exec": {"sql": "SELECT d.name FROM cards c JOIN decks d ON d.id = c.did ORDER BY c.id"}}
COL_MOD = {"db_exec": {"sql": "SELECT mod FROM col"}}
UNDO = {"observe": ["undo_status"]}
# deck-ops.md K13: a filtered deck row as a build leaves it.
CRAM = {"db_exec": {"sql": "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES "
                           "(100, 'Cram', 0, 0, X'08011001', X'12130801120a0a066465636b3a78106428d804383c')"}}


def scenario(steps, notes, notetypes=(VOCAB, PLAIN), decks=None):
    setup = {"notetypes": list(notetypes), "notes": notes}
    if decks:
        setup["decks"] = list(decks)
    return {"id": "t/bulk", "clock": {"tz": "UTC", "start": START}, "setup": setup, "steps": steps}


def note(notetype, *fields, tags=None, deck=None):
    out = {"notetype": notetype, "fields": list(fields)}
    if tags:
        out["tags"] = tags
    if deck:
        out["deck"] = deck
    return out


def rows(step):
    return [row[0] for row in step["db_exec"]["rows"]]


def undo_status(undo, redo=None):
    return {"undo_status": {"undo": undo, "redo": redo}}


def run_error(test, worker, sc):
    resp = worker.request({"cmd": "run", "scenario": sc})
    test.assertFalse(resp["ok"], resp)
    return resp["error"]["kind"], resp["error"].get("step")


class FindReplaceFields(unittest.TestCase):
    def test_a_named_field_is_rewritten_literally_and_undone_in_one_step(self):
        notes = [note("Vocab", "price", "the cost of it"), note("Vocab", "charge", "Cost cost"),
                 note("Vocab", "cost", "worth"), note("Plain", "cost here", "x")]
        steps = [
            {"db_exec": {"sql": "UPDATE notes SET usn = 0"}},
            {"advance": "1m"},
            {"find_replace": {"notes": ["#n1", "#n2", "#n3", "#n4"], "find": "cost", "replace": "$5",
                              "in": {"field": "meaning"}}},
            FLDS,
            UNDO,
            {"undo": {}},
            FLDS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes))
        # F5 literal $; F6 step 2: field matched under unicase, Plain skipped.
        self.assertEqual(out[2], {"find_replace": {"count": 2}})
        self.assertEqual(rows(out[3]), ["price\x1fthe $5 of it", "charge\x1f$5 $5", "cost\x1fworth", "cost here\x1fx"])
        self.assertEqual(out[4], undo_status("find_and_replace"))
        self.assertEqual(rows(out[6]), ["price\x1fthe cost of it", "charge\x1fCost cost", "cost\x1fworth", "cost here\x1fx"])

    def test_all_notes_and_all_fields_with_a_regex_and_match_case(self):
        notes = [note("Plain", "Foo@ example", "bar@"), note("Vocab", "a@", "x")]
        steps = [
            {"delete_notes": {"notes": ["#n2"]}},
            {"add_note": note("Plain", "BAZ@", "y")},
            {"find_replace": {"find": "([a-z]+)@", "replace": "${1} at", "regex": True, "ignore_case": False}},
            FLDS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes))
        # F8: every note there is; F4 step 3 off: "BAZ@" has no match.
        self.assertEqual(out[2], {"find_replace": {"count": 1}})
        self.assertEqual(rows(out[3]), ["Foo at example\x1fbar at", "BAZ@\x1fy"])

    def test_refusals_change_nothing(self):
        notes = [note("Plain", "f(x)", "y", tags=["keep"]), note("Plain", "damaged", "y")]
        steps = [
            {"db_exec": {"sql": "UPDATE notes SET flds = 'damaged' || char(31) || 'y' || char(31) || 'z' "
                                "WHERE sfld = 'damaged'"}},
            COL_MOD,
            {"advance": "1m"},
            {"find_replace": {"find": "(", "replace": "z", "regex": True}},
            {"find_replace": {"find": "[", "replace": "z", "regex": True, "in": "tags"}},
            {"find_replace": {"find": "keep", "replace": "two words", "in": "tags"}},
            {"find_replace": {"find": "keep", "replace": "two　words", "in": "tags"}},
            {"find_replace": {"find": "(", "replace": "a b", "regex": True, "in": "tags"}},
            {"find_replace": {"find": "damaged", "replace": "fixed"}},
            {"find_replace": {"find": "(", "replace": "z", "regex": True, "in": {"field": "Text"}}},
            COL_MOD,
            FLDS,
            TAGS,
            {"find_replace": {"notes": ["#n1", "#n2"], "find": "f(", "replace": "g("}},
            FLDS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes))
        self.assertEqual(out[3:10], [
            {"find_replace": {"error": "invalid_regex"}},
            {"find_replace": {"error": "invalid_regex"}},
            {"find_replace": {"error": "space_in_replacement"}},
            {"find_replace": {"error": "space_in_replacement"}},
            {"find_replace": {"error": "space_in_replacement"}},
            {"find_replace": {"error": "field_count"}},
            {"find_replace": {"error": "invalid_regex"}},
        ])
        self.assertEqual(out[10], out[1])
        self.assertEqual(rows(out[11]), ["f(x)\x1fy", "damaged\x1fy\x1fz"])
        self.assertEqual(rows(out[12]), [" keep ", ""])
        # F6 step 7: a damaged note the pattern does not match refuses nothing.
        self.assertEqual(out[13], {"find_replace": {"count": 1}})
        self.assertEqual(rows(out[14]), ["g(x)\x1fy", "damaged\x1fy\x1fz"])


    def test_a_refusal_inside_anki_s_transaction_empties_the_undo_history(self):
        notes = [note("Plain", "a", "x", tags=["t"])]
        steps = [
            {"find_replace": {"find": "a", "replace": "b"}},
            {"find_replace": {"find": "t", "replace": "u v", "in": "tags"}},
            UNDO,
            {"find_replace": {"find": "(", "replace": "z", "regex": True}},
            UNDO,
            {"find_replace": {"find": "b", "replace": "c"}},
            {"find_replace": {"find": "[", "replace": "z", "regex": True, "in": "tags"}},
            UNDO,
            {"find_replace": {"find": "c", "replace": "d"}},
            {"db_exec": {"sql": "UPDATE notes SET flds = flds || char(31) || 'extra'"}},
            {"find_replace": {"find": "t", "replace": "u", "in": "tags"}},
            {"find_replace": {"find": "d", "replace": "e"}},
            UNDO,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes))
        # The separator check comes before Anki's transaction and keeps the
        # history; a pattern that does not compile fails inside it.
        self.assertEqual(out[2], undo_status("find_and_replace"))
        self.assertEqual(out[3], {"find_replace": {"error": "invalid_regex"}})
        self.assertEqual(out[4], undo_status(None))
        self.assertEqual(out[6], {"find_replace": {"error": "invalid_regex"}})
        self.assertEqual(out[7], undo_status(None))
        # The db_exec empties the history; the tag step after it is the one
        # a field-count refusal drops.
        self.assertEqual(out[10], {"find_replace": {"count": 1}})
        self.assertEqual(out[11], {"find_replace": {"error": "field_count"}})
        self.assertEqual(out[12], undo_status(None))

class FindReplaceTags(unittest.TestCase):
    def test_tags_are_matched_one_by_one_in_an_update_tag_step(self):
        notes = [note("Plain", "a", "x", tags=["keep", "old::x"]), note("Plain", "b", "x", tags=["also", "solo"])]
        steps = [
            {"find_replace": {"find": "old", "replace": "new", "in": "tags"}},
            {"find_replace": {"notes": ["#n2"], "find": "^so", "replace": "x", "regex": True, "in": "tags"}},
            TAGS,
            UNDO,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes))
        self.assertEqual(out[0:2], [{"find_replace": {"count": 1}}, {"find_replace": {"count": 1}}])
        self.assertEqual(rows(out[2]), [" keep new::x ", " also xlo "])
        self.assertEqual(out[3], undo_status("update_tag"))


class FindReplaceShapes(unittest.TestCase):
    def test_malformed_arguments_are_invalid_scenarios(self):
        notes = [note("Plain", "a", "x")]
        cases = [
            {"notes": ["#n1", "#n1"], "find": "a", "replace": "b"},
            {"notes": [], "find": "a", "replace": "b"},
            {"find": "a"},
            {"find": "a", "replace": "b", "extra": 1},
            {"find": "a", "replace": 5},
            {"find": "a", "replace": "b", "regex": "yes"},
            {"find": "a", "replace": "b", "in": {"field": ""}},
            {"find": "a", "replace": "b", "in": "fields"},
        ]
        with WorkerProcess("UTC") as w:
            for args in cases:
                with self.subTest(args=args):
                    self.assertEqual(run_error(self, w, scenario([{"find_replace": args}], notes)),
                                     ("invalid_scenario", 0))


class ChangeDeck(unittest.TestCase):
    def test_cards_move_once_and_a_filtered_target_is_refused(self):
        notes = [note("Plain", "a", "x", deck="Home"), note("Plain", "b", "x", deck="Target")]
        steps = [
            CRAM,
            {"change_deck": {"cards": ["#c1", "#c2"], "deck": "Target"}},
            DECKS,
            UNDO,
            {"change_deck": {"cards": ["#c1"], "deck": "Cram"}},
            DECKS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes, decks=["Home", "Target"]))
            # B7.2 step 2: c2 already in Target is not written or counted.
            self.assertEqual(out[1], {"change_deck": {"count": 1}})
            self.assertEqual(rows(out[2]), ["Target", "Target"])
            self.assertEqual(out[3], undo_status("change_deck"))
            self.assertEqual(out[4], {"change_deck": {"error": "filtered_deck"}})
            self.assertEqual(rows(out[5]), ["Target", "Target"])
            unknown = scenario([{"change_deck": {"cards": ["#c1"], "deck": "Nowhere"}}], notes, decks=["Home", "Target"])
            self.assertEqual(run_error(self, w, unknown), ("unknown_ref", 0))
            repeated = scenario([{"change_deck": {"cards": ["#c1", "#c1"], "deck": "Target"}}], notes,
                                decks=["Home", "Target"])
            self.assertEqual(run_error(self, w, repeated), ("invalid_scenario", 0))


class FieldNames(unittest.TestCase):
    def test_distinct_names_of_the_notes_note_types_in_unicase_order(self):
        notes = [note("Vocab", "a", "b"), note("Gaps", "c", "d"), note("Plain", "e", "f")]
        steps = [
            {"delete_notes": {"notes": ["#n3"]}},
            {"observe": [{"find_replace_fields": {"notes": ["#n1", "#n2"]}}]},
            {"observe": [{"find_replace_fields": {"notes": ["#n3"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, notes, notetypes=(VOCAB, PLAIN, GAPS)))
            # F3's first row; a deleted note adds nothing.
            self.assertEqual(out[1], {"find_replace_fields": ["Back Extra", "Meaning", "Text", "Word"]})
            self.assertEqual(out[2], {"find_replace_fields": []})
            twice = scenario([{"observe": [{"find_replace_fields": {"notes": ["#n1"]}},
                                           {"find_replace_fields": {"notes": ["#n2"]}}]}], notes,
                             notetypes=(VOCAB, PLAIN, GAPS))
            self.assertEqual(run_error(self, w, twice), ("invalid_scenario", 0))


if __name__ == "__main__":
    unittest.main()
