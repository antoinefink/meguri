# SPDX-License-Identifier: AGPL-3.0-or-later
"""card_stats, stats and graph_preferences (specs/conformance/stats-ops.md H9,
H10, H13) through real workers. Expected values come from the rows each test
writes and the rules of stats/card-info C3 and stats/graphs-common G7, G14,
graphs-revlog V8 and graphs-cards C12, worked by hand; never from the
driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, next_rollover, run_ok, scenario

TZ = "Europe/Paris"
START = "2030-01-15T10:00:00"
NOW = epoch(START, TZ)
N = next_rollover(NOW, TZ)  # next_day_at, rollover 4
CARD_STATS_KEYS = {
    "card_id", "note_id", "deck", "original_deck", "card_type", "notetype", "preset", "custom_data", "added",
    "first_review", "latest_review", "due_date", "due_position", "interval", "ease", "reviews", "lapses",
    "average_secs", "total_secs", "desired_retention", "fsrs_retrievability", "memory_state", "fsrs_params", "revlog",
}
ENTRY_KEYS = {"time", "review_kind", "button_chosen", "interval", "ease", "taken_secs", "memory_state", "last_interval"}
WARM = {"observe": ["timing"]}


def revlog(*rows):
    """INSERT for (id ms, cid SQL, type, ease, lastIvl, time ms) rows."""
    values = ", ".join(
        f"({i}, {cid}, -1, {ease}, 1, {last}, 2500, {t}, {kind})" for i, cid, kind, ease, last, t in rows
    )
    return {"db_exec": {"sql": f"INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES {values}"}}


def error_of(worker, steps, notes=0, setup=None):
    resp = worker.request({"cmd": "run", "scenario": scenario("t/stats", TZ, START, steps, notes=notes, setup=setup)})
    return resp.get("error")


FIRST_CARD = "(SELECT id FROM cards ORDER BY id LIMIT 1)"


class CardStats(unittest.TestCase):
    def test_a_new_card(self):
        steps = [{"observe": [{"card_stats": ["#c1"]}]}]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/stats", TZ, START, steps, notes=1))
        stats = out[0]["card_stats"]["#c1"]
        self.assertEqual(set(stats), CARD_STATS_KEYS)
        self.assertEqual(
            {k: stats[k] for k in CARD_STATS_KEYS - {"due_position", "fsrs_params"}},
            {
                "card_id": "#c1", "note_id": "#n1", "deck": "A", "original_deck": None, "card_type": "C1",
                "notetype": "QA", "preset": "P", "custom_data": "", "added": NOW, "first_review": None,
                "latest_review": None, "due_date": None, "interval": 0, "ease": 0, "reviews": 0, "lapses": 0,
                "average_secs": 0.0, "total_secs": 0.0, "desired_retention": None, "fsrs_retrievability": None,
                "memory_state": None, "revlog": [],
            },
        )
        self.assertIsInstance(stats["due_position"], int)
        self.assertIsInstance(stats["fsrs_params"], list)

    def test_rows_newest_first_with_times_and_dates(self):
        # Two rated rows of 3,000 and 6,000 ms and a Manual row of 0 ms
        # (card-info C4.3): average 4.5 s, total 9 s; first and latest review
        # from the rated rows only; the Manual row is newest.
        t0 = (NOW - 3 * 86_400) * 1000
        steps = [
            {"db_exec": {"sql": f"UPDATE cards SET type = 2, queue = 2, ivl = 3, due = 2, data = '{{\"s\":3.0,\"d\":5.0,\"lrt\":{NOW - 86_400}}}'"}},
            revlog(
                (t0, FIRST_CARD, 0, 3, -600, 3_000),
                (t0 + 86_400_000, FIRST_CARD, 1, 3, 1, 6_000),
                (t0 + 2 * 86_400_000, FIRST_CARD, 4, 0, 0, 0),
            ),
            {"observe": [{"card_stats": ["#c1"]}]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/stats", TZ, START, steps, notes=1))
        stats = out[2]["card_stats"]["#c1"]
        self.assertEqual((stats["average_secs"], stats["total_secs"]), (4.5, 9.0))
        self.assertEqual((stats["first_review"], stats["latest_review"]), (t0 // 1000, t0 // 1000 + 86_400))
        self.assertEqual([e["time"] for e in stats["revlog"]], [t0 // 1000 + 2 * 86_400, t0 // 1000 + 86_400, t0 // 1000])
        self.assertEqual([e["review_kind"] for e in stats["revlog"]], [4, 1, 0])
        self.assertEqual([e["button_chosen"] for e in stats["revlog"]], [0, 3, 3])
        self.assertEqual([e["interval"] for e in stats["revlog"]], [86_400] * 3)  # ivl 1
        self.assertEqual([e["last_interval"] for e in stats["revlog"]], [0, 86_400, 600])
        self.assertEqual([e["taken_secs"] for e in stats["revlog"]], [0.0, 6.0, 3.0])
        for entry in stats["revlog"]:
            self.assertEqual(set(entry), ENTRY_KEYS)
        self.assertEqual(stats["memory_state"], {"stability": 3.0, "difficulty": 5.0})
        # Day-number due 2 on day 0 (the collection's first day): now + 2 days.
        self.assertEqual(stats["due_date"], NOW + 2 * 86_400)
        self.assertEqual(stats["interval"], 3)
        self.assertIsInstance(stats["fsrs_retrievability"], float)

    def test_arguments_are_checked(self):
        bad = [None, [], "#c1", ["#c1", "#c1"], ["#n1"], [1], {"cards": ["#c1"]}]
        with WorkerProcess(TZ) as w:
            for args in bad:
                with self.subTest(args=args):
                    err = error_of(w, [{"observe": [{"card_stats": args}]}], notes=1)
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)
            err = error_of(w, [{"observe": [{"card_stats": ["#c2"]}]}], notes=1)
            self.assertEqual((err["kind"], err.get("step")), ("unknown_ref", 0), err)

    def test_a_card_whose_deck_is_gone_is_a_run_error(self):
        steps = [{"db_exec": {"sql": "UPDATE cards SET did = 999"}}, {"observe": [{"card_stats": ["#c1"]}]}]
        with WorkerProcess(TZ) as w:
            err = error_of(w, steps, notes=1)
        self.assertEqual((err["kind"], err.get("step")), ("anki_error", 1), err)

    def test_a_missing_home_preset_is_a_run_error(self):
        # stats/card-info C11 (missing_preset): the preset name falls back to
        # Default, but the history replay reads the preset again with no
        # fallback, so the whole call fails, a new card with no row too.
        steps = [{"db_exec": {"sql": "DELETE FROM deck_config WHERE name = 'P'"}}, {"observe": [{"card_stats": ["#c1"]}]}]
        with WorkerProcess(TZ) as w:
            err = error_of(w, steps, notes=1)
        self.assertEqual((err["kind"], err.get("step")), ("anki_error", 1), err)

    def test_a_preset_with_18_parameters_fails_for_a_new_card(self):
        # stats/card-info C11 (fsrs_failed): the model is built before the
        # history is looked at, so a new card with no row fails too.
        params = [0.5] * 18
        steps = [{"legacy_update": {"preset": "P", "patch": {"fsrsParams6": params}}}, {"observe": [{"card_stats": ["#c1"]}]}]
        with WorkerProcess(TZ) as w:
            err = error_of(w, steps, notes=1)
        self.assertEqual((err["kind"], err.get("step")), ("anki_error", 1), err)


class GraphData(unittest.TestCase):
    TODAY_NOON = (N - 86_400 + 8 * 3600) * 1000  # 12:00 local on day 0

    def test_parts_in_the_protobuf_form(self):
        rows = revlog(
            (self.TODAY_NOON, 1, 1, 3, 30, 4_000),  # deleted card's Review, mature, Good
            (self.TODAY_NOON + 1, 1, 0, 1, 0, 1_000),  # Learn, Again
        )
        parts = ["reviews", "true_retention", "hours", "buttons", "card_counts", "added", "context", "intervals", "stability"]
        steps = [rows, {"observe": [{"stats": [
            {"scope": "collection", "history": "year", "parts": parts},
            {"scope": "collection", "history": "all", "parts": ["true_retention", "hours", "buttons"]},
        ]}]}]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/stats", TZ, START, steps, notes=1))
        year, all_ = out[1]["stats"]
        self.assertEqual(year["reviews"], {
            "count": {"0": {"learn": 1, "mature": 1}},
            "time": {"0": {"learn": 1_000, "mature": 4_000}},
        })
        zero = {}
        self.assertEqual(year["true_retention"], {
            "today": {"mature_passed": 1}, "yesterday": zero, "week": {"mature_passed": 1},
            "month": {"mature_passed": 1}, "year": {"mature_passed": 1},
        })
        hours = [{} for _ in range(24)]
        hours[12] = {"total": 2, "correct": 1}
        self.assertEqual(year["hours"], {"one_month": hours, "three_months": hours, "one_year": hours})
        buttons = {"learning": [1, 0, 0, 0], "young": [0, 0, 0, 0], "mature": [0, 0, 1, 0]}
        self.assertEqual(year["buttons"], {"one_month": buttons, "three_months": buttons, "one_year": buttons})
        self.assertEqual(year["card_counts"], {"including_inactive": {"newCards": 1}, "excluding_inactive": {"newCards": 1}})
        self.assertEqual(year["added"], {"added": {"0": 1}})
        self.assertEqual(year["context"], {"rollover_hour": 4, "fsrs": True})
        self.assertEqual(year["intervals"], {})
        self.assertEqual(year["stability"], {})
        self.assertEqual(all_["hours"]["all_time"], hours)
        self.assertEqual(all_["buttons"]["all_time"], buttons)
        self.assertEqual(all_["true_retention"]["all_time"], {"mature_passed": 1})

    def test_scopes_errors_and_unreadable_rows(self):
        setup = {
            "decks": [{"name": "A", "preset": "P"}, {"name": "B", "preset": "P"}],
            "notes": [{"notetype": "QA", "deck": "A", "fields": ["q1", "a"]}, {"notetype": "QA", "deck": "B", "fields": ["q2", "a"]}],
        }
        old = (N - 400 * 86_400) * 1000
        steps = [
            revlog((old, FIRST_CARD, 1, 3, 30, 0)),
            {"observe": [{"stats": [
                {"scope": {"deck": "A"}, "history": "all", "parts": ["card_counts"]},
                {"scope": {"deck": "B"}, "history": "all", "parts": ["card_counts"]},
                {"scope": {"search": "prop:ivl>"}, "history": "year", "parts": ["added"]},
                {"scope": {"search": "\"abc"}, "history": "year", "parts": ["added"]},
            ]}]},
            {"db_exec": {"sql": f"UPDATE revlog SET ease = 300 WHERE id = {old}"}},
            {"observe": [{"stats": [
                {"scope": "collection", "history": "year", "parts": ["reviews"]},
                {"scope": "collection", "history": "all", "parts": ["reviews"]},
            ]}]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/stats", TZ, START, steps, setup=setup))
            err = error_of(w, [{"observe": [{"stats": [{"scope": {"deck": "Nope"}, "history": "year", "parts": ["added"]}]}]}])
        deck_a, deck_b, bad_prop, bad_quote = out[1]["stats"]
        self.assertEqual(deck_a, {"card_counts": {"including_inactive": {"newCards": 1}, "excluding_inactive": {"newCards": 1}}})
        self.assertEqual(deck_b, deck_a)
        self.assertEqual(set(bad_prop) - {"provided", "context"}, {"error"})
        self.assertEqual(bad_quote, {"error": "unclosed_quote"})
        self.assertEqual(out[3]["stats"], [{"reviews": {}}, {"error": "unreadable_row"}])
        self.assertEqual((err["kind"], err.get("ref"), err.get("step")), ("unknown_ref", "Nope", 0), err)

    def test_arguments_are_checked(self):
        ok = {"scope": "collection", "history": "year", "parts": ["added"]}
        bad = [
            None, [], ok, [{}],
            [dict(ok, scope="Collection")], [dict(ok, scope={"deck": ""})], [dict(ok, scope={"search": 1})],
            [dict(ok, history="365")], [dict(ok, history=None)], [dict(ok, parts=[])], [dict(ok, parts=["today"])],
            [dict(ok, parts=["added", "added"])], [dict(ok, extra=1)], [{"scope": "collection", "parts": ["added"]}],
        ]
        with WorkerProcess(TZ) as w:
            for args in bad:
                with self.subTest(args=args):
                    err = error_of(w, [{"observe": [{"stats": args}]}])
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)

    def test_read_only(self):
        parts = ["reviews", "true_retention", "hours", "buttons", "future_due", "card_counts", "intervals",
                 "stability", "difficulty", "retrievability", "added", "context"]
        steps = [
            revlog((self.TODAY_NOON, FIRST_CARD, 1, 3, 30, 4_000)),
            WARM,
            {"observe": ["db_dump"]},
            {"observe": [{"stats": [
                {"scope": "collection", "history": "year", "parts": parts},
                {"scope": {"deck": "A"}, "history": "all", "parts": parts},
                {"scope": {"search": "is:new"}, "history": "all", "parts": parts},
            ]}]},
            {"observe": ["db_dump"]},
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/stats", TZ, START, steps, notes=2))
        self.assertEqual(out[2], out[4])


class GraphPreferences(unittest.TestCase):
    """graph_preferences (H13): stats/graphs-common G7's readings, worked by hand."""

    KEYS = ("cardCountsSeparateInactive", "futureDueShowBacklog", "firstDayOfWeek")

    @staticmethod
    def prefs(weekday, separate, backlog):
        return {"graph_preferences": {
            "calendar_first_day_of_week": weekday,
            "card_counts_separate_inactive": separate,
            "future_due_show_backlog": backlog,
        }}

    def writes(self, *values):
        return [{"set_config": {"key": k, "value": v}} for k, v in zip(self.KEYS, values)]

    def test_readings_defaults_and_removal(self):
        read = {"observe": ["graph_preferences"]}
        steps = [
            read,
            *self.writes(False, False, 1), read,
            *self.writes(True, True, 5), read,
            *self.writes(0, "false", 6), read,
            *self.writes(None, 1, 3), read,
            *[{"remove_config": {"key": k}} for k in self.KEYS], read,
        ]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/prefs", TZ, START, steps))
        reads = [o for o in out if "graph_preferences" in o]
        self.assertEqual(reads, [
            self.prefs(0, True, True),
            self.prefs(1, False, False),
            self.prefs(5, True, True),
            self.prefs(6, True, True),
            self.prefs(0, True, True),
            self.prefs(0, True, True),
        ])

    def test_arguments_are_checked(self):
        with WorkerProcess(TZ) as w:
            for spec in ({"graph_preferences": {}}, {"graph_preferences": []}, "graph_preferences(#c1)"):
                with self.subTest(spec=spec):
                    err = error_of(w, [{"observe": [spec]}], notes=1)
                    self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)

    def test_read_only(self):
        steps = [WARM, {"observe": ["db_dump"]}, {"observe": ["graph_preferences"]}, {"observe": ["db_dump"]}]
        with WorkerProcess(TZ) as w:
            out = run_ok(self, w, scenario("t/prefs", TZ, START, steps, notes=1))
        self.assertEqual(out[1], out[3])


if __name__ == "__main__":
    unittest.main()
