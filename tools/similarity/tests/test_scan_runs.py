"""Allowlisted synthetic fingerprints separate structural matching runs."""

import unittest
from unittest.mock import Mock

import numpy as np

from support import CONFIG

from meguri_similarity.config import AllowEntry, load_config
from meguri_similarity.scan import Scanner


class AllowlistedRuns(unittest.TestCase):
    def setUp(self):
        self.cfg = load_config(CONFIG)
        self.limit = self.cfg["channels"]["struct"]["consecutive"]

    def evaluate(self, matching, excluded):
        matching = np.asarray(matching, dtype=bool)
        hashes = np.arange(len(matching), dtype=np.uint64)
        first = 10 + 4 * np.arange(len(matching), dtype=np.uint32)
        last = first + 2
        index = Mock()
        index.lookup.side_effect = lambda _channel, h: (
            matching[h.astype(int)], np.zeros(len(h), dtype=np.uint32), np.ones(len(h), dtype=np.uint32)
        )
        index.blob.return_value = ("synthetic", "synthetic", "source.rs")
        allow = [
            AllowEntry(i + 1, "core/own.rs", ("struct",), (int(first[i]), int(last[i])), "synthetic")
            for i in excluded
        ]
        scanner = Scanner(self.cfg, index, None, allow)
        result = scanner._evaluate("core/own.rs", "struct", (hashes, first, last, None), allow)
        kept = [i for i in range(len(matching)) if i not in excluded]
        self.assertEqual(result.fingerprints, len(kept))
        self.assertEqual(result.matched, sum(bool(matching[i]) for i in kept))
        self.assertEqual(result.score, result.matched / len(kept) if kept else 0.0)
        self.assertEqual(
            [(r.start, r.end) for r in result.regions],
            [(int(first[i]), int(last[i])) for i in kept if matching[i]],
        )
        return result, first, last

    def test_excluded_middle_does_not_join_two_short_runs(self):
        result, _first, _last = self.evaluate([True] * (2 * self.limit - 1), [self.limit - 1])
        self.assertEqual(result.longest_run, self.limit - 1)
        self.assertFalse(result.flagged)

    def test_contiguous_run_after_exclusions_still_flags_original_lines(self):
        matching = [True, True, True, False] + [True] * self.limit + [False, False]
        result, first, last = self.evaluate(matching, [0, 1])
        self.assertEqual(result.longest_run, self.limit)
        self.assertTrue(result.flagged)
        self.assertIn(f"lines {first[4]}-{last[4 + self.limit - 1]}", result.reasons[0])


if __name__ == "__main__":
    unittest.main()
