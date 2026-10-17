# SPDX-License-Identifier: AGPL-3.0-or-later
"""The fields_check observation (specs/conformance/note-ops.md N4): states and
their order, duplicates of new and stored drafts, request checks, errors, the
dupe search's text guard, and that the scenario's collection never changes.
Expected values come from specs/notes/field-check.md K2-K4 and the scenario's
own setup, never from the driver."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.session import Session

STD = {"name": "Std", "fields": ["Front", "Back"], "templates": [{"name": "One", "front": "{{Front}}", "back": "{{Back}}"}]}
GAP = {"name": "Gap", "kind": "cloze", "fields": ["Text", "Extra"],
       "templates": [{"name": "Gap", "front": "{{cloze:Text}}", "back": "{{cloze:Text}}<br>{{Extra}}"}]}
GAP2 = {"name": "Gap2", "kind": "cloze", "fields": ["Text", "Extra"],
        "templates": [{"name": "Gap", "front": "{{cloze:Text}}{{cloze:Extra}}", "back": "{{cloze:Text}}"}]}
KAISHI = {"name": "Kaishi", "fields": ["Word", "Meaning"],
          "templates": [{"name": "Recognize", "front": "{{Word}}", "back": "{{Meaning}}"}]}
OTHER = dict(KAISHI, name="Other")


def check(*cases):
    return {"fields_check": {"cases": list(cases)}}


def new(notetype, *fields):
    return {"notetype": notetype, "fields": list(fields)}


def state(name):
    return {"state": name}


def dupes(*refs):
    return {"state": "duplicate", "duplicates": list(refs)}


class FieldsCheck(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, steps, setup=None):
        sc = {"id": "t/fields-check", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        if setup is not None:
            sc["setup"] = setup
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def run_steps(self, steps, setup=None):
        s = self.session(steps, setup)
        try:
            return s.run_all()["steps"]
        finally:
            s.close()

    def error(self, steps, setup=None) -> dict:
        with self.assertRaises(DriverError) as ctx:
            self.run_steps(steps, setup)
        return ctx.exception.to_json()

    # --- values --------------------------------------------------------------

    def test_states_and_their_order(self):
        setup = {"notetypes": [STD, GAP, GAP2], "notes": [{"notetype": "Std", "fields": ["dup", "x"]}]}
        out = self.run_steps([{"observe": [check(
            new("Std", "x", ""),
            new("Std", "", "y"),
            new("Std", "<br> <div></div>", ""),
            new("Std", "&nbsp;\u3000", ""),
            new("Std", '<img src="a.jpg">', ""),
            new("Std", "{{c1::x}}", ""),
            new("Std", "x", "{{c1::y}}"),
            new("Std", "", "{{c1::y}}"),
            new("Std", "{{c0::x}}", ""),
            new("Std", "{{c1::x", ""),
            new("Std", "dup", "{{c2::z}}"),
            new("Gap", "{{c1::x}}", ""),
            new("Gap", "x", ""),
            new("Gap", "", ""),
            new("Gap", "x", "{{c1::y}}"),
            new("Gap", "", "{{c1::y}}"),
            new("Gap", "{{c0::x {{c1::y}}}}", ""),
            new("Gap2", "x", "{{c1::y}}"),
            new("Gap2", "", "{{c1::y}}"),
            new("Std", "dup", "z"),
        )]}], setup)
        # K3's table: a misplaced cloze first, then an empty first field, then
        # cloze problems, then a duplicate (the stored note is #n1).
        self.assertEqual(out[0]["fields_check"], [
            state("normal"), state("empty"), state("empty"), state("empty"), state("normal"),
            state("notetype_not_cloze"), state("notetype_not_cloze"), state("empty"), state("normal"),
            state("normal"), state("notetype_not_cloze"),
            state("normal"), state("missing_cloze"), state("empty"), state("field_not_cloze"),
            state("field_not_cloze"), state("missing_cloze"),
            state("normal"), state("empty"),
            dupes("#n1"),
        ])

    def test_a_cloze_type_whose_first_front_does_not_parse_has_no_cloze_field(self):
        setup = {"notetypes": [dict(GAP, name="Broken")]}
        # The front becomes "{{#Text}}{{cloze:Text}}" (note-ops.md N2: field 1
        # of templates.config appended, then reopen).
        front = "{{#Text}}{{cloze:Text}}".encode()
        blob = (b"\x0a" + bytes([len(front)]) + front).hex()
        steps = [
            {"db_exec": {"sql": f"UPDATE templates SET config = CAST(config || X'{blob}' AS BLOB) "
                                "WHERE ntid = (SELECT id FROM notetypes WHERE name = 'Broken')"}},
            {"reopen": {}},
            {"observe": [check(new("Broken", "{{c1::x}}"), new("Broken", "x"))]},
        ]
        out = self.run_steps(steps, setup)
        self.assertEqual(out[2]["fields_check"], [state("field_not_cloze"), state("missing_cloze")])

    def test_duplicates_are_other_notes_of_the_type_in_note_id_order(self):
        setup = {
            "notetypes": [KAISHI, OTHER],
            "notes": [
                {"notetype": "Kaishi", "fields": ["私", "I"]},
                {"notetype": "Other", "fields": ["私", "I"]},
                {"notetype": "Kaishi", "fields": ["<b>私</b>", "me"]},
                {"notetype": "Kaishi", "fields": ["caf\u00e9", "coffee"]},
                {"notetype": "Kaishi", "fields": ["stale", "old"]},
            ],
        }
        cases = [
            new("Kaishi", "私"),
            new("Other", "<i>私</i>"),
            {"notetype": "Kaishi", "fields": {"Word": "&#31169;"}},
            {"note": "#n1"},
            {"note": "#n3"},
            {"note": "#n3", "fields": ["other"]},
            {"note": "#n1", "fields": {"Meaning": "changed"}},
            {"note": "#n2"},
            new("Kaishi", "私 "),
            new("Kaishi", "私\u0001"),
            new("Kaishi", "Caf\u00e9"),
            new("Kaishi", "stale"),
            new("Kaishi", "cafe\u0301"),
        ]
        steps = [
            {"db_exec": {"sql": "UPDATE notes SET csum = 1 WHERE sfld = 'stale'"}},
            {"observe": [check(*cases)]},
            {"set_config": {"key": "normalize_note_text", "value": False}},
            {"observe": [check(new("Kaishi", "cafe\u0301"), new("Kaishi", "caf\u00e9"))]},
        ]
        out = self.run_steps(steps, setup)
        # K4: same note type, not the draft's own note, stored csum, stripped
        # stored first field equal to the stripped (NFC when the key is on)
        # draft; ascending note id order, which is creation order here.
        self.assertEqual(out[1]["fields_check"], [
            dupes("#n1", "#n3"),
            dupes("#n2"),
            dupes("#n1", "#n3"),
            dupes("#n3"),
            dupes("#n1"),
            state("normal"),
            dupes("#n3"),
            state("normal"),
            state("normal"),
            state("normal"),
            state("normal"),
            state("normal"),
            dupes("#n4"),
        ])
        self.assertEqual(out[3]["fields_check"], [state("normal"), dupes("#n4")])

    def test_the_search_string_carries_quotes_backslashes_brackets_and_spaces(self):
        words = ['a "b" \\c (d) e\\', 'x"y\\', "and", "-x", "a:b", "or ", " x"]
        setup = {"notetypes": [KAISHI], "notes": [{"notetype": "Kaishi", "fields": [w, "x"]} for w in words]}
        out = self.run_steps([{"observe": [check(*(new("Kaishi", w) for w in words))]}], setup)
        self.assertEqual(out[0]["fields_check"], [dupes(f"#n{i}") for i in range(1, len(words) + 1)])

    def test_a_first_field_the_search_string_cannot_carry_is_refused(self):
        # The search parser turns other white space into U+0020 (and trims an
        # unquoted term): the dupe search would look for the second text, the
        # stored #n2, instead of the draft's twin #n1.
        for word, parsed in (("a\tb", "a b"), ("a\u3000b", "a b"), ("a\nb", "a b"), ("a\u00a0b", "a b"),
                             ("ab\t", "ab")):
            notes = [{"notetype": "Kaishi", "fields": [word, "x"]}, {"notetype": "Kaishi", "fields": [parsed, "x"]}]
            setup = {"notetypes": [KAISHI], "notes": notes}
            with self.subTest(word=word):
                err = self.error([{"observe": [check(new("Kaishi", "x"), new("Kaishi", word))]}], setup)
                self.assertEqual((err["kind"], err["step"], err["observation"]), ("internal_error", 0, "fields_check"))
        # Without a duplicate there is no search.
        setup = {"notetypes": [KAISHI]}
        out = self.run_steps([{"observe": [check(new("Kaishi", "a\tb"))]}], setup)
        self.assertEqual(out[0]["fields_check"], [state("normal")])

    def test_the_collection_never_changes(self):
        setup = {"notetypes": [KAISHI, GAP], "notes": [{"notetype": "Kaishi", "fields": ["私", "I"]},
                                                     {"notetype": "Kaishi", "fields": ["私", "I"]}]}
        steps = [
            {"observe": ["db_dump"]},
            {"observe": [check(new("Kaishi", "私"), {"note": "#n1", "fields": ["new"]}, {"note": "#n2"},
                               new("Gap", "{{c1::x}}"))]},
            {"observe": ["db_dump"]},
        ]
        out = self.run_steps(steps, setup)
        self.assertEqual(out[1]["fields_check"], [dupes("#n1", "#n2"), state("normal"), dupes("#n1"), state("normal")])
        self.assertEqual(out[2]["db_dump"], out[0]["db_dump"])

    # --- errors ----------------------------------------------------------------

    def test_requests_outside_n4_are_invalid_before_any_collection(self):
        good = new("Std", "x")
        bad = [
            {"fields_check": None},
            {"fields_check": []},
            {"fields_check": {}},
            {"fields_check": {"cases": []}},
            {"fields_check": {"cases": good}},
            {"fields_check": {"cases": [good], "extra": 1}},
            check("Std"),
            check({"notetype": "Std"}),
            check({"fields": ["x"]}),
            check({"notetype": "", "fields": ["x"]}),
            check({"notetype": 1, "fields": ["x"]}),
            check({"notetype": "Std", "fields": ["x"], "tags": []}),
            check({"notetype": "Std", "note": "#n1", "fields": ["x"]}),
            check({"note": "#c1"}),
            check({"note": "#n0"}),
            check({"note": 1}),
            check({"note": "#n1", "deck": "A"}),
            check(new("Std", 1)),
            check(new("Std", None)),
            check(new("Std", True)),
            check({"notetype": "Std", "fields": {"Front": 1.0}}),
            check({"notetype": "Std", "fields": "x"}),
            check({"note": "#n1", "fields": None}),
        ]
        for req in bad:
            with self.subTest(req=req), self.assertRaises(DriverError) as ctx:
                self.session([{"observe": ["ids"]}, {"observe": [req]}])
            self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("invalid_scenario", 1))
        with self.assertRaises(DriverError) as ctx:
            self.session([{"observe": [check(good), check(good)]}])
        self.assertEqual(ctx.exception.kind, "invalid_scenario")
        with self.assertRaises(DriverError) as ctx:
            self.session([{"observe": ["fields_check"]}])
        self.assertEqual(ctx.exception.kind, "invalid_scenario")

    def test_field_names_and_counts_are_checked_when_the_step_runs(self):
        setup = {"notetypes": [STD], "notes": [{"notetype": "Std", "fields": ["a", "b"]}]}
        for case in (new("Std", "a", "b", "c"), {"notetype": "Std", "fields": {"front": "a"}},
                     {"note": "#n1", "fields": ["a", "b", "c"]}, {"note": "#n1", "fields": {"Nope": "a"}}):
            with self.subTest(case=case):
                err = self.error([{"observe": ["ids"]}, {"observe": [check(new("Std", "x"), case)]}], setup)
                self.assertEqual((err["kind"], err["step"]), ("invalid_scenario", 1))

    def test_unknown_note_types_and_notes(self):
        setup = {"notetypes": [STD], "notes": [{"notetype": "Std", "fields": ["a", "b"]}]}
        # Stored names match exactly: the notetypes table's unicase collation
        # is not the scenario's.
        self.assertEqual(self.error([{"observe": [check(new("std", "x"))]}], setup),
                         {"kind": "unknown_ref", "ref": "std", "step": 0})
        self.assertEqual(self.error([{"observe": [check({"note": "#n2"})]}], setup),
                         {"kind": "unknown_ref", "ref": "#n2", "step": 0})
        gone = self.error([{"delete_notes": {"notes": ["#n1"]}}, {"observe": [check({"note": "#n1"})]}], setup)
        self.assertEqual(gone, {"kind": "unknown_ref", "ref": "#n1", "step": 1})
        renamed = [{"db_exec": {"sql": "UPDATE notetypes SET name = 'Renamed' WHERE name = 'Std'"}}, {"reopen": {}},
                   {"observe": [check(new("Renamed", "x"))]}]
        self.assertEqual(self.run_steps(renamed, setup)[2]["fields_check"], [state("normal")])
