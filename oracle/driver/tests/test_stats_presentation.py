# SPDX-License-Identifier: AGPL-3.0-or-later
"""stats_presentation (specs/conformance/stats-ops.md H14,
stats/graph-presentation P22) through real workers on the graphs-page image
(oracle/graphs-page/Dockerfile). Expected values are P6's and P9's rules
worked by hand for the cards each test writes, never taken from the driver.

They need the graphs-page image, which sets ORACLE_GRAPHS_PAGE; elsewhere
they skip (oracle/graphs-page/README.md)."""

from __future__ import annotations

import os
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_stats import START, TZ, WARM, error_of
from driver.tests.test_worker import run_ok, scenario

PAGE = os.environ.get("ORACLE_GRAPHS_PAGE")
FUTURE = {"scope": "collection", "history": "year", "graph": "future_due"}


def due(offset: int, count: int, day: int) -> dict:
    sql = (f"UPDATE cards SET type = 2, queue = 2, ivl = 10, due = {day} "
           f"WHERE id IN (SELECT id FROM cards ORDER BY id LIMIT {count} OFFSET {offset})")
    return {"db_exec": {"sql": sql}}


@unittest.skipUnless(PAGE, "needs the graphs-page image (ORACLE_GRAPHS_PAGE)")
class FutureDue(unittest.TestCase):
    # future_due {0: 1, 1: 2, 31: 1}: Backlog off, 1 month gives the domain
    # [0, 31], 31 + 1 bins (P4.1 step 1), the last [31, 31] undrawn (P4.4).
    STEPS = [due(0, 1, 0), due(1, 2, 1), due(3, 1, 31)]

    def test_bins_tooltips_queries_and_table(self):
        steps = self.STEPS + [{"observe": [{"stats_presentation": [
            dict(FUTURE, controls={"range": "1 month", "backlog": False}),
        ]}]}]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/presentation", TZ, START, steps, notes=4))
        (value,) = out[-1]["stats_presentation"]
        self.assertEqual(value["data"]["future_due"]["future_due"], {"0": 1, "1": 2, "31": 1})
        page = value["page"]
        self.assertEqual(page["no_data"], False)
        bins = page["bins"]
        self.assertEqual(len(bins), 32)
        self.assertEqual(bins[0], {"x0": 0, "x1": 1, "value": 1, "drawn": True,
                                   "tooltip": ["Today:", "1 card due", "Running total: 1"], "query": '"prop:due=0"'})
        self.assertEqual(bins[1]["tooltip"], ["Tomorrow:", "2 cards due", "Running total: 3"])
        self.assertEqual(bins[2], {"x0": 2, "x1": 3, "value": 0, "drawn": True,
                                   "tooltip": ["In 2 days:", "0 cards due", "Running total: 3"], "query": None})
        self.assertEqual(bins[30]["tooltip"][0], "In 30-31 days:")
        self.assertEqual(bins[30]["query"], None)
        self.assertEqual(bins[31], {"x0": 31, "x1": 31, "value": 1, "drawn": False, "tooltip": None, "query": None})
        # Average round(4 / 31); the Daily load row reads the record's value.
        self.assertEqual(page["table"][:3], [["Total", "4 reviews"], ["Average", "0 reviews/day"], ["Due tomorrow", "2 reviews"]])
        self.assertEqual((len(page["table"]), page["table"][3][0]), (4, "Daily load"))

    def test_backlog_is_the_request_value_and_nothing_is_written(self):
        # A card due on -2: Backlog on starts the domain there; the stored
        # key stays true while the page draws with false, and the
        # observation writes no row (G11).
        steps = self.STEPS + [
            due(0, 1, -2),
            {"set_config": {"key": "futureDueShowBacklog", "value": True}},
            WARM,
            {"observe": ["db_dump"]},
            {"observe": [{"stats_presentation": [
                dict(FUTURE, controls={"range": "1 month", "backlog": True}),
                dict(FUTURE, controls={"range": "1 month", "backlog": False}),
            ]}]},
            {"observe": ["db_dump"]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/presentation", TZ, START, steps, notes=4))
        self.assertEqual(out[-3], out[-1])
        on, off = out[-2]["stats_presentation"]
        self.assertEqual((on["page"]["bins"][0]["x0"], on["page"]["bins"][0]["tooltip"][0]), (-2, "2 days ago:"))
        self.assertEqual(off["page"]["bins"][0]["x0"], 0)

    def test_an_empty_collection_shows_no_data(self):
        steps = [{"observe": [{"stats_presentation": [dict(FUTURE, controls={"range": "1 month", "backlog": True})]}]}]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/presentation", TZ, START, steps))
        self.assertEqual(out[0]["stats_presentation"], [{"data": {"future_due": {}}, "page": {"no_data": True, "bins": [], "table": []}}])


@unittest.skipUnless(PAGE, "needs the graphs-page image (ORACLE_GRAPHS_PAGE)")
class CardCounts(unittest.TestCase):
    def test_rows_and_sectors(self):
        # Two new cards and one suspended new card: separating, New 2 and
        # Suspended 1 of 3 (P9: 66.67% and 33.33%); not separating, New 3.
        steps = [
            {"db_exec": {"sql": "UPDATE cards SET queue = -1 WHERE id = (SELECT max(id) FROM cards)"}},
            {"observe": [{"stats_presentation": [
                {"scope": "collection", "history": "year", "graph": "card_counts", "controls": {"separate": True}},
                {"scope": "collection", "history": "year", "graph": "card_counts", "controls": {"separate": False}},
            ]}]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/presentation", TZ, START, steps, notes=3))
        on, off = (v["page"] for v in out[1]["stats_presentation"])
        self.assertEqual(on["sectors"], [2, 0, 0, 0, 0, 1, 0])
        self.assertEqual(on["table"][0], ["New", "2", "66.67%"])
        self.assertEqual(on["table"][5], ["Suspended", "1", "33.33%"])
        self.assertEqual(on["table"][-1], ["Total", "3"])
        self.assertEqual(len(on["table"]), 8)
        self.assertEqual(off["sectors"], [3, 0, 0, 0, 0, 0, 0])
        self.assertEqual(off["table"][0], ["New", "3", "100%"])
        self.assertEqual(len(off["table"]), 6)
        self.assertEqual((on["no_data"], on["bins"]), (False, []))


@unittest.skipUnless(PAGE, "needs the graphs-page image (ORACLE_GRAPHS_PAGE)")
class Requests(unittest.TestCase):
    def test_arguments_are_checked(self):
        ok = dict(FUTURE, controls={"range": "1 month", "backlog": True})
        bad = [
            None, [], ok, [dict(ok, graph="today")], [dict(ok, controls={"range": "1 month"})],
            [dict(ok, controls={"range": "50%", "backlog": True})], [dict(ok, controls={"range": "1 month", "backlog": 1})],
            [dict(ok, history="365")], [dict(ok, extra=1)],
            [{"scope": "collection", "history": "year", "graph": "intervals", "controls": {"range": "100%"}}],
        ]
        with WorkerProcess(TZ) as w:
            for args in bad:
                with self.subTest(args=args):
                    err = error_of(w, [{"observe": [{"stats_presentation": args}]}])
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)

    def test_graphs_not_read_yet_fail_before_any_collection(self):
        req = {"scope": "collection", "history": "year", "graph": "hours", "controls": {"range": "1 year"}}
        with WorkerProcess(TZ) as w:
            err = error_of(w, [{"db_exec": {"sql": "SELECT 1"}}, {"observe": [{"stats_presentation": [req]}]}])
        self.assertEqual((err["kind"], err.get("observation"), err.get("step")), ("unsupported_observation", "stats_presentation", 1), err)


if __name__ == "__main__":
    unittest.main()
