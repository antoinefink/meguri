# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shortest round-trip f32 text (the float trap)."""

from __future__ import annotations

import json
import random
import struct
import unittest

from anki.cards_pb2 import FsrsMemoryState

from driver import convert
from driver.numbers import f32_shortest, to_f32


def f32_from_bits(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b))[0]


def naive_shortest_digits(v: float) -> int:
    """Fewest significant digits of a %g rendering that survives f64 -> f32."""
    for p in range(1, 10):
        if to_f32(float(f"{v:.{p}g}")) == v:
            return p
    return 9


def sig_digits(text: str) -> int:
    mantissa = text.lstrip("-").split("e")[0].replace(".", "").lstrip("0").rstrip("0")
    return max(1, len(mantissa))


class F32Text(unittest.TestCase):
    def test_stability_prints_as_f32_not_widened_f64(self):
        widened = to_f32(11.262237)
        self.assertNotEqual(repr(widened), "11.262237")  # the trap: 11.262236595153809
        self.assertEqual(f32_shortest(widened), "11.262237")

    def test_known_values(self):
        cases = {
            0.9: "0.9",
            0.1: "0.1",
            2.11810398: "2.118104",
            1.0: "1.0",
            16777216.0: "16777216.0",
            3.4028234663852886e38: "3.4028235e+38",  # largest finite f32
            f32_from_bits(1): "1e-45",  # smallest subnormal
            1e-5: "1e-05",
            -0.25: "-0.25",
        }
        for value, text in cases.items():
            with self.subTest(value=value):
                self.assertEqual(f32_shortest(value), text)

    def test_random_bit_patterns_round_trip_with_no_extra_digits(self):
        rng = random.Random(20261013)
        for _ in range(20000):
            bits = rng.getrandbits(31)  # positive, finite unless the exponent is all ones
            v = f32_from_bits(bits)
            if v != v or v == float("inf") or v == 0:
                continue
            text = f32_shortest(v)
            self.assertEqual(to_f32(float(text)), v, text)
            self.assertLessEqual(sig_digits(text), naive_shortest_digits(v), (bits, text))

    def test_power_of_two_boundaries_round_trip(self):
        for exp_bits in range(1, 255):
            for delta in (-1, 0, 1):
                bits = (exp_bits << 23) + delta
                if bits <= 0:
                    continue
                v = f32_from_bits(bits)
                if v == float("inf"):
                    continue
                self.assertEqual(to_f32(float(f32_shortest(v))), v, bits)


class ObservationJson(unittest.TestCase):
    def test_memory_state_reaches_the_wire_as_shortest_f32(self):
        state = FsrsMemoryState(stability=11.262237, difficulty=2.11810398)
        line = json.dumps(convert.message(state), separators=(",", ":"))
        self.assertEqual(line, '{"stability":11.262237,"difficulty":2.118104}')
        self.assertNotIn("11.26223659", line)


if __name__ == "__main__":
    unittest.main()
