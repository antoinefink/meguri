"""Tests for the screen status ledger writer (U10, R1985): the transitions a
critic check makes, sample screens, tasks and explicit transitions."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
sys.path.insert(0, os.path.dirname(HERE))

import screen_status  # noqa: E402

PASS = {"run": "r", "status": "pass", "commit": "c"}


def issue(severity, rule="text-truncation", screen="sample.deckOverview", variant="dark-ax5-iphoneSmall", summary="Deck name cut off"):
    return {"screen": screen, "state": "overflow", "variant": variant, "image": "screens/x.png", "rule": rule,
            "severity": severity, "region": {"x": 0, "y": 0, "width": 10, "height": 10}, "summary": summary}


class ScreenStatusTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.ledger_path = os.path.join(self.dir, "screen-status.yaml")
        self.ledger = screen_status.load(self.ledger_path)

    def report(self, run, issues, evidence=None):
        return {"run": run, "issues": issues, "dropped": [], "evidence": evidence or {"snapshots": PASS, "a11y": PASS}}

    def test_a_high_issue_keeps_the_screen_out_of_verified_until_a_new_check_passes(self):
        # AE90: the truncated deck name keeps the screen built...
        status = screen_status.record_critic(self.ledger, "sample.deckOverview", self.report("critic-1", [issue("high")]))
        self.assertEqual(status, "built")
        entry = self.ledger["screens"]["sample.deckOverview"]
        self.assertEqual(entry["evidence"]["critic"]["status"], "fail")
        self.assertEqual(entry["issues"][0]["rule"], "text-truncation")
        self.assertEqual(entry["issues"][0]["region"], {"x": 0, "y": 0, "width": 10, "height": 10})
        # ...and a later check by a new critic with no high issue verifies it.
        status = screen_status.record_critic(self.ledger, "sample.deckOverview", self.report("critic-2", []))
        self.assertEqual(status, "verified")
        self.assertNotIn("issues", entry)
        self.assertEqual([h["to"] for h in entry["history"]], ["built", "verified"])

    def test_verified_needs_passing_snapshots_and_accessibility_runs(self):
        report = self.report("critic-1", [], {"snapshots": PASS, "a11y": {"run": "a", "status": "fail"}})
        self.assertEqual(screen_status.record_critic(self.ledger, "sample.x", report), "built")
        report = self.report("critic-2", [], {"snapshots": PASS})
        self.assertEqual(screen_status.record_critic(self.ledger, "sample.x", report), "built")
        self.assertEqual(screen_status.record_critic(self.ledger, "sample.x", self.report("critic-3", [])), "verified")

    def test_a_high_issue_demotes_a_verified_screen(self):
        screen_status.record_critic(self.ledger, "sample.x", self.report("c1", []))
        self.assertEqual(screen_status.record_critic(self.ledger, "sample.x", self.report("c2", [issue("high", screen="sample.x")])), "built")

    def test_medium_issues_become_tasks_once_and_do_not_block(self):
        medium = issue("medium", rule="layout-alignment", screen="sample.x", summary="Counts not aligned with the title")
        self.assertEqual(screen_status.record_critic(self.ledger, "sample.x", self.report("c1", [medium, issue("low", rule="copy-plain", screen="sample.x")])), "verified")
        screen_status.record_critic(self.ledger, "sample.x", self.report("c2", [medium]))
        tasks = self.ledger["screens"]["sample.x"]["tasks"]
        self.assertEqual(len(tasks), 1)
        self.assertEqual(tasks[0]["rule"], "layout-alignment")

    def test_the_canonical_ledger_takes_inventory_screens_only(self):
        inventory = {"home.deckList": {"states": ["typical"], "signature": True}}
        with self.assertRaises(screen_status.LedgerError):
            screen_status.check_screen_id("sample.deckOverview", inventory, canonical=True)
        screen_status.check_screen_id("sample.deckOverview", inventory, canonical=False)
        with self.assertRaises(screen_status.LedgerError):
            screen_status.check_screen_id("home.unknown", inventory, canonical=False)

    def test_problems_flag_a_verified_screen_with_a_high_issue(self):
        self.ledger["screens"]["home.deckList"] = {"status": "verified", "fix_rounds": 0, "issues": [issue("high", screen="home.deckList")]}
        found = screen_status.problems(self.ledger, {"home.deckList": {}}, canonical=True)
        self.assertTrue(any("high-severity" in p for p in found), found)
        self.assertTrue(any("without passing" in p for p in found), found)

    def test_approval_needs_a_verified_screen_a_logged_pick_and_an_approved_baseline(self):
        decisions = os.path.join(self.dir, "decisions.md")
        with open(decisions, "w") as handle:
            handle.write("| Date | Screen | Decision | Reason | Source |\n|---|---|---|---|---|\n")
        with self.assertRaises(screen_status.LedgerError):
            screen_status.transition(self.ledger, "sample.x", "approved", "antoine", 1, decisions)
        screen_status.record_critic(self.ledger, "sample.x", self.report("c1", []))
        with self.assertRaises(screen_status.LedgerError):
            screen_status.transition(self.ledger, "sample.x", "approved", "antoine", 1, decisions)
        with open(decisions, "a") as handle:
            handle.write("| 2026-10-14 | sample.x | Picked B | Calmer | gallery |\n")
        with self.assertRaises(screen_status.LedgerError):
            screen_status.transition(self.ledger, "sample.x", "approved", "antoine", 0, decisions)
        self.assertEqual(screen_status.transition(self.ledger, "sample.x", "approved", "antoine", 1, decisions), "approved")

    def test_the_ledger_round_trips_with_its_header(self):
        screen_status.record_critic(self.ledger, "sample.x", self.report("c1", []))
        screen_status.dump(self.ledger, self.ledger_path)
        with open(self.ledger_path) as handle:
            self.assertTrue(handle.read().startswith("# Screen status ledger"))
        self.assertEqual(screen_status.load(self.ledger_path)["screens"]["sample.x"]["status"], "verified")

    def test_the_committed_ledger_lists_every_inventory_screen_and_is_valid(self):
        os.chdir(REPO)
        proc = subprocess.run([sys.executable, os.path.join(os.path.dirname(HERE), "screen_status.py"), "check"], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        ledger = screen_status.load()
        inventory = screen_status.load_inventory()
        self.assertEqual(sorted(ledger["screens"]), sorted(inventory))

    def test_inventory_json_lists_ids_and_states(self):
        os.chdir(REPO)
        proc = subprocess.run([sys.executable, os.path.join(os.path.dirname(HERE), "screen_status.py"), "inventory-json"], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        screens = {s["id"]: s for s in json.loads(proc.stdout)["screens"]}
        self.assertEqual(screens["home.deckList"]["states"], ["empty", "typical", "overflow", "error", "loading"])


if __name__ == "__main__":
    unittest.main()
