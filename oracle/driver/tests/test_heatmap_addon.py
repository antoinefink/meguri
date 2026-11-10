# SPDX-License-Identifier: AGPL-3.0-or-later
"""heatmap_totals and heatmap_report (specs/conformance/stats-ops.md H1, H11)
through real workers on the add-on image (oracle/heatmap/Dockerfile). Expected
values come from the rows and cards each test writes and the add-on 1.0.1's
rules as its source states them (history and forecast queries, _get_activity),
worked by hand with zoneinfo, never from the driver.

They need the add-on image, which sets ORACLE_HEATMAP_ADDON; elsewhere they
skip (oracle/heatmap/README.md)."""

from __future__ import annotations

import os
import shutil
import tempfile
import unittest
from datetime import date, datetime, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok, scenario

TZ = "Europe/Paris"
START = "2030-01-15T10:00:00"  # Anki's today 0 (the collection's first day)
ADDON = os.environ.get("ORACLE_HEATMAP_ADDON")
SETUP = {
    "decks": [{"name": "A", "preset": "P"}, {"name": "A::B", "preset": "P"}, {"name": "C", "preset": "P"}],
    "notes": [
        {"notetype": "QA", "deck": "A", "fields": ["q1", "a"]},
        {"notetype": "QA", "deck": "A::B", "fields": ["q2", "a"]},
        {"notetype": "QA", "deck": "C", "fields": ["q3", "a"]},
    ],
}
CONFIG_ROWS = {"db_exec": {"sql": "SELECT KEY, usn, mtime_secs, val FROM config WHERE KEY IN ('heatmap', 'curDeck') ORDER BY KEY"}}


def ms(local: str) -> int:
    return epoch(local, TZ) * 1000


def card_of(front: str) -> str:
    return f"(SELECT id FROM cards WHERE nid = (SELECT id FROM notes WHERE sfld = '{front}'))"


def revlog(*rows):
    """INSERT for (id ms, cid SQL, ease, time, type) rows."""
    values = ", ".join(f"({i}, {cid}, -1, {ease}, 1, 0, 2500, {t}, {kind})" for i, cid, ease, t, kind in rows)
    return {"db_exec": {"sql": f"INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES {values}"}}


def due(front: str, day: int, queue: int = 2):
    """Make the card of note `front` a review card (queue 2, or 3) due on Anki's day `day`."""
    return {"db_exec": {"sql": f"UPDATE cards SET type = 2, queue = {queue}, ivl = 1, factor = 2500, due = {day} WHERE id = {card_of(front)}"}}


def run(test, steps, tz=TZ, start=START, setup=SETUP):
    with WorkerProcess(tz) as w:
        return run_ok(test, w, scenario("t/heatmap", tz, start, steps, setup=setup))


def errors(steps_list, tz=TZ):
    out = []
    with WorkerProcess(tz) as w:
        for steps in steps_list:
            resp = w.request({"cmd": "run", "scenario": scenario("t/heatmap", tz, START, steps, setup=SETUP)})
            out.append(resp.get("error"))
    return out


@unittest.skipUnless(ADDON, "needs the add-on image (ORACLE_HEATMAP_ADDON)")
class HeatmapTotals(unittest.TestCase):
    ROWS = revlog(
        # rollover 4: 03:59:59 is on the 14th, 04:00 on the 15th
        (ms("2030-01-15T03:59:59"), card_of("q1"), 3, 1_000, 1),
        (ms("2030-01-15T04:00:00"), card_of("q2"), 3, 1_000, 1),
        (ms("2030-01-15T09:00:00"), 1, 3, 1_000, 1),  # a deleted card's row
        (ms("2030-01-15T09:30:00"), card_of("q1"), 0, 0, 4),  # no rating: limresched drops it
    )

    def test_home_and_decks_count_rated_rows_per_day(self):
        out = run(self, [self.ROWS, {"observe": [{"heatmap_totals": {"decks": ["A", "A::B", "C"]}}]}])
        self.assertEqual(
            out[1]["heatmap_totals"],
            {
                "home": {"2030-01-14": 1, "2030-01-15": 2},
                "decks": {"A": {"2030-01-14": 1, "2030-01-15": 1}, "A::B": {"2030-01-15": 1}, "C": {}},
            },
        )

    def test_the_key_is_read_at_each_observation_and_never_written(self):
        out = run(self, [
            self.ROWS,
            CONFIG_ROWS,
            {"observe": [{"heatmap_totals": {"decks": ["A::B"]}}, {"heatmap_report": {"decks": ["A::B"]}}]},
            CONFIG_ROWS,
            {"set_config": {"key": "heatmap", "value": {"limresched": False}}},
            CONFIG_ROWS,
            {"observe": ["heatmap_totals"]},
            CONFIG_ROWS,
            {"set_config": {"key": "heatmap", "value": {"limcdel": True, "version": "1.0.1", "limhist": 0, "limdate": 0,
                                                        "limfcst": 0, "limresched": True, "limdecks": [], "mode": "year",
                                                        "colors": "lime"}}},
            {"observe": ["heatmap_totals"]},
        ])
        # An absent key stays absent; the current deck stays where it was.
        self.assertEqual([r[0] for r in out[1]["db_exec"]["rows"]], ["curDeck"])
        self.assertEqual(out[3], out[1])
        # A key without a version takes the defaults for its missing members
        # and keeps its bytes.
        self.assertEqual(out[6]["heatmap_totals"], {"home": {"2030-01-14": 1, "2030-01-15": 3}})
        self.assertEqual(out[7], out[5])
        # The next observation reads the new key: limcdel drops the deleted card's row.
        self.assertEqual(out[9]["heatmap_totals"], {"home": {"2030-01-14": 1, "2030-01-15": 1}})

    def test_an_unknown_deck_is_unknown_ref_and_arguments_are_checked(self):
        for kind in ("heatmap_totals", "heatmap_report"):
            first, *rest = errors(
                [[{"observe": [{kind: {"decks": ["A", "a::b"]}}]}]]
                + [[{"observe": [{kind: args}]}] for args in (
                    {"decks": []}, {"decks": "A"}, {"decks": ["A", "A"]}, {"decks": [""]}, {"decks": [1]},
                    {"deck": ["A"]}, {}, ["A"], {"decks": ["A"], "x": 1})]
                + [[{"observe": [f"{kind}(#c1)"]}]]
            )
            with self.subTest(kind=kind):
                self.assertEqual(first, {"kind": "unknown_ref", "ref": "a::b", "step": 0})
                self.assertEqual([(e or {}).get("kind") for e in rest], ["invalid_scenario"] * len(rest))

    def test_counts_go_to_the_day_the_calendar_draws_them_on(self):
        from driver.observe.heatmap_addon import drawn_date, key_date

        def key(day):
            return int(datetime.fromisoformat(day).replace(tzinfo=timezone.utc).timestamp())

        # The web view moves a key (00:00 UTC of a date) by the offset at that
        # instant, then the calendar takes the local date. Lord Howe went from
        # +10:30 to +11:00 at 2026-10-03 15:30 UTC: 2026-10-04 00:00 UTC minus
        # 11 h is 13:00 UTC on the 3rd, 23:30 local (+10:30), so the 4th's
        # count is drawn on the 3rd. Elsewhere the date stays.
        cases = [
            ("Australia/Lord_Howe", "2026-10-04", date(2026, 10, 3)),
            ("Australia/Lord_Howe", "2026-10-03", date(2026, 10, 3)),
            ("Australia/Lord_Howe", "2026-04-05", date(2026, 4, 5)),
            ("America/New_York", "2026-03-08", date(2026, 3, 8)),
            ("America/St_Johns", "2026-03-08", date(2026, 3, 8)),
            ("Europe/Paris", "2026-03-29", date(2026, 3, 29)),
            ("Asia/Kathmandu", "2026-10-12", date(2026, 10, 12)),
        ]
        for tz, day, drawn in cases:
            with self.subTest(tz=tz, day=day):
                self.assertEqual(drawn_date(key(day), ZoneInfo(tz)), drawn)
                self.assertEqual(key_date(key(day)), date.fromisoformat(day))

    def test_a_drawn_day_shift_adds_two_dates_up(self):
        # Lord Howe, rollover 4: rows at 2026-10-03 12:00 (+10:30) and
        # 2026-10-04 12:00 (+11:00) are on the 3rd and the 4th (B4.1); the
        # 4th's key is drawn on the 3rd, so the 3rd holds both.
        tz = "Australia/Lord_Howe"

        def at(local):
            return epoch(local, tz) * 1000

        out = run(self, [
            revlog((at("2026-10-03T12:00:00"), 1, 3, 1_000, 1), (at("2026-10-04T12:00:00"), 1, 3, 1_000, 1)),
            {"observe": ["heatmap_totals", "heatmap_report"]},
        ], tz=tz, start="2026-10-04T20:00:00")
        self.assertEqual(out[1]["heatmap_totals"], {"home": {"2026-10-03": 2}})
        report = out[1]["heatmap_report"]["home"]
        self.assertEqual((report["today"], report["history"]), ("2026-10-04", {"2026-10-03": 2}))
        # The streaks read the add-on's own keys: two days, the last today.
        self.assertEqual(report["streaks"], {"daily_average": 1, "days_learned": 100, "longest_streak": 2, "current_streak": 2})
        self.assertEqual(report["today_cell"], {"kind": "today", "count": 1})

    def test_only_the_pinned_add_on_loads(self):
        from driver.observe.heatmap_addon import differences

        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / "addon"
            shutil.copytree(ADDON, copy)
            self.assertEqual(differences(copy), [])
            pkg = copy / "src" / "review_heatmap"
            activity = (pkg / "activity.py").read_bytes()
            times = (pkg / "times.py").read_bytes()
            (pkg / "__pycache__").mkdir()
            (pkg / "__pycache__" / "x.pyc").write_bytes(b"cache")  # byte-code caches aside
            self.assertEqual(differences(copy), [])
            for change, undo in (
                (lambda: (pkg / "activity.py").write_bytes(activity + b"\n"), lambda: (pkg / "activity.py").write_bytes(activity)),
                (lambda: (pkg / "meta.json").write_text("{}"), lambda: (pkg / "meta.json").unlink()),
                (lambda: (pkg / "times.py").unlink(), lambda: (pkg / "times.py").write_bytes(times)),
                (lambda: (pkg / "times.py").rename(pkg / "time.py"), lambda: (pkg / "time.py").rename(pkg / "times.py")),
            ):
                change()
                with self.subTest(change=change):
                    [diff] = differences(copy)
                    self.assertRegex(diff, r"^tree [0-9a-f]{64} of \d+ files is not the pinned [0-9a-f]{64} of 100$")
                undo()
                self.assertEqual(differences(copy), [])


@unittest.skipUnless(ADDON, "needs the add-on image (ORACLE_HEATMAP_ADDON)")
class HeatmapReport(unittest.TestCase):
    def test_history_forecast_today_and_streaks(self):
        out = run(self, [
            revlog(
                (ms("2030-01-13T12:00:00"), 1, 3, 1_000, 1),
                (ms("2030-01-14T12:00:00"), 1, 3, 1_000, 1),
                (ms("2030-01-14T13:00:00"), card_of("q2"), 3, 1_000, 1),
                (ms("2030-01-15T09:00:00"), card_of("q1"), 3, 1_000, 1),
            ),
            due("q1", 0), due("q2", 2), due("q3", 2, queue=3),
            {"observe": [{"heatmap_report": {"decks": ["A", "A::B"]}}]},
        ])
        self.assertEqual(out[4]["heatmap_report"], {
            "home": {
                "today": "2030-01-15",
                "history": {"2030-01-13": 1, "2030-01-14": 2, "2030-01-15": 1},
                "forecast": {"2030-01-15": 1, "2030-01-17": 2},
                # _get_activity's stop: the last forecast key.
                "last_forecast_day": "2030-01-17",
                # The history's last day is today: its count wins over the forecast's.
                "today_cell": {"kind": "today", "count": 1},
                # 4 rows on 3 days: round(1.33) = 1; 3 of 3 days; one run of 3 ending today.
                "streaks": {"daily_average": 1, "days_learned": 100, "longest_streak": 3, "current_streak": 3},
            },
            "decks": {
                "A": {
                    "today": "2030-01-15",
                    "history": {"2030-01-14": 1, "2030-01-15": 1},
                    "forecast": {"2030-01-15": 1, "2030-01-17": 1},
                    "last_forecast_day": "2030-01-17",
                    "today_cell": {"kind": "today", "count": 1},
                    "streaks": {"daily_average": 1, "days_learned": 100, "longest_streak": 2, "current_streak": 2},
                },
                "A::B": {
                    "today": "2030-01-15",
                    "history": {"2030-01-14": 1},
                    "forecast": {"2030-01-17": 1},
                    "last_forecast_day": "2030-01-17",
                    # The history's last day is not today and the forecast has none on it.
                    "today_cell": {"kind": "today", "count": 0},
                    # One day, yesterday: 1 of 2 days (50 %); the current streak keeps it.
                    "streaks": {"daily_average": 1, "days_learned": 50, "longest_streak": 1, "current_streak": 1},
                },
            },
        })

    def test_no_history_still_reports_the_forecast(self):
        out = run(self, [
            due("q1", 0), due("q2", 0), due("q3", 5),
            {"observe": ["heatmap_report"]},
        ])
        self.assertEqual(out[3]["heatmap_report"], {"home": {
            "today": "2030-01-15",
            "history": {},
            "forecast": {"2030-01-15": 2, "2030-01-20": 1},
            "last_forecast_day": "2030-01-20",
            "today_cell": {"kind": "future", "count": 2},
            "streaks": None,
        }})

    def test_today_cell_from_the_forecast_when_today_has_no_review(self):
        out = run(self, [
            revlog((ms("2030-01-13T12:00:00"), 1, 3, 1_000, 1), (ms("2030-01-13T12:01:00"), 1, 3, 1_000, 1),
                   (ms("2030-01-13T12:02:00"), 1, 3, 1_000, 1)),
            due("q1", 0), due("q2", -1),
            {"observe": ["heatmap_report"]},
        ])
        report = out[3]["heatmap_report"]["home"]
        # Overdue cards are not drawn; the last day is two days back: no current streak.
        self.assertEqual(report["forecast"], {"2030-01-15": 1})
        self.assertEqual(report["today_cell"], {"kind": "future", "count": 1})
        self.assertEqual(report["streaks"], {"daily_average": 3, "days_learned": 33, "longest_streak": 1, "current_streak": 0})

    def test_overdue_only_has_no_forecast_and_limfcst_ends_before_its_day(self):
        out = run(self, [
            due("q1", -2), due("q2", -1),
            {"observe": ["heatmap_report"]},
            due("q1", 1), due("q2", 3), due("q3", 4),
            {"set_config": {"key": "heatmap", "value": {"limfcst": 3}}},
            {"observe": ["heatmap_report"]},
        ])
        first = out[2]["heatmap_report"]["home"]
        self.assertEqual((first["forecast"], first["last_forecast_day"], first["today_cell"]),
                         ({}, None, {"kind": "today", "count": 0}))
        # The forecast query keeps keys below today + 3 days: day 3 and 4 are out.
        second = out[7]["heatmap_report"]["home"]
        self.assertEqual((second["forecast"], second["last_forecast_day"]), ({"2030-01-16": 1}, "2030-01-16"))
