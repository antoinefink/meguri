# SPDX-License-Identifier: AGPL-3.0-or-later
"""TAP runner for the driver self-tests: python -m driver.tests [name-filter...].

Exit 0 when every selected test passes, 1 when one fails, 2 when the runner
breaks or the filters select no test.
"""

from __future__ import annotations

import sys
import time
import traceback
import unittest
from pathlib import Path


def _flatten(suite):
    for t in suite:
        if isinstance(t, unittest.TestSuite):
            yield from _flatten(t)
        else:
            yield t


def main(argv: list[str]) -> int:
    here = Path(__file__).resolve().parent
    loader = unittest.TestLoader()
    suite = loader.discover(str(here), pattern="test_*.py", top_level_dir=str(here.parent.parent))
    tests = [t for t in _flatten(suite) if not argv or any(a in t.id() for a in argv)]
    if not tests:  # a misspelled filter must not pass with nothing run
        print(f"driver.tests: no self-test matches {' '.join(argv) or '(no filter)'}", file=sys.stderr)
        return 2
    print(f"1..{len(tests)}", flush=True)
    failed = 0
    for n, test in enumerate(tests, 1):
        result = unittest.TestResult()
        started = time.perf_counter()
        test.run(result)
        elapsed = time.perf_counter() - started
        name = test.id().removeprefix("driver.tests.")
        problems = result.errors + result.failures + [(t, "unexpected success") for t in result.unexpectedSuccesses]
        if result.skipped:
            print(f"ok {n} {name} # SKIP {result.skipped[0][1]}", flush=True)
        elif problems:
            failed += 1
            print(f"not ok {n} {name}", flush=True)
            for _, text in problems:
                for line in str(text).rstrip().splitlines():
                    print(f"# {line}", flush=True)
        else:
            print(f"ok {n} {name}", flush=True)
        for line in getattr(test, "diagnostics", []):
            print(f"# {name}: {line}", file=sys.stderr, flush=True)
        print(f"# {name}: {elapsed:.2f}s", file=sys.stderr, flush=True)
    return 1 if failed else 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Exception:  # noqa: BLE001 - report a broken runner as a failed plan
        traceback.print_exc()
        sys.exit(2)
