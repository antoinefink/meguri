#!/usr/bin/env python3
"""Docker-only generated inputs; all native child operations are stubs."""
import contextlib
import copy
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("fetch_subject", ROOT / "tools/scripts/cloud-fetch-probe-host.py")
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
SECRET = "generated-private-native-description"


def good_observation():
    value = {k: {**dict.fromkeys(p.COUNTERS, 0), **dict.fromkeys(p.FLAGS, False)} for k in p.STAGES}
    for key in p.STAGES:
        value[key].update(fetchCompleted=True, drained=True)
    value["seed"].update(sawExpectedBytes=True, statePresent=True)
    value["fresh"]["sawExpectedBytes"] = True
    value.update(failurePhase="none", failureCategory="none", cleanupAttempted=True,
                 cleanupSucceeded=True, cleanupFailure="none")
    return value


class DiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.app = self.base / "Build/Products/Debug/Meguri.app"
        (self.app / "Contents/PlugIns/MeguriSnapshotTests.xctest").mkdir(parents=True)
        self.info = {"CFBundleIdentifier": p.APP_ID, "CFBundleExecutable": "Meguri",
                     "MeguriCardInspectionBuild": "debug", "LSUIElement": True}
        (self.app / "Contents/Info.plist").write_bytes(plistlib.dumps(self.info))
        self.plan = plistlib.loads((ROOT / "tools/tests/fixtures/cloud-agent-xctestrun-layout-mac.plist").read_bytes())
        self.target = self.plan["TestConfigurations"][0]["TestTargets"][0]
        self.target.pop("OnlyTestIdentifiers")
        self.target["SkipTestIdentifiers"] = ["CloudKitSoakTests", "CloudKitAccountSetupTests",
                                               "NativeCloudFetchScopeProbeTests"]
        self.target["TestingEnvironmentVariables"]["XCODE_TEST_PLAN_NAME"] = "Snapshot"
        self.xctestrun = self.app.parent.parent / "Meguri_Snapshot_macosx_generated.xctestrun"
        self.value = good_observation()
        self.summary = {"result": "Passed", "passedTests": 1, "failedTests": 0, "skippedTests": 0}
        self.tree = {"testNodes": [{"nodeType": "Test Case", "nodeIdentifier": p.SELECTOR, "result": "Passed"}]}
        self.code = 0
        self.commands, self.joined = [], []
        self.extra_line = ""
        self.fault = None

    def probe(self, stale=False):
        private = Path(tempfile.mkdtemp(dir=self.base)).resolve()
        (private / "build-start").touch()
        self.xctestrun.write_bytes(plistlib.dumps(self.plan))
        if stale:
            os.utime(self.xctestrun, ns=(1, 1))
        owner = self
        class Child:
            def __init__(self, argv, **kwargs):
                self.argv, self.kwargs = argv, kwargs
                self.returncode = owner.code if argv[0] == "/usr/bin/xcodebuild" else 0
                owner.commands.append((argv, kwargs))
                if argv[0] == "/usr/bin/xcodebuild":
                    raw = p.PREFIX + json.dumps(owner.value) + "\n" + owner.extra_line
                elif "summary" in argv:
                    raw = json.dumps(owner.summary)
                elif "tests" in argv:
                    raw = json.dumps(owner.tree)
                elif "--entitlements" in argv:
                    raw = plistlib.dumps({"com.apple.developer.team-identifier": "TESTTEAM00",
                                          "com.apple.application-identifier": "TESTTEAM00." + p.APP_ID})
                else:
                    raw = b""
                kwargs["stdout"].write(raw.encode() if isinstance(raw, str) else raw)
                metadata = ("Identifier=" + p.APP_ID + "\nTeamIdentifier=TESTTEAM00\n"
                            "Authority=Apple Development: generated\n") if "--verbose=4" in argv else ""
                kwargs["stderr"].write((metadata + SECRET + "\n").encode())
            def wait(self, timeout):
                owner.commands[-1] += (timeout,)
                if owner.fault and self.argv[0] == "/usr/bin/xcodebuild":
                    raise owner.fault
                return self.returncode
        with patch.object(p.sys, "platform", "darwin"), patch.dict(os.environ, {
                "MEGURI_APP_SIGNING": "team", "TEST_RUNNER_SECRET": SECRET,
                "MEGURI_CLOUD_AGENT_PRIVATE": SECRET}), \
                patch.object(p.h, "setup_launch", side_effect=contextlib.nullcontext), \
                patch.object(p.h, "setup_process", side_effect=Child), \
                patch.object(p.h, "setup_join", side_effect=self.joined.append):
            result = p.run(private, self.xctestrun, self.app)
        self.assertNotIn(SECRET, json.dumps(result))
        return result, private

    def test_fixed_selector_managed_reads_and_safe_limited_wide_misses(self):
        (report, code), private = self.probe()
        self.assertEqual(code, 0)
        self.assertEqual(report["scope"], p.SCOPE)
        self.assertFalse(report["full_suite"])
        self.assertEqual(report["counts"], dict(total=1, executed=1, passed=1, failed=0, skipped=0))
        launch = [call for call in self.commands if call[0][0] == "/usr/bin/xcodebuild"]
        self.assertEqual(len(launch), 1)
        self.assertEqual(launch[0][0], ["/usr/bin/xcodebuild", "test-without-building", "-xctestrun",
                         str(self.xctestrun), "-destination", "platform=macOS,arch=arm64",
                         "-resultBundlePath", str(private / "probe.xcresult"),
                         "-collect-test-diagnostics", "never", "-only-testing:" + p.SELECTOR])
        self.assertEqual(launch[0][2], 600)
        self.assertEqual(len(self.joined), len(self.commands))
        for argv, kwargs, timeout in self.commands:
            self.assertTrue(kwargs["start_new_session"])
            self.assertNotIn("shell", kwargs)
            self.assertFalse(any(k.startswith(("TEST_RUNNER_", "MEGURI_CLOUD_AGENT_")) for k in kwargs["env"]))
            self.assertIn(argv[0], ("/usr/bin/codesign", "/usr/bin/xcodebuild", "/usr/bin/xcrun"))
            if argv[0] == "/usr/bin/xcrun":
                self.assertIn(argv[4], ("summary", "tests"))
                self.assertEqual(timeout, 180)
        self.assertEqual(private.stat().st_mode & 0o777, 0o700)
        self.assertIn(SECRET, (private / "console.stderr").read_text())

    def test_native_test_failure_and_nonzero_exit_stay_red(self):
        self.summary.update(result="Failed", passedTests=0, failedTests=1)
        self.tree["testNodes"][0]["result"] = "Failed"
        self.code = 65
        (report, code), _ = self.probe()
        self.assertEqual(code, 1)
        self.assertEqual(report["counts"]["executed"], 1)
        self.assertEqual(report["category"], "diagnostic-failed")
        self.summary.update(result="Passed", passedTests=1, failedTests=0)
        self.tree["testNodes"][0]["result"] = "Passed"
        (report, code), _ = self.probe()
        self.assertEqual(code, 1)

    def test_sdk_failure_cleanup_seed_fresh_and_drains_are_required(self):
        changes = [("failureCategory", "sdkNotAuthenticated"), ("cleanupFailure", "sdkNotAuthenticated"),
                   ("cleanupSucceeded", False), ("cleanupAttempted", False),
                   ("seed.sawExpectedBytes", False), ("seed.statePresent", False),
                   ("fresh.sawExpectedBytes", False), ("limited.drained", False),
                   ("wide.fetchCompleted", False), ("fresh.callbackErrors", 1),
                   ("seed.unexpectedActivity", True)]
        for field, value in changes:
            with self.subTest(field=field):
                self.value = good_observation()
                node = self.value
                parts = field.split(".")
                for key in parts[:-1]:
                    node = node[key]
                node[parts[-1]] = value
                (report, code), _ = self.probe()
                self.assertEqual(code, 1)
                self.assertEqual(report["category"], "diagnostic-failed")

    def test_closed_schema_rejects_unknown_types_keys_missing_duplicate_and_multiple_lines(self):
        valid = p.PREFIX + json.dumps(good_observation())
        bad = [b"", (valid + "\n" + valid).encode(), ("prefix " + valid).encode(),
               (p.PREFIX + '{"seed":{},"seed":{}}').encode(), (p.PREFIX + "malformed").encode()]
        for field, value in (("failureCategory", SECRET), ("failurePhase", SECRET),
                             ("cleanupFailure", SECRET), ("cleanupSucceeded", 1), ("extra", SECRET)):
            data = good_observation()
            data[field] = value
            bad.append((p.PREFIX + json.dumps(data)).encode())
        for key, value in (("signIns", True), ("callbackErrors", -1), ("drained", "true"), ("extra", SECRET)):
            data = good_observation()
            data["seed"][key] = value
            bad.append((p.PREFIX + json.dumps(data)).encode())
        for raw in bad:
            with self.subTest(raw=raw), self.assertRaises((p.Refused, ValueError, TypeError)):
                p.observation(raw)
        self.extra_line = valid
        (report, code), _ = self.probe()
        self.assertEqual(code, 1)
        self.assertNotIn("observation", report)

    def test_result_requires_one_selected_execution_zero_skips_no_other_cases(self):
        for case in ("zero", "skip", "wrong", "duplicate", "other", "aggregate", "badtype"):
            with self.subTest(case=case):
                summary, tree = copy.deepcopy(self.summary), copy.deepcopy(self.tree)
                if case == "zero": tree["testNodes"] = []
                if case == "skip": summary.update(passedTests=0, skippedTests=1); tree["testNodes"][0]["result"] = "Skipped"
                if case == "wrong": tree["testNodes"][0]["nodeIdentifier"] = "Other/test()"
                if case in ("duplicate", "other"):
                    tree["testNodes"].append(copy.deepcopy(tree["testNodes"][0]))
                    if case == "other": tree["testNodes"][1]["nodeIdentifier"] = "Other/test()"
                if case == "aggregate": summary["passedTests"] = 2
                if case == "badtype": summary["passedTests"] = True
                with self.assertRaises(p.Refused): p.test_counts(summary, tree)

    def test_product_stale_malformed_wrong_identity_or_launch_override_never_launch(self):
        for case in ("stale", "malformed", "identity", "override", "suite"):
            with self.subTest(case=case):
                original = copy.deepcopy(self.plan)
                if case == "malformed": self.plan["TestConfigurations"] = []
                if case == "identity": self.target["TestHostBundleIdentifier"] = "generated.other"
                if case == "override": self.target["EnvironmentVariables"]["MEGURI_FIXTURE"] = SECRET
                if case == "suite": self.target["OnlyTestIdentifiers"] = ["Other"]
                (report, code), _ = self.probe(stale=case == "stale")
                self.assertEqual(code, 1)
                self.assertEqual(report["category"], "product-refused")
                self.assertEqual(self.commands, [])
                self.plan = original
                self.target = self.plan["TestConfigurations"][0]["TestTargets"][0]

    def test_signature_refusal_never_launches(self):
        with patch.object(p, "native", side_effect=p.Refused()):
            (report, code), _ = self.probe()
        self.assertEqual(code, 1)
        self.assertEqual(report["category"], "product-refused")
        self.assertEqual(self.commands, [])

    def test_timeout_failure_and_cancellation_always_join_original(self):
        for failure in (subprocess.TimeoutExpired("generated", 600), p.h.v.Refused("setup-cancelled"), KeyboardInterrupt()):
            with self.subTest(failure=type(failure)):
                self.commands, self.joined = [], []
                self.fault = failure
                (report, code), _ = self.probe()
                self.assertEqual(code, 1)
                self.assertEqual(report["category"], "execution-refused")
                self.assertEqual(len(self.commands), len(self.joined))
                self.assertEqual(self.joined[-1].argv[0], "/usr/bin/xcodebuild")


class EntryTests(unittest.TestCase):
    # Source the real entry, stubbing its existing MG/native boundaries only.
    SCRIPT = r'''
mg_job() { :; }
source "$REPO/tools/mg.d/cloud-fetch-probe.sh"
mg_failure() { printf '%s\n' "$1" >>"$MG_STATE/failures"; }
mg_status() { printf '%s\n' "$1" >"$MG_STATE/status"; }
mg_message() { :; }
mg_detail() { printf '%s\n' "$1=$2" >>"$MG_STATE/details"; }
mg_count() { printf '%s=%s\n' "$1" "$2" >>"$MG_STATE/counts"; }
uname() { printf '%s\n' "${OS:-Darwin}"; }
mg_host_lease() { echo lease >>"$CALLS"; [ "$FAIL" != lease ]; }
mg_app_mac_guard() {
  echo guard >>"$CALLS"
  echo generated-private-native-description
  mg_detail private '"generated-private-native-description"'
  n=$((n+1)); [ "$FAIL" != "guard$n" ]
}
mg_app_input_digest() {
  echo digest >>"$CALLS"
  if { [ "$FAIL" = drift ] && [ -f "$MG_RUN_DIR/built" ]; } || [ -f "$MG_RUN_DIR/drift-run" ]; then printf '%064d\n' 2; else printf '%064d\n' 1; fi
}
mg_app_build() { echo "build $*" >>"$CALLS"; touch "$MG_RUN_DIR/built"; mg_detail build '"generated-private-native-description"'; [ "$FAIL" != build ]; }
mg__vm_xctestrun() { echo "lookup $*" >>"$CALLS"; echo generated-local-product; }
mg_app_products() { echo generated-local-products; }
python3() {
  echo helper >>"$CALLS"
  [ "$FAIL" != drift-run ] || touch "$4/drift-run"
  echo '{"scope":"HOST-CK-SDK-DIAGNOSTIC-ONLY","full_suite":false,"category":"observed","counts":{"total":1,"executed":1,"passed":1,"failed":0,"skipped":0}}'
}
mg_sim_begin() { exit 97; }
mg_sim_take() { exit 97; }
mg_app_mac_agent_profile_check() { exit 97; }
n=0
mg_cloud_fetch_probe_host "$@"
'''

    def entry(self, args=(), **env):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            run, state = root / "run", root / "state"
            run.mkdir(); state.mkdir()
            environment = dict(os.environ, REPO=str(ROOT), MG_ROOT=str(ROOT), MG_TOOLS=str(ROOT / "tools"),
                               MG_RUN_DIR=str(run), MG_STATE=str(state), CALLS=str(root / "calls"), FAIL="", OS="Darwin",
                               MEGURI_APP_SIGNING="team")
            environment.update(env)
            result = subprocess.run(["/bin/bash", "-c", self.SCRIPT, "entry", *args], env=environment,
                                    capture_output=True, text=True, check=True)
            public = "\n".join(path.read_text() for path in state.iterdir()) + result.stdout + result.stderr
            calls = (root / "calls").read_text() if (root / "calls").exists() else ""
            if (run / "private").exists():
                self.assertEqual((run / "private").stat().st_mode & 0o777, 0o700)
                if "guard" in calls:
                    self.assertIn(SECRET, (run / "private/workflow.log").read_text())
            self.assertNotIn(SECRET, public)
            return (state / "status").read_text().strip(), calls, public

    def test_zeroargs_explicit_team_and_darwin_before_work(self):
        for args, env in ((["--suite", "Other/test"], {}), ([""], {}), ([], {"MEGURI_APP_SIGNING": ""}),
                          ([], {"MEGURI_APP_SIGNING": "adhoc"}), ([], {"MEGURI_APP_SIGNING": "team adhoc"}),
                          ([], {"OS": "Linux"})):
            status, calls, _ = self.entry(args, **env)
            self.assertEqual(status, "error")
            self.assertEqual(calls, "")

    def test_lease_guards_build_and_source_order_with_private_native_state(self):
        status, calls, public = self.entry()
        self.assertEqual(status, "warn")
        self.assertEqual(calls.splitlines(), ["lease", "guard", "digest", "build mac Debug Snapshot", "digest",
                                              "lookup Snapshot macosx Debug", "guard", "helper", "digest"])
        self.assertIn('full_suite=false', public)
        for phase, last in (("lease", "lease"), ("guard1", "guard"), ("build", "build mac Debug Snapshot"),
                            ("guard2", "guard"), ("drift", "digest")):
            # lease refusal precedes the guard's private-console sentinel.
            status, calls, public = self.entry(FAIL=phase)
            self.assertEqual(status, "fail")
            self.assertEqual(calls.splitlines()[-1], last)
            self.assertNotIn("helper", calls)
        status, calls, public = self.entry(FAIL="drift-run")
        self.assertEqual(status, "fail")
        self.assertIn("helper", calls)
        self.assertIn("source-binding", public)


if __name__ == "__main__":
    unittest.main()
