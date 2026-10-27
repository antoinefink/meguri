# SPDX-License-Identifier: AGPL-3.0-or-later
"""The tag_complete observation (specs/tags/complete.md C7): matching, order and
the limit cut, request checks, and that the collection never changes.
Expected values are the example tables of complete.md C3 and C4, never the
driver's output."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.session import Session

PLAIN = {"name": "Plain", "fields": ["Text"], "templates": [{"name": "Card", "front": "{{Text}}", "back": "{{Text}}"}]}


def complete(*cases):
    return {"observe": [{"tag_complete": {"cases": [{"input": i, "limit": n} for i, n in cases]}}]}


class TagComplete(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, steps, tags=()):
        setup = {"notetypes": [PLAIN], "notes": [{"notetype": "Plain", "fields": ["x"], "tags": list(tags)}]}
        sc = {"id": "t/tag-complete", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "setup": setup, "steps": steps}
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def run_steps(self, steps, tags=()):
        s = self.session(steps, tags)
        try:
            return s.run_all()["steps"]
        finally:
            s.close()

    def test_c3_matches(self):
        tags = ["German::Verb::modal", "japan", "Japanese::Audio"]
        cases = [("jap", 500), ("jap::aud", 500), ("verb::mod", 500), ("pan", 500), ("a", 500),
                 ("audio::jap", 500), ("::", 500)]
        out = self.run_steps([complete(*cases)], tags)[0]["tag_complete"]
        self.assertEqual(out, [
            ["japan", "Japanese::Audio"],
            ["Japanese::Audio"],
            ["German::Verb::modal"],
            ["japan", "Japanese::Audio"],
            ["German::Verb::modal", "japan", "Japanese::Audio"],
            [],
            ["German::Verb::modal", "Japanese::Audio"],
        ])

    def test_c4_cut_before_reordering(self):
        tags = ["Grammar::Japan", "Japan", "Japanese"]
        out = self.run_steps([complete(("jap", 1), ("jap", 2), ("jap", 3), ("jap", 500))], tags)[0]["tag_complete"]
        self.assertEqual(out, [
            ["Grammar::Japan"],
            ["Japan", "Grammar::Japan"],
            ["Japan", "Japanese", "Grammar::Japan"],
            ["Japan", "Japanese", "Grammar::Japan"],
        ])

    def test_the_collection_never_changes(self):
        steps = [{"observe": ["db_dump"]}, complete(("", 500), ("x", 1)), {"observe": ["db_dump"]}]
        out = self.run_steps(steps, ["x::y"])
        self.assertEqual(out[1]["tag_complete"], [["x::y"], ["x::y"]])
        self.assertEqual(out[2]["db_dump"], out[0]["db_dump"])

    def test_requests_outside_c7_are_invalid_before_any_collection(self):
        good = {"input": "a", "limit": 1}
        bad = [None, [], {}, {"cases": []}, {"cases": good}, {"cases": [good], "extra": 1},
               {"cases": [{"input": "a"}]}, {"cases": [{"limit": 1}]}, {"cases": [dict(good, extra=1)]},
               {"cases": [{"input": 1, "limit": 1}]}, {"cases": [{"input": None, "limit": 1}]},
               {"cases": [{"input": "a", "limit": 0}]}, {"cases": [{"input": "a", "limit": True}]},
               {"cases": [{"input": "a", "limit": 1.0}]}, {"cases": [{"input": "a", "limit": 2**32 + 1}]},
               {"cases": [{"input": "\ud800", "limit": 1}]}]
        for args in bad:
            with self.subTest(args=args), self.assertRaises(DriverError) as ctx:
                self.session([{"observe": ["ids"]}, {"observe": [{"tag_complete": args}]}])
            self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("invalid_scenario", 1))
        with self.assertRaises(DriverError) as ctx:
            self.session([{"observe": [{"tag_complete": {"cases": [good]}}, {"tag_complete": {"cases": [good]}}]}])
        self.assertEqual(ctx.exception.kind, "invalid_scenario")


if __name__ == "__main__":
    unittest.main()
