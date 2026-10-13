# SPDX-License-Identifier: AGPL-3.0-or-later
"""Preset options land on the deck-config fields they name (read back through
Anki's typed deck config, not through the JSON the driver wrote)."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.session import Session

OPTIONS = {
    "learn_steps": ["30s", "10m", "1d"],
    "graduating_interval_days": 3,
    "easy_interval_days": 6,
    "new_per_day": 17,
    "starting_ease": 2.3,
    "bury_new": True,
    "reviews_per_day": 345,
    "max_interval_days": 999,
    "easy_bonus": 1.4,
    "hard_interval": 1.1,
    "interval_modifier": 0.95,
    "bury_reviews": True,
    "relearn_steps": ["5m"],
    "lapse_new_interval": 0.2,
    "min_lapse_interval_days": 2,
    "leech_threshold": 5,
    "leech_action": "suspend",
    "bury_interday_learning": True,
    "desired_retention": 0.85,
    "fsrs_params": [0.5] * 21,
}


class Presets(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def session(self, setup):
        sc = {"id": "t/p", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "setup": setup, "steps": []}
        return Session(sc, "UTC", self.tmp.name, self.clock)

    def test_every_option_reaches_its_deck_config_field(self):
        s = self.session({"presets": [{"name": "P", **OPTIONS}], "decks": [{"name": "A", "preset": "P"}]})
        try:
            c = s.col._backend.get_deck_config(s.presets["P"]).config
            deck = s.col.decks.get(s.decks["A"])
        finally:
            s.close()
        self.assertEqual(list(c.learn_steps), [0.5, 10.0, 1440.0])
        self.assertEqual((c.graduating_interval_good, c.graduating_interval_easy), (3, 6))
        self.assertEqual((c.new_per_day, c.reviews_per_day, c.maximum_review_interval), (17, 345, 999))
        self.assertAlmostEqual(c.initial_ease, 2.3, places=6)
        self.assertAlmostEqual(c.easy_multiplier, 1.4, places=6)
        self.assertAlmostEqual(c.hard_multiplier, 1.1, places=6)
        self.assertAlmostEqual(c.interval_multiplier, 0.95, places=6)
        self.assertAlmostEqual(c.lapse_multiplier, 0.2, places=6)
        self.assertAlmostEqual(c.desired_retention, 0.85, places=6)
        self.assertEqual((c.bury_new, c.bury_reviews, c.bury_interday_learning), (True, True, True))
        self.assertEqual(list(c.relearn_steps), [5.0])
        self.assertEqual((c.minimum_lapse_interval, c.leech_threshold), (2, 5))
        self.assertEqual(c.leech_action, 0)  # suspend
        self.assertEqual(list(c.fsrs_params_6), [0.5] * 21)
        self.assertEqual(deck["conf"], s.presets["P"])

    def test_unknown_option_is_refused(self):
        with self.assertRaises(DriverError) as cm:
            self.session({"presets": [{"name": "P", "learning_stepz": ["1m"]}]})
        self.assertEqual(cm.exception.kind, "invalid_scenario")


if __name__ == "__main__":
    unittest.main()
