# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shortest round-trip text for f32 values.

Anki keeps FSRS memory states, ease factors and similar values as f32. Python
widens them to f64, whose repr (11.262236595153809) is not what any f32-aware
printer emits (11.262237). Observations carry the shortest decimal that parses
back to the same f32, chosen like Ryu: fewest significant digits, then the one
closest to the exact value, then (on a tie) the one with an even last digit.
"""

from __future__ import annotations

import math
import struct
from decimal import Decimal
from fractions import Fraction


def _bits(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", x))[0]


def _from_bits(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b))[0]


def to_f32(x: float) -> float:
    """The f32 nearest to x (round half to even), widened back to a Python float."""
    return struct.unpack("<f", struct.pack("<f", x))[0]


def _rounding_interval(v: float) -> tuple[Fraction, Fraction, bool]:
    """Exact bounds of the decimals that parse to the positive finite f32 v, and
    whether the bounds themselves do (ties round to the even significand)."""
    b = _bits(v)
    exact = Fraction(v)
    up = _from_bits(b + 1)
    hi_gap = (Fraction(up) - exact) if math.isfinite(up) else (exact - Fraction(_from_bits(b - 1)))
    lo_gap = exact - Fraction(_from_bits(b - 1)) if b > 1 else exact
    return exact - lo_gap / 2, exact + hi_gap / 2, b % 2 == 0


def f32_shortest(x: float) -> str:
    """Shortest decimal text that parses back to the f32 nearest to x."""
    v = to_f32(x)
    if math.isnan(v) or math.isinf(v):
        raise ValueError(f"non-finite f32 {x!r} has no JSON text")
    if v == 0:
        return "-0.0" if math.copysign(1.0, v) < 0 else "0.0"
    sign = "-" if v < 0 else ""
    v = abs(v)
    lo, hi, inclusive = _rounding_interval(v)
    exact = Fraction(v)

    def inside(d: Decimal) -> bool:
        f = Fraction(d)
        return lo < f < hi or (inclusive and (f == lo or f == hi))

    for digits in range(1, 10):
        nearest = Decimal(f"{v:.{digits - 1}e}")
        step = Decimal(1).scaleb(nearest.adjusted() - digits + 1)
        candidates = [c for c in (nearest - step, nearest, nearest + step) if c > 0 and inside(c)]
        candidates = [c for c in candidates if len(c.normalize().as_tuple().digits) <= digits]
        if candidates:
            # Closest to the exact value; a tie keeps the even last digit (Ryu).
            best = min(candidates, key=lambda c: (abs(Fraction(c) - exact), int(c / step) % 2))
            return sign + _plain(best)
    raise AssertionError(f"no 9-digit decimal round-trips {x!r}")  # f32 needs at most 9


def _plain(d: Decimal) -> str:
    """Decimal text in the form Python's float repr uses (1e-05, 11.262237, 100.0)."""
    return repr(float(d))


def f32_value(x: float) -> float:
    """The Python float whose repr is f32_shortest(x): what observations store."""
    return float(f32_shortest(x))
