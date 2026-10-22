# SPDX-License-Identifier: AGPL-3.0-or-later
"""The deck options save, the scheduling preferences' save and the ignore-date
count (specs/conformance/deck-options-ops.md E1, E2, E4) through real workers,
plus the save's undo step in process. Expected values come from the scenarios
(their clock, rows, options and names), Anki's legacy JSON of the records read
back with legacy_json (model-ops.md L1) and the spec texts
(specs/sched/deck-options.md O7 to O10), never from the driver."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.harness import WorkerProcess
from driver.session import Session
from driver.tests.test_worker import epoch, next_rollover, run_ok, scenario

START = "2030-01-15T10:00:00"  # UTC; today is 2030-01-15 (rollover 4:00)
FSRS6 = [0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796,
         1.4835, 0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542]
OPTIONS = {
    "learn_steps": ["30s", "10m", "1d"],
    "graduating_interval_days": 3,
    "easy_interval_days": 6,
    "new_per_day": 17,
    "starting_ease": 2.3,
    "bury_new": True,
    "reviews_per_day": 345,
    "max_interval_days": 999,
    "easy_bonus": 1.4,
    "hard_interval": 1.1,
    "interval_modifier": 0.95,
    "bury_reviews": True,
    "relearn_steps": ["5m"],
    "lapse_new_interval": 0.2,
    "min_lapse_interval_days": 2,
    "leech_threshold": 5,
    "leech_action": "suspend",
    "bury_interday_learning": True,
    "desired_retention": 0.85,
    "fsrs_params": FSRS6,
}
STAMP = ("mod", "usn")
CONFIG_ROWS = {"db_exec": {"sql": "SELECT key, CAST(val AS TEXT) FROM config ORDER BY key"}}


def run(worker, steps, notes=0, setup=None):
    return worker.request({"cmd": "run", "scenario": scenario("t/deck-options", "UTC", START, steps, notes=notes, setup=setup)})


def without(obj: dict, *keys: str) -> dict:
    return {k: v for k, v in obj.items() if k not in keys}


def legacy(step, section, name):
    return step["legacy_json"][section][name]


SAVE_SETUP = {
    "config": {
        # A new collection writes rollover the first time it computes the
        # day (the save does): set it so that the config rows compare.
        "rollover": 4,
        "fsrs": True,
        "newCardsIgnoreReviewLimit": True,
        "applyAllParentLimits": True,
        "fsrsHealthCheck": False,
        "cardStateCustomizer": "customData.good.c = 1;",
    },
    "presets": [{"name": "P", "learn_steps": ["1m", "10m"], "desired_retention": 0.9}, {"name": "Q", "new_per_day": 11}],
    "decks": [{"name": "A", "preset": "P"}, {"name": "A::Sub", "preset": "P"}, {"name": "B", "preset": "P"}],
}
RECORDS = {"observe": [{"legacy_json": {"presets": ["P", "Q"], "decks": ["A", "A::Sub", "B"]}}]}


class SaveDeckOptions(unittest.TestCase):
    maxDiff = None

    def test_the_deck_takes_the_preset_and_the_options_change_only_that_preset(self):
        save = {
            "deck": "A",
            "preset": "Q",
            "options": {"new_per_day": 7, "historical_retention": 0.8, "ignore_reviews_before": "2029-12-01"},
            "deck_desired_retention": 0.75,
        }
        steps = [RECORDS, CONFIG_ROWS, {"save_deck_options": save}, RECORDS, CONFIG_ROWS]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=SAVE_SETUP))
        before, after = out[0], out[3]
        self.assertEqual(out[2], {})  # E1: no output
        p, q = legacy(before, "presets", "P"), legacy(before, "presets", "Q")
        # E1.1: the deck uses Q; Q holds the options; P and the other decks,
        # the subdeck included, are untouched.
        self.assertEqual(legacy(after, "decks", "A")["conf"], q["id"])
        for deck in ("A::Sub", "B"):
            self.assertEqual(legacy(after, "decks", deck), legacy(before, "decks", deck), deck)
        self.assertEqual(legacy(after, "presets", "P"), p)
        q_after = legacy(after, "presets", "Q")
        self.assertEqual(q_after["new"]["perDay"], 7)
        self.assertEqual(q_after["sm2Retention"], 0.8)
        self.assertEqual(q_after["ignoreRevlogsBeforeDate"], "2029-12-01")
        changed = ("new", "sm2Retention", "ignoreRevlogsBeforeDate") + STAMP
        self.assertEqual(without(q_after, *changed), without(q, *changed))
        self.assertEqual(without(q_after["new"], "perDay"), without(q["new"], "perDay"))
        # E1.2: the deck's own desired retention (stored in percent); its
        # other values kept.
        a_before, a_after = legacy(before, "decks", "A"), legacy(after, "decks", "A")
        self.assertEqual(a_after["desiredRetention"], 75)
        self.assertEqual(without(a_after, "conf", "desiredRetention", *STAMP), without(a_before, "conf", "desiredRetention", *STAMP))
        # E1.3: the collection-wide values the screen holds keep their values.
        self.assertEqual(out[4], out[1])

    def test_the_deck_desired_retention_is_set_kept_and_removed(self):
        observe = {"observe": [{"legacy_json": {"decks": ["A"]}}]}
        steps = [
            {"save_deck_options": {"deck": "A", "deck_desired_retention": 0.75}},
            observe,
            {"save_deck_options": {"deck": "A", "options": {"new_per_day": 3}}},
            observe,
            {"save_deck_options": {"deck": "A", "deck_desired_retention": None}},
            observe,
            {"observe": [{"legacy_json": {"presets": ["P"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=SAVE_SETUP))
        decks = [legacy(out[i], "decks", "A") for i in (1, 3, 5)]
        self.assertEqual([d.get("desiredRetention") for d in decks], [75, 75, None])
        preset = legacy(out[6], "presets", "P")
        # Without `preset` the deck keeps its own, which takes the options.
        self.assertEqual({d["conf"] for d in decks}, {preset["id"]})
        self.assertEqual(preset["new"]["perDay"], 3)

    def test_options_reach_the_preset_as_setup_presets_writes_them(self):
        setup = {"presets": [{"name": "R", **OPTIONS}, {"name": "S"}], "decks": [{"name": "A", "preset": "S"}]}
        steps = [
            {"save_deck_options": {"deck": "A", "options": OPTIONS}},
            {"observe": [{"legacy_json": {"presets": ["R", "S"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=setup))
        r, s = legacy(out[1], "presets", "R"), legacy(out[1], "presets", "S")
        self.assertEqual(without(s, "id", "name", *STAMP), without(r, "id", "name", *STAMP))
        self.assertEqual((s["new"]["perDay"], s["new"]["delays"], s["lapse"]["leechAction"]), (17, [0.5, 10.0, 1440.0], 0))
        self.assertEqual(s["fsrsParams6"], FSRS6)

    def test_a_cutoff_after_today_is_saved_as_today(self):
        # sched/fsrs-items I1.3: the save clamps the date text to today's date
        # (the scenario clock's 2030-01-15).
        steps = [
            {"save_deck_options": {"deck": "A", "options": {"ignore_reviews_before": "2031-06-01"}}},
            {"observe": [{"legacy_json": {"presets": ["P"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=SAVE_SETUP))
        self.assertEqual(legacy(out[1], "presets", "P")["ignoreRevlogsBeforeDate"], "2030-01-15")

    def test_a_save_recomputes_memory_states_without_rescheduling(self):
        steps = [
            {"answer": {"card": "#c1", "rating": "good"}},
            {"advance": "10m"},
            {"answer": {"card": "#c1", "rating": "good"}},
            {"observe": ["card(#c1)"]},
            {"save_deck_options": {"deck": "A", "options": {"desired_retention": 0.85}}},
            {"observe": ["card(#c1)"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, notes=1))
        before, after = out[3]["card"]["#c1"], out[5]["card"]["#c1"]
        self.assertEqual((before["type"], before["data"]["dr"]), (2, 0.9))
        self.assertEqual(after["data"]["dr"], 0.85)  # M4: dr from the preset after the save
        for column in ("type", "queue", "due", "ivl"):  # E1.3: reschedule off
            self.assertEqual(after[column], before[column], column)

    def test_bad_arguments_are_refused_before_anything_changes(self):
        cases = [
            ({"preset": "Q"}, "invalid_scenario"),
            ({"deck": "A", "colour": 1}, "invalid_scenario"),
            ({"deck": "A", "options": [1]}, "invalid_scenario"),
            ({"deck": "A", "options": {"new_per_dya": 1}}, "invalid_scenario"),
            ({"deck": "A", "options": {"new_per_day": "7"}}, "invalid_scenario"),
            ({"deck": "A", "options": {"new_per_day": -1}}, "invalid_scenario"),
            ({"deck": "A", "options": {"ignore_reviews_before": 20300101}}, "invalid_scenario"),
            ({"deck": "A", "options": {"historical_retention": True}}, "invalid_scenario"),
            ({"deck": "A", "deck_desired_retention": "0.8"}, "invalid_scenario"),
            ({"deck": "A", "options": {"insertion_order": "due"}}, "invalid_scenario"),
            ({"deck": "A", "options": {"insertion_order": 0}}, "invalid_scenario"),
            ({"deck": "A", "limits": [30]}, "invalid_scenario"),
            ({"deck": "A", "limits": {"new_limit": 30}}, "invalid_scenario"),
            ({"deck": "A", "limits": {"new": 10000}}, "invalid_scenario"),
            ({"deck": "A", "limits": {"review_today": -1}}, "invalid_scenario"),
            ({"deck": "A", "limits": {"new": "30"}}, "invalid_scenario"),
            ({"deck": "A", "limits": {"new": True}}, "invalid_scenario"),
            ({"deck": "A", "limits": {"new": 30.0}}, "invalid_scenario"),
            ({"deck": "A", "globals": {"apply_all_parent_limits": 1}}, "invalid_scenario"),
            ({"deck": "A", "globals": {"fsrs": True}}, "invalid_scenario"),
            ({"deck": "A", "globals": None}, "invalid_scenario"),
            ({"deck": "A", "subtree": 1}, "invalid_scenario"),
            ({"deck": "A", "subtree": None}, "invalid_scenario"),
            ({"deck": "A", "remove": []}, "invalid_scenario"),
            ({"deck": "A", "remove": "Q"}, "invalid_scenario"),
            ({"deck": "A", "remove": ["Q", "Q"]}, "invalid_scenario"),
            ({"deck": "A", "remove": [""]}, "invalid_scenario"),
            # E1: removing the preset the deck is switched to.
            ({"deck": "A", "remove": ["P"]}, "invalid_scenario"),
            ({"deck": "A", "preset": "Q", "remove": ["Q"]}, "invalid_scenario"),
            ({"deck": "Nowhere"}, "unknown_ref"),
            ({"deck": "A", "preset": "Nothing"}, "unknown_ref"),
            ({"deck": "A", "remove": ["Nothing"]}, "unknown_ref"),
        ]
        with WorkerProcess("UTC") as w:
            for args, kind in cases:
                resp = run(w, [{"save_deck_options": args}], setup=SAVE_SETUP)
                with self.subTest(args=args):
                    self.assertFalse(resp["ok"])
                    self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), (kind, 0))

DECK_A = {"observe": [{"legacy_json": {"decks": ["A"]}}]}
PRESET_NAMES = {"db_exec": {"sql": "SELECT name FROM deck_config ORDER BY name"}}
SCM_AFTER_LS = {"db_exec": {"sql": "SELECT scm > ls FROM col"}}


def rows(step) -> list:
    return step["db_exec"]["rows"]


def day_limit(limit: int, today: int) -> dict:
    return {"limit": limit, "today": today}


class SaveLimitsGlobalsSubtreeRemoval(unittest.TestCase):
    maxDiff = None

    def test_limits_are_set_cleared_and_passed_on_as_the_screen_leaves_them(self):
        # O9 and E1. The collection starts on day 0 (2030-01-15); set_clock
        # moves it to day 2.
        steps = [
            {"save_deck_options": {"deck": "A", "limits": {"new": 30, "review": 300, "new_today": 50, "review_today": 500}}},
            DECK_A,
            {"save_deck_options": {"deck": "A", "limits": {"new_today": None}}},
            DECK_A,
            {"set_clock": "2030-01-17T10:00:00"},
            {"save_deck_options": {"deck": "A", "limits": {"new_today": 60}}},
            DECK_A,
            {"save_deck_options": {"deck": "A", "limits": {"new_today": None}}},
            DECK_A,
            {"save_deck_options": {"deck": "A"}},
            DECK_A,
            {"legacy_update": {"deck": "A", "patch": {"reviewLimitToday": {"limit": 70, "today": 9}}}},
            {"save_deck_options": {"deck": "A"}},
            DECK_A,
            {"save_deck_options": {"deck": "A", "limits": {"new": None, "review": None}}},
            DECK_A,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=SAVE_SETUP))
        keys = ("newLimit", "reviewLimit", "newLimitToday", "reviewLimitToday")
        seen = [tuple(legacy(out[i], "decks", "A").get(k) for k in keys) for i in (1, 3, 6, 8, 10, 13, 15)]
        self.assertEqual(seen, [
            # Every limit set, the Today-only ones for day 0.
            (30, 300, day_limit(50, 0), day_limit(500, 0)),
            # Cleared on day 0: removed; the review one applies today and is
            # passed on.
            (30, 300, None, day_limit(500, 0)),
            # Day 2: 60 for today; the review one (day 0) no longer applies,
            # so the screen clears it, which removes a day-0 limit.
            (30, 300, day_limit(60, 2), None),
            # Cleared on day 2: kept, its day moved to 1.
            (30, 300, day_limit(60, 1), None),
            # Not named, not today's: passed on cleared, so it stays as it is
            # (never set again for today).
            (30, 300, day_limit(60, 1), None),
            # A Today-only limit for a future day (9) is removed.
            (30, 300, day_limit(60, 1), None),
            # This-deck limits cleared.
            (None, None, day_limit(60, 1), None),
        ])

    def test_the_global_limit_options_are_set_or_kept(self):
        # SAVE_SETUP stores both as true.
        steps = [
            CONFIG_ROWS,
            {"save_deck_options": {"deck": "A", "globals": {"new_cards_ignore_review_limit": False}}},
            CONFIG_ROWS,
            {"save_deck_options": {"deck": "A", "globals": {"apply_all_parent_limits": False}}},
            CONFIG_ROWS,
            {"save_deck_options": {"deck": "A", "globals": {"new_cards_ignore_review_limit": True, "apply_all_parent_limits": True}}},
            CONFIG_ROWS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=SAVE_SETUP))
        before = dict(rows(out[0]))
        expected = [("false", "true"), ("false", "false"), ("true", "true")]
        for i, (ignore, parents) in zip((2, 4, 6), expected):
            after = dict(rows(out[i]))
            self.assertEqual((after["newCardsIgnoreReviewLimit"], after["applyAllParentLimits"]), (ignore, parents))
            self.assertEqual(without(after, "newCardsIgnoreReviewLimit", "applyAllParentLimits"),
                             without(before, "newCardsIgnoreReviewLimit", "applyAllParentLimits"))

    def test_a_subtree_save_gives_every_subdeck_the_preset_and_the_decks_values(self):
        records = {"observe": [{"legacy_json": {"decks": ["A", "A::Sub", "B"], "presets": ["P", "Q"]}}]}
        steps = [
            {"legacy_update": {"deck": "A::Sub", "patch": {"newLimit": 5, "desiredRetention": 85}}},
            {"save_deck_options": {"deck": "A", "preset": "Q", "subtree": True, "limits": {"new": 15}}},
            records,
            {"save_deck_options": {"deck": "A", "preset": "P", "subtree": False}},
            records,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=SAVE_SETUP))
        p, q = legacy(out[2], "presets", "P")["id"], legacy(out[2], "presets", "Q")["id"]
        fields = ("conf", "newLimit", "desiredRetention")

        def decks(step):
            return {name: tuple(legacy(step, "decks", name).get(k) for k in fields) for name in ("A", "A::Sub", "B")}

        # O7.2 step 3: the subdeck's own limit and retention replaced by A's.
        self.assertEqual(decks(out[2]), {"A": (q, 15, None), "A::Sub": (q, 15, None), "B": (p, None, None)})
        # Without subtree only A changes.
        self.assertEqual(decks(out[4]), {"A": (p, 15, None), "A::Sub": (q, 15, None), "B": (p, None, None)})

    def test_a_removed_preset_goes_and_its_decks_get_the_saved_preset(self):
        setup = dict(SAVE_SETUP, decks=[{"name": "A", "preset": "P"}, {"name": "A::Sub", "preset": "P"}, {"name": "B", "preset": "Q"}])
        records = {"observe": [{"legacy_json": {"decks": ["A", "B"], "presets": ["P"]}}]}
        # undo_status and undo before legacy_json, which empties the histories
        # (deck-options-ops.md E5).
        steps = [
            {"legacy_update": {"deck": "A", "patch": {"newLimit": 12}}},
            {"legacy_update": {"deck": "B", "patch": {"newLimit": 7}}},
            {"db_exec": {"sql": "UPDATE col SET ls = scm"}},
            {"advance": "1h"},
            # 4: the default preset, alone or after another one: refused, and
            # nothing changes (Anki rolls the removal of Q back).
            {"save_deck_options": {"deck": "A", "remove": ["Default"]}},
            {"save_deck_options": {"deck": "A", "remove": ["Q", "Default"]}},
            PRESET_NAMES,
            SCM_AFTER_LS,
            # 8: Q removed from A's screen.
            {"save_deck_options": {"deck": "A", "remove": ["Q"]}},
            PRESET_NAMES,
            SCM_AFTER_LS,
            {"db_exec": {"sql": "SELECT count(*) FROM graves"}},
            {"observe": ["undo_status"]},
            # 13: undone, then redone (legacy_json empties the histories:
            # read last).
            {"undo": {}},
            PRESET_NAMES,
            SCM_AFTER_LS,
            {"redo": {}},
            records,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, setup=setup))
        self.assertEqual(out[4:6], [{"save_deck_options": {"error": "default_preset"}}] * 2)
        self.assertEqual(rows(out[6]), [["Default"], ["P"], ["Q"]])
        self.assertEqual(rows(out[7]), [[0]])
        self.assertEqual(out[8], {})
        self.assertEqual(rows(out[9]), [["Default"], ["P"]])
        self.assertEqual(rows(out[10]), [[1]])  # a one-way sync is next
        self.assertEqual(rows(out[11]), [[0]])  # no grave for a preset
        self.assertEqual(out[12], {"undo_status": {"undo": "deck_options", "redo": None}})
        self.assertEqual(rows(out[14]), [["Default"], ["P"], ["Q"]])
        self.assertEqual(rows(out[15]), [[0]])
        p = legacy(out[17], "presets", "P")["id"]
        a, b = legacy(out[17], "decks", "A"), legacy(out[17], "decks", "B")
        # B takes P and A's own values (its new limit 7 replaced by 12).
        self.assertEqual((a["conf"], a["newLimit"]), (p, 12))
        self.assertEqual((b["conf"], b["newLimit"]), (p, 12))

    def test_the_insertion_order_option_resorts_the_decks_new_cards(self):
        positions = {"db_exec": {"sql": "SELECT due FROM cards ORDER BY id"}}
        order = {"observe": [{"legacy_json": {"presets": ["P"]}}]}
        steps = [
            {"db_exec": {"sql": "UPDATE cards SET due = 100 - (SELECT count(*) FROM cards c WHERE c.id < cards.id)"}},
            positions,
            {"save_deck_options": {"deck": "A", "options": {"insertion_order": "random"}}},
            positions,
            order,
            {"save_deck_options": {"deck": "A", "options": {"insertion_order": "sequential"}}},
            positions,
            order,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/deck-options", "UTC", START, steps, notes=4))
        self.assertEqual(rows(out[1]), [[100], [99], [98], [97]])
        # O8: Random, positions 1 to 4 in some order; legacy new.order 0.
        self.assertEqual(sorted(r[0] for r in rows(out[3])), [1, 2, 3, 4])
        self.assertEqual(legacy(out[4], "presets", "P")["new"]["order"], 0)
        # Sequential: by order added (card ids follow the notes); new.order 1.
        self.assertEqual(rows(out[6]), [[1], [2], [3], [4]])
        self.assertEqual(legacy(out[7], "presets", "P")["new"]["order"], 1)


START_SECS = epoch(START, "UTC")
PREF_ROWS = {"db_exec": {"sql": "SELECT key, CAST(val AS TEXT) FROM config "
                                "WHERE key IN ('collapseTime', 'loadBalancerEnabled', 'rollover') ORDER BY key"}}


class SetSchedulingPrefs(unittest.TestCase):
    maxDiff = None

    def test_the_three_settings_are_saved_as_one_preferences_step(self):
        steps = [
            PREF_ROWS,
            {"set_scheduling_prefs": {"learn_ahead_minutes": 30, "load_balancer": False}},
            PREF_ROWS,
            {"observe": ["undo_status"]},
            {"undo": {}},
            PREF_ROWS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/prefs", "UTC", START, steps, setup={"config": {"rollover": 4}}))
        self.assertEqual(out[1], {})  # E4: no output
        self.assertEqual(dict(rows(out[2])), dict(rows(out[0]), collapseTime="1800", loadBalancerEnabled="false"))
        self.assertEqual(out[3], {"undo_status": {"undo": "preferences", "redo": None}})
        self.assertEqual(rows(out[5]), rows(out[0]))

    def test_a_new_rollover_takes_effect_at_once(self):
        # O10: day 0 starts 2030-01-15; at 10:00 an hour of 6 keeps day 0 and
        # moves the next day to 06:00. At 2030-01-17 05:00 it is still day 1;
        # back to 4, day 2 has begun.
        steps = [
            {"observe": ["timing"]},
            {"set_scheduling_prefs": {"rollover": 6}},
            {"observe": ["timing"]},
            {"set_clock": "2030-01-17T05:00:00"},
            {"observe": ["timing"]},
            {"set_scheduling_prefs": {"rollover": 4}},
            {"observe": ["timing"]},
            PREF_ROWS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/prefs", "UTC", START, steps))
        timing = [(out[i]["timing"]["days_elapsed"], out[i]["timing"]["next_day_at"]) for i in (0, 2, 4, 6)]
        later = epoch("2030-01-17T05:00:00", "UTC")
        self.assertEqual(timing, [
            (0, next_rollover(START_SECS, "UTC")),
            (0, next_rollover(START_SECS, "UTC", hour=6)),
            (1, next_rollover(later, "UTC", hour=6)),
            (2, next_rollover(later, "UTC")),
        ])
        self.assertEqual(dict(rows(out[7]))["rollover"], "4")

    def test_bad_arguments_are_refused_before_anything_changes(self):
        cases = [
            {},
            [],
            {"rollover": 24},
            {"rollover": -1},
            {"rollover": "6"},
            {"rollover": 6.0},
            {"rollover": None},
            {"learn_ahead_minutes": 1000},
            {"learn_ahead_minutes": True},
            {"load_balancer": 1},
            {"load_balancer": None},
            {"rollover": 6, "colour": "red"},
        ]
        with WorkerProcess("UTC") as w:
            for args in cases:
                resp = w.request({"cmd": "run", "scenario": scenario("t/prefs", "UTC", START, [{"set_scheduling_prefs": args}])})
                with self.subTest(args=args):
                    self.assertFalse(resp["ok"])
                    self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 0))


class SaveUndo(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_save_is_one_deck_options_undo_step(self):
        sc = {"id": "t/undo", "clock": {"tz": "UTC", "start": START}, "setup": SAVE_SETUP, "steps": []}
        s = Session(sc, "UTC", self.tmp.name, self.clock)
        try:
            s.run_step(0, {"save_deck_options": {"deck": "A", "preset": "Q", "options": {"new_per_day": 7}}})
            # E1.5: one step, named as the backend names "Deck Options".
            self.assertEqual(s.col.undo_status().undo, s.col.tr.deck_config_title())
            s.col.undo()
            self.assertEqual(s.col.decks.get(s.decks["A"])["conf"], s.presets["P"])
            self.assertEqual(s.col.decks.get_config(s.presets["Q"])["new"]["perDay"], 11)
        finally:
            s.close()


def ms(local: str) -> int:
    return epoch(local, "UTC") * 1000


T1 = ms("2030-01-10T12:00:00")
T2 = ms("2030-01-12T12:00:00")
COUNT_SETUP = {
    "presets": [{"name": "P"}, {"name": 'Say "hi"'}],
    "decks": [{"name": "A", "preset": "P"}, {"name": "B"}, {"name": "C", "preset": 'Say "hi"'}],
    "notes": [{"notetype": "QA", "deck": deck, "fields": [f"q{i}", "a"]} for i, deck in enumerate("AAAABC", 1)],
}


def card(offset: int, queue: int = 2) -> dict:
    return {"db_exec": {
        "sql": "UPDATE cards SET type = 2, queue = ?, due = 3, ivl = 5, factor = 2500, reps = 2, data = '{}' "
               "WHERE id = (SELECT id FROM cards ORDER BY id LIMIT 1 OFFSET ?)",
        "params": [queue, offset],
    }}


def row(offset: int, rid: int, kind: int) -> dict:
    return {"db_exec": {
        "sql": "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) "
               "VALUES (?, (SELECT id FROM cards ORDER BY id LIMIT 1 OFFSET ?), -1, 3, 5, 1, 2500, 5000, ?)",
        "params": [rid, offset, kind],
    }}


# c1 (A): a type-0 row at T1, a review row at T2; c2 (A): a review row only;
# c3 (A): new; c4 (A): suspended, a type-0 row; c5 (B, Default): a type-0 row;
# c6 (C, Say "hi"): a type-0 row.
COUNT_ROWS = [
    card(0), row(0, T1 + 1, 0), row(0, T2 + 1, 1),
    card(1), row(1, T1 + 2, 1),
    card(3, queue=-1), row(3, T1 + 4, 0),
    card(4), row(4, T1 + 5, 0),
    card(5), row(5, T1 + 6, 0),
]
COL_MOD = {"db_exec": {"sql": "SELECT mod FROM col"}}


def count(preset, dates):
    return {"observe": [{"ignored_before_count": {"preset": preset, "dates": dates}}]}


class IgnoredBeforeCount(unittest.TestCase):
    def test_counts_per_date_on_the_presets_search(self):
        dates = ["2030-01-01", "2030-01-11", "", "garbage", "2030-02-30"]
        steps = COUNT_ROWS + [
            COL_MOD,
            count("P", dates),
            count("Default", ["2030-01-01"]),
            count('Say "hi"', ["2030-01-01"]),
            COL_MOD,
        ]
        first = len(COUNT_ROWS)
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/count", "UTC", START, steps, setup=COUNT_SETUP))
        # M5: total = P's cards neither new nor suspended (c1, c2); included =
        # those with a type-0 row after the date's UTC midnight (c1, at T1).
        self.assertEqual(out[first + 1]["ignored_before_count"], [
            {"included": 1, "total": 2},
            {"included": 0, "total": 2},
            {"included": 1, "total": 2},
            {"error": "invalid_date"},
            {"error": "invalid_date"},
        ])
        self.assertEqual(out[first + 2]["ignored_before_count"], [{"included": 1, "total": 1}])  # c5
        # The screen escapes '"' and '\\' in the preset name, and Anki's preset
        # search compares the name without unescaping it: no card for c6's
        # preset.
        self.assertEqual(out[first + 3]["ignored_before_count"], [{"included": 0, "total": 0}])
        self.assertEqual(out[first + 4], out[first])  # reading changes nothing

    def test_a_param_search_replaces_the_preset_search(self):
        steps = COUNT_ROWS + [
            {"legacy_update": {"preset": "P", "patch": {"weightSearch": "deck:A"}}},
            count("P", ["2030-01-01"]),
            {"legacy_update": {"preset": "P", "patch": {"weightSearch": "deck:B"}}},
            count("P", ["2030-01-01"]),
        ]
        first = len(COUNT_ROWS)
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/count", "UTC", START, steps, setup=COUNT_SETUP))
        # deck:A without -is:suspended: c1, c2 and the suspended c4 (c3 is new).
        self.assertEqual(out[first + 1]["ignored_before_count"], [{"included": 2, "total": 3}])
        self.assertEqual(out[first + 3]["ignored_before_count"], [{"included": 1, "total": 1}])

    def test_bad_requests_are_invalid_scenarios(self):
        cases = [
            ({"preset": "P"}, "invalid_scenario"),
            ({"preset": "P", "dates": []}, "invalid_scenario"),
            ({"preset": "P", "dates": "2030-01-01"}, "invalid_scenario"),
            ({"preset": "P", "dates": [20300101]}, "invalid_scenario"),
            ({"preset": "P", "dates": ["2030-01-01"], "deck": "A"}, "invalid_scenario"),
            ({"preset": "Nothing", "dates": ["2030-01-01"]}, "unknown_ref"),
        ]
        with WorkerProcess("UTC") as w:
            for request, kind in cases:
                resp = run(w, [{"observe": [{"ignored_before_count": request}]}], setup=COUNT_SETUP)
                with self.subTest(request=request):
                    self.assertFalse(resp["ok"])
                    self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), (kind, 0))


if __name__ == "__main__":
    unittest.main()
