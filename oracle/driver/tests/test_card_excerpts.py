# SPDX-License-Identifier: AGPL-3.0-or-later
"""The card_excerpts observation (specs/conformance/card-excerpt-ops.md E1-E3)
through real workers. Expected cells come from the examples of
specs/browser/columns.md C7.4 and C10 and the row errors of C3.1, applied to
the scenario's own fields and decks, never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
PAIR = {
    "name": "ExcerptFixture",
    "fields": ["Prompt", "Response"],
    "templates": [{"name": "Excerpt 1", "front": "{{Prompt}}", "back": "{{Response}}"}],
}
REVERSED = "Basic (and reversed card)"


def nth_note(offset: int) -> str:
    return f"(SELECT id FROM notes ORDER BY id LIMIT 1 OFFSET {offset})"


def to_filtered(offset: int) -> dict:
    """E5: the card of the note at this offset moved into 'ExcerptDeckStage', its
    home deck and due kept in odid and odue."""
    return {"db_exec": {"sql": (
        "UPDATE cards SET odid = did, odue = due, did = (SELECT id FROM decks WHERE name = 'ExcerptDeckStage')"
        f" WHERE nid = {nth_note(offset)}"
    )}}


def found(question: str, deck: str) -> dict:
    return {"question": question, "deck": deck}


MISSING = {"error": "missing"}
DAMAGED = {"error": "damaged"}


def excerpts_scenario(steps, notes, decks=("ExcerptDeck::Nested", "ExcerptDeckStage")):
    return scenario("t/ce", "UTC", START, steps, setup={"decks": list(decks), "notetypes": [PAIR], "notes": notes})


class CardExcerpts(unittest.TestCase):
    def test_one_entry_per_reference_in_request_order(self):
        notes = [
            {"notetype": "ExcerptFixture", "deck": "ExcerptDeck::Nested", "fields": ["<b>猫</b><br>[sound:excerpt301.mp3]", "A 501"]},
            {"notetype": "ExcerptFixture", "deck": "ExcerptDeck::Nested", "fields": ['<div>犬</div><img src="excerpt302.jpg">', "A 502"]},
            {"notetype": "ExcerptFixture", "fields": ["Q 301", "A 301"]},
            {"notetype": "ExcerptFixture", "fields": ["Q 302", "A 302"]},
            {"notetype": "ExcerptFixture", "fields": ["ねこ [anki:tts lang=ja_JP]猫[/anki:tts] &amp; 犬", "A 503"]},
        ]
        steps = [
            to_filtered(1),
            {"db_exec": {"sql": f"UPDATE cards SET did = 9999 WHERE nid = {nth_note(3)}"}},
            {"delete_notes": {"notes": ["#n3"]}},
            {"observe": [{"card_excerpts": ["#c5", "#c3", "#c1", "#c4", "#c2", "#c1"]}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, excerpts_scenario(steps, notes))
        self.assertEqual(out[-1], {"card_excerpts": [
            found("ねこ 💬猫💬 & 犬", "Default"),
            MISSING,
            found("猫 🔉excerpt301.mp3🔉", "ExcerptDeck::Nested"),
            DAMAGED,
            found("犬 excerpt302.jpg", "ExcerptDeckStage (ExcerptDeck::Nested)"),
            found("猫 🔉excerpt301.mp3🔉", "ExcerptDeck::Nested"),
        ]})

    def test_browser_formats_and_cloze_cards(self):
        steps = [
            {"add_notetype": {"name": "ExcerptVariants", "fields": ["Prompt", "Response", "Context"], "templates": [
                {"name": "ExcerptBrowser", "front": "{{Prompt}}", "back": "{{Response}}"},
                {"name": "ExcerptReview", "front": "<div>{{Prompt}}</div>{{Context}}", "back": "{{Response}}"},
                {"name": "ExcerptBlank", "front": "{{Prompt}}?", "back": "{{Response}}"},
            ]}},
            {"add_note": {"notetype": "ExcerptVariants", "fields": ["Q 401", "A 401", ""]}},
            {"update_notetype": {"notetype": "ExcerptVariants", "edits": [
                {"template": {"name": "ExcerptBrowser", "browser_front": "{{Response}} ({{Prompt}})"}},
                {"template": {"name": "ExcerptBlank", "browser_front": "{{Context}}"}},
            ]}},
            {"add_note": {"notetype": "Cloze", "fields": ["{{c1::犬}} と {{c2::猫::ねこ}}<br>[sound:excerpt301.mp3]", ""]}},
            {"observe": [{"card_excerpts": ["#c1", "#c2", "#c3", "#c5"]}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, excerpts_scenario(steps, []))
        self.assertEqual(out[-1], {"card_excerpts": [
            found("A 401 (Q 401)", "Default"),
            found("Q 401", "Default"),
            found("", "Default"),
            found("犬 と [ねこ] 🔉excerpt301.mp3🔉", "Default"),
        ]})

    def test_every_damaged_record_of_c3_1_and_the_other_rows_unaffected(self):
        # The note type and template removals reopen, so that Anki reads the
        # file rather than its note type cache (card-excerpt-ops.md E5).
        notes = [
            {"notetype": "ExcerptFixture", "fields": ["Q 301", "A 301"]},  # c1: home deck missing
            {"notetype": "ExcerptFixture", "fields": ["Q 302", "A 302"]},  # c2: note row missing
            {"notetype": REVERSED, "fields": ["Q 303", "A 303"]},  # c3 ok, c4: template missing
            {"notetype": "ExcerptSingle", "fields": ["Q 304", "A 304"]},  # c5: note type missing
            {"notetype": "ExcerptFixture", "fields": ["Q 305", "A 305"]},  # c6 ok
        ]
        lone = {**PAIR, "name": "ExcerptSingle"}
        steps = [
            {"db_exec": {"sql": f"UPDATE cards SET odid = 9999, odue = due WHERE nid = {nth_note(0)}"}},
            {"db_exec": {"sql": f"DELETE FROM notes WHERE id = {nth_note(1)}"}},
            {"db_exec": {"sql": (
                f"DELETE FROM templates WHERE ord = 1 AND ntid = (SELECT id FROM notetypes WHERE name = '{REVERSED}')"
            )}},
            {"db_exec": {"sql": "DELETE FROM notetypes WHERE name = 'ExcerptSingle'"}},
            {"reopen": {}},
            {"observe": [{"card_excerpts": ["#c1", "#c6", "#c2", "#c3", "#c4", "#c5", "#c6"]}]},
        ]
        sc = scenario("t/ce", "UTC", START, steps, setup={"decks": [], "notetypes": [PAIR, lone], "notes": notes})
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc)
        self.assertEqual(out[-1], {"card_excerpts": [
            DAMAGED, found("Q 305", "Default"), DAMAGED, found("Q 303", "Default"), DAMAGED, DAMAGED,
            found("Q 305", "Default"),
        ]})

    def test_any_other_row_failure_is_a_run_error(self):
        # A note type whose config blob does not decode: Anki's protobuf error,
        # neither missing nor damaged.
        notes = [{"notetype": "ExcerptFixture", "fields": ["Q 301", "A 301"]}]
        steps = [
            {"db_exec": {"sql": "UPDATE notetypes SET config = x'ff' WHERE name = 'ExcerptFixture'"}},
            {"reopen": {}},
            {"observe": [{"card_excerpts": ["#c1"]}]},
        ]
        with WorkerProcess("UTC") as w:
            resp = w.request({"cmd": "run", "scenario": excerpts_scenario(steps, notes)})
        self.assertFalse(resp["ok"], resp)
        self.assertEqual(resp["error"], {"kind": "anki_error", "op": "observe", "exception": "BackendError", "step": 2})

    def test_notes_mode_is_invalid_scenario(self):
        notes = [{"notetype": "ExcerptFixture", "fields": ["Q 301", "A 301"]}]
        off = [
            {"set_config": {"key": "browserTableShowNotesMode", "value": False}},
            {"observe": [{"card_excerpts": ["#c1"]}]},
        ]
        on = [
            {"set_config": {"key": "browserTableShowNotesMode", "value": True}},
            {"observe": [{"card_excerpts": ["#c1"]}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, excerpts_scenario(off, notes))
            refused = w.request({"cmd": "run", "scenario": excerpts_scenario(on, notes)})
        self.assertEqual(out[-1], {"card_excerpts": [found("Q 301", "Default")]})
        self.assertEqual((refused["error"]["kind"], refused["error"].get("step")), ("invalid_scenario", 1), refused)

    def test_requests_outside_e1(self):
        notes = [{"notetype": "ExcerptFixture", "fields": ["Q 301", "A 301"]}]
        invalid = {
            "empty list": [{"card_excerpts": []}],
            "note reference": [{"card_excerpts": ["#n1"]}],
            "mapping": [{"card_excerpts": {"cards": ["#c1"]}}],
            "one reference": [{"card_excerpts": "#c1"}],
            "not a string": [{"card_excerpts": [1]}],
            "text form": ["card_excerpts(#c1)"],
            "bare text form": ["card_excerpts"],
            "twice in a step": [{"card_excerpts": ["#c1"]}, {"card_excerpts": ["#c1"]}],
        }
        with WorkerProcess("UTC") as w:
            refused = {
                name: w.request({"cmd": "run", "scenario": excerpts_scenario(
                    [{"add_note": {"notetype": "ExcerptFixture", "fields": ["Q 302", "A 302"]}}, {"observe": request}], notes,
                )})
                for name, request in invalid.items()
            }
            not_yet = w.request({"cmd": "run", "scenario": excerpts_scenario([
                {"observe": [{"card_excerpts": ["#c1", "#c2"]}]},
                {"add_note": {"notetype": "ExcerptFixture", "fields": ["Q 302", "A 302"]}},
            ], notes)})
        for name, resp in refused.items():
            self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), (name, resp))
        self.assertEqual(not_yet["error"], {"kind": "unknown_ref", "ref": "#c2", "step": 0})

    def test_the_collection_does_not_change(self):
        # Timing first, as specs/sched/day-clock.md D14.2 says, then a later
        # instant, so that any write by the observation would show in the dump.
        notes = [
            {"notetype": "ExcerptFixture", "deck": "ExcerptDeck::Nested", "fields": ["猫 [sound:excerpt301.mp3]", "A 501"]},
            {"notetype": "ExcerptFixture", "deck": "ExcerptDeck::Nested", "fields": ["犬", "A 502"]},
        ]
        steps = [
            {"update_notetype": {"notetype": "ExcerptFixture", "edits": [
                {"template": {"name": "Excerpt 1", "browser_front": "{{Prompt}} / {{Response}}"}},
            ]}},
            to_filtered(1),
            {"observe": ["timing"]},
            {"advance": "1h"},
            {"observe": ["db_dump"]},
            {"observe": [{"card_excerpts": ["#c1", "#c2", "#c1"]}]},
            {"observe": ["db_dump"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, excerpts_scenario(steps, notes))
        self.assertEqual(out[-2], {"card_excerpts": [
            found("猫 🔉excerpt301.mp3🔉 / cat", "ExcerptDeck::Nested"),
            found("犬 / dog", "ExcerptDeckStage (ExcerptDeck::Nested)"),
            found("猫 🔉excerpt301.mp3🔉 / cat", "ExcerptDeck::Nested"),
        ]})
        self.assertEqual(out[-1], out[-3])


if __name__ == "__main__":
    unittest.main()
