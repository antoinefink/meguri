# SPDX-License-Identifier: AGPL-3.0-or-later
"""The template observation (specs/conformance/render-ops.md Q1-Q2): values,
composition of FrontSide and blank questions, error kinds and sides, the
special values of new notes and stored cards, request checks, and that the
scenario's collection never changes. Expected values come from the specs
(render-ops.md, specs/render/*.md) and the scenario's own setup, never from
the driver."""

from __future__ import annotations

import html
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from anki.collection import Collection

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.observe import template as template_module
from driver.session import Session

PROBE = {"name": "Probe", "fields": ["Front", "Back"]}


def request(cases, notetype=PROBE):
    args = {"cases": cases}
    if notetype is not None:
        args["notetype"] = notetype
    return {"template": args}


def ok(question, answer):
    return {"question": question, "answer": answer, "empty": False}


def blank(question):
    return {"question": question, "answer": None, "empty": True}


def error(kind, side):
    return {"error": {"kind": kind, "side": side}}


class TemplateObservation(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, steps, setup=None):
        sc = {"id": "t/template", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        if setup is not None:
            sc["setup"] = setup
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def run_steps(self, steps, setup=None):
        s = self.session(steps, setup)
        try:
            return s.run_all()["steps"]
        finally:
            s.close()

    def values(self, cases, notetype=PROBE):
        return self.run_steps([{"observe": [request(cases, notetype)]}])[0]["template"]

    def assert_invalid(self, steps, setup=None):
        with self.assertRaises(DriverError) as ctx:
            self.run_steps(steps, setup)
        self.assertEqual(ctx.exception.kind, "invalid_scenario", ctx.exception.to_json())

    # --- values --------------------------------------------------------------

    def test_question_and_answer_with_front_side(self):
        values = self.values([
            {"front": "{{Front}}", "back": "{{FrontSide}}<hr>{{Back}}", "fields": ["ein", "zwei"]},
            {"front": "{{Back}}", "fields": {"Back": "zwei"}},
            {"front": "<{{FrontSide}}>{{Front}}", "back": "[{{FrontSide}}][{{FrontSide}}]", "fields": ["ein"]},
        ])
        # Q1.4: FrontSide is nothing on the question side and the question
        # output on the answer side; back defaults to empty, missing values too.
        self.assertEqual(values, [ok("ein", "ein<hr>zwei"), ok("zwei", ""), ok("<>ein", "[<>ein][<>ein]")])

    def test_unknown_filters_end_the_chain_and_front_side_carries_their_text(self):
        values = self.values([
            {"front": "{{custom:Front}}", "back": "{{FrontSide}}|{{custom:text:Back}}|{{text:custom:Back}}",
             "fields": ["<b>a</b>", "<i>b</i>"]},
        ])
        # filters.md F2: built-ins nearest the key run, the first unknown name
        # stops the chain and nothing after it runs.
        self.assertEqual(values, [ok("<b>a</b>", "<b>a</b>|b|<i>b</i>")])

    def test_blank_questions_keep_their_output_and_hide_the_answer(self):
        values = self.values([
            {"front": "{{Front}}", "back": "{{Back}}", "fields": ["", "zwei"]},
            {"front": "x{{text:}}<!--{{Front}}-->", "back": "{{Nope}}", "fields": ["ein", "zwei"]},
            {"front": "{{^Front}}none{{/Front}}", "back": "{{Back}}", "fields": ["", "zwei"]},
        ])
        # T10: the blank notice is no part of the value; the answer's error
        # stays hidden behind a blank question (T9).
        self.assertEqual(values, [blank(""), blank("x<!--{{Front}}-->"), blank("none")])

    def test_every_error_kind_and_side(self):
        values = self.values([
            {"front": "{{Front}}{{#Back}}x", "fields": ["f", "b"]},
            {"front": "{{Front}}", "back": "{{/Back}}", "fields": ["f", "b"]},
            {"front": "{{#Front}}{{/Back}}", "fields": ["f", "b"]},
            {"front": "{{Nope}}", "fields": ["f", "b"]},
            {"front": "{{Front}}", "back": "{{text:hint:Nope}}", "fields": ["f", "b"]},
            {"front": "{{Front}}{{#}}", "fields": ["f", "b"]},
            {"front": "{{Front}}{{^}}", "fields": ["f", "b"]},
            {"front": "{{Front}}{{#Nope}}{{/Nope}}", "fields": ["f", "b"]},
            {"front": "{{Front}}", "back": "{{^N}}{{/N}}", "fields": ["f", "b"]},
            {"front": "{{Front}}{{#x\n<'\"&>}}{{/x\n<'\"&>}}", "fields": ["f", "b"]},
            {"front": "{{Front}}{{<'\"&>\n}}", "fields": ["f", "b"]},
        ])
        self.assertEqual(values, [
            error("unclosed_conditional", "question"),
            error("unopened_conditional", "answer"),
            error("mismatched_conditional", "question"),
            error("unknown_field", "question"),
            error("unknown_field", "answer"),
            # Q2 / T5: a one-character "#" or "^" tag is a replacement.
            error("unknown_field", "question"),
            error("unknown_field", "question"),
            error("unknown_conditional", "question"),
            error("unknown_conditional", "answer"),
            # Quoted tags are HTML-escaped and may span lines.
            error("unknown_conditional", "question"),
            error("unknown_field", "question"),
        ])

    def test_new_note_special_values(self):
        values = self.values([
            {"front": "{{Type}}|{{Card}}|{{Deck}}|{{Subdeck}}|{{CardFlag}}|{{CardID}}|{{Tags}}|{{c2}}|{{Front}}",
             "ord": 1, "fields": ["f"], "tags": ["zeta", "alpha"]},
            {"front": "{{Card}}|{{c1}}{{Front}}", "name": "Reverse", "fields": ["f"]},
            {"front": "{{Card}}|{{c500}}{{Front}}", "ord": 499, "fields": ["f"]},
        ])
        # Q1.2 / special-fields P3: tags as given in their order, the default
        # template name "Card <ord + 1>", and a new card's deck, flag and id.
        self.assertEqual(values, [
            ok("Probe|Card 2|(Deck)|(Deck)|flag0|0|zeta alpha|1|f", ""),
            ok("Reverse|1f", ""),
            ok("Card 500|1f", ""),
        ])

    def test_note_type_names_are_exact_even_over_stock_ones(self):
        steps = [
            {"observe": [request([{"front": "{{Type}}{{Q}}", "fields": ["q"]}], {"name": "Basic", "fields": ["Q"]})]},
            {"observe": [request([{"front": "{{Type}}{{Q}}{{R}}", "fields": ["q", "r"]}],
                                 {"name": "basic", "fields": ["Q", "R"]})]},
            {"observe": [request([{"front": "{{Type}}{{cloze:T}}", "fields": ["{{c1::x}}"]}],
                                 {"name": "Cloze", "kind": "cloze", "fields": ["T"]})]},
            {"observe": [request([{"front": "{{Type}}{{T}}", "fields": ["x"]}],
                                 {"name": "Cloze", "kind": "cloze", "fields": ["T"]})]},
        ]
        out = [step["template"][0] for step in self.run_steps(steps)]
        self.assertEqual(out[0], ok("Basicq", ""))
        self.assertEqual(out[1], ok("basicqr", ""))
        self.assertEqual(out[2]["question"][:5], "Cloze")
        self.assertIs(out[2]["empty"], False)
        # T10 for a cloze note type: no cloze 1 in any field is blank.
        self.assertEqual(out[3], blank("Clozex"))

    def test_card_cases_use_the_stored_note_card_and_deck(self):
        setup = {
            "decks": ["Lang::Vocab"],
            "notetypes": [
                {"name": "Vocab", "fields": ["Word", "Meaning"],
                 "templates": [{"name": "Recognition", "front": "{{Word}}", "back": "{{Meaning}}"},
                               {"name": "Recall", "front": "{{Meaning}}", "back": "{{Word}}"}]},
                {"name": "Gap", "kind": "cloze", "fields": ["Text"],
                 "templates": [{"name": "Gap Card", "front": "{{cloze:Text}}", "back": "{{cloze:Text}}"}]},
            ],
            "notes": [
                {"notetype": "Vocab", "deck": "Lang::Vocab", "fields": ["neko", "cat"], "tags": ["animal"]},
                {"notetype": "Gap", "deck": "Lang::Vocab", "fields": ["a {{c2::b}}"]},
            ],
        }
        specials = "{{Deck}}|{{Subdeck}}|{{Card}}|{{Type}}|{{CardFlag}}|{{CardID}}|{{Tags}}|{{#c1}}one{{/c1}}{{#c2}}two{{/c2}}"
        steps = [
            {"observe": ["ids", "db_dump"]},
            {"observe": [request([
                {"card": "#c2", "front": specials, "back": "{{FrontSide}}|{{Word}}"},
                {"card": "#c3", "front": specials + "|{{Text}}"},
                {"card": "#c1", "front": "{{Nope}}"},
            ], notetype=None)]},
            {"observe": ["db_dump"]},
        ]
        out = self.run_steps(steps, setup)
        ids = out[0]["ids"]
        # P3: the card's deck and its last component, the stored template's
        # name (the first one for cloze), the card's id; c<ord+1> only.
        front2 = f"Lang::Vocab|Vocab|Recall|Vocab|flag0|{ids['#c2']}|animal|two"
        front3 = f"Lang::Vocab|Vocab|Gap Card|Gap|flag0|{ids['#c3']}||two|a {{{{c2::b}}}}"
        self.assertEqual(out[1]["template"], [
            ok(front2, front2 + "|neko"),
            ok(front3, ""),
            error("unknown_field", "question"),
        ])
        # Q1.6: nothing in the scenario's collection changes.
        self.assertEqual(out[2]["db_dump"], out[0]["db_dump"])

    def test_fill_empty_on_new_notes(self):
        # render-ops.md Q1.4 step 0, render/preview V6 and V7: every empty
        # value (E1 on the given text) is filled, the question's cloze
        # references with the sample cloze, the others with "(" name ")".
        sample_q = 'This is a <span class="cloze" data-cloze="sample" data-ordinal="1">[...]</span> cloze deletion.'
        sample_a = 'This is a <span class="cloze" data-ordinal="1">sample</span> cloze deletion.'
        values = self.values([
            {"front": "{{Front}}", "back": "{{FrontSide}}|{{Back}}|{{Extra}}", "fields": ["neko"], "fill_empty": True},
            {"front": "{{Back}}", "fill_empty": True},
            {"front": "{{#Extra}}E{{Extra}}{{/Extra}}{{^Extra}}none{{/Extra}}", "fields": ["f"], "fill_empty": True},
            {"front": "{{cloze:Front}}|{{Back}}", "back": "{{cloze:Front}}", "fill_empty": True},
            {"front": "{{cloze-only:Front}}|{{Front}}", "fill_empty": True},
            {"front": "{{Front}}", "back": "{{cloze:Front}}", "fill_empty": True},
            {"front": "{{Front}}|{{Back}}", "fields": ["<br>", "&nbsp;"], "fill_empty": True},
            {"front": "{{Front}}", "back": "{{FrontSide}}|{{Back}}|{{Extra}}", "fields": ["neko"], "fill_empty": False},
            {"front": "{{Back}}"},
        ], {"name": "Probe", "fields": ["Front", "Back", "Extra"]})
        self.assertEqual(values, [
            ok("neko", "neko|(Back)|(Extra)"),
            ok("(Back)", ""),
            ok("E(Extra)", ""),
            ok(sample_q + "|(Back)", sample_a),
            ok("|(Front)", ""),
            # An answer-only cloze reference is no reference: "(Front)" holds
            # no cloze, so the cloze filter gives nothing (cloze.md C6).
            ok("(Front)", ""),
            ok("(Front)|&nbsp;", ""),
            ok("neko", "neko||"),
            blank(""),
        ])

    def test_fill_empty_on_a_card_case_leaves_the_stored_note(self):
        setup = {
            "decks": ["Lang"],
            "notetypes": [{"name": "Vocab", "fields": ["Word", "Meaning"],
                           "templates": [{"name": "Recognition", "front": "{{Word}}", "back": "{{Meaning}}"}]}],
            "notes": [{"notetype": "Vocab", "deck": "Lang", "fields": ["neko", ""]}],
        }
        steps = [
            {"observe": ["db_dump"]},
            {"observe": [request([
                {"card": "#c1", "front": "{{Word}}|{{Deck}}|{{Card}}", "back": "{{Meaning}}", "fill_empty": True},
                {"card": "#c1", "front": "{{Word}}", "back": "{{Meaning}}"},
            ], notetype=None)]},
            {"observe": ["db_dump"]},
        ]
        out = self.run_steps(steps, setup)
        self.assertEqual(out[1]["template"], [ok("neko|Lang|Recognition", "(Meaning)"), ok("neko", "")])
        self.assertEqual(out[2]["db_dump"], out[0]["db_dump"])

    def test_a_card_that_does_not_exist_yet_is_an_unknown_ref(self):
        with self.assertRaises(DriverError) as ctx:
            self.run_steps([{"observe": [request([{"card": "#c1", "front": "x"}], notetype=None)]}])
        self.assertEqual(ctx.exception.to_json(), {"kind": "unknown_ref", "ref": "#c1", "step": 0})

    def test_requests_outside_q1_are_invalid(self):
        good = {"front": "{{Front}}"}
        bad_requests = [
            {"template": None},
            {"template": {"notetype": PROBE}},
            {"template": {"notetype": PROBE, "cases": []}},
            {"template": {"notetype": PROBE, "cases": [good], "extra": 1}},
            {"template": {"cases": [good]}},
            {"template": {"notetype": PROBE, "cases": [{"card": "#c1", "front": "x"}]}},
            request([{"card": "#c1", "front": "x", "ord": 0}], notetype=None),
            request([{"card": "#n1", "front": "x"}], notetype=None),
            request([{"back": "x"}]),
            request([{"front": 1}]),
            request([{"front": "x", "back": None}]),
            request([{"front": "x", "ord": 500}]),
            request([{"front": "x", "ord": -1}]),
            request([{"front": "x", "ord": True}]),
            request([{"front": "x", "ord": 1.0}]),
            request([{"front": "x", "name": ""}]),
            request([{"front": "x", "fields": ["a", "b", "c"]}]),
            request([{"front": "x", "fields": {"front": "a"}}]),
            request([{"front": "x", "fields": [1]}]),
            request([{"front": "x", "tags": "a"}]),
            request([{"front": "x", "tags": [1]}]),
            request([{"front": "x", "unknown": 1}]),
            request([{"front": "x", "fill_empty": 1}]),
            request([{"front": "x", "fill_empty": "true"}]),
            request([{"front": "x", "fill_empty": None}]),
            request([{"card": "#c1", "front": "x", "fill_empty": 0}], notetype=None),
            request([good], {"name": "", "fields": ["F"]}),
            request([good], {"name": 'Pro"be', "fields": ["F"]}),
            request([good], {"name": "Cafe\u0301", "fields": ["F"]}),
            request([good], {"name": "P", "fields": []}),
            request([good], {"name": "P", "fields": ["F"], "kind": "image"}),
            request([good], {"name": "P", "fields": ["F"], "css": ""}),
        ]
        # Q1.1 field names: Unicode White_Space includes U+3000, U+2028 and U+0085.
        for name in ("", "a:b", "a{b", "a}b", 'a"b', "#a", "/a", "^a", " a", "a ", "\u3000a", "a\u2028",
                     "\u0085a", "e\u0301", 1):
            bad_requests.append(request([good], {"name": "P", "fields": [name]}))
        bad_requests += [
            request([good], {"name": "P", "fields": ["F", "F"]}),
            # unicase-equal names (text/unicode-primitives U2): a save would rename one.
            request([good], {"name": "P", "fields": ["Front", "front"]}),
            request([good], {"name": "P", "fields": ["Stra\u00dfe", "STRASSE"]}),
        ]
        for req in bad_requests:
            with self.subTest(req=req):
                self.assert_invalid([{"observe": [req]}])

    def test_one_template_request_per_observe_step(self):
        req = request([{"front": "{{Front}}", "fields": ["f"]}])
        self.assert_invalid([{"observe": [req, req]}])
        self.assert_invalid([{"observe": ["template"]}])
        out = self.run_steps([{"observe": [req]}, {"observe": [req]}])
        self.assertEqual([s["template"] for s in out], [[ok("f", "")], [ok("f", "")]])

    def test_scenario_checks_run_before_any_collection(self):
        bad = request([{"front": "x", "ord": 500}])
        with self.assertRaises(DriverError) as ctx:
            self.session([{"observe": ["ids"]}, {"observe": [bad]}])
        self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("invalid_scenario", 1))

    def test_run_errors(self):
        setup = {
            "notetypes": [{"name": "One", "fields": ["F"], "templates": [{"name": "T", "front": "{{F}}", "back": ""}]}],
            "notes": [{"notetype": "One", "fields": ["f"]}],
        }
        steps = [
            {"db_exec": {"sql": "UPDATE cards SET ord = 5"}},
            {"observe": [request([{"card": "#c1", "front": "{{F}}"}], notetype=None)]},
        ]
        # Q1.6: a card whose ordinal has no template is a run error.
        with self.assertRaises(DriverError) as ctx:
            self.run_steps(steps, setup)
        self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("anki_error", 1))
        # Q2: a message that is no kind of the list is a run error too.
        with mock.patch.object(template_module, "classify", return_value=None), self.assertRaises(DriverError) as ctx:
            self.values([{"front": "{{Nope}}", "fields": ["f"]}])
        self.assertEqual(ctx.exception.to_json(), {"kind": "anki_error", "op": "observe", "exception": "TemplateError", "step": 0})


class Classify(unittest.TestCase):
    """Messages as Anki lays them out: side header, escaped details, help link,
    all built from the backend's translations."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.col = Collection(str(Path(self.tmp.name) / "collection.anki2"))
        self.tr = self.col.tr

    def tearDown(self):
        self.col.close(downgrade=False)
        self.tmp.cleanup()

    def message(self, details, header=None):
        header = header or self.tr.card_template_rendering_back_side_problem()
        more = self.tr.card_template_rendering_more_info()
        return f"{header}<br>{html.escape(details)}<br><a href='https://example.invalid/'>{more}</a>"

    def test_kinds_and_sides(self):
        tr = self.tr
        cases = [
            (tr.card_template_rendering_conditional_not_closed(missing="{{/A}}"), "unclosed_conditional"),
            (tr.card_template_rendering_wrong_conditional_closed(found="{{/A}}", expected="{{/B}}"),
             "mismatched_conditional"),
            (tr.card_template_rendering_conditional_not_open(found="{{/A}}", missing1="{{#A}}", missing2="{{^A}}"),
             "unopened_conditional"),
            (tr.card_template_rendering_no_such_field(found="{{text:A}}", field="A"), "unknown_field"),
            (tr.card_template_rendering_no_such_field(found="{{^}}", field="^"), "unknown_field"),
            (tr.card_template_rendering_no_such_field(found="{{#A\n'<b>}}", field="A\n'<b>"), "unknown_conditional"),
        ]
        for details, kind in cases:
            with self.subTest(details=details):
                self.assertEqual(template_module.classify(self.tr, self.message(details)), (kind, "answer"))
        front = self.tr.card_template_rendering_front_side_problem()
        self.assertEqual(template_module.classify(self.tr, self.message(cases[0][0], front)),
                         ("unclosed_conditional", "question"))

    def test_messages_outside_the_list_are_not_classified(self):
        tr = self.tr
        brackets = tr.card_template_rendering_no_closing_brackets(missing="}}", tag="{{A")
        browser = tr.card_template_rendering_browser_front_side_problem()
        unknown = tr.card_template_rendering_no_such_field(found="{{A}}", field="A")
        self.assertIsNone(template_module.classify(tr, self.message(brackets)))
        self.assertIsNone(template_module.classify(tr, self.message(unknown, browser)))
        self.assertIsNone(template_module.classify(tr, self.message("something else")))
        self.assertIsNone(template_module.classify(tr, self.message(unknown).replace("<br><a", "<a")))


if __name__ == "__main__":
    unittest.main()
