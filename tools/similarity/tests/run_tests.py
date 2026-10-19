#!/usr/bin/env python3
"""Runs tools/similarity/tests/test_*.py and prints TAP (for tools/mg).

usage: run_tests.py [--out DIR] [-k REGEX]
--out: where calibration.json goes (the tools/mg run directory).
-k: only tests whose id matches REGEX.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import time
import traceback
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)


def flatten(suite):
    for t in suite:
        if isinstance(t, unittest.TestSuite):
            yield from flatten(t)
        else:
            yield t


def name_of(test):
    method = getattr(test, "_testMethodName", None)
    if method is None:  # a class or module fixture error
        return str(test)
    return f"{test.__class__.__module__}.{test.__class__.__name__}.{method}"


class TapResult(unittest.TestResult):
    def __init__(self):
        super().__init__()
        self.n = 0

    def _line(self, ok, test, extra=""):
        self.n += 1
        print(f"{'ok' if ok else 'not ok'} {self.n} {name_of(test)}{extra}", flush=True)

    def _diag(self, err):
        for line in "".join(traceback.format_exception(*err)).rstrip().splitlines()[-25:]:
            print(f"# {line}", flush=True)

    def addSuccess(self, test):
        self._line(True, test)

    def addFailure(self, test, err):
        self._line(False, test)
        self._diag(err)

    def addError(self, test, err):
        self._line(False, test)
        self._diag(err)

    def addSkip(self, test, reason):
        self._line(True, test, f" # SKIP {reason}")

    def addExpectedFailure(self, test, err):
        self._line(True, test)

    def addUnexpectedSuccess(self, test):
        self._line(False, test)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out")
    ap.add_argument("-k")
    args = ap.parse_args()
    if args.out:
        os.makedirs(args.out, exist_ok=True)
        os.environ["MG_TEST_OUT"] = args.out
    suite = unittest.defaultTestLoader.discover(HERE, pattern="test_*.py", top_level_dir=HERE)
    tests = list(flatten(suite))
    if args.k is not None:
        tests = [t for t in tests if re.search(args.k, name_of(t))]
        if not tests:
            print("Bail out! explicit test filter matched no discovered tests", flush=True)
            return 2
    print(f"1..{len(tests)}", flush=True)
    result = TapResult()
    t0 = time.monotonic()
    unittest.TestSuite(tests).run(result)
    print(f"# {len(tests)} tests in {time.monotonic() - t0:.1f}s", flush=True)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
