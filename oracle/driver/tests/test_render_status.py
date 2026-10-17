# SPDX-License-Identifier: AGPL-3.0-or-later
"""The render_status observation (specs/conformance/render-ops.md Q4) through
real workers. Expected statuses come from Q4.2's table applied to the
scenario's own templates and fields (render/template-syntax T6-T10,
render/cloze C15), never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
PLAIN = {"name": "One", "front": "{{F}}", "back": "{{G}}"}
# Every table a step could write, with its mod column where it has one.
STATE = (
    "SELECT (SELECT mod FROM col), (SELECT count() FROM cards), (SELECT max(mod) FROM cards),"
    " (SELECT count() FROM notes), (SELECT max(mod) FROM notes), (SELECT max(mtime_secs) FROM notetypes),"
    " (SELECT count() FROM revlog), (SELECT count() FROM graves)"
)
DELETE_REVERSE = (
    "DELETE FROM templates WHERE ord = 1"
    " AND ntid = (SELECT id FROM notetypes WHERE name = 'Basic (and reversed card)')"
)


def nt(name):
    return {"add_notetype": {"name": name, "fields": ["F", "G"], "templates": [PLAIN]}}


def template(name, **text):
    return {"update_notetype": {"notetype": name, "edits": [{"template": {"name": "One", **text}}], "skip_checks": True}}


def status(ord_, value):
    return {"ord": ord_, "status": value}


class RenderStatus(unittest.TestCase):
    def test_each_status_of_q4_2_and_null_for_a_deleted_card_without_writing(self):
        steps = [
            nt("S"),
            {"add_note": {"notetype": "S", "fields": ["a", "b"]}},  # c1: ok
            {"add_note": {"notetype": "S", "fields": ["a", "b"]}},  # c2: blank question
            nt("Q"),
            {"add_note": {"notetype": "Q", "fields": ["a", "b"]}},  # c3: question error
            nt("R"),
            {"add_note": {"notetype": "R", "fields": ["a", "b"]}},  # c4: answer error
            {"add_note": {"notetype": "R", "fields": ["a", "b"]}},  # c5: blank question hides the answer error
            {"add_note": {"notetype": "Cloze", "fields": ["{{c1::x}} {{c2::y}}", ""]}},  # c6 ok, c7 blank cloze
            {"add_note": {"notetype": "S", "fields": ["d", "e"]}},  # c8: deleted
            template("Q", front="{{#F}}"),  # unclosed conditional (T6)
            template("R", back="{{/G}}"),  # unopened conditional (T6)
            {"edit_note": {"note": "#n2", "fields": ["", "b"]}},
            {"edit_note": {"note": "#n5", "fields": ["", "b"]}},
            {"edit_note": {"note": "#n6", "fields": ["{{c1::x}} y", ""]}},
            {"delete_notes": {"notes": ["#n7"]}},
            {"db_exec": {"sql": STATE}},
            {"observe": ["render_status"]},
            {"db_exec": {"sql": STATE}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/rs", "UTC", START, steps))
        self.assertEqual(out[-2], {"render_status": {
            "#c1": status(0, "ok"),
            "#c2": status(0, "empty"),
            "#c3": status(0, "template_error"),
            "#c4": status(0, "template_error"),
            "#c5": status(0, "empty"),
            "#c6": status(0, "ok"),
            "#c7": status(1, "empty"),
            "#c8": None,
        }})
        self.assertEqual(list(out[-2]["render_status"]), [f"#c{i}" for i in range(1, 9)])
        self.assertEqual(out[-1], out[-3])

    def test_requests_name_cards_and_merge_per_card(self):
        steps = [
            {"add_note": {"notetype": "Basic (and reversed card)", "fields": ["a", "b"]}},
            {"add_note": {"notetype": "Basic", "fields": ["c", "d"]}},
            {"observe": ["render_status(#c3, #c1)", {"render_status": ["#c2"]}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/rs", "UTC", START, steps))
            bad = {
                req: w.request({"cmd": "run", "scenario": scenario("t/rs", "UTC", START, steps[:2] + [{"observe": [req]}])})
                for req in ("render_status(#c1, #c1)", "render_status(#n1)", "render_status(#c4)")
            }
            twice = w.request({"cmd": "run", "scenario": scenario(
                "t/rs", "UTC", START, steps[:2] + [{"observe": ["render_status(#c1)", {"render_status": ["#c1"]}]}],
            )})
        self.assertEqual(out[2], {"render_status": {
            "#c3": status(0, "ok"), "#c1": status(0, "ok"), "#c2": status(1, "ok"),
        }})
        for req in ("render_status(#c1, #c1)", "render_status(#n1)"):
            self.assertEqual((bad[req]["error"]["kind"], bad[req]["error"].get("step")), ("invalid_scenario", 2), bad[req])
        self.assertEqual(bad["render_status(#c4)"]["error"], {"kind": "unknown_ref", "ref": "#c4", "step": 2})
        self.assertEqual((twice["error"]["kind"], twice["error"].get("step")), ("invalid_scenario", 2), twice)

    def test_cards_render_from_the_templates_the_file_holds(self):
        # The first observation loads the note type with both templates; the
        # db_exec removes the second from the file only. A card whose ordinal
        # has no template is a run error (Q4.4).
        steps = [
            {"add_note": {"notetype": "Basic (and reversed card)", "fields": ["a", "b"]}},
            {"observe": ["render_status"]},
            {"db_exec": {"sql": DELETE_REVERSE}},
            {"observe": ["render_status(#c1)"]},
            {"observe": ["render_status(#c2)"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/rs", "UTC", START, steps[:4]))
            failed = w.request({"cmd": "run", "scenario": scenario("t/rs", "UTC", START, steps)})
        self.assertEqual(out[1], {"render_status": {"#c1": status(0, "ok"), "#c2": status(1, "ok")}})
        self.assertEqual(out[3], {"render_status": {"#c1": status(0, "ok")}})
        self.assertEqual(failed["error"], {"kind": "anki_error", "op": "observe", "exception": "InvalidInput", "step": 4})


if __name__ == "__main__":
    unittest.main()
