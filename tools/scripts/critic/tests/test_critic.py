"""Tests for the critic packet and report checks (U10, R1983-R1986)."""

import json
import os
import shutil
import sys
import tempfile
import unittest

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
sys.path.insert(0, os.path.dirname(HERE))

import critic  # noqa: E402
import screen_status  # noqa: E402

SCREEN = "sample.deckOverview"


class Args:
    def __init__(self, **kwargs):
        self.screen = None
        self.drive = None
        self.checks = None
        self.a11y = None
        self.host_root = ""
        self.run = "critic-test"
        self.__dict__.update(kwargs)


class CriticTest(unittest.TestCase):
    def setUp(self):
        os.chdir(REPO)
        self.dir = tempfile.mkdtemp()
        self.run_dir = os.path.join(self.dir, "snapshots-run")
        os.makedirs(self.run_dir)
        self.results = []
        self.image("overflow", "dark-ax5-iphoneSmall", "recorded")
        self.image("typical", "light-standard-iphoneLarge", "pass")
        self.image("typical", "dark-standard-iphoneLarge", "deferred")
        self.results.append({"name": "designsystem.toast/typical/x", "subject": "designsystem.toast", "state": "typical",
                             "cell": "x", "status": "pass", "baseline": self.png("toast.png")})
        self.results.append({"name": "cardface.sample/typical/x", "subject": "cardface.sample", "state": "typical",
                             "cell": "x", "status": "pass", "baseline": self.png("card.png")})
        with open(os.path.join(self.run_dir, "results.jsonl"), "w") as handle:
            for entry in self.results:
                handle.write(json.dumps(entry) + "\n")
        self.registry({SCREEN: ["iphone"]})
        with open(os.path.join(self.run_dir, "result.json"), "w") as handle:
            json.dump({"job": "snapshots", "status": "pass", "commit": {"sha": "abc"}, "artifacts": [],
                       "details": {"input": "src1"}}, handle)
        self.packet = os.path.join(self.dir, "packet")

    def registry(self, platforms, cells=None):
        """The run's registry.json: where the snapshot matrix registers each
        screen and the cells it renders there (by default those the run has
        images of)."""
        def rendered(screen, platform):
            return sorted({"%s/%s" % (e["state"], e["cell"]) for e in self.results
                           if e["subject"] == screen and e.get("platform") == platform and e["status"] != "deferred"})
        with open(os.path.join(self.run_dir, "registry.json"), "w") as handle:
            json.dump({"screens": {s: {"platforms": p, "cells": (cells or {}).get(s) or {q: rendered(s, q) for q in p}}
                                   for s, p in platforms.items()}}, handle)

    def a11y_run(self, name, platform, status="pass", screen=SCREEN, source="src1"):
        run = os.path.join(self.dir, name)
        os.makedirs(run)
        with open(os.path.join(run, "result.json"), "w") as handle:
            json.dump({"job": "a11y", "run_id": name, "status": status, "commit": {"sha": "abc"},
                       "details": {"platform": platform, "input": source}}, handle)
        with open(os.path.join(run, "screens.json"), "w") as handle:
            json.dump([{"screen": screen, "platform": platform, "status": status}], handle)
        return run

    def png(self, name, size=(780, 1688)):
        path = os.path.join(self.dir, "images", name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        Image.new("RGB", size, (255, 255, 255)).save(path)
        return path

    def image(self, state, cell, status):
        name = "%s/%s/%s" % (SCREEN, state, cell)
        entry = {"name": name, "subject": SCREEN, "state": state, "cell": cell, "status": status, "platform": "iphone",
                 "axes": {"appearance": cell.split("-")[0]}, "layout": "screen"}
        if status == "pass":
            entry["baseline"] = self.png(name.replace("/", "_") + "-baseline.png")
        elif status != "deferred":
            entry["actual"] = self.png(name.replace("/", "_") + "-actual.png")
        self.results.append(entry)

    def build(self, **kwargs):
        return critic.build_packet(Args(snapshots=self.run_dir, out=self.packet, **kwargs))

    def output(self, body, structured=True):
        path = os.path.join(self.dir, "output.json")
        outer = {"type": "result", "is_error": False}
        if structured:
            outer["structured_output"] = body
        else:
            outer["result"] = "Here is the report:\n```json\n%s\n```" % json.dumps(body)
        with open(path, "w") as handle:
            json.dump(outer, handle)
        return path

    def truncation(self, **changes):
        issue = {"screen": SCREEN, "state": "overflow", "variant": "dark-ax5-iphoneSmall",
                 "image": "screens/%s/overflow/dark-ax5-iphoneSmall.png" % SCREEN, "rule": "text-truncation",
                 "severity": "high", "region": {"x": 32, "y": 120, "width": 700, "height": 90},
                 "summary": "The deck name is cut off after one line at the largest text size."}
        issue.update(changes)
        return issue

    def report(self, issues, **changes):
        body = {"rubric_version": critic.rubric()["version"], "screens": [SCREEN], "issues": issues}
        body.update(changes)
        return body

    def test_the_packet_holds_the_screens_images_rubric_schema_and_calibration(self):
        manifest = self.build()
        self.assertEqual([s["id"] for s in manifest["screens"]], [SCREEN])
        paths = sorted(i["path"] for i in manifest["images"])
        self.assertEqual(paths, ["screens/%s/overflow/dark-ax5-iphoneSmall.png" % SCREEN, "screens/%s/typical/light-standard-iphoneLarge.png" % SCREEN])
        by_variant = {i["variant"]: i for i in manifest["images"]}
        self.assertEqual(by_variant["dark-ax5-iphoneSmall"]["source"], "render")
        self.assertEqual(by_variant["light-standard-iphoneLarge"]["source"], "baseline")
        self.assertEqual((by_variant["dark-ax5-iphoneSmall"]["width_px"], by_variant["dark-ax5-iphoneSmall"]["height_px"]), (780, 1688))
        for name in ("packet.json", "rubric.md", "brief.md", "schema.json", "role.md", "calibration/decisions.md", "references/index.md"):
            self.assertTrue(os.path.exists(os.path.join(self.packet, name)), name)
        self.assertFalse(any("designsystem" in p or "cardface" in p for p in paths))

    def test_a_screen_without_images_is_an_error(self):
        with self.assertRaises(critic.PacketError):
            self.build(screen=["home.deckList"])

    def test_a_valid_report_keeps_a_high_truncation_issue(self):
        self.build()
        report, errors = critic.validate(self.packet, self.output(self.report([self.truncation()])))
        self.assertEqual(errors, [])
        self.assertEqual(len(report["issues"]), 1)
        self.assertEqual(report["issues"][0]["rule"], "text-truncation")
        self.assertEqual(report["issues"][0]["severity"], "high")

    def test_the_answer_may_come_in_the_session_message_list(self):
        self.build()
        path = os.path.join(self.dir, "messages.json")
        with open(path, "w") as handle:
            json.dump([{"type": "system", "subtype": "init"}, {"type": "assistant"},
                       {"type": "result", "is_error": False, "structured_output": self.report([self.truncation()])}], handle)
        report, errors = critic.validate(self.packet, path)
        self.assertEqual(errors, [])
        self.assertEqual(len(report["issues"]), 1)

    def test_the_answer_may_come_as_fenced_text(self):
        self.build()
        report, errors = critic.validate(self.packet, self.output(self.report([]), structured=False))
        self.assertEqual(errors, [])
        self.assertEqual(report["issues"], [])

    def test_output_that_breaks_the_schema_is_rejected(self):
        self.build()
        broken = self.truncation()
        del broken["region"]
        report, errors = critic.validate(self.packet, self.output(self.report([broken])))
        self.assertIsNone(report)
        self.assertTrue(any("region" in e for e in errors), errors)
        report, errors = critic.validate(self.packet, self.output(self.report([self.truncation(severity="blocker")])))
        self.assertIsNone(report)

    def test_rules_images_variants_and_regions_must_exist(self):
        self.build()
        cases = [
            (self.truncation(rule="text-too-long"), "not in rubric"),
            (self.truncation(image="screens/%s/overflow/none.png" % SCREEN), "not in the packet"),
            (self.truncation(variant="light-standard-iphoneLarge"), "shows"),
            (self.truncation(region={"x": 700, "y": 0, "width": 200, "height": 10}), "outside"),
        ]
        for issue, message in cases:
            report, errors = critic.validate(self.packet, self.output(self.report([issue])))
            self.assertIsNone(report)
            self.assertTrue(any(message in e for e in errors), (message, errors))
        report, errors = critic.validate(self.packet, self.output(self.report([], rubric_version=99)))
        self.assertIsNone(report)
        report, errors = critic.validate(self.packet, self.output(self.report([], screens=[])))
        self.assertTrue(any("does not grade" in e for e in errors), errors)

    def test_an_error_result_or_non_json_answer_is_rejected(self):
        self.build()
        path = os.path.join(self.dir, "bad.json")
        with open(path, "w") as handle:
            json.dump({"type": "result", "is_error": True, "result": "rate limited"}, handle)
        report, errors = critic.validate(self.packet, path)
        self.assertIsNone(report)
        with open(path, "w") as handle:
            handle.write("not json")
        self.assertIsNone(critic.validate(self.packet, path)[0])

    def test_a_passing_deterministic_check_drops_the_issue_and_an_uncovered_cell_keeps_it(self):
        checks = os.path.join(self.dir, "checks.json")
        with open(checks, "w") as handle:
            json.dump({"checks": [
                {"screen": SCREEN, "state": "*", "variant": "*", "rule": "color-contrast", "status": "pass"},
                {"screen": SCREEN, "state": "typical", "variant": "light-standard-iphoneLarge", "rule": "text-truncation", "status": "pass"},
            ]}, handle)
        self.build(checks=checks)
        contrast = self.truncation(rule="color-contrast", summary="Counts look faint")
        report, errors = critic.validate(self.packet, self.output(self.report([contrast, self.truncation()])))
        self.assertEqual(errors, [])
        self.assertEqual([i["rule"] for i in report["issues"]], ["text-truncation"])
        self.assertEqual([i["rule"] for i in report["dropped"]], ["color-contrast"])

    def test_a_passing_check_on_one_state_never_drops_an_issue_on_another(self):
        # The same rule and variant pass in the typical state and fail in the
        # overflow state: only the typical state's issue is dropped.
        self.image("typical", "dark-ax5-iphoneSmall", "pass")
        with open(os.path.join(self.run_dir, "results.jsonl"), "w") as handle:
            for entry in self.results:
                handle.write(json.dumps(entry) + "\n")
        checks = os.path.join(self.dir, "checks.json")
        with open(checks, "w") as handle:
            json.dump({"checks": [
                {"screen": SCREEN, "state": "typical", "variant": "dark-ax5-iphoneSmall", "rule": "text-truncation", "status": "pass"},
                {"screen": SCREEN, "state": "overflow", "variant": "dark-ax5-iphoneSmall", "rule": "text-truncation", "status": "fail"},
            ]}, handle)
        self.build(checks=checks)
        typical = self.truncation(state="typical", image="screens/%s/typical/dark-ax5-iphoneSmall.png" % SCREEN)
        report, errors = critic.validate(self.packet, self.output(self.report([typical, self.truncation()])))
        self.assertEqual(errors, [])
        self.assertEqual([i["state"] for i in report["issues"]], ["overflow"])
        self.assertEqual([i["state"] for i in report["dropped"]], ["typical"])
        # A check that names no state covers none: the issue stands.
        with open(checks, "w") as handle:
            json.dump({"checks": [{"screen": SCREEN, "variant": "*", "rule": "text-truncation", "status": "pass"}]}, handle)
        self.build(checks=checks)
        report, errors = critic.validate(self.packet, self.output(self.report([self.truncation()])))
        self.assertEqual(len(report["issues"]), 1)

    def test_a_run_without_every_platform_the_screen_is_registered_on_is_refused(self):
        self.registry({SCREEN: ["iphone", "ipad", "mac"]})
        with self.assertRaises(critic.PacketError) as caught:
            self.build()
        self.assertIn("ipad, mac", str(caught.exception))
        self.assertIn("--platform iphone,ipad,mac", str(caught.exception))
        # A screen the registry does not know is refused too.
        self.registry({})
        with self.assertRaises(critic.PacketError):
            self.build()
        os.remove(os.path.join(self.run_dir, "registry.json"))
        with self.assertRaises(critic.PacketError):
            self.build()

    def test_a_run_that_leaves_out_a_registered_cell_is_refused(self):
        # A filtered run (typical and overflow only) of a screen registered
        # in the error state too: a clean answer would clear issues in cells
        # nobody looked at, so the critic does not grade it.
        rendered = ["overflow/dark-ax5-iphoneSmall", "typical/light-standard-iphoneLarge"]
        self.registry({SCREEN: ["iphone"]}, {SCREEN: {"iphone": rendered + ["error/light-standard-iphoneLarge"]}})
        with self.assertRaises(critic.PacketError) as caught:
            self.build()
        self.assertIn("error/light-standard-iphoneLarge", str(caught.exception))
        self.assertIn("filter", str(caught.exception))
        # A registry from before the cells were listed cannot show coverage.
        with open(os.path.join(self.run_dir, "registry.json"), "w") as handle:
            json.dump({"screens": {SCREEN: {"platforms": ["iphone"]}}}, handle)
        with self.assertRaises(critic.PacketError):
            self.build()
        self.registry({SCREEN: ["iphone"]}, {SCREEN: {"iphone": rendered}})
        self.assertEqual(len(self.build()["images"]), 2)

    def test_an_a11y_run_of_other_app_sources_is_refused_before_the_session(self):
        with self.assertRaises(critic.PacketError) as caught:
            self.build(a11y=[self.a11y_run("a11y-old", "iphone", source="src0")])
        self.assertIn("src0", str(caught.exception))
        self.assertIn("src1", str(caught.exception))
        self.assertEqual(len(self.build(a11y=[self.a11y_run("a11y-new", "iphone")])["images"]), 2)

    def test_the_packet_records_each_platforms_snapshot_evidence_and_issues_name_their_platform(self):
        manifest = self.build()
        [screen] = manifest["screens"]
        self.assertEqual(screen["platforms"], ["iphone"])
        iphone = screen["evidence"]["iphone"]["snapshots"]
        self.assertEqual((iphone["run"], iphone["commit"], iphone["status"], iphone["input"]), ("snapshots-run", "abc", "pass", "src1"))
        self.assertRegex(iphone["digest"], "^[0-9a-f]{64}$")
        report, _ = critic.validate(self.packet, self.output(self.report([self.truncation()])))
        self.assertEqual(report["issues"][0]["platform"], "iphone")

    def test_a_failed_snapshot_of_the_screen_fails_its_platforms_evidence(self):
        self.image("error", "light-standard-iphoneLarge", "fail")
        with open(os.path.join(self.run_dir, "results.jsonl"), "w") as handle:
            for entry in self.results:
                handle.write(json.dumps(entry) + "\n")
        self.assertEqual(self.build()["screens"][0]["evidence"]["iphone"]["snapshots"]["status"], "fail")

    def test_the_rubric_lists_rules_owners_and_its_version(self):
        rules = critic.rubric()
        self.assertGreaterEqual(rules["version"], 1)
        self.assertTrue(rules["rules"]["text-truncation"]["deterministic"])
        self.assertFalse(rules["rules"]["type-hierarchy"]["deterministic"])

    def test_apply_records_the_report_in_a_ledger(self):
        self.build()
        report, _ = critic.validate(self.packet, self.output(self.report([self.truncation()])))
        report_path = os.path.join(self.dir, "report.json")
        with open(report_path, "w") as handle:
            json.dump(report, handle)
        ledger = os.path.join(self.dir, "ledger.yaml")
        a11y = self.a11y_run("a11y-1", "iphone")
        self.assertEqual(critic.main(["apply", "--report", report_path, "--packet", self.packet, "--a11y", a11y,
                                      "--ledger", ledger, "--session", "s-1"]), 0)
        entry = screen_status.load(ledger)["screens"][SCREEN]
        self.assertEqual(entry["status"], "built")
        self.assertEqual(entry["platforms"], ["iphone"])
        self.assertEqual(entry["evidence"]["iphone"]["critic"]["session"], "s-1")
        self.assertEqual(entry["evidence"]["iphone"]["critic"]["status"], "fail")
        self.assertEqual(entry["evidence"]["iphone"]["a11y"]["run"], "a11y-1")
        self.assertEqual(entry["evidence"]["iphone"]["snapshots"]["status"], "pass")
        # A clean re-check verifies it; an a11y run of another platform is no
        # evidence for this one.
        clean, _ = critic.validate(self.packet, self.output(self.report([])))
        with open(report_path, "w") as handle:
            json.dump(clean, handle)
        self.assertEqual(critic.main(["apply", "--report", report_path, "--packet", self.packet, "--ledger", ledger]), 0)
        self.assertEqual(screen_status.load(ledger)["screens"][SCREEN]["status"], "verified")
        mac = self.a11y_run("a11y-mac", "mac", status="fail")
        self.assertEqual(critic.main(["apply", "--report", report_path, "--packet", self.packet, "--a11y", mac, "--ledger", ledger]), 0)
        self.assertEqual(screen_status.load(ledger)["screens"][SCREEN]["evidence"]["iphone"]["a11y"]["run"], "a11y-1")
        # A failed a11y run demotes it.
        failed = self.a11y_run("a11y-2", "iphone", status="fail")
        self.assertEqual(critic.main(["apply", "--report", report_path, "--packet", self.packet, "--a11y", failed, "--ledger", ledger]), 0)
        self.assertEqual(screen_status.load(ledger)["screens"][SCREEN]["status"], "built")
        # An a11y run of other sources is refused and records nothing.
        old = self.a11y_run("a11y-3", "iphone", source="src0")
        self.assertEqual(critic.main(["apply", "--report", report_path, "--packet", self.packet, "--a11y", old, "--ledger", ledger]), 1)
        self.assertEqual(screen_status.load(ledger)["screens"][SCREEN]["evidence"]["iphone"]["a11y"]["run"], "a11y-2")
        # The canonical ledger refuses a sample screen.
        self.assertEqual(critic.main(["apply", "--report", report_path, "--packet", self.packet]), 1)

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
