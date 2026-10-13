# SPDX-License-Identifier: AGPL-3.0-or-later
"""Clock ops. Time stands still between them (R1870).

advance: <duration>      move forward by elapsed seconds ('20m', '1d', '23h40m');
                         a day is 86400 s, also across DST changes
set_clock: <date-time>   jump to an instant: wall time in the scenario's zone,
                         or ISO 8601 with an offset
"""

from __future__ import annotations

from ..clock import parse_duration_secs, parse_instant
from ..registry import op


@op("advance")
def advance(session, args):
    session.clock.advance(parse_duration_secs(args))


@op("set_clock")
def set_clock(session, args):
    session.clock.set(parse_instant(args, session.tz))
