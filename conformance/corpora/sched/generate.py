#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes the scheduler day-clock matrix as generated scenarios.

    python3 -B -I conformance/corpora/sched/generate.py [--out conformance/scenarios/sched/day/matrix]

Output (specs/conformance/dsl.md D1, generated `.json` scenarios with oracle
goldens; specs/sched/day-clock.md D14): one file per time zone of R1899,

  <zone>.json   every rollover hour 0 to 23, each observed one second before,
                at, and one second after its boundary (R1898)

Each scenario creates its collection at 2030-01-15 10:00 local time. Hour h is
observed on 2030-01-17 + 2h days, so the hours run from 2030-01-17 to
2030-03-04: no zone of the list changes its UTC offset in that span, and each
block starts after the day the previous block computed has ended (the oracle
reuses a computed day until its boundary passes, day-clock D6). The block sets
`rollover` first, then moves the clock.

Nothing here is random. Instants are written with their UTC offset, taken from
Python's IANA database (zoneinfo); 3.13.1 with the system tzdata wrote the
committed files. Never edit the output by hand: regenerate, then re-record the
goldens with `tools/mg oracle record sched/day/matrix`.
"""

from __future__ import annotations

import argparse
import json
from datetime import date, datetime, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

# R1899's list, in its order.
ZONES = [
    "UTC",
    "Europe/Paris",
    "America/New_York",
    "America/Los_Angeles",
    "Asia/Tokyo",
    "Asia/Kolkata",
    "Asia/Kathmandu",
    "Pacific/Chatham",
    "Australia/Lord_Howe",
    "Pacific/Kiritimati",
    "America/St_Johns",
]

START = "2030-01-15T10:00:00"
FIRST_DAY = date(2030, 1, 17)
DAYS_BETWEEN_HOURS = 2


def slug(zone: str) -> str:
    return zone.lower().replace("/", "-").replace("_", "-")


def stamp(instant: datetime) -> str:
    """ISO date-time in whole seconds with its offset, as dsl.md D5 reads it."""
    text = instant.isoformat(timespec="seconds")
    return text[:-6] + "Z" if instant.utcoffset() == timedelta(0) else text


def boundary(zone: ZoneInfo, hour: int) -> datetime:
    day = FIRST_DAY + timedelta(days=DAYS_BETWEEN_HOURS * hour)
    local = datetime(day.year, day.month, day.day, hour, 0, 0, tzinfo=zone)
    # The span holds no offset change, so the wall time exists exactly once.
    assert local.utcoffset() == (local - timedelta(seconds=1)).astimezone(zone).utcoffset()
    return local


def scenario(zone_name: str) -> dict:
    zone = ZoneInfo(zone_name)
    steps: list[dict] = []
    first = boundary(zone, 0).astimezone(zone)
    last = boundary(zone, 23).astimezone(zone)
    start = datetime.fromisoformat(START).replace(tzinfo=zone)
    for probe in (start, first, last):
        assert probe.utcoffset() == start.utcoffset(), f"{zone_name}: offset changes inside the matrix span"
    for hour in range(24):
        b = boundary(zone, hour)
        before = (b - timedelta(seconds=1)).astimezone(zone)
        steps.append({"set_config": {"key": "rollover", "value": hour}})
        steps.append({"set_clock": stamp(before)})
        steps.append({"observe": ["timing"]})
        steps.append({"advance": "1s"})
        steps.append({"observe": ["timing"]})
        steps.append({"advance": "1s"})
        steps.append({"observe": ["timing"]})
    return {
        "id": f"sched/day/matrix/{slug(zone_name)}",
        "description": f"Day number and next day boundary in {zone_name} for rollover hours 0 to 23, one second before, at and after each boundary (R1898, R1899).",
        "tags": ["generated", "rollover-matrix", f"tz:{zone_name}"],
        "clock": {"tz": zone_name, "start": START, "mode": "step"},
        "steps": steps,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", default="conformance/scenarios/sched/day/matrix")
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    for zone_name in ZONES:
        path = out / f"{slug(zone_name)}.json"
        text = json.dumps(scenario(zone_name), indent=2, ensure_ascii=False) + "\n"
        path.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
