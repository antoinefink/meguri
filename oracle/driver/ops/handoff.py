# SPDX-License-Identifier: AGPL-3.0-or-later
"""Handoffs of live scenarios (dsl.md D9.2).

handoff: meguri | oracle   the side that runs the next steps. In a whole run
                           (a golden) the oracle runs every segment, so a
                           handoff only closes and reopens the collection; a
                           segment run (protocol.json run with steps/handoff)
                           ends before it and hands the session directory over.
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import op

SIDES = ("meguri", "oracle")


@op("handoff")
def handoff(session, args):
    if args not in SIDES:
        raise invalid("handoff takes meguri or oracle")
    session.reopen()
