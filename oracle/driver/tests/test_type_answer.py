# SPDX-License-Identifier: AGPL-3.0-or-later
"""The type_answer and review_html observations (specs/conformance/
type-answer-ops.md K1-K3): values, request checks, unknown cards, and that
neither changes the scenario's collection. Expected strings are the examples
of specs/render/type-answer.md (A4.4, A5, A7, A8) and
specs/render/media-display.md (D3), never the driver's output."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.observe import type_answer as type_answer_module
from driver.session import Session

ARROW = "<br><span id=typearrow>&darr;</span><br>"
BOX = (
    "\n<center>\n"
    '<input type=text id=typeans onkeypress="_typeAnsPress();"\n'
    "   style=\"font-family: '{font}'; font-size: {size}px;\">\n"
    "</center>\n"
)


def code(*parts: str) -> str:
    return "<code id=typeans>" + "".join(parts) + "</code>"


def span(cls: str, text: str) -> str:
    return f"<span class={cls}>{text}</span>"


def good(text: str) -> str:
    return span("typeGood", text)


def bad(text: str) -> str:
    return span("typeBad", text)


def missed(text: str) -> str:
    return span("typeMissed", text)


def box(font: str = "Arial", size: int = 20) -> str:
    return BOX.format(font=font, size=size)


def compared(comparison: str, font: str = "Arial", size: int = 20) -> str:
    """A5's B: the comparison in its font div."""
    return f"\n<div style=\"font-family: '{font}'; font-size: {size}px\">{comparison}</div>"


def field_fonts(notetype: str, fonts: list[tuple[str, str, int]]) -> dict:
    """A legacy_update step giving each field its font and size."""
    flds = [
        {"name": name, "ord": i, "sticky": False, "rtl": False, "font": font, "size": size}
        for i, (name, font, size) in enumerate(fonts)
    ]
    return {"legacy_update": {"notetype": notetype, "patch": {"flds": flds}}}


class Base(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        # The scratch collection lives for the worker process; here it lives in
        # this test's directory, so close it before that goes.
        if type_answer_module._scratch is not None:
            type_answer_module._scratch.col.close()
            type_answer_module._scratch = None
        self.tmp.cleanup()

    def session(self, steps, setup=None):
        sc = {"id": "t/type-answer", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        if setup is not None:
            sc["setup"] = setup
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def run_steps(self, steps, setup=None):
        s = self.session(steps, setup)
        try:
            return s.run_all()["steps"]
        finally:
            s.close()

    def assert_invalid(self, steps, setup=None):
        with self.assertRaises(DriverError) as ctx:
            self.run_steps(steps, setup)
        self.assertEqual(ctx.exception.kind, "invalid_scenario", ctx.exception.to_json())


class TypeAnswerObservation(Base):
    def test_values_follow_a7(self):
        inputs = [["ka", "ka"], ["ka", "kq"], ["ka", ""], ["qávul", "qavul"]]
        steps = [
            {"observe": [{"type_answer": {"fns": ["compare_nc", "compare"], "inputs": inputs}}]},
            {"observe": [{"type_answer": {"fns": ["compare"], "inputs": []}}]},
        ]
        out = self.run_steps(steps)
        kq = code(good("k"), bad("q"), ARROW, good("k"), missed("a"))
        self.assertEqual(out[0]["type_answer"], {
            "compare_nc": [code(good("ka")), kq, code("ka"), code(good("qávul"))],
            "compare": [
                code(good("ka")),
                kq,
                code("ka"),
                code(good("q"), bad("a"), good("vul"), ARROW,
                     good("q"), missed("á"), good("vul")),
            ],
        })
        self.assertEqual(list(out[0]["type_answer"]), ["compare_nc", "compare"])
        self.assertEqual(out[1]["type_answer"], {"compare": []})

    def test_the_scenarios_collection_is_never_touched(self):
        setup = {
            "notetypes": [{"name": "One", "fields": ["F"], "templates": [{"name": "T", "front": "{{F}}", "back": ""}]}],
            "notes": [{"notetype": "One", "fields": ["f"]}],
        }
        req = {"type_answer": {"fns": ["compare"], "inputs": [["a", "b"]]}}
        out = self.run_steps([{"observe": ["db_dump"]}, {"observe": [req]}, {"observe": ["db_dump"]}], setup)
        self.assertEqual(out[2]["db_dump"], out[0]["db_dump"])

    def test_requests_outside_k1_are_invalid(self):
        bad_requests = [
            None,
            [],
            {"fns": ["compare"]},
            {"inputs": []},
            {"fns": ["compare"], "inputs": [], "extra": 1},
            {"fns": [], "inputs": []},
            {"fns": "compare", "inputs": []},
            {"fns": ["compare", "compare"], "inputs": []},
            {"fns": ["ci"], "inputs": []},
            {"fns": [1], "inputs": []},
            {"fns": ["compare"], "inputs": {}},
            {"fns": ["compare"], "inputs": ["a"]},
            {"fns": ["compare"], "inputs": [["a"]]},
            {"fns": ["compare"], "inputs": [["a", "b", "c"]]},
            {"fns": ["compare"], "inputs": [["a", None]]},
            {"fns": ["compare"], "inputs": [[1, "b"]]},
        ]
        for req in bad_requests:
            with self.subTest(req=req):
                self.assert_invalid([{"observe": [{"type_answer": req}]}])


class ReviewHtmlObservation(Base):
    def test_question_box_follows_a4_4(self):
        setup = {
            "notetypes": [{
                "name": "Typing", "fields": ["Front", "Back"], "css": ".card { text-align: center; }",
                "templates": [{"name": "Basic", "front": "{{Front}}<br>{{type:Back}}", "back": "{{FrontSide}}"}],
            }],
            "notes": [{"notetype": "Typing", "fields": ["qav", "qavrul"]}],
        }
        steps = [
            {"observe": [{"review_html": [{"card": "#c1"}]}]},
            field_fonts("Typing", [("Front", "Arial", 20), ("Back", "Qav Serif", 28)]),
            {"observe": [{"review_html": [{"card": "#c1", "typed": "qavrul"}]}]},
        ]
        out = self.run_steps(steps, setup)
        style = "<style>.card { text-align: center; }</style>"
        # A new field gets Arial and 20 (A1).
        self.assertEqual(out[0]["review_html"], [{
            "question": f"{style}qav<br>{box()}",
            "answer": f"{style}qav<br>{compared(code('qavrul'))}",
        }])
        # A4.4's example; on the answer every marker becomes B (A5 rule 2.1).
        self.assertEqual(out[2]["review_html"], [{
            "question": f"{style}qav<br>{box('Qav Serif', 28)}",
            "answer": f"{style}qav<br>{compared(code(good('qavrul')), 'Qav Serif', 28)}",
        }])

    def test_answer_rules_follow_a5(self):
        backs = [
            "{{FrontSide}}<hr id=answer>{{Capital}}",
            "{{Country}}<hr id=answer>{{FrontSide}}",
            "{{FrontSide}}<br>{{Capital}}",
            '{{FrontSide}}<hr id="answer">{{Capital}}',
            "{{Country}}<hr id=answer>{{Capital}}",
            "{{Country}}<hr id=answer>{{type:Country}}",
        ]
        setup = {
            "notetypes": [{
                "name": "Answers", "fields": ["Country", "Capital"],
                "templates": [
                    {"name": f"T{n}", "front": f"{n}. {{{{Country}}}} {{{{type:Capital}}}}", "back": back}
                    for n, back in enumerate(backs, 1)
                ],
            }],
            "notes": [{"notetype": "Answers", "fields": ["Spain", "Madrid"]}],
        }
        cases = [{"card": f"#c{n}", "typed": "Madrid"} for n in range(1, len(backs) + 1)]
        cases.append({"card": "#c1"})
        out = self.run_steps([{"observe": [{"review_html": cases}]}], setup)
        b = compared(code(good("Madrid")))
        self.assertEqual([v["answer"] for v in out[0]["review_html"]], [
            f"<style></style>1. Spain <hr id=answer>{b}Madrid",
            f"<style></style>Spain2. Spain <hr id=answer>{b}",
            f"<style></style>3. Spain {b}<br>Madrid",
            f'<style></style>4. Spain {b}<hr id="answer">Madrid',
            "<style></style>Spain<hr id=answer>Madrid",
            f"<style></style>Spain<hr id=answer>{b}",
            # A6.2: nothing typed (the default) shows the expected text.
            f"<style></style>1. Spain <hr id=answer>{compared(code('Madrid'))}Madrid",
        ])
        self.assertEqual(out[0]["review_html"][0]["question"], f"<style></style>1. Spain {box()}")

    def test_empty_and_missing_fields_follow_a4_3_and_a8(self):
        setup = {
            "notetypes": [
                {"name": "Typing", "fields": ["Front", "Back"],
                 "templates": [
                     {"name": "Basic", "front": "{{Front}}{{type:Back}}", "back": "{{FrontSide}}"},
                     {"name": "Special", "front": "{{Front}}{{type:Tags}}", "back": "{{FrontSide}}"},
                 ]},
                {"name": "Gaps", "kind": "cloze", "fields": ["Text", "Extra"],
                 "templates": [{"name": "Gap", "front": "{{cloze:Text}}{{type:cloze:Extra}}",
                                "back": "{{cloze:Text}}<hr id=answer>{{type:cloze:Extra}}"}]},
            ],
            "notes": [
                # #c1, #c2
                {"notetype": "Typing", "fields": ["qav", "qavrul"]},
                # #c3, #c4: Back is empty
                {"notetype": "Typing", "fields": ["zex", ""]},
                # #c5, #c6: Back holds only a line break, which is not empty (A4.3 rule 2)
                {"notetype": "Typing", "fields": ["vux", "<br>"]},
                # #c7: cloze 1 of Text; Extra holds no cloze for it
                {"notetype": "Gaps", "fields": ["{{c1::x}}", "plain"]},
            ],
        }
        cases = [{"card": "#c2"}, {"card": "#c3", "typed": "zex"}, {"card": "#c5", "typed": "x"}]
        out = self.run_steps([{"observe": [{"review_html": cases}]}], setup)
        values = out[0]["review_html"]
        # A8: the unknown-field token for a special name; no type-in on the answer.
        self.assertEqual(values[0], {
            "question": "<style></style>qav[[warning:unknown_field:Tags]]",
            "answer": "<style></style>qav",
        })
        # A4.3 rule 2: an empty field removes the markers on both sides.
        self.assertEqual(values[1], {"question": "<style></style>zex", "answer": "<style></style>zex"})
        # A4.3 rule 2: <br> gives the box, and the comparison expects "" (A7 row `<br>`/`x`).
        self.assertEqual(values[2], {
            "question": f"<style></style>vux{box()}",
            "answer": f"<style></style>vux{compared(code(bad('x'), ARROW))}",
        })
        # The cloze warning token when the extraction is empty (A4.3 rule 1).
        cloze = self.run_steps([{"observe": [{"review_html": [{"card": "#c7"}]}]}], setup)[0]["review_html"][0]
        self.assertTrue(cloze["question"].endswith("[[warning:empty_cards]]"), cloze["question"])
        self.assertNotIn("[[type:", cloze["answer"])
        self.assertNotIn("typeans", cloze["answer"])

    def test_media_names_are_encoded_once_per_side(self):
        setup = {
            "notetypes": [{
                "name": "Pictures", "fields": ["Word", "Image", "File", "Audio"],
                "templates": [{
                    "name": "Picture",
                    "front": '{{Word}}<br>{{Image}}<img src="{{File}}"><img src="_shared logo.png">{{Audio}}',
                    "back": '{{FrontSide}}<hr id=answer><img src="https://example.org/remote pic.png">',
                }],
            }],
            "notes": [{"notetype": "Pictures", "fields": [
                "le chat", "", "qav photo 3.jpg", '[sound:qav sound.mp3]<audio src="meow 2.ogg" controls></audio>',
            ]}],
        }
        out = self.run_steps([{"observe": [{"review_html": [{"card": "#c1"}]}]}], setup)
        # D3's whole-card example.
        front = ('<style></style>le chat<br><img src="qav%20photo%203.jpg"><img src="_shared%20logo.png">'
                 '[anki:play:q:0]<audio src="meow%202.ogg" controls></audio>')
        self.assertEqual(out[0]["review_html"], [{
            "question": front,
            "answer": front + '<hr id=answer><img src="https://example.org/remote pic.png">',
        }])

    def test_cases_are_independent_and_the_collection_never_changes(self):
        setup = {
            "notetypes": [{"name": "Typing", "fields": ["Front", "Back"],
                           "templates": [{"name": "Basic", "front": "{{Front}}{{type:Back}}", "back": "{{FrontSide}}"},
                                         {"name": "Plain", "front": "{{Front}}", "back": "{{Back}}"}]}],
            "notes": [{"notetype": "Typing", "fields": ["qav", "qavrul"]}],
        }
        cases = [{"card": "#c1", "typed": "qavrul"}, {"card": "#c2", "typed": "qavrul"}, {"card": "#c1", "typed": "qavr"}]
        steps = [
            {"observe": ["db_dump"]},
            {"observe": [{"review_html": cases}]},
            {"observe": ["db_dump"]},
            {"edit_note": {"note": "#n1", "fields": {"Back": "zexul"}}},
            {"observe": [{"review_html": [{"card": "#c1", "typed": "zexul"}]}]},
        ]
        out = self.run_steps(steps, setup)
        self.assertEqual(out[2]["db_dump"], out[0]["db_dump"])
        values = out[1]["review_html"]
        self.assertEqual(values[0]["answer"], f"<style></style>qav{compared(code(good('qavrul')))}")
        # A card without a type-in: the typed text plays no part.
        self.assertEqual(values[1], {"question": "<style></style>qav", "answer": "<style></style>qavrul"})
        self.assertEqual(values[2]["answer"],
                         f"<style></style>qav{compared(code(good('qavr'), missed('--'), ARROW, good('qavr'), missed('ul')))}")
        # The card is read as stored when the step runs.
        self.assertEqual(out[4]["review_html"][0]["answer"], f"<style></style>qav{compared(code(good('zexul')))}")

    def test_replacement_escapes_and_refusal_preserve_rows(self):
        setup = {
            "notetypes": [{"name": "Escape", "fields": ["F"],
                           "templates": [{"name": "Side", "front": "{{type:F}}", "back": "{{FrontSide}}"}]}],
            "notes": [{"notetype": "Escape", "fields": ["qav"]}],
        }
        steps = [field_fonts("Escape", [("F", r"Qav\nVex", 20)]),
                 {"observe": [{"review_html": [{"card": "#c1", "typed": "qav"}]}]},
                 field_fonts("Escape", [("F", r"Qav\q", 20)]),
                 {"observe": ["db_dump"]},
                 {"observe": [{"review_html": [{"card": "#c1", "typed": "qav"}]}]},
                 {"observe": ["db_dump"]}]
        out = self.run_steps(steps, setup)
        self.assertIn("font-family: 'Qav\nVex'", out[1]["review_html"][0]["question"])
        self.assertIn(r"font-family: 'Qav\nVex'", out[1]["review_html"][0]["answer"])
        self.assertEqual(out[4]["review_html"], [{"error": "invalid_type_answer_replacement"}])
        self.assertEqual(out[3]["db_dump"], out[5]["db_dump"])

    def test_a_card_that_does_not_exist_yet_is_an_unknown_ref(self):
        with self.assertRaises(DriverError) as ctx:
            self.run_steps([{"observe": [{"review_html": [{"card": "#c1"}]}]}])
        self.assertEqual(ctx.exception.to_json(), {"kind": "unknown_ref", "ref": "#c1", "step": 0})

    def test_requests_outside_k2_are_invalid(self):
        bad_requests = [
            None,
            {},
            [],
            ["#c1"],
            [{}],
            [{"typed": "x"}],
            [{"card": "#c1", "typed": None}],
            [{"card": "#c1", "typed": 1}],
            [{"card": "#c1", "extra": "x"}],
            [{"card": "#n1"}],
            [{"card": "c1"}],
            [{"card": 1}],
        ]
        for req in bad_requests:
            with self.subTest(req=req):
                self.assert_invalid([{"observe": [{"review_html": req}]}])
        self.assert_invalid([{"observe": ["review_html"]}])
        self.assert_invalid([{"observe": ["review_html(#c1)"]}])

    def test_one_review_html_request_per_observe_step_checked_before_any_collection(self):
        req = {"review_html": [{"card": "#c1"}]}
        with self.assertRaises(DriverError) as ctx:
            self.session([{"observe": ["ids"]}, {"observe": [req, req]}])
        self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("invalid_scenario", 1))
        with self.assertRaises(DriverError) as ctx:
            self.session([{"observe": ["ids"]}, {"observe": [{"review_html": []}]}])
        self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("invalid_scenario", 1))


if __name__ == "__main__":
    unittest.main()
