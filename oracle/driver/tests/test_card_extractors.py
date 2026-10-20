# SPDX-License-Identifier: AGPL-3.0-or-later
"""The card_extractors observation (extractor-ops.md E1-E5): rows from Anki's
card-data SQL functions, the storage class of each bound value, infinities and
NULL kept apart, argument checks, and that it never touches the scenario's
collection.

Expected values come from the spec tables of specs/search/card-extractors.md
(X3-X7, X2.2), which the spec writer read from this oracle. r and rr compare
within E4's relative 1e-5, so the test holds on either oracle lane."""

from __future__ import annotations

import math
import re
import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.numbers import f32_value
from driver.observe import card_extractors as module
from driver.session import Session

NOW = 1897293600  # 2030-02-14 10:00:00 UTC
NULL4 = {"s": None, "d": None, "dr": None, "pos": None}
# hex of {"s":5,"d":5,"lrt":1896429600}
OBJECT_HEX = "7b2273223a352c2264223a352c226c7274223a313839363432393630307d"


def inp(ident, data, due=35, ivl=10, today=30, now=NOW):
    return {"id": ident, "data": data, "due": due, "ivl": ivl, "today": today, "now": now}


def request(inputs):
    return {"card_extractors": {"inputs": inputs}}


# (input, the members the spec gives for it)
CASES = [
    # X3, X4, X7: reading cards.data.
    (inp("all-keys", '{"s":2.3,"d":3.7,"dr":0.85,"decay":0.3,"lrt":1896429600,"pos":4}'),
     {"s": 2.3, "d": 3.7, "dr": 0.85, "pos": 4}),
    (inp("empty-object", "{}"), dict(NULL4, r=None)),
    (inp("empty-text", ""), dict(NULL4, r=None)),
    (inp("garbage", "garbage"), dict(NULL4, r=None)),
    (inp("json-null", "null"), dict(NULL4, r=None)),
    (inp("empty-array", "[]"), dict(NULL4, r=None)),
    (inp("s-only", '{"s":5}', ivl=20), {"s": 5.0, "d": None, "dr": None, "pos": None, "r": None}),
    (inp("s-null", '{"s":null,"d":5}'), {"s": None, "d": 5.0, "dr": None, "pos": None}),
    (inp("pos-negative", '{"pos":-1,"s":5,"d":5}'), {"s": 5.0, "d": 5.0, "dr": None, "pos": None}),
    (inp("cd-string", '{"cd":"x","s":5,"d":5,"pos":3}'), {"s": 5.0, "d": 5.0, "dr": None, "pos": 3}),
    (inp("cd-number", '{"s":5,"d":5,"cd":5}', ivl=20), dict(NULL4, r=None)),
    (inp("control-character", '{"s":5,"d":5,"zz":"a\u0001b"}'), dict(NULL4, r=None)),
    (inp("array", "[1,2,3]", ivl=20), {"s": 2.0, "d": 3.0, "dr": None, "pos": 1}),
    (inp("array-seven", '[1,2,3,4,5,1896429600,"{}"]'), {"s": 2.0, "d": 3.0, "dr": 4.0, "pos": 1, "rr": 0.4397356}),
    (inp("s-tenth", '{"s":0.1,"d":5}'), {"s": 0.1}),
    (inp("s-integer-rounding", '{"s":16777217,"d":5}', ivl=20), {"s": 16777216.0}),
    (inp("s-underflow", '{"s":1e-50,"d":5,"lrt":1896429600}'), {"s": 0.0}),
    (inp("s-above-f32", '{"s":1e40,"d":1}'), {"s": "inf", "d": 1.0}),
    (inp("s-below-f32", '{"s":-1e40,"d":1}'), {"s": "-inf", "d": 1.0}),
    (inp("pos-max", '{"pos":4294967295}', due=0, ivl=0), {"pos": 4294967295}),
    (inp("pos-too-large", '{"pos":4294967296}', due=0, ivl=0), {"pos": None}),
    # X5: retrievability.
    (inp("lrt-10-days", '{"s":2.5,"d":5.5,"lrt":1896429600}'), {"r": 0.7182781}),
    (inp("lrt-10-days-1-second-earlier", '{"s":2.5,"d":5.5,"lrt":1896429600}', now=NOW - 1), {"r": 0.71827835}),
    (inp("no-lrt", '{"s":5,"d":5}', ivl=20), {"r": 0.7661308}),
    (inp("due-365001", '{"s":5,"d":5}', due=365001, ivl=0), {"r": 0.031143775}),
    (inp("stability-zero-no-time", '{"s":0,"d":5,"lrt":1897293600}'), {"r": None}),
    (inp("stability-zero", '{"s":0,"d":5,"lrt":1897207200}'), {"r": 0.0}),
    (inp("stability-negative", '{"s":-1,"d":5,"lrt":1896429600}'), {"r": None}),
    # X6: relative retrievability.
    (inp("fsrs-lrt", '{"s":5,"d":5,"dr":0.9,"lrt":1896429600}'), {"rr": -2.0}),
    (inp("fsrs-dr-one", '{"s":5,"d":5,"dr":1,"lrt":1896429600}'), {"rr": "-inf"}),
    (inp("fsrs-dr-one-at-lrt", '{"s":5,"d":5,"dr":1,"lrt":1897293600}'), {"rr": None}),
    (inp("fsrs-not-yet-due", '{"s":5,"d":5,"dr":0.9}', due=36, ivl=1), {"rr": -0.0}),
    (inp("sm2-empty", "{}", ivl=20), {"rr": -0.75005}),
    (inp("sm2-due-i64-min", "{}", due={"int": "-9223372036854775808"}, ivl=1), {"rr": -30.001}),
    # X2.2 and E2: arguments by storage class.
    (inp("data-blob", {"blob_hex": OBJECT_HEX}, ivl=20), dict(NULL4, r=None, rr=-0.75005)),
    (inp("data-text-hex", {"text_hex": OBJECT_HEX}), {"s": 5.0, "d": 5.0, "dr": None, "pos": None}),
    (inp("data-empty-blob", {"blob_hex": ""}), dict(NULL4, r=None)),
    (inp("data-byte-order-mark", {"text_hex": "efbbbf7b2273223a352c2264223a357d"}), dict(NULL4, r=None)),
    (inp("data-invalid-utf8", {"text_hex": OBJECT_HEX + "ff"}), dict(NULL4, r=None)),
    (inp("data-integer", 5, ivl=20), dict(NULL4, r=None, rr=-0.75005)),
    (inp("data-real", 5.5, ivl=20), dict(NULL4, r=None, rr=-0.75005)),
    (inp("data-null", None, ivl=20), dict(NULL4, r=None, rr=-0.75005)),
    (inp("due-real-lrt", '{"s":5,"d":5,"lrt":1896429600}', due=35.0), {"s": 5.0, "r": None, "rr": None}),
    (inp("due-text", '{"s":5,"d":5}', due="35", ivl=20), {"r": None, "rr": None}),
    (inp("today-real", '{"s":5,"d":5,"dr":0.9}', ivl=20, today=30.0), {"r": None, "rr": None}),
    (inp("now-null-sm2", "{}", ivl=20, now=None), {"rr": None}),
]


class CardExtractors(unittest.TestCase):
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
        sc = {"id": "t/card-extractors", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        if setup:
            sc["setup"] = setup
        s = Session(sc, "UTC", self.tmp.name, self.clock)
        try:
            return s.run_all()["steps"]
        finally:
            s.close()

    def observe(self, inputs):
        return self.run_steps([{"observe": [request(inputs)]}])[0]["card_extractors"]["rows"]

    def assert_member(self, ident, member, got, want):
        where = f"{ident}.{member}"
        if member in ("r", "rr") and isinstance(want, float) and want != 0 and isinstance(got, float):
            self.assertLessEqual(abs(got - want), 1e-5 * abs(want), where)  # E4
            return
        self.assertEqual(type(got), type(want), where)
        self.assertEqual(got, want, where)
        if isinstance(want, float) and want == 0:  # -0.0 and 0.0 differ (X3 rule 5)
            self.assertEqual(math.copysign(1, got), math.copysign(1, want), where)

    def test_rows_are_the_spec_values_in_input_order(self):
        rows = self.observe([case for case, _ in CASES])
        self.assertEqual([row["id"] for row in rows], [case["id"] for case, _ in CASES])
        for row, (case, want) in zip(rows, CASES):
            self.assertEqual(set(row), {"id", "s", "d", "dr", "r", "rr", "pos"}, case["id"])
            for member, value in want.items():
                self.assert_member(case["id"], member, row[member], value)

    def test_every_real_is_a_shortest_f32_decimal(self):
        rows = self.observe([case for case, _ in CASES])
        for row in rows:
            for member in ("s", "d", "dr", "r", "rr"):
                value = row[member]
                if isinstance(value, float):
                    self.assertEqual(f32_value(value), value, f"{row['id']}.{member}")  # O6
                else:
                    self.assertIn(value, (None, "inf", "-inf"), f"{row['id']}.{member}")
            self.assertTrue(row["pos"] is None or type(row["pos"]) is int, row["id"])

    def test_the_memory_state_extractor_only_gets_its_three_keys(self):
        # Anki panics on any other key, which kills the worker.
        ((_, sql, _),) = module.check_request(request([inp("x", "{}")])["card_extractors"])
        self.assertEqual(re.findall(r"extract_fsrs_variable\(([^)]*)\)", sql), ["?1, 's'", "?1, 'd'", "?1, 'dr'"])
        self.assertEqual(len(re.findall(r"extract_fsrs_\w*retrievability\(\?1, \?2, \?3, \?4, 0, \?5\)", sql)), 2)

    def test_a_real_that_is_not_a_widened_f32_is_a_run_error(self):
        self.assertEqual(module.real_value(-module.MAX, "rr"), "-inf")
        for value in (0.1, 2.3, 1e300, -3.5e38, 5):
            with self.subTest(value=value), self.assertRaises(DriverError) as ctx:
                module.real_value(value, "s")
            self.assertEqual(ctx.exception.kind, "internal_error")

    def test_the_scratch_collection_is_kept_and_the_scenario_collection_untouched(self):
        setup = {
            "notetypes": [{"name": "QA", "fields": ["Q", "A"], "templates": [{"name": "C1", "front": "{{Q}}", "back": "{{A}}"}]}],
            "notes": [{"notetype": "QA", "fields": ["q", "a"]}],
        }
        steps = [{"observe": ["card(#c1)", "ids"]}, {"observe": [request([inp("a", "{}")])]},
                 {"observe": ["card(#c1)", "ids"]}]
        out = self.run_steps(steps, setup=setup)
        self.assertEqual(out[0], out[2])
        scratch = module._scratch
        self.observe([inp("b", "{}")])
        self.assertIs(module._scratch, scratch)
        self.assertEqual(scratch.col.db.scalar("select count() from cards"), 0)

    def test_bad_requests_are_invalid(self):
        good = inp("a", "{}")
        bad_requests = [
            {},
            {"inputs": [good], "extra": 1},
            {"inputs": []},
            {"inputs": [inp(f"i{i}", "{}") for i in range(module.MAX_INPUTS + 1)]},
            {"inputs": good},
            {"inputs": [[good]]},
            {"inputs": [dict(good, extra=1)]},
            {"inputs": [{k: v for k, v in good.items() if k != "now"}]},
            {"inputs": [dict(good, id="")]},
            {"inputs": [dict(good, id=1)]},
            {"inputs": [good, dict(good, due=36)]},
            {"inputs": [dict(good, data=True)]},
            {"inputs": [dict(good, due=[35])]},
            {"inputs": [dict(good, due=2**53 + 1)]},
            {"inputs": [dict(good, due={"int": "9223372036854775808"})]},
            {"inputs": [dict(good, due={"int": "-9223372036854775809"})]},
            {"inputs": [dict(good, due={"int": "12345678901234567890"})]},
            {"inputs": [dict(good, due={"int": "1.5"})]},
            {"inputs": [dict(good, due={"int": "+1"})]},
            {"inputs": [dict(good, due={"int": 35})]},
            {"inputs": [dict(good, data={"text_hex": "7b2"})]},
            {"inputs": [dict(good, data={"text_hex": "7g"})]},
            {"inputs": [dict(good, data={"blob_hex": "7b 7d"})]},
            {"inputs": [dict(good, data={"blob_hex": 7})]},
            {"inputs": [dict(good, data={"text_hex": "7b", "blob_hex": "7b"})]},
            {"inputs": [dict(good, data={"hex": "7b"})]},
            [good],
        ]
        for args in bad_requests:
            with self.subTest(args=str(args)[:120]), self.assertRaises(DriverError) as ctx:
                self.run_steps([{"observe": [{"card_extractors": args}]}])
            self.assertEqual(ctx.exception.kind, "invalid_scenario")
        # Twice in one observe step, rejected before any collection exists.
        with self.assertRaises(DriverError) as ctx:
            self.run_steps([{"observe": [request([good]), request([inp("b", "{}")])]}])
        self.assertEqual((ctx.exception.kind, ctx.exception.fields.get("step")), ("invalid_scenario", 0))
        self.assertIsNone(module._scratch)
        # The limits themselves are valid, and the same id may return in another step.
        edges = [inp(f"i{i}", "{}") for i in range(module.MAX_INPUTS - 2)] + [
            inp("i64-max", "{}", due={"int": "9223372036854775807"}, now={"int": "-0"}),
            inp("exact-max", {"blob_hex": "aAfF"}, due=2**53, ivl=-(2**53)),
        ]
        out = self.run_steps([{"observe": [request(edges)]}, {"observe": [request([inp("i0", "{}")])]}])
        self.assertEqual(len(out[0]["card_extractors"]["rows"]), module.MAX_INPUTS)
        self.assertEqual(out[1]["card_extractors"]["rows"][0]["id"], "i0")


if __name__ == "__main__":
    unittest.main()
