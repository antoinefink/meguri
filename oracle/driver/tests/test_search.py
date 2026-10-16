# SPDX-License-Identifier: AGPL-3.0-or-later
"""search and canonical_search (specs/conformance/search-ops.md S1-S3).

Expected values come from the spec (search-ops.md, specs/search/errors.md E1-E3,
canonical-form.md F2-F5) and from how each scenario builds its collection,
never from the driver."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import anki.errors

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.session import Session
from driver.tests.test_worker import scenario

START = "2030-01-15T10:00:00"
CARD_COLUMNS = "nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data"
NOTE_COLUMNS = "mid, mod, usn, tags, flds, sfld, csum, flags, data"


def extra_card(card_id: int, ord_: int) -> dict:
    """A second card of the first note, stored under card_id (no logical id names it)."""
    return {"db_exec": {"sql": f"INSERT INTO cards (id, {CARD_COLUMNS}) "
                               f"SELECT {card_id}, {CARD_COLUMNS.replace('ord,', f'{ord_},')} "
                               "FROM cards ORDER BY id LIMIT 1"}}


def extra_note(note_id: int) -> dict:
    return {"db_exec": {"sql": f"INSERT INTO notes (id, guid, {NOTE_COLUMNS}) "
                               f"SELECT {note_id}, guid || 'x', {NOTE_COLUMNS} FROM notes ORDER BY id LIMIT 1"}}


# errors.md E1/E2: input -> the error output (provided and context as Anki's
# message shows them, a backtick already an apostrophe).
PARSE_ERRORS = {
    "and": {"error": "misplaced_and"},
    "a or": {"error": "misplaced_or"},
    "()": {"error": "empty_group"},
    "a)": {"error": "unopened_group"},
    "(a": {"error": "unclosed_group"},
    '""': {"error": "empty_quote"},
    '"a': {"error": "unclosed_quote"},
    ":a": {"error": "missing_key"},
    "\\x": {"error": "unknown_escape", "provided": "\\x"},
    "is:foo": {"error": "invalid_state", "provided": "foo"},
    "is:a`b": {"error": "invalid_state", "provided": "a'b"},
    "flag:8": {"error": "invalid_flag"},
    "prop:x": {"error": "invalid_property", "provided": "x"},
    "prop:ivl~1": {"error": "invalid_operator", "provided": "ivl"},
    "prop:ease>x": {"error": "invalid_number", "provided": "x", "context": "ease>x"},
    "prop:due=0.5": {"error": "invalid_whole_number", "provided": "0.5", "context": "due=0.5"},
    "mid:x": {"error": "invalid_whole_number", "provided": "x", "context": "mid:"},
    "prop:ivl>x": {"error": "invalid_positive_whole_number", "provided": "x", "context": "ivl>x"},
    "added:x": {"error": "invalid_positive_whole_number", "provided": "x", "context": "added:"},
    "prop:rated=1": {"error": "invalid_negative_whole_number", "provided": "1", "context": "rated=1"},
    "rated:1:5": {"error": "invalid_answer_button", "provided": "5", "context": "1:5"},
    "nid:1,a": {"error": "invalid_id_list", "context": "nid:"},
    "NID:1,a": {"error": "invalid_id_list", "context": "NID:"},
    "dupe:1": {"error": "invalid_dupe"},
}


class SearchCase(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, steps, notes=0):
        s = Session(scenario("t/search", "UTC", START, steps, notes=notes), "UTC", self.tmp.name, self.clock)
        self.addCleanup(s.close)
        return s

    def run_steps(self, steps, notes=0):
        return self.session(steps, notes).run_all()["steps"]

    def observe(self, notes=0, **kinds):
        return self.run_steps([{"observe": [{k: v} for k, v in kinds.items()]}], notes)[0]


class Results(SearchCase):
    def test_ids_are_logical_refs_by_number_then_unnamed_real_ids_ascending(self):
        steps = [extra_card(5, 1), extra_card(3, 2), extra_note(4), {"observe": [{"search": [
            {"query": ""}, {"query": "", "kind": "cards"}, {"query": "", "kind": "notes"},
            {"query": "q2"}, {"query": "q2", "kind": "notes"}, {"query": "nothing-matches"},
        ]}]}]
        out = self.run_steps(steps, notes=11)[-1]["search"]
        cards = [f"#c{i}" for i in range(1, 12)] + [3, 5]  # #c10 after #c9; 3 and 5 sort before every real card id
        notes = [f"#n{i}" for i in range(1, 12)] + [4]
        self.assertEqual(out, [{"ids": cards}, {"ids": cards}, {"ids": notes},
                               {"ids": ["#c2"]}, {"ids": ["#n2"]}, {"ids": []}])

    def test_searching_changes_nothing(self):
        dump = {"db_dump": None}
        steps = [{"observe": [dump]},
                 {"observe": [{"search": [{"query": "q1"}, {"query": "", "kind": "notes"}, {"query": "is:foo"}]},
                              {"canonical_search": ["a  b", "(a"]}]},
                 {"observe": [dump]}]
        out = self.run_steps(steps, notes=2)
        self.assertEqual(out[0], out[2])


class Errors(SearchCase):
    def test_parse_errors_are_the_same_in_both_modes_and_in_canonical_search(self):
        texts = list(PARSE_ERRORS)
        requests = [{"query": t, "kind": k} for t in texts for k in ("cards", "notes")]
        out = self.observe(notes=1, search=requests, canonical_search=texts)
        expected = list(PARSE_ERRORS.values())
        self.assertEqual(out["search"], [e for e in expected for _ in range(2)])
        self.assertEqual(out["canonical_search"], expected)
        text = json.dumps(out, ensure_ascii=False)
        self.assertNotIn("\u2068", text)
        self.assertNotIn("\u2069", text)
        self.assertNotIn("Invalid", text)

    def test_run_errors(self):
        queries = ["-and", "-OR", "a -and b", "(-or)", "nid:\u0661", "cid:\u0661,2", "did:\u0663", "re:[", '"re:("']
        requests = [{"query": q, "kind": k} for q in queries for k in ("cards", "notes")]
        with_notes = self.observe(notes=1, search=requests)["search"]
        failed, regex = {"error": "search_failed"}, {"error": "invalid_regex"}
        self.assertEqual(with_notes, [failed] * 14 + [regex] * 4)
        # errors.md E3: Anki compiles a pattern only when a row reaches it.
        empty = self.observe(notes=0, search=requests)["search"]
        self.assertEqual(empty, [failed] * 14 + [{"ids": []}] * 4)

    def test_canonical_search_has_no_run_errors(self):
        out = self.observe(canonical_search=["-and", "-OR", "a -and b", "nid:\u0661", "re:["])["canonical_search"]
        # canonical-form.md F3: a negated and is "- ", a negated or "- OR ".
        self.assertEqual(out, [{"text": "- "}, {"text": "- OR "}, {"text": "a -  b"}, {"text": "nid:\u0661"},
                               {"text": "re:["}])

    def failure(self, patch, **kinds):
        s = self.session([{"observe": [{k: v} for k, v in kinds.items()]}], notes=1)
        patch(s.col)
        with self.assertRaises(DriverError) as ctx:
            s.run_all()
        return ctx.exception.to_json()

    def test_anything_else_is_anki_error_without_its_message(self):
        def raising(exc):
            def fn(*_args, **_kwargs):
                raise exc
            return fn

        def search_error(text_fn):
            def patch(col):
                col.find_cards = raising(anki.errors.SearchError(text_fn(col.tr), None, None, None))
            return patch

        cases = {
            "unknown text": search_error(lambda tr: "Invalid search: \u2068Something else\u2069"),
            "unknown argument term": search_error(
                lambda tr: tr.search_invalid_search(reason=tr.search_invalid_argument(term="deck:", argument="x"))),
            "no outer wrapper": search_error(lambda tr: tr.search_misplaced_and()),
        }
        for name, patch in cases.items():
            with self.subTest(name):
                err = self.failure(patch, search=[{"query": "a"}])
                self.assertEqual((err["kind"], err["exception"]), ("anki_error", "SearchError"))
                self.assertNotIn("Something", json.dumps(err))

        def other_input(col):
            col.find_notes = raising(anki.errors.InvalidInput("<pre>other</pre>", None, None, None))

        err = self.failure(other_input, search=[{"query": "a", "kind": "notes"}])
        self.assertEqual((err["kind"], err["exception"]), ("anki_error", "InvalidInput"))

        def not_found(col):
            col.find_cards = raising(anki.errors.NotFoundError("gone", None, None, None))

        err = self.failure(not_found, search=[{"query": "a"}])
        self.assertEqual((err["kind"], err["exception"]), ("anki_error", "NotFoundError"))

        def db_in_canonical(col):
            col.build_search_string = raising(anki.errors.DBError("db", None, None, None))

        err = self.failure(db_in_canonical, canonical_search=["a"])
        self.assertEqual((err["kind"], err["exception"]), ("anki_error", "DBError"))

    def test_the_generic_message_is_malformed(self):
        # errors.md E1: no input reaches it at 26.09.3, so its message is planted.
        def patch(col):
            message = col.tr.search_invalid_search(reason=col.tr.search_invalid_other())
            exc = anki.errors.SearchError(message, None, None, None)

            def fn(*_args, **_kwargs):
                raise exc

            col.build_search_string = fn

        s = self.session([{"observe": [{"canonical_search": ["a"]}]}])
        patch(s.col)
        self.assertEqual(s.run_all()["steps"][0], {"canonical_search": [{"error": "malformed"}]})


class Requests(SearchCase):
    def test_bad_requests_are_invalid(self):
        bad = {
            "search": [None, {}, [], "a", ["a"], [{}], [{"query": 1}], [{"query": None}],
                       [{"query": "a", "kind": "card"}], [{"query": "a", "kind": None}],
                       [{"query": "a", "order": True}], [{"query": "a"}, 1]],
            "canonical_search": [None, {}, [], "a", [1], [None], [["a"]], ["a", {"query": "a"}]],
        }
        for kind, values in bad.items():
            for value in values:
                with self.subTest(kind=kind, value=value), self.assertRaises(DriverError) as ctx:
                    self.run_steps([{"observe": [{kind: value}]}])
                self.assertEqual(ctx.exception.kind, "invalid_scenario")

    def test_a_bare_name_has_no_arguments(self):
        for kind in ("search", "canonical_search"):
            with self.subTest(kind=kind), self.assertRaises(DriverError) as ctx:
                self.run_steps([{"observe": [kind]}])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")


if __name__ == "__main__":
    unittest.main()
