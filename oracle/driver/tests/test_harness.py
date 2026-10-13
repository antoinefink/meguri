# SPDX-License-Identifier: AGPL-3.0-or-later
"""record, compare and diff-report through the harness entry point."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from driver import harness
from driver.canon import load_scenario_file, scenario_sha256

SCENARIO = """\
# FSRS: first answers of one new card
id: smoke/fsrs
clock: {tz: Europe/Paris, start: "2030-01-15T10:00:00"}
setup:
  config: {fsrs: true}
  presets: [{name: P, learn_steps: [1m, 10m], desired_retention: 0.9}]
  decks: [{name: A, preset: P}]
  notetypes:
    - {name: QA, fields: [Q, A], templates: [{name: C1, front: "{{Q}}", back: "{{A}}"}]}
  notes: [{notetype: QA, deck: A, fields: [q1, a1]}]
steps:
  - observe: [next_states(#c1)]
  - answer: {card: "#c1", rating: good}
  - advance: 1d
  - observe: [next_states(#c1), card(#c1), revlog(#c1), timing]
"""

UTC_SCENARIO = """\
id: other/utc
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
steps:
  - observe: [timing]
"""


class Harness(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.scenarios, self.goldens, self.report = root / "scenarios", root / "goldens", root / "report.json"
        (self.scenarios / "smoke").mkdir(parents=True)
        (self.scenarios / "other").mkdir()
        (self.scenarios / "smoke" / "fsrs.yaml").write_text(SCENARIO)
        (self.scenarios / "smoke" / "area.yaml").write_text("active: [fsrs]\n")
        (self.scenarios / "other" / "utc.yaml").write_text(UTC_SCENARIO)

    def tearDown(self):
        self.tmp.cleanup()

    def harness(self, *argv):
        rc = harness.main([*argv, "--report", str(self.report)] if argv[0] != "diff-report" else list(argv))
        report = json.loads(self.report.read_text()) if self.report.exists() else None
        return rc, report

    def common(self, cmd, *extra):
        return self.harness(cmd, "--scenarios", str(self.scenarios), "--goldens", str(self.goldens), "--jobs", "2", *extra)

    def test_record_then_compare_round_trip(self):
        rc, report = self.common("record")
        self.assertEqual(rc, 0, report)
        self.assertEqual(report["totals"], {"recorded": 2})
        golden = json.loads((self.goldens / "smoke" / "fsrs.json").read_text())
        hello = report["oracle"]
        self.assertEqual(golden["scenario_id"], "smoke/fsrs")
        self.assertEqual(golden["scenario_sha256"], scenario_sha256(load_scenario_file(self.scenarios / "smoke" / "fsrs.yaml")))
        self.assertEqual(golden["oracle"], hello)
        self.assertEqual(golden["tz"], "Europe/Paris")
        self.assertEqual(golden["clock"], {"start": "2030-01-15T10:00:00", "mode": "frozen"})
        steps = golden["observations"]["steps"]
        self.assertEqual(len(steps), 4)
        self.assertEqual(steps[3]["timing"]["days_elapsed"], 1)
        self.assertFalse((self.goldens / "smoke" / "area.json").exists())

        rc, report = self.common("compare")
        self.assertEqual((rc, report["totals"]), (0, {"pass": 2}))

        # A comment or key-order edit keeps the golden fresh.
        path = self.scenarios / "smoke" / "fsrs.yaml"
        path.write_text("# reformatted\n" + SCENARIO.replace("{tz: Europe/Paris, start:", "{start:").replace(
            '"2030-01-15T10:00:00"}', '"2030-01-15T10:00:00", tz: Europe/Paris}'))
        rc, report = self.common("compare", "--filter", "smoke")
        self.assertEqual((rc, report["totals"]), (0, {"pass": 1}))

    def test_compare_reports_drift_staleness_and_missing_goldens(self):
        self.assertEqual(self.common("record", "--filter", "smoke")[0], 0)
        golden_path = self.goldens / "smoke" / "fsrs.json"
        golden = json.loads(golden_path.read_text())
        good = golden["observations"]["steps"][3]["next_states"]["#c1"]["good"]
        good["scheduled_days"] += 1
        golden_path.write_text(json.dumps(golden))
        rc, report = self.common("compare")
        by_name = {s["scenario"]: s for s in report["scenarios"]}
        self.assertEqual(rc, 1)
        self.assertEqual(by_name["other/utc"]["status"], "missing")
        drift = by_name["smoke/fsrs"]
        self.assertEqual(drift["status"], "fail")
        self.assertEqual(drift["first_step"], 3)
        self.assertEqual(drift["diffs"][0]["pointer"], "/steps/3/next_states/#c1/good/scheduled_days")
        self.assertEqual(drift["diffs"][0]["expected"], drift["diffs"][0]["actual"] + 1)

        golden["oracle"]["lock_sha256"] = "0" * 64
        good["scheduled_days"] -= 1
        golden_path.write_text(json.dumps(golden))
        rc, report = self.common("compare", "--filter", "smoke/fsrs")
        self.assertEqual(report["scenarios"][0]["status"], "stale_oracle")

        path = self.scenarios / "smoke" / "fsrs.yaml"
        path.write_text(SCENARIO.replace("rating: good", "rating: hard"))
        rc, report = self.common("compare", "--filter", "smoke/*")
        self.assertEqual((rc, report["scenarios"][0]["status"]), (1, "stale_scenario"))

    def test_invalid_scenarios_and_errors_are_reported_not_recorded(self):
        (self.scenarios / "smoke" / "broken.yaml").write_text("id: smoke/broken\nclock: {tz: UTC, start: x}\nsteps: []\n")
        (self.scenarios / "smoke" / "alias.yaml").write_text("id: &a smoke/alias\nb: *a\n")
        rc, report = self.common("record", "--filter", "smoke/broken", "--filter", "smoke/alias")
        by_name = {s["scenario"]: s for s in report["scenarios"]}
        self.assertEqual(rc, 1)
        self.assertEqual(by_name["smoke/broken"]["error"]["kind"], "invalid_scenario")
        self.assertEqual(by_name["smoke/alias"]["error"]["kind"], "invalid_scenario")
        self.assertFalse((self.goldens / "smoke" / "broken.json").exists())

    def test_diff_report_groups_changes_by_area_and_kind(self):
        self.assertEqual(self.common("record")[0], 0)
        new = Path(self.tmp.name) / "new"
        for p in self.goldens.rglob("*.json"):
            target = new / p.relative_to(self.goldens)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(p.read_text())
        changed = json.loads((new / "smoke" / "fsrs.json").read_text())
        changed["observations"]["steps"][3]["card"]["#c1"]["ivl"] += 1
        changed["observations"]["steps"][3]["timing"]["next_day_at"] += 1
        (new / "smoke" / "fsrs.json").write_text(json.dumps(changed))
        md = Path(self.tmp.name) / "report.md"
        rc, report = self.harness("diff-report", "--old", str(self.goldens), "--new", str(new),
                                  "--report", str(self.report), "--markdown", str(md))
        self.assertEqual(rc, 0)
        self.assertEqual(report["totals"], {"changed": 1, "unchanged": 1})
        self.assertEqual(
            report["changed_observations"],
            [{"area": "smoke", "kind": "card", "count": 1}, {"area": "smoke", "kind": "timing", "count": 1}],
        )
        self.assertIn("| smoke | card | 1 |", md.read_text())


if __name__ == "__main__":
    unittest.main()
