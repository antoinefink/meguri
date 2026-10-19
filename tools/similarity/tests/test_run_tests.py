"""Filtered TAP runner regressions without corpus test execution."""

from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
import subprocess
import sys
import unittest

from run_tests import TapResult


class TapResultBookkeeping(unittest.TestCase):
    def test_assertion_failure_is_unsuccessful(self):
        class Case(unittest.TestCase):
            def test_failure(self):
                self.fail("synthetic assertion failure")

        result = TapResult()
        with redirect_stdout(StringIO()) as output:
            unittest.TestSuite([Case("test_failure")]).run(result)

        self.assertFalse(result.wasSuccessful())
        self.assertEqual(result.testsRun, 1)
        self.assertEqual(len(result.failures), 1)
        self.assertEqual(output.getvalue().splitlines()[0], f"not ok 1 {__name__}.Case.test_failure")

    def test_raised_error_is_unsuccessful(self):
        class Case(unittest.TestCase):
            def test_error(self):
                raise RuntimeError("synthetic error")

        result = TapResult()
        with redirect_stdout(StringIO()) as output:
            unittest.TestSuite([Case("test_error")]).run(result)

        self.assertFalse(result.wasSuccessful())
        self.assertEqual(result.testsRun, 1)
        self.assertEqual(len(result.errors), 1)
        self.assertEqual(output.getvalue().splitlines()[0], f"not ok 1 {__name__}.Case.test_error")

    def test_unexpected_success_is_unsuccessful(self):
        class Case(unittest.TestCase):
            @unittest.expectedFailure
            def test_unexpected_success(self):
                pass

        result = TapResult()
        with redirect_stdout(StringIO()) as output:
            unittest.TestSuite([Case("test_unexpected_success")]).run(result)

        self.assertFalse(result.wasSuccessful())
        self.assertEqual(result.testsRun, 1)
        self.assertEqual(len(result.unexpectedSuccesses), 1)
        self.assertEqual(
            output.getvalue(), f"not ok 1 {__name__}.Case.test_unexpected_success\n"
        )


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
