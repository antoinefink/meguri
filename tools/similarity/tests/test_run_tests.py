"""Filtered TAP runner regressions without corpus test execution."""

from pathlib import Path
import subprocess
import sys
import unittest


class FilteredRunner(unittest.TestCase):
    def run_filter(self, pattern):
        return subprocess.run(
            [sys.executable, str(Path(__file__).with_name("run_tests.py")), "-k", pattern],
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_unmatched_filter_fails_closed(self):
        result = self.run_filter(r"^$")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(
            result.stdout,
            "Bail out! explicit test filter matched no discovered tests\n",
        )
        self.assertEqual(result.stderr, "")

    def test_matched_filter_runs_exactly_one_fingerprint_test(self):
        result = self.run_filter(r"^test_fingerprint\.Winnowing\.test_longest_true_run$")
        self.assertEqual(result.returncode, 0)
        lines = result.stdout.splitlines()
        self.assertEqual(
            lines[:2],
            ["1..1", "ok 1 test_fingerprint.Winnowing.test_longest_true_run"],
        )
        self.assertEqual(len(lines), 3)
        self.assertRegex(lines[2], r"^# 1 tests in [0-9]+\.[0-9]s$")
        self.assertEqual(result.stderr, "")
