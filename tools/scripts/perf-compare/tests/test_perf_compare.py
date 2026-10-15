"""Tests for perf_compare (U12, R1863, R1864): XCTest metrics and gungraun
summaries into perf results (tools/perf/perf-result.schema.json), the
percentile rank rule and schema validation."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "data")
SCRIPT = os.path.join(os.path.dirname(HERE), "perf_compare.py")
sys.path.insert(0, os.path.dirname(HERE))

import perf_compare  # noqa: E402

SIMULATOR = {"kind": "simulator", "id": "iPhone 17 Pro/24A434", "model": "iPhone 17 Pro", "os": "iOS 27.0",
             "os_build": "24A434", "arch": "arm64"}


def data(name):
    return os.path.join(DATA, name)


def run(*args):
    return subprocess.run([sys.executable, SCRIPT, *args], capture_output=True, text=True)


class PercentileTest(unittest.TestCase):
    def test_one_slow_value_in_a_hundred_is_the_p99(self):
        values = [1.0] * 99 + [20.0]
        self.assertEqual(perf_compare.percentile(values, 99), 20.0)
        self.assertEqual(perf_compare.percentile(values, 95), 1.0)
        self.assertEqual(perf_compare.percentile(values, 100), 20.0)

    def test_five_iterations(self):
        values = [31.0, 30.0, 33.0, 30.5, 34.0]
        self.assertEqual(perf_compare.percentile(values, 50), 31.0)
        self.assertEqual(perf_compare.percentile(values, 95), 34.0)
        self.assertEqual(perf_compare.percentile(values, 99), 34.0)


class XcresultTest(unittest.TestCase):
    """On the metrics of real SmokePerfTests runs: the iPhone 17 Pro
    simulator (xcresult-smoke-metrics.json) and the Mac test VM
    (xcresult-smoke-metrics-mac.json). smoke-mac-vm/ and smoke-iphone/ each
    hold one whole run: metrics, tests and the activities that declare its
    intervals (smoke-iphone/ feeds tools/tests/perf.bats)."""

    def setUp(self):
        self.dir = tempfile.mkdtemp()
        with open(data("xcresult-smoke-metrics.json")) as f:
            self.metrics = json.load(f)

    def convert(self, suite="SmokePerfTests"):
        return perf_compare.from_xcresult(self.metrics, suite, "Profile", SIMULATOR, [])

    def mac_run(self):
        with open(data("smoke-mac-vm/metrics.json")) as f:
            metrics = json.load(f)
        with open(data("smoke-mac-vm/activities.json")) as f:
            activities = json.load(f)
        return metrics, activities

    def rows(self, doc, name, metric):
        return [r for r in doc["results"] if r["name"] == name and r["metric"] == metric]

    def test_a_registered_interval_is_written_with_its_percentiles_and_measured_values(self):
        doc = self.convert()
        self.assertEqual(doc["kind"], "ui")
        self.assertEqual(doc["configuration"], "Profile")
        self.assertEqual(doc["runner"]["id"], "iPhone 17 Pro/24A434")
        probe = self.rows(doc, "probe.syntheticDelay", "duration")
        self.assertEqual([r["percentile"] for r in probe], ["p50", "p95", "p99", "max"])
        for row in probe:
            self.assertEqual(row["unit"], "ms")
            self.assertEqual(row["test"], "SmokePerfTests/testProbeIntervalMeasuresItsSyntheticDelay()")
            self.assertEqual(row["samples"], len(row["measurements"]))
            # The probe waits 30 ms between begin and end.
            self.assertGreaterEqual(row["value"], 30.0)
            self.assertLess(row["value"], 100.0)
        self.assertEqual(probe[0]["value"], perf_compare.percentile(probe[0]["measurements"], 50))

    def test_xctest_metrics_keep_their_kind(self):
        doc = self.convert()
        names = {(r["name"], r["metric"], r["unit"]) for r in doc["results"]}
        self.assertTrue(any(n.startswith("xctest.") for n, _, _ in names), names)
        self.assertTrue(any(u == "MB" for _, _, u in names), names)
        for name, metric, _ in names:
            self.assertRegex(metric, r"^[a-z][a-z0-9-]*$")

    def test_mac_hitch_metrics_convert(self):
        # The Mac (test VM) reports XCTHitchMetric; the iOS simulator does not.
        with open(data("xcresult-smoke-metrics-mac.json")) as f:
            mac = json.load(f)
        doc = perf_compare.from_xcresult(mac, "SmokePerfTests", "Profile", SIMULATOR, [])
        hitch = {(r["metric"], r["unit"]) for r in doc["results"] if r["name"] == "xctest.hitch"}
        self.assertEqual(hitch, {("hitch-time-ratio", "ms/s"), ("number-of-hitches", "count"),
                                 ("hitches-total-duration", "ms")})
        memory = {r["metric"] for r in doc["results"] if r["name"] == "xctest.memory"}
        self.assertEqual(memory, {"memory-physical", "absolute-memory-physical", "memory-peak-physical"})
        launch = [r for r in doc["results"] if r["name"] == "xctest.application-launch"]
        self.assertEqual({r["metric"] for r in launch}, {"duration"})

    def test_a_suite_without_metrics_fails(self):
        with self.assertRaisesRegex(perf_compare.InputError, "^suite NoSuchPerfTests has no metrics$"):
            self.convert("NoSuchPerfTests")
        _, activities = self.mac_run()
        with self.assertRaisesRegex(perf_compare.InputError, "suite SmokePerfTests has no metrics"):
            perf_compare.from_xcresult([], "SmokePerfTests", "Profile", SIMULATOR, activities)

    def test_declared_intervals_come_from_the_tests_activities(self):
        _, activities = self.mac_run()
        self.assertEqual(perf_compare.declared_intervals(activities, "SmokePerfTests"),
                         {"SmokePerfTests/testProbeIntervalMeasuresItsSyntheticDelay()": ["probe.syntheticDelay"]})
        self.assertEqual(perf_compare.declared_intervals(activities, "OtherPerfTests"), {})

    def test_a_declared_interval_with_measurements_converts(self):
        for run_ in ("smoke-mac-vm", "smoke-iphone"):
            with open(data(f"{run_}/metrics.json")) as f:
                metrics = json.load(f)
            with open(data(f"{run_}/activities.json")) as f:
                activities = json.load(f)
            doc = perf_compare.from_xcresult(metrics, "SmokePerfTests", "Profile", SIMULATOR, activities)
            probe = self.rows(doc, "probe.syntheticDelay", "duration")
            self.assertEqual([r["percentile"] for r in probe], ["p50", "p95", "p99", "max"], run_)
            self.assertTrue(all(30.0 <= r["value"] < 100.0 for r in probe), probe)

    def test_a_declared_interval_without_measurements_fails(self):
        metrics, activities = self.mac_run()
        for test in metrics:
            for run_ in test["testRuns"]:
                run_["metrics"] = [m for m in run_["metrics"] if "OSSignpost" not in m["identifier"]]
        with self.assertRaisesRegex(
                perf_compare.InputError,
                r"^SmokePerfTests/testProbeIntervalMeasuresItsSyntheticDelay\(\) declares probe.syntheticDelay "
                r"but has no measurements of it$"):
            perf_compare.from_xcresult(metrics, "SmokePerfTests", "Profile", SIMULATOR, activities)
        # A signpost metric whose iterations measured nothing counts as missing too.
        metrics, activities = self.mac_run()
        for test in metrics:
            for run_ in test["testRuns"]:
                for metric in run_["metrics"]:
                    if "OSSignpost" in metric["identifier"]:
                        metric["measurements"] = []
        with self.assertRaisesRegex(perf_compare.InputError, "declares probe.syntheticDelay"):
            perf_compare.from_xcresult(metrics, "SmokePerfTests", "Profile", SIMULATOR, activities)

    def test_the_cli_writes_a_valid_result(self):
        runner = os.path.join(self.dir, "runner.json")
        out = os.path.join(self.dir, "out", "SmokePerfTests.json")
        with open(runner, "w") as f:
            json.dump(SIMULATOR, f)
        done = run("xcresult", data("smoke-mac-vm/metrics.json"), "--suite", "SmokePerfTests",
                   "--configuration", "Profile", "--runner", runner,
                   "--activities", data("smoke-mac-vm/activities.json"), "--out", out)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(perf_compare.validate([out]), [])

    def test_the_cli_writes_nothing_when_a_declared_interval_is_missing(self):
        runner = os.path.join(self.dir, "runner.json")
        out = os.path.join(self.dir, "out", "SmokePerfTests.json")
        with open(runner, "w") as f:
            json.dump(SIMULATOR, f)
        empty = os.path.join(self.dir, "metrics.json")
        with open(empty, "w") as f:
            json.dump([], f)
        done = run("xcresult", empty, "--suite", "SmokePerfTests", "--configuration", "Profile", "--runner", runner,
                   "--activities", data("smoke-mac-vm/activities.json"), "--out", out)
        self.assertEqual(done.returncode, 1)
        self.assertIn("suite SmokePerfTests has no metrics", done.stderr)
        self.assertIn("declares probe.syntheticDelay but has no measurements of it", done.stderr)
        self.assertFalse(os.path.exists(out))

    def test_an_unknown_unit_is_refused(self):
        metrics = json.loads(json.dumps(self.metrics))
        metrics[0]["testRuns"][0]["metrics"][0]["unitOfMeasurement"] = "furlongs"
        with self.assertRaises(perf_compare.InputError):
            perf_compare.from_xcresult(metrics, metrics[0]["testIdentifier"].split("/")[0], "Profile", SIMULATOR, [])


class GungraunTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def test_per_bench_instruction_counts(self):
        with open(data("gungraun-open.jsonl")) as f:
            doc = perf_compare.from_gungraun(f.readlines(), "icount_open", perf_compare.docker_runner())
        self.assertEqual(doc["kind"], "instruction-count")
        self.assertEqual(doc["configuration"], "bench")
        self.assertEqual(doc["runner"]["kind"], "docker")
        self.assertEqual(doc["results"], [{
            "name": "collection.open",
            "test": "icount_open::collection::open cards_10k",
            "metric": "instructions",
            "unit": "count",
            "value": 1834060,
            "samples": 1,
            "fixture": "10k",
        }])

    def test_the_cli_writes_a_valid_result(self):
        out = os.path.join(self.dir, "icount_open.json")
        done = run("gungraun", data("gungraun-open.jsonl"), "--suite", "icount_open", "--out", out)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(perf_compare.validate([out]), [])

    def test_lines_that_are_not_summaries_are_skipped_and_other_versions_refused(self):
        with open(data("gungraun-open.jsonl")) as f:
            line = f.readline()
        doc = perf_compare.from_gungraun(["Running benches", line], "s", perf_compare.docker_runner())
        self.assertEqual(len(doc["results"]), 1)
        old = json.loads(line)
        old["version"] = "6"
        with self.assertRaises(perf_compare.InputError):
            perf_compare.from_gungraun([json.dumps(old)], "s", perf_compare.docker_runner())
        with self.assertRaises(perf_compare.InputError):
            perf_compare.from_gungraun(["no summary here"], "s", perf_compare.docker_runner())


class SignpostsTest(unittest.TestCase):
    def setUp(self):
        with open(data("os-signpost.xml")) as f:
            self.xml = f.read()

    def test_the_registry_is_read_from_interval_names(self):
        names = perf_compare.registry_names()
        for name in ("review.answerToNextVisible", "sync.startToEnd", "probe.syntheticDelay", "core_version",
                     "CancelToken.new"):
            self.assertIn(name, names)
        self.assertNotIn("review.notRegistered", names)

    def test_meguri_intervals_are_counted_and_unregistered_names_listed(self):
        summary = perf_compare.summarize_signposts(self.xml, set(perf_compare.registry_names()))
        self.assertEqual(summary["subsystem"], "com.antoinefink.meguri")
        self.assertEqual(summary["intervals"], [
            {"name": "core_build_info", "category": "CoreBridge", "count": 1},
            {"name": "probe.syntheticDelay", "category": "Probe", "count": 2},
            {"name": "review.notRegistered", "category": "Review", "count": 1},
        ])
        self.assertEqual(summary["unregistered"], ["review.notRegistered"])

    def test_the_cli_writes_the_summary(self):
        out = os.path.join(tempfile.mkdtemp(), "signposts.json")
        done = run("signposts", data("os-signpost.xml"), "--out", out)
        self.assertEqual(done.returncode, 0, done.stderr)
        with open(out) as f:
            self.assertEqual(json.load(f)["unregistered"], ["review.notRegistered"])


class ValidateTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def write(self, doc):
        path = os.path.join(self.dir, "result.json")
        with open(path, "w") as f:
            json.dump(doc, f)
        return path

    def good(self):
        return {
            "schema_version": 1, "suite": "open", "kind": "wall-clock", "configuration": "bench",
            "runner": {"kind": "docker", "id": "docker/linux-aarch64", "memory_measure": "peak_rss"},
            "results": [{"name": "collection.open", "metric": "duration", "unit": "ms", "percentile": "p95",
                         "value": 0.4, "samples": 400, "fixture": "150k"}],
        }

    def test_a_good_result_passes(self):
        self.assertEqual(perf_compare.validate([self.write(self.good())]), [])
        self.assertEqual(run("validate", self.write(self.good())).returncode, 0)

    def test_missing_and_unknown_fields_fail(self):
        doc = self.good()
        del doc["results"][0]["samples"]
        doc["results"][0]["percentile"] = "p90"
        doc["extra"] = True
        problems = perf_compare.validate([self.write(doc)])
        self.assertEqual(len(problems), 3, problems)
        self.assertEqual(run("validate", self.write(doc)).returncode, 1)


if __name__ == "__main__":
    unittest.main()
