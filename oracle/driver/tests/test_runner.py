# SPDX-License-Identifier: AGPL-3.0-or-later
"""The self-test runner itself (python -m driver.tests [name-filter])."""

from __future__ import annotations

import subprocess
import sys
import unittest

from driver.harness import ORACLE_HOME


def selftest(*argv):
    return subprocess.run([sys.executable, "-m", "driver.tests", *argv], cwd=ORACLE_HOME,
                          capture_output=True, text=True, timeout=300)


class Runner(unittest.TestCase):
    def test_a_filter_that_selects_no_test_fails(self):
        proc = selftest("NoSuchSelfTest")
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn("NoSuchSelfTest", proc.stderr)
        self.assertNotIn("ok ", proc.stdout)

    def test_a_filter_runs_only_the_matching_tests(self):
        proc = selftest("F32Text.test_known_values")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertEqual(proc.stdout.splitlines(), ["1..1", "ok 1 test_numbers.F32Text.test_known_values"])


if __name__ == "__main__":
    unittest.main()
