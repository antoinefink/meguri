# SPDX-License-Identifier: AGPL-3.0-or-later
"""The worker's start-up check of its frozen clock (float-determinism.md F6.3).

Before it answers anything, a worker freezes the clock file at a fixed instant
and checks that Python's clock and Anki's scheduler day follow it, then moves
it one day and checks again. The clock file then holds what it held before.

- faketime: every check held. Scenarios may move time (clock.start, advance,
  set_clock). Container workers must get this or refuse to start.
- none: a native worker whose libfaketime is missing or does not reach
  Python or Anki's backend. It runs on the real clock, and tools give it only
  scenarios tagged clock-free.
"""

from __future__ import annotations

import os
import shutil
import tempfile
import time

from anki.collection import Collection

from .clock import write_epoch

# 2030-01-15T10:00:00Z, the start most scenarios use.
INSTANT = 1894701600
DAY = 86400
# A day is 23 to 25 hours long where the zone changes its offset.
SHORTEST_DAY, LONGEST_DAY = DAY - 3600, DAY + 3600


def _restore(path: str, previous: str | None) -> None:
    if previous is None:
        try:
            os.remove(path)
        except FileNotFoundError:
            pass
        return
    tmp = f"{path}.tmp"
    with open(tmp, "w", encoding="ascii") as f:
        f.write(previous)
    os.replace(tmp, path)


def _follows(clock_file: str, scratch: str) -> bool:
    write_epoch(clock_file, INSTANT)
    if time.time() != INSTANT:
        return False
    directory = tempfile.mkdtemp(prefix="selftest-", dir=scratch)
    col = None
    try:
        col = Collection(os.path.join(directory, "collection.anki2"))
        first = col._backend.sched_timing_today()
        if not (INSTANT - LONGEST_DAY < col.crt <= INSTANT and INSTANT < first.next_day_at <= INSTANT + LONGEST_DAY):
            return False
        write_epoch(clock_file, INSTANT + DAY)
        if time.time() != INSTANT + DAY:
            return False
        second = col._backend.sched_timing_today()
        return (
            second.days_elapsed == first.days_elapsed + 1
            and SHORTEST_DAY <= second.next_day_at - first.next_day_at <= LONGEST_DAY
        )
    finally:
        if col is not None:
            col.close(downgrade=False)
        shutil.rmtree(directory, ignore_errors=True)


def check(clock_file: str, scratch: str) -> str:
    """'faketime' when the frozen clock reaches Python and Anki, else 'none'."""
    try:
        with open(clock_file, encoding="ascii") as f:
            previous: str | None = f.read()
    except FileNotFoundError:
        previous = None
    try:
        return "faketime" if _follows(clock_file, scratch) else "none"
    finally:
        _restore(clock_file, previous)
