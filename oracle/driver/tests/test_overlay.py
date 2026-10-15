# SPDX-License-Identifier: AGPL-3.0-or-later
"""Overlay goldens (float-determinism.md F7, F8): the value-by-value diff, the
fp-libm-boundary class with the spec's own examples (F8.4), unrounded
intervals, freshness and application. Pure Python, no worker."""

from __future__ import annotations

import unittest

from driver import overlay as ov
from driver.numbers import to_f32

ROW = "/steps/0/fsrs_next/rows/417"
CARD = "/steps/2/next_states/#c1"


def day(ptr, linux, apple, u_l=None, u_a=None):
    entry = {"pointer": ptr, "linux": linux, "apple": apple}
    if u_l is not None:
        entry["unrounded"] = {"linux": u_l, "apple": u_l if u_a is None else u_a}
    return entry


def outside(entries):
    return [e["pointer"] for e in ov.outside_class(entries)]


class Diff(unittest.TestCase):
    def test_differing_scalars_in_pointer_order(self):
        linux = {"steps": [{"fsrs_next": {"rows": [[15, 13.836124, 2.0], [3, 1.0, 1.0]]}}, {"timing": {"now": 1}}]}
        apple = {"steps": [{"fsrs_next": {"rows": [[16, 13.836125, 2.0], [3, 1.0, 1]]}}, {"timing": {"now": 1}}]}
        self.assertEqual(ov.scalar_diff(linux, apple), [
            {"pointer": "/steps/0/fsrs_next/rows/0/0", "linux": 15, "apple": 16},
            {"pointer": "/steps/0/fsrs_next/rows/0/1", "linux": 13.836124, "apple": 13.836125},
            # 1.0 and 1 are different JSON values (F7.2 step 3).
            {"pointer": "/steps/0/fsrs_next/rows/1/2", "linux": 1.0, "apple": 1},
        ])
        self.assertEqual(ov.scalar_diff({"a": [True, None, "x"]}, {"a": [True, None, "x"]}), [])
        self.assertEqual(ov.scalar_diff({"a": True}, {"a": 1}), [{"pointer": "/a", "linux": True, "apple": 1}])

    def test_structural_differences_raise(self):
        for linux, apple, ptr in (
            ({"a": 1}, {"a": 1, "b": 2}, "/b"),
            ({"a": [1, 2]}, {"a": [1]}, "/a/1"),
            ({"a": 1}, {"a": [1]}, "/a"),
            ({"a": {}}, {"a": None}, "/a"),
            ({"a/b": {"x~": 1}}, {"a/b": {}}, "/a~1b/x~0"),
        ):
            with self.subTest(ptr=ptr), self.assertRaises(ov.Structural) as ctx:
                ov.scalar_diff(linux, apple)
            self.assertEqual(ctx.exception.pointer, ptr)


class Class(unittest.TestCase):
    """float-determinism.md F8.4, entry by entry."""

    def test_a_day_on_a_half_is_in_the_class(self):
        self.assertEqual(outside([day(f"{ROW}/6", 15, 16, 15.499999046325684, 15.500000953674316)]), [])

    def test_three_days_apart_is_not(self):
        self.assertEqual(outside([day(f"{ROW}/6", 15, 18, 15.499999046325684, 15.500000953674316)]), [f"{ROW}/6"])

    def test_no_boundary_near_the_interval_is_not(self):
        # B(15.2) is about {15.2, 12.705, 17.695}.
        self.assertEqual(outside([day(f"{ROW}/6", 15, 16, 15.2)]), [f"{ROW}/6"])

    def test_the_lower_end_of_the_fuzz_range_on_a_half_is_in_the_class(self):
        # Delta(14.972222) = 2.472222, so u - Delta is 12.5.
        self.assertAlmostEqual(ov.fuzz_delta(14.972222), 2.472222, places=6)
        self.assertEqual(outside([day(f"{ROW}/6", 13, 14, 14.972222)]), [])

    def test_easy_follows_good_and_good_follows_hard(self):
        good = day(f"{ROW}/6", 13, 14, 14.972222)
        easy = day(f"{ROW}/9", 26, 27, 25.3)
        self.assertEqual(outside([good, easy]), [])
        self.assertEqual(outside([easy]), [f"{ROW}/9"])
        hard = day(f"{CARD}/hard/scheduled_days", 9, 10, 9.5)
        chained_good = day(f"{CARD}/good/scheduled_days", 14, 15, 13.84)
        chained_easy = day(f"{CARD}/easy/scheduled_days", 25, 26, 23.9)
        self.assertEqual(outside([hard, chained_good, chained_easy]), [])
        # Another card's Hard does not carry this card's Good.
        other = day("/steps/2/next_states/#c2/hard/scheduled_days", 9, 10, 9.5)
        self.assertEqual(outside([other, chained_good]), [chained_good["pointer"]])

    def test_stabilities_within_relative_1e4_or_1e5_apart_are_in_the_class(self):
        # F8.3: a stability is in the class when |apple - linux| <= max(1e-4 m, 1e-5).
        for ptr, linux, apple in (
            (f"{ROW}/7", 13.836124, 13.836125),  # relative 7e-8
            # fp-probe/corpus/next-states-029 row 820: 3.9e-7 apart, under the floor (relative 9.4e-5)
            (f"{ROW}/1", 0.0041170507, 0.0041174362),
            (f"{ROW}/1", 0.0011, 0.0011005),  # relative 4.5e-4, but 5e-7 apart
            # fp-probe/corpus/next-states-089 row 977, the easy stability: relative 1.04e-5
            (f"{ROW}/10", 59.598206, 59.598827),
            (f"{ROW}/1", 2.3065, 2.3066),  # relative 4.3e-5
            (f"{CARD}/again/learning/memory_state/stability", 0.0011, 0.0011005),
            (f"{CARD}/good/review/memory_state/stability", 2.3065, 2.3066),
            (f"{CARD}/easy/memory_state/stability", 59.598206, 59.598827),
        ):
            with self.subTest(ptr=ptr, linux=linux, apple=apple):
                self.assertEqual(outside([{"pointer": ptr, "linux": linux, "apple": apple}]), [])

    def test_stabilities_apart_by_more_are_not(self):
        for ptr, linux, apple in (
            (f"{ROW}/1", 2.3065, 2.3075),  # relative 4.3e-4, 1e-3 apart
            (f"{ROW}/4", 0.0041, 0.0042),  # 1e-4 apart, above the floor
            (f"{CARD}/hard/review/memory_state/stability", 0.0041, 0.0042),
            (f"{ROW}/1", 2, 2.0000001),  # not both written with a fraction
        ):
            with self.subTest(ptr=ptr, linux=linux, apple=apple):
                self.assertEqual(outside([{"pointer": ptr, "linux": linux, "apple": apple}]), [ptr])

    def test_difficulties_keep_relative_1e5(self):
        inside = (f"{CARD}/again/learning/memory_state/difficulty", 7.3921676, 7.392168)  # relative 5e-8
        self.assertEqual(outside([{"pointer": inside[0], "linux": inside[1], "apple": inside[2]}]), [])
        for ptr in (f"{CARD}/again/learning/memory_state/difficulty", f"{ROW}/2", f"{ROW}/11"):
            # relative 4.5e-5: inside a stability's bound, not a difficulty's
            with self.subTest(ptr=ptr):
                self.assertEqual(outside([{"pointer": ptr, "linux": 7.3921676, "apple": 7.3925}]), [ptr])
        # The floor is for stability only: 5e-7 apart is relative 4.5e-4 here.
        self.assertEqual(outside([{"pointer": f"{ROW}/5", "linux": 0.0011, "apple": 0.0011005}]), [f"{ROW}/5"])

    def test_reasons_name_the_bound_of_the_member(self):
        reasons = {e["pointer"]: e["reason"] for e in ov.outside_class([
            {"pointer": f"{ROW}/1", "linux": 2.3065, "apple": 2.3075},
            {"pointer": f"{ROW}/2", "linux": 7.3921676, "apple": 7.3925},
        ])}
        self.assertIn("1e-4", reasons[f"{ROW}/1"])
        self.assertIn("stabilit", reasons[f"{ROW}/1"])
        self.assertIn("difficult", reasons[f"{ROW}/2"])
        self.assertNotIn("1e-4", reasons[f"{ROW}/2"])

    def test_other_pointers_are_not(self):
        for ptr, linux, apple in (
            (f"{CARD}/current/memory_state/stability", 2.3065, 2.30651),
            ("/steps/3/card/#c1/data/s", 2.3065, 2.30651),
            ("/steps/0/fuzz_table/rows/12/5", 3, 4),
            (f"{CARD}/again/learning/scheduled_secs", 600, 601),
            (f"{ROW}/12", 1, 2),
        ):
            with self.subTest(ptr=ptr):
                self.assertEqual(outside([{"pointer": ptr, "linux": linux, "apple": apple}]), [ptr])

    def test_day_entries_need_their_unrounded_intervals(self):
        self.assertEqual(outside([day(f"{ROW}/0", 15, 16)]), [f"{ROW}/0"])
        far = day(f"{ROW}/0", 15, 16, 15.5, 15.6)  # rule 2: apart by more than 1e-5
        self.assertEqual(outside([far]), [f"{ROW}/0"])
        self.assertEqual(outside([day(f"{CARD}/again/review/scheduled_days", 15, 16, 15.5)]), [])


class Unrounded(unittest.TestCase):
    def test_at_retention_09_the_interval_is_the_stability(self):
        # sched/fsrs-transitions T6: with desired retention 0.9 the unrounded
        # intervals equal the new stabilities.
        for decay in (0.1542, 0.5, 0.3412):
            self.assertAlmostEqual(ov.unrounded_interval(13.836124, 0.9, decay), to_f32(13.836124), places=9)
        self.assertGreater(ov.unrounded_interval(10.0, 0.8, 0.5), 10.0)
        self.assertLess(ov.unrounded_interval(10.0, 0.95, 0.5), 10.0)

    def test_day_entries_take_their_inputs_from_the_scenario_or_the_run(self):
        scenario = {"steps": [{"observe": [{"fsrs_next": {
            "param_sets": {"default": [], "old": [0.4] * 19},
            "inputs": [{"dr": 0.9, "params": "default"}, {"dr": 0.8, "params": "old"}]}}]}]}
        linux = {"steps": [{"fsrs_next": {"rows": [[15, 15.5, 5.0] * 4, [9, 9.2, 5.0] * 4]}}]}
        apple = {"steps": [{"fsrs_next": {"rows": [[16, 15.500001, 5.0] * 4, [10, 9.2, 5.0] * 4]}}]}
        entries = ov.scalar_diff(linux, apple)
        ov.add_unrounded(entries, linux, apple, ov.FsrsInputs(scenario, None))
        first = entries[0]
        self.assertEqual(first["pointer"], "/steps/0/fsrs_next/rows/0/0")
        self.assertAlmostEqual(first["unrounded"]["linux"], to_f32(15.5), places=9)
        self.assertAlmostEqual(first["unrounded"]["apple"], to_f32(15.500001), places=9)
        second = next(e for e in entries if e["pointer"] == "/steps/0/fsrs_next/rows/1/0")
        self.assertAlmostEqual(second["unrounded"]["linux"], ov.unrounded_interval(9.2, 0.8, 0.5), places=12)
        # next_states: retention and decay come from the run's fsrs_context.
        ctx = {CARD: {"desired_retention": 0.9, "decay": 0.1542}}
        linux = {"steps": [{}, {}, {"next_states": {"#c1": {"good": {"scheduled_days": 15, "memory_state": {"stability": 15.5}}}}}]}
        apple = {"steps": [{}, {}, {"next_states": {"#c1": {"good": {"scheduled_days": 16, "memory_state": {"stability": 15.5}}}}}]}
        entries = ov.scalar_diff(linux, apple)
        ov.add_unrounded(entries, linux, apple, ov.FsrsInputs({"steps": []}, ctx))
        self.assertAlmostEqual(entries[0]["unrounded"]["apple"], 15.5, places=9)
        entries = ov.scalar_diff(linux, apple)
        ov.add_unrounded(entries, linux, apple, ov.FsrsInputs({"steps": []}, None))
        self.assertNotIn("unrounded", entries[0])


class Files(unittest.TestCase):
    GOLDEN = {"observations": {"steps": [{"fsrs_next": {"rows": [[15, 15.5, 5.0]]}}]}}
    PIN = {"anki_version": "26.9.3", "lock_sha256": "a" * 64, "macos_lock_sha256": "b" * 64}
    HELLO = {"anki_version": "26.9.3", "lock_sha256": "b" * 64, "driver_sha256": "c" * 64, "platform": "macos-arm64",
             "tz": "UTC", "time_control": "faketime"}

    def test_document_freshness_and_application(self):
        entries = [day("/steps/0/fsrs_next/rows/0/0", 15, 16, 15.5)]
        doc = ov.document("fp/x", "d" * 64, self.GOLDEN, self.HELLO, entries)
        self.assertEqual(set(doc), {"scenario_id", "scenario_sha256", "linux_observations_sha256", "oracle", "entries"})
        self.assertEqual(doc["oracle"], {"anki_version": "26.9.3", "driver_sha256": "c" * 64, "lock_sha256": "b" * 64,
                                         "platform": "macos-arm64"})
        self.assertIsNone(ov.staleness(doc, "d" * 64, self.GOLDEN, self.PIN))
        self.assertIsNotNone(ov.staleness(doc, "e" * 64, self.GOLDEN, self.PIN))
        changed = {"observations": {"steps": [{"fsrs_next": {"rows": [[15, 15.5, 5.1]]}}]}}
        self.assertIsNotNone(ov.staleness(doc, "d" * 64, changed, self.PIN))
        self.assertIsNotNone(ov.staleness(doc, "d" * 64, self.GOLDEN, dict(self.PIN, anki_version="26.10.0")))
        self.assertIsNotNone(ov.staleness(doc, "d" * 64, self.GOLDEN, dict(self.PIN, macos_lock_sha256="f" * 64)))
        self.assertIsNotNone(ov.staleness(doc, "d" * 64, self.GOLDEN, {k: v for k, v in self.PIN.items() if k != "macos_lock_sha256"}))
        applied = ov.apply(self.GOLDEN["observations"], doc["entries"])
        self.assertEqual(applied["steps"][0]["fsrs_next"]["rows"][0], [16, 15.5, 5.0])
        self.assertEqual(self.GOLDEN["observations"]["steps"][0]["fsrs_next"]["rows"][0][0], 15)
        with self.assertRaises(ValueError):
            ov.apply(self.GOLDEN["observations"], [dict(doc["entries"][0], linux=14)])
        with self.assertRaises(ValueError):
            ov.apply(self.GOLDEN["observations"], [dict(doc["entries"][0], pointer="/steps/0/fsrs_next/rows/1/0")])


if __name__ == "__main__":
    unittest.main()
