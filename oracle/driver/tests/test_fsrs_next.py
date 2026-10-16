# SPDX-License-Identifier: AGPL-3.0-or-later
"""The fsrs_next observation (float-determinism.md F3, F5): rows, the stored
card each input becomes, argument checks, and that it never touches the
scenario's collection.

Expected values come from the spec: F3.5 gives input 4 of
fp-probe/next-states-edges (the review card of smoke/fsrs-new-card step 6) and
its row; the card fields and elapsed days are F3.1 and sched/fsrs-transitions
T2.3, computed here from the scratch collection's own timing."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.numbers import f32_value, to_f32
from driver.observe import fsrs_next as module
from driver.session import Session

DEFAULT: list = []
FSRS6_A = [0.1544, 0.8813, 2.9105, 11.2071, 6.8114, 0.7103, 2.6541, 0.0052, 1.6812, 0.1801, 0.8724, 1.5932, 0.0712,
           0.2891, 1.7524, 0.5103, 2.0542, 0.4817, 0.1123, 0.0841, 0.1305]
FSRS5_A = FSRS6_A[:19]
FSRS4_A = FSRS6_A[:17]

# F3.5: the review card of smoke/fsrs-new-card step 6 and its next states.
SMOKE_INPUT = {"id": 4, "card_id": 1894698000000, "reps": 2, "s": 2.3065, "d": 2.111, "elapsed_days": 3,
               "scheduled_days": 2, "dr": 0.9, "params": "default", "max_ivl": 36500}
SMOKE_ROW = [1, 0.6369821, 7.3921676, 10, 9.240417, 4.7481427, 15, 13.836124, 2.1041174, 26, 23.900333, 1.0]


def make_input(i, **kw):
    base = {"id": i, "card_id": 1700000000000 + i, "reps": 5, "s": 10.0, "d": 5.0, "elapsed_days": 10,
            "scheduled_days": 10, "dr": 0.9, "params": "default", "max_ivl": 36500}
    base.update(kw)
    return base


def request(inputs, sets=None):
    return {"fsrs_next": {"param_sets": sets or {"default": DEFAULT}, "inputs": inputs}}


class FsrsNext(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        module._scratch = None  # a fresh scratch collection per test
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        if module._scratch is not None:
            module._scratch.col.close(downgrade=False)
            module._scratch = None
        self.tmp.cleanup()

    def run_steps(self, steps, setup=None):
        sc = {"id": "t/fsrs-next", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        if setup:
            sc["setup"] = setup
        s = Session(sc, "UTC", self.tmp.name, self.clock)
        try:
            return s.run_all()["steps"]
        finally:
            s.close()

    def test_the_smoke_review_card_gives_its_golden_next_states(self):
        out = self.run_steps([{"observe": [request([make_input(0), SMOKE_INPUT])]}])
        rows = out[0]["fsrs_next"]["rows"]
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[1], SMOKE_ROW)

    def test_one_row_of_twelve_values_per_input_in_input_order(self):
        sets = {"default": DEFAULT, "fsrs6-a": FSRS6_A, "fsrs5-a": FSRS5_A, "fsrs4-a": FSRS4_A}
        inputs = [make_input(i, params=p, max_ivl=m) for i, (p, m) in
                  enumerate([("default", 36500), ("fsrs6-a", 100), ("fsrs5-a", 3650), ("fsrs4-a", 30), ("default", 1)])]
        rows = self.run_steps([{"observe": [request(inputs, sets)]}])[0]["fsrs_next"]["rows"]
        self.assertEqual(len(rows), len(inputs))
        for inp, row in zip(inputs, rows):
            self.assertEqual(len(row), 12)
            for col in (0, 3, 6, 9):
                self.assertIs(type(row[col]), int)
                self.assertTrue(1 <= row[col], row)
            for col in (3, 6, 9):  # the maximum interval holds Hard, Good and Easy (T5)
                self.assertLessEqual(row[col], inp["max_ivl"], row)
            for col in (1, 2, 4, 5, 7, 8, 10, 11):
                self.assertIs(type(row[col]), float)
                self.assertEqual(f32_value(row[col]), row[col])  # a shortest f32 decimal (O6)
        self.assertEqual(rows[4][3:12:3], [1, 1, 1])
        # Evaluating the same inputs again gives the same rows.
        again = self.run_steps([{"observe": [request(inputs, sets)]}])[0]["fsrs_next"]["rows"]
        self.assertEqual(again, rows)

    def test_each_input_is_one_stored_review_card(self):
        sets = {"fsrs6-a": FSRS6_A}
        inp = make_input(7, params="fsrs6-a", s=12.5, d=6.25, elapsed_days=40, scheduled_days=30, reps=11,
                         card_id=1600000000123, dr=0.85, max_ivl=365)
        self.run_steps([{"observe": [request([inp], sets)]}])
        col = module._scratch.col
        timing = col._backend.sched_timing_today()
        row = col.db.first("select id, did, type, queue, ivl, due, factor, reps, lapses, left, odid, odue, data from cards")
        cid, did, ctype, queue, ivl, due, factor, reps, lapses, left, odid, odue, data = row
        self.assertEqual((cid, ctype, queue, ivl, due, factor, reps, lapses, left, odid, odue),
                         (1600000000123, 2, 2, 30, timing.days_elapsed, 2500, 11, 0, 0, 0, 0))
        data = json.loads(data)
        self.assertEqual((data["s"], data["d"]), (12.5, 6.25))
        self.assertEqual((timing.next_day_at - data["lrt"]) // 86400, 40)
        self.assertLessEqual(data["lrt"], self.clock.now)
        self.assertFalse(col._backend.get_deck(did).normal.HasField("desired_retention"))
        conf = col.decks.config_dict_for_deck_id(did)
        self.assertEqual([to_f32(x) for x in conf["fsrsParams6"]], [to_f32(x) for x in FSRS6_A])
        self.assertEqual((conf["fsrsParams5"], conf["fsrsWeights"]), ([], []))
        self.assertEqual(to_f32(conf["desiredRetention"]), to_f32(0.85))
        self.assertEqual(conf["rev"]["maxIvl"], 365)
        self.assertEqual((conf["lapse"]["delays"], conf["new"]["delays"], conf["lapse"]["leechFails"]), ([10.0], [1.0, 10.0], 8))
        self.assertIs(col.get_config("fsrs", False), True)
        self.assertIs(col.get_config("loadBalancerEnabled", True), False)
        self.assertIs(col.get_config("fsrsShortTermWithStepsEnabled", True), False)

    def test_a_day_change_between_placing_and_reading_evaluates_again(self):
        steady = self.run_steps([{"observe": [request([SMOKE_INPUT])]}])[0]["fsrs_next"]["rows"]
        backend = module._scratch.col._backend
        real = type(backend).sched_timing_today
        calls = []

        def rolling(self_):
            t = real(self_)
            calls.append(t.next_day_at)
            if len(calls) == 2:  # the read after the first evaluation sees a new day
                t.next_day_at += 86400
            return t

        backend.sched_timing_today = rolling.__get__(backend)
        try:
            rows = self.run_steps([{"observe": [request([SMOKE_INPUT])]}])[0]["fsrs_next"]["rows"]
        finally:
            del backend.sched_timing_today
        self.assertEqual(len(calls), 4)
        self.assertEqual(rows, steady)

    def test_the_scenario_collection_is_untouched(self):
        setup = {
            "notetypes": [{"name": "QA", "fields": ["Q", "A"], "templates": [{"name": "C1", "front": "{{Q}}", "back": "{{A}}"}]}],
            "notes": [{"notetype": "QA", "fields": ["q", "a"]}],
        }
        steps = [{"observe": ["card(#c1)", "ids"]}, {"observe": [request([SMOKE_INPUT])]}, {"observe": ["card(#c1)", "ids"]}]
        out = self.run_steps(steps, setup=setup)
        self.assertEqual(out[0], out[2])

    def test_bad_requests_are_invalid(self):
        good = make_input(0)
        too_many = [make_input(i) for i in range(module.MAX_INPUTS + 1)]
        bad_requests = [
            {"param_sets": {"default": []}},
            {"param_sets": {}, "inputs": [good]},
            {"param_sets": {"Default": []}, "inputs": [dict(good, params="Default")]},
            {"param_sets": {"default": [0.1] * 18}, "inputs": [good]},
            {"param_sets": {"default": [True] * 17}, "inputs": [good]},
            {"param_sets": {"default": []}, "inputs": []},
            {"param_sets": {"default": []}, "inputs": too_many},
            {"param_sets": {"default": []}, "inputs": [dict(good, params="other")]},
            {"param_sets": {"default": []}, "inputs": [dict(good, extra=1)]},
            {"param_sets": {"default": []}, "inputs": [{k: v for k, v in good.items() if k != "reps"}]},
            {"param_sets": {"default": []}, "inputs": [dict(good, card_id=0)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, card_id=2**53)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, card_id=1.0e12)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, reps=True)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, reps=4294967295)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, s=0.0009)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, s="10")]},
            {"param_sets": {"default": []}, "inputs": [dict(good, d=10.5)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, elapsed_days=20001)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, scheduled_days=0)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, dr=1)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, dr=0)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, max_ivl=36501)]},
            {"param_sets": {"default": []}, "inputs": [dict(good, id=-1)]},
            [good],
        ]
        for args in bad_requests:
            with self.subTest(args=str(args)[:120]), self.assertRaises(DriverError) as ctx:
                self.run_steps([{"observe": [{"fsrs_next": args}]}])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")
        # Twice in one observe step.
        with self.assertRaises(DriverError) as ctx:
            self.run_steps([{"observe": [request([good]), request([good])]}])
        self.assertEqual(ctx.exception.kind, "invalid_scenario")
        # The limits themselves are valid.
        edges = [make_input(i) for i in range(module.MAX_INPUTS - 1)] + [
            dict(good, id=999, card_id=2**53 - 1, reps=4294967294, s=36500, d=1, elapsed_days=20000, scheduled_days=36500,
                 dr=0.99, max_ivl=1)]
        rows = self.run_steps([{"observe": [request(edges)]}])[0]["fsrs_next"]["rows"]
        self.assertEqual(len(rows), module.MAX_INPUTS)


if __name__ == "__main__":
    unittest.main()
