# SPDX-License-Identifier: AGPL-3.0-or-later
"""The answer_labels observation (specs/conformance/observations.md O10).

Expected labels come from specs/sched/answer-labels.md A7 (a 90-minute step
shows 1.5h and Hard on it 2.3h; steps 5m 20m under the 1,200 s limit show <5m,
<13m and 20m; a 0 s step shows the end label). Review labels are worked out
here from the next_states the same step records (A1 to A4: a review state's
delay is scheduled_days days, shown in days below 30.42 days)."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from anki._backend import RustBackend
from anki.collection import Collection

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.harness import WorkerProcess
from driver.observe.labels import split_label
from driver.session import Session

MARKS = ("⁨", "⁩")
DAY = 86400
MONTH = 2628000  # A2: a twelfth of 365 days

SETUP = {
    "config": {"fsrs": True, "loadBalancerEnabled": False, "collapseTime": 1200},
    "presets": [
        {"name": "Hours", "learn_steps": ["90m"], "relearn_steps": ["10m"]},
        {"name": "Minutes", "learn_steps": ["5m", "20m"], "relearn_steps": ["10m"]},
        {"name": "Zero", "learn_steps": ["0s"], "relearn_steps": ["10m"]},
    ],
    "decks": [{"name": "Hours", "preset": "Hours"}, {"name": "Minutes", "preset": "Minutes"},
              {"name": "Zero", "preset": "Zero"}],
    "notetypes": [{"name": "QA", "fields": ["Q", "A"], "templates": [{"name": "C1", "front": "{{Q}}", "back": "{{A}}"}],
                   "css": ".card {}"}],
    "notes": [{"notetype": "QA", "deck": d, "fields": [f"q{i}", "a"]} for i, d in enumerate(("Hours", "Minutes", "Zero"))],
}


def label(amount, unit, less_than=False):
    return {"amount": amount, "less_than": less_than, "unit": unit}


END = label("", "end")


class SplitLabel(unittest.TestCase):
    """Anki's English label text, isolation marks included, split into parts."""

    def test_amounts_prefixes_and_units(self):
        cases = {
            "⁨45⁩s": label("45", "seconds"),
            "<⁨10⁩m": label("10", "minutes", True),
            "⁨1.5⁩h": label("1.5", "hours"),
            "⁨1⁩d": label("1", "days"),
            "⁨2.3⁩mo": label("2.3", "months"),
            "<⁨1.1⁩y": label("1.1", "years", True),
            "26d": label("26", "days"),
            "(end)": END,
        }
        for text, parts in cases.items():
            with self.subTest(text=text):
                self.assertEqual(split_label(text), parts)

    def test_text_that_does_not_split_gives_none(self):
        for text in ("", "1,5h", "1.5 h", "10min", "10", "h", "<<10m", "-1d", "1.5.2h", "10m\n", " 10m",
                     "<(end)", "(end) ", "١٠m", ".5h", "1.h", "10M", "2mo2"):
            with self.subTest(text=text):
                self.assertIsNone(split_label(text))


class ObservationErrors(unittest.TestCase):
    """In-process sessions: Anki's label text or language replaced, so the
    observation's refusals can be seen."""

    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))
        sc = {"id": "t/labels", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "setup": SETUP, "steps": []}
        self.session = Session(sc, "UTC", self.tmp.name, self.clock)

    def tearDown(self):
        self.session.close()
        self.tmp.cleanup()

    def test_a_label_that_does_not_split_is_an_anki_error_without_its_text(self):
        backend = self.session.col._backend
        real = backend.describe_next_states
        # The second card's Hard label has a comma; the others are Anki's own.
        calls = []

        def describe(states):
            calls.append(states)
            labels = list(real(states))
            if len(calls) == 2:
                labels[1] = "⁨1,5⁩h"
            return labels

        backend.describe_next_states = describe
        with self.assertRaises(DriverError) as ctx:
            self.session.run_step(3, {"observe": ["answer_labels"]})
        err = ctx.exception.to_json()
        self.assertEqual({k: err.get(k) for k in ("kind", "op", "observation", "step")},
                         {"kind": "anki_error", "op": "observe", "observation": "answer_labels", "step": 3})
        self.assertNotIn("1,5", json.dumps(err, ensure_ascii=False))
        self.assertEqual(self.session.outputs, [])

    def test_a_backend_in_another_language_is_refused(self):
        self.session.col.close(downgrade=False)
        self.session.col = Collection(self.session.path, backend=RustBackend(langs=["pl"]))
        with self.assertRaises(DriverError) as ctx:
            self.session.run_step(0, {"observe": ["answer_labels(#c1)"]})
        # The driver's refusal, not an exception it caught (that would name its class).
        self.assertEqual(ctx.exception.to_json(), {"kind": "internal_error", "op": "observe", "observation": "answer_labels",
                                                   "detail": "the collection's backend is not English", "step": 0})


class WorkerLabels(unittest.TestCase):
    """A real worker (libfaketime, 10:00 UTC, next day at 04:00)."""

    def run_scenario(self):
        sc = {"id": "t/labels", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "setup": SETUP,
              "steps": [{"observe": ["answer_labels", "next_states"]}, {"observe": ["answer_labels(#c2)"]}]}
        with WorkerProcess("UTC") as w:
            resp = w.request({"cmd": "run", "scenario": sc})
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"]

    def review_label(self, state):
        # A1-A4 for a review state under the 1,200 s limit: whole days.
        self.assertEqual(state["kind"], "review")
        delay = state["scheduled_days"] * DAY
        self.assertTrue(DAY <= delay < MONTH, state)
        return label(str(state["scheduled_days"]), "days")

    def test_labels_of_learning_steps_review_intervals_and_the_end(self):
        first, second = self.run_scenario()
        labels, states = first["answer_labels"], first["next_states"]
        self.assertEqual(list(labels), ["#c1", "#c2", "#c3"])
        self.assertEqual(labels["#c1"], {"again": label("1.5", "hours"), "hard": label("2.3", "hours"),
                                         "good": self.review_label(states["#c1"]["good"]),
                                         "easy": self.review_label(states["#c1"]["easy"])})
        self.assertEqual(labels["#c2"], {"again": label("5", "minutes", True), "hard": label("13", "minutes", True),
                                         "good": label("20", "minutes"),
                                         "easy": self.review_label(states["#c2"]["easy"])})
        self.assertEqual(labels["#c3"]["again"], END)
        self.assertEqual(labels["#c3"]["easy"], self.review_label(states["#c3"]["easy"]))
        self.assertEqual(second, {"answer_labels": {"#c2": labels["#c2"]}})
        text = json.dumps(first, ensure_ascii=False)
        self.assertFalse(any(m in text for m in MARKS))


if __name__ == "__main__":
    unittest.main()
