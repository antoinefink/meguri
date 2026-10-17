"""Tests for tools/scripts/gates/perf_gate.py (U241): the budget catalog's
checks, the evaluation of perf results against budgets and baselines, the
R1866 refusals, rebaselining and the device-run report a milestone reads.
Expected values come from the plan's U241 test scenarios and requirement
text (R1803, R1807, R1813, R1825, R1861, R1863, R1864, R1866), never from
the gate's own output.

Run: python3 -m unittest discover -s tools/scripts/gates/tests
"""

import copy
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
GATES = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(GATES)))
SCRIPT = os.path.join(GATES, "perf_gate.py")
sys.path.insert(0, GATES)

import perf_gate  # noqa: E402

SIM = {"kind": "simulator", "id": "vm/meguri-ui/iPhone 17 Pro/24A434", "model": "iPhone 17 Pro",
       "os": "iOS 27.0", "os_build": "24A434", "arch": "arm64"}
IPAD_SIM = {"kind": "simulator", "id": "vm/meguri-ui/iPad Pro 13-inch (M5)/24A434", "model": "iPad Pro 13-inch (M5)",
            "os": "iOS 27.0", "os_build": "24A434", "arch": "arm64"}
IPHONE = {"kind": "device", "id": "iPhone18,1/24A434", "model": "iPhone 17 Pro", "os": "iOS 27.0",
          "os_build": "24A434", "arch": "arm64"}
MAC = {"kind": "mac", "id": "Apple M3 Max/macOS 27.0", "cpu": "Apple M3 Max", "os": "macOS 27.0", "arch": "arm64",
       "memory_measure": "phys_footprint"}
MAC_VM = {"kind": "mac", "id": "vm/meguri-ui", "model": "test VM meguri-ui"}
DOCKER = {"kind": "docker", "id": "docker/linux-aarch64", "os": "linux", "arch": "aarch64", "memory_measure": "peak_rss"}
COMMIT = "a" * 40


def case(cid, suite, name, limit, where, metric="duration", unit="ms", **extra):
    out = {"id": cid, "suite": suite, "name": name, "metric": metric, "unit": unit, "limit": limit, "where": where}
    out.update(extra)
    return out


def raw_catalog():
    """A small catalog in budgets.yaml's shape: the budgets the scenarios
    name, with the plan's numbers."""
    return {
        "version": 1,
        "rules": {"ui_regression_percent": 10, "icount_regression_percent": 5,
                  "simulator_absolute_units": ["frames", "ms/s", "MB", "count"], "time_units": ["ms", "s"]},
        "defaults": {"fixture": "150k", "percentile": "p95", "multiplier_500k": 2, "rule_1m": "completes"},
        "groups": {"launch": "launch", "review": "review", "review-loop": "review loop", "search": "search",
                   "editor": "editor", "open": "open", "memory": "memory", "smoke": "smoke"},
        "budgets": [
            {"id": "R1807", "title": "answer to next card", "group": "review-loop", "cases": [
                case("frames-p95", "ReviewLoopPerfTests", "review.answerToNextVisible", 0,
                     ["simulator", "device"], metric="frames", unit="frames", relative_to="probe.nativeSwap",
                     fixture="review-corpus", simulator="regression"),
                case("frames-p99", "ReviewLoopPerfTests", "review.answerToNextVisible", 1,
                     ["simulator", "device"], metric="frames", unit="frames", relative_to="probe.nativeSwap",
                     fixture="review-corpus", simulator="regression", percentile="p99")]},
            {"id": "R1813", "title": "start review", "group": "launch", "cases": [
                case("warm", "ReviewLoopPerfTests", "review.startToFirstCard", 150, ["simulator", "device"])]},
            {"id": "R1816", "title": "tint", "group": "review-loop", "devices": ["iphone", "ipad"], "cases": [
                case("same-frame", "ReviewLoopPerfTests", "review.touchDownToTint", 0, ["simulator", "device"],
                     metric="frames", unit="frames")]},
            {"id": "R1823", "title": "open", "group": "open", "cases": [
                case("core", "open", "collection.open", 150, ["core-mac"]),
                case("device", "LaunchHomePerfTests", "collection.open", 150, ["device"], pending="U256")]},
            {"id": "R1825", "title": "search first page", "group": "search", "cases": [
                case("first-page", "BrowserSearchPerfTests", "search.firstPage", 100, ["simulator", "device"])]},
            {"id": "R1833", "title": "editor", "group": "editor", "cases": [
                case("open", "EditorPerfTests", "editor.openToEditable", 100, ["simulator", "device"],
                     pending="U244")]},
            {"id": "R1851", "title": "memory", "group": "memory", "devices": ["iphone"], "cases": [
                case("normal-use", "MemoryPerfTests", "xctest.memory", 150, ["simulator", "device"],
                     metric="memory-peak-physical", unit="MB", percentile="max", pending="U245")]},
        ],
        "probes": [
            {"id": "probe-synthetic-delay", "title": "the smoke probe", "group": "smoke", "cases": [
                case("delay", "SmokePerfTests", "probe.syntheticDelay", None, ["simulator", "device"])]},
        ],
    }


def catalog(raw=None):
    cat, errors = perf_gate.normalize(raw or raw_catalog())
    assert not errors, errors
    return cat


def row(name, value, metric="duration", unit="ms", percentile="p95", test=None, fixture="150k", samples=5):
    out = {"name": name, "metric": metric, "unit": unit, "value": value, "samples": samples}
    if percentile:
        out["percentile"] = percentile
    if test:
        out["test"] = test
    if fixture:
        out["fixture"] = fixture
    return out


def ui(suite, rows, runner=SIM, configuration="Profile", attachments=0):
    out = {"schema_version": 1, "suite": suite, "kind": "ui", "configuration": configuration, "runner": runner,
           "results": rows}
    if attachments is not None:
        out["attachments"] = attachments
    return out


def bench(suite, rows, runner=MAC, kind="wall-clock"):
    return {"schema_version": 1, "suite": suite, "kind": kind, "configuration": "bench", "runner": runner,
            "results": rows}


def inputs(*docs):
    return [{"path": "/results/%d.json" % i, "job": "perf-ui", "run_id": "run-%d" % i, "result": d}
            for i, d in enumerate(docs)]


def review_rows(start=120.0, tint=0, frames=(3, 4), swap=(3, 3), start_fixture="150k", omit=()):
    """A full ReviewLoopPerfTests run as raw_catalog() gates it: R1807's
    frames against the native swap, R1813's start and R1816's tint."""
    rows = [row("review.startToFirstCard", start, fixture=start_fixture),
            row("review.answerToNextVisible", frames[0], metric="frames", unit="frames", fixture="review-corpus"),
            row("review.answerToNextVisible", frames[1], metric="frames", unit="frames", percentile="p99",
                fixture="review-corpus"),
            row("probe.nativeSwap", swap[0], metric="frames", unit="frames"),
            row("probe.nativeSwap", swap[1], metric="frames", unit="frames", percentile="p99"),
            row("review.touchDownToTint", tint, metric="frames", unit="frames")]
    return [r for r in rows if r["name"] not in omit]


def baseline(runner, suite, rows, kind="ui", reason="first baseline"):
    return {"schema_version": 1, "runner": {"id": runner["id"], "kind": runner["kind"]}, "suite": suite,
            "kind": kind, "measured_commit": COMMIT, "reason": reason, "rows": rows}


def baselines(*docs):
    return {(d["runner"]["id"], d["suite"]): d for d in docs}


def failure_text(ev):
    return " | ".join(f["message"] for f in ev["failures"])


class EvaluateScenarios(unittest.TestCase):
    """The plan's U241 test scenarios."""

    def test_a_device_p95_over_the_review_start_budget_fails_and_names_r1813(self):
        res = ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 160, test="ReviewLoopPerfTests/testStart()")],
                 runner=IPHONE)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1813", failure_text(ev))
        self.assertIn("160", failure_text(ev))
        self.assertIn("150", failure_text(ev))
        self.assertEqual(ev["budgets"]["R1813"], "fail")

    def test_the_same_value_within_the_budget_passes(self):
        res = ui("ReviewLoopPerfTests", review_rows(start=149), runner=IPHONE)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "pass", failure_text(ev))

    def search(self, value):
        return ui("BrowserSearchPerfTests", [row("search.firstPage", value, test="BrowserSearchPerfTests/testPlain()")])

    def search_baseline(self, value=40.0):
        return baselines(baseline(SIM, "BrowserSearchPerfTests", [
            {"name": "search.firstPage", "metric": "duration", "unit": "ms", "percentile": "p95",
             "test": "BrowserSearchPerfTests/testPlain()", "fixture": "150k", "value": value}]))

    def test_a_simulator_search_8_percent_slower_than_its_baseline_passes(self):
        ev = perf_gate.evaluate(catalog(), inputs(self.search(43.2)), self.search_baseline())
        self.assertEqual(ev["status"], "pass", failure_text(ev))
        [e] = [e for e in ev["evaluations"] if e["budget"] == "R1825"]
        self.assertAlmostEqual(e["regression_percent"], 8.0)

    def test_a_simulator_search_11_percent_slower_than_its_baseline_fails(self):
        ev = perf_gate.evaluate(catalog(), inputs(self.search(44.4)), self.search_baseline())
        self.assertEqual(ev["status"], "fail")
        self.assertIn("regression +11% > 10%", failure_text(ev))
        self.assertIn("R1825", failure_text(ev))

    def test_simulator_ms_budgets_are_not_absolute(self):
        # 120 ms is over R1825's 100 ms, but simulator timing is not device
        # timing: only the regression rule applies to ms on simulators.
        ev = perf_gate.evaluate(catalog(), inputs(self.search(120)), self.search_baseline(115))
        self.assertEqual(ev["status"], "pass", failure_text(ev))

    def test_a_simulator_ms_case_without_a_baseline_is_unchecked_and_warns(self):
        ev = perf_gate.evaluate(catalog(), inputs(self.search(43.0)), {})
        self.assertEqual(ev["status"], "warn")
        [e] = [e for e in ev["evaluations"] if e["budget"] == "R1825"]
        self.assertEqual(e["status"], "unchecked")
        self.assertIn("--rebaseline", " ".join(ev["warnings"]))

    def icount(self, name, value, base):
        res = bench("icount_answer", [row(name, value, metric="instructions", unit="count", percentile=None,
                                          samples=1, fixture="10k")], runner=DOCKER, kind="instruction-count")
        bl = baselines(baseline(DOCKER, "icount_answer", [
            {"name": name, "metric": "instructions", "unit": "count", "fixture": "10k", "value": base}],
            kind="instruction-count"))
        return perf_gate.evaluate(catalog(), inputs(res), bl)

    def test_instruction_counts_fail_above_5_percent(self):
        ev = self.icount("collection.open", 1060000, 1000000)
        self.assertEqual(ev["status"], "fail")
        self.assertIn("+6% > 5%", failure_text(ev))
        self.assertEqual(self.icount("collection.open", 1040000, 1000000)["status"], "pass")

    def test_instruction_counts_at_5_2_percent_fail_and_at_4_9_pass(self):
        ev = self.icount("bench.answer.apply", 1052000, 1000000)
        self.assertEqual(ev["status"], "fail")
        self.assertIn("bench.answer.apply", failure_text(ev))
        self.assertIn("+5.2% > 5%", failure_text(ev))
        self.assertEqual(self.icount("bench.answer.apply", 1049000, 1000000)["status"], "pass")

    def test_an_instruction_count_without_a_baseline_warns(self):
        res = bench("icount_new", [row("x.y", 5, metric="instructions", unit="count", percentile=None, samples=1)],
                    runner=DOCKER, kind="instruction-count")
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "warn")

    def open_bench(self, value, fixture):
        return bench("open", [row("collection.open", value, fixture=fixture)])

    def test_a_500k_result_at_1_9_times_the_150k_budget_passes_and_at_2_1_fails(self):
        ev = perf_gate.evaluate(catalog(), inputs(self.open_bench(150 * 1.9, "500k")), {})
        self.assertEqual(ev["status"], "pass", failure_text(ev))
        ev = perf_gate.evaluate(catalog(), inputs(self.open_bench(150 * 2.1, "500k")), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1823", failure_text(ev))
        self.assertIn("300", failure_text(ev))

    def test_the_150k_budget_holds_at_every_size_up_to_150k(self):
        self.assertEqual(perf_gate.evaluate(catalog(), inputs(self.open_bench(160, "10k")), {})["status"], "fail")
        self.assertEqual(perf_gate.evaluate(catalog(), inputs(self.open_bench(149, "150k")), {})["status"], "pass")

    def test_at_1m_cards_only_completion_counts(self):
        ev = perf_gate.evaluate(catalog(), inputs(self.open_bench(5000, "1m")), {})
        self.assertEqual(ev["status"], "pass", failure_text(ev))
        [e] = ev["evaluations"]
        self.assertEqual(e["rule"], "completes")

    def test_a_debug_bundle_is_rejected_before_evaluating(self):
        res = ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 10)], runner=IPHONE, configuration="Debug")
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertEqual(ev["evaluations"], [])
        self.assertIn("Debug", failure_text(ev))
        self.assertIn("R1866", failure_text(ev))

    def test_a_release_bundle_is_rejected_too(self):
        res = ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 10)], runner=IPHONE, configuration="Release")
        self.assertEqual(perf_gate.evaluate(catalog(), inputs(res), {})["evaluations"], [])

    def test_a_result_with_attachments_or_screenshots_is_rejected(self):
        res = ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 10)], runner=IPHONE, attachments=2)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual((ev["status"], ev["evaluations"]), ("fail", []))
        self.assertIn("attachments", failure_text(ev))
        res = ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 10)], runner=IPHONE, attachments=None)
        self.assertEqual(perf_gate.evaluate(catalog(), inputs(res), {})["status"], "fail")


class EvaluateRules(unittest.TestCase):
    def test_frame_relative_budgets_compare_with_the_native_swap_on_the_same_device(self):
        rows = review_rows(frames=(3, 4), swap=(3, 3))
        ev = perf_gate.evaluate(catalog(), inputs(ui("ReviewLoopPerfTests", rows, runner=IPHONE)), {})
        self.assertEqual(ev["status"], "pass", failure_text(ev))
        rows = review_rows(frames=(3, 5), swap=(3, 3))  # p99 two frames after the baseline swap
        ev = perf_gate.evaluate(catalog(), inputs(ui("ReviewLoopPerfTests", rows, runner=IPHONE)), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1807", failure_text(ev))

    def test_a_frame_relative_case_without_its_reference_fails(self):
        rows = [row("review.answerToNextVisible", 3, metric="frames", unit="frames", fixture="review-corpus")]
        ev = perf_gate.evaluate(catalog(), inputs(ui("ReviewLoopPerfTests", rows, runner=IPHONE)), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("probe.nativeSwap", failure_text(ev))

    def test_simulator_frame_budgets_are_absolute(self):
        # R1816 on the iPhone simulator: a frame count, so absolute per change.
        res = ui("ReviewLoopPerfTests", [row("review.touchDownToTint", 1, metric="frames", unit="frames")])
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1816", failure_text(ev))

    def test_the_ktd19_exception_keeps_r1807_on_simulators_to_the_regression_rule(self):
        # Three and four frames after the swap: absolute, both would fail.
        rows = review_rows(frames=(6, 7), swap=(3, 3))
        bl = baselines(baseline(SIM, "ReviewLoopPerfTests", [
            {"name": "review.answerToNextVisible", "metric": "frames", "unit": "frames", "percentile": "p95",
             "fixture": "review-corpus", "value": 6},
            {"name": "review.answerToNextVisible", "metric": "frames", "unit": "frames", "percentile": "p99",
             "fixture": "review-corpus", "value": 7},
            {"name": "review.startToFirstCard", "metric": "duration", "unit": "ms", "percentile": "p95",
             "fixture": "150k", "value": 120.0}]))
        ev = perf_gate.evaluate(catalog(), inputs(ui("ReviewLoopPerfTests", rows)), bl)
        self.assertEqual(ev["status"], "pass", failure_text(ev))

    def test_a_device_restricted_budget_ignores_other_devices(self):
        # R1816 has no Mac counterpart and R1851 is iPhone only: an iPad
        # simulator's memory is not judged against the iPhone budget.
        res = ui("MemoryPerfTests", [row("xctest.memory", 400, metric="memory-peak-physical", unit="MB",
                                         percentile="max")], runner=IPAD_SIM)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["evaluations"], [])
        res["runner"] = SIM
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "fail")  # memory is absolute on simulators
        self.assertIn("R1851", failure_text(ev))

    def test_a_device_restriction_does_not_apply_to_core_benches(self):
        raw = raw_catalog()
        memory = [b for b in raw["budgets"] if b["id"] == "R1851"][0]
        memory["cases"].append(case("core", "optimize", "fsrs.optimize", 300, ["core-mac"], metric="memory",
                                    unit="MB", percentile="max"))
        res = bench("optimize", [row("fsrs.optimize", 320, metric="memory", unit="MB", percentile="max")])
        ev = perf_gate.evaluate(catalog(raw), inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1851", failure_text(ev))

    def test_the_mac_in_the_test_vm_follows_the_simulator_rules(self):
        res = ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 400)], runner=MAC_VM)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        [e] = ev["evaluations"]
        self.assertEqual((e["placement"], e["status"]), ("simulator", "unchecked"))

    def test_the_mac_host_is_a_device(self):
        res = ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 400)], runner=MAC)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        [e] = ev["evaluations"]
        self.assertEqual((e["placement"], e["family"], e["status"]), ("device", "mac", "fail"))

    def test_a_suite_or_placement_the_case_does_not_name_does_not_match(self):
        res = ui("OtherPerfTests", [row("review.startToFirstCard", 400)], runner=IPHONE)
        self.assertEqual(perf_gate.evaluate(catalog(), inputs(res), {})["evaluations"], [])
        res = bench("ReviewLoopPerfTests", [row("review.startToFirstCard", 400)])  # core-mac
        self.assertEqual(perf_gate.evaluate(catalog(), inputs(res), {})["evaluations"], [])

    def test_wall_clock_benches_are_tracked_but_never_regress(self):
        res = bench("open", [row("collection.open", v, percentile=p) for p, v in
                             (("p50", 50), ("p95", 100), ("p99", 140))])
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "pass", failure_text(ev))
        tracked = {t["percentile"]: t["value"] for t in ev["tracked"] if t["name"] == "collection.open"}
        self.assertEqual(tracked, {"p50": 50, "p95": 100, "p99": 140})

    def test_nothing_to_evaluate_warns(self):
        ev = perf_gate.evaluate(catalog(), [], {})
        self.assertEqual(ev["status"], "warn")

    def test_rows_at_other_sizes_are_judged_but_only_the_catalog_fixture_proves_a_budget(self):
        # R1803: at 1M only completion counts and at 500k the 2x rule; both
        # are judged, neither proves the budget stated at 150k.
        for fixture, value in (("1m", 5000), ("500k", 150 * 1.9), ("10k", 100)):
            ev = perf_gate.evaluate(catalog(), inputs(bench("open", [row("collection.open", value, fixture=fixture)])),
                                    {})
            self.assertEqual(ev["status"], "pass", failure_text(ev))
            self.assertEqual(ev["budgets"]["R1823"], "missing", fixture)
            self.assertEqual(ev["groups"]["open"]["status"], "missing", fixture)
        res = bench("open", [row("collection.open", 140, fixture="150k"), row("collection.open", 5000, fixture="1m")])
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual((ev["status"], ev["budgets"]["R1823"]), ("pass", "pass"), failure_text(ev))
        # A breach at another size still fails the budget.
        res = bench("open", [row("collection.open", 140, fixture="150k"), row("collection.open", 310, fixture="500k")])
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual((ev["status"], ev["budgets"]["R1823"]), ("fail", "fail"))

    def test_groups(self):
        res = [bench("open", [row("collection.open", 100)]),
               ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 200)], runner=IPHONE)]
        ev = perf_gate.evaluate(catalog(), inputs(*res), {})
        groups = ev["groups"]
        self.assertEqual(groups["open"]["status"], "pass")  # the device-only case waits on U256
        self.assertEqual(groups["launch"]["status"], "fail")
        self.assertEqual(groups["editor"]["status"], "pending")
        self.assertEqual(groups["editor"]["pending"], ["U244"])
        self.assertEqual(groups["search"]["status"], "missing")
        self.assertEqual(groups["review-loop"]["status"], "missing")


class Completeness(unittest.TestCase):
    """R1861: a declared interval is not an emitted one. Every result of a
    suite must measure each case the catalog gates on that suite (pending
    cases aside), or the gate fails."""

    def test_a_row_without_a_fixture_is_not_taken_at_the_cases(self):
        res = bench("open", [row("collection.open", 100, fixture=None)])
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertEqual(ev["budgets"]["R1823"], "missing")
        self.assertEqual(ev["groups"]["open"]["status"], "missing")
        self.assertEqual(ev["evaluations"], [])
        self.assertIn("its rows are at no fixture, the case is at 150k", failure_text(ev))

    def test_a_case_without_a_fixture_accepts_a_row_without_fixture_metadata(self):
        raw = raw_catalog()
        [b for b in raw["budgets"] if b["id"] == "R1823"][0]["cases"][0]["fixture"] = None
        res = bench("open", [row("collection.open", 100, fixture=None)])
        ev = perf_gate.evaluate(catalog(raw), inputs(res), {})
        self.assertEqual(ev["status"], "pass", failure_text(ev))
        self.assertEqual(ev["budgets"]["R1823"], "pass")

    def test_a_suite_that_declares_the_probe_but_never_measures_it_fails(self):
        # The probe's test removed, testLaunch kept: the result holds the
        # launch time only, and SmokePerfTests still declares the probe.
        res = ui("SmokePerfTests", [row("xctest.application-launch", 900.0, test="SmokePerfTests/testLaunch()")])
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("probe.syntheticDelay", failure_text(ev))
        self.assertIn("probe-synthetic-delay/delay", failure_text(ev))
        self.assertEqual(ev["groups"]["smoke"]["status"], "missing")
        res = ui("SmokePerfTests", [row("probe.syntheticDelay", 30.0)])
        self.assertNotEqual(perf_gate.evaluate(catalog(), inputs(res), {})["status"], "fail")

    def test_a_result_without_one_of_its_suites_cases_fails_and_names_the_budget(self):
        res = ui("ReviewLoopPerfTests", review_rows(omit=("review.touchDownToTint",)), runner=IPHONE)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1816/same-frame", failure_text(ev))
        self.assertIn("review.touchDownToTint", failure_text(ev))
        self.assertEqual(ev["budgets"]["R1816"], "missing")

    def test_rows_only_at_a_fixture_the_case_does_not_take_are_missing(self):
        rows = [dict(r, fixture="review-corpus-scripted") if r["name"] == "review.answerToNextVisible" else r
                for r in review_rows()]
        ev = perf_gate.evaluate(catalog(), inputs(ui("ReviewLoopPerfTests", rows, runner=IPHONE)), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1807/frames-p95", failure_text(ev))
        self.assertIn("review-corpus-scripted", failure_text(ev))

    def test_a_core_bench_must_measure_its_cases_too(self):
        ev = perf_gate.evaluate(catalog(), inputs(bench("open", [row("collection.close", 5)])), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1823/core", failure_text(ev))

    def test_pending_cases_other_devices_and_instruction_counts_are_not_required(self):
        # MemoryPerfTests' only case waits on U245.
        ev = perf_gate.evaluate(catalog(), inputs(ui("MemoryPerfTests", [row("xctest.application-launch", 1.0)])), {})
        self.assertNotEqual(ev["status"], "fail", failure_text(ev))
        # R1816 is iPhone and iPad only: the Mac host's run does not owe it.
        res = ui("ReviewLoopPerfTests", review_rows(omit=("review.touchDownToTint",)), runner=MAC)
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        self.assertEqual(ev["status"], "pass", failure_text(ev))
        # Instruction counts are judged against their baseline, not cases.
        raw = raw_catalog()
        [b for b in raw["budgets"] if b["id"] == "R1823"][0]["cases"][0]["where"] = ["core-docker", "core-mac"]
        res = bench("open", [row("open.instructions", 10, metric="instructions", unit="count", percentile=None)],
                    runner=DOCKER, kind="instruction-count")
        ev = perf_gate.evaluate(catalog(raw), inputs(res), {})
        self.assertNotIn("R1823", failure_text(ev))
        self.assertEqual(ev["status"], "warn")


class Baselines(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="perf-baselines-")
        self.addCleanup(shutil.rmtree, self.dir)

    def test_rebaselining_writes_baselines_the_next_evaluation_reads(self):
        res = ui("BrowserSearchPerfTests", [row("search.firstPage", 40.0, test="BrowserSearchPerfTests/testPlain()"),
                                            row("xctest.application-launch", 900.0)])
        ev = perf_gate.evaluate(catalog(), inputs(res), {})
        written = perf_gate.rebaseline_docs(catalog(), ev, "first simulator baseline", COMMIT)
        self.assertEqual(len(written), 1)
        path, doc = list(written.items())[0]
        self.assertEqual(path, os.path.join(perf_gate.runner_slug(SIM["id"]), "BrowserSearchPerfTests.json"))
        self.assertEqual(doc["reason"], "first simulator baseline")
        # Only rows the catalog gates are kept.
        self.assertEqual([r["name"] for r in doc["rows"]], ["search.firstPage"])
        perf_gate.write_baselines(self.dir, written)
        read, errors = perf_gate.read_baselines(self.dir)
        self.assertEqual(errors, [])
        ev = perf_gate.evaluate(catalog(), inputs(ui("BrowserSearchPerfTests", [
            row("search.firstPage", 44.8, test="BrowserSearchPerfTests/testPlain()")])), read)
        self.assertEqual(ev["status"], "fail")
        self.assertIn("+12% > 10%", failure_text(ev))

    def test_rebaselining_needs_a_reason(self):
        ev = perf_gate.evaluate(catalog(), inputs(ui("BrowserSearchPerfTests", [row("search.firstPage", 40.0)])), {})
        with self.assertRaises(perf_gate.Invalid):
            perf_gate.rebaseline_docs(catalog(), ev, " ", COMMIT)

    def test_rebaselining_skips_rejected_results_and_wall_clock_benches(self):
        res = [ui("BrowserSearchPerfTests", [row("search.firstPage", 40.0)], configuration="Debug"),
               bench("open", [row("collection.open", 100)])]
        ev = perf_gate.evaluate(catalog(), inputs(*res), {})
        self.assertEqual(perf_gate.rebaseline_docs(catalog(), ev, "why", COMMIT), {})

    def test_a_baseline_without_a_reason_or_in_the_wrong_place_is_an_error(self):
        doc = baseline(SIM, "BrowserSearchPerfTests", [], reason="")
        perf_gate.write_baselines(self.dir, {os.path.join(perf_gate.runner_slug(SIM["id"]), "BrowserSearchPerfTests.json"): doc})
        _, errors = perf_gate.read_baselines(self.dir)
        self.assertTrue(any("reason" in e for e in errors), errors)
        shutil.rmtree(self.dir)
        doc = baseline(SIM, "BrowserSearchPerfTests", [])
        perf_gate.write_baselines(self.dir, {os.path.join("elsewhere", "BrowserSearchPerfTests.json"): doc})
        _, errors = perf_gate.read_baselines(self.dir)
        self.assertTrue(any("runner" in e for e in errors), errors)


class Discovery(unittest.TestCase):
    """Which perf results a run of the gate reads: results of the perf jobs
    on its own commit and tree state, the latest per suite and runner."""

    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="perf-gate-results-")
        self.addCleanup(shutil.rmtree, self.root)
        self.seq = 0

    def job(self, job, docs, sha=COMMIT, dirty=False, dry_run=False, status="pass"):
        self.seq += 1
        run = os.path.join(self.root, job, "20261016T2000%02dZ-1" % self.seq)
        os.makedirs(run)
        artifacts = []
        for i, doc in enumerate(docs):
            path = os.path.join(run, "results", "%s.json" % doc["suite"])
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w") as f:
                json.dump(doc, f)
            artifacts.append({"kind": "perf-result", "path": path})
        with open(os.path.join(run, "result.json"), "w") as f:
            json.dump({"job": job, "run_id": os.path.basename(run), "status": status, "dry_run": dry_run,
                       "finished_at": "2026-10-16T20:00:%02dZ" % self.seq,
                       "commit": {"sha": sha, "dirty": dirty}, "artifacts": artifacts}, f)
        return run

    def test_only_this_commit_and_tree_state_and_the_latest_per_suite_and_runner(self):
        self.job("perf-ui", [ui("SmokePerfTests", [row("probe.syntheticDelay", 30)])], sha="b" * 40)
        self.job("perf-ui", [ui("SmokePerfTests", [row("probe.syntheticDelay", 31)])], dirty=True)
        self.job("perf-ui", [ui("SmokePerfTests", [row("probe.syntheticDelay", 32)])], dry_run=True)
        self.job("perf-ui", [ui("SmokePerfTests", [row("probe.syntheticDelay", 33)])])
        latest = self.job("perf-ui", [ui("SmokePerfTests", [row("probe.syntheticDelay", 34)])])
        self.job("bench-mac", [bench("open", [row("collection.open", 100)])])
        self.job("other", [bench("open", [row("collection.open", 100)])])
        found, problems = perf_gate.discover(self.root, ["perf-ui", "bench-mac"], COMMIT, False)
        self.assertEqual(problems, [])
        self.assertEqual(sorted((i["job"], i["result"]["suite"]) for i in found),
                         [("bench-mac", "open"), ("perf-ui", "SmokePerfTests")])
        smoke = [i for i in found if i["job"] == "perf-ui"][0]
        self.assertEqual(smoke["run_id"], os.path.basename(latest))
        self.assertEqual(smoke["result"]["results"][0]["value"], 34)

    def test_required_jobs_without_results_fail_the_evaluation(self):
        self.job("perf-ui", [ui("SmokePerfTests", [row("probe.syntheticDelay", 30)])])
        found, _ = perf_gate.discover(self.root, ["perf-ui", "bench-mac"], COMMIT, False)
        ev = perf_gate.evaluate(catalog(), found, {}, require=["perf-ui", "bench-mac"])
        self.assertEqual(ev["status"], "fail")
        self.assertIn("bench-mac", failure_text(ev))


class CatalogChecks(unittest.TestCase):
    REGISTRY = """
public enum IntervalNames {
    public static let answerToNextVisible = IntervalName(
        "review.answerToNextVisible", .review, fieldReported: true)
    public static let editorOpenToEditable = IntervalName(
        "editor.openToEditable", .editor, fieldReported: true)
    public static let reviewStartToFirstCard = IntervalName("review.startToFirstCard", .review)
    public static let searchFirstPage = IntervalName("search.firstPage", .search)
    public static let probeSyntheticDelay = IntervalName("probe.syntheticDelay", .probe)
    public static let probeNativeSwap = IntervalName("probe.nativeSwap", .probe)
}
public struct BridgeCall {
    public static let coreVersion = BridgeCall("core_version")
}
"""
    SMOKE = """
final class SmokePerfTests: PerfTestCase {
    override nonisolated class var intervalNames: [String] { [IntervalNames.probeSyntheticDelay.name] }

    func testProbe() throws {
        let app = launch(MeguriLaunch(route: "perf/probe"))
        try measure(app: app) { _ in post("probe") }
    }

    func testLaunch() throws {
        measureLaunch(MeguriLaunch(animations: false))
    }
}
"""
    TESTS = {
        "SmokePerfTests.swift": SMOKE,
        # The measuring call in a base class's helper.
        "ReviewLoopPerfTests.swift": """
class ReviewPerfBase: PerfTestCase {
    override nonisolated class var intervalNames: [String] {
        [IntervalNames.answerToNextVisible.name, "probe.nativeSwap",
         IntervalNames.reviewStartToFirstCard.name]
    }

    func session(_ app: XCUIApplication) throws {
        try measure(
            app: app, prepare: { _ in }) { _ in }
    }
}
final class ReviewLoopPerfTests: ReviewPerfBase {
    func testSession() throws { try session(launch(MeguriLaunch())) }
}
""",
        # ... and in an extension.
        "BrowserSearchPerfTests.swift": """
final class BrowserSearchPerfTests: PerfTestCase {
    override nonisolated class var intervalNames: [String] { [IntervalNames.searchFirstPage.name] }
    func testPlain() throws { try search() }
}

extension BrowserSearchPerfTests {
    func search() throws { try self.measure(app: launch(MeguriLaunch())) { _ in } }
}
""",
    }

    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="perf-gate-repo-")
        self.addCleanup(shutil.rmtree, self.root)
        os.makedirs(os.path.join(self.root, "tests"))
        with open(os.path.join(self.root, "IntervalNames.swift"), "w") as f:
            f.write(self.REGISTRY)
        for name, text in self.TESTS.items():
            with open(os.path.join(self.root, "tests", name), "w") as f:
                f.write(text)
        lines = ["# Plan", "", "### Requirements", ""]
        lines += ["- R%d. Budget %d." % (n, n) for n in (1583, 1806, 1807, 1808, 1809, 1810)]
        with open(os.path.join(self.root, "plan.md"), "w") as f:
            f.write("\n".join(lines) + "\n")

    def raw(self):
        raw = raw_catalog()
        raw.update({"plan": "plan.md", "budget_range": ["R1807", "R1810"], "extra_budgets": ["R1583"],
                    "registry": "IntervalNames.swift", "perf_tests": "tests"})
        keep = {"R1807": None, "R1813": "R1808", "R1825": "R1809", "R1833": "R1810", "R1823": "R1583"}
        budgets = []
        for b in raw["budgets"]:
            if b["id"] in keep:
                b = copy.deepcopy(b)
                b["id"] = keep[b["id"]] or b["id"]
                budgets.append(b)
        raw["budgets"] = budgets
        return raw

    @staticmethod
    def entry(raw, rid):
        return [b for b in raw["budgets"] if b["id"] == rid][0]

    def check(self, raw, milestones=None):
        cat, errors = perf_gate.normalize(raw)
        self.assertEqual(errors, [])
        return perf_gate.check(cat, self.root, milestones)

    def test_a_consistent_catalog_passes(self):
        self.assertEqual(self.check(self.raw()), [])

    def test_the_registry_has_editor_open_to_editable_but_no_perf_test_emits_it(self):
        raw = self.raw()
        del self.entry(raw, "R1810")["cases"][0]["pending"]
        errors = self.check(raw)
        self.assertTrue(any("editor.openToEditable" in e and "EditorPerfTests" in e for e in errors), errors)

    def test_an_r1861_interval_without_a_catalog_entry_fails(self):
        raw = self.raw()
        raw["budgets"] = [b for b in raw["budgets"] if b["id"] != "R1810"]
        raw["budget_range"] = ["R1807", "R1809"]
        errors = self.check(raw)
        self.assertTrue(any("editor.openToEditable" in e and "R1861" in e for e in errors), errors)

    def test_every_budget_r_id_in_the_range_has_exactly_one_entry(self):
        raw = self.raw()
        raw["budgets"] = [b for b in raw["budgets"] if b["id"] != "R1809"]
        self.assertTrue(any("R1809" in e for e in self.check(raw)))
        raw = self.raw()
        raw["budgets"].append(copy.deepcopy(raw["budgets"][0]))
        _, errors = perf_gate.normalize(raw)
        self.assertTrue(any("R1807" in e and "twice" in e for e in errors), errors)
        raw = self.raw()
        extra = copy.deepcopy(raw["budgets"][0])
        extra["id"] = "R1900"
        raw["budgets"].append(extra)
        self.assertTrue(any("R1900" in e for e in self.check(raw)))

    def test_unregistered_names_fail_unless_pending(self):
        raw = self.raw()
        self.entry(raw, "R1807")["cases"][0]["name"] = "review.answerToNext"
        errors = self.check(raw)
        self.assertTrue(any("review.answerToNext" in e and "IntervalNames" in e for e in errors), errors)
        self.entry(raw, "R1807")["cases"][0]["pending"] = "U243"
        self.assertFalse(any("review.answerToNext " in e for e in self.check(raw)))

    def write_test(self, name, text):
        with open(os.path.join(self.root, "tests", name), "w") as f:
            f.write(text)

    def test_a_suite_that_declares_an_interval_but_never_measures_it_fails(self):
        # The probe's test removed, testLaunch kept: the declaration stays,
        # but measureLaunch measures no interval, so nothing emits the probe.
        smoke = self.SMOKE.replace("""    func testProbe() throws {
        let app = launch(MeguriLaunch(route: "perf/probe"))
        try measure(app: app) { _ in post("probe") }
    }
""", """    /// The probe used to be measured with measure(app:) here.
    func measure(app: XCUIApplication, action: () -> Void) {}
""")
        self.assertNotEqual(smoke, self.SMOKE)
        self.write_test("SmokePerfTests.swift", smoke)
        errors = self.check(self.raw())
        self.assertTrue(any("SmokePerfTests" in e and "probe.syntheticDelay" in e and "measure(app:" in e
                            for e in errors), errors)
        # A measured interval the catalog does not need is not an error.
        raw = self.raw()
        raw["probes"][0]["cases"][0]["pending"] = "U243"
        self.assertEqual(self.check(raw), [])

    def test_a_case_whose_suite_does_not_declare_its_interval_fails(self):
        raw = self.raw()
        self.entry(raw, "R1809")["cases"][0]["suite"] = "SmokePerfTests"  # search.firstPage
        errors = self.check(raw)
        self.assertTrue(any("SmokePerfTests" in e and "search.firstPage" in e for e in errors), errors)

    def test_milestone_budget_devices_and_groups_must_agree(self):
        milestones = {"budget_devices": {"R1808": ["ipad", "mac"]}, "budget_groups": {"M1": {"open": ["R1583"]}},
                      "perf_gate_groups": {"M1": ["launch", "nowhere"]}}
        errors = self.check(self.raw(), milestones)
        self.assertTrue(any("R1808" in e and "budget_devices" in e for e in errors), errors)
        self.assertTrue(any("nowhere" in e for e in errors), errors)


class Normalize(unittest.TestCase):
    def errors(self, mutate):
        raw = raw_catalog()
        mutate(raw)
        return perf_gate.normalize(raw)[1]

    def test_invalid_cases(self):
        def where(raw):
            raw["budgets"][1]["cases"][0]["where"] = ["laptop"]
        self.assertTrue(any("where" in e for e in self.errors(where)))

        def nolimit(raw):
            raw["budgets"][1]["cases"][0]["limit"] = None
        self.assertTrue(any("limit" in e for e in self.errors(nolimit)))

        def nodevice(raw):
            raw["budgets"][1]["cases"][0]["where"] = ["simulator"]
        self.assertTrue(any("device" in e for e in self.errors(nodevice)))

        def group(raw):
            raw["budgets"][1]["group"] = "elsewhere"
        self.assertTrue(any("elsewhere" in e for e in self.errors(group)))

        def unit(raw):
            raw["budgets"][1]["cases"][0]["unit"] = "seconds"
        self.assertTrue(any("unit" in e for e in self.errors(unit)))


class DeviceRun(unittest.TestCase):
    """The report a device run records (U256's evidence) and the checks the
    milestone gate applies to it."""

    def device_results(self, start=120.0, search=80.0):
        return inputs(
            ui("ReviewLoopPerfTests", [
                row("review.startToFirstCard", start),
                row("review.answerToNextVisible", 3, metric="frames", unit="frames", fixture="review-corpus"),
                row("review.answerToNextVisible", 4, metric="frames", unit="frames", percentile="p99",
                    fixture="review-corpus"),
                row("probe.nativeSwap", 3, metric="frames", unit="frames"),
                row("probe.nativeSwap", 3, metric="frames", unit="frames", percentile="p99"),
                row("review.touchDownToTint", 0, metric="frames", unit="frames")], runner=IPHONE),
            ui("BrowserSearchPerfTests", [row("search.firstPage", search)], runner=IPHONE))

    def report(self, results=None):
        ev = perf_gate.evaluate(catalog(), results or self.device_results(), {})
        report, problems = perf_gate.device_report(catalog(), ev, "M1", "iphone", COMMIT)
        self.assertEqual(problems, [])
        return report

    def test_a_passing_report_verifies(self):
        report = self.report()
        self.assertEqual(report["configuration"], "Profile")
        self.assertEqual(report["runner"]["id"], IPHONE["id"])
        statuses = {b["id"]: b["status"] for b in report["budgets"]}
        self.assertEqual(statuses["R1813"], "pass")
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1807", "R1813", "R1825"])
        self.assertEqual(outcome, "pass", message)

    def test_a_breach_is_recorded_as_a_failure(self):
        report = self.report(self.device_results(start=160.0))
        self.assertEqual({b["id"]: b["status"] for b in report["budgets"]}["R1813"], "fail")
        outcome, message, details = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
        self.assertEqual(outcome, "fail")
        self.assertIn("R1813", message)

    def test_a_pass_whose_value_breaches_the_budget_fails(self):
        report = self.report()
        b = [b for b in report["budgets"] if b["id"] == "R1813"][0]
        b["cases"][0]["value"] = 170.0
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
        self.assertEqual(outcome, "fail")
        self.assertIn("170", message)

    def test_a_missing_case_or_budget_fails(self):
        report = self.report()
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813", "R1816", "R1851"])
        self.assertEqual(outcome, "pending")  # R1851's iPhone case waits on U245
        self.assertIn("U245", message)
        report["budgets"] = [b for b in report["budgets"] if b["id"] != "R1813"]
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
        self.assertEqual(outcome, "fail")
        self.assertIn("no result for R1813", message)

    def test_wrong_configuration_or_runner_fails(self):
        for key, value, expected in (("configuration", "Debug", "Profile"),
                                     ("runner", dict(SIM), "device"),
                                     ("schema_version", 2, "schema_version")):
            report = self.report()
            report[key] = value
            outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
            self.assertEqual(outcome, "fail", key)
            self.assertIn(expected, message)
        report = self.report()
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "ipad", ["R1813"])
        self.assertEqual(outcome, "fail")

    def test_a_1m_completion_does_not_satisfy_the_150k_budget(self):
        # R1803: at 1M the start only has to complete; R1813 is stated at 150k.
        results = inputs(ui("ReviewLoopPerfTests", review_rows(start=5000.0, start_fixture="1m"), runner=IPHONE),
                         ui("BrowserSearchPerfTests", [row("search.firstPage", 80.0)], runner=IPHONE))
        report = self.report(results)
        r1813 = [b for b in report["budgets"] if b["id"] == "R1813"][0]
        self.assertEqual(r1813["status"], "incomplete")
        self.assertIn("missing", [c["status"] for c in r1813["cases"]])
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
        self.assertEqual(outcome, "fail")
        self.assertIn("R1813/warm", message)
        self.assertIn("150k", message)
        # With the 150k measurement too, the budget passes.
        rows = review_rows(start=120.0) + [row("review.startToFirstCard", 5000.0, fixture="1m")]
        report = self.report(inputs(ui("ReviewLoopPerfTests", rows, runner=IPHONE)))
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
        self.assertEqual(outcome, "pass", message)

    def test_a_report_whose_only_record_of_a_case_is_a_1m_completion_fails(self):
        # A hand-made report: R1813's one record moved to 1M, still "pass".
        report = self.report()
        b = [b for b in report["budgets"] if b["id"] == "R1813"][0]
        b["cases"] = [dict(c, fixture="1m", value=5000.0, raw=5000.0) for c in b["cases"]]
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
        self.assertEqual(outcome, "fail")
        self.assertIn("150k", message)

    def test_a_report_without_fixture_metadata_does_not_prove_a_fixture_bound_case(self):
        report = self.report()
        b = [b for b in report["budgets"] if b["id"] == "R1813"][0]
        for c in b["cases"]:
            c.pop("fixture", None)
        outcome, message, _ = perf_gate.verify_device_report(report, catalog(), "iphone", ["R1813"])
        self.assertEqual(outcome, "fail")
        self.assertIn("150k", message)

    def test_a_report_needs_device_results(self):
        ev = perf_gate.evaluate(catalog(), inputs(ui("SmokePerfTests", [row("probe.syntheticDelay", 30)])), {})
        _, problems = perf_gate.device_report(catalog(), ev, "M1", "iphone", COMMIT)
        self.assertTrue(problems)


class CommandLine(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="perf-gate-cli-")
        self.addCleanup(shutil.rmtree, self.dir)

    def write(self, name, value):
        path = os.path.join(self.dir, name)
        with open(path, "w") as f:
            json.dump(value, f)
        return path

    def run_(self, *args):
        return subprocess.run([sys.executable, "-B", "-I", SCRIPT] + list(args), stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, universal_newlines=True)

    def test_evaluate_writes_the_evaluation_and_exits_with_its_status(self):
        cat = self.write("catalog.json", {"catalog": raw_catalog(), "milestones": None})
        res = self.write("r.json", ui("ReviewLoopPerfTests", [row("review.startToFirstCard", 160)], runner=IPHONE))
        out = os.path.join(self.dir, "evaluation.json")
        p = self.run_("evaluate", "--catalog", cat, "--baselines", os.path.join(self.dir, "none"),
                      "--results", res, "--out", out)
        self.assertEqual(p.returncode, 1, p.stderr)
        with open(out) as f:
            ev = json.load(f)
        self.assertEqual(ev["status"], "fail")
        self.assertIn("R1813", failure_text(ev))


@unittest.skipUnless(perf_gate.yaml_available(), "PyYAML reads budgets.yaml (the dev container has it)")
class RealCatalog(unittest.TestCase):
    """tools/perf/budgets.yaml itself."""

    @classmethod
    def setUpClass(cls):
        loaded = perf_gate.load(os.path.join(REPO, "tools/perf/budgets.yaml"),
                                os.path.join(REPO, "docs/process/milestones.yaml"))
        cls.raw, cls.milestones = loaded["catalog"], loaded["milestones"]
        cls.catalog, cls.errors = perf_gate.normalize(cls.raw)

    def test_it_is_valid_and_consistent_with_the_repository(self):
        self.assertEqual(self.errors, [])
        self.assertEqual(perf_gate.check(self.catalog, REPO, self.milestones), [])

    def test_every_r1807_to_r1855_budget_and_r1583_has_exactly_one_entry(self):
        ids = [b["id"] for b in self.raw["budgets"]]
        expected = ["R%d" % n for n in range(1807, 1856)] + ["R1583"]
        self.assertEqual(sorted(ids), sorted(expected))
        self.assertEqual(len(ids), len(set(ids)))

    def test_the_plan_numbers(self):
        cases = {c["key"]: c for c in self.catalog["cases"]}
        self.assertEqual((cases["R1813/warm"]["limit"], cases["R1813/warm"]["percentile"]), (150, "p95"))
        self.assertEqual(cases["R1813/cold"]["limit"], 400)
        self.assertEqual(cases["R1818/cold-launch"]["limit"], 500)
        self.assertEqual(cases["R1825/first-page"]["limit"], 100)
        # R1861's keystroke interval is the 60 ms debounce plus R1825's value.
        self.assertEqual(cases["R1825/keystroke-to-results"]["limit"], 160)
        self.assertEqual(cases["R1823/core"]["limit"], 150)
        self.assertEqual(cases["R1851/normal-use"]["limit"], 150)
        self.assertEqual(cases["R1852/heavy-operations"]["limit"], 300)
        self.assertEqual((cases["R1807/frames-p95"]["limit"], cases["R1807/frames-p99"]["limit"]), (0, 1))
        self.assertEqual(self.catalog["rules"]["ui_regression_percent"], 10)
        self.assertEqual(self.catalog["rules"]["icount_regression_percent"], 5)
        # Search decomposition (U90): the core first page on the M3 Max.
        self.assertEqual(cases["R1825/core-simple"]["limit"], 15)
        self.assertEqual(cases["R1825/core-plain"]["limit"], 50)
        self.assertEqual(cases["R1826/core-slow"]["limit"], 150)

    def test_the_perf_gate_groups_milestones_scope(self):
        for groups in self.milestones["perf_gate_groups"].values():
            for g in groups:
                self.assertIn(g, self.catalog["groups"])

    def device_clauses(self, rid):
        return sorted((c["name"], c["limit"], c["fixture"]) for c in self.catalog["cases"]
                      if c["budget"] == rid and "device" in c["where"])

    def test_every_clause_of_the_sync_import_and_media_budgets_has_a_device_case(self):
        # R1802: every budget holds on the user's devices, so each limit the
        # requirement states is measured there, not only in the Mac benches.
        r1842 = [limit for _, limit, _ in self.device_clauses("R1842")]
        self.assertIn(1000, r1842)  # 1 s of local work
        self.assertIn(3000, r1842)  # 3 s end to end
        r1843 = self.device_clauses("R1843")
        for name in ("sync.fullUpload.localWork", "sync.fullDownload.localWork"):
            self.assertIn((name, 3000, "private-collection"), r1843)  # the user's collection
            self.assertIn((name, 10000, "150k"), r1843)
        r1846 = self.device_clauses("R1846")
        self.assertIn(("import.apkg", 3000, "apkg-10k"), r1846)
        self.assertIn(10000, [limit for name, limit, _ in r1846 if name == "import.colpkg"])
        self.assertEqual(sorted(n for n, _, _ in self.device_clauses("R1855")),
                         ["media.fileHashes", "media.folderListings"])

    def iphone_results(self, *suites):
        return inputs(*(ui(s, rows, runner=IPHONE) for s, rows in suites))

    def test_a_device_report_without_those_clauses_stays_incomplete(self):
        results = self.iphone_results(
            ("SyncPerfTests", [row("sync.startToEnd", 2000.0),
                               row("sync.fullDownload.localWork", 2000.0, fixture="private-collection")]),
            ("MaintenancePerfTests", [row("import.apkg", 2000.0, fixture="apkg-10k")]))
        required = ["R1842", "R1843", "R1846"]
        ev = perf_gate.evaluate(self.catalog, results, {})
        report, problems = perf_gate.device_report(self.catalog, ev, "M1", "iphone", COMMIT)
        self.assertEqual(problems, [])
        statuses = {b["id"]: b["status"] for b in report["budgets"]}
        self.assertEqual([statuses[r] for r in required], ["incomplete"] * 3)
        outcome, message, _ = perf_gate.verify_device_report(report, self.catalog, "iphone", required)
        self.assertEqual(outcome, "pending", message)  # U256 lands the device legs
        # Once U256 lands them, the report misses them: it fails.
        raw = copy.deepcopy(self.raw)
        for b in raw["budgets"]:
            if b["id"] in required:
                for c in b["cases"]:
                    if c["where"] == ["device"]:
                        c.pop("pending", None)
        landed, errors = perf_gate.normalize(raw)
        self.assertEqual(errors, [])
        ev = perf_gate.evaluate(landed, results, {})
        report, _ = perf_gate.device_report(landed, ev, "M1", "iphone", COMMIT)
        self.assertEqual([b["status"] for b in report["budgets"] if b["id"] in required], ["incomplete"] * 3)
        outcome, message, _ = perf_gate.verify_device_report(report, landed, "iphone", required)
        self.assertEqual(outcome, "fail")
        for name in ("sync.normal.localWork", "sync.fullUpload.localWork", "import.colpkg"):
            case = [c for c in landed["cases"] if c["name"] == name and c["where"] == ["device"]][0]
            self.assertIn(case["key"], message)

    def test_a_1m_launch_does_not_prove_the_150k_launch_budget(self):
        rows = [row(n, 900.0, fixture="1m") for n in
                ("xctest.application-launch", "launch.toDeckListInteractive", "launch.resumeToInteractive")]
        ev = perf_gate.evaluate(self.catalog, self.iphone_results(("LaunchHomePerfTests", rows)), {})
        report, problems = perf_gate.device_report(self.catalog, ev, "M1", "iphone", COMMIT)
        self.assertEqual(problems, [])
        self.assertEqual([b["status"] for b in report["budgets"] if b["id"] == "R1818"], ["incomplete"])
        outcome, message, _ = perf_gate.verify_device_report(report, self.catalog, "iphone", ["R1818"])
        self.assertNotEqual(outcome, "pass", message)

    def test_the_smoke_without_its_probe_fails(self):
        res = ui("SmokePerfTests", [row("xctest.application-launch", 900.0, test="SmokePerfTests/testLaunch()")])
        ev = perf_gate.evaluate(self.catalog, inputs(res), {})
        self.assertEqual(ev["status"], "fail")
        self.assertIn("probe.syntheticDelay", failure_text(ev))


if __name__ == "__main__":
    unittest.main()
