# SPDX-License-Identifier: AGPL-3.0-or-later
"""The text observation: argument checks, and that it never touches the
scenario's collection."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.session import Session


class TextObservation(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def run_steps(self, steps):
        sc = {"id": "t/text", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        s = Session(sc, "UTC", self.tmp.name, self.clock)
        try:
            out = s.run_all()["steps"]
            notes = s.col.db.scalar("select count() from notes")
            media = list(Path(s.col.media.dir()).iterdir())
        finally:
            s.close()
        return out, notes, media

    def test_values_are_keyed_by_function_in_input_order(self):
        req = {"text": {"fns": ["strip_markup", "checksum", "field_empty"], "inputs": ["<b>a</b>", "", "c"]}}
        names = {"text": {"fns": ["media_filename"], "inputs": ["con.jpg", "a?b.png", "con.jpg"]}}
        out, notes, media = self.run_steps([{"observe": [req, names]}])
        value = out[0]["text"]
        self.assertEqual(value["strip_markup"], ["a", "", "c"])
        self.assertEqual(value["field_empty"], [False, True, False])
        self.assertEqual(value["checksum"][1], 0xDA39A3EE)  # first 4 bytes of SHA-1("")
        self.assertEqual(value["media_filename"], ["con_.jpg", "ab.png", "con_.jpg"])
        self.assertEqual((notes, media), (0, []))

    def test_pairs(self):
        req = {"text": {"fns": ["unicase_compare"], "inputs": [["a", "B"], ["ß", "SS"], ["b", "A"]]}}
        out, _, _ = self.run_steps([{"observe": [req]}])
        self.assertEqual(out[0]["text"], {"unicase_compare": [-1, 0, 1]})

    def test_bad_requests_are_invalid(self):
        for args in (
            {"fns": ["strip_markup"]},
            {"fns": [], "inputs": []},
            {"fns": ["nope"], "inputs": []},
            {"fns": ["strip_markup", "strip_markup"], "inputs": []},
            {"fns": ["strip_markup"], "inputs": [1]},
            {"fns": ["unicase_compare"], "inputs": ["a"]},
            {"fns": ["unicase_compare", "strip_markup"], "inputs": []},
            {"fns": ["media_filename"], "inputs": ["A.jpg"]},
        ):
            with self.subTest(args=args), self.assertRaises(DriverError) as ctx:
                self.run_steps([{"observe": [{"text": args}]}])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")


if __name__ == "__main__":
    unittest.main()
