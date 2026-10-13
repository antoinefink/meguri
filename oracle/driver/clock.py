# SPDX-License-Identifier: AGPL-3.0-or-later
"""The worker's frozen clock.

libfaketime reads $FAKETIME_TIMESTAMP_FILE on every clock call
(FAKETIME_NO_CACHE=1) and parses it with FAKETIME_FMT=%s, so the file holds
plain epoch seconds and the clock stays frozen at that instant until the driver
writes another one (R1870). Epoch seconds avoid any local-time parsing, so DST
gaps and repeated hours cannot shift the clock. Monotonic clocks are real.
"""

from __future__ import annotations

import os
import re
import time
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from .errors import invalid

_DURATION = re.compile(r"^(?:\d+[dhms])+$")
_DURATION_PART = re.compile(r"(\d+)([dhms])")
_UNIT_SECS = {"d": 86400, "h": 3600, "m": 60, "s": 1}


def parse_duration_secs(text: object) -> int:
    """'20m', '1d', '23h40m', '90s' -> elapsed seconds."""
    if not isinstance(text, str) or not _DURATION.match(text):
        raise invalid(f"duration {text!r} must look like 20m, 1d or 23h40m")
    return sum(int(n) * _UNIT_SECS[u] for n, u in _DURATION_PART.findall(text))


def parse_duration_minutes(text: object) -> float:
    """Learning steps: '30s' -> 0.5, '10m' -> 10.0, '1d' -> 1440.0."""
    return parse_duration_secs(text) / 60


def zone(tz: object) -> ZoneInfo:
    if not isinstance(tz, str):
        raise invalid("clock.tz must be an IANA time zone name")
    try:
        return ZoneInfo(tz)
    except (ZoneInfoNotFoundError, ValueError) as exc:
        raise invalid(f"unknown time zone {tz!r}") from exc


def parse_instant(text: object, tz: ZoneInfo) -> int:
    """ISO 8601 date-time -> epoch seconds. Without an offset it is wall time in
    `tz`; a wall time that does not exist (DST gap) is an error, and a repeated
    one means its first occurrence."""
    if not isinstance(text, str):
        raise invalid(f"instant {text!r} must be an ISO 8601 string")
    try:
        dt = datetime.fromisoformat(text)
    except ValueError as exc:
        raise invalid(f"instant {text!r} is not ISO 8601") from exc
    if dt.microsecond:
        raise invalid(f"instant {text!r} has a fraction of a second; the clock moves in whole seconds")
    if dt.tzinfo is None:
        local = dt.replace(tzinfo=tz, fold=0)
        roundtrip = datetime.fromtimestamp(local.timestamp(), tz).replace(tzinfo=None)
        if roundtrip != dt:
            raise invalid(f"wall time {text!r} does not exist in {tz.key}")
        return int(local.timestamp())
    return int(dt.astimezone(timezone.utc).timestamp())


class Clock:
    def __init__(self, path: str) -> None:
        self.path = path
        self.now = self._read()

    def _read(self) -> int:
        try:
            with open(self.path, encoding="ascii") as f:
                return int(f.read().strip())
        except (OSError, ValueError):
            return int(time.time())

    def set(self, epoch_secs: int) -> None:
        if epoch_secs < 0:
            raise invalid("the clock cannot go before 1970")
        tmp = f"{self.path}.tmp"
        with open(tmp, "w", encoding="ascii") as f:
            f.write(f"{epoch_secs}\n")
        os.replace(tmp, self.path)
        self.now = epoch_secs

    def advance(self, secs: int) -> None:
        self.set(self.now + secs)

    def now_ms(self) -> int:
        return self.now * 1000


def utc_offset_secs(epoch_secs: int, tz: ZoneInfo) -> int:
    offset = datetime.fromtimestamp(epoch_secs, tz).utcoffset() or timedelta(0)
    return int(offset.total_seconds())
