# SPDX-License-Identifier: AGPL-3.0-or-later
"""search_build, search_join and search_replace (specs/conformance/search-ops.md
S5-S6).

Expected values are the examples of specs/search/join-and-replace.md J9 that
J10 does not set apart, and the error outputs of specs/search/errors.md E1/E2;
never the driver's own output."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.session import Session
from driver.tests.test_worker import scenario

START = "2030-01-15T10:00:00"

# join-and-replace.md J9: built item -> text.
BUILD = [
    ({"deck": "Lang::Verbs"}, "deck:Lang::Verbs"),
    ({"deck": "My Deck"}, '"deck:My Deck"'),
    ({"deck": "a_b*"}, "deck:a\\_b\\*"),
    ({"deck": "C++ (old)"}, '"deck:C++ (old)"'),
    ({"deck": 'say "hi"'}, '"deck:say \\"hi\\""'),
    ({"deck": "x\\y"}, "deck:x\\\\y"),
    ({"tag": "x y"}, '"tag:x y"'),
    ({"notetype": "Basic (and reversed card)"}, '"note:Basic (and reversed card)"'),
    ({"field_present": "Extra Info"}, '"Extra Info:_*"'),
    ({"field_present": "my:field"}, "my\\:field:_*"),
    ({"literal": "and"}, '"and"'),
    ({"literal": "-x"}, '"-x"'),
    ({"literal": "3:30"}, "3\\:30"),
    ({"literal": "50%_off"}, "50%\\_off"),
    ({"group": {"joiner": "and", "items": [{"notetype": "Basic"}, {"template": 0}]}}, "note:Basic card:1"),
    ({"group": {"joiner": "or", "items": [{"state": "new"}, {"state": "learn"}]}}, "is:new OR is:learn"),
    ({"group": {"joiner": "and", "items": [{"state": "due"}, {"negated": {"due_on": 0}}]}}, "is:due -prop:due=0"),
    ({"flag": "any"}, "-flag:0"),
    ({"flag": "none"}, "flag:0"),
    ({"flag": 1}, "flag:1"),
    ({"answered": {"days": 1, "button": 1}}, "rated:1:1"),
    ({"rescheduled": 0}, "resched:0"),
    ({"note_ids": [5, 3]}, "nid:5,3"),
    ({"dupe": {"notetype_id": 1600000000000, "text": 'a\\b "c"'}}, '"dupe:1600000000000,a\\\\b \\"c\\""'),
    ("any_deck", "deck:_*"),
    ("untagged", "-tag:_*"),
    ("whole_collection", "deck:*"),
    ({"negated": {"negated": {"flag": 2}}}, "flag:2"),
    ({"text": "((a b))"}, "(a b)"),
]
# J9: (existing, item, joiner, negate) -> text.
JOIN = [
    ("deck:Lang is:due", {"tag": "verbs"}, "and", True, "(deck:Lang is:due) -tag:verbs"),
    ("deck:Lang", {"tag": "verbs"}, "or", False, "deck:Lang OR tag:verbs"),
    ("a or b", {"literal": "c"}, "and", False, "(a OR b) c"),
    ("", {"deck": "Other"}, "and", False, "deck:* deck:Other"),
    ("-is:new", {"flag": "any"}, "and", False, "-is:new -flag:0"),
    ("-is:new", {"flag": "any"}, "and", True, "-is:new flag:0"),
    ("a", {"group": {"joiner": "and", "items": [{"notetype": "Basic"}, {"template": 0}]}}, "and", False,
     "a (note:Basic card:1)"),
    ("(x y)", {"state": "due"}, "or", False, "(x y) OR is:due"),
    ("a -and b", {"literal": "c"}, "and", False, "(a -  b) c"),
]
# J9: (existing, item) -> text.
REPLACE = [
    ("deck:Lang is:due", {"deck": "Other"}, "deck:Other is:due"),
    ("deck:A or (deck:B -deck:C)", {"deck": "D"}, "deck:D OR (deck:D -deck:D)"),
    ("is:new added:3", {"state": "review"}, "is:review added:3"),
    ("resched:1 is:due", {"answered": {"days": 1, "button": 1}}, "rated:1:1 is:due"),
    ("front:x back:y", {"field_present": "Notes"}, "Notes:_* Notes:_*"),
    ("deck:Lang", {"flag": "any"}, "deck:Lang"),
    ("", {"deck": "X"}, "deck:*"),
    ("(a  b)", {"deck": "X"}, "a b"),
    ("rated:2 prop:ivl>10", {"due_on": 0}, "rated:2 prop:due=0"),
    ("((a b))", {"deck": "X"}, "(a b)"),
]


class BuildCase(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def run_steps(self, steps, notes=0):
        s = Session(scenario("t/build", "UTC", START, steps, notes=notes), "UTC", self.tmp.name, self.clock)
        self.addCleanup(s.close)
        return s.run_all()["steps"]

    def observe(self, **kinds):
        return self.run_steps([{"observe": [{k: v} for k, v in kinds.items()]}])[0]


class Outputs(BuildCase):
    def test_spec_examples(self):
        out = self.observe(
            search_build=[item for item, _ in BUILD],
            search_join=[{"existing": e, "item": i, "joiner": j, "negate": n} for e, i, j, n, _ in JOIN],
            search_replace=[{"existing": e, "item": i} for e, i, _ in REPLACE],
        )
        self.assertEqual(out["search_build"], [{"text": t} for _, t in BUILD])
        self.assertEqual(out["search_join"], [{"text": t} for *_, t in JOIN])
        self.assertEqual(out["search_replace"], [{"text": t} for *_, t in REPLACE])

    def test_parse_errors_carry_no_message(self):
        out = self.observe(
            search_build=[{"text": "(a"}, {"group": {"joiner": "and", "items": [{"deck": "x"}, {"text": "is:foo"}]}}],
            search_join=[{"existing": "a)", "item": {"deck": "x"}, "joiner": "and", "negate": False},
                         {"existing": "a", "item": {"text": "prop:ivl~1"}, "joiner": "or", "negate": True}],
            search_replace=[{"existing": '"a', "item": {"deck": "x"}},
                            {"existing": "a", "item": {"text": "rated:1:5"}}],
        )
        self.assertEqual(out["search_build"], [{"error": "unclosed_group"}, {"error": "invalid_state", "provided": "foo"}])
        self.assertEqual(out["search_join"], [{"error": "unopened_group"},
                                              {"error": "invalid_operator", "provided": "ivl"}])
        self.assertEqual(out["search_replace"], [{"error": "unclosed_quote"},
                                                 {"error": "invalid_answer_button", "provided": "5", "context": "1:5"}])
        text = json.dumps(out, ensure_ascii=False)
        self.assertNotIn("⁨", text)
        self.assertNotIn("Invalid", text)

    def test_field_text_gets_the_search_field_rule(self):
        # join-and-replace.md J2: U+001F in field text is a space, so two terms.
        out = self.observe(search_build=[{"text": "a\u001fb", "field_text": True}],
                           search_join=[{"existing": "a\u001cb", "item": {"literal": "c"}, "joiner": "and",
                                         "negate": False}])
        self.assertEqual(out["search_build"], [{"text": "a b"}])
        self.assertEqual(out["search_join"], [{"text": "(a b) c"}])

    def test_nothing_in_the_collection_changes(self):
        dump = {"db_dump": None}
        out = self.run_steps([{"observe": [dump]},
                              {"observe": [{"search_build": ["any_tag"]},
                                           {"search_join": [{"existing": "", "item": "current_deck", "joiner": "or",
                                                             "negate": False}]},
                                           {"search_replace": [{"existing": "a", "item": {"literal": "b"}}]}]},
                              {"observe": [dump]}], notes=1)
        self.assertEqual(out[0], out[2])
        self.assertEqual(out[1]["search_join"], [{"text": "deck:* OR deck:current"}])


class Requests(BuildCase):
    def test_bad_requests_and_j10_inputs_are_invalid(self):
        items = [
            None, 1, [], {}, "nothing", {"deck": ""}, {"deck": 1}, {"deck": "_*"}, {"deck": "current"},
            {"deck": "a b"}, {"deck": "a::"}, {"deck": "a:: b"}, {"deck": "é"}, {"tag": "none"},
            {"tag": "a\tb"}, {"notetype": "a　b"}, {"field_present": "Deck"}, {"field_present": "has-cd"},
            {"field_present": "_*"}, {"literal": ""}, {"flag": 0}, {"flag": 8}, {"flag": True}, {"flag": []},
            {"state": "buried-manually"}, {"added": 0}, {"edited": -1}, {"answered": {"days": 0}},
            {"answered": {"days": 1, "button": 5}}, {"rescheduled": 4294967296}, {"due_on": 2147483648},
            {"note_ids": []}, {"note_ids": [-1]}, {"dupe": {"notetype_id": 1, "text": "a b"}},
            {"group": {"joiner": "and", "items": []}}, {"group": {"joiner": "xor", "items": ["any_tag"]}},
            {"group": {"joiner": [], "items": ["any_tag"]}},
            {"text": 1}, {"text": "a", "field_text": 1}, {"deck": "a", "tag": "b"}, {"template": 65535},
            {"negated": None},
        ]
        for item in items:
            with self.subTest(item=item), self.assertRaises(DriverError) as ctx:
                self.observe(search_build=[item])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")
        cases = {
            "search_build": [None, [], {}],
            "search_join": [[], [{"existing": "a", "item": "any_tag", "joiner": "and"}],
                            [{"existing": 1, "item": "any_tag", "joiner": "and", "negate": False}],
                            [{"existing": "a", "item": "any_tag", "joiner": "AND", "negate": False}],
                            [{"existing": "a", "item": "any_tag", "joiner": {}, "negate": False}],
                            [{"existing": "a", "item": "any_tag", "joiner": "and", "negate": 0}]],
            "search_replace": [[], [{"existing": "a"}], [{"existing": "a", "item": "any_tag", "x": 1}]],
        }
        for kind, values in cases.items():
            for value in values:
                with self.subTest(kind=kind, value=value), self.assertRaises(DriverError) as ctx:
                    self.observe(**{kind: value})
                self.assertEqual(ctx.exception.kind, "invalid_scenario")


if __name__ == "__main__":
    unittest.main()
