# SPDX-License-Identifier: AGPL-3.0-or-later
"""timing: the scheduler's day at the current instant.

{days_elapsed, next_day_at: epoch secs of the next rollover,
 now: epoch secs as this process sees it, utc_offset_secs: this process's
 local offset now}. now and utc_offset_secs come from the worker's own
libc clock and TZ, so a worker that reads another worker's clock or zone shows it.
The collection check reports the same value (collection-check.md K3).
"""

from __future__ import annotations

import time

from ..registry import observation


def value(col) -> dict:
    t = col.sched._timing_today()
    local = time.localtime()
    return {
        "days_elapsed": t.days_elapsed,
        "next_day_at": t.next_day_at,
        "now": int(time.time()),
        "utc_offset_secs": local.tm_gmtoff,
    }


@observation("timing")
def timing(session, args):
    return value(session.col)
