#!/usr/bin/env python3
"""Fake generated plists and build commands, exercised by mg selftest."""
import copy
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
VALIDATOR = TOOLS / "scripts/performance-test-policy.py"
FLAGS = ("UITargetAppMainThreadCheckerEnabled",
         "UITargetAppPerformanceAntipatternCheckerEnabled")


def target():
    return dict(IsUITestBundle=True, **{key: False for key in FLAGS})


def document(targets):
    return {"TestConfigurations": [{"TestTargets": targets}],
            "__xctestrun_metadata__": {"FormatVersion": 2}}


class Policy(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="policy quote's space ")
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / "generated.xctestrun"

    def check(self, data, passes=False, fmt=plistlib.FMT_XML):
        if data is not None:
            self.path.write_bytes(plistlib.dumps(data, fmt=fmt))
        before = self.path.read_bytes() if self.path.exists() else None
        result = subprocess.run([sys.executable, str(VALIDATOR), str(self.path)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0 if passes else 1, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assertLess(len(result.stdout), 1200)
        self.assertEqual(before, self.path.read_bytes() if self.path.exists() else None)
        return json.loads(result.stdout)

    def test_valid(self):
        for fmt in (plistlib.FMT_XML, plistlib.FMT_BINARY):
            report = self.check(document([target(), target()]), True, fmt)
            self.assertEqual(report["ui_targets"], 2)
        self.check({"MeguriUITests": target(), "__xctestrun_metadata__": {"FormatVersion": 1}}, True)
        ui = target()
        del ui["IsUITestBundle"]
        ui["UITargetAppPath"] = "__TESTROOT__/Profile/Meguri.app"
        self.check(document([ui]), True)
        # A second generated configuration is checked too.
        data = document([target()])
        data["TestConfigurations"].append({"TestTargets": [dict(target(), **{FLAGS[1]: True})]})
        self.check(data)

    def test_flags(self):
        for key in FLAGS:
            for value in ("missing", True, 0, "NO"):
                with self.subTest(key=key, value=value):
                    ui = target()
                    if value == "missing":
                        del ui[key]
                    else:
                        ui[key] = value
                    self.check(document([ui]))

    def test_injection(self):
        for environment in ("EnvironmentVariables", "TestingEnvironmentVariables", "UITargetAppEnvironmentVariables"):
            for library, count in (("__DEVELOPERUSRLIB__/libMainThreadChecker.dylib", "main_thread_checker_injections"),
                                   ("/usr/lib/libRPAC.dylib", "performance_antipattern_checker_injections")):
                ui = dict(target(), **{environment: {"DYLD_INSERT_LIBRARIES": library}})
                self.assertEqual(self.check(document([ui]))[count], 1)
        # Also reject injection on a non-UI target in the selected file.
        self.check(document([target(), {"TestingEnvironmentVariables": {"DYLD_INSERT_LIBRARIES": "libRPAC.dylib"}}]))

    def test_unavailable_and_malformed(self):
        self.assertEqual(self.check(None)["reason"], "unavailable")
        self.path.write_bytes(b"not a plist")
        self.assertEqual(self.check(None)["reason"], "malformed")
        for data in ([], {"TestConfigurations": "wrong"}, {"TestConfigurations": [{}]},
                     document(["wrong"]), document([dict(target(), EnvironmentVariables=[])]),
                     document([dict(target(), IsUITestBundle="YES")])):
            self.assertEqual(self.check(data)["reason"], "malformed")

    def test_no_ui_targets(self):
        for data in ({}, document([]), document([{"IsUITestBundle": False}])):
            self.assertEqual(self.check(data)["ui_targets"], 0)

    def test_bounded_metadata(self):
        ui = dict(target(), EnvironmentVariables={"SECRET_TOKEN": "secret-never-report-me" * 10000})
        data = document([copy.deepcopy(ui) for _ in range(100)])
        report = self.check(data, True)
        self.assertNotIn("secret", json.dumps(report).lower())
        self.assertNotIn("TOKEN", json.dumps(report))
        self.assertEqual(report["environment_dictionaries"], 100)


class BuildGuard(unittest.TestCase):
    def run_build(self, platform="mac", config="Profile", plan="Performance", valid=True,
                  available=True, build_rc=0):
        with tempfile.TemporaryDirectory(prefix="build policy quote's space ") as scratch:
            root = Path(scratch)
            fixture = root / "fixture.plist"
            ui = target()
            ui["TestBundlePath"] = "__TESTROOT__/" + config + ("-iphonesimulator" if platform != "mac" else "") + "/MeguriUITests.xctest"
            if not valid:
                del ui[FLAGS[0]]
            fixture.write_bytes(plistlib.dumps(document([ui])))
            # Exercise the existing selector without sourcing VM setup.
            source = (TOOLS / "mg.d/mac-vm.sh").read_text()
            selector = source.split("mg__vm_xctestrun() {", 1)[1].split("\n}", 1)[0]
            script = "mg__vm_xctestrun() {" + selector + "\n}\n" + r'''
mg_job() { :; }
. "$MG_TOOLS/mg.d/app.sh"
MG_RUN_DIR="$MG_ROOT/run"
MG_JOB=fake-build
mkdir -p "$MG_RUN_DIR"
mg_word_in() { return 0; }
mg_lint_ui_first() { return 0; }
mg__xcframework() { return 0; }
mg_artifact() { :; }
mg_detail() { printf 'detail %s %s\n' "$1" "$2"; }
mg_failure() { printf 'failure %s\n' "$1"; }
mg_message() { :; }
mg_status() { printf 'status %s\n' "$1"; }
xcrun() { printf '{"errorCount":0,"warningCount":0}'; }
xcodebuild() {
  printf '%s\n' "$*" >"$MG_ROOT/build-args"
  [ "$FAKE_BUILD_RC" = 0 ] || return "$FAKE_BUILD_RC"
  [ "$FAKE_AVAILABLE" = yes ] || return 0
  local sdk=macosx
  [ "$FAKE_PLATFORM" = mac ] || sdk=iphonesimulator
  mkdir -p "$MG_ROOT/$MG_APP_DD/Build/Products"
  cp "$MG_ROOT/fixture.plist" "$MG_ROOT/$MG_APP_DD/Build/Products/Meguri-Profile_Performance_${sdk}27.xctestrun"
}
mg_app_build "$@" || exit $?
echo CONSUMER_ALLOWED
'''
            env = dict(os.environ, MG_ROOT=str(root), MG_TOOLS=str(TOOLS),
                       FAKE_PLATFORM=platform, FAKE_BUILD_RC=str(build_rc),
                       FAKE_AVAILABLE="yes" if available else "no",
                       MG_APP_TEST_SCHEME="WrongAmbientScheme")
            result = subprocess.run(["bash", "-u", "-c", script, "fake", platform, config, plan,
                                     "Meguri-Profile", "CODE_SIGN_INJECT_BASE_ENTITLEMENTS=YES"],
                                    env=env, capture_output=True, text=True)
            args = (root / "build-args").read_text()
            self.assertIn("-configuration " + config, args)
            self.assertIn("CODE_SIGN_INJECT_BASE_ENTITLEMENTS=YES", args)
            return result

    def test_profile_guard(self):
        for platform in ("mac", "ios-sim"):
            result = self.run_build(platform=platform)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertIn("performance_test_policy", result.stdout)
            self.assertIn("CONSUMER_ALLOWED", result.stdout)
            for options in ({"valid": False}, {"available": False}):
                result = self.run_build(platform=platform, **options)
                self.assertEqual(result.returncode, 1, result.stderr + result.stdout)
                self.assertIn("failure performance-test-policy", result.stdout)
                self.assertNotIn("CONSUMER_ALLOWED", result.stdout)

    def test_ordinary_builds_unaffected(self):
        for config, plan in (("Debug", "UI"), ("Debug", "Performance"), ("Profile", "UI")):
            result = self.run_build(config=config, plan=plan, available=False)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertNotIn("performance_test_policy", result.stdout)
            self.assertIn("CONSUMER_ALLOWED", result.stdout)

    def test_failed_build_stops_before_guard(self):
        result = self.run_build(build_rc=7)
        self.assertEqual(result.returncode, 1)
        self.assertIn("failure build", result.stdout)
        self.assertNotIn("performance_test_policy", result.stdout)
        self.assertNotIn("CONSUMER_ALLOWED", result.stdout)


if __name__ == "__main__":
    unittest.main()
