# SPDX-License-Identifier: AGPL-3.0-or-later
"""Sorted searches and browser_rows (specs/conformance/browser-ops.md B1-B4).

Expected values come from the specs (browser-ops.md, specs/browser/columns.md,
specs/browser/sorting.md, browser/config-keys K2.1, K3) and from how each test
builds its collection, never from the driver. Rows that show dates run in a
worker, whose clock is the scenario's; in-process sessions run on real time.

Anki's first day computation in a collection without the config key rollover
writes it (4), so the tests that check that nothing changes set it first."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import anki.errors
from anki import backend_pb2, generic_pb2

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.harness import WorkerProcess
from driver.observe.browser_rows import parse_cell
from driver.session import Session
from driver.tests.test_search import extra_card
from driver.tests.test_worker import NOTETYPE, run_ok, scenario

START = "2030-01-15T10:00:00"  # UTC: 1894701600, today 0
PAIR = {
    "name": "Pair",
    "fields": ["Front", "Back"],
    "templates": [{"name": "Forward", "front": "{{Front}}", "back": "{{Back}}"},
                  {"name": "Backward", "front": "{{Back}}", "back": "{{Front}}"}],
    "css": ".card {}",
}
ALL = ["noteFld", "question", "answer", "template", "deck", "cardDue", "cardEase", "cardIvl", "cardReps", "cardLapses",
       "noteCrt", "cardMod", "noteMod", "note", "noteTags", "originalPosition", "stability", "difficulty",
       "retrievability"]


def card(k: int, sets: str, params=None) -> dict:
    """db_exec on the k-th card by id (each test note has one card here)."""
    step = {"sql": f"UPDATE cards SET {sets} WHERE id = (SELECT id FROM cards ORDER BY id LIMIT 1 OFFSET {k})"}
    if params is not None:
        step["params"] = params
    return {"db_exec": step}


def qa(i: int, **extra) -> dict:
    return {"notetype": "QA", "deck": "A", "fields": [f"q{i}", f"a{i}"], **extra}


ROLLOVER = {"set_config": {"key": "rollover", "value": 4}}


class BrowserCase(unittest.TestCase):
    maxDiff = None

    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, steps, notes=0, setup=None):
        s = Session(scenario("t/browser", "UTC", START, steps, notes=notes, setup=setup), "UTC", self.tmp.name,
                    self.clock)
        self.addCleanup(s.close)
        return s

    def run_steps(self, steps, notes=0, setup=None):
        return self.session(steps, notes, setup).run_all()["steps"]

    def invalid(self, steps, notes=0, setup=None):
        with self.assertRaises(DriverError) as ctx:
            self.run_steps(steps, notes, setup)
        return ctx.exception

    def run_worker(self, steps, notes=0, setup=None):
        """The steps' outputs from a worker, on the scenario clock."""
        with WorkerProcess("UTC") as w:
            return run_ok(self, w, scenario("t/browser", "UTC", START, steps, notes=notes, setup=setup))


class SortedSearch(BrowserCase):
    def test_equal_key_runs_are_listed_in_logical_order_and_unnamed_rows_where_they_fall(self):
        # B2: cardMod keys c2 100, c3 200, then c1 and two unnamed copies of
        # it (real ids 5 and 3, mod 300): one run, listed #c1, 3, 5 both ways.
        steps = [card(0, "mod = 300"), card(1, "mod = 100"), card(2, "mod = 200"), extra_card(5, 1), extra_card(3, 2),
                 {"observe": [{"search": [
                     {"query": "", "order": {"column": "cardMod"}},
                     {"query": "", "order": {"column": "cardMod", "reverse": False}},
                     {"query": "", "order": {"column": "cardMod", "reverse": True}},
                     {"query": "", "kind": "notes", "order": {"column": "noteFld"}},
                     {"query": "", "kind": "notes", "order": {"column": "noteFld", "reverse": True}},
                     {"query": "q2 or q3", "order": {"column": "cardMod", "reverse": True}},
                     {"query": "nothing-matches", "order": {"column": "cardMod"}},
                 ]}]}]
        out = self.run_steps(steps, notes=3)[-1]["search"]
        self.assertEqual(out, [
            {"ids": ["#c2", "#c3", "#c1", 3, 5]},
            {"ids": ["#c2", "#c3", "#c1", 3, 5]},
            {"ids": ["#c1", 3, 5, "#c3", "#c2"]},
            {"ids": ["#n1", "#n2", "#n3"]},
            {"ids": ["#n3", "#n2", "#n1"]},
            {"ids": ["#c3", "#c2"]},
            {"ids": []},
        ])

    def test_unsortable_columns_and_the_error_order(self):
        # B1 and sorting.md S1: Question and Answer sort in neither mode, the
        # FSRS columns only in cards mode, keys outside the 19 never (case
        # counts). A parse error comes first, then unsortable, then
        # invalid_regex and search_failed (search/errors E3).
        u = {"error": "unsortable"}
        requests_and_outputs = [
            ({"query": "", "order": {"column": "question"}}, u),
            ({"query": "", "kind": "notes", "order": {"column": "question"}}, u),
            ({"query": "", "order": {"column": "answer", "reverse": True}}, u),
            ({"query": "", "kind": "notes", "order": {"column": "answer"}}, u),
            ({"query": "", "order": {"column": "bogus"}}, u),
            ({"query": "", "order": {"column": ""}}, u),
            ({"query": "", "order": {"column": "NoteFld"}}, u),
            ({"query": "", "kind": "notes", "order": {"column": "stability"}}, u),
            ({"query": "", "kind": "notes", "order": {"column": "retrievability", "reverse": True}}, u),
            ({"query": "(", "order": {"column": "question"}}, {"error": "unclosed_group"}),
            ({"query": "(", "kind": "notes", "order": {"column": "bogus"}}, {"error": "unclosed_group"}),
            ({"query": '"re:("', "order": {"column": "question"}}, u),
            ({"query": '"re:("', "order": {"column": "noteFld"}}, {"error": "invalid_regex"}),
            ({"query": "-and", "order": {"column": "question"}}, u),
            ({"query": "-and", "kind": "notes", "order": {"column": "noteFld"}}, {"error": "search_failed"}),
            # Two cards without memory states: equal (NULL) keys, one run.
            ({"query": "", "order": {"column": "stability", "reverse": True}}, {"ids": ["#c1", "#c2"]}),
        ]
        steps = [{"observe": [{"search": [r for r, _ in requests_and_outputs]}]}]
        out = self.run_steps(steps, notes=2)[0]["search"]
        self.assertEqual(out, [o for _, o in requests_and_outputs])

    def test_bad_orders_are_invalid(self):
        for order in [True, None, [], {}, {"column": 1}, {"column": None}, {"reverse": True},
                      {"column": "noteFld", "reverse": "yes"}, {"column": "noteFld", "reverse": None},
                      {"column": "noteFld", "reverse": 1}, {"column": "noteFld", "direction": "asc"}]:
            with self.subTest(order=order):
                err = self.invalid([{"observe": [{"search": [{"query": "", "order": order}]}]}], notes=1)
                self.assertEqual(err.kind, "invalid_scenario")

    def test_another_invalid_input_is_anki_error_without_its_message(self):
        s = self.session([{"observe": [{"search": [{"query": "a", "order": {"column": "noteFld"}}]}]}], notes=1)
        real = s.col._backend.search_cards

        def search_cards(*, search, order):
            if search:
                raise anki.errors.InvalidInput("<pre>Something else</pre>", None, None, None)
            return real(search=search, order=order)

        s.col._backend.search_cards = search_cards
        with self.assertRaises(DriverError) as ctx:
            s.run_all()
        err = ctx.exception.to_json()
        self.assertEqual((err["kind"], err["exception"]), ("anki_error", "InvalidInput"))
        self.assertNotIn("Something", json.dumps(err))

    def test_sorting_changes_nothing(self):
        dump = {"db_dump": None}
        orders = [{"query": "", "kind": k, "order": {"column": c, "reverse": r}}
                  for k in ("cards", "notes") for c in ("cardDue", "deck", "template", "cardEase", "question")
                  for r in (False, True)]
        steps = [ROLLOVER, {"observe": [dump]}, {"observe": [{"search": orders}]}, {"observe": [dump]}]
        out = self.run_worker(steps, notes=2)
        self.assertEqual(out[1], out[3])


# Cards-mode rows (columns.md C4, C5, C7): five cards, today 0, now 2030-01-15 10:00 UTC.
CARDS_SETUP = {
    "decks": [{"name": "A", "preset": "P"}, "Cram"],
    "notes": [qa(1), qa(2), qa(3), qa(4), qa(5, tags=["Marked"])],
}
CARDS_STEPS = [
    card(0, "due = 5"),
    card(1, "type = 2, queue = 2, due = 3, ivl = 10, factor = 2505, reps = 4, lapses = 1, flags = 3, data = ?",
         ['{"s":10.0,"d":3.0}']),
    card(2, "type = 1, queue = -1, due = 1894702800, reps = 1"),
    card(3, "queue = -2, due = 7"),
    card(4, "type = 2, queue = 2, ivl = 8, factor = 2500, reps = 3, odid = did, odue = 4, "
            "did = (SELECT id FROM decks WHERE name = 'Cram'), due = -100000"),
]


def base_cells(i: int) -> dict:
    """The cells of note i's card that no test step changes (C7); each
    expected row adds its Due and Position."""
    stamp = {"datetime": "2030-01-15 10:00"}
    return {
        "noteFld": {"text": f"q{i}"}, "question": {"text": f"q{i}"}, "answer": {"text": f"a{i}"},
        "template": {"text": "C1"}, "deck": {"text": "A"}, "cardEase": {"marker": "new"},
        "cardIvl": {"marker": "new"}, "cardReps": {"count": 0}, "cardLapses": {"count": 0},
        "noteCrt": stamp, "cardMod": stamp, "noteMod": stamp, "note": {"text": "QA"}, "noteTags": {"text": ""},
        "stability": None, "difficulty": None, "retrievability": None,
    }


class Rows(BrowserCase):
    def test_cards_mode_cells_and_tints(self):
        steps = CARDS_STEPS + [{"observe": [{"browser_rows": {"columns": ALL, "rows": [f"#c{i}" for i in range(1, 6)]}}]}]
        out = self.run_worker(steps, setup=CARDS_SETUP)[-1]["browser_rows"]
        expected = [
            # New, due 5.
            {"tint": "none", "cells": {**base_cells(1), "cardDue": {"new_position": 5}, "originalPosition": {"position": 5}}},
            # Review due in 3 days, ivl 10, flag 3, s 10 and d 3 without lrt:
            # C11's example (E = 7 days) gives 93; C7.12 d 3.0 gives 22.
            {"tint": "flag_3", "cells": {
                **base_cells(2), "cardDue": {"date": "2030-01-18"}, "cardEase": {"percent": 250},
                "cardIvl": {"span": {"unit": "days", "amount": "10"}}, "cardReps": {"count": 4},
                "cardLapses": {"count": 1}, "originalPosition": None,
                "stability": {"span": {"unit": "days", "amount": "10"}}, "difficulty": {"percent": 22},
                "retrievability": {"percent": 93}}},
            # Suspended learning card with a timestamp due: empty Due.
            {"tint": "suspended", "cells": {
                **base_cells(3), "cardDue": None, "cardEase": {"percent": 0}, "cardIvl": {"marker": "learning"},
                "cardReps": {"count": 1}, "originalPosition": None}},
            # Buried new card: wrapped new position.
            {"tint": "buried", "cells": {
                **base_cells(4), "cardDue": {"new_position": 7, "wrapped": True}, "originalPosition": {"position": 7}}},
            # Filtered review card of a marked note.
            {"tint": "marked", "cells": {
                **base_cells(5), "deck": {"text": "Cram (A)"}, "cardDue": {"filtered": True},
                "cardEase": {"percent": 250}, "cardIvl": {"span": {"unit": "days", "amount": "8"}},
                "cardReps": {"count": 3}, "noteTags": {"text": "Marked"}, "originalPosition": None}},
        ]
        for i, (got, want) in enumerate(zip(out, expected), 1):
            with self.subTest(card=f"#c{i}"):
                self.assertEqual(got, want)
        self.assertEqual(len(out), len(expected))

    def test_cells_follow_the_requested_columns_in_order(self):
        steps = [{"observe": [{"browser_rows": {"columns": ["cardReps", "noteFld"], "rows": ["#c2", "#c1"]}}]}]
        out = self.run_steps(steps, notes=2)[0]["browser_rows"]
        self.assertEqual(out, [{"tint": "none", "cells": {"cardReps": {"count": 0}, "noteFld": {"text": "q2"}}},
                               {"tint": "none", "cells": {"cardReps": {"count": 0}, "noteFld": {"text": "q1"}}}])
        self.assertEqual(list(out[0]["cells"]), ["cardReps", "noteFld"])

    def test_notes_mode_and_a_first_card_of_ordinal_1(self):
        # #n1 is added with only its Backward card (#c1, ordinal 1); edit_note
        # then generates the Forward card (ordinal 0) with a higher id, which
        # adopt names #c3 (edit_note names none, note-ops.md N3). Notes mode
        # reads the first (lowest-id) card (C8).
        setup = {"notetypes": [NOTETYPE, PAIR],
                 "notes": [{"notetype": "Pair", "deck": "A", "fields": ["", "b1"]}, qa(2, tags=["marked"])]}
        columns = ["noteFld", "question", "template", "deck", "cardDue", "cardEase", "cardIvl"]
        steps = [
            {"edit_note": {"note": "#n1", "fields": {"Front": "f1"}}},
            {"adopt": {}},
            {"observe": ["ids", {"browser_rows": {"columns": ["template", "noteFld"], "rows": ["#c1", "#c3", "#c2"]}}]},
            {"set_config": {"key": "browserTableShowNotesMode", "value": True}},
            {"observe": [{"browser_rows": {"columns": columns, "rows": ["#n1", "#n2"]}}]},
        ]
        out = self.run_worker(steps, setup=setup)
        self.assertEqual(out[1], {"adopt": {"notes": 0, "cards": 1}})
        self.assertEqual(sorted(out[2]["ids"]), ["#c1", "#c2", "#c3", "#n1", "#n2"])
        self.assertEqual(out[2]["browser_rows"], [
            {"tint": "none", "cells": {"template": {"text": "Backward"}, "noteFld": {"text": "f1"}}},
            {"tint": "none", "cells": {"template": {"text": "Forward"}, "noteFld": {"text": "f1"}}},
            {"tint": "marked", "cells": {"template": {"text": "C1"}, "noteFld": {"text": "q2"}}},
        ])
        self.assertEqual(out[4]["browser_rows"], [
            {"tint": "none", "cells": {
                "noteFld": {"text": "f1"}, "question": {"text": "b1"}, "template": {"count": 2},
                "deck": {"text": "A"}, "cardDue": None, "cardEase": {"marker": "new"}, "cardIvl": None}},
            {"tint": "marked", "cells": {
                "noteFld": {"text": "q2"}, "question": {"text": "q2"}, "template": {"count": 1},
                "deck": {"text": "A"}, "cardDue": None, "cardEase": {"marker": "new"}, "cardIvl": None}},
        ])

    def test_notes_mode_only_when_the_key_holds_true(self):
        # browser/config-keys K2.1: any other value is cards mode.
        rows = {"browser_rows": {"columns": ["noteFld"], "rows": ["#c1"]}}
        for value in ["true", 1, None, {"a": True}]:
            with self.subTest(value=value):
                out = self.run_steps([{"set_config": {"key": "browserTableShowNotesMode", "value": value}},
                                      {"observe": [rows]}], notes=1)
                self.assertEqual(out[1]["browser_rows"], [{"tint": "none", "cells": {"noteFld": {"text": "q1"}}}])

    def test_references_of_the_other_kind_are_invalid(self):
        cards = {"browser_rows": {"columns": ["noteFld"], "rows": ["#c1", "#n1"]}}
        notes = {"browser_rows": {"columns": ["noteFld"], "rows": ["#n1", "#c1"]}}
        notes_mode = {"set_config": {"key": "browserTableShowNotesMode", "value": True}}
        for steps in ([{"observe": [cards]}], [notes_mode, {"observe": [notes]}]):
            with self.subTest(steps=steps):
                self.assertEqual(self.invalid(steps, notes=1).kind, "invalid_scenario")
        unknown = self.invalid([{"observe": [{"browser_rows": {"columns": ["noteFld"], "rows": ["#c2"]}}]}], notes=1)
        self.assertEqual(unknown.to_json()["kind"], "unknown_ref")

    def test_bad_requests_are_invalid(self):
        bad = [None, [], "#c1", {}, {"columns": ["noteFld"]}, {"rows": ["#c1"]},
               {"columns": [], "rows": ["#c1"]}, {"columns": ["noteFld"], "rows": []},
               {"columns": ["bogus"], "rows": ["#c1"]}, {"columns": ["NoteFld"], "rows": ["#c1"]},
               {"columns": ["noteFld", "noteFld"], "rows": ["#c1"]}, {"columns": ["noteFld"], "rows": ["#c1", "#c1"]},
               {"columns": "noteFld", "rows": ["#c1"]}, {"columns": ["noteFld"], "rows": "#c1"},
               {"columns": ["noteFld"], "rows": [1]}, {"columns": [1], "rows": ["#c1"]},
               {"columns": ["noteFld"], "rows": ["#c1"], "mode": "cards"}]
        for args in bad:
            with self.subTest(args=args):
                self.assertEqual(self.invalid([{"observe": [{"browser_rows": args}]}], notes=1).kind, "invalid_scenario")
        twice = {"browser_rows": {"columns": ["noteFld"], "rows": ["#c1"]}}
        self.assertEqual(self.invalid([{"observe": [twice, twice]}], notes=1).kind, "invalid_scenario")
        for text in ("browser_rows", "browser_rows(#c1)"):
            with self.subTest(text=text):
                self.assertEqual(self.invalid([{"observe": [text]}], notes=1).kind, "invalid_scenario")

    def test_bad_requests_fail_before_any_step_runs(self):
        # B3's shape is checked when the scenario loads: a later bad request
        # stops the run before the first step.
        steps = [{"db_exec": {"sql": "SELECT 1"}},
                 {"observe": [{"browser_rows": {"columns": ["bogus"], "rows": ["#c1"]}}]}]
        err = self.invalid(steps, notes=1)
        self.assertEqual((err.kind, err.to_json().get("step")), ("invalid_scenario", 1))

    def test_reading_rows_changes_nothing(self):
        dump = {"db_dump": None}
        steps = CARDS_STEPS + [
            ROLLOVER,
            {"observe": [dump]},
            {"observe": [{"browser_rows": {"columns": ALL, "rows": [f"#c{i}" for i in range(1, 6)]}}]},
            {"observe": [dump]},
            {"set_config": {"key": "browserTableShowNotesMode", "value": True}},
            {"observe": [dump]},
            {"observe": [{"browser_rows": {"columns": ALL, "rows": ["#n1", "#n5"]}}]},
            {"observe": [dump]},
        ]
        out = self.run_worker(steps, setup=CARDS_SETUP)
        n = len(CARDS_STEPS) + 1
        self.assertEqual(out[n], out[n + 2])
        self.assertEqual(out[n + 4], out[n + 6])


class RowErrors(BrowserCase):
    # columns.md C3.1: #n2 is deleted with #c2; #n3's card #c3 is removed from
    # under it; #c4's deck and #n5's note type do not exist.
    SETUP = {"notes": [qa(1), qa(2), qa(3), qa(4), qa(5)]}
    STEPS = [
        {"delete_notes": {"notes": ["#n2"]}},
        {"db_exec": {"sql": "DELETE FROM cards WHERE nid = (SELECT id FROM notes ORDER BY id LIMIT 1 OFFSET 1)"}},
        {"db_exec": {"sql": "UPDATE cards SET did = 424242 WHERE nid = (SELECT id FROM notes ORDER BY id LIMIT 1 OFFSET 2)"}},
        {"db_exec": {"sql": "UPDATE notes SET mid = 99 WHERE id = (SELECT id FROM notes ORDER BY id LIMIT 1 OFFSET 3)"}},
    ]

    def test_deleted_and_damaged_rows(self):
        kept = {"tint": "none", "cells": {"noteFld": {"text": "q1"}}}
        steps = self.STEPS + [
            {"observe": [{"browser_rows": {"columns": ["noteFld"], "rows": ["#c1", "#c2", "#c3", "#c4", "#c5"]}}]},
            {"set_config": {"key": "browserTableShowNotesMode", "value": True}},
            {"observe": [{"browser_rows": {"columns": ["noteFld"], "rows": ["#n1", "#n2", "#n3", "#n4", "#n5"]}}]},
        ]
        out = self.run_worker(steps, setup=self.SETUP)
        deleted, damaged = {"error": "deleted"}, {"error": "damaged"}
        self.assertEqual(out[4]["browser_rows"], [kept, deleted, deleted, damaged, damaged])
        self.assertEqual(out[6]["browser_rows"], [kept, deleted, damaged, damaged, damaged])

    def planted(self, kind: int, message):
        """A session whose backend answers #c1's Browse row with a backend
        error of that kind; message(session) is its message."""
        s = self.session([{"observe": [{"browser_rows": {"columns": ["noteFld"], "rows": ["#c1"]}}]}], notes=1)
        target = generic_pb2.Int64(val=s.cards[0]).SerializeToString()
        error = backend_pb2.BackendError(kind=kind, message=message(s)).SerializeToString()

        class Planted:
            def __init__(self, real):
                self.real = real

            def command(self, service, method, data):
                if data == target:
                    raise Exception(error)
                return self.real.command(service, method, data)

            def __getattr__(self, name):
                return getattr(self.real, name)

        backend = s.col._backend
        real = backend._backend
        backend._backend = Planted(real)
        self.addCleanup(setattr, backend, "_backend", real)
        return s

    def test_any_other_backend_error_is_anki_error_without_its_message(self):
        kinds = backend_pb2.BackendError
        cases = {
            "database": (kinds.DB_ERROR, "DBError"),
            "other invalid input": (kinds.INVALID_INPUT, "InvalidInput"),
            "io": (kinds.IO_ERROR, "BackendIOError"),
        }
        for name, (kind, exception) in cases.items():
            with self.subTest(name):
                s = self.planted(kind, lambda _s: "Something else")
                with self.assertRaises(DriverError) as ctx:
                    s.run_all()
                err = ctx.exception.to_json()
                self.assertEqual((err["kind"], err["exception"]), ("anki_error", exception))
                self.assertNotIn("Something", json.dumps(err))

    def test_row_errors_follow_the_backend_error_kind(self):
        # Check Database travels as an invalid-input error with its own text.
        s = self.planted(backend_pb2.BackendError.INVALID_INPUT, lambda s: s.col.tr.errors_please_check_database())
        self.assertEqual(s.run_all()["steps"][0], {"browser_rows": [{"error": "damaged"}]})
        for kind, row in ((backend_pb2.BackendError.DELETED, "deleted"), (backend_pb2.BackendError.NOT_FOUND_ERROR, "damaged")):
            with self.subTest(row=row):
                s = self.planted(kind, lambda _s: "Something else")
                self.assertEqual(s.run_all()["steps"][0], {"browser_rows": [{"error": row}]})


class Cells(unittest.TestCase):
    """B4's forms, one column family at a time."""

    def check(self, column, cases, notes_mode=False):
        for text, cell in cases.items():
            with self.subTest(column=column, text=text):
                self.assertEqual(parse_cell(column, text, notes_mode), cell)

    def test_text_columns_keep_the_text_without_isolation_marks(self):
        for column in ("noteFld", "question", "answer", "deck", "note", "noteTags"):
            self.check(column, {"": {"text": ""}, "a b": {"text": "a b"}, "\u2068(2)\u2069": {"text": "(2)"},
                                "New #5": {"text": "New #5"}})
        self.check("template", {"Cloze 10": {"text": "Cloze 10"}, "2": {"text": "2"}})

    def test_counts(self):
        cases = {"0": {"count": 0}, "12": {"count": 12}, "\u20683\u2069": {"count": 3}, "": {"unparsed": ""},
                 "-1": {"unparsed": "-1"}, "1.5": {"unparsed": "1.5"}, "\u0663": {"unparsed": "\u0663"}}
        for column in ("cardReps", "cardLapses"):
            self.check(column, cases)
        self.check("template", cases, notes_mode=True)

    def test_percents(self):
        cases = {"": None, "250%": {"percent": 250}, "7%": {"percent": 7}, "-3%": {"percent": -3},
                 "\u2068250\u2069%": {"percent": 250}, "250": {"unparsed": "250"}, "2.5%": {"unparsed": "2.5%"}}
        for column in ("cardEase", "difficulty", "retrievability"):
            self.check(column, {**cases, "(new)": {"marker": "new"} if column == "cardEase" else {"unparsed": "(new)"}})

    def test_positions(self):
        self.check("originalPosition", {"": None, "3": {"position": 3}, "-100000": {"position": -100000},
                                        "1.5": {"unparsed": "1.5"}, "New #3": {"unparsed": "New #3"}})

    def test_due(self):
        self.check("cardDue", {
            "": None,
            "(filtered)": {"filtered": True},
            "((filtered))": {"filtered": True, "wrapped": True},
            "New #5": {"new_position": 5},
            "New #-100000": {"new_position": -100000},
            "(New #5)": {"new_position": 5, "wrapped": True},
            "\u2068New #\u20685\u2069\u2069": {"new_position": 5},
            "2030-01-17": {"date": "2030-01-17"},
            "(2030-01-17)": {"date": "2030-01-17", "wrapped": True},
            "((2030-01-17))": {"unparsed": "((2030-01-17))"},
            "(New #5": {"unparsed": "(New #5"},
            "()": {"unparsed": "()"},
            "filtered": {"unparsed": "filtered"},
            "2030-1-17": {"unparsed": "2030-1-17"},
            "2030-01-17 @ 10:00": {"unparsed": "2030-01-17 @ 10:00"},
        })

    def test_date_times(self):
        for column in ("noteCrt", "cardMod", "noteMod"):
            self.check(column, {"2030-01-15 @ 10:00": {"datetime": "2030-01-15 10:00"},
                                "": {"unparsed": ""}, "2030-01-15": {"unparsed": "2030-01-15"},
                                "2030-01-15 10:00": {"unparsed": "2030-01-15 10:00"}})

    def test_time_spans(self):
        def span(unit, amount):
            return {"span": {"unit": unit, "amount": amount}}

        cases = {
            "": None,
            "(new)": {"marker": "new"},
            "(learning)": {"marker": "learning"},
            "0 seconds": span("seconds", "0"),
            "1 second": span("seconds", "1"),
            "1.5 minutes": span("minutes", "1.5"),
            "1 minute": span("minutes", "1"),
            "2 hours": span("hours", "2"),
            "1 hour": span("hours", "1"),
            "1 day": span("days", "1"),
            "10 days": span("days", "10"),
            "1.3 months": span("months", "1.3"),
            "1 month": span("months", "1"),
            "1 year": span("years", "1"),
            "82.2 years": span("years", "82.2"),
            "\u20681.3\u2069 months": span("months", "1.3"),
            "10 fortnights": {"unparsed": "10 fortnights"},
            "10  days": {"unparsed": "10  days"},
            "10days": {"unparsed": "10days"},
            "1. days": {"unparsed": "1. days"},
            "(filtered)": {"unparsed": "(filtered)"},
        }
        self.check("cardIvl", cases)
        # Stability is a time span or empty only (B4): no marker form.
        cases["(new)"] = {"unparsed": "(new)"}
        cases["(learning)"] = {"unparsed": "(learning)"}
        self.check("stability", cases)


if __name__ == "__main__":
    unittest.main()
