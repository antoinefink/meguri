# SPDX-License-Identifier: AGPL-3.0-or-later
"""The studied-today observations (specs/conformance/stats-ops.md H2, H5-H7)
through real workers. Expected lines come from stats/studied-today T2 (two of
them are Anki's own unit-test lines) and expected values from the rows each
test writes and the rules of T2.1 and T3, worked by hand; never from the
driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, next_rollover, run_ok, scenario

TZ = "Europe/Paris"
START = "2030-01-15T10:00:00"
# Day 0 starts 86,400 s before the next rollover (04:00 local, the default).
S = (next_rollover(epoch(START, TZ), TZ) - 86_400) * 1000
MINUTE = 60_000
DAY = 86_400_000
ZERO_LINE = "Studied 0 cards in 0 seconds today (0s/card)"
FIELDS = ("answers", "time_ms", "correct", "learn", "review", "relearn", "filtered", "mature", "mature_correct")


def revlog(*rows):
    """INSERT for (id ms, cid SQL, type, ease, lastIvl, time ms) rows."""
    values = ", ".join(
        f"({i}, {cid}, -1, {ease}, 1, {last}, 2500, {t}, {kind})" for i, cid, kind, ease, last, t in rows
    )
    return {"db_exec": {"sql": f"INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES {values}"}}


def card_of(front):
    return f"(SELECT id FROM cards WHERE nid = (SELECT id FROM notes WHERE sfld = '{front}'))"


def today(**values):
    return {name: values.get(name, 0) for name in FIELDS}


# Anki writes the config key localOffset the first time it computes the day in a
# zone (any day computation, `timing` included). Steps that compare dumps
# around an observation compute the day first, so the dumps show only what the
# observation itself changes.
WARM = {"observe": ["timing"]}


def error_of(worker, steps, setup=None):
    resp = worker.request({"cmd": "run", "scenario": scenario("t/today", TZ, START, steps, setup=setup)})
    return resp.get("error")


# Today's rows on card id 1, which no card has (T3.3 worked by hand):
# answers 13, time 91,000 ms, correct 8 (ease above 1), learn 1, review 3,
# relearn 4, filtered 5, mature 6 (lastIvl 21 or more), mature correct 2. Every
# value differs, so a field read from the wrong member shows. The Home line
# leaves out the row at S: 12 rows, 90,000 ms.
MIX = revlog(
    (S - 1, 1, 1, 3, 30, 100_000),  # yesterday
    (S, 1, 0, 3, 0, 1_000),  # Learn, at the start of the day
    (S + 1 * MINUTE, 1, 1, 3, 21, 2_000),  # Review, mature, correct
    (S + 2 * MINUTE, 1, 1, 1, 30, 3_000),  # Review, mature
    (S + 2 * DAY, 1, 1, 3, 5, 4_000),  # Review, after the next day start
    (S + 3 * MINUTE, 1, 2, 1, 25, 5_000),  # Relearn, mature
    (S + 4 * MINUTE, 1, 2, 3, -600, 6_000),  # Relearn
    (S + 5 * MINUTE, 1, 2, 2, 1, 7_000),  # Relearn
    (S + 6 * MINUTE, 1, 2, 1, 21, 8_000),  # Relearn, mature
    (S + 7 * MINUTE, 1, 3, 3, 40, 9_000),  # Filtered, mature, correct
    (S + 8 * MINUTE, 1, 3, 0, 0, 10_000),  # Filtered, no rating
    (S + 9 * MINUTE, 1, 3, 2, 5, 11_000),  # Filtered
    (S + 10 * MINUTE, 1, 3, 4, 5, 12_000),  # Filtered
    (S + 11 * MINUTE, 1, 3, 0, 21, 13_000),  # Filtered, mature, no rating
    (S + 12 * MINUTE, 1, 4, 0, 50, 200_000),  # Manual
    (S + 13 * MINUTE, 1, 5, 0, 30, 300_000),  # Rescheduled
)
MIX_TODAY = today(answers=13, time_ms=91_000, correct=8, learn=1, review=3, relearn=4, filtered=5, mature=6, mature_correct=2)


class StudiedTodayRaw(unittest.TestCase):
    def test_counts_rows_after_the_day_start_except_manual_and_rescheduled_ones(self):
        steps = [
            revlog(
                (S - 1, 1, 1, 3, 0, 100),  # before day 0
                (S, 1, 1, 3, 0, 200),  # at its start: not after it
                (S + 1, 1, 1, 3, 0, 1_000),  # a deleted card's row counts
                (S + 2, 1, 3, 0, 0, 2_000),  # an unrated Filtered row counts
                (S + 3, 1, 4, 0, 0, 0),  # Manual
                (S + 4, 1, 4, 2, 0, 5_000),  # a rated Manual row
                (S + 5, 1, 5, 0, 0, 0),  # Rescheduled
                (S + 6, 1, 1, 4, 0, 900_000),  # stored time as is
                (S + 3 * DAY, 1, 0, 3, 0, 4_000),  # after now and after the next day start
            ),
            {"observe": ["studied_today_raw"]},
            {"advance": "1d"},
            {"observe": ["studied_today_raw"]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/today", TZ, START, steps))
        self.assertEqual(out[1], {"studied_today_raw": {"cards": 4, "time_ms": 907_000}})
        # A day later only the future row is after the new start.
        self.assertEqual(out[3], {"studied_today_raw": {"cards": 1, "time_ms": 4_000}})

    def test_takes_no_arguments(self):
        with WorkerProcess(TZ) as w:
            for spec in ({"studied_today_raw": {}}, {"studied_today_raw": []}, "studied_today_raw(#c1)"):
                with self.subTest(spec=spec):
                    resp = w.request({"cmd": "run", "scenario": scenario("t/today", TZ, START, [{"observe": [spec]}], notes=1)})
                    err = resp.get("error")
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), resp)


class StudiedTodayText(unittest.TestCase):
    def test_the_home_line_in_english_with_spaces_for_line_breaks(self):
        steps = [
            {"observe": ["studied_today_text"]},
            revlog(
                (S - 1, 1, 1, 3, 30, 500),  # yesterday
                (S, 1, 1, 3, 30, 700),  # at the start: not after it
                (S + 1, 1, 1, 3, 30, 4_000),
                (S + 2, 1, 0, 1, 0, 4_000),
                (S + 3, 1, 3, 0, 0, 5_000),
                (S + 4, 1, 4, 0, 0, 9_000),  # Manual
                (S + 5, 1, 5, 0, 30, 9_000),  # Rescheduled
            ),
            {"observe": ["studied_today_text"]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/today", TZ, START, steps))
        self.assertEqual(out[0], {"studied_today_text": ZERO_LINE})
        self.assertEqual(out[2], {"studied_today_text": "Studied 3 cards in 13 seconds today (4.33s/card)"})

    def test_minutes_from_sixty_seconds_on(self):
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/today", TZ, START, [MIX, {"observe": ["studied_today_text"]}]))
        self.assertEqual(out[1], {"studied_today_text": "Studied 12 cards in 1.5 minutes today (7.5s/card)"})

    def test_takes_no_arguments(self):
        with WorkerProcess(TZ) as w:
            for spec in ({"studied_today_text": {}}, {"studied_today_text": []}, "studied_today_text(#c1)"):
                with self.subTest(spec=spec):
                    err = error_of(w, [{"observe": [spec]}])
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)


class StudiedTodayFormat(unittest.TestCase):
    INPUTS = [[3, 13_000], [300, 5_400_000], [0, 0], [1, 60_000], [1, -1_000], [2, 1_000]]
    LINES = [
        "Studied 3 cards in 13 seconds today (4.33s/card)",
        "Studied 300 cards in 90 minutes today (18s/card)",
        ZERO_LINE,
        "Studied 1 card in 1 minute today (60s/card)",
        "Studied 1 card in -1 second today (-1s/card)",
        "Studied 2 cards in 1 second today (0.5s/card)",
    ]

    def test_lines_for_given_numbers_in_request_order(self):
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/today", TZ, START, [{"observe": [{"studied_today_format": {"inputs": self.INPUTS}}]}]))
        self.assertEqual(out[0], {"studied_today_format": self.LINES})

    def test_the_collection_neither_matters_nor_changes(self):
        fmt = {"studied_today_format": {"inputs": self.INPUTS}}
        steps = [
            MIX,
            {"observe": ["db_dump"]},
            {"observe": [fmt]},
            {"observe": ["db_dump"]},
            {"set_config": {"key": "rollover", "value": 12}},
            {"observe": [fmt]},
        ]
        with WorkerProcess("Asia/Kathmandu") as w:
            out = run_ok(self, w, scenario("t/today", "Asia/Kathmandu", START, steps, notes=2))
        self.assertEqual(out[2]["studied_today_format"], self.LINES)
        self.assertEqual(out[5]["studied_today_format"], self.LINES)
        # Not even the day is computed: localOffset stays unwritten.
        self.assertEqual(out[1], out[3])

    def test_arguments_are_checked(self):
        bad = [
            None,
            [],
            {},
            {"inputs": []},
            {"inputs": [[1, 1]], "extra": 1},
            {"inputs": [1, 1]},
            {"inputs": [[1]]},
            {"inputs": [[1, 1, 1]]},
            {"inputs": [[True, 1]]},
            {"inputs": [[1, False]]},
            {"inputs": [[1.0, 1]]},
            {"inputs": [[1, 1.0]]},
            {"inputs": [["1", 1]]},
            {"inputs": [[1, None]]},
            {"inputs": [[-1, 1]]},
            {"inputs": [[2**32, 1]]},
            {"inputs": [[1, 2**53]]},
            {"inputs": [[1, -(2**53)]]},
            {"inputs": [[1, 1], [1, 2**53]]},
        ]
        with WorkerProcess(TZ) as w:
            for args in bad:
                with self.subTest(args=args):
                    err = error_of(w, [{"observe": [{"studied_today_format": args}]}])
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)
            # The bounds themselves are valid.
            ok = {"studied_today_format": {"inputs": [[2**32 - 1, 2**53 - 1], [0, -(2**53 - 1)]]}}
            resp = w.request({"cmd": "run", "scenario": scenario("t/today", TZ, START, [{"observe": [ok]}])})
            self.assertTrue(resp["ok"], resp)
            self.assertEqual(len(resp["observations"]["steps"][0]["studied_today_format"]), 2)


class TodayStats(unittest.TestCase):
    def test_values_of_the_rows_from_the_day_start_on(self):
        steps = [{"observe": [{"today_stats": ["collection"]}]}, MIX, {"observe": [{"today_stats": ["collection", {"search": ""}]}]}]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/today", TZ, START, steps))
        self.assertEqual(out[0], {"today_stats": [today()]})
        self.assertEqual(out[2], {"today_stats": [MIX_TODAY, MIX_TODAY]})

    SCOPES = {
        "decks": [{"name": "A", "preset": "P"}, {"name": "A::B", "preset": "P"}, {"name": "x_y", "preset": "P"}, {"name": "xzy", "preset": "P"}],
        "notes": [
            {"notetype": "QA", "deck": "A", "fields": ["q1", "a"]},
            {"notetype": "QA", "deck": "A::B", "fields": ["q2", "a"]},
            {"notetype": "QA", "deck": "x_y", "fields": ["q3", "a"]},
            {"notetype": "QA", "deck": "xzy", "fields": ["q4", "a"]},
        ],
    }
    # One Review row each, time as a bit per card: 1 s q1, 2 s q2, 4 s q3,
    # 8 s q4, 16 s card id 1 (no card).
    SCOPE_ROWS = revlog(
        (S + 1 * MINUTE, card_of("q1"), 1, 3, 5, 1_000),
        (S + 2 * MINUTE, card_of("q2"), 1, 3, 5, 2_000),
        (S + 3 * MINUTE, card_of("q3"), 1, 3, 5, 4_000),
        (S + 4 * MINUTE, card_of("q4"), 1, 3, 5, 8_000),
        (S + 5 * MINUTE, 1, 1, 3, 5, 16_000),
    )

    @staticmethod
    def reviews(n, ms):
        return today(answers=n, time_ms=ms, correct=n, review=n)

    def test_scopes_collection_deck_subtree_and_search_text(self):
        scopes = [
            "collection",
            {"deck": "A"},
            {"deck": "A::B"},
            {"deck": "x_y"},  # a deck node escapes _, so xzy stays out
            {"deck": "xzy"},
            {"search": ""},
            {"search": " \t "},
            {"search": "deck:*"},
            {"search": "deck:A -deck:A::B"},
            {"search": "deck:x_y"},  # as written: _ matches any character
        ]
        steps = [self.SCOPE_ROWS, {"observe": [{"today_stats": scopes}]}]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/today", TZ, START, steps, setup=self.SCOPES))
        r = self.reviews
        self.assertEqual(
            out[1]["today_stats"],
            [r(5, 31_000), r(2, 3_000), r(1, 2_000), r(1, 4_000), r(1, 8_000),
             r(5, 31_000), r(5, 31_000), r(4, 15_000), r(1, 1_000), r(2, 12_000)],
        )

    def test_a_missing_deck_is_unknown_ref_and_a_refused_search_anki_error(self):
        with WorkerProcess(TZ) as w:
            for scopes, ref in (([{"deck": "Nope"}], "Nope"), (["collection", {"deck": "a"}], "a"), ([{"deck": "A::"}], "A::")):
                with self.subTest(scopes=scopes):
                    err = error_of(w, [self.SCOPE_ROWS, {"observe": [{"today_stats": scopes}]}], setup=self.SCOPES)
                    self.assertEqual((err["kind"], err.get("ref"), err.get("step")), ("unknown_ref", ref, 1), err)
            err = error_of(w, [{"observe": [{"today_stats": [{"search": "deck:A ("}]}]}])
            self.assertEqual((err["kind"], err.get("step")), ("anki_error", 0), err)

    def test_arguments_are_checked(self):
        bad = [
            None,
            "collection",
            [],
            ["Collection"],
            [["collection"]],
            [{}],
            [{"deck": ""}],
            [{"deck": 1}],
            [{"deck": None}],
            [{"search": None}],
            [{"search": 1}],
            [{"deck": "A", "search": ""}],
            [{"collection": None}],
            ["collection", {"dek": "A"}],
        ]
        with WorkerProcess(TZ) as w:
            for args in bad:
                with self.subTest(args=args):
                    err = error_of(w, [{"observe": [{"today_stats": args}]}])
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)

    def test_read_only(self):
        steps = [
            self.SCOPE_ROWS,
            WARM,
            {"observe": ["db_dump"]},
            {"observe": ["studied_today_text", {"today_stats": ["collection", {"deck": "A"}, {"search": "deck:x_y"}]}]},
            {"observe": ["db_dump"]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/today", TZ, START, steps, setup=self.SCOPES))
        self.assertEqual(out[2], out[4])


if __name__ == "__main__":
    unittest.main()
