# SPDX-License-Identifier: AGPL-3.0-or-later
"""Optimizer ops (specs/conformance/optimizer-ops.md P1-P4) through real
workers, and the pure search and cutoff readings. Expected counts come from
specs/sched/fsrs-items.md I7's example history and the I1.3 table; stored
values are read back with db_exec in the same run, never from the driver."""

from __future__ import annotations

import json
import math
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok, scenario

START = "2026-03-02T10:00:00"
# I7's example history (H): 6 training items and 9 reviews on one card.
H = [
    (1759309200000, 3, -600, -60, 0, 0),
    (1759309800000, 3, 1, -600, 2500, 0),
    (1759399200000, 3, 3, 1, 2500, 1),
    (1759658400000, 3, 9, 3, 2500, 1),
    (1760436000000, 1, -600, 9, 2300, 1),
    (1760436600000, 3, 2, -600, 2300, 2),
    (1760608800000, 3, 6, 2, 2300, 1),
    (1761127200000, 4, 20, 6, 2450, 1),
    (1762855200000, 2, 25, 20, 2300, 1),
]
REVIEW = "UPDATE cards SET type = 2, queue = 2, due = 30, ivl = 3, factor = 2500, reps = 9, left = 0, data = '{}'"
PARAMS_21 = [0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835,
             0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542]
PARAMS_19 = PARAMS_21[:19]


def history(card_offset: int, id_bump: int) -> dict:
    values = ", ".join(f"({rid + id_bump}, {e}, {i}, {li}, {f}, {t})" for rid, e, i, li, f, t in H)
    sql = (
        "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) SELECT v.column1, "
        f"(SELECT id FROM cards ORDER BY id LIMIT 1 OFFSET {card_offset}), -1, v.column2, v.column3, v.column4, "
        f"v.column5, 5000, v.column6 FROM (VALUES {values}) AS v"
    )
    return {"db_exec": {"sql": sql}}


def setup(presets, decks, notes):
    return {
        "config": {"fsrs": True, "loadBalancerEnabled": False},
        "presets": [{"name": n} for n in presets],
        "decks": [{"name": d, "preset": p} for d, p in decks],
        "notes": [{"notetype": "QA", "deck": d, "fields": [f"q{i}", "a"]} for i, d in enumerate(notes)],
    }


def run(test, steps, presets=("P",), decks=(("A", "P"),), notes=("A",)):
    with WorkerProcess("UTC") as w:
        return run_ok(test, w, scenario("t/optimizer", "UTC", START, steps, setup=setup(presets, decks, notes)))


def compute(preset="P", steps=1, **extra):
    return {"compute_fsrs_params": {"preset": preset, "relearning_steps": steps, **extra}}


def stored_params(name):
    return {"observe": [{"legacy_json": {"presets": [name]}}]}


class PureReadings(unittest.TestCase):
    def test_cutoff_texts_follow_i1_3(self):
        from driver.ops.optimizer import _date_ms

        day = 86_400_000
        jan3 = 20_456 * day  # 2026-01-03
        for text in ("2026-01-03", "2026-1-3", " 2026-01-03", "2026- 1- 3", "+2026-01-03"):
            with self.subTest(text=text):
                self.assertEqual(_date_ms(text), jan3)
        self.assertEqual(_date_ms("1970-01-01"), 0)
        self.assertEqual(_date_ms("1969-12-31"), -day)
        self.assertEqual(_date_ms("2000-02-29"), 11_016 * day)
        self.assertIsNotNone(_date_ms("0-02-29"))  # year 0 is a leap year
        for text in ("2026-01-03 ", "02026-01-03", "2026-02-30", "2026-13-01", "2026/01/03", "2026-01-03T00:00",
                     "garbage", "1900-02-29"):
            with self.subTest(text=text):
                self.assertIsNone(_date_ms(text))

    def test_default_searches_of_both_forms(self):
        from driver.ops.optimizer import _default_search

        cases = {
            "Plain": ('preset:"Plain" -is:suspended', "(preset:Plain -is:suspended)"),
            'Q"uote': ('preset:"Q\\"uote" -is:suspended', '(preset:Q\\"uote -is:suspended)'),
            "Back\\slash": ('preset:"Back\\\\slash" -is:suspended', "(preset:Back\\slash -is:suspended)"),
            "Two words": ('preset:"Two words" -is:suspended', '("preset:Two words" -is:suspended)'),
            "Tab\tSplit": ('preset:"Tab\tSplit" -is:suspended', "(preset:Tab\tSplit -is:suspended)"),
            "a(b": ('preset:"a(b" -is:suspended', '("preset:a(b" -is:suspended)'),
            "x　y": ('preset:"x　y" -is:suspended', '("preset:x　y" -is:suspended)'),
        }
        for name, (one, every) in cases.items():
            with self.subTest(name=name):
                self.assertEqual(_default_search(name, "one_preset"), one)
                self.assertEqual(_default_search(name, "all_presets"), every)


class Arguments(unittest.TestCase):
    def test_malformed_arguments_are_invalid_scenario(self):
        bad = [
            {"compute_fsrs_params": {"preset": "P"}},
            {"compute_fsrs_params": {"preset": "P", "relearning_steps": -1}},
            {"compute_fsrs_params": {"preset": "P", "relearning_steps": True}},
            {"compute_fsrs_params": {"preset": "P", "relearning_steps": 1.0}},
            {"compute_fsrs_params": {"preset": "P", "relearning_steps": 1, "form": "both"}},
            {"compute_fsrs_params": {"preset": "P", "relearning_steps": 1, "deck": "A"}},
            {"optimize_all_presets": {}},
            {"optimizer_run": {"deck": "A"}},
            {"observe": [{"fsrs_model_check": {"presets": []}}]},
            {"observe": [{"fsrs_model_check": {"presets": ["P", "P"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            for step in bad:
                sc = scenario("t/optimizer", "UTC", START, [step], setup=setup(["P"], [("A", "P")], []))
                resp = w.request({"cmd": "run", "scenario": sc})
                with self.subTest(step=step):
                    self.assertFalse(resp["ok"], resp)
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0))

    def test_an_unknown_preset_is_unknown_ref(self):
        with WorkerProcess("UTC") as w:
            sc = scenario("t/optimizer", "UTC", START, [compute("Nope")], setup=setup(["P"], [("A", "P")], []))
            resp = w.request({"cmd": "run", "scenario": sc})
        self.assertFalse(resp["ok"], resp)
        self.assertEqual(resp["error"]["kind"], "unknown_ref")


class ComputeFsrsParams(unittest.TestCase):
    def test_counts_items_and_reviews_and_keeps_the_list_in_use(self):
        out = run(self, [{"db_exec": {"sql": REVIEW}}, history(0, 1), compute(), stored_params("P"), compute()])
        result = out[2]["compute_fsrs_params"]
        self.assertEqual(set(result), {"items", "reviews", "kept", "health", "eval"})
        self.assertEqual((result["items"], result["reviews"]), (6, 9))
        # One card: the crate's preparation leaves fewer than 8 items, so its
        # defaults come back, which the empty list in use evaluates with too.
        self.assertIs(result["kept"], True)
        self.assertIsNone(result["health"])
        # Only the log loss: the crate sums RMSE(bins) in hash-map order, so
        # its last bits differ between two calls (optimizer-ops.md P1.4).
        self.assertEqual(set(result["eval"]), {"log_loss"})
        self.assertTrue(isinstance(result["eval"]["log_loss"], float) and math.isfinite(result["eval"]["log_loss"]))
        # Writes nothing: the same call again gives the same output.
        self.assertEqual(out[4], out[2])

    def test_no_item_returns_the_list_in_use_without_evaluation(self):
        steps = [
            {"legacy_update": {"preset": "P", "patch": {"fsrsParams6": [], "fsrsParams5": PARAMS_19}}},
            compute(),
        ]
        out = run(self, steps, notes=())
        self.assertEqual(out[1], {"compute_fsrs_params": {"items": 0, "reviews": 0, "kept": True, "health": None,
                                                          "eval": None}})

    def test_a_refused_cutoff_and_a_refused_search_are_rejections(self):
        steps = [
            {"db_exec": {"sql": REVIEW}},
            history(0, 1),
            {"legacy_update": {"preset": "P", "patch": {"ignoreRevlogsBeforeDate": "2025/10/01"}}},
            compute(),
            {"legacy_update": {"preset": "P", "patch": {"ignoreRevlogsBeforeDate": "", "weightSearch": "(deck:A"}}},
            compute(),
            compute(form="all_presets"),
            {"legacy_update": {"preset": "P", "patch": {"ignoreRevlogsBeforeDate": "2025/10/01"}}},
            compute(),
        ]
        out = run(self, steps)
        self.assertEqual(out[3], {"compute_fsrs_params": {"error": "invalid_cutoff"}})
        self.assertEqual(out[5], {"compute_fsrs_params": {"error": "invalid_search"}})
        self.assertEqual(out[6], {"compute_fsrs_params": {"error": "invalid_search"}})
        # Both refused: the cutoff comes first (O3 rule 2 before rule 3).
        self.assertEqual(out[8], {"compute_fsrs_params": {"error": "invalid_cutoff"}})

    def test_a_cutoff_after_the_learning_start_leaves_no_item(self):
        steps = [
            {"db_exec": {"sql": REVIEW}},
            history(0, 1),
            {"legacy_update": {"preset": "P", "patch": {"ignoreRevlogsBeforeDate": "2025-10-02"}}},
            compute(),
            {"legacy_update": {"preset": "P", "patch": {"ignoreRevlogsBeforeDate": "2025-10-01"}}},
            compute(),
        ]
        out = run(self, steps)
        self.assertEqual(out[3]["compute_fsrs_params"]["items"], 0)
        self.assertEqual(out[5]["compute_fsrs_params"]["items"], 6)

    def test_white_space_alone_differs_between_the_forms(self):
        # The one-preset form reads the whole collection, rows of a card that
        # no longer exists included; the all-preset form takes the default set.
        steps = [
            {"db_exec": {"sql": REVIEW}},
            history(0, 1),
            history(1, 2),
            {"db_exec": {"sql": history(0, 3)["db_exec"]["sql"].replace(
                "(SELECT id FROM cards ORDER BY id LIMIT 1 OFFSET 0)", "4242")}},
            {"legacy_update": {"preset": "P", "patch": {"weightSearch": "  "}}},
            compute(form="one_preset"),
            compute(form="all_presets"),
        ]
        out = run(self, steps, presets=("P", "Q"), decks=(("A", "P"), ("B", "Q")), notes=("A", "B"))
        self.assertEqual(out[5]["compute_fsrs_params"]["items"], 18)
        self.assertEqual(out[6]["compute_fsrs_params"]["items"], 6)

    def test_the_health_check_follows_the_key_above_300_items(self):
        # Seven cards with a learning row and 43 reviews each: 301 items.
        sql = (
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 43), c(k) AS (SELECT 0 "
            "UNION ALL SELECT k + 1 FROM c WHERE k < 6) INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, "
            "factor, time, type) SELECT 1748772000000 + 86400000 * (n.i * (c.k % 3 + 1)) + c.k, (SELECT x.id FROM "
            "cards x WHERE (SELECT count(*) FROM cards y WHERE y.id < x.id) = c.k), -1, CASE WHEN n.i = 0 THEN 3 "
            "WHEN (n.i + c.k) % 5 = 0 THEN 1 ELSE 3 END, c.k % 3 + 1, -60, 2500, 6000, CASE WHEN n.i = 0 THEN 0 "
            "ELSE 1 END FROM n, c"
        )
        steps = [
            {"db_exec": {"sql": REVIEW}},
            {"db_exec": {"sql": sql}},
            compute(),
            {"set_config": {"key": "fsrsHealthCheck", "value": False}},
            compute(),
        ]
        out = run(self, steps, notes=("A",) * 7)
        self.assertEqual(out[2]["compute_fsrs_params"]["items"], 301)
        self.assertIn(out[2]["compute_fsrs_params"]["health"], ("good", "bad"))
        self.assertIsNone(out[4]["compute_fsrs_params"]["health"])


class OptimizeAllPresets(unittest.TestCase):
    def test_writes_every_preset_and_the_optimize_day(self):
        steps = [
            {"legacy_update": {"preset": "Old", "patch": {"fsrsParams6": [], "fsrsParams5": PARAMS_19}}},
            {"legacy_update": {"preset": "Gone", "patch": {"fsrsParams6": [], "fsrsParams5": PARAMS_19,
                                                           "weightSearch": "(deck:A"}}},
            {"optimize_all_presets": {"deck": "A"}},
            {"observe": [{"legacy_json": {"presets": ["P", "Old", "Gone"]}}, "timing"]},
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT) FROM config WHERE key = 'lastFsrsOptimize'"}},
        ]
        out = run(self, steps, presets=("P", "Old", "Gone"), decks=(("A", "P"), ("B", "Old"), ("C", "Gone")),
                  notes=())
        self.assertEqual(out[2], {})
        presets = out[3]["legacy_json"]["presets"]
        self.assertEqual(presets["P"]["fsrsParams6"], [])
        # No item: the list in use comes back and is written into FSRS-6.
        self.assertEqual(len(presets["Old"]["fsrsParams6"]), 19)
        # A refused search keeps the empty FSRS-6 list, which clears FSRS-5.
        self.assertEqual((presets["Gone"]["fsrsParams6"], presets["Gone"]["fsrsParams5"]), ([], []))
        today = out[3]["timing"]["days_elapsed"]
        self.assertEqual(json.loads(out[4]["db_exec"]["rows"][0][0]), today)

    def test_optimizer_run_is_the_same_save_on_the_default_deck(self):
        steps = [
            {"legacy_update": {"preset": "Old", "patch": {"fsrsParams6": [], "fsrsParams5": PARAMS_19}}},
            {"optimizer_run": {}},
            {"observe": [{"legacy_json": {"presets": ["Old"]}}]},
            {"db_exec": {"sql": "SELECT count(*) FROM config WHERE key = 'lastFsrsOptimize'"}},
        ]
        out = run(self, steps, presets=("Old",), decks=(("B", "Old"),), notes=())
        self.assertEqual(out[1], {})
        self.assertEqual(len(out[2]["legacy_json"]["presets"]["Old"]["fsrsParams6"]), 19)
        self.assertEqual(out[3]["db_exec"]["rows"], [[1]])


class FsrsModelCheck(unittest.TestCase):
    def test_reports_whether_the_list_in_use_builds_a_model(self):
        steps = [
            {"legacy_update": {"preset": "Six", "patch": {"fsrsParams6": PARAMS_21}}},
            {"legacy_update": {"preset": "Five", "patch": {"fsrsParams6": [], "fsrsParams5": PARAMS_19}}},
            {"legacy_update": {"preset": "Twenty", "patch": {"fsrsParams6": PARAMS_19 + [0.1]}}},
            {"legacy_update": {"preset": "Later", "patch": {"fsrsParams6": PARAMS_21 + [0.1] * 13}}},
            {"observe": [{"fsrs_model_check": {"presets": ["Default", "Six", "Five", "Twenty", "Later"]}}]},
        ]
        names = ("Six", "Five", "Twenty", "Later")
        out = run(self, steps, presets=names, decks=tuple((f"D{n}", n) for n in names), notes=())
        self.assertEqual(
            out[4]["fsrs_model_check"],
            {"Default": "ok", "Six": "ok", "Five": "ok", "Twenty": "invalid_params", "Later": "invalid_params"},
        )


if __name__ == "__main__":
    unittest.main()
