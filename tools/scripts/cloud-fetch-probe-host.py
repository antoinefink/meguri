#!/usr/bin/env python3
"""Internal fixed host SDK diagnostic. All native evidence stays private."""
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import re
import signal
import stat
import subprocess
import sys

_spec = importlib.util.spec_from_file_location(
    "fetch_probe_managed", Path(__file__).with_name("cloud-agent-handoff.py"))
h = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = h
_spec.loader.exec_module(h)

SCOPE = "HOST-CK-SDK-DIAGNOSTIC-ONLY"
METHOD = "NativeCloudFetchScopeProbeTests/observeSavedStateFetchScopes()"
SELECTOR = "MeguriSnapshotTests/" + METHOD
APP_ID = "com.antoinefink.meguri.dev"
PREFIX = "MG_SDK_SCOPE_PROBE "
STAGES = ("seed", "limited", "wide", "fresh")
PHASES = {"none", "configuration", "create", "inventory", "save1", "seed", "save2",
          "limited", "wide", "fresh", "cleanup"}
ERRORS = {"none", "configuration", "cancelled", "sdkFailure", "invalidResult", "timeout",
          "missingExpectedBytes", "missingState", "incompleteFetch", "unexpectedActivity",
          "sdkNotAuthenticated", "sdkPermissionFailure", "sdkMissingEntitlement",
          "sdkBadContainer", "sdkBadDatabase", "sdkNetworkUnavailable", "sdkNetworkFailure",
          "sdkServiceUnavailable", "sdkRequestRateLimited", "sdkServerRejectedRequest",
          "sdkPartialFailure", "sdkZoneNotFound", "sdkUnknownItem", "sdkInternalError",
          "sdkIncompatibleVersion"}
COUNTERS = {"willFetch", "didFetch", "databaseCallbacks", "willFetchZone", "zoneCallbacks",
            "didFetchZone", "generatedRecordModifications", "fetchOptionsCallbacks",
            "batchRequests", "callbackErrors", "signIns"}
FLAGS = {"sawExpectedBytes", "statePresent", "unexpectedActivity", "fetchCompleted", "drained"}
CAP = 16 * 1024 * 1024


class Refused(Exception):
    pass


def need(value):
    if not value:
        raise Refused()


def strict(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            need(key not in result)
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(Refused()))


def managed(argv, private, label, timeout):
    """Reuse the original-child join authority, including cancellation fences."""
    environment = {k: v for k, v in os.environ.items()
                   if not k.startswith(("TEST_RUNNER_", "MEGURI_CLOUD_AGENT_"))}
    child = None
    with (private / (label + ".stdout")).open("xb") as out, \
            (private / (label + ".stderr")).open("xb") as err:
        try:
            with h.setup_launch():
                child = h.setup_process(argv, stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                        env=environment, start_new_session=True)
            child.wait(timeout=timeout)
        finally:
            if child is not None:
                h.setup_join(child)
    return child.returncode


def read(path):
    info = path.lstat()
    need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= CAP)
    return path.read_bytes()


def native(argv, private, label):
    need(managed(argv, private, label, 180) == 0)
    return read(private / (label + ".stdout")), read(private / (label + ".stderr"))


def product(xctestrun, app, private):
    # The lookup is local, even though the shared shell helper is VM-named.
    products = app.parent.parent
    need(app.name == "Meguri.app" and app.parent.name == "Debug" and
         products.name == "Products" and xctestrun.parent == products and
         re.fullmatch(r"Meguri_Snapshot_macosx[^/]*\.xctestrun", xctestrun.name))
    for path in (private, xctestrun, app):
        need(path.is_absolute() and path.resolve() == path)
    need(xctestrun.stat().st_mtime_ns >= (private / "build-start").stat().st_mtime_ns)
    plan = plistlib.loads(read(xctestrun))
    need(plan.get("__xctestrun_metadata__", {}).get("FormatVersion") == 2)
    configs = plan.get("TestConfigurations")
    need(isinstance(configs, list) and len(configs) == 1 and isinstance(configs[0], dict))
    targets = configs[0].get("TestTargets")
    need(isinstance(targets, list) and len(targets) == 1 and isinstance(targets[0], dict))
    target = targets[0]
    need(target.get("BlueprintName") == "MeguriSnapshotTests" and
         target.get("ProductModuleName") == "MeguriSnapshotTests" and
         target.get("TestHostBundleIdentifier") == APP_ID and
         target.get("IsAppHostedTestBundle") is True and
         target.get("IsUITestBundle", False) is False and
         target.get("TestHostPath") == "__TESTROOT__/Debug/Meguri.app" and
         target.get("TestBundlePath") == "__TESTHOST__/Contents/PlugIns/MeguriSnapshotTests.xctest")
    # Preserve Xcode's native generated injection and Snapshot exclusions. No
    # edited xctestrun, fixture, selection or launch-environment overrides.
    for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
        environment = target.get(key, {})
        need(isinstance(environment, dict) and
             all(isinstance(k, str) and isinstance(v, str) and
                 not k.startswith(("TEST_RUNNER_", "MEGURI_")) for k, v in environment.items()))
        if "XCODE_TEST_PLAN_NAME" in environment:
            need(environment["XCODE_TEST_PLAN_NAME"] == "Snapshot")
    need(not target.get("CommandLineArguments") and not target.get("UITargetAppCommandLineArguments") and
         not target.get("UITargetAppEnvironmentVariables") and not target.get("OnlyTestIdentifiers"))
    skipped = target.get("SkipTestIdentifiers")
    need(isinstance(skipped, list) and len(skipped) == 3 and
         all(isinstance(item, str) for item in skipped) and
         set(skipped) == {"CloudKitSoakTests", "CloudKitAccountSetupTests", "NativeCloudFetchScopeProbeTests"})
    info = plistlib.loads(read(app / "Contents/Info.plist"))
    need(info.get("CFBundleIdentifier") == APP_ID and info.get("CFBundleExecutable") == "Meguri" and
         info.get("MeguriCardInspectionBuild") == "debug" and info.get("LSUIElement") is True)
    need((app / "Contents/PlugIns/MeguriSnapshotTests.xctest").is_dir())
    native(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)], private, "signature")
    _, description = native(["/usr/bin/codesign", "-d", "--verbose=4", str(app)], private, "identity")
    attributes = {}
    for record in description.decode().splitlines():
        field, separator, content = record.partition("=")
        if separator:
            attributes.setdefault(field, []).append(content)
    signing_groups = attributes.get("TeamIdentifier", [])
    need(len(signing_groups) == 1)
    signing_group = signing_groups[0]
    need(re.fullmatch(r"[A-Z0-9]{10}", signing_group))
    need("adhoc" not in attributes.get("Signature", []))
    need(APP_ID in attributes.get("Identifier", []))
    need(any(authority.startswith("Apple Development:")
             for authority in attributes.get("Authority", [])))
    encoded_grants, _ = native(
        ["/usr/bin/codesign", "-d", "--entitlements", "-", "--xml", str(app)], private, "grants")
    signed_grants = plistlib.loads(encoded_grants)
    for entitlement, expected in (
            ("com.apple.developer.team-identifier", signing_group),
            ("com.apple.application-identifier", ".".join((signing_group, APP_ID)))):
        need(signed_grants.get(entitlement) == expected)


def observation(raw):
    lines = raw.decode().splitlines()
    candidates = [line for line in lines if "MG_SDK_SCOPE_PROBE" in line]
    need(len(candidates) == 1 and candidates[0].startswith(PREFIX))
    value = strict(candidates[0][len(PREFIX):])
    need(isinstance(value, dict) and set(value) == set(STAGES) | {
        "failurePhase", "failureCategory", "cleanupAttempted", "cleanupSucceeded", "cleanupFailure"})
    for key in STAGES:
        counts = value[key]
        need(isinstance(counts, dict) and set(counts) == COUNTERS | FLAGS)
        need(all(type(counts[k]) is int and 0 <= counts[k] <= sys.maxsize for k in COUNTERS))
        need(all(type(counts[k]) is bool for k in FLAGS))
    need(type(value["cleanupAttempted"]) is bool and type(value["cleanupSucceeded"]) is bool)
    need(value["failurePhase"] in PHASES and value["failureCategory"] in ERRORS and
         value["cleanupFailure"] in ERRORS)
    return value


def test_counts(summary, tree):
    need(isinstance(summary, dict) and isinstance(tree, dict) and isinstance(tree.get("testNodes"), list))
    for key in ("passedTests", "failedTests", "skippedTests"):
        need(type(summary.get(key)) is int and 0 <= summary[key] <= sys.maxsize)
    cases = []
    def walk(value):
        if isinstance(value, dict):
            if value.get("nodeType") == "Test Case":
                cases.append(value)
            for child in value.values():
                walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)
    walk(tree["testNodes"])
    need(len(cases) == 1 and cases[0].get("nodeIdentifier") in (METHOD, SELECTOR, METHOD.removesuffix("()"), SELECTOR.removesuffix("()")) and
         cases[0].get("result") in ("Passed", "Failed") and summary["skippedTests"] == 0)
    passed = int(cases[0]["result"] == "Passed")
    need(summary["passedTests"] == passed and summary["failedTests"] == 1 - passed and
         summary.get("result") == cases[0]["result"])
    return {"total": 1, "executed": 1, "passed": passed, "failed": 1 - passed, "skipped": 0}


def diagnostic_success(value):
    return (value["failurePhase"] == "none" and value["failureCategory"] == "none" and
            value["cleanupFailure"] == "none" and value["cleanupAttempted"] and value["cleanupSucceeded"] and
            value["seed"]["sawExpectedBytes"] and value["seed"]["statePresent"] and
            value["fresh"]["sawExpectedBytes"] and
            all(value[k]["fetchCompleted"] and value[k]["drained"] and
                value[k]["callbackErrors"] == 0 and not value[k]["unexpectedActivity"] for k in STAGES))


def run(private, xctestrun, app):
    counts = {key: 0 for key in ("total", "executed", "passed", "failed", "skipped")}
    report = {"scope": SCOPE, "full_suite": False, "category": "product-refused", "counts": counts}
    try:
        need(sys.platform == "darwin" and os.environ.get("MEGURI_APP_SIGNING") == "team")
        need(stat.S_IMODE(private.lstat().st_mode) == 0o700 and private.stat().st_uid == os.getuid())
        product(xctestrun, app, private)
        report["category"] = "execution-refused"
        rc = managed(["/usr/bin/xcodebuild", "test-without-building", "-xctestrun", str(xctestrun),
                      "-destination", "platform=macOS,arch=arm64", "-resultBundlePath", str(private / "probe.xcresult"),
                      "-collect-test-diagnostics", "never", "-only-testing:" + SELECTOR], private, "console", 600)
        report["category"] = "result-refused"
        summary, _ = native(["/usr/bin/xcrun", "xcresulttool", "get", "test-results", "summary", "--path",
                             str(private / "probe.xcresult")], private, "summary")
        tree, _ = native(["/usr/bin/xcrun", "xcresulttool", "get", "test-results", "tests", "--path",
                          str(private / "probe.xcresult")], private, "tests")
        report["counts"] = test_counts(strict(summary), strict(tree))
        value = observation(read(private / "console.stdout") + b"\n" + read(private / "console.stderr"))
        report["observation"] = value
        success = rc == 0 and report["counts"]["passed"] == 1 and diagnostic_success(value)
        report["category"] = "observed" if success else "diagnostic-failed"
        return report, 0 if success else 1
    except (Exception, KeyboardInterrupt):
        # No exception messages, raw native errors, IDs or paths cross this boundary.
        return report, 1


def main():
    os.umask(0o077)
    if len(sys.argv) != 4:
        return 1
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(number, h.setup_cancel)
    report, code = run(*(Path(arg) for arg in sys.argv[1:]))
    print(json.dumps(report, sort_keys=True))
    return code


if __name__ == "__main__":
    sys.exit(main())
