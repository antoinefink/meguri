# SPDX-License-Identifier: AGPL-3.0-or-later
"""The render observation (specs/conformance/render-ops.md Q4) and the browser
formats of setup.notetypes (dsl.md D6.4): review and browser outputs, play
markers and AV entries, blank cards, template errors per mode, the TTS
normalization, deleted notes, request checks, and that rendering changes
nothing. Expected values come from the specs (render-ops.md Q4,
specs/render/card-render-pipeline.md K4-K7, av-tags.md A5-A8) and the
scenario's own setup, never from the driver."""

from __future__ import annotations

import html
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import anki.cards
import anki.template
from anki.collection import Collection

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.observe import template as template_module

PAIR = {
    "name": "Pair",
    "fields": ["Front", "Back", "Extra"],
    "templates": [
        {"name": "Forward", "front": "{{Front}}[sound:q.mp3]",
         "back": "{{FrontSide}}<hr id=answer>{{Back}}[sound:a.mp3]"},
        {"name": "Optional", "front": "Listen [sound:p.mp3]{{#Extra}}{{Extra}}{{/Extra}}",
         "back": "{{FrontSide}}|{{Back}}", "browser_front": "B:{{Front}}", "browser_back": "{{Back}}|{{FrontSide}}|"},
    ],
}
GAP = {
    "name": "Gap",
    "kind": "cloze",
    "fields": ["Text"],
    "templates": [{"name": "Gap Card", "front": "Q:{{cloze:Text}}", "back": "A:{{cloze:Text}}"}],
}


def rendered(question, answer, question_av=(), answer_av=()):
    return {"question": question, "answer": answer, "question_av": list(question_av),
            "answer_av": list(answer_av), "blank": None}


def blank(question, kind, question_av=()):
    return {"question": question, "answer": None, "question_av": list(question_av), "answer_av": [], "blank": kind}


def browser(question, answer, kind=None):
    return {"question": question, "answer": answer, "blank": kind}


def error(kind, side):
    return {"error": {"kind": kind, "side": side}}


def sound(name):
    return {"sound": name}


class RenderObservation(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, steps, setup=None):
        from driver.session import Session

        sc = {"id": "t/render", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        if setup is not None:
            sc["setup"] = setup
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def run_steps(self, steps, setup=None, inspect=None):
        s = self.session(steps, setup)
        try:
            out = s.run_all()["steps"]
            if inspect is not None:
                inspect(s)
            return out
        finally:
            s.close()

    def renders(self, notetypes, notes, steps=None, request="render"):
        setup = {"notetypes": notetypes, "notes": notes}
        steps = (steps or []) + [{"observe": [request]}]
        return self.run_steps(steps, setup)[-1]["render"]

    def assert_failure(self, kind, steps, setup=None):
        with self.assertRaises(DriverError) as ctx:
            self.run_steps(steps, setup)
        self.assertEqual(ctx.exception.kind, kind, ctx.exception.to_json())
        return ctx.exception

    # --- values ----------------------------------------------------------------

    def test_review_and_browser_outputs_of_a_rendered_card(self):
        out = self.renders([PAIR], [{"notetype": "Pair", "fields": ["sun", "Sonne", "x"]}])
        # K4: play markers numbered per side from 0, FrontSide = the question
        # text with its markers, the answer list holds only the answer's tags.
        # K5: no extraction, FrontSide empty, browser formats when not empty.
        self.assertEqual(out, {
            "#c1": {
                "review": rendered("sun[anki:play:q:0]", "sun[anki:play:q:0]<hr id=answer>Sonne[anki:play:a:0]",
                                   [sound("q.mp3")], [sound("a.mp3")]),
                "browser": browser("sun[sound:q.mp3]", "<hr id=answer>Sonne[sound:a.mp3]"),
            },
            "#c2": {
                "review": rendered("Listen [anki:play:q:0]x", "Listen [anki:play:q:0]x|Sonne", [sound("p.mp3")]),
                "browser": browser("B:sun", "Sonne||"),
            },
        })

    def test_blank_cards_in_review_and_browser_mode(self):
        out = self.renders(
            [PAIR, GAP],
            [{"notetype": "Pair", "fields": ["sun", "Sonne", "x"]}, {"notetype": "Gap", "fields": ["{{c1::a}} {{c2::b}}"]}],
            steps=[{"edit_note": {"note": "#n1", "fields": ["sun", "Sonne", ""]}},
                   {"edit_note": {"note": "#n2", "fields": ["{{c1::a}} b"]}}],
            request="render(#c2, #c4)",
        )
        # K4 step 2: a blank question keeps its text and audio, without the
        # notice; K5 step 3: standard cards are never blank in browser mode,
        # a missing cloze is blank in both (C9: no active cloze renders "").
        self.assertEqual(out, {
            "#c2": {"review": blank("Listen [anki:play:q:0]", "empty_front", [sound("p.mp3")]),
                    "browser": browser("B:sun", "Sonne||")},
            "#c4": {"review": blank("Q:", "missing_cloze"), "browser": browser("Q:", None, "missing_cloze")},
        })

    def test_sound_entries_are_decoded_names_and_tts_entries_keep_their_index(self):
        spoken = {
            "name": "Spoken",
            "fields": ["Front", "Back"],
            "templates": [{
                "name": "Card 1",
                "front": "{{tts en_US:Front}}|[anki:tts]{{Back}}[/anki:tts][sound:a&amp;b.mp3][sound:dir/clip.mp3]",
                "back": "{{FrontSide}}#[sound:z.mp3]",
            }],
        }
        out = self.renders([spoken], [{"notetype": "Spoken", "fields": ["sun", "Sonne"]}])
        # A6: a TTS directive with no language adds no entry, keeps no index,
        # and Anki's message is reported as Meguri's text (Q4.4), also inside
        # the answer's FrontSide; sound names are entity-decoded, kept whole.
        question = "[anki:play:q:0]|[tts: lang not set][anki:play:q:1][anki:play:q:2]"
        self.assertEqual(out["#c1"]["review"], rendered(
            question, question + "#[anki:play:a:0]",
            [{"tts": "en_US"}, sound("a&b.mp3"), sound("dir/clip.mp3")], [sound("z.mp3")],
        ))
        self.assertEqual(out["#c1"]["browser"], browser(
            "[anki:tts lang=en_US]sun[/anki:tts]|[anki:tts]Sonne[/anki:tts][sound:a&amp;b.mp3][sound:dir/clip.mp3]",
            "#[sound:z.mp3]",
        ))

    def test_template_errors_per_mode_and_side(self):
        faulty = {
            "name": "Faulty",
            "fields": ["Front", "Back"],
            "templates": [
                {"name": "Question Error", "front": "{{Front}}{{^FrontSide}}!{{/FrontSide}}", "back": "{{Back}}"},
                {"name": "Browser Question Error", "front": "{{Front}}", "back": "{{FrontSide}}|{{Back}}",
                 "browser_front": "{{#Front}}open"},
                {"name": "Browser Answer Error", "front": "<{{Front}}>", "back": "{{Back}}", "browser_back": "{{Nope}}"},
                {"name": "Answer Error", "front": "[{{Front}}]", "back": "{{FrontSide}}{{#FrontSide}}?{{/FrontSide}}"},
            ],
        }
        out = self.renders([faulty], [{"notetype": "Faulty", "fields": ["f", "b"]}])
        # K6 / T9: FrontSide is no field, so a section on it is
        # unknown_conditional; unchecked browser formats fail in browser mode only.
        self.assertEqual(out, {
            "#c1": {"review": error("unknown_conditional", "question"),
                    "browser": error("unknown_conditional", "question")},
            "#c2": {"review": rendered("f", "f|b"), "browser": error("unclosed_conditional", "question")},
            "#c3": {"review": rendered("<f>", "b"), "browser": error("unknown_field", "answer")},
            "#c4": {"review": error("unknown_conditional", "answer"),
                    "browser": error("unknown_conditional", "answer")},
        })

    def test_a_card_whose_note_was_deleted_is_null(self):
        out = self.renders(
            [PAIR], [{"notetype": "Pair", "fields": ["sun", "Sonne", "x"]}, {"notetype": "Pair", "fields": ["a", "b", ""]}],
            steps=[{"delete_notes": {"notes": ["#n1"]}}],
        )
        # Q4.2: null for a card that no longer exists; the others as usual.
        self.assertEqual(list(out), ["#c1", "#c2", "#c3"])
        self.assertIsNone(out["#c1"])
        self.assertIsNone(out["#c2"])
        self.assertEqual(out["#c3"]["review"]["question"], "a[anki:play:q:0]")

    def test_rendering_changes_nothing_and_leaves_latex_as_written(self):
        busy = {
            "name": "Busy",
            "fields": ["Front"],
            "templates": [{"name": "Busy", "front": "[latex]x^2[/latex]{{myaddon:Front}}{{tts en_US:Front}}",
                           "back": "{{FrontSide}}[$]y[/$]"}],
        }
        setup = {"notetypes": [busy], "notes": [{"notetype": "Busy", "fields": ["sun"]}]}
        steps = [{"observe": ["db_dump"]}, {"observe": ["render"]}, {"observe": ["db_dump"]}]
        media = []
        out = self.run_steps(steps, setup, inspect=lambda s: media.extend(os.listdir(s.col.media.dir())))
        # K7: no row and no media file changes; K8: LaTeX stays as written;
        # filters.md F2: an unknown filter leaves the field text.
        self.assertEqual(out[2]["db_dump"], out[0]["db_dump"])
        self.assertEqual(media, [])
        self.assertEqual(out[1]["render"]["#c1"]["review"], rendered(
            "[latex]x^2[/latex]sun[anki:play:q:0]", "[latex]x^2[/latex]sun[anki:play:q:0][$]y[/$]", [{"tts": "en_US"}],
        ))

    def test_pylib_card_rendering_is_never_called(self):
        # render-pipeline provenance: pylib's card rendering runs card_did_render
        # hooks (LaTeX images); the observation composes the pipeline itself.
        refuse = mock.Mock(side_effect=AssertionError("pylib card rendering was called"))
        with mock.patch.object(anki.cards.Card, "question", refuse), \
                mock.patch.object(anki.cards.Card, "answer", refuse), \
                mock.patch.object(anki.cards.Card, "render_output", refuse), \
                mock.patch.object(anki.template.TemplateRenderContext, "render", refuse):
            out = self.renders([PAIR], [{"notetype": "Pair", "fields": ["sun", "Sonne", "x"]}], request="render(#c1)")
        self.assertEqual(out["#c1"]["review"]["question"], "sun[anki:play:q:0]")

    # --- requests --------------------------------------------------------------

    def test_requests_name_cards_by_reference(self):
        setup = {"notetypes": [PAIR], "notes": [{"notetype": "Pair", "fields": ["sun", "Sonne", "x"]}]}
        out = self.run_steps([
            {"observe": ["render(#c2)"]},
            {"observe": [{"render": ["#c2", "#c1"]}]},
            {"observe": [{"render": "#c1"}, "render(#c2)"]},
        ], setup)
        self.assertEqual(list(out[0]["render"]), ["#c2"])
        self.assertEqual(list(out[1]["render"]), ["#c2", "#c1"])
        self.assertEqual(sorted(out[2]["render"]), ["#c1", "#c2"])
        self.assertEqual(out[1]["render"]["#c1"], out[2]["render"]["#c1"])

    def test_requests_outside_q4_1(self):
        setup = {"notetypes": [PAIR], "notes": [{"notetype": "Pair", "fields": ["sun", "Sonne", "x"]}]}
        for req in ({"render": {"card": "#c1"}}, {"render": {}}, {"render": [1]}, {"render": 1}, "render(#n1)",
                    "render(#c1, #c1)", {"render": ["#c1", "#c1"]}, "render(c1)"):
            with self.subTest(req=req):
                self.assert_failure("invalid_scenario", [{"observe": [req]}], setup)
        err = self.assert_failure("unknown_ref", [{"observe": ["render(#c3)"]}], setup)
        self.assertEqual(err.to_json(), {"kind": "unknown_ref", "ref": "#c3", "step": 0})

    def test_run_errors(self):
        setup = {"notetypes": [PAIR], "notes": [{"notetype": "Pair", "fields": ["sun", "Sonne", "x"]}]}
        # Q4.5: a card whose ordinal has no template cannot be rendered.
        err = self.assert_failure("anki_error", [{"db_exec": {"sql": "UPDATE cards SET ord = 5"}}, {"observe": ["render"]}],
                                  setup)
        self.assertEqual(err.fields.get("step"), 1)
        # Q4.5: a template error whose message is no kind of Q2.
        faulty = {"name": "F", "fields": ["Front"],
                  "templates": [{"name": "T", "front": "{{Front}}", "back": "", "browser_front": "{{Nope}}"}]}
        with mock.patch.object(template_module, "classify", return_value=None):
            err = self.assert_failure("anki_error", [{"observe": ["render"]}],
                                      {"notetypes": [faulty], "notes": [{"notetype": "F", "fields": ["f"]}]})
        self.assertEqual(err.to_json(), {"kind": "anki_error", "op": "observe", "exception": "TemplateError", "step": 0})


class BrowserFormats(unittest.TestCase):
    """dsl.md D6.4: browser_front and browser_back are stored as bqfmt and bafmt."""

    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, setup, steps):
        from driver.session import Session

        sc = {"id": "t/browser-formats", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "setup": setup,
              "steps": steps}
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def test_formats_are_stored_and_default_to_empty(self):
        s = self.session({"notetypes": [PAIR]}, [{"observe": [{"legacy_json": {"notetypes": ["Pair"]}}]}])
        try:
            tmpls = s.run_all()["steps"][0]["legacy_json"]["notetypes"]["Pair"]["tmpls"]
        finally:
            s.close()
        self.assertEqual([(t["bqfmt"], t["bafmt"]) for t in tmpls], [("", ""), ("B:{{Front}}", "{{Back}}|{{FrontSide}}|")])

    def test_wrong_types_are_refused_before_any_collection(self):
        for key, value in (("browser_front", 1), ("browser_back", None), ("browser_front", ["x"]), ("bqfmt", "x")):
            tmpl = {"name": "T", "front": "{{F}}", key: value}
            with self.subTest(key=key, value=value), self.assertRaises(DriverError) as ctx, \
                    mock.patch.object(Collection, "__init__", side_effect=AssertionError("a collection was made")):
                self.session({"notetypes": [{"name": "N", "fields": ["F"], "templates": [tmpl]}]}, [])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")


class ClassifyBrowser(unittest.TestCase):
    """Browser mode's errors carry the browser headers (render-ops.md Q4.2)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.col = Collection(str(Path(self.tmp.name) / "collection.anki2"))
        self.tr = self.col.tr

    def tearDown(self):
        self.col.close(downgrade=False)
        self.tmp.cleanup()

    def message(self, header):
        details = self.tr.card_template_rendering_no_such_field(found="{{A}}", field="A")
        more = self.tr.card_template_rendering_more_info()
        return f"{header}<br>{html.escape(details)}<br><a href='https://example.invalid/'>{more}</a>"

    def test_sides_by_mode(self):
        tr = self.tr
        browser_front = self.message(tr.card_template_rendering_browser_front_side_problem())
        browser_back = self.message(tr.card_template_rendering_browser_back_side_problem())
        review_front = self.message(tr.card_template_rendering_front_side_problem())
        self.assertEqual(template_module.classify(tr, browser_front, browser=True), ("unknown_field", "question"))
        self.assertEqual(template_module.classify(tr, browser_back, browser=True), ("unknown_field", "answer"))
        self.assertIsNone(template_module.classify(tr, review_front, browser=True))
        self.assertIsNone(template_module.classify(tr, browser_front))


if __name__ == "__main__":
    unittest.main()
