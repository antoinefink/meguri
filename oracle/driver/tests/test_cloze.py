# SPDX-License-Identifier: AGPL-3.0-or-later
"""The cloze observation (specs/conformance/cloze-ops.md): request checks, value
shape, the sentinel against the blank-front message, and that it never touches
the scenario's collection. Expected strings are specs/render/cloze.md's
examples."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.observe import cloze as cloze_module
from driver.session import Session

PARIS = "{{c1::Paris::capital}} is in {{c2::France}}"


class ClozeObservation(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        # The scratch collection lives for the worker process; here it lives in
        # this test's directory, so close it before that goes.
        if cloze_module._scratch is not None:
            cloze_module._scratch.col.close()
            cloze_module._scratch = None
        self.tmp.cleanup()

    def run_steps(self, steps):
        sc = {"id": "t/cloze", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        s = Session(sc, "UTC", self.tmp.name, self.clock)
        try:
            out = s.run_all()["steps"]
            counts = [s.col.db.scalar(f"select count() from {t}") for t in ("notes", "cards", "notetypes")]
        finally:
            s.close()
        return out, counts

    def test_values_are_keyed_by_function_in_input_order(self):
        req = {"cloze": {"fns": ["typing", "front", "back", "only_front", "only_back"], "inputs": [[PARIS, 1], [PARIS, 3]]}}
        nums = {"observe": [{"cloze": {"fns": ["numbers"], "inputs": ["{{c1::a}}{{c3::b}}{{c0::l}}{{c700::z}}", "{{c1::open"]}}]}
        empty = {"observe": [{"cloze": {"fns": ["front"], "inputs": []}}]}
        out, _ = self.run_steps([{"observe": [req]}, nums, empty])
        self.assertEqual(
            out[0]["cloze"],
            {
                "front": [
                    '<span class="cloze" data-cloze="Paris" data-ordinal="1">[capital]</span> is in '
                    '<span class="cloze-inactive" data-ordinal="2">France</span>',
                    "",
                ],
                "back": [
                    '<span class="cloze" data-ordinal="1">Paris</span> is in '
                    '<span class="cloze-inactive" data-ordinal="2">France</span>',
                    "",
                ],
                "only_front": ["capital", ""],
                "only_back": ["Paris", ""],
                "typing": ["Paris", ""],
            },
        )
        self.assertEqual(list(out[0]["cloze"]), ["typing", "front", "back", "only_front", "only_back"])
        self.assertEqual(out[1]["cloze"], {"numbers": [[1, 3, 700], []]})
        self.assertEqual(out[2]["cloze"], {"front": []})

    def test_texts_that_count_as_empty_give_empty_output(self):
        # A standard template whose fields are all empty renders the blank-front
        # message; the sentinel field keeps it out of the value.
        inputs = [["", 1], ["  ", 1], ["<br>", 2], ["{{c2::x}}", 1]]
        req = {"cloze": {"fns": ["front", "back", "only_front", "only_back"], "inputs": inputs}}
        out, _ = self.run_steps([{"observe": [req]}])
        for fn in ("front", "back", "only_front", "only_back"):
            self.assertEqual(out[0]["cloze"][fn], ["", "", "", ""], fn)

    def test_controls_and_card_bounds_pass_through(self):
        inputs = [["{{c1::\x00nul\x01}}\r ", 1], ["{{c65535::last}}", 65535]]
        req = {"cloze": {"fns": ["front", "back"], "inputs": inputs}}
        out, _ = self.run_steps([{"observe": [req]}])
        self.assertEqual(
            out[0]["cloze"]["front"],
            [
                '<span class="cloze" data-cloze="&#x00;nul&#x01;" data-ordinal="1">[...]</span>\r ',
                '<span class="cloze" data-cloze="last" data-ordinal="65535">[...]</span>',
            ],
        )
        self.assertEqual(out[0]["cloze"]["back"][0], '<span class="cloze" data-ordinal="1">\x00nul\x01</span>\r ')

    def test_requests_merge_and_never_touch_the_collection(self):
        a = {"cloze": {"fns": ["front"], "inputs": [["{{c1::x}}", 1]]}}
        b = {"cloze": {"fns": ["typing"], "inputs": [["{{c1::x}}", 1]]}}
        out, before = self.run_steps([{"observe": []}])
        out, after = self.run_steps([{"observe": [a, b]}])
        self.assertEqual(set(out[0]["cloze"]), {"front", "typing"})
        self.assertEqual(after, before)
        self.assertEqual(after[:2], [0, 0])

    def test_bad_requests_are_invalid(self):
        for args in (
            {"fns": ["front"]},
            {"fns": ["front"], "inputs": [], "extra": 1},
            {"fns": [], "inputs": []},
            "front",
            {"fns": "front", "inputs": []},
            {"fns": ["nope"], "inputs": []},
            {"fns": ["front", "front"], "inputs": []},
            {"fns": ["numbers", "front"], "inputs": []},
            {"fns": ["numbers"], "inputs": [["{{c1::a}}", 1]]},
            {"fns": ["numbers"], "inputs": [1]},
            {"fns": ["front"], "inputs": "x"},
            {"fns": ["front"], "inputs": ["{{c1::a}}"]},
            {"fns": ["front"], "inputs": [["{{c1::a}}"]]},
            {"fns": ["front"], "inputs": [["{{c1::a}}", 1, 2]]},
            {"fns": ["front"], "inputs": [[1, 1]]},
            {"fns": ["front"], "inputs": [["{{c1::a}}", 0]]},
            {"fns": ["front"], "inputs": [["{{c1::a}}", 65536]]},
            {"fns": ["typing"], "inputs": [["{{c1::a}}", -1]]},
            {"fns": ["back"], "inputs": [["{{c1::a}}", 1.0]]},
            {"fns": ["back"], "inputs": [["{{c1::a}}", "1"]]},
            {"fns": ["only_back"], "inputs": [["{{c1::a}}", True]]},
            {"fns": ["only_front"], "inputs": [["{{c1::a}}", None]]},
        ):
            with self.subTest(args=args), self.assertRaises(DriverError) as ctx:
                self.run_steps([{"observe": [{"cloze": args}]}])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")


if __name__ == "__main__":
    unittest.main()
