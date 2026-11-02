#!/usr/bin/env python3
"""Fixture admission checks with isolated platform command doubles."""

import contextlib
import copy
import datetime
import importlib.util
import io
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/cloud-agent-verify.py"
spec = importlib.util.spec_from_file_location("cloud_agent_verify", SCRIPT)
agent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(agent)

BUILD_RUN = "20261102T010000Z-101"
INSTALL_RUN = "20261102T010100Z-102"
RUN = "20261102T010200Z-103"
SHA = "a" * 40
TEAM = "ABCDEFGHIJ"
FIXTURE_VM_ID = "fixture-vm-identifier"
FIXTURE_SIGNER_LABEL = "fixture-authority-value"
CONTAINER = "iCloud.example.generated"


def settings():
    return {
        "CONFIGURATION": "Debug", "PRODUCT_BUNDLE_IDENTIFIER": "com.antoinefink.meguri.dev",
        "MEGURI_BUNDLE_ID_SUFFIX": ".dev", "MEGURI_CORE_FEATURES": "ankiweb,test-hooks",
        "SWIFT_ACTIVE_COMPILATION_CONDITIONS": "DEBUG MEGURI_TEST_HOOKS",
        "MEGURI_CLOUD_CONTAINER_development": CONTAINER,
        "MEGURI_CLOUD_CONTAINER_production": "iCloud.example.daily",
        "MEGURI_CLOUD_CONFIGURATION": "development", "MEGURI_CLOUD_CONTAINER": CONTAINER,
        "MEGURI_CLOUD_ENVIRONMENT": "development", "MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT": "Development",
        "MEGURI_CLOUD_PUSH_ENVIRONMENT": "development", "INFOPLIST_KEY_MeguriCloudContainer": CONTAINER,
        "INFOPLIST_KEY_MeguriCloudEnvironment": "development",
        "MEGURI_CLOUD_SCHEMA_CONFIGURED_development": "YES", "MEGURI_CLOUD_SCHEMA_CONFIGURED_production": "NO",
        "MEGURI_CLOUD_SCHEMA_CONFIGURED": "YES", "MEGURI_CLOUD_SCHEMA_BOOLEAN": "1",
    }


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.results = self.root / "build/mg"
        self.products = self.root / "build/product"
        self.platform = "mac"
        self.product = self.products / "Debug/Meguri.app"
        self.xctestrun = self.products / "Meguri_CloudAgent_macosx.xctestrun"
        self.manifest_path = self.results / "build-app" / BUILD_RUN / "agent-build.json"
        self.expected = {
            "protocol_version": 1, "job": "cloudkit-signed", "run_id": RUN, "lane": "cloud-signed",
            "source_commit": SHA, "tree_clean": True, "device": "slot1", "device_kind": "mac",
            "os_build": "fixture-os-build", "app_build": "42", "executable_sha256": agent.digest(b"executable"),
            "build_receipt": {"job": "build-app", "run_id": BUILD_RUN}, "build_receipt_sha256": "0" * 64,
            "deployment_receipt": {"job": "cloudkit-agent-install", "run_id": INSTALL_RUN},
            "container": CONTAINER, "environment": "development", "account_scope": "fixture-scope",
            "account_labels": ["acct1"], "selection": "smoke", "fixture_delivery": {},
        }
        self.ent = {
            "com.apple.developer.team-identifier": TEAM,
            "com.apple.application-identifier": TEAM + ".com.antoinefink.meguri.dev",
            "com.apple.security.get-task-allow": True,
            "com.apple.developer.icloud-container-identifiers": [CONTAINER],
            "com.apple.developer.icloud-services": ["CloudKit"],
            "com.apple.developer.icloud-container-environment": "Development",
            "com.apple.developer.aps-environment": "development",
        }
        self.info = {
            "CFBundleExecutable": "Meguri", "CFBundleIdentifier": "com.antoinefink.meguri.dev", "CFBundleVersion": "42",
            "CFBundleSupportedPlatforms": ["MacOSX"], "MeguriCardInspectionBuild": "debug",
            "MeguriCloudContainer": CONTAINER, "MeguriCloudEnvironment": "development",
            "MeguriCloudSchemaConfigured": True,
        }
        self.profile = {
            "Entitlements": dict(self.ent), "TeamIdentifier": [TEAM], "Platform": ["OSX"],
            "DeveloperCertificates": [b"generated-certificate"], "ProvisionedDevices": [FIXTURE_VM_ID],
            "ExpirationDate": datetime.datetime(2099, 1, 1),
        }
        self.build = self.envelope("build-app", BUILD_RUN)
        self.build["args"] = ["--platform", "mac", "--test-plan", "CloudAgent"]
        self.build["details"] = {
            "build": {"platform": "mac", "config": "Debug", "app": str(self.product),
                      "standalone": False, "action": "build-for-testing", "personal_preview": False, "test_plan": "CloudAgent"},
        }
        self.settings_path = self.results / "build-app" / BUILD_RUN / "agent-cloud-settings.json"
        self.build["artifacts"] = [{"kind": "app", "path": str(self.product)},
                                   {"kind": "json", "path": str(self.settings_path)},
                                   {"kind": "json", "path": str(self.manifest_path)},
                                   {"kind": "directory", "path": str(self.products)}]
        self.save_settings()
        self.deployment = self.envelope("cloudkit-agent-install", INSTALL_RUN)
        self.save_receipts()
        self.header_path = self.results / "cloudkit-signed" / RUN / "slot1/run.json"
        self.save_json(self.header_path, self.expected)
        self.make_product()
        self.metadata = ("Identifier=com.antoinefink.meguri.dev\nTeamIdentifier=" + TEAM +
                         "\nAuthority=" + FIXTURE_SIGNER_LABEL + "\n").encode()
        self.make_agent()
        self.refresh_manifest()
        self.save_receipts()
        self.save_json(self.header_path, self.expected)

    def save_settings(self):
        host = str(self.product / ("Contents/MacOS/Meguri" if self.platform == "mac" else "Meguri"))
        self.save_json(self.settings_path, [{"target": "Meguri", "buildSettings": settings()},
                       {"target": "MeguriSnapshotTests", "buildSettings": {
                           "CONFIGURATION": "Debug", "PRODUCT_BUNDLE_IDENTIFIER": "example.generated.agent",
                           "TEST_HOST": host, "BUNDLE_LOADER": host}}])

    def refresh_manifest(self):
        with patch.object(agent, "source", return_value=SHA), patch.object(agent.subprocess, "run", side_effect=self.fake_command):
            manifest = agent.freeze_build(str(self.root), str(self.products), str(self.xctestrun),
                                          self.platform, BUILD_RUN, SHA)
        self.save_json(self.manifest_path, manifest)
        self.build["details"]["agent_build"] = agent.file_reference(str(self.root / "build"), str(self.manifest_path))
        return manifest

    def make_agent(self):
        mac = self.platform == "mac"
        self.agent_bundle = self.product / ("Contents/PlugIns" if mac else "PlugIns") / "MeguriSnapshotTests.xctest"
        prefix = self.agent_bundle / "Contents" if mac else self.agent_bundle
        prefix.mkdir(parents=True, exist_ok=True)
        info = {"CFBundleExecutable": "MeguriSnapshotTests", "CFBundleIdentifier": "example.generated.agent",
                "CFBundleSupportedPlatforms": ["MacOSX" if mac else "iPhoneSimulator"],
                "CFBundlePackageType": "BNDL"}
        (prefix / "Info.plist").write_bytes(plistlib.dumps(info))
        executable = prefix / "MacOS/MeguriSnapshotTests" if mac else prefix / "MeguriSnapshotTests"
        executable.parent.mkdir(parents=True, exist_ok=True)
        executable.write_bytes(b"fixture-agent-code")
        dylib = self.product / ("Contents/MacOS/Meguri.debug.dylib" if mac else "Meguri.debug.dylib")
        dylib.write_bytes(b"fixture-debug-library")
        # Captured Xcode layout data supplies no build or runtime evidence.
        lane = "mac" if mac else "sim"
        layout = Path(__file__).parent / "fixtures" / ("cloud-agent-xctestrun-layout-" + lane + ".plist")
        self.captured_xml = layout.read_bytes()
        self.captured_layout = plistlib.loads(self.captured_xml)
        # Only the product tree is synthetic. Preserve every captured loader value.
        self.inject_library = self.product / ("Contents/Frameworks" if mac else "Frameworks") / "libXCTestBundleInject.dylib"
        self.inject_library.parent.mkdir(parents=True, exist_ok=True)
        self.inject_library.write_bytes(b"fixture-xctest-injection-code")
        (self.product.parent / "PackageFrameworks").mkdir(exist_ok=True)
        widget = self.product.parent / "MeguriWidget.appex"
        widget_prefix = widget / "Contents" if mac else widget
        widget_prefix.mkdir(parents=True, exist_ok=True)
        (widget_prefix / "Info.plist").write_bytes(plistlib.dumps({"CFBundleExecutable": "MeguriWidget"}))
        widget_executable = widget_prefix / "MacOS/MeguriWidget" if mac else widget_prefix / "MeguriWidget"
        widget_executable.parent.mkdir(parents=True, exist_ok=True)
        widget_executable.write_bytes(b"fixture-widget-code")
        self.xctestrun.write_bytes(self.captured_xml)

    def use_simulator(self, kind):
        self.platform = "ios-sim"
        self.expected["device_kind"] = kind
        self.product = self.products / "Debug-iphonesimulator/Meguri.app"
        self.info["CFBundleSupportedPlatforms"] = ["iPhoneSimulator"]
        self.info["UIDeviceFamily"] = [1 if kind == "iphone" else 2]
        self.ent.pop("com.apple.security.get-task-allow", None)
        self.ent.pop("com.apple.developer.aps-environment", None)
        self.ent.update({"get-task-allow": True, "aps-environment": "development"})
        self.build["args"] = ["--platform", "ios-sim", "--test-plan", "CloudAgent"]
        self.build["details"]["build"].update(platform="ios-sim", app=str(self.product))
        self.build["artifacts"][0]["path"] = str(self.product)
        self.make_product(kind)
        self.make_agent()
        self.save_settings()
        self.refresh_manifest()
        self.save_receipts()
        self.save_json(self.header_path, self.expected)

    def envelope(self, job, run):
        return {"schema_version": 1, "job": job, "run_id": run, "status": "pass", "exit_code": 0,
                "placement": "host", "service": None, "dry_run": False, "args": [], "root": str(self.root),
                "started_at": "2026-11-01T00:00:00Z", "finished_at": "2026-11-01T00:00:01Z",
                "duration_ms": 1000, "commit": {"sha": SHA, "dirty": False}, "message": "generated",
                "summary": {"total": 1, "passed": 1, "failed": 0, "skipped": 0}, "failures": [],
                "artifacts": [], "user_steps": [], "details": {}}

    def save_json(self, path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value))

    def save_receipts(self):
        for receipt in (self.build, self.deployment):
            path = self.results / receipt["job"] / receipt["run_id"] / "result.json"
            self.save_json(path, receipt)
        self.expected["build_receipt_sha256"] = agent.digest(
            (self.results / "build-app" / BUILD_RUN / "result.json").read_bytes())

    def make_product(self, kind="mac"):
        prefix = self.product / "Contents" if kind == "mac" else self.product
        prefix.mkdir(parents=True, exist_ok=True)
        (prefix / "Info.plist").write_bytes(plistlib.dumps(self.info))
        executable = prefix / "MacOS/Meguri" if kind == "mac" else prefix / "Meguri"
        executable.parent.mkdir(parents=True, exist_ok=True)
        executable.write_bytes(b"executable")
        if kind == "mac":
            (prefix / "embedded.provisionprofile").write_bytes(b"cms")

    def fake_command(self, argv, **kwargs):
        self.assertFalse(kwargs.get("shell", False))
        stdout, stderr = b"", b""
        if argv[0] == "/usr/bin/codesign":
            if "--verbose=4" in argv:
                stderr = (self.metadata.replace(b"Identifier=com.antoinefink.meguri.dev",
                                               b"Identifier=example.generated.agent")
                          if argv[-1].endswith(".xctest") else self.metadata)
            elif "--entitlements" in argv:
                stdout = plistlib.dumps(self.ent)
            elif any(arg.startswith("--extract-certificates=") for arg in argv):
                prefix = next(arg.split("=", 1)[1] for arg in argv if arg.startswith("--extract-certificates="))
                Path(prefix + "0").write_bytes(b"generated-certificate")
            else:
                self.assertIn("--verify", argv)
        elif argv[0] == "/usr/bin/security":
            stdout = plistlib.dumps(self.profile)
        elif argv[0] == "/usr/bin/lipo":
            stdout = b"arm64\n"
        elif argv[:2] == ["/usr/bin/xcrun", "vtool"]:
            stdout = (b"platform IOSSIMULATOR\n" if "Debug-iphonesimulator" in argv[-1]
                      else b"platform MACOS\n")
        else:
            self.fail("unexpected command")
        return subprocess.CompletedProcess(argv, 0, stdout, stderr)

    def check_product(self):
        with patch.object(agent.subprocess, "run", side_effect=self.fake_command):
            agent.independent_product(str(self.root), str(self.product), self.expected, settings(), FIXTURE_VM_ID)

    def reject_product(self, category):
        with self.assertRaises(agent.Refused) as caught:
            self.check_product()
        self.assertEqual(caught.exception.args, (category,))
        self.assertNotIn(FIXTURE_VM_ID, str(caught.exception))
        self.assertNotIn(FIXTURE_SIGNER_LABEL, str(caught.exception))

    def test_receipt_reference_is_exact_and_rejects_alias_or_other_run(self):
        ref = self.expected["build_receipt"]
        receipt, _ = agent.receipt(str(self.root), str(self.results), ref, SHA)
        self.assertEqual(receipt["run_id"], BUILD_RUN)
        for candidate in ({"job": "build-app", "run_id": "latest"}, dict(ref, path="result.json")):
            with self.assertRaises(agent.Refused):
                agent.receipt(str(self.root), str(self.results), candidate, SHA)
        self.build["run_id"] = INSTALL_RUN
        self.save_json(self.results / "build-app" / BUILD_RUN / "result.json", self.build)
        with self.assertRaisesRegex(agent.Refused, "receipt-reference"):
            agent.receipt(str(self.root), str(self.results), ref, SHA)

    def test_receipt_rejects_broken_json_duplicates_and_missing_fields(self):
        path = self.results / "build-app" / BUILD_RUN / "result.json"
        for raw in (b"{", b'{"job":"build-app","job":"other"}', b'{"job":"build-app"}', b'{"x":NaN}'):
            with self.subTest(raw=raw):
                path.write_bytes(raw)
                with self.assertRaises(agent.Invalid):
                    agent.receipt(str(self.root), str(self.results), self.expected["build_receipt"], SHA)

    def test_dirty_foreign_source_dry_run_failures_and_time(self):
        for key, value in (("commit", {"sha": SHA, "dirty": True}), ("root", "/foreign"),
                           ("dry_run", True), ("args", [True]), ("finished_at", "2020-01-01T00:00:00Z"),
                           ("summary", {"total": 1, "passed": 0, "failed": 1, "skipped": 0})):
            candidate = copy.deepcopy(self.build)
            candidate[key] = value
            self.save_json(self.results / "build-app" / BUILD_RUN / "result.json", candidate)
            with self.assertRaises((agent.Refused, agent.Invalid)):
                agent.receipt(str(self.root), str(self.results), self.expected["build_receipt"], SHA)

    def test_symlink_receipt_and_artifact_parent_substitution(self):
        path = self.results / "build-app" / BUILD_RUN / "result.json"
        other = path.with_name("other.json")
        path.rename(other)
        path.symlink_to(other)
        with self.assertRaises((agent.Refused, agent.Invalid)):
            agent.receipt(str(self.root), str(self.results), self.expected["build_receipt"], SHA)
        path.unlink()
        other.rename(path)
        original = self.settings_path.parent
        moved = original.with_name("moved")
        original.rename(moved)
        original.symlink_to(moved, target_is_directory=True)
        with self.assertRaises(agent.Invalid):
            agent.declared_file(str(self.root), self.build, str(self.settings_path))

    def test_missing_declaration_and_parent_escape(self):
        with self.assertRaisesRegex(agent.Refused, "artifact-declaration"):
            agent.declared_file(str(self.root), dict(self.build, artifacts=[]), str(self.settings_path))
        self.build["artifacts"].append({"kind": "json", "path": str(self.root / "build/../outside")})
        self.save_receipts()
        with self.assertRaisesRegex(agent.Refused, "artifact-path"):
            agent.receipt(str(self.root), str(self.results), self.expected["build_receipt"], SHA)

    def test_independent_mac_product_and_adhoc_refusal(self):
        self.check_product()
        self.metadata += b"Signature=adhoc\n"
        self.reject_product("product-signature")

    def test_wrong_cloud_push_and_profile_grants(self):
        for key in ("com.apple.developer.icloud-container-identifiers", "com.apple.developer.aps-environment"):
            original = self.ent.pop(key)
            self.reject_product("product-cloud-grants")
            self.ent[key] = original
        self.profile["Entitlements"]["com.apple.developer.aps-environment"] = "production"
        self.reject_product("profile-grants")

    def test_profile_authorizes_documented_cloud_services_and_environments(self):
        for services in ("*", ["CloudKit"], ["CloudKit", "CloudDocuments"], ["CloudDocuments", "CloudKit"]):
            for environment in ("Development", ["Development"],
                                ["Development", "Production"], ["Production", "Development"]):
                with self.subTest(services=services, environment=environment):
                    self.profile["Entitlements"]["com.apple.developer.icloud-services"] = services
                    self.profile["Entitlements"]["com.apple.developer.icloud-container-environment"] = environment
                    self.check_product()

    def test_profile_rejects_undocumented_or_non_authorizing_cloud_grants(self):
        for key, invalid in (
            ("com.apple.developer.icloud-services", (
                "CloudKit", "CloudDocuments", "**", "", [], ["CloudDocuments"], ["*"],
                ["CloudKit", "*"], ["CloudKit", "CloudKit"], ["CloudKit", "Unknown"],
                ["cloudkit"], ["CloudKit", True], ["CloudKit", ["CloudDocuments"]], True, {})),
            ("com.apple.developer.icloud-container-environment", (
                "Production", "development", "*", "", [], ["Production"], ["*"],
                ["Development", "Development"], ["Development", "Unknown"], ["development"],
                ["Development", False], ["Development", ["Production"]], False, {})),
            ("com.apple.developer.icloud-container-identifiers", (
                ["*"], ["iCloud.example.*"], ["iCloud.example.daily"], [CONTAINER, "iCloud.example.daily"])),
            ("com.apple.developer.aps-environment", ("production", ["development"], "*")),
        ):
            original = self.profile["Entitlements"].pop(key)
            with self.subTest(key=key, missing=True):
                self.reject_product("profile-grants")
            for value in invalid:
                with self.subTest(key=key, value=value):
                    self.profile["Entitlements"][key] = value
                    self.reject_product("profile-grants")
            self.profile["Entitlements"][key] = original

    def test_authorizing_profile_does_not_relax_exact_signed_cloud_claims(self):
        self.profile["Entitlements"]["com.apple.developer.icloud-services"] = "*"
        self.profile["Entitlements"]["com.apple.developer.icloud-container-environment"] = ["Production", "Development"]
        for key, invalid in (
            ("com.apple.developer.icloud-services", (["CloudDocuments"], ["*"], "*", ["CloudKit", "CloudDocuments"])),
            ("com.apple.developer.icloud-container-environment", ("Production", ["Development"], "development")),
            ("com.apple.developer.icloud-container-identifiers", (["iCloud.example.daily"], ["*"], [CONTAINER, "iCloud.example.daily"])),
            ("com.apple.developer.aps-environment", ("production", ["development"])),
        ):
            original = self.ent.pop(key)
            with self.subTest(key=key, missing=True):
                self.reject_product("product-cloud-grants")
            for value in invalid:
                with self.subTest(key=key, value=value):
                    self.ent[key] = value
                    self.reject_product("product-cloud-grants")
            self.ent[key] = original
        self.ent["com.apple.developer.ubiquity-container-identifiers"] = [CONTAINER]
        self.reject_product("product-cloud-grants")

    def test_profile_wrong_device_expiry_and_signer(self):
        self.profile["ProvisionedDevices"] = ["other"]
        self.reject_product("profile-device")
        self.profile["ProvisionedDevices"] = [FIXTURE_VM_ID]
        self.profile["DeveloperCertificates"] = [b"other"]
        self.reject_product("profile-signer-platform")
        self.profile["DeveloperCertificates"] = [b"generated-certificate"]
        self.profile["ExpirationDate"] = datetime.datetime(2000, 1, 1)
        self.reject_product("profile-expired")

    def test_stale_executable_info_and_symlink(self):
        executable = self.product / "Contents/MacOS/Meguri"
        executable.write_bytes(b"stale")
        self.reject_product("executable-bytes")
        executable.unlink()
        other = self.root / "build/other"
        other.write_bytes(b"executable")
        executable.symlink_to(other)
        with self.assertRaises((agent.Refused, agent.Invalid)):
            self.check_product()

    def test_product_mutation_during_signature_inspection(self):
        def mutate(argv, **kwargs):
            result = self.fake_command(argv, **kwargs)
            if argv[0] == "/usr/bin/security":
                (self.product / "Contents/MacOS/Meguri").write_bytes(b"substituted")
            return result
        with patch.object(agent.subprocess, "run", side_effect=mutate):
            with self.assertRaisesRegex(agent.Refused, "product-changed"):
                agent.independent_product(str(self.root), str(self.product), self.expected, settings(), FIXTURE_VM_ID)

    def test_actual_info_executable_and_debug_identity(self):
        self.info["CFBundleExecutable"] = "../other"
        self.make_product()
        self.reject_product("product-identity")
        self.info["CFBundleExecutable"] = "Meguri"
        self.info["MeguriCardInspectionBuild"] = "personalRelease"
        self.make_product()
        self.reject_product("product-debug")

    def test_header_duplicate_and_build_receipt_byte_binding(self):
        self.header_path.write_bytes(b'{"protocol_version":1,"protocol_version":1}')
        with self.assertRaises(agent.Invalid):
            agent.header(str(self.root), str(self.header_path), RUN, SHA)
        self.expected["build_receipt_sha256"] = "0" * 64
        self.save_json(self.header_path, self.expected)
        with patch.object(agent, "source", return_value=SHA):
            with self.assertRaisesRegex(agent.Refused, "build-receipt-bytes"):
                agent.admit(str(self.root), str(self.results), str(self.header_path), RUN)

    def test_reserved_simulator_products_use_simulator_platform_without_hardware_profile(self):
        for kind in ("iphone", "ipad"):
            with self.subTest(kind=kind):
                self.use_simulator(kind)
                self.check_product()
                self.info["CFBundleSupportedPlatforms"] = ["iPhoneOS"]
                self.make_product(kind)
                self.reject_product("product-platform")
                self.info["CFBundleSupportedPlatforms"] = ["iPhoneSimulator"]
                self.info["UIDeviceFamily"] = [2 if kind == "iphone" else 1]
                self.make_product(kind)
                self.reject_product("product-platform")

    def test_simulator_load_command_cannot_be_replaced_with_device_platform(self):
        self.use_simulator("iphone")
        def wrong(argv, **kwargs):
            result = self.fake_command(argv, **kwargs)
            if argv[:2] == ["/usr/bin/xcrun", "vtool"]:
                result.stdout = b"platform IOS\n"
            return result
        with patch.object(agent.subprocess, "run", side_effect=wrong):
            with self.assertRaisesRegex(agent.Refused, "product-platform"):
                agent.independent_product(str(self.root), str(self.product), self.expected, settings())

    def test_explicit_cloudagent_plan_and_full_manifest_are_required(self):
        manifest, _ = agent.build_product(str(self.root), self.build, self.expected)
        self.assertEqual(manifest["agent"]["bundle_id"], "example.generated.agent")
        for args, category in ((["--platform", "mac"], "build-plan"),
                               (["--platform", "mac", "--standalone"], "build-arguments"),
                               (["--platform", "mac", "--test-plan", "UI"], "build-plan"),
                               (["--platform", "mac", "--test-plan", "CloudAgent", "--config", "Release"], "product-debug")):
            changed = copy.deepcopy(self.build)
            changed["args"] = args
            with self.assertRaisesRegex(agent.Refused, category):
                agent.build_product(str(self.root), changed, self.expected)
        changed = copy.deepcopy(self.build)
        del changed["details"]["agent_build"]
        with self.assertRaisesRegex(agent.Refused, "implementation_missing-agent-build"):
            agent.build_product(str(self.root), changed, self.expected)

    def test_frozen_testbundle_debug_dylib_and_xctestrun_substitution_refused(self):
        for path in (self.agent_bundle / "Contents/MacOS/MeguriSnapshotTests",
                     self.product / "Contents/MacOS/Meguri.debug.dylib", self.xctestrun):
            with self.subTest(path=path.name):
                raw = path.read_bytes()
                path.write_bytes(raw + b"substitution")
                with self.assertRaisesRegex(agent.Refused, "product-inventory"):
                    agent.build_product(str(self.root), self.build, self.expected)
                path.write_bytes(raw)

    def test_exact_cloudagent_suite_selections_are_order_independent(self):
        original = self.xctestrun.read_bytes()
        for selection in (["CloudKitSoakTests"],
                          ["CloudKitSoakTests", "CloudKitPathPreflightTests"],
                          ["CloudKitPathPreflightTests", "CloudKitSoakTests"]):
            with self.subTest(selection=selection):
                data = plistlib.loads(original)
                data["TestConfigurations"][0]["TestTargets"][0]["OnlyTestIdentifiers"] = selection
                self.xctestrun.write_bytes(plistlib.dumps(data))
                manifest = self.refresh_manifest()
                self.assertEqual(manifest["agent"]["bundle_id"], "example.generated.agent")

    def test_extra_or_reselected_tests_cannot_be_frozen_as_cloudagent(self):
        original = self.xctestrun.read_bytes()
        for selection in (
            None, [], "", "CloudKitSoakTests", b"CloudKitSoakTests", True, 1, 1.0, {},
            {"CloudKitSoakTests": True}, [""], [True], [1], [[]], [{}],
            ["CloudKitSoakTests", ""], ["CloudKitSoakTests", True],
            ["CloudKitSoakTests", 1], ["CloudKitSoakTests", b"CloudKitPathPreflightTests"],
            ["CloudKitSoakTests", []], ["CloudKitSoakTests", {}],
            ["CloudKitPathPreflightTests"], ["AnotherSuite"],
            ["CloudKitSoakTests", "AnotherSuite"],
            ["CloudKitSoakTests", "CloudKitPathPreflightTests", "AnotherSuite"],
            ["CloudKitSoakTests", "CloudKitSoakTests"],
            ["CloudKitPathPreflightTests", "CloudKitPathPreflightTests"],
            ["CloudKitSoakTests", "CloudKitPathPreflightTests", "CloudKitSoakTests"],
            ["CloudKitPathPreflightTests", "CloudKitSoakTests", "CloudKitPathPreflightTests"],
            ["*"], ["CloudKitSoakTests*"], ["*CloudKitSoakTests"],
            ["MeguriSnapshotTests/CloudKitSoakTests"], ["CloudKitSoakTests/testSmoke"],
            [" CloudKitSoakTests"], ["CloudKitSoakTests "],
            ["CloudKitSoakTests", "CloudKitPathPreflightTests/testPreflight"],
            ["CloudKitSoakTests", "MeguriSnapshotTests/CloudKitPathPreflightTests"],
        ):
            with self.subTest(selection=selection):
                data = plistlib.loads(original)
                target = data["TestConfigurations"][0]["TestTargets"][0]
                if selection is None:
                    target.pop("OnlyTestIdentifiers", None)
                else:
                    target["OnlyTestIdentifiers"] = selection
                self.xctestrun.write_bytes(plistlib.dumps(data))
                with self.assertRaisesRegex(agent.Refused, "agent-selection"):
                    self.refresh_manifest()
        data = plistlib.loads(original)
        data["TestConfigurations"][0]["TestTargets"].append(copy.deepcopy(data["TestConfigurations"][0]["TestTargets"][0]))
        self.xctestrun.write_bytes(plistlib.dumps(data))
        with self.assertRaisesRegex(agent.Refused, "agent-xctestrun"):
            self.refresh_manifest()

    def test_cloudagent_suite_selection_does_not_allow_skips(self):
        original = self.xctestrun.read_bytes()
        for selection in (["CloudKitSoakTests"],
                          ["CloudKitSoakTests", "CloudKitPathPreflightTests"],
                          ["CloudKitPathPreflightTests", "CloudKitSoakTests"]):
            for skipped in (["CloudKitSoakTests"], ["CloudKitPathPreflightTests"],
                            ["CloudKitSoakTests/testSmoke"], ["AnotherSuite"]):
                with self.subTest(selection=selection, skipped=skipped):
                    data = plistlib.loads(original)
                    target = data["TestConfigurations"][0]["TestTargets"][0]
                    target["OnlyTestIdentifiers"] = selection
                    target["SkipTestIdentifiers"] = skipped
                    self.xctestrun.write_bytes(plistlib.dumps(data))
                    with self.assertRaisesRegex(agent.Refused, "agent-selection"):
                        self.refresh_manifest()

    def test_captured_host_bundles_executables_and_dependencies_are_bound(self):
        for lane in ("mac", "sim"):
            with self.subTest(lane=lane):
                if lane == "sim":
                    self.use_simulator("iphone")
                data = plistlib.loads(self.xctestrun.read_bytes())
                target = data["TestConfigurations"][0]["TestTargets"][0]
                self.assertEqual(target["BlueprintProviderRelativePath"], "Meguri.xcodeproj")
                self.assertFalse((self.products / "Meguri.xcodeproj").exists())
                self.assertTrue(target["IsAppHostedTestBundle"])
                self.assertEqual(target["TestHostPath"], "__TESTROOT__/" + str(self.product.relative_to(self.products)))
                manifest, _ = agent.build_product(str(self.root), self.build, self.expected)
                prefix = "Contents/MacOS/" if lane == "mac" else ""
                self.assertEqual(manifest["app"]["executable"], prefix + "Meguri")
                self.assertEqual(manifest["agent"]["path"], str(self.agent_bundle.relative_to(self.products)))
                self.assertEqual(manifest["agent"]["executable"], prefix + "MeguriSnapshotTests")
                for item in (manifest["app"], manifest["agent"]):
                    executable = item["path"] + "/" + item["executable"]
                    self.assertEqual(item["executable_sha256"], manifest["products"]["entries"][executable]["sha256"])
                dependencies = target["DependentProductPaths"]
                self.assertEqual(len(dependencies), 3)
                for dependency in dependencies:
                    path = dependency[len("__TESTROOT__/"):]
                    self.assertEqual(manifest["products"]["entries"][path]["type"], "directory")
                widget_path = next(path for path in manifest["products"]["entries"]
                                   if path.endswith(".appex/" + prefix + "MeguriWidget"))
                widget = self.products / widget_path
                raw = widget.read_bytes()
                widget.write_bytes(raw + b"substitution")
                with self.assertRaisesRegex(agent.Refused, "product-inventory"):
                    agent.build_product(str(self.root), self.build, self.expected)
                widget.write_bytes(raw)

    def assert_captured_loader_inputs(self):
        # Producer UNIT evidence uses command doubles, never actual signed proof.
        self.assertEqual(self.xctestrun.read_bytes(), self.captured_xml)
        target = plistlib.loads(self.xctestrun.read_bytes())["TestConfigurations"][0]["TestTargets"][0]
        self.assertEqual(target, self.captured_layout["TestConfigurations"][0]["TestTargets"][0])
        self.assertEqual(target["EnvironmentVariables"]["DYLD_INSERT_LIBRARIES"], "/usr/lib/libRPAC.dylib")
        expected = ("__TESTHOST__/Contents/Frameworks/libXCTestBundleInject.dylib:"
                    "__DEVELOPERUSRLIB__/libMainThreadChecker.dylib:/usr/lib/libRPAC.dylib"
                    if self.platform == "mac" else
                    "__TESTHOST__/Frameworks/libXCTestBundleInject.dylib:/usr/lib/libRPAC.dylib")
        self.assertEqual(target["TestingEnvironmentVariables"]["DYLD_INSERT_LIBRARIES"], expected)
        frozen = self.refresh_manifest()
        measured, _ = agent.build_product(str(self.root), self.build, self.expected)
        self.assertEqual(measured, frozen)
        self.assertEqual(frozen["source_commit"], SHA)
        self.assertEqual(frozen["xctestrun"]["sha256"], agent.digest(self.captured_xml))
        entry = frozen["products"]["entries"][str(self.inject_library.relative_to(self.products))]
        self.assertEqual(entry["type"], "file")
        self.assertEqual(entry["sha256"], agent.digest(self.inject_library.read_bytes()))
        self.check_product()
        with patch.object(agent.subprocess, "run", side_effect=self.fake_command):
            agent.independent_agent(str(self.root), frozen, TEAM)

    def test_unchanged_captured_mac_loader_layout_freezes_and_binds_fake_product(self):
        self.assert_captured_loader_inputs()

    def test_unchanged_captured_simulator_loader_layout_freezes_and_binds_fake_product(self):
        self.use_simulator("iphone")
        self.assert_captured_loader_inputs()

    def test_captured_embedded_loader_library_bytes_are_bound(self):
        for lane in ("mac", "sim"):
            with self.subTest(lane=lane):
                if lane == "sim":
                    self.use_simulator("iphone")
                original = self.inject_library.read_bytes()
                self.inject_library.write_bytes(original + b"substitution")
                with self.assertRaisesRegex(agent.Refused, "^product-inventory$"):
                    agent.build_product(str(self.root), self.build, self.expected)
                self.inject_library.write_bytes(original)

    def test_final_freeze_binds_generated_nested_and_outer_signature_bytes(self):
        # Byte markers exercise inventory timing, never simulate valid signing.
        self.inject_library.write_bytes(b"generated-copy-plus-final-nested-signature")
        signature = self.product / "Contents/_CodeSignature/CodeResources"
        signature.parent.mkdir(parents=True, exist_ok=True)
        signature.write_bytes(b"generated-final-outer-signature")
        frozen = self.refresh_manifest()
        entries = frozen["products"]["entries"]
        for path in (self.inject_library, signature):
            entry = entries[str(path.relative_to(self.products))]
            self.assertEqual(entry["sha256"], agent.digest(path.read_bytes()))
            original = path.read_bytes()
            path.write_bytes(original + b"post-freeze-change")
            with self.assertRaisesRegex(agent.Refused, "^product-inventory$"):
                agent.build_product(str(self.root), self.build, self.expected)
            path.write_bytes(original)

    def test_captured_embedded_loader_library_is_required(self):
        for lane in ("mac", "sim"):
            with self.subTest(lane=lane):
                if lane == "sim":
                    self.use_simulator("iphone")
                original = self.inject_library.read_bytes()
                self.inject_library.unlink()
                with self.assertRaisesRegex(agent.Refused, "^agent-environment$"):
                    self.refresh_manifest()
                self.inject_library.write_bytes(original)

    def test_captured_host_and_embedded_agent_paths_are_exact(self):
        for lane in ("mac", "sim"):
            if lane == "sim":
                self.use_simulator("iphone")
            original = self.xctestrun.read_bytes()
            target = plistlib.loads(original)["TestConfigurations"][0]["TestTargets"][0]
            executable_host = target["TestHostPath"] + ("/Contents/MacOS/Meguri" if lane == "mac" else "/Meguri")
            changes = (("TestHostPath", executable_host, "agent-host"),
                       ("TestHostPath", "__TESTROOT__/Other.app", "agent-host"),
                       ("IsAppHostedTestBundle", False, "agent-selection"),
                       ("TestHostBundleIdentifier", "example.other.host", "agent-selection"),
                       ("TestBundlePath", "__TESTROOT__/Other.app/PlugIns/MeguriSnapshotTests.xctest", "agent-bundle"))
            for key, value, category in changes:
                with self.subTest(lane=lane, key=key, value=value):
                    data = plistlib.loads(original)
                    data["TestConfigurations"][0]["TestTargets"][0][key] = value
                    self.xctestrun.write_bytes(plistlib.dumps(data))
                    with self.assertRaisesRegex(agent.Refused, category):
                        self.refresh_manifest()
            self.xctestrun.write_bytes(original)

    def test_metadata_exception_does_not_exclude_code_dependencies(self):
        original = self.xctestrun.read_bytes()
        changes = (("BlueprintProviderRelativePath", "/tmp/other.xcodeproj"),
                   ("OtherCodePath", "/tmp/other.dylib"),
                   ("Nested", {"BlueprintProviderRelativePath": "/tmp/other.dylib"}),
                   ("DependentProductPaths", ["/tmp/other.appex"]),
                   ("DependentProductPaths", ["__PLATFORMS__/Other.appex"]),
                   ("DependentProductPaths", ["__TESTROOT__/Debug/missing.appex"]),
                   ("DependentProductPaths", [1]),
                   ("DependentProductPaths", "__TESTROOT__/Debug/Meguri.app"))
        for key, value in changes:
            with self.subTest(key=key, value=value):
                data = plistlib.loads(original)
                data["TestConfigurations"][0]["TestTargets"][0][key] = value
                self.xctestrun.write_bytes(plistlib.dumps(data))
                with self.assertRaises(agent.Refused):
                    self.refresh_manifest()
        self.xctestrun.write_bytes(original)

    def test_loader_environment_cannot_add_external_or_unbound_code(self):
        for lane in ("mac", "sim"):
            if lane == "sim":
                self.use_simulator("iphone")
            original = self.xctestrun.read_bytes()
            debug = "__TESTHOST__/" + ("Contents/MacOS/" if lane == "mac" else "") + "Meguri.debug.dylib"
            changes = (("DYLD_INSERT_LIBRARIES", "/tmp/other.dylib", "agent-environment"),
                       ("DYLD_INSERT_LIBRARIES", debug, "agent-environment"),
                       ("__XPC_DYLD_INSERT_LIBRARIES", "/tmp/other.dylib", "agent-environment"),
                       ("LD_PRELOAD", "/tmp/other.dylib", "agent-environment"),
                       ("DYLD_FRAMEWORK_PATH", "/System/Library/Frameworks", "artifact-path"),
                       ("DYLD_FRAMEWORK_PATH", "__PLATFORMS__/../Other", "agent-environment"),
                       ("DYLD_FRAMEWORK_PATH", "__SHAREDFRAMEWORKS__/Other", "artifact-path"),
                       ("DYLD_LIBRARY_PATH", "__SHAREDFRAMEWORKS__", "artifact-path"),
                       ("__XPC_DYLD_LIBRARY_PATH", "/tmp/other", "artifact-path"),
                       ("__XCODE_BUILT_PRODUCTS_DIR_PATHS", "__TESTROOT__/" + self.product.parent.name + "/missing",
                        "agent-environment"),
                       ("OtherCodePath", "/tmp/other.dylib", "artifact-path"))
            for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
                for name, value, category in changes:
                    with self.subTest(lane=lane, key=key, name=name, value=value):
                        data = plistlib.loads(original)
                        data["TestConfigurations"][0]["TestTargets"][0][key][name] = value
                        self.xctestrun.write_bytes(plistlib.dumps(data))
                        with self.assertRaises(agent.Refused) as caught:
                            self.refresh_manifest()
                        self.assertEqual(caught.exception.args, (category,))
            self.xctestrun.write_bytes(original)

    def test_loader_insertions_reject_additions_unknown_libraries_and_malformed_references(self):
        for lane in ("mac", "sim"):
            if lane == "sim":
                self.use_simulator("iphone")
            original = self.xctestrun.read_bytes()
            unknown = self.inject_library.with_name("FixtureAux.dylib")
            unknown.write_bytes(b"fixture-other-library")
            embedded = "__TESTHOST__/" + str(unknown.relative_to(self.product))
            additions = ("/tmp/other.dylib", embedded,
                         "__DEVELOPERUSRLIB__/FixtureAux.dylib",
                         "__PLATFORMS__/" + ("MacOSX" if lane == "mac" else "iPhoneSimulator") +
                         ".platform/Developer/usr/lib/FixtureAux.dylib",
                         "/usr/lib/FixtureAux.dylib", "/usr/lib/libobjc.A.dylib",
                         "/System/Library/FixtureAux.dylib",
                         "__TESTHOST__/../FixtureAux.dylib",
                         "__TESTHOST__/Frameworks/../../FixtureAux.dylib",
                         "__DEVELOPERUSRLIB__/../libMainThreadChecker.dylib",
                         "/usr/lib/../lib/libRPAC.dylib", "__OTHER__/FixtureAux.dylib",
                         "__TESTHOST__Frameworks/libXCTestBundleInject.dylib",
                         "__DEVELOPERUSRLIB__libMainThreadChecker.dylib")
            for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
                baseline = self.captured_layout["TestConfigurations"][0]["TestTargets"][0][key]["DYLD_INSERT_LIBRARIES"]
                values = [baseline + ":" + item for item in additions]
                values += [baseline + ":", ":" + baseline, baseline + "::/usr/lib/libRPAC.dylib",
                           baseline + ":__TESTHOST__//Frameworks/libXCTestBundleInject.dylib", [baseline], 1]
                for value in values:
                    with self.subTest(lane=lane, key=key, value=value):
                        data = plistlib.loads(original)
                        data["TestConfigurations"][0]["TestTargets"][0][key]["DYLD_INSERT_LIBRARIES"] = value
                        self.xctestrun.write_bytes(plistlib.dumps(data))
                        with self.assertRaises(agent.Refused) as caught:
                            self.refresh_manifest()
                        self.assertEqual(caught.exception.args, ("agent-environment",))
            self.xctestrun.write_bytes(original)

    def test_generated_binary_over_json_cap_reaches_signature_inspection(self):
        executable = self.product / "Contents/MacOS/Meguri"
        block = b"x" * (1024 * 1024)
        with executable.open("wb") as stream:
            for _ in range(17):
                stream.write(block)
        self.expected["executable_sha256"] = agent.binary(str(self.root / "build"), str(executable))[0]["sha256"]
        reached = []
        def inspect(argv, **kwargs):
            if argv[0] == "/usr/bin/codesign" and "--verify" in argv:
                reached.append(argv[-1])
            return self.fake_command(argv, **kwargs)
        with patch.object(agent.subprocess, "run", side_effect=inspect):
            agent.independent_product(str(self.root), str(self.product), self.expected, settings(), FIXTURE_VM_ID)
        self.assertEqual(reached, [str(self.product)])
        self.assertGreater(executable.stat().st_size, 16 * 1024 * 1024)
        with self.assertRaises(agent.Invalid):
            agent._first_sync_json(str(self.root / "build"), str(executable))

    def test_aggregate_budget_covers_all_binary_inputs(self):
        with patch.object(agent, "PRODUCT_BYTES", 8):
            with self.assertRaisesRegex(agent.Refused, "product-size"):
                agent.snapshot(str(self.root / "build"), str(self.products))

    def test_stream_refuses_write_and_same_bytes_inode_replacement(self):
        executable = self.product / "Contents/MacOS/Meguri"
        original_read = agent.os.read
        for replace_inode in (False, True):
            executable.write_bytes(b"initial-code")
            changed = []
            def mutate(fd, count):
                raw = original_read(fd, count)
                if raw and not changed:
                    changed.append(True)
                    if replace_inode:
                        other = executable.with_name("replacement")
                        other.write_bytes(b"initial-code")
                        other.replace(executable)
                    else:
                        executable.write_bytes(b"changed-code")
                return raw
            with patch.object(agent.os, "read", side_effect=mutate):
                with self.assertRaisesRegex(agent.Refused, "product-changed"):
                    agent.binary(str(self.root / "build"), str(executable))

    def test_parent_binding_substitution_is_refused(self):
        executable = self.product / "Contents/MacOS/Meguri"
        parent = executable.parent
        moved = parent.with_name("MacOS-moved")
        original_read = agent.os.read
        replaced = []
        def move_parent(fd, count):
            raw = original_read(fd, count)
            if raw and not replaced:
                replaced.append(True)
                parent.rename(moved)
                parent.mkdir()
                (parent / "Meguri").write_bytes(b"executable")
            return raw
        with patch.object(agent.os, "read", side_effect=move_parent):
            with self.assertRaisesRegex(agent.Refused, "product-changed"):
                agent.binary(str(self.root / "build"), str(executable))

    def test_internal_framework_links_and_escape_cycle_rejection(self):
        framework = self.products / "Fixture.framework"
        (framework / "Versions/A").mkdir(parents=True)
        (framework / "Versions/A/Fixture").write_bytes(b"framework-code")
        (framework / "Versions/Current").symlink_to("A")
        link = framework / "Fixture"
        link.symlink_to("Versions/Current/Fixture")
        entries, _ = agent.snapshot(str(self.root / "build"), str(self.products))
        self.assertEqual(entries["Fixture.framework/Fixture"]["target"], "Versions/Current/Fixture")
        for target in ("/tmp/elsewhere", "../../outside", "Fixture"):
            link.unlink()
            link.symlink_to(target)
            with self.assertRaises(agent.Refused):
                agent.snapshot(str(self.root / "build"), str(self.products))

    def test_only_cf_bundle_version_binds_app_build(self):
        for value in (None, 1, "1.0 (Debug)"):
            self.info["CFBundleVersion"] = value if value is not None else ""
            self.make_product()
            self.reject_product("app-build")

    def test_testbundle_signer_must_match_measured_host_team(self):
        manifest = self.refresh_manifest()
        with patch.object(agent.subprocess, "run", side_effect=self.fake_command):
            agent.independent_agent(str(self.root), manifest, TEAM)
            with self.assertRaisesRegex(agent.Refused, "agent-signature"):
                agent.independent_agent(str(self.root), manifest, "ZZZZZZZZZZ")

    def test_false_deployment_flags_cannot_produce_execution_evidence(self):
        self.deployment["details"] = {"fixture_install_claim": True, "fixture_signing_claim": True, "fixture_scope_claim": True}
        self.save_receipts()
        self.save_json(self.header_path, self.expected)
        with patch.object(agent, "source", return_value=SHA), patch.object(agent.subprocess, "run", side_effect=self.fake_command):
            with self.assertRaisesRegex(agent.Refused, "implementation_missing-vm-pool-install-runtime-proof"):
                agent.admit(str(self.root), str(self.results), str(self.header_path), RUN)

    def test_producer_cannot_freeze_changed_source_or_adhoc_signatures(self):
        with patch.object(agent, "source", return_value="b" * 40):
            with self.assertRaisesRegex(agent.Refused, "source-changed"):
                agent.freeze_build(str(self.root), str(self.products), str(self.xctestrun), "mac", BUILD_RUN, SHA)
        self.metadata += b"Signature=adhoc\n"
        with self.assertRaisesRegex(agent.Refused, "build-signature"):
            self.refresh_manifest()

    def test_public_failure_omits_command_diagnostics(self):
        args = ["verify", "--root", str(self.root), "--results-directory", str(self.results),
                "--header", str(self.header_path), "--run-id", RUN]
        with patch.object(sys, "argv", args), patch.object(agent, "admit", side_effect=ValueError(FIXTURE_SIGNER_LABEL)):
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(agent.main(), 1)
        report = json.loads(output.getvalue())
        self.assertEqual(report["category"], "invalid-input")
        self.assertNotIn(FIXTURE_SIGNER_LABEL, output.getvalue())
        self.assertFalse(report["signed_execution"])
        self.assertNotIn("scenarios", report)


if __name__ == "__main__":
    unittest.main()
