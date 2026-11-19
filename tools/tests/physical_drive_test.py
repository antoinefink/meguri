#!/usr/bin/env python3
"""Synthetic public-device evidence tests. Never run Apple tools or hardware."""
import copy
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import stat
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("physical_drive", Path(__file__).parents[1] / "scripts/physical-drive.py")
drive = importlib.util.module_from_spec(spec)
spec.loader.exec_module(drive)


def generated(version=2):
    target = {
        "BlueprintName": "MeguriUITests", "IsUITestBundle": True,
        "TestHostBundleIdentifier": drive.RUNNER,
        "TestHostPath": "__TESTROOT__/Debug-iphoneos/MeguriUITests-Runner.app",
        "TestBundlePath": "__TESTHOST__/PlugIns/MeguriUITests.xctest",
        "UITargetAppPath": "__TESTROOT__/Debug-iphoneos/Meguri.app",
        "DependentProductPaths": ["__TESTROOT__/Debug-iphoneos/Meguri.app"],
        "ProductModuleName": "MeguriUITests", "IsXCTRunnerHostedTestBundle": True,
        "TestHostEnvironmentVariables": {"HOST_METADATA": "retained"},
        "EnvironmentVariables": {"RUNNER_METADATA": "retained"},
        "TestingEnvironmentVariables": {"TEST_HOST_METADATA": "retained"},
        "OnlyTestIdentifiers": ["OtherTests"], "SkipTestIdentifiers": ["AgentDriverTests"],
    }
    metadata = {"FormatVersion": version}
    if version == 1:
        return {"MeguriUITests": target, "__xctestrun_metadata__": metadata}
    return {"TestConfigurations": [{"Name": "UI", "TestTargets": [target]}], "__xctestrun_metadata__": metadata}


class PhysicalDriveEvidenceTests(unittest.TestCase):
    def test_signature_requires_team_identity_and_development_grants(self):
        metadata = ("Identifier=" + drive.APP + "\nTeamIdentifier=ABCDEFGHIJ\nCDHash=" + "a" * 40 +
                    "\nAuthority=Apple Development: synthetic\n").encode()
        grants = {"get-task-allow": True, "application-identifier": "ABCDEFGHIJ." + drive.APP,
                  "com.apple.developer.team-identifier": "ABCDEFGHIJ"}
        for change in (None, "adhoc", "identifier", "grant", "team"):
            with self.subTest(change=change):
                lines = metadata
                entitlements = dict(grants)
                if change == "adhoc":
                    lines += b"Signature=adhoc\n"
                if change == "identifier":
                    lines = lines.replace(drive.APP.encode(), b"other.app")
                if change == "grant":
                    entitlements["get-task-allow"] = False
                if change == "team":
                    entitlements["application-identifier"] = "OTHERTEAM.other.app"
                def signed(argv, category):
                    if "--verify" in argv:
                        return subprocess.CompletedProcess(argv, 0, b"", b"")
                    if "--entitlements" in argv:
                        return subprocess.CompletedProcess(argv, 0, plistlib.dumps(entitlements), b"")
                    return subprocess.CompletedProcess(argv, 0, b"", lines)
                with patch.object(drive, "command", side_effect=signed):
                    if change is None:
                        self.assertIs(drive.signature(Path("/unused/Meguri.app"), drive.APP, True)["verified"], True)
                    else:
                        with self.assertRaises(drive.Refused):
                            drive.signature(Path("/unused/Meguri.app"), drive.APP, True)

    def test_destination_contract_omits_local_paths_retains_host_metadata(self):
        for version in (1, 2):
            original = generated(version)
            saved = copy.deepcopy(original)
            result = drive.destination_xctestrun(original, drive.RUNNER, "__TESTHOST__/PlugIns/MeguriUITests.xctest")
            self.assertEqual(original, saved)
            target = drive.ui_target(result)
            for key in ("TestBundlePath", "TestHostPath", "UITargetAppPath", "DependentProductPaths"):
                self.assertNotIn(key, target)
            self.assertIs(target["UseDestinationArtifacts"], True)
            self.assertEqual(target["TestHostBundleIdentifier"], drive.RUNNER)
            self.assertEqual(target["TestBundleDestinationRelativePath"], "__TESTHOST__/PlugIns/MeguriUITests.xctest")
            self.assertEqual(target["UITargetAppBundleIdentifier"], drive.APP)
            self.assertEqual(target["ProductModuleName"], "MeguriUITests")
            self.assertIs(target["IsXCTRunnerHostedTestBundle"], True)
            self.assertEqual(target["TestHostEnvironmentVariables"], {"HOST_METADATA": "retained"})
            self.assertEqual(target["EnvironmentVariables"], {"RUNNER_METADATA": "retained"})
            self.assertEqual(target["TestingEnvironmentVariables"], {"TEST_HOST_METADATA": "retained"})
            self.assertEqual(target["OnlyTestIdentifiers"], ["AgentDriverTests/testDriveScript"])
            self.assertNotIn("SkipTestIdentifiers", target)
            self.assertEqual(plistlib.loads(plistlib.dumps(result)), result)

    def test_destination_contract_strips_only_approved_target_build_metadata(self):
        metadata = {
            "APP_DISTRIBUTOR_ID_OVERRIDE": "synthetic-distributor-sentinel",
            "XCODE_SCHEME_NAME": "synthetic-scheme-sentinel",
            "XCODE_TEST_PLAN_NAME": "synthetic-test-plan-sentinel",
        }
        environments = [None] + [
            {key: value for index, (key, value) in enumerate(metadata.items()) if mask & (1 << index)}
            for mask in range(1 << len(metadata))
        ]
        for version in (1, 2):
            for environment in environments:
                for empty_arguments in (False, True):
                    with self.subTest(version=version, environment=environment, empty_arguments=empty_arguments):
                        source = generated(version)
                        target = drive.ui_target(source)
                        if environment is not None:
                            target["UITargetAppEnvironmentVariables"] = environment
                        if empty_arguments:
                            target["UITargetAppCommandLineArguments"] = []
                        saved = copy.deepcopy(source)
                        result = drive.destination_xctestrun(source, drive.RUNNER, "__TESTHOST__/PlugIns/MeguriUITests.xctest")
                        self.assertEqual(source, saved)
                        target = drive.ui_target(result)
                        self.assertNotIn("UITargetAppEnvironmentVariables", target)
                        self.assertNotIn("UITargetAppCommandLineArguments", target)
                        serialized = plistlib.dumps(result)
                        for sentinel in metadata.values():
                            self.assertNotIn(sentinel.encode(), serialized)
                        for key in ("TestHostEnvironmentVariables", "EnvironmentVariables", "TestingEnvironmentVariables"):
                            self.assertEqual(target[key], drive.ui_target(saved)[key])
                        self.assertEqual(plistlib.loads(serialized), result)

    def test_destination_contract_refuses_malformed_target_launch_overrides(self):
        environments = [None, [], ["XCODE_SCHEME_NAME"], "", "metadata", 0, False,
                        {"UNKNOWN_KEY": "synthetic"},
                        {"XCODE_SCHEME_NAME": "synthetic", "UNKNOWN_KEY": "synthetic"}]
        for key in ("APP_DISTRIBUTOR_ID_OVERRIDE", "XCODE_SCHEME_NAME", "XCODE_TEST_PLAN_NAME"):
            environments.extend({key: value} for value in (None, 0, False, [], {}))
        arguments = [None, {}, "", "argument", 0, False, (), ["synthetic-argument"], [None], [0], [[]], [{}]]
        for version in (1, 2):
            for key, values in (("UITargetAppEnvironmentVariables", environments),
                                ("UITargetAppCommandLineArguments", arguments)):
                for value in values:
                    with self.subTest(version=version, key=key, value=value):
                        source = generated(version)
                        drive.ui_target(source)[key] = value
                        saved = copy.deepcopy(source)
                        with self.assertRaisesRegex(drive.Refused, "^target-launch-overrides$"):
                            drive.destination_xctestrun(source, drive.RUNNER, "__TESTHOST__/PlugIns/MeguriUITests.xctest")
                        self.assertEqual(source, saved)

    def test_destination_contract_refuses_overrides_unknown_host_and_multiple_configurations(self):
        for key, value in (("UITargetAppCommandLineArguments", ["PRIVATE_ARGUMENT"]),
                           ("UITargetAppEnvironmentVariables", {"PRIVATE_ENV": "value"}),
                           ("TestHostBundleIdentifier", "other.app"), ("IsUITestBundle", False)):
            source = generated()
            drive.ui_target(source)[key] = value
            with self.assertRaises(drive.Refused):
                drive.destination_xctestrun(source, drive.RUNNER, "__TESTHOST__/PlugIns/MeguriUITests.xctest")
        source = generated()
        source["TestConfigurations"] *= 2
        with self.assertRaises(drive.Refused):
            drive.destination_xctestrun(source, drive.RUNNER, "__TESTHOST__/PlugIns/MeguriUITests.xctest")
        for relative in ("../MeguriUITests.xctest", "/private/MeguriUITests.xctest", "other.xctest"):
            with self.assertRaises(drive.Refused):
                drive.destination_xctestrun(generated(), drive.RUNNER, relative)

    def test_selected_process_brackets_require_same_pid_executable_and_ignore_undocumented_fields(self):
        process = {"processIdentifier": 17, "executable": "file:///private/selected/Meguri.app/Meguri"}
        self.assertEqual(drive.continuity(process, dict(process))["status"], "verified")
        changed = {**process, "processIdentifier": 18}
        self.assertEqual(drive.continuity(process, changed)["status"], "failed")
        installed = {"url": "file:///private/selected/Meguri.app/"}
        with patch.object(drive, "device_query", return_value={"runningProcesses": [
                {**process, "startTime": "invented", "otherPrivateField": "discard"}]}) as query:
            selected = drive.selected_process("selected", Path("/unused"), 17, installed)
        self.assertEqual(selected, process)
        self.assertIn("--filter", query.call_args.args[2])
        self.assertEqual(query.call_args.args[2][-1], "processIdentifier == 17")

    def test_targeted_queries_refuse_ambiguity_other_app_and_missing_identity(self):
        installed = {"url": "file:///private/selected/Meguri.app/"}
        row = {"processIdentifier": 17, "executable": installed["url"] + "Meguri"}
        for rows in ([], [row, row], [{**row, "processIdentifier": 18}],
                     [{**row, "executable": "file:///private/other/Other.app/Meguri"}],
                     [{"processIdentifier": 17}]):
            with patch.object(drive, "device_query", return_value={"runningProcesses": rows}):
                with self.assertRaises(drive.Refused):
                    drive.selected_process("selected", Path("/unused"), 17, installed)
        app = {"bundleIdentifier": drive.APP, "bundleVersion": "1", "builtByDeveloper": True,
               "url": installed["url"], "name": "PRIVATE_APP_NAME"}
        with patch.object(drive, "device_query", return_value={"apps": [app]}) as query:
            selected = drive.installation("selected", Path("/unused"), drive.APP)
        self.assertNotIn("name", selected)
        self.assertEqual(query.call_args.args[2][-1], "bundleIdentifier == '" + drive.APP + "'")
        for rows in ([], [app, app], [{**app, "bundleIdentifier": "other.app"}],
                     [{**app, "builtByDeveloper": False}], [{**app, "url": "unknown"}]):
            with patch.object(drive, "device_query", return_value={"apps": rows}):
                with self.assertRaises(drive.Refused):
                    drive.installation("selected", Path("/unused"), drive.APP)

    def test_device_query_places_common_options_before_unchanged_operation_tail(self):
        operations = [
            ["device", "install", "app", "/synthetic/products/Meguri Development.app"],
            ["device", "info", "apps", "--filter", "bundleIdentifier == '" + drive.APP + "'"],
            ["device", "process", "launch", "--terminate-existing", drive.APP],
            ["device", "info", "processes", "--filter", "processIdentifier == 17"],
        ]
        for arguments in operations:
            with self.subTest(arguments=arguments), tempfile.TemporaryDirectory() as directory:
                output = Path(directory)
                def fake(argv, **kwargs):
                    self.assertTrue(kwargs["capture_output"])
                    self.assertEqual(kwargs["timeout"], 60)
                    response = Path(argv[8])
                    log = Path(argv[10])
                    self.assertEqual(argv, ["/usr/bin/xcrun", "devicectl", *arguments[:3],
                        "--device", "synthetic-device", "--json-output", str(response),
                        "--log-output", str(log), *arguments[3:]])
                    self.assertEqual(response.name, "response.json")
                    self.assertEqual(log, response.parent / "diagnostic.log")
                    self.assertEqual(response.parent.parent, output)
                    if arguments[:3] == ["device", "process", "launch"]:
                        self.assertEqual(argv[argv.index(drive.APP) + 1:], [])
                    response.write_text(json.dumps({"info": {"outcome": "success"}, "result": {}}))
                    return subprocess.CompletedProcess(argv, 0, b"", b"")
                with patch.object(drive.products.subprocess, "run", side_effect=fake) as query:
                    self.assertEqual(drive.device_query("synthetic-device", output, arguments), {})
                query.assert_called_once()

    def test_device_responses_and_diagnostics_never_echo_and_raw_response_is_removed(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            def fake(argv, category):
                self.assertEqual(argv[:3], ["/usr/bin/xcrun", "devicectl", "device"])
                destination = Path(argv[argv.index("--json-output") + 1])
                destination.write_text(json.dumps({"info": {"outcome": "success"}, "result": {"apps": []}}))
                Path(argv[argv.index("--log-output") + 1]).write_text("PRIVATE_DIAGNOSTIC")
                return subprocess.CompletedProcess(argv, 0, b"PRIVATE_STDOUT", b"PRIVATE_DIAGNOSTIC")
            stdout, stderr = io.StringIO(), io.StringIO()
            with patch.object(drive, "command", side_effect=fake), redirect_stdout(stdout), redirect_stderr(stderr):
                self.assertEqual(drive.device_query("selected", output, ["device", "info", "apps", "--filter", "selected"]), {"apps": []})
            self.assertEqual(list(output.iterdir()), [])
            self.assertEqual(stdout.getvalue() + stderr.getvalue(), "")

    def test_failed_public_queries_retain_private_artifacts_and_fixed_stage_metadata(self):
        operations = [(["device", "process", "launch", "--terminate-existing", drive.APP], "normal-app-launch"),
                      (["device", "info", "apps", "--filter", "synthetic-selection"], "targeted-installation"),
                      (["device", "info", "processes", "--filter", "synthetic-selection"], "selected-process-query"),
                      (["device", "install", "app", "/synthetic/product"], "install-app")]
        for case in ("command", "malformed", "failed", "invalid-result"):
            for arguments, operation in operations:
                with self.subTest(case=case, operation=operation), tempfile.TemporaryDirectory() as directory:
                    output = Path(directory)
                    raw = (b'{"info":{"outcome":"failed"},"result":{},"detail":"SYNTHETIC_PRIVATE"}'
                           if case in ("command", "failed") else
                           b'{"info":{"outcome":"success"},"result":[],"detail":"SYNTHETIC_PRIVATE"}'
                           if case == "invalid-result" else b'{SYNTHETIC_PRIVATE')
                    def fake(argv, **kwargs):
                        self.assertTrue(kwargs["capture_output"])
                        self.assertEqual(kwargs["timeout"], 60)
                        self.assertEqual(argv[2:5], arguments[:3])
                        self.assertEqual(argv[11:], arguments[3:])
                        Path(argv[argv.index("--json-output") + 1]).write_bytes(raw)
                        Path(argv[argv.index("--log-output") + 1]).write_bytes(b"SYNTHETIC_PRIVATE_LOG")
                        return subprocess.CompletedProcess(argv, 1 if case == "command" else 0,
                                                           b"SYNTHETIC_PRIVATE_STDOUT", b"SYNTHETIC_PRIVATE_STDERR")
                    stdout, stderr = io.StringIO(), io.StringIO()
                    with patch.object(drive.products.subprocess, "run", side_effect=fake) as query, \
                         redirect_stdout(stdout), redirect_stderr(stderr):
                        with self.assertRaises((drive.Refused, ValueError)) as failure:
                            drive.device_query("synthetic-device", output, arguments)
                    query.assert_called_once()
                    self.assertEqual(stdout.getvalue() + stderr.getvalue(), "")
                    retained = output / "failed-device-query"
                    self.assertEqual(list(output.iterdir()), [retained])  # scratch already cleaned
                    self.assertEqual((retained / "response.json").read_bytes(), raw)
                    self.assertEqual((retained / "diagnostic.log").read_bytes(), b"SYNTHETIC_PRIVATE_LOG")
                    metadata = json.loads((retained / "metadata.json").read_bytes())
                    category = "public-device-query" if case == "command" else (
                        "physical-evidence-unavailable" if case == "malformed" else "public-device-response")
                    self.assertEqual(metadata, {"operation": operation,
                        "stage": "command" if case == "command" else (
                            "response-read" if case == "malformed" else "response-validation"),
                        "category": category,
                        "artifacts": {"response.json": "retained", "diagnostic.log": "retained"}})
                    if case != "malformed":
                        self.assertEqual(str(failure.exception), category)
                    self.assertNotIn("SYNTHETIC_PRIVATE", json.dumps(metadata))
                    self.assertEqual(stat.S_IMODE(retained.stat().st_mode), 0o700)
                    for path in retained.iterdir():
                        self.assertTrue(stat.S_ISREG(path.lstat().st_mode))
                        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
                        self.assertEqual(path.stat().st_nlink, 1)

    def test_failed_query_retention_rejects_oversize_and_unsafe_files_independently(self):
        for case in ("response-oversize", "log-oversize", "symlink", "hardlink", "fifo"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                output = Path(directory)
                outside = output / "synthetic-source"
                outside.write_bytes(b"SYNTHETIC_PRIVATE")
                failure = drive.Refused("public-device-query")
                def fake(argv, category):
                    response = Path(argv[argv.index("--json-output") + 1])
                    log = Path(argv[argv.index("--log-output") + 1])
                    if case == "symlink":
                        response.symlink_to(outside)
                    elif case == "hardlink":
                        os.link(outside, response)
                    elif case == "fifo":
                        os.mkfifo(response)
                    else:
                        response.write_bytes(b"x" * (drive.DIAGNOSTIC_BYTES + 1)
                                             if case == "response-oversize" else b"{}")
                    log.write_bytes(b"x" * (drive.DIAGNOSTIC_BYTES + 1)
                                    if case == "log-oversize" else b"SYNTHETIC_PRIVATE_LOG")
                    raise failure
                stdout, stderr = io.StringIO(), io.StringIO()
                with patch.object(drive, "command", side_effect=fake), redirect_stdout(stdout), redirect_stderr(stderr):
                    with self.assertRaises(drive.Refused) as caught:
                        drive.device_query("synthetic-device", output, ["device", "info", "apps"])
                self.assertIs(caught.exception, failure)
                self.assertEqual(stdout.getvalue() + stderr.getvalue(), "")
                retained = output / "failed-device-query"
                rejected = "diagnostic.log" if case == "log-oversize" else "response.json"
                other = "response.json" if rejected == "diagnostic.log" else "diagnostic.log"
                metadata = json.loads((retained / "metadata.json").read_bytes())
                self.assertEqual(metadata["artifacts"], {rejected: "unavailable", other: "retained"})
                self.assertFalse((retained / rejected).exists())
                self.assertTrue((retained / other).is_file())
                self.assertEqual(len(list(retained.iterdir())), 2)
                self.assertTrue(all(path.stat().st_size <= drive.DIAGNOSTIC_BYTES for path in retained.iterdir()))
                self.assertEqual(outside.read_bytes(), b"SYNTHETIC_PRIVATE")

    def test_diagnostic_write_failure_preserves_original_query_category_and_exception(self):
        for failed_write in ("directory", "response.json", "diagnostic.log", "metadata.json"):
            for command_failed in (False, True):
                with self.subTest(write=failed_write, command_failed=command_failed), tempfile.TemporaryDirectory() as directory:
                    output = Path(directory)
                    failure = drive.Refused("public-device-query")
                    def fake(argv, category):
                        Path(argv[argv.index("--json-output") + 1]).write_bytes(
                            b'{"info":{"outcome":"failed"},"result":{}}')
                        Path(argv[argv.index("--log-output") + 1]).write_bytes(b"SYNTHETIC_PRIVATE")
                        if command_failed:
                            raise failure
                        return subprocess.CompletedProcess(argv, 0, b"", b"")
                    original_open = os.open
                    def guarded_open(path, flags, *args, **kwargs):
                        if path == failed_write and flags & os.O_CREAT:
                            raise OSError("SYNTHETIC_PRIVATE_WRITE_ERROR")
                        return original_open(path, flags, *args, **kwargs)
                    if failed_write == "directory":
                        (output / "failed-device-query").symlink_to(output, target_is_directory=True)
                    stdout, stderr = io.StringIO(), io.StringIO()
                    with patch.object(drive, "command", side_effect=fake), \
                         patch.object(drive.os, "open", side_effect=guarded_open), \
                         redirect_stdout(stdout), redirect_stderr(stderr):
                        with self.assertRaises(drive.Refused) as caught:
                            drive.device_query("synthetic-device", output, ["device", "process", "launch"])
                    self.assertEqual(str(caught.exception), "public-device-query" if command_failed else "public-device-response")
                    if command_failed:
                        self.assertIs(caught.exception, failure)
                    self.assertEqual(stdout.getvalue() + stderr.getvalue(), "")
                    self.assertFalse(any(path.name.startswith("device-query-") for path in output.iterdir()))

    def setup_preparation(self, root, output):
        run = root / "build/mg/run/prepared"
        (run / "physical").mkdir(parents=True)
        (run.parent / "latest").symlink_to(run.name)
        installed = {"bundleIdentifier": drive.APP, "bundleVersion": "1", "builtByDeveloper": True,
                     "url": "file:///private/selected/Meguri.app/"}
        runner = {**installed, "bundleIdentifier": drive.RUNNER, "url": "file:///private/runner/MeguriUITests-Runner.app/"}
        artifacts = {"products": {kind: {"build": "1", "path": str(root / "build" / kind)}
                                  for kind in ("app", "runner", "test")},
                     "xctestrun": str(root / "build/products.xctestrun")}
        process = {"processIdentifier": 17, "executable": installed["url"] + "Meguri"}
        witness = {"success": True, "device": "selected", "source": "a" * 64, "artifacts": artifacts,
                   "installed_app": installed, "installed_runner": runner}
        witness_path = run / "physical/install-witness.json"
        witness_path.write_text(json.dumps(witness))
        prepared = {"version": 2, "job": "run", "configuration": "Debug", "normal_launch_verified": True,
                    "device": "selected", "source": "a" * 64, "artifacts": artifacts,
                    "installed_app": installed, "installed_runner": runner, "installed_test_bundle": artifacts["products"]["test"], "process": process,
                    "install_witness_sha256": drive.products.digest(witness_path.read_bytes())}
        (run / "physical/preparation.json").write_text(json.dumps(prepared))
        (run / "result.json").write_text(json.dumps({"job": "run", "status": "pass", "dry_run": False,
            "root": str(root), "placement": "host", "failures": []}))
        return SimpleNamespace(root=root, output=output, source="a" * 64, device="selected"), prepared

    def test_witnessed_admission_generates_destination_run_and_completion_brackets_failed_or_successful_steps(self):
        for failed in (False, True):
            with self.subTest(failed=failed), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                output = root / "build/mg/drive/inspection/physical"
                output.mkdir(parents=True)
                args, prepared = self.setup_preparation(root, output)
                def installed(device, destination, identifier):
                    return prepared["installed_app" if identifier == drive.APP else "installed_runner"]
                with patch.object(drive, "selected_process", return_value=prepared["process"]) as process, \
                     patch.object(drive, "local_artifacts", return_value=(prepared["artifacts"], generated())), \
                     patch.object(drive, "installation", side_effect=installed), \
                     patch.object(drive, "command") as command:
                    drive.admit(args)
                    self.assertEqual(process.call_count, 1)  # first bracket, before runner startup
                    target = drive.ui_target(plistlib.loads((output / "destination.xctestrun").read_bytes()))
                    self.assertTrue(target["UseDestinationArtifacts"])
                    (output.parent / "drive.log").write_text("selected test failed" if failed else "selected test passed")
                    (output.parent / "drive-log.json").write_text(json.dumps({"kept_running": True,
                        "activation_completed": True, "steps": [{"status": "failed" if failed else "ok"}]}))
                    drive.complete(args)
                    self.assertEqual(process.call_count, 2)  # final bracket, even after a failed step
                    command.assert_not_called()  # preserve admission/completion never install or launch
                evidence = json.loads((output / "evidence.json").read_text())
                self.assertIs(evidence["activated_existing"], True)
                self.assertIs(evidence["installed_test_bundle_verified"], True)
                self.assertEqual(evidence["continuity"]["status"], "verified")
                self.assertEqual(evidence["before"], evidence["after"])
                # Completion evidence never rewrites a failed step into success.
                self.assertEqual(json.loads((output.parent / "drive-log.json").read_text())["steps"][0]["status"],
                                 "failed" if failed else "ok")

    def test_completion_refuses_missing_changed_or_interrupted_evidence(self):
        for case in ("process-missing", "process-changed", "app", "runner", "products", "source",
                     "transport", "restart", "diagnostics-missing", "activation-missing", "log-missing"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                output = root / "build/mg/drive/inspection/physical"
                output.mkdir(parents=True)
                args, prepared = self.setup_preparation(root, output)
                drive.write_json(output / "admission.json", {"prepared": prepared, "before": prepared["process"]})
                diagnostic = "Connection lost to selected device" if case == "transport" else (
                    "Restarting after unexpected exit" if case == "restart" else "selected test failed")
                if case != "diagnostics-missing":
                    (output.parent / "drive.log").write_text(diagnostic)
                if case != "log-missing":
                    drive.write_json(output.parent / "drive-log.json", {"kept_running": True,
                        "activation_completed": case != "activation-missing"})
                if case == "source":
                    args.source = "b" * 64
                process = {**prepared["process"], "processIdentifier": 18} if case == "process-changed" else prepared["process"]
                def selected(*_):
                    if case == "process-missing":
                        raise drive.Refused("targeted-process-response")
                    return process
                def installed(device, destination, identifier):
                    value = prepared["installed_app" if identifier == drive.APP else "installed_runner"]
                    return {**value, "url": value["url"] + "changed"} if case == (
                        "app" if identifier == drive.APP else "runner") else value
                with patch.object(drive, "local_artifacts", return_value=(
                        {} if case == "products" else prepared["artifacts"], generated())), \
                     patch.object(drive, "selected_process", side_effect=selected), \
                     patch.object(drive, "installation", side_effect=installed):
                    with self.assertRaises(drive.Refused):
                        drive.complete(args)
                evidence = json.loads((output / "evidence.json").read_text())
                self.assertIs(evidence["activated_existing"], False)
                self.assertNotEqual(evidence["continuity"]["status"], "verified")

    def test_missing_mismatching_or_untrusted_preparation_cannot_start_work(self):
        for case in ("missing", "source", "device", "failed-receipt", "products", "runner", "process", "witness-missing", "witness-changed", "test-binding"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                output = root / "build/mg/drive/inspection/physical"
                output.mkdir(parents=True)
                args, prepared = self.setup_preparation(root, output)
                run = root / "build/mg/run/prepared"
                if case == "missing":
                    (run / "physical/preparation.json").unlink()
                if case in ("source", "device"):
                    setattr(args, case, "mismatch")
                if case == "witness-missing":
                    (run / "physical/install-witness.json").unlink()
                if case == "witness-changed":
                    (run / "physical/install-witness.json").write_text('{}')
                if case == "test-binding":
                    prepared["installed_test_bundle"] = {}
                    (run / "physical/preparation.json").write_text(json.dumps(prepared))
                if case == "failed-receipt":
                    (run / "result.json").write_text('{"job":"run","status":"fail"}')
                local = {} if case == "products" else prepared["artifacts"]
                process = {**prepared["process"], "processIdentifier": 18} if case == "process" else prepared["process"]
                def installed(device, destination, identifier):
                    return prepared["installed_app"] if identifier == drive.APP else ({} if case == "runner" else prepared["installed_runner"])
                with patch.object(drive, "selected_process", return_value=process), \
                     patch.object(drive, "local_artifacts", return_value=(local, generated())), \
                     patch.object(drive, "installation", side_effect=installed), patch.object(drive, "command") as command:
                    with self.assertRaises((drive.Refused, OSError)):
                        drive.admit(args)
                    command.assert_not_called()

    def test_normal_preparation_witnesses_sealed_runner_install_before_final_normal_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            args = SimpleNamespace(root=output, output=output, device="selected", source="a" * 64,
                                   xctestrun=output / "prepared-input.xctestrun")
            artifacts = {"products": {kind: {"path": "/repository/build/" + kind, "build": "1"}
                                      for kind in ("app", "runner", "test")}}
            installed = {"bundleVersion": "1", "url": "file:///private/selected/Meguri.app/"}
            runner = {"bundleVersion": "1", "url": "file:///private/runner/MeguriUITests-Runner.app/"}
            process = {"processIdentifier": 17, "executable": installed["url"] + "Meguri"}
            with patch.object(drive, "local_artifacts", return_value=(artifacts, generated())) as local, \
                 patch.object(drive, "installation", side_effect=[installed, runner]), \
                 patch.object(drive, "selected_process", return_value=process), \
                 patch.object(drive, "device_query", return_value={"process": {"processIdentifier": 17}}) as query:
                drive.prepare(args)
            self.assertEqual([call.args[2] for call in query.call_args_list], [
                ["device", "install", "app", "/repository/build/app"],
                ["device", "install", "app", "/repository/build/runner"],
                ["device", "process", "launch", "--terminate-existing", drive.APP]])
            self.assertEqual(local.call_count, 2)  # source/signature bytes stable across bootstrap
            receipt = json.loads((output / "preparation.json").read_text())
            self.assertIs(receipt["normal_launch_verified"], True)
            self.assertEqual(receipt["survival"], "unknown")
            self.assertEqual(receipt["installed_test_bundle"], artifacts["products"]["test"])
            self.assertEqual(receipt["install_witness_sha256"], drive.products.digest((output / "install-witness.json").read_bytes()))

    def test_failed_bootstrap_or_changed_sealed_products_cannot_launch_or_publish_preparation(self):
        for case in ("install-failed", "products-changed"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                output = Path(directory)
                args = SimpleNamespace(root=output, output=output, device="selected", source="a" * 64,
                                       xctestrun=output / "prepared-input.xctestrun")
                artifacts = {"products": {kind: {"path": "/repository/build/" + kind, "build": "1"}
                                          for kind in ("app", "runner", "test")}}
                calls = []
                def query(device, destination, arguments):
                    calls.append(arguments)
                    if case == "install-failed" and len(calls) == 2:
                        raise drive.Refused("public-device-query")
                    return {}
                with patch.object(drive, "local_artifacts", side_effect=[(artifacts, generated()), ({}, generated())]), \
                     patch.object(drive, "installation", return_value={"bundleVersion": "1"}), \
                     patch.object(drive, "device_query", side_effect=query):
                    with self.assertRaises(drive.Refused):
                        drive.prepare(args)
                self.assertFalse((output / "preparation.json").exists())
                self.assertFalse((output / "install-witness.json").exists())
                self.assertFalse(any("launch" in call for call in calls))


class PersonalPreviewTests(unittest.TestCase):
    def fixture(self, root):
        root = Path(root)
        output = root / "build/mg/run/synthetic/personal-preview"
        output.mkdir(parents=True)
        (output.parent / "personal-preview-settings.json").write_text("[]")
        app = root / drive.PREVIEW_PRODUCTS / "Release-iphoneos/Meguri.app"
        widget = app / "PlugIns/MeguriWidget.appex"
        for path, identifier, executable in ((app, drive.APP, "Meguri"), (widget, drive.APP + ".widget", "MeguriWidget")):
            path.mkdir(parents=True, exist_ok=True)
            (path / "Info.plist").write_bytes(plistlib.dumps({
                "CFBundleIdentifier": identifier, "CFBundleExecutable": executable,
                "CFBundleSupportedPlatforms": ["iPhoneOS"], "CFBundleVersion": "17",
                "CFBundleShortVersionString": "1.0", "MeguriCardInspectionBuild": "personalRelease"}))
            (path / executable).write_bytes(b"synthetic executable")
        stamp = root / "app/Packages/MeguriCore/Binaries/.stamp"
        stamp.parent.mkdir(parents=True)
        stamp.write_text("hash=" + "a" * 32 + "\nconfiguration=Release\nprofile=release\nfeatures=ankiweb\nslices=device,mac\n")
        return SimpleNamespace(root=root, output=output, source="a" * 64, device="selected", xctestrun=None), app, widget, stamp

    def binary_command(self, argv, category):
        return subprocess.CompletedProcess(argv, 0, b"arm64\n" if "-archs" in argv else b"platform IOS\n", b"")

    def test_personal_preview_guard_fixes_release_product_and_refuses_test_hooks_platform_versions_and_links(self):
        for case in ("valid", "hooks", "test", "xctestrun", "debug-dylib", "runner", "release-test",
                     "products-xctestrun", "identity", "version", "typed-version", "link", "simulator"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                args, app, widget, stamp = self.fixture(directory)
                if case == "hooks":
                    stamp.write_text(stamp.read_text().replace("features=ankiweb", "features=ankiweb,test-hooks"))
                elif case in ("test", "xctestrun", "debug-dylib", "runner"):
                    name = {"test": "Other.xctest", "xctestrun": "Other.xctestrun",
                            "debug-dylib": "Meguri.debug.dylib", "runner": "OtherTests-Runner.app"}[case]
                    (app / name).write_bytes(b"forbidden preview artifact")
                elif case == "release-test":
                    (app.parent / "Other.xctest").symlink_to("/missing/unrelated-product")
                elif case == "products-xctestrun":
                    (app.parent.parent / "Other.xctestrun").write_bytes(b"forbidden product name")
                elif case in ("identity", "version", "typed-version"):
                    metadata = plistlib.loads((widget / "Info.plist").read_bytes())
                    metadata["CFBundleIdentifier" if case == "identity" else "CFBundleVersion"] = (
                        17 if case == "typed-version" else "other")
                    (widget / "Info.plist").write_bytes(plistlib.dumps(metadata))
                elif case == "link":
                    (app / "alias").symlink_to("Meguri")
                def binary(argv, category):
                    if case == "simulator" and "vtool" in argv:
                        return subprocess.CompletedProcess(argv, 0, b"platform IOSSIMULATOR\n", b"")
                    return self.binary_command(argv, category)
                with patch.object(drive, "signature", return_value={"team": "TESTTEAM01", "verified": True}) as signed, \
                     patch.object(drive, "command", side_effect=binary), patch.object(drive, "device_query") as query:
                    if case == "valid":
                        artifacts = drive.preview_artifacts(args)
                        self.assertEqual(artifacts["products"]["app"]["path"], str(app))
                        self.assertEqual(set(artifacts["products"]), {"app", "widget"})
                        self.assertEqual([call.args[2] for call in signed.call_args_list], [False, False])
                    else:
                        with self.assertRaises(drive.Refused):
                            drive.preview_artifacts(args)
                    query.assert_not_called()

    def test_personal_preview_installs_only_sealed_app_and_keeps_witness_when_normal_launch_is_locked(self):
        for locked in (False, True):
            with self.subTest(locked=locked), tempfile.TemporaryDirectory() as directory:
                args, app, _, _ = self.fixture(directory)
                # These siblings must never be hashed, traversed or required to
                # resolve. A separate widget product is not the installed widget.
                (app.parent / "Meguri.app.dSYM").mkdir()
                (app.parent / "Meguri.app.dSYM/ignored").symlink_to("/missing/symbols")
                (app.parent / "MeguriWidget.appex").symlink_to("/missing/widget")
                (app.parent.parent / "PackageFrameworks").symlink_to("/missing/packages")
                (app.parent.parent / "Debug-iphoneos").mkdir()
                (app.parent.parent / "Debug-iphoneos/Other.xctest").write_bytes(b"unrelated debug product")
                installed = {"bundleVersion": "17", "url": "file:///private/synthetic/Meguri.app/"}
                process = {"processIdentifier": 17, "executable": installed["url"] + "Meguri"}
                calls = []
                def query(device, output, arguments):
                    self.assertEqual((device, output), (args.device, args.output))
                    calls.append(arguments)
                    if arguments[:3] == ["device", "process", "launch"]:
                        self.assertTrue((output / "preview-install-witness.json").is_file())
                        if locked:
                            raise drive.Refused("public-device-query")
                        return {"process": {"processIdentifier": 17}}
                    return {}
                with patch.object(drive, "signature", return_value={"team": "TESTTEAM01"}) as signed, \
                     patch.object(drive, "command", side_effect=self.binary_command) as binary, \
                     patch.object(drive.products, "snapshot", wraps=drive.products.snapshot) as snapshot, \
                     patch.object(drive.products, "stream_file", wraps=drive.products.stream_file) as stream, \
                     patch.object(drive, "installation", return_value=installed) as installation, \
                     patch.object(drive, "selected_process", return_value=process), \
                     patch.object(drive, "device_query", side_effect=query):
                    drive.preview_seal(args)
                    self.assertEqual(snapshot.call_count, 1)
                    sealed_reads = stream.call_count
                    self.assertEqual(sealed_reads, 4)  # app/widget Info.plist and executable only
                    signed.reset_mock()
                    binary.reset_mock()
                    if locked:
                        with self.assertRaises(drive.Refused):
                            drive.preview(args)
                    else:
                        drive.preview(args)
                    installation.assert_called_once_with(args.device, args.output, drive.APP)
                    self.assertEqual([call.args for call in snapshot.call_args_list],
                                     [(str(args.root / "build"), str(app))] * 3)
                    self.assertEqual(stream.call_count, sealed_reads * 3)
                    signed.assert_not_called()
                    binary.assert_not_called()
                self.assertEqual(calls, [["device", "install", "app", str(app)],
                                        ["device", "process", "launch", "--terminate-existing", drive.APP]])
                witness = drive.read_json(args.output / "preview-install-witness.json")
                self.assertIs(witness["success"], True)
                self.assertEqual(witness["configuration"], "Release")
                self.assertNotIn("installed_runner", witness)
                self.assertEqual((args.output / "preview-launch.json").exists(), not locked)
                for filename in ("preparation.json", "install-witness.json", "destination.xctestrun"):
                    self.assertFalse((args.output / filename).exists())

    def test_personal_preview_refuses_artifact_changes_before_install_and_during_install_without_launch(self):
        cases = ("app", "widget", "metadata", "add", "remove", "mode", "stamp", "settings", "device", "source")
        for phase, change in [(phase, change) for phase in ("before", "during") for change in cases] + [("installed-version", None)]:
            with self.subTest(phase=phase, change=change), tempfile.TemporaryDirectory() as directory:
                args, app, widget, stamp = self.fixture(directory)
                def mutate():
                    if change in ("app", "widget", "metadata"):
                        path = {"app": app / "Meguri", "widget": widget / "MeguriWidget",
                                "metadata": widget / "Info.plist"}[change]
                        path.write_bytes(b"changed after sealing")
                    elif change == "add":
                        (app / "new-resource").write_bytes(b"new")
                    elif change == "remove":
                        (app / "Meguri").unlink()
                    elif change == "mode":
                        (app / "Meguri").chmod(0o700)
                    elif change == "stamp":
                        stamp.write_text(stamp.read_text() + "changed=yes\n")
                    elif change == "settings":
                        (args.output.parent / "personal-preview-settings.json").write_text("[ ]")
                    elif change == "device":
                        args.device = "other-device"
                    elif change == "source":
                        args.source = "b" * 64
                def query(device, output, arguments):
                    self.assertEqual(arguments, ["device", "install", "app", str(app)])
                    if phase == "during":
                        mutate()
                    return {}
                with patch.object(drive, "signature", return_value={"team": "TESTTEAM01"}), \
                     patch.object(drive, "command", side_effect=self.binary_command), \
                     patch.object(drive, "installation", return_value={"bundleVersion": "18" if phase == "installed-version" else "17"}), \
                     patch.object(drive, "device_query", side_effect=query) as device:
                    drive.preview_seal(args)
                    if phase == "before":
                        mutate()
                    with self.assertRaises(drive.Refused):
                        drive.preview(args)
                    self.assertEqual(device.call_count, 0 if phase == "before" else 1)
                self.assertFalse((args.output / "preview-install-witness.json").exists())
                self.assertFalse((args.output / "preview-launch.json").exists())

    def test_personal_preview_arguments_refuse_xctestrun_and_debug_output_before_device_work(self):
        for case in ("xctestrun", "debug-output"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                args, _, _, _ = self.fixture(directory)
                output = args.output if case == "xctestrun" else args.output.with_name("physical")
                argv = ["physical-drive.py", "preview", "--root", str(args.root), "--output", str(output),
                        "--source", args.source, "--device", "00000000-0000000000000001"]
                if case == "xctestrun":
                    argv += ["--xctestrun", str(args.root / "input.xctestrun")]
                with patch.object(drive.sys, "argv", argv), patch.object(drive.os, "umask"), \
                     patch.object(drive, "device_query") as query, patch.object(drive, "preview_artifacts") as artifacts:
                    self.assertEqual(drive.main(), 1)
                    query.assert_not_called()
                    artifacts.assert_not_called()

    def test_personal_preview_cloud_policy_allows_team_ios_release_only_with_explicit_development_facts(self):
        spec = importlib.util.spec_from_file_location("cloud_configuration", Path(__file__).with_name("cloud_build_configuration.py"))
        cloud = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cloud)
        settings = {
            "PRODUCT_BUNDLE_IDENTIFIER": drive.APP, "MEGURI_BUNDLE_ID_SUFFIX": ".dev",
            "MEGURI_CLOUD_CONTAINER_development": "iCloud.example.isolated",
            "MEGURI_CLOUD_CONTAINER_production": "iCloud.example.daily",
            "MEGURI_CLOUD_CONFIGURATION": "development", "MEGURI_CLOUD_CONTAINER": "iCloud.example.isolated",
            "MEGURI_CLOUD_ENVIRONMENT": "development", "MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT": "Development",
            "MEGURI_CLOUD_PUSH_ENVIRONMENT": "development", "INFOPLIST_KEY_MeguriCloudContainer": "iCloud.example.isolated",
            "INFOPLIST_KEY_MeguriCloudEnvironment": "development", "MEGURI_CLOUD_SCHEMA_CONFIGURED_development": "NO",
            "MEGURI_CLOUD_SCHEMA_CONFIGURED_production": "NO", "MEGURI_CLOUD_SCHEMA_CONFIGURED": "NO", "MEGURI_CLOUD_SCHEMA_BOOLEAN": "0"}
        info = {"MeguriCloudContainer": "iCloud.example.isolated", "MeguriCloudEnvironment": "development", "MeguriCloudSchemaConfigured": False}
        grants = {"com.apple.developer.icloud-container-identifiers": ["iCloud.example.isolated"],
                  "com.apple.developer.icloud-services": ["CloudKit"], "com.apple.developer.icloud-container-environment": "Development",
                  "aps-environment": "development", "get-task-allow": True}
        cloud.check(settings, "Release", "ios", "team", info, grants, {}, personal_preview=True)
        for preview, config, signing, entitlements, widget in ((False, "Release", "team", grants, {}),
                (True, "Debug", "team", grants, {}), (True, "Release", "adhoc", grants, {}),
                (True, "Release", "team", {**grants, "aps-environment": "production"}, {}),
                (True, "Release", "team", grants, {"aps-environment": "development"})):
            with self.assertRaises(ValueError):
                cloud.check(settings, config, "ios", signing, info, entitlements, widget, personal_preview=preview)


if __name__ == "__main__":
    unittest.main()
