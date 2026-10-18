# SPDX-License-Identifier: AGPL-3.0-or-later
"""FSRS policy ops (specs/conformance/fsrs-policy-ops.md F1-F3) through real
workers: enable_fsrs with its F2 check, and enable_v3. Expected values come
from the scenario (its clock, config and SQL), the spec texts and db_dump
observations taken in the same run, never from the driver."""

from __future__ import annotations

import hashlib
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok, scenario

START = "2030-01-15T10:00:00"
# The four global keys F2.1 needs, as the compact JSON Anki writes for them.
GLOBAL_KEYS = {
    "cardStateCustomizer": "",
    "newCardsIgnoreReviewLimit": False,
    "applyAllParentLimits": False,
    "fsrsHealthCheck": True,
}


def config(fsrs):
    return {"config": {"fsrs": fsrs, **GLOBAL_KEYS}}


def run(worker, steps, notes=0, setup=None):
    return worker.request({"cmd": "run", "scenario": scenario("t/fsrs", "UTC", START, steps, notes=notes, setup=setup)})


def detail_rows(test, error):
    """The rows an F2 failure lists at the end of its detail."""
    # protocol.json's error object allows no other member here.
    test.assertEqual(set(error), {"kind", "step", "detail"}, error)
    return error["detail"].rsplit(": ", 1)[1].split(", ")


def changed_rows(before, after):
    """{(table, key)} of the db_dump rows that differ, appear or disappear."""
    out = set()
    for table in before.keys() | after.keys():
        b, a = before.get(table, {}), after.get(table, {})
        out |= {(table, key) for key in b.keys() | a.keys() if b.get(key) != a.get(key)}
    return out


class Arguments(unittest.TestCase):
    def test_both_ops_take_exactly_an_empty_mapping(self):
        with WorkerProcess("UTC") as w:
            for name in ("enable_fsrs", "enable_v3"):
                for args in (None, [], "", 0, {"deck": "Default"}):
                    resp = run(w, [{"observe": ["timing"]}, {name: args}], setup=config(False))
                    with self.subTest(op=name, args=args):
                        self.assertFalse(resp["ok"], resp)
                        self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1))


class EnableFsrs(unittest.TestCase):
    def test_turns_fsrs_on_and_outputs_nothing(self):
        # The first day query of a new collection writes rollover: the timing
        # observation makes it before the step (F2.1).
        steps = [
            {"observe": ["timing"]},
            {"advance": "1m"},
            {"enable_fsrs": {}},
            {"db_exec": {"sql": "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY = 'fsrs'"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/fsrs", "UTC", START, steps, setup=config(False)))
        self.assertEqual(out[2], {})
        self.assertEqual(out[3]["db_exec"]["rows"], [["fsrs", -1, epoch(START, "UTC") + 60, "true"]])

    def test_with_fsrs_already_on_it_writes_nothing(self):
        steps = [
            {"answer": {"card": "#c1", "rating": "good", "time_ms": 3000}},
            {"advance": "10m"},
            {"answer": {"card": "#c1", "rating": "good", "time_ms": 3000}},
            {"answer": {"card": "#c2", "rating": "again", "time_ms": 3000}},
            {"advance": "1m"},
            {"observe": ["db_dump"]},
            {"enable_fsrs": {}},
            {"observe": ["db_dump"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/fsrs", "UTC", START, steps, notes=2, setup=config(True)))
        self.assertEqual(out[6], {})
        before, after = out[5]["db_dump"], out[7]["db_dump"]
        # The cards carry memory states, so a recomputation would show.
        self.assertTrue(all('"s":' in row["data"] for row in before["cards"].values()), before["cards"])
        self.assertEqual(changed_rows(before, after), set())

    def test_check_fails_the_step_when_a_global_key_is_missing(self):
        with WorkerProcess("UTC") as w:
            for key in GLOBAL_KEYS:
                steps = [
                    {"observe": ["timing"]},
                    {"db_exec": {"sql": "DELETE FROM config WHERE KEY = ?", "params": [key]}},
                    {"advance": "1m"},
                    {"enable_fsrs": {}},
                ]
                resp = run(w, steps, setup=config(False))
                with self.subTest(key=key):
                    self.assertFalse(resp["ok"], resp)
                    error = resp["error"]
                    self.assertEqual((error["kind"], error.get("step")), ("invalid_scenario", 3))
                    # Anki's save adds the missing key back.
                    self.assertEqual(detail_rows(self, error), [f"config:{key}"])

    def test_check_fails_the_step_when_a_deck_preset_id_names_no_preset(self):
        # Deck A uses preset P (the scenario helper's setup); P is removed.
        steps = [
            {"observe": ["timing"]},
            {"db_exec": {"sql": "SELECT id FROM decks WHERE name = 'A'"}},
            {"db_exec": {"sql": "DELETE FROM deck_config WHERE name = 'P'"}},
            {"advance": "1m"},
            {"enable_fsrs": {}},
        ]
        with WorkerProcess("UTC") as w:
            first = run_ok(self, w, scenario("t/fsrs", "UTC", START, steps[:2], setup=config(False)))
            resp = run(w, steps, setup=config(False))
        [[deck_a]] = first[1]["db_exec"]["rows"]
        self.assertFalse(resp["ok"], resp)
        error = resp["error"]
        self.assertEqual((error["kind"], error.get("step")), ("invalid_scenario", 4))
        # Anki's save moves deck A to the Default deck's preset.
        self.assertEqual(detail_rows(self, error), [f"decks:{deck_a}"])


class EnableV3(unittest.TestCase):
    def test_writes_only_sched2021_when_creation_offset_is_present(self):
        steps = [
            {"db_exec": {"sql": "DELETE FROM config WHERE KEY = 'sched2021'"}},
            {"advance": "1m"},
            {"reopen": {}},
            {"db_exec": {"sql": "SELECT count(*) FROM config WHERE KEY = 'creationOffset'"}},
            {"observe": ["db_dump"]},
            {"advance": "1m"},
            {"enable_v3": {}},
            {"observe": ["db_dump"]},
            {"advance": "1m"},
            {"enable_v3": {}},
            {"observe": ["db_dump"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/v3", "UTC", START, steps))
        t = epoch(START, "UTC") + 120
        self.assertEqual(out[3]["db_exec"]["rows"], [[1]])
        self.assertEqual(out[6], {})
        before, after, again = out[4]["db_dump"], out[7]["db_dump"], out[10]["db_dump"]
        self.assertEqual(changed_rows(before, after), {("config", "sched2021"), ("col", "1")})
        self.assertNotIn("sched2021", before["config"])
        # Stored as Anki's JSON true, with the local usn, stamped now.
        self.assertEqual(after["config"]["sched2021"], {
            "KEY": "sched2021", "usn": -1, "mtime_secs": t, "val": {"sha256": hashlib.sha256(b"true").hexdigest()},
        })
        self.assertEqual(after["col"]["1"]["mod"], t * 1000)
        self.assertEqual({k: v for k, v in after["col"]["1"].items() if k != "mod"},
                         {k: v for k, v in before["col"]["1"].items() if k != "mod"})
        # On the current scheduler it writes nothing, col.mod included.
        self.assertEqual(changed_rows(after, again), set())


if __name__ == "__main__":
    unittest.main()
