#!/usr/bin/env python3
"""Docker-only generated inputs; all native child operations are stubs."""
import contextlib
import copy
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import signal
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


def good_record_input(count=1):
    return {"version": 1, "collectionZone": "c-" + "a" * 26, "recordType": "MGEntity",
            "records": [{"name": "synthetic-" + str(i), "encrypted": "AQ==", "systemFields": "Ag=="}
                        for i in range(count)]}


def good_record_observation():
    return {"expected": 1, "found": 1, "exactByteMatch": 1, "missing": 0, "different": 0,
            "readError": 0, "completed": True, "failureCategory": "none", "terminalError": "none",
            "slots": [{"found": True, "bytesMatch": True, "changeTagMatch": True,
                       "status": "match", "error": "none"}]}


class RecordInputTests(unittest.TestCase):
    def test_closed_bounded_unique_canonical_input(self):
        for count in (1, 8):
            self.assertEqual(len(p.record_input(json.dumps(good_record_input(count)).encode())["records"]), count)
        invalid = [good_record_input(0), good_record_input(9)]
        for key, value in (("version", True), ("version", 1.0), ("recordType", "other"),
                           ("collectionZone", "c-" + "a" * 25 + "b"), ("container", SECRET)):
            bad = good_record_input(); bad[key] = value; invalid.append(bad)
        bad = good_record_input(2); bad["records"][1] = bad["records"][0]; invalid.append(bad)
        for key, value in (("encrypted", ""), ("encrypted", "AQ==\n"), ("systemFields", "AR=="),
                           ("encrypted", "AQ"), ("encrypted", "*"), ("name", ""),
                           ("name", "x" * 256), ("extra", SECRET)):
            bad = good_record_input(); bad["records"][0][key] = value; invalid.append(bad)
        raw = [json.dumps(item).encode() for item in invalid]
        raw += [b" " * (p.INPUT_CAP + 1), b'{"version":1,"version":1}', b'NaN']
        for item in raw:
            with self.assertRaises((p.Refused, ValueError, TypeError)):
                p.record_input(item)

    def storage(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name).resolve()
        build, support, private = root / "build", root / "support", root / "private"
        for path in (build, support, private): path.mkdir(mode=0o700)
        source = build / "synthetic.json"
        source.write_bytes(json.dumps(good_record_input()).encode())
        transfer = p.RecordInput()
        self.addCleanup(transfer.close)
        return transfer, source, private, build, support

    @contextlib.contextmanager
    def fifo_rejection(self):
        def deadline_reached(_signal_number, _stack):
            raise AssertionError("FIFO rejection blocked")
        previous = signal.signal(signal.SIGALRM, deadline_reached)
        try:
            signal.alarm(2)
            with self.assertRaises(p.Refused):
                yield
        finally:
            signal.alarm(0)
            signal.signal(signal.SIGALRM, previous)

    def test_install_rejects_fifo_without_blocking(self):
        transfer, source, private, build, support = self.storage()
        source.unlink()
        os.mkfifo(source, mode=0o600)
        with self.fifo_rejection():
            transfer.install(source, private, build, support)
        self.assertTrue(source.is_fifo())
        self.assertFalse((support / "MeguriDiagnostics").exists())
        self.assertEqual(list(private.iterdir()), [])

    def test_check_rejects_source_replaced_by_fifo_without_blocking(self):
        transfer, source, private, build, support = self.storage()
        transfer.install(source, private, build, support)
        source.unlink()
        os.mkfifo(source, mode=0o600)
        with self.fifo_rejection():
            transfer.check()
        self.assertTrue(source.is_fifo())
        self.assertEqual((support / "MeguriDiagnostics" / p.INPUT_NAME).read_bytes(), transfer.raw)
        self.assertEqual((private / "record-input.json").read_bytes(), transfer.raw)

    def test_owned_copy_bound_evidence_and_joined_cleanup_preserves_other_files(self):
        transfer, source, private, build, support = self.storage()
        other = support / "user-file"; other.write_text("synthetic user file")
        self.assertEqual(transfer.install(source, private, build, support), 1)
        path = support / "MeguriDiagnostics" / p.INPUT_NAME
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(path.read_bytes(), source.read_bytes())
        self.assertEqual((private / "record-input.json").read_bytes(), source.read_bytes())
        self.assertEqual(json.loads((private / "record-input-binding.json").read_bytes())["records"], 1)
        transfer.check(); transfer.cleanup()
        self.assertFalse(path.parent.exists())
        self.assertEqual(other.read_text(), "synthetic user file")

    def test_source_and_destination_storage_guards(self):
        for fault in ("existing", "symlink", "hardlink", "oversize", "outside", "owner", "parent-link"):
            with self.subTest(fault=fault):
                transfer, source, private, build, support = self.storage()
                if fault == "existing": (support / "MeguriDiagnostics").mkdir()
                if fault == "symlink":
                    original = source.with_suffix(".original"); source.rename(original); source.symlink_to(original)
                if fault == "hardlink": os.link(source, source.with_suffix(".link"))
                if fault == "oversize": source.write_bytes(b"x" * (p.INPUT_CAP + 1))
                if fault == "outside": source = support / "outside.json"; source.write_text("{}")
                if fault == "parent-link":
                    alias = build / "alias"; alias.symlink_to(build, target_is_directory=True); source = alias / source.name
                original = p.os.getuid
                with patch.object(p.os, "getuid", return_value=original() + 1 if fault == "owner" else original()):
                    with self.assertRaises((p.Refused, OSError)):
                        transfer.install(source, private, build, support)
                if fault == "existing": self.assertTrue((support / "MeguriDiagnostics").is_dir())

    def test_changed_source_destination_or_directory_is_preserved(self):
        for fault in ("source", "bytes", "replacement", "mode", "extra", "directory"):
            with self.subTest(fault=fault):
                transfer, source, private, build, support = self.storage()
                transfer.install(source, private, build, support)
                folder = support / "MeguriDiagnostics"; path = folder / p.INPUT_NAME
                if fault == "source": source.write_text("{}")
                if fault == "bytes": path.write_text("{}")
                if fault == "replacement": path.unlink(); path.write_bytes(transfer.raw)
                if fault == "mode": path.chmod(0o644)
                if fault == "extra": (folder / "user-file").touch()
                if fault == "directory": folder.rename(support / "retained"); folder.mkdir(mode=0o700)
                with self.assertRaises((p.Refused, OSError)):
                    transfer.cleanup()
                if fault not in ("extra", "directory"): self.assertTrue(path.exists())
                if fault == "extra": self.assertTrue((folder / "user-file").exists())

    def test_strict_record_report_counts_slots_and_fixed_classes(self):
        value = good_record_observation()
        def parse(value): return p.record_observation((p.READ_PREFIX + json.dumps(value)).encode(), 1)
        self.assertEqual(parse(value), value)
        for status, error in (("missing", "sdkUnknownItem"), ("readError", "sdkNetworkFailure")):
            value = good_record_observation()
            value.update(found=0, exactByteMatch=0, **{status: 1})
            value["slots"] = [dict(found=False, bytesMatch=False, changeTagMatch=False, status=status, error=error)]
            self.assertEqual(parse(value), value)
        value = good_record_observation(); value["different"] = 1
        value["slots"][0].update(changeTagMatch=False, status="different")
        self.assertEqual(parse(value)["exactByteMatch"], 1)
        for key, entry in (("expected", True), ("found", 0), ("extra", SECRET),
                           ("terminalError", SECRET), ("slots", []), ("completed", 1)):
            value = good_record_observation(); value[key] = entry
            with self.assertRaises(p.Refused): parse(value)
        raw = (p.READ_PREFIX + json.dumps(good_record_observation())).encode()
        for bad in (raw + b"\n" + raw, b"prefix " + raw, raw + b"\nMG_SDK_SCOPE_PROBE {}"):
            with self.assertRaises(p.Refused): p.record_observation(bad, 1)


class DiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.app = self.base / "Build/Products/Debug/Meguri.app"
        (self.app / "Contents/PlugIns/MeguriSnapshotTests.xctest").mkdir(parents=True)
        self.info = {"CFBundleIdentifier": p.APP_ID, "CFBundleExecutable": "Meguri",
                     "MeguriCardInspectionBuild": "debug", "LSUIElement": True,
                     "MeguriCloudContainer": "iCloud.synthetic", "MeguriCloudEnvironment": "development",
                     "MeguriCloudSchemaConfigured": True}
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
        self.join_fail_at = None
        self.unknown_at = None
        self.spawn_fault = False
        self.record_read = False
        self.mutate_input = False
        self.record_source = None
        self.home = self.base / "home"
        self.support = self.home / "Library/Containers" / p.APP_ID / "Data/Library/Application Support"
        self.support.mkdir(parents=True)

    def probe(self, stale=False):
        private = Path(tempfile.mkdtemp(dir=self.base)).resolve()
        (private / "build-start").touch()
        self.xctestrun.write_bytes(plistlib.dumps(self.plan))
        if stale:
            os.utime(self.xctestrun, ns=(1, 1))
        owner = self
        launched = []
        class Child:
            def __init__(self, argv, **kwargs):
                self.argv, self.kwargs = argv, kwargs
                self.ordinal = len(launched)
                launched.append(self)
                self.returncode = owner.code if argv[0] == "/usr/bin/xcodebuild" else 0
                if self.ordinal == owner.unknown_at:
                    self.returncode = None
                owner.commands.append((argv, kwargs))
                if owner.spawn_fault and argv[0] == "/usr/bin/xcodebuild":
                    raise p.h.v.Refused("setup-join-uncertain")
                if argv[0] == "/usr/bin/xcodebuild":
                    if owner.mutate_input:
                        (owner.support / "MeguriDiagnostics" / p.INPUT_NAME).write_text("{}")
                    raw = (p.READ_PREFIX if owner.record_read else p.PREFIX) + json.dumps(owner.value) + "\n" + owner.extra_line
                elif "summary" in argv:
                    raw = json.dumps(owner.summary)
                elif "tests" in argv:
                    raw = json.dumps(owner.tree)
                elif "--entitlements" in argv:
                    raw = plistlib.dumps({"com.apple.developer.team-identifier": "TESTTEAM00",
                                          "com.apple.application-identifier": "TESTTEAM00." + p.APP_ID,
                                          "com.apple.developer.icloud-container-environment": "Development",
                                          "com.apple.developer.icloud-container-identifiers": ["iCloud.synthetic"],
                                          "com.apple.developer.icloud-services": ["CloudKit"]})
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
        def join(child):
            self.joined.append(child)
            if child.ordinal == self.join_fail_at:
                raise p.h.v.Refused("setup-join-uncertain")
        with patch.object(p.sys, "platform", "darwin"), patch.dict(os.environ, {
                "MEGURI_APP_SIGNING": "team", "TEST_RUNNER_SECRET": SECRET,
                "MEGURI_CLOUD_AGENT_PRIVATE": SECRET, "MG_ROOT": str(self.base)}), \
                patch.object(p.Path, "home", return_value=self.home), \
                patch.object(p.h, "setup_launch", side_effect=contextlib.nullcontext), \
                patch.object(p.h, "setup_process", side_effect=Child), \
                patch.object(p.h, "setup_join", side_effect=join):
            result = p.run(private, self.xctestrun, self.app, self.record_source)
        self.assertNotIn(SECRET, json.dumps(result))
        proof = private / "originals-joined"
        if result[0]["category"] in ("join-uncertain", "completion-refused"):
            self.assertFalse(proof.exists())
        else:
            self.assertEqual(proof.read_bytes(), p.COMPLETION)
        return result, private

    def record_mode(self):
        self.record_read = True
        self.value = good_record_observation()
        self.tree["testNodes"][0]["nodeIdentifier"] = p.READ_SELECTOR
        build = self.base / "build"; build.mkdir()
        self.record_source = build / "synthetic.json"
        self.record_source.write_bytes(json.dumps(good_record_input()).encode())

    def test_record_mode_only_fixed_selector_and_no_launch_overrides(self):
        self.record_mode()
        (report, code), _ = self.probe()
        self.assertEqual(code, 0)
        self.assertEqual(report["category"], "observed")
        public = json.dumps(report)
        self.assertNotIn(str(self.record_source), public)
        self.assertNotIn(self.record_source.read_text(), public)
        for record in good_record_input()["records"]:
            for value in record.values():
                self.assertNotIn(value, public)
        launch = [argv for argv, _, _ in self.commands if argv[0] == "/usr/bin/xcodebuild"]
        self.assertEqual(len(launch), 1)
        self.assertEqual(launch[0][-1], "-only-testing:" + p.READ_SELECTOR)
        self.assertFalse(any("environment" in item.lower() for item in launch[0]))
        self.assertFalse((self.support / "MeguriDiagnostics").exists())

    def test_record_mode_original_join_and_input_drift_preserve_input_and_proof_fence(self):
        self.record_mode()
        self.join_fail_at = 3
        (report, code), _ = self.probe()
        self.assertEqual((report["category"], code), ("join-uncertain", 1))
        self.assertTrue((self.support / "MeguriDiagnostics" / p.INPUT_NAME).exists())

    def test_record_mode_changed_owned_input_withholds_completion(self):
        self.record_mode(); self.mutate_input = True
        (report, code), _ = self.probe()
        self.assertEqual((report["category"], code), ("completion-refused", 1))
        self.assertEqual((self.support / "MeguriDiagnostics" / p.INPUT_NAME).read_text(), "{}")

    def test_record_mode_refuses_production_or_unconfigured_product_before_install(self):
        self.record_mode()
        for key, value in (("MeguriCloudEnvironment", "production"), ("MeguriCloudSchemaConfigured", False)):
            original = self.info[key]; self.info[key] = value
            (self.app / "Contents/Info.plist").write_bytes(plistlib.dumps(self.info))
            (report, code), _ = self.probe()
            self.assertEqual((report["category"], code), ("product-refused", 1))
            self.assertFalse((self.support / "MeguriDiagnostics").exists())
            self.assertFalse(any(argv[0] == "/usr/bin/xcodebuild" for argv, _, _ in self.commands))
            self.info[key] = original

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

    def test_priority_rebuilt_byte_misses_remain_observations(self):
        for stage in ("priority", "rebuilt"):
            with self.subTest(stage=stage):
                self.value = good_observation()
                self.value[stage]["sawExpectedBytes"] = False
                (report, code), _ = self.probe()
                self.assertEqual(code, 0)
                self.assertEqual(report["category"], "observed")
                self.assertFalse(report["observation"][stage]["sawExpectedBytes"])

    def test_priority_rebuilt_completion_errors_and_cleanup_stay_red(self):
        for stage in ("priority", "rebuilt"):
            changes = [("fetchCompleted", False), ("drained", False),
                       ("callbackErrors", 1), ("unexpectedActivity", True),
                       ("sdkFailure", True), ("cleanupSucceeded", False),
                       ("cleanupFailure", "sdkFailure")]
            for field, value in changes:
                with self.subTest(stage=stage, field=field):
                    self.value = good_observation()
                    if field == "sdkFailure":
                        self.value.update(failurePhase=stage, failureCategory="sdkFailure")
                    elif field.startswith("cleanup"):
                        self.value[field] = value
                    else:
                        self.value[stage][field] = value
                    (report, code), _ = self.probe()
                    self.assertEqual(code, 1)
                    self.assertEqual(report["category"], "diagnostic-failed")

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

    def test_native_worker_checks(self):
        for field in ("join_fail_at", "unknown_at"):
            for ordinal in range(6):
                with self.subTest(field=field, ordinal=ordinal):
                    self.commands, self.joined = [], []
                    setattr(self, field, ordinal)
                    (report, code), _ = self.probe()
                    self.assertEqual(code, 1)
                    self.assertEqual(report["category"], "join-uncertain")
                    self.assertEqual(len(self.commands), ordinal + 1)
                    self.assertEqual(len(self.joined), len(self.commands))
            setattr(self, field, None)

    def test_cancellation_join_failure_stays_uncertain(self):
        self.fault = p.h.v.Refused("setup-cancelled")
        self.join_fail_at = 3
        (report, code), _ = self.probe()
        self.assertEqual(code, 1)
        self.assertEqual(report["category"], "join-uncertain")
        self.assertEqual(len(self.joined), 4)

    def test_uncertain_spawn_withholds_run_proof(self):
        self.spawn_fault = True
        (report, code), _ = self.probe()
        self.assertEqual(code, 1)
        self.assertEqual(report["category"], "join-uncertain")
        self.assertEqual(len(self.commands), 4)
        self.assertEqual(len(self.joined), 3)

    def test_spawn_window_and_join_failure_are_sticky(self):
        for failure in (p.h.v.Refused("setup-join-uncertain"), KeyboardInterrupt()):
            for during in ("spawn", "join"):
                with self.subTest(during=during, failure=type(failure)):
                    private = Path(tempfile.mkdtemp(dir=self.base))
                    joins = p.Joins()
                    child = type("Child", (), {"returncode": 0, "wait": lambda *a, **k: 0})()
                    with patch.object(p.h, "setup_launch", side_effect=contextlib.nullcontext), \
                            patch.object(p.h, "setup_process", side_effect=failure if during == "spawn" else None,
                                         return_value=child) as spawn, \
                            patch.object(p.h, "setup_join", side_effect=failure if during == "join" else None) as join:
                        with self.assertRaises((p.h.v.Refused, KeyboardInterrupt)):
                            p.managed(["generated"], private, "first", 1, joins)
                        self.assertFalse(joins.safe())
                        self.assertEqual(join.call_count, int(during == "join"))
                        # A later command cannot reset the original's obligation.
                        spawn.side_effect = join.side_effect = None
                        with self.assertRaises(p.Refused):
                            p.managed(["generated"], private, "later", 1, joins)
                        self.assertEqual(spawn.call_count, 1)
                        self.assertFalse(joins.safe())

    def test_launch_context_cancellation_joins_before_completion(self):
        @contextlib.contextmanager
        def cancelled_launch():
            yield
            raise p.h.v.Refused("setup-cancelled")
        with patch.object(p.h, "setup_launch", side_effect=cancelled_launch):
            private = Path(tempfile.mkdtemp(dir=self.base))
            joins = p.Joins()
            events = []
            child = type("Child", (), {"returncode": 0, "wait": lambda *a, **k: events.append("wait")})()
            with patch.object(p.h, "setup_process", return_value=child), \
                    patch.object(p.h, "setup_join", side_effect=lambda _: events.append("join")):
                with self.assertRaises(p.h.v.Refused):
                    p.managed(["generated"], private, "cancel", 1, joins)
            self.assertEqual(events, ["join"])
            self.assertTrue(joins.safe())


class EntryTests(unittest.TestCase):
    # Source the real entry, stubbing its existing MG/native boundaries only.
    SCRIPT = r'''
mg_job() { :; }
source "$REPO/tools/mg.d/host.sh"
source "$REPO/tools/mg.d/cloud-fetch-probe.sh"
mg_failure() { printf '%s: %s\n' "$1" "$2" >>"$MG_STATE/failures"; }
mg_status() { printf '%s\n' "$1" >"$MG_STATE/status"; }
mg_message() { :; }
mg_detail() { printf '%s\n' "$1=$2" >>"$MG_STATE/details"; }
mg_count() { printf '%s=%s\n' "$1" "$2" >>"$MG_STATE/counts"; }
uname() { printf '%s\n' "${OS:-Darwin}"; }
mg_host_lease_dir() { echo "$LEASE"; }
mg_host_lease() {
  echo lease >>"$CALLS"
  [ "$FAIL" != lease ] || return 1
  mkdir "$LEASE" || return 1
  MG_LEASE_DIR="$LEASE" MG_LEASE_SLOT="$LEASE"
  mg__lease_write "$LEASE" false || return 1
  case "$FAIL" in
    nested) mkdir "$LEASE/inner"; MG_LEASE_SLOT="$LEASE/inner"; mg__lease_write "$MG_LEASE_SLOT" true ;;
    nested-mark) mg__lease_write "$LEASE" true ;;
    wrong-run) MG_RUN_ID=another-run ;;
    pending) mkdir "$LEASE/vm-xcode-pending" ;;
  esac
}
mg_app_mac_guard() {
  echo guard >>"$CALLS"
  echo generated-private-native-description
  mg_detail private '"generated-private-native-description"'
  if [ "$FAIL" = preexisting-proof ]; then touch "$MG_RUN_DIR/originals-joined"; fi
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
  if [ "$MG_JOB" = cloud-record-read-host ]; then
    [ "$#" -eq 8 ] && [ "$7" = --record-read ] && [ "$8" = "$MG_ROOT/build/synthetic.json" ] || exit 95
  else
    [ "$#" -eq 6 ] || exit 95
  fi
  [ -d "$LEASE/vm-xcode-pending" ] || exit 96
  [ "$FAIL" != drift-run ] || touch "$4/drift-run"
  [ "$FAIL" != ownership-lost ] || MG_RUN_ID=another-run
  case "$FAIL" in
    absent-proof|crash) ;;
    invalid-proof) echo invalid >"$4/originals-joined" ;;
    partial-proof) printf 'HOST-CK-SDK-ORIGINALS-JOINED-v1' >"$4/originals-joined" ;;
    symlink-proof) printf 'HOST-CK-SDK-ORIGINALS-JOINED-v1\n' >"$4/other"; ln -s other "$4/originals-joined" ;;
    directory-proof) mkdir "$4/originals-joined" ;;
    *) printf 'HOST-CK-SDK-ORIGINALS-JOINED-v1\n' >"$4/originals-joined" ;;
  esac
  echo '{"scope":"HOST-CK-SDK-DIAGNOSTIC-ONLY","full_suite":false,"category":"observed","counts":{"total":1,"executed":1,"passed":1,"failed":0,"skipped":0}}'
  case "$FAIL" in crash) return 137 ;; sdk-failure|test-failure) return 65 ;; esac
  return 0
}
mg_sim_begin() { exit 97; }
mg_sim_take() { exit 97; }
mg_app_mac_agent_profile_check() { exit 97; }
n=0
if [ "$MG_JOB" = cloud-record-read-host ]; then
  mg_cloud_record_read_host "$@"
else
  mg_cloud_fetch_probe_host "$@"
fi
[ ! -d "$LEASE/vm-xcode-pending" ] || touch "$FENCED"
mg__host_lease_release "$LEASE" "$$"
'''

    def entry(self, args=(), **env):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            run, state = root / "run", root / "state"
            run.mkdir(); state.mkdir()
            environment = dict(os.environ, REPO=str(ROOT), MG_ROOT=str(ROOT), MG_TOOLS=str(ROOT / "tools"),
                               MG_RUN_DIR=str(run), MG_STATE=str(state), CALLS=str(root / "calls"), FAIL="", OS="Darwin",
                               MEGURI_APP_SIGNING="team", MEGURI_CLOUD_RECORD_READ_INPUT="",
                               MG_RUN_ID="generated-run", MG_JOB="cloud-fetch-probe-host",
                               LEASE=str(root / "lease"), FENCED=str(root / "fenced"))
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
            self.failures = (state / "failures").read_text() if (state / "failures").exists() else ""
            self.fenced = (root / "fenced").exists()
            self.lease_exists = (root / "lease").exists()
            return (state / "status").read_text().strip(), calls, public

    def test_zeroargs_explicit_team_and_darwin_before_work(self):
        for args, env in ((["--suite", "Other/test"], {}), ([""], {}), ([], {"MEGURI_APP_SIGNING": ""}),
                          ([], {"MEGURI_APP_SIGNING": "adhoc"}), ([], {"MEGURI_APP_SIGNING": "team adhoc"}),
                          ([], {"OS": "Linux"})):
            status, calls, _ = self.entry(args, **env)
            self.assertEqual(status, "error")
            self.assertEqual(calls, "")

    def test_record_entry_only_accepts_environment_input_and_uses_same_root_lease(self):
        environment = dict(MG_JOB="cloud-record-read-host", MEGURI_CLOUD_RECORD_READ_INPUT="build/synthetic.json")
        for args in ([""], ["--input"], ["--input", ""], ["--suite", "Other/test"],
                     ["--input", "build/synthetic.json"], ["--input", "build/synthetic.json", "extra"]):
            status, calls, public = self.entry(args, **environment)
            self.assertEqual((status, calls), ("error", ""))
            self.assertEqual(self.failures, "usage: cloud-record-read-host requires zero arguments\n")
        status, calls, public = self.entry(MG_JOB="cloud-record-read-host")
        self.assertEqual((status, calls), ("error", ""))
        self.assertEqual(self.failures, "usage: cloud-record-read-host requires explicit MEGURI_CLOUD_RECORD_READ_INPUT\n")
        for env in ({"MEGURI_APP_SIGNING": ""}, {"MEGURI_APP_SIGNING": "adhoc"},
                    {"MEGURI_APP_SIGNING": "team adhoc"}, {"OS": "Linux"}):
            status, calls, _ = self.entry(**dict(environment, **env))
            self.assertEqual((status, calls), ("error", ""))
        status, calls, public = self.entry(**environment)
        self.assertEqual(status, "warn")
        self.assertIn("helper", calls)
        self.assertNotIn("build/synthetic.json", public)
        self.assertFalse(self.fenced)
        self.assertFalse(self.lease_exists)

    def test_record_environment_is_ignored_by_old_scope_entry(self):
        status, calls, public = self.entry(MEGURI_CLOUD_RECORD_READ_INPUT="build/synthetic.json")
        self.assertEqual(status, "warn")
        self.assertIn("helper", calls)
        self.assertNotIn("build/synthetic.json", public)
        self.assertFalse(self.fenced)
        self.assertFalse(self.lease_exists)

    def test_supported_zeroarg_invocation_has_empty_real_mg_args_and_job_only_repro(self):
        # Exercise the unchanged dispatcher/result writer in a synthetic checkout.
        # Only the native private workflow is replaced; no host work is launched.
        for mode in ("observed", "native-failure", "unset-input", "empty-input", "unsigned"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                tools = root / "tools"; tools.mkdir()
                families = tools / "mg.d"; families.mkdir()
                (tools / "mg").write_bytes((ROOT / "tools/mg").read_bytes())
                (families / "cloud-fetch-probe.sh").write_bytes(
                    (ROOT / "tools/mg.d/cloud-fetch-probe.sh").read_bytes())
                (families / "zz-synthetic.sh").write_text(r'''
uname() { printf 'Darwin\n'; }
mg_cloud_fetch_probe__private() {
  [ "$2" = record-read ] && [ "$3" = "$MEGURI_CLOUD_RECORD_READ_INPUT" ] || return 95
  printf 'source=%s\n' "$3"
  cat "$3" || return 96
  printf '%s\n' '{"scope":"HOST-CK-SDK-DIAGNOSTIC-ONLY","full_suite":false,"category":"observed","counts":{"total":1,"executed":1,"passed":1,"failed":0,"skipped":0}}' >"$1/report.json"
  [ "$SYNTHETIC_MODE" != native-failure ]
}
''')
                build = root / "build"; build.mkdir()
                source = build / "synthetic private source.json"
                raw = json.dumps(good_record_input())
                source.write_text(raw)
                environment = dict(os.environ, MEGURI_APP_SIGNING="team", SYNTHETIC_MODE=mode)
                environment.pop("MEGURI_CLOUD_RECORD_READ_INPUT", None)
                if mode != "unset-input":
                    environment["MEGURI_CLOUD_RECORD_READ_INPUT"] = "" if mode == "empty-input" else str(source)
                if mode == "unsigned":
                    environment.pop("MEGURI_APP_SIGNING", None)
                result = subprocess.run(["/bin/bash", str(tools / "mg"), "cloud-record-read-host"],
                                        env=environment, capture_output=True, text=True)
                receipt_path = Path(result.stdout.strip())
                receipt = json.loads(receipt_path.read_text())
                self.assertEqual(receipt["args"], [])
                self.assertEqual(receipt["status"], "warn" if mode == "observed" else
                                 "fail" if mode == "native-failure" else "error")
                self.assertEqual(result.returncode, 0 if mode == "observed" else
                                 1 if mode == "native-failure" else 2)
                self.assertEqual(len(receipt["failures"]), 0 if mode == "observed" else 1)
                for failure in receipt["failures"]:
                    self.assertEqual(failure["repro"], "tools/mg cloud-record-read-host")
                if mode in ("unset-input", "empty-input"):
                    self.assertEqual(receipt["failures"][0]["message"],
                                     "cloud-record-read-host requires explicit MEGURI_CLOUD_RECORD_READ_INPUT")
                public = result.stdout + result.stderr + "".join(
                    path.read_text() for path in receipt_path.parent.rglob("*")
                    if path.is_file() and "private" not in path.relative_to(receipt_path.parent).parts)
                for secret in (str(source), source.name, raw, SECRET, *good_record_input()["records"][0].values()):
                    self.assertNotIn(secret, public)
                workflow = receipt_path.parent / "private/workflow.log"
                if mode in ("observed", "native-failure"):
                    self.assertIn(str(source), workflow.read_text())
                    self.assertIn(raw, workflow.read_text())
                else:
                    self.assertFalse(workflow.exists())

    def test_lease_guards_build_and_source_order_with_private_native_state(self):
        status, calls, public = self.entry()
        self.assertEqual(status, "warn")
        self.assertFalse(self.fenced)
        self.assertFalse(self.lease_exists)
        self.assertEqual(calls.splitlines(), ["lease", "guard", "digest", "build mac Debug Snapshot", "digest",
                                              "lookup Snapshot macosx Debug", "guard", "helper", "digest"])
        self.assertIn('full_suite=false', public)
        for phase, last in (("lease", "lease"), ("guard1", "guard"), ("build", "build mac Debug Snapshot"),
                            ("guard2", "guard"), ("drift", "digest"), ("preexisting-proof", "guard")):
            # lease refusal precedes the guard's private-console sentinel.
            status, calls, public = self.entry(FAIL=phase)
            self.assertEqual(status, "fail")
            self.assertEqual(calls.splitlines()[-1], last)
            self.assertNotIn("helper", calls)
            self.assertFalse(self.fenced)
            self.assertFalse(self.lease_exists)
        status, calls, public = self.entry(FAIL="drift-run")
        self.assertEqual(status, "fail")
        self.assertIn("helper", calls)
        self.assertIn("source-binding", public)
        self.assertFalse(self.fenced)
        self.assertFalse(self.lease_exists)

    def test_root_ownership_refuses_nested_or_unresolved_lease_before_work(self):
        for phase in ("nested", "nested-mark", "wrong-run", "pending"):
            with self.subTest(phase=phase):
                status, calls, _ = self.entry(FAIL=phase)
                self.assertEqual(status, "fail")
                self.assertEqual(calls.splitlines(), ["lease"])
                self.assertEqual(self.fenced, phase == "pending")
                self.assertEqual(self.lease_exists, phase in ("nested", "pending"))

    def test_absent_invalid_or_crashed_completion_keeps_fence_even_on_zero_exit(self):
        for phase in ("absent-proof", "invalid-proof", "partial-proof", "symlink-proof", "directory-proof", "crash", "ownership-lost"):
            with self.subTest(phase=phase):
                status, calls, public = self.entry(FAIL=phase)
                self.assertEqual(status, "fail")
                self.assertEqual(calls.splitlines()[-1], "helper")
                self.assertIn("join-uncertain", public)
                self.assertTrue(self.fenced)
                self.assertTrue(self.lease_exists)

    def test_normally_joined_sdk_and_test_failures_release_only_own_fence(self):
        for phase in ("sdk-failure", "test-failure"):
            with self.subTest(phase=phase):
                status, calls, public = self.entry(FAIL=phase)
                self.assertEqual(status, "fail")
                self.assertIn("helper", calls)
                self.assertIn("diagnostic", public)
                self.assertFalse(self.fenced)
                self.assertFalse(self.lease_exists)

    def test_pending_marker_blocks_existing_stale_drop_and_new_admission(self):
        script = r'''
mg_job() { :; }
source "$REPO/tools/mg.d/host.sh"
mkdir "$LEASE"
holder='{"pid":1234,"job":"cloud-fetch-probe-host","run_id":"old-generated"}'
printf '%s\n' "$holder" >"$LEASE/holder.json"
mkdir "$LEASE/vm-xcode-pending"
# Model a dead shell without querying or signalling any real process.
kill() { return 1; }
if mg__lease_stale "$LEASE" "$holder"; then exit 91; fi
host_lease_lock "$LEASE" || exit 92
host_lease_drop "$LEASE"
rc=$?
host_lease_unlock
[ "$rc" -ne 0 ] && [ -d "$LEASE/vm-xcode-pending" ] || exit 93
mg__lease_try "$LEASE"
[ "$?" -eq 1 ] && [ -z "$MG_LEASE_SLOT" ] && [ ! -e "$LEASE/inner" ] || exit 94
[ "$(cat "$LEASE/holder.json")" = "$holder" ] || exit 95
'''
        with tempfile.TemporaryDirectory() as temp:
            subprocess.run(["/bin/bash", "-c", script], check=True, capture_output=True,
                           env=dict(os.environ, REPO=str(ROOT), MG_TOOLS=str(ROOT / "tools"),
                                    MG_ROOT=str(ROOT), LEASE=str(Path(temp) / "lease")))


if __name__ == "__main__":
    unittest.main()
