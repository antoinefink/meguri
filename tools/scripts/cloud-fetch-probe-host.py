#!/usr/bin/env python3
"""Internal fixed host SDK diagnostic. All native evidence stays private."""
import importlib.util
import base64
import hashlib
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
STAGES = ("seed", "limited", "wide", "priority", "rebuilt", "fresh")
PHASES = {"none", "configuration", "create", "inventory", "save1", "seed", "save2",
          "limited", "wide", "priority", "rebuilt", "fresh", "cleanup"}
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
COMPLETION = b"HOST-CK-SDK-ORIGINALS-JOINED-v1\n"
READ_METHOD = "NativeCloudFetchScopeProbeTests/observeAcknowledgedRecords()"
READ_SELECTOR = "MeguriSnapshotTests/" + READ_METHOD
READ_PREFIX = "MG_SDK_RECORD_READ "
INPUT_CAP = 64 * 1024
INPUT_NAME = "acknowledged-records.json"


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


def record_input(raw):
    need(0 < len(raw) <= INPUT_CAP)
    value = strict(raw)
    need(type(value) is dict and set(value) == {"version", "collectionZone", "recordType", "records"})
    need(type(value["version"]) is int and value["version"] == 1 and value["recordType"] == "MGEntity")
    need(type(value["collectionZone"]) is str and
         re.fullmatch(r"c-[a-z2-7]{25}[aeimquy4]", value["collectionZone"]))
    records = value["records"]
    need(type(records) is list and 1 <= len(records) <= 8)
    names = set()
    for item in records:
        need(type(item) is dict and set(item) == {"name", "encrypted", "systemFields"})
        name = item["name"]
        need(type(name) is str and 0 < len(name.encode("utf-8")) <= 255 and
             not any(ord(c) < 32 or ord(c) == 127 for c in name) and name not in names)
        names.add(name)
        for key in ("encrypted", "systemFields"):
            encoded = item[key]
            need(type(encoded) is str)
            decoded = base64.b64decode(encoded, validate=True)
            need(0 < len(decoded) <= INPUT_CAP and base64.b64encode(decoded).decode("ascii") == encoded)
    return value


def directory(path):
    """Walk an absolute binding without following any directory symlink."""
    need(path.is_absolute() and ".." not in path.parts)
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parts[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
            info = os.fstat(fd)
            need(info.st_uid in (0, os.getuid()) and
                 (not info.st_mode & 0o022 or (info.st_uid == 0 and info.st_mode & stat.S_ISVTX)))
        return fd
    except BaseException:
        os.close(fd)
        raise


def binding(info):
    # Reads may advance atime. Content/identity metadata must remain exact.
    return (info.st_dev, info.st_ino, info.st_uid, info.st_mode, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def bound_bytes(fd, cap=INPUT_CAP):
    before = os.fstat(fd)
    need(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid() and
         before.st_nlink == 1 and 0 < before.st_size <= cap)
    os.lseek(fd, 0, os.SEEK_SET)
    raw = os.read(fd, cap + 1)
    after = os.fstat(fd)
    need(binding(before) == binding(after) and len(raw) == before.st_size)
    return raw, before


class RecordInput:
    """One owned app-private file. Never adopt, replace or clean user files."""
    def __init__(self):
        self.parent = self.folder = self.file = None
        self.created_folder = self.created_file = False

    def install(self, source, private, build, support):
        need(source.is_absolute() and source != build and build in source.parents)
        parent = directory(source.parent)
        try:
            fd = os.open(source.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
            try:
                raw, self.source_info = bound_bytes(fd)
            finally:
                os.close(fd)
        finally:
            os.close(parent)
        value = record_input(raw)
        self.source, self.raw = source, raw
        self.support = support
        self.parent = directory(support)
        self.parent_info = os.fstat(self.parent)
        need(os.fstat(self.parent).st_uid == os.getuid())
        # Refuse even an empty preexisting diagnostic directory: no adoption.
        os.mkdir("MeguriDiagnostics", 0o700, dir_fd=self.parent)
        self.created_folder = True
        self.folder = os.open("MeguriDiagnostics", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                              dir_fd=self.parent)
        self.folder_info = os.fstat(self.folder)
        self.file = os.open(INPUT_NAME, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                            0o600, dir_fd=self.folder)
        self.created_file = True
        os.fchmod(self.file, 0o600)
        with os.fdopen(os.dup(self.file), "wb") as out:
            out.write(raw)
            out.flush()
            os.fsync(out.fileno())
        _, self.file_info = bound_bytes(self.file)
        with (private / "record-input.json").open("xb") as evidence:
            evidence.write(raw)
        with (private / "record-input-binding.json").open("x") as evidence:
            json.dump({"sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw),
                       "records": len(value["records"])}, evidence)
        self.check()
        return len(value["records"])

    def check(self):
        parent = directory(self.support)
        try:
            info = os.fstat(parent)
            need((info.st_dev, info.st_ino, info.st_mode, info.st_uid) ==
                 (self.parent_info.st_dev, self.parent_info.st_ino, self.parent_info.st_mode, self.parent_info.st_uid))
        finally:
            os.close(parent)
        info = os.stat("MeguriDiagnostics", dir_fd=self.parent, follow_symlinks=False)
        need((info.st_dev, info.st_ino, info.st_mode) ==
             (self.folder_info.st_dev, self.folder_info.st_ino, self.folder_info.st_mode))
        info = os.stat(INPUT_NAME, dir_fd=self.folder, follow_symlinks=False)
        need(binding(info) == binding(self.file_info) and stat.S_IMODE(info.st_mode) == 0o600)
        raw, info = bound_bytes(self.file)
        need(binding(info) == binding(self.file_info) and hashlib.sha256(raw).digest() == hashlib.sha256(self.raw).digest())
        parent = directory(self.source.parent)
        try:
            fd = os.open(self.source.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
            try:
                raw, info = bound_bytes(fd)
                need(binding(info) == binding(self.source_info) and raw == self.raw)
            finally:
                os.close(fd)
        finally:
            os.close(parent)

    def cleanup(self):
        # Invoked only after the native originals joined. A changed binding is
        # preserved, never unlinked by path on the assumption that we own it.
        if self.created_file:
            self.check()
            os.unlink(INPUT_NAME, dir_fd=self.folder)
        if self.created_folder:
            os.rmdir("MeguriDiagnostics", dir_fd=self.parent)

    def close(self):
        for fd in (self.file, self.folder, self.parent):
            if fd is not None:
                os.close(fd)


class Joins:
    def __init__(self):
        self.pending = 0
        self.uncertain = False

    def safe(self):
        return self.pending == 0 and not self.uncertain


def managed(argv, private, label, timeout, joins):
    """Reuse the original-child join authority, including cancellation fences."""
    need(joins.safe())
    environment = {k: v for k, v in os.environ.items()
                   if not k.startswith(("TEST_RUNNER_", "MEGURI_CLOUD_AGENT_"))}
    child = None
    with (private / (label + ".stdout")).open("xb") as out, \
            (private / (label + ".stderr")).open("xb") as err:
        try:
            with h.setup_launch():
                joins.pending += 1
                child = h.setup_process(argv, stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                        env=environment, start_new_session=True)
            child.wait(timeout=timeout)
        finally:
            if child is not None:
                try:
                    h.setup_join(child)
                    need(type(child.returncode) is int)
                    joins.pending -= 1
                except BaseException:
                    joins.uncertain = True
                    raise
    return child.returncode


def read(path):
    info = path.lstat()
    need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= CAP)
    return path.read_bytes()


def native(argv, private, label, joins):
    need(managed(argv, private, label, 180, joins) == 0)
    return read(private / (label + ".stdout")), read(private / (label + ".stderr"))


def product(xctestrun, app, private, joins, record_read=False):
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
    native(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)], private, "signature", joins)
    _, description = native(["/usr/bin/codesign", "-d", "--verbose=4", str(app)], private, "identity", joins)
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
        ["/usr/bin/codesign", "-d", "--entitlements", "-", "--xml", str(app)], private, "grants", joins)
    signed_grants = plistlib.loads(encoded_grants)
    for entitlement, expected in (
            ("com.apple.developer.team-identifier", signing_group),
            ("com.apple.application-identifier", ".".join((signing_group, APP_ID)))):
        need(signed_grants.get(entitlement) == expected)
    if record_read:
        container = info.get("MeguriCloudContainer")
        need(type(container) is str and container.startswith("iCloud.") and
             info.get("MeguriCloudEnvironment") == "development" and
             info.get("MeguriCloudSchemaConfigured") is True and
             signed_grants.get("com.apple.developer.icloud-container-environment") == "Development" and
             signed_grants.get("com.apple.developer.icloud-container-identifiers") == [container] and
             "CloudKit" in signed_grants.get("com.apple.developer.icloud-services", []))


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


def test_counts(summary, tree, record_read=False):
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
    method, selector = (READ_METHOD, READ_SELECTOR) if record_read else (METHOD, SELECTOR)
    need(len(cases) == 1 and cases[0].get("nodeIdentifier") in (method, selector, method.removesuffix("()"), selector.removesuffix("()")) and
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


def record_observation(raw, expected):
    lines = [line for line in raw.decode().splitlines() if "MG_SDK_RECORD_READ" in line]
    need(len(lines) == 1 and lines[0].startswith(READ_PREFIX) and b"MG_SDK_SCOPE_PROBE" not in raw)
    value = strict(lines[0][len(READ_PREFIX):])
    keys = {"expected", "found", "exactByteMatch", "missing", "different", "readError"}
    need(type(value) is dict and set(value) == keys | {"completed", "failureCategory", "terminalError", "slots"})
    need(type(expected) is int and 1 <= expected <= 8 and value["expected"] == expected)
    need(all(type(value[k]) is int and 0 <= value[k] <= expected for k in keys))
    need(type(value["completed"]) is bool and value["failureCategory"] in ERRORS and value["terminalError"] in ERRORS)
    slots = value["slots"]
    need(type(slots) is list and len(slots) == expected)
    totals = dict.fromkeys(keys - {"expected"}, 0)
    for slot in slots:
        need(type(slot) is dict and set(slot) == {"found", "bytesMatch", "changeTagMatch", "status", "error"})
        need(all(type(slot[k]) is bool for k in ("found", "bytesMatch", "changeTagMatch")))
        need(slot["status"] in {"match", "missing", "different", "readError"} and slot["error"] in ERRORS)
        found = slot["status"] in {"match", "different"}
        need(slot["found"] == found)
        if found:
            need(slot["error"] == "none" and
                 (slot["status"] == "match") == (slot["bytesMatch"] and slot["changeTagMatch"]))
        else:
            need(not slot["bytesMatch"] and not slot["changeTagMatch"] and slot["error"] != "none")
            need((slot["error"] == "sdkUnknownItem") == (slot["status"] == "missing"))
        totals["found"] += int(found)
        totals["exactByteMatch"] += int(slot["bytesMatch"])
        for status in ("missing", "different", "readError"):
            totals[status] += int(slot["status"] == status)
    need(all(value[k] == count for k, count in totals.items()))
    return value


def run(private, xctestrun, app, record_source=None):
    joins = Joins()
    record_read = record_source is not None
    supplied = RecordInput() if record_read else None
    counts = {key: 0 for key in ("total", "executed", "passed", "failed", "skipped")}
    report = {"scope": SCOPE, "full_suite": False, "category": "product-refused", "counts": counts}
    try:
        need(sys.platform == "darwin" and os.environ.get("MEGURI_APP_SIGNING") == "team")
        need(stat.S_IMODE(private.lstat().st_mode) == 0o700 and private.stat().st_uid == os.getuid())
        product(xctestrun, app, private, joins, record_read)
        expected = None
        if record_read:
            report["category"] = "input-refused"
            build = Path(os.environ["MG_ROOT"]) / "build"
            support = Path.home() / "Library/Containers" / APP_ID / "Data/Library/Application Support"
            expected = supplied.install(record_source, private, build, support)
        report["category"] = "execution-refused"
        rc = managed(["/usr/bin/xcodebuild", "test-without-building", "-xctestrun", str(xctestrun),
                      "-destination", "platform=macOS,arch=arm64", "-resultBundlePath", str(private / "probe.xcresult"),
                      "-collect-test-diagnostics", "never", "-only-testing:" + (READ_SELECTOR if record_read else SELECTOR)], private, "console", 600, joins)
        if supplied:
            supplied.check()
        report["category"] = "result-refused"
        summary, _ = native(["/usr/bin/xcrun", "xcresulttool", "get", "test-results", "summary", "--path",
                             str(private / "probe.xcresult")], private, "summary", joins)
        tree, _ = native(["/usr/bin/xcrun", "xcresulttool", "get", "test-results", "tests", "--path",
                          str(private / "probe.xcresult")], private, "tests", joins)
        report["counts"] = test_counts(strict(summary), strict(tree), record_read)
        raw = read(private / "console.stdout") + b"\n" + read(private / "console.stderr")
        value = record_observation(raw, expected) if record_read else observation(raw)
        report["observation"] = value
        success = rc == 0 and report["counts"]["passed"] == 1 and (
            value["completed"] and value["failureCategory"] == "none" if record_read else diagnostic_success(value))
        report["category"] = "observed" if success else "diagnostic-failed"
        code = 0 if success else 1
    except (Exception, KeyboardInterrupt):
        # No exception messages, raw native errors, IDs or paths cross this boundary.
        code = 1
    if not joins.safe():
        report["category"] = "join-uncertain"
        if supplied:
            supplied.close()
        return report, 1
    try:
        if supplied:
            supplied.cleanup()
            supplied.close()
        # Private, exclusive run storage binds this constant proof to this run.
        # A crash or partial write cannot establish completion in the shell.
        with (private / "originals-joined").open("xb") as proof:
            proof.write(COMPLETION)
    except (Exception, KeyboardInterrupt):
        report["category"] = "completion-refused"
        return report, 1
    return report, code


def main():
    os.umask(0o077)
    if len(sys.argv) != 4 and not (len(sys.argv) == 6 and sys.argv[4] == "--record-read"):
        return 1
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(number, h.setup_cancel)
    report, code = run(*(Path(arg) for arg in sys.argv[1:4]),
                       record_source=Path(sys.argv[5]) if len(sys.argv) == 6 else None)
    print(json.dumps(report, sort_keys=True))
    return code


if __name__ == "__main__":
    sys.exit(main())
