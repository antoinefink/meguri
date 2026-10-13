# SPDX-License-Identifier: AGPL-3.0-or-later
"""Live-chain segments and handoffs (dsl.md D9.2) and recorded rejections
(D7.3), through real workers: a chain of segments, each on a fresh worker,
must observe what one whole run observes."""

from __future__ import annotations

import os
import shutil
import tempfile
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
# The whole run's steps; handoffs at 2 and 5 split it into three segments.
STEPS = [
    {"answer": {"card": "#c1", "rating": "good"}},
    {"advance": "1d"},
    {"handoff": "meguri"},
    {"add_note": {"notetype": "QA", "deck": "A", "fields": ["q2", "a2"]}},
    {"observe": ["card(#c1, #c2)", "timing"]},
    {"handoff": "oracle"},
    {"advance": "2h"},
    {"observe": ["card", "revlog(#c1)", "ids", "timing"]},
]


class Handoff(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="handoff-", dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        self.env = {"ORACLE_HANDOFF": self.root}

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def segment(self, sc, steps, resume=None, handoff=None):
        req = {"cmd": "run", "scenario": sc, "steps": steps}
        if resume is not None:
            req["resume"] = resume
        if handoff is not None:
            req["handoff"] = handoff
        with WorkerProcess("UTC", extra_env=self.env) as w:
            resp = w.request(req)
        self.assertTrue(resp["ok"], resp)
        return resp

    def test_a_chain_of_segments_observes_what_one_whole_run_observes(self):
        sc = scenario("t/chain", "UTC", START, STEPS, notes=1)
        with WorkerProcess("UTC", extra_env=self.env) as w:
            whole = run_ok(self, w, sc)
        self.assertEqual((whole[2], whole[5]), ({}, {}))

        first = self.segment(sc, [0, 2], handoff="t/chain/2")
        state = first["state"]
        self.assertEqual(state["dir"], "t/chain/2")
        self.assertEqual(len(state["notes"]), 1)
        self.assertTrue(os.path.isfile(os.path.join(self.root, "t/chain/2/collection.anki2")))
        # The next side resumes at the handed clock and ids, without setup,
        # and finds the scenario's note types and decks by name.
        second = self.segment(sc, [3, 5], resume=state, handoff="t/chain/5")
        self.assertEqual(len(second["state"]["cards"]), 2)
        third = self.segment(sc, [6, 8], resume=second["state"])
        self.assertNotIn("state", third)
        chained = first["observations"]["steps"] + [{}] + second["observations"]["steps"] + [{}] + third["observations"]["steps"]
        self.assertEqual(chained, whole)

    def test_handoff_errors_are_structured(self):
        sc = scenario("t/chain", "UTC", START, STEPS, notes=1)
        first = self.segment(sc, [0, 2], handoff="t/chain/2")
        with WorkerProcess("UTC") as no_dir, WorkerProcess("UTC", extra_env=self.env) as w:
            cases = {
                "no ORACLE_HANDOFF": no_dir.request({"cmd": "run", "scenario": sc, "steps": [3, 5], "resume": first["state"]}),
                "already there": w.request({"cmd": "run", "scenario": sc, "steps": [0, 2], "handoff": "t/chain/2"}),
                "outside": w.request({"cmd": "run", "scenario": sc, "steps": [0, 2], "handoff": "../x"}),
                "no collection": w.request({"cmd": "run", "scenario": sc, "steps": [3, 5],
                                            "resume": dict(first["state"], dir="t/none")}),
            }
            bad_range = w.request({"cmd": "run", "scenario": sc, "steps": [5, 3]})
            bad_state = w.request({"cmd": "run", "scenario": sc, "steps": [3, 5], "resume": {"dir": "t/chain/2"}})
            twice = w.request({"cmd": "run", "scenario": scenario("t/x", "UTC", START, [{"handoff": "oracle"}, {"handoff": "oracle"}])})
            alive = w.request({"cmd": "hello"})
        for case, resp in cases.items():
            self.assertEqual((resp["ok"], resp["error"]["kind"]), (False, "handoff_error"), f"{case}: {resp}")
        self.assertEqual(bad_range["error"]["kind"], "protocol_error")
        self.assertEqual(bad_state["error"]["kind"], "protocol_error")
        self.assertEqual((twice["error"]["kind"], twice["error"]["step"]), ("invalid_scenario", 1))
        self.assertTrue(alive["ok"])


class Rejection(unittest.TestCase):
    def test_a_refusal_in_the_ops_list_is_recorded_and_the_run_goes_on(self):
        env = {"ORACLE_EXTRA_MODULES": "driver.tests.extra_area"}
        steps = [{"claim": "taken"}, {"claim": "free"}, {"observe": ["ids(#n1)"]}]
        with WorkerProcess("UTC", extra_env=env) as w:
            out = run_ok(self, w, scenario("t/claim", "UTC", START, steps, notes=1))
            other = w.request({"cmd": "run", "scenario": scenario("t/other", "UTC", START, [{"claim": "other"}])})
        self.assertEqual(out[:2], [{"claim": {"error": "taken"}}, {"claim": "free"}])
        self.assertIn("ids", out[2])
        # A kind outside the op's list is a driver bug, not an output.
        self.assertEqual((other["ok"], other["error"]["kind"], other["error"]["step"]), (False, "internal_error", 0))


if __name__ == "__main__":
    unittest.main()
