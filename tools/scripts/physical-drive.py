#!/usr/bin/env python3
"""Physical development preparation and destination-artifact continuity (D1-D4).

Only targeted public devicectl queries are used. A witnessed sealed runner
install binds its nested test bundle. Selected PID, executable and installation
brackets cover all preserve-mode MG work; interruption or missing evidence
cannot establish continuity. All selected metadata stays in private build output.
"""

import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import sys
import tempfile
from urllib.parse import unquote, urlparse

# Reuse MG's descriptor-bound product inventory, bounded reads, signature
# command handling and fixed refusal categories. No installed code is imported.
spec = importlib.util.spec_from_file_location("mg_products", Path(__file__).with_name("cloud-agent-verify.py"))
products = importlib.util.module_from_spec(spec)
spec.loader.exec_module(products)
Refused, need, command = products.Refused, products.need, products.command
APP = "com.antoinefink.meguri.dev"
TEST = APP + ".uitests"
RUNNER = TEST + ".xctrunner"
PREVIEW_PRODUCTS = "build/app/PersonalDevicePreviewDerivedData/Build/Products"
# The owner-requested real copy (run --real-copy): daily-driver ids, its own
# products and settings, installed only, never launched.
DAILY = "com.antoinefink.meguri"
REAL_PRODUCTS = "build/app/RealCopyDeviceDerivedData/Build/Products"
MODES = {
    "preview": {"app": APP, "products": PREVIEW_PRODUCTS, "settings": "personal-preview-settings.json"},
    "real": {"app": DAILY, "products": REAL_PRODUCTS, "settings": "real-copy-settings.json"},
}
DIAGNOSTIC_BYTES = 16 * 1024 * 1024  # Per artifact, matching the guarded reader.


def read_bytes(path, boundary):
    try:
        return products._first_sync_bytes(str(boundary), str(path))
    except products.Invalid:
        raise Refused("guarded-input") from None


def read_json(path):
    return json.loads(read_bytes(path, path.parent))


def write_json(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, sort_keys=True, indent=2)


def info(path):
    return products.plist(read_bytes(path / "Info.plist", path), "product-info")


def signature(path, identifier, development):
    command(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(path)], "product-signature")
    report_rows = command(
        ["/usr/bin/codesign", "-d", "--verbose=4", str(path)], "product-signature"
    ).stderr.decode().splitlines()
    team_values = [row.partition("=")[2] for row in report_rows if row.startswith("TeamIdentifier=")]
    digest_values = [row.partition("=")[2] for row in report_rows if row.startswith("CDHash=")]
    need(
        len(team_values) == 1
        and re.fullmatch(r"[A-Z0-9]{10}", team_values[0])
        and len(digest_values) == 1
        and re.fullmatch(r"[a-fA-F0-9]+", digest_values[0])
        and "Identifier=" + identifier in report_rows
        and "Signature=adhoc" not in report_rows
        and any(row.startswith("Authority=") for row in report_rows),
        "product-signature",
    )
    if development:
        grants = products.plist(
            command(
                ["/usr/bin/codesign", "-d", "--entitlements", "-", "--xml", str(path)],
                "product-entitlements",
            ).stdout,
            "product-entitlements",
        )
        need(
            grants.get("get-task-allow") is True
            and grants.get("application-identifier") == team_values[0] + "." + identifier
            and grants.get("com.apple.developer.team-identifier") == team_values[0],
            "development-signature",
        )
    return {
        "team": team_values[0],
        "cdhash": digest_values[0],
        "verified": True,
        "development": development,
    }


def ui_target(value):
    """Retain generated metadata, selecting only the existing UI test target."""
    if "TestConfigurations" in value:
        configs = value["TestConfigurations"]
        need(isinstance(configs, list) and len(configs) == 1, "xctestrun-configuration")
        targets = configs[0].get("TestTargets", [])
    else:
        targets = [v for k, v in value.items() if k != "__xctestrun_metadata__" and isinstance(v, dict)]
    matches = [t for t in targets if t.get("BlueprintName") == "MeguriUITests"]
    need(len(matches) == 1 and matches[0].get("IsUITestBundle") is True, "xctestrun-ui-target")
    return matches[0]


def destination_xctestrun(value, runner_id, test_relative):
    """Public xcodebuild.xctestrun(5) destination contract, never fake paths.

    This transformation is necessary, but is not installed-artifact admission.
    Keep TestHostBundleIdentifier and all required generated test-host metadata.
    """
    need(runner_id == RUNNER and test_relative == "__TESTHOST__/PlugIns/MeguriUITests.xctest", "destination-artifacts")
    value = copy.deepcopy(value)
    target = ui_target(value)
    need(target.get("TestHostBundleIdentifier") == RUNNER, "test-host-identity")
    if "UITargetAppEnvironmentVariables" in target:
        environment = target["UITargetAppEnvironmentVariables"]
        need(isinstance(environment, dict) and environment.keys() <= {
            "APP_DISTRIBUTOR_ID_OVERRIDE", "XCODE_SCHEME_NAME", "XCODE_TEST_PLAN_NAME"
        } and all(isinstance(item, str) for item in environment.values()), "target-launch-overrides")
    if "UITargetAppCommandLineArguments" in target:
        arguments = target["UITargetAppCommandLineArguments"]
        need(isinstance(arguments, list) and not arguments, "target-launch-overrides")
    # Accepted build metadata must never become target-app launch overrides.
    for key in ("UITargetAppCommandLineArguments", "UITargetAppEnvironmentVariables"):
        target.pop(key, None)
    for key in ("TestBundlePath", "TestHostPath", "UITargetAppPath", "DependentProductPaths"):
        target.pop(key, None)
    target.update(UseDestinationArtifacts=True, TestHostBundleIdentifier=RUNNER,
                  TestBundleDestinationRelativePath=test_relative, UITargetAppBundleIdentifier=APP)
    target["OnlyTestIdentifiers"] = ["AgentDriverTests/testDriveScript"]
    target.pop("SkipTestIdentifiers", None)
    # Other targets must never start implicitly in a destination-only drive.
    if "TestConfigurations" in value:
        value["TestConfigurations"][0]["TestTargets"] = [target]
    else:
        value = {"MeguriUITests": target, **({"__xctestrun_metadata__": value["__xctestrun_metadata__"]}
                                          if "__xctestrun_metadata__" in value else {})}
    return value


def local_artifacts(root, xctestrun):
    boundary = root / "build"
    base = root / "build/app/DerivedData/Build/Products"
    products.contained(str(base), str(xctestrun))
    source = read_bytes(xctestrun, base)
    generated = products.plist(source, "xctestrun")
    target = ui_target(generated)
    app = base / "Debug-iphoneos/Meguri.app"
    runner = base / "Debug-iphoneos/MeguriUITests-Runner.app"
    test = runner / "PlugIns/MeguriUITests.xctest"

    def expand(path):
        need(isinstance(path, str), "xctestrun-path")
        path = path.replace("__TESTROOT__", str(base)).replace("__TESTHOST__", str(runner))
        need("__" not in path, "xctestrun-path")
        return path

    need(expand(target.get("TestHostPath")) == str(runner) and
         expand(target.get("TestBundlePath")) == str(test) and
         expand(target.get("UITargetAppPath")) == str(app), "xctestrun-product-binding")
    before = {str(p): products.snapshot(str(boundary), str(p))[0] for p in (app, runner)}
    # Device products must be contained regular products, including their
    # nested bundle. Do not bootstrap a runner through a symlink alias.
    need(all(entry["type"] != "symlink" for entries in before.values() for entry in entries.values()),
         "product-path")
    artifacts = {}
    team = None
    for kind, path, identifier in (("app", app, APP), ("runner", runner, RUNNER), ("test", test, TEST)):
        metadata = info(path)
        need(metadata.get("CFBundleIdentifier") == identifier and
             isinstance(metadata.get("CFBundleVersion"), str) and metadata["CFBundleVersion"], "product-identity")
        if kind == "app":
            need(metadata.get("CFBundleSupportedPlatforms") == ["iPhoneOS"] and
                 metadata.get("MeguriCardInspectionBuild") == "debug", "product-debug-device")
        signed = signature(path, identifier, kind != "test")
        need(team is None or team == signed["team"], "product-team")
        team = signed["team"]
        artifacts[kind] = {"path": str(path), "bundle_id": identifier,
                           "build": metadata["CFBundleVersion"], "signature": signed}
    need(all(products.snapshot(str(boundary), path)[0] == entries for path, entries in before.items()),
         "product-changed")
    need(read_bytes(xctestrun, base) == source, "product-changed")
    return {"products": artifacts, "inventory": before, "xctestrun": str(xctestrun),
            "xctestrun_sha256": products.digest(source)}, generated


def retain_failed_query(output, scratch, arguments, stage, error):
    """Best effort only: two bounded private files and fixed metadata, no echo.

    Never serialize argv, stdout/stderr, exception text or runner environment.
    Raw public-tool evidence may contain sensitive values; it stays under MG's
    private output, behind nofollow parents and exclusive mode-0600 files.
    """
    operation = {
        ("device", "install", "app"): "install-app",
        ("device", "info", "apps"): "targeted-installation",
        ("device", "process", "launch"): "normal-app-launch",
        ("device", "info", "processes"): "selected-process-query",
    }.get(tuple(arguments[:3]), "public-device-query")
    category = str(error) if isinstance(error, Refused) else "physical-evidence-unavailable"
    try:
        destination = output / "failed-device-query"
        products.contained(str(output), str(destination))
        with products.authority(str(output)) as parent:
            os.mkdir(destination.name, mode=0o700, dir_fd=parent)
            with products.authority(str(destination)) as retained:
                def write(name, raw, bound):
                    need(len(raw) <= bound, "diagnostic-size")
                    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                                 0o600, dir_fd=retained)
                    with os.fdopen(fd, "wb") as stream:
                        info = os.fstat(stream.fileno())
                        need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                             stat.S_IMODE(info.st_mode) == 0o600, "diagnostic-path")
                        stream.write(raw)

                artifacts = {}
                for name in ("response.json", "diagnostic.log"):
                    try:
                        raw = read_bytes(scratch / name, scratch)
                        write(name, raw, DIAGNOSTIC_BYTES)
                        artifacts[name] = "retained"
                    except Exception:
                        artifacts[name] = "unavailable"
                metadata = {"operation": operation, "stage": stage, "category": category,
                            "artifacts": artifacts}
                write("metadata.json", json.dumps(metadata, sort_keys=True).encode(), 1024)
    except Exception:
        # Diagnostic failures must never replace the original query failure.
        pass


def device_query(device, output, arguments):
    """Successful raw responses are transient; failed evidence stays private."""
    with tempfile.TemporaryDirectory(prefix="device-query-", dir=output) as scratch:
        scratch = Path(scratch)
        stage = "command"
        try:
            command(["/usr/bin/xcrun", "devicectl", *arguments[:3], "--device", device,
                     "--json-output", str(scratch / "response.json"),
                     "--log-output", str(scratch / "diagnostic.log"), *arguments[3:]], "public-device-query")
            stage = "response-read"
            value = read_json(scratch / "response.json")
            stage = "response-validation"
            need(value.get("info", {}).get("outcome") == "success" and isinstance(value.get("result"), dict),
                 "public-device-response")
            return value["result"]
        except Exception as error:
            retain_failed_query(output, scratch, arguments, stage, error)
            raise


def installation(device, output, identifier, required=True):
    need(identifier in (APP, RUNNER, DAILY), "installation-selection")
    result = device_query(device, output, ["device", "info", "apps", "--filter",
                                          "bundleIdentifier == '" + identifier + "'"])
    apps = result.get("apps")
    need(isinstance(apps, list) and len(apps) <= 1, "targeted-installation-response")
    if not apps and not required:
        return None
    need(len(apps) == 1 and apps[0].get("bundleIdentifier") == identifier, "installed-app-missing")
    app = apps[0]
    need(app.get("builtByDeveloper") is True and isinstance(app.get("bundleVersion"), str) and
         isinstance(app.get("url"), str), "installed-development-app")
    installed_path(app["url"])
    return {key: app[key] for key in ("bundleIdentifier", "bundleVersion", "builtByDeveloper", "url")}


def installed_path(url):
    parsed = urlparse(url)
    need(parsed.scheme == "file" and not parsed.netloc and not parsed.query and not parsed.fragment,
         "installation-path")
    path = unquote(parsed.path).rstrip("/")
    need(path.startswith("/") and ".." not in path.split("/") and "\x00" not in path, "installation-path")
    return path


def selected_process(device, output, pid, installed):
    need(type(pid) is int and pid > 0, "selected-process")
    result = device_query(device, output, ["device", "info", "processes", "--filter",
                                          "processIdentifier == " + str(pid)])
    rows = result.get("runningProcesses")
    need(isinstance(rows, list) and len(rows) == 1 and rows[0].get("processIdentifier") == pid,
         "targeted-process-response")
    row = rows[0]
    need(isinstance(row.get("executable"), str) and
         installed_path(row["executable"]) == installed_path(installed["url"]) + "/Meguri", "process-app-binding")
    # Do not retain other process fields or accept undocumented start-time keys.
    return {"processIdentifier": pid, "executable": row["executable"]}


def continuity(before, after):
    if not before or not after:
        return {"status": "unknown", "reason": "selected-process-unavailable"}
    if before != after:
        return {"status": "failed", "reason": "selected-process-changed"}
    return {"status": "verified", "reason": "selected-process-and-installation-brackets"}


def prepare(args):
    artifacts, generated = local_artifacts(args.root, args.xctestrun)
    with (args.output / "prepared.xctestrun").open("xb") as stream:
        plistlib.dump(generated, stream)
    # Normal preparation may install both verified products, before the final
    # app launch. Public install success plus stable sealed local bytes is the
    # private bootstrap witness for the runner's nested test bundle.
    for kind in ("app", "runner"):
        device_query(args.device, args.output, ["device", "install", "app", artifacts["products"][kind]["path"]])
    installed = installation(args.device, args.output, APP)
    runner = installation(args.device, args.output, RUNNER)
    need(installed["bundleVersion"] == artifacts["products"]["app"]["build"] and
         runner["bundleVersion"] == artifacts["products"]["runner"]["build"], "installed-product-build")
    current, _ = local_artifacts(args.root, args.xctestrun)
    need(current == artifacts, "bootstrap-products-changed")
    witness = {"success": True, "device": args.device, "source": args.source,
               "artifacts": artifacts, "installed_app": installed, "installed_runner": runner}
    write_json(args.output / "install-witness.json", witness)
    # Only the development app may be restarted. Never fixture, reset or uninstall.
    launch = device_query(args.device, args.output, ["device", "process", "launch", "--terminate-existing", APP])
    pid = launch.get("process", {}).get("processIdentifier")
    process = selected_process(args.device, args.output, pid, installed)
    write_json(args.output / "preparation.json", {
        "version": 2, "job": "run", "device": args.device, "source": args.source,
        "configuration": "Debug", "artifacts": artifacts, "installed_app": installed,
        "installed_runner": runner, "installed_test_bundle": artifacts["products"]["test"], "process": process,
        "install_witness_sha256": products.digest(read_bytes(args.output / "install-witness.json", args.output)),
        "normal_launch_verified": True, "survival": "unknown",
    })


def preview_inputs(args, mode="preview"):
    """Hash only the fixed app (including its widget) at each boundary."""
    base = args.root / MODES[mode]["products"]
    app = base / "Release-iphoneos/Meguri.app"

    def allowed(name):
        return not name.endswith((".xctest", ".xctestrun", ".debug.dylib", "-Runner.app", ".xctrunner"))

    # Inspect product names without opening unrelated products or following
    # their links. Generated xctestrun files can sit beside Release-iphoneos.
    for directory in (base, app.parent):
        with products.authority(str(directory)) as fd:
            need(all(allowed(name) for name in os.listdir(fd)), "preview-test-products")
    inventory = products.snapshot(str(args.root / "build"), str(app))[0]
    need(all(entry["type"] != "symlink" and all(allowed(part) for part in name.split("/"))
             for name, entry in inventory.items()), "preview-test-products")
    stamp = read_bytes(args.root / "app/Packages/MeguriCore/Binaries/.stamp", args.root / "app").decode()
    settings = read_bytes(args.output.parent / MODES[mode]["settings"], args.output.parent)
    return {"device": args.device, "source": args.source, "configuration": "Release",
            "inventory": inventory, "core_stamp": stamp, "settings_sha256": products.digest(settings)}


def preview_artifacts(args, mode="preview"):
    """Verify typed metadata, signatures and device binaries only when sealing.

    The shell brackets its shared settings/profile/cloud policy with this seal.
    Installation compares the exact sealed inputs without repeating validation.
    """
    inputs = preview_inputs(args, mode)
    app_id = MODES[mode]["app"]
    app = args.root / MODES[mode]["products"] / "Release-iphoneos/Meguri.app"
    widget = app / "PlugIns/MeguriWidget.appex"
    artifacts = {}
    team = None
    for kind, path, identifier, executable in (("app", app, app_id, "Meguri"),
            ("widget", widget, app_id + ".widget", "MeguriWidget")):
        raw = read_bytes(path / "Info.plist", path)
        name = str((path / "Info.plist").relative_to(app))
        need(products.digest(raw) == inputs["inventory"][name]["sha256"], "product-changed")
        metadata = products.plist(raw, "product-info")
        need(metadata.get("CFBundleIdentifier") == identifier and
             metadata.get("CFBundleExecutable") == executable and
             metadata.get("CFBundleSupportedPlatforms") == ["iPhoneOS"] and
             all(isinstance(metadata.get(key), str) and metadata[key] for key in
                 ("CFBundleVersion", "CFBundleShortVersionString")), "preview-product-identity")
        if kind == "app":
            need(metadata.get("MeguriCardInspectionBuild") == "personalRelease", "preview-release")
        signed = signature(path, identifier, False)  # Not Debug's debugger-grant policy.
        need(team is None or team == signed["team"], "product-team")
        team = signed["team"]
        binary = str(path / executable)
        need(command(["/usr/bin/lipo", "-archs", binary], "preview-architecture").stdout.strip() == b"arm64",
             "preview-architecture")
        load = command(["/usr/bin/xcrun", "vtool", "-show-build", binary], "preview-platform").stdout.decode()
        platforms = re.findall(r"\bplatform\s+(\S+)", load)
        need(platforms and set(platforms) == {"IOS"}, "preview-platform")
        artifacts[kind] = {"path": str(path), "bundle_id": identifier, "build": metadata["CFBundleVersion"],
                           "version": metadata["CFBundleShortVersionString"], "signature": signed}
    need(all(artifacts["app"][key] == artifacts["widget"][key] for key in ("build", "version")),
         "preview-product-version")
    rows = [row.split("=", 1) for row in inputs["core_stamp"].splitlines()]
    need(all(len(row) == 2 for row in rows) and len({row[0] for row in rows}) == len(rows), "preview-core")
    core = dict(rows)
    need(core.get("configuration") == "Release" and core.get("profile") == "release" and
         core.get("features") == "ankiweb" and "device" in core.get("slices", "").split(",") and
         re.fullmatch(r"[a-f0-9]{32}", core.get("hash", "")), "preview-core")
    return dict(inputs, products=artifacts)


def preview_seal(args):
    write_json(args.output / "preview-artifacts.json", preview_artifacts(args))


def preview(args):
    artifacts = read_json(args.output / "preview-artifacts.json")
    sealed_inputs = {key: value for key, value in artifacts.items() if key != "products"}
    need(preview_inputs(args) == sealed_inputs, "preview-products-changed")
    app = artifacts["products"]["app"]
    need(app["path"] == str(args.root / PREVIEW_PRODUCTS / "Release-iphoneos/Meguri.app"),
         "preview-products-changed")
    device_query(args.device, args.output, ["device", "install", "app", app["path"]])
    installed = installation(args.device, args.output, APP)
    need(installed["bundleVersion"] == app["build"], "installed-product-build")
    need(preview_inputs(args) == sealed_inputs, "preview-products-changed")
    # Persist independently of launch: a locked phone can still prove install.
    # Different filenames/schema confer no Debug preparation/drive authority.
    write_json(args.output / "preview-install-witness.json", {
        "version": 1, "success": True, "device": args.device, "source": args.source,
        "configuration": "Release", "personal_preview": True, "artifacts": artifacts, "installed_app": installed})
    launch = device_query(args.device, args.output, ["device", "process", "launch", "--terminate-existing", APP])
    process = selected_process(args.device, args.output, launch.get("process", {}).get("processIdentifier"), installed)
    write_json(args.output / "preview-launch.json", {
        "version": 1, "configuration": "Release", "personal_preview": True,
        "device": args.device, "source": args.source, "process": process, "normal_launch_verified": True,
        "install_witness_sha256": products.digest(read_bytes(args.output / "preview-install-witness.json", args.output))})


def real_seal(args):
    write_json(args.output / "real-artifacts.json", preview_artifacts(args, "real"))


def real_install(args):
    """Install the sealed real copy and witness it; it is never launched."""
    artifacts = read_json(args.output / "real-artifacts.json")
    sealed_inputs = {key: value for key, value in artifacts.items() if key != "products"}
    need(preview_inputs(args, "real") == sealed_inputs, "real-products-changed")
    app = artifacts["products"]["app"]
    need(app["path"] == str(args.root / REAL_PRODUCTS / "Release-iphoneos/Meguri.app") and app["bundle_id"] == DAILY,
         "real-products-changed")
    device_query(args.device, args.output, ["device", "install", "app", app["path"]])
    installed = installation(args.device, args.output, DAILY)
    need(installed["bundleVersion"] == app["build"], "installed-product-build")
    need(preview_inputs(args, "real") == sealed_inputs, "real-products-changed")
    write_json(args.output / "real-install-witness.json", {
        "version": 1, "success": True, "device": args.device, "source": args.source,
        "configuration": "Release", "real_copy": True, "launched": False,
        "artifacts": artifacts, "installed_app": installed})


def matching_artifacts(args, prepared):
    current, generated = local_artifacts(args.root, Path(prepared["artifacts"]["xctestrun"]))
    app = installation(args.device, args.output, APP)
    runner = installation(args.device, args.output, RUNNER)
    phase = "completion" if (args.output / "admission.json").exists() else "admission"
    write_json(args.output / (phase + "-artifacts.json"), {
        "artifacts": current, "installed_app": app, "installed_runner": runner,
        "products_match": current == prepared["artifacts"],
        "app_matches": app == prepared["installed_app"], "runner_matches": runner == prepared["installed_runner"]})
    need(current == prepared["artifacts"], "matching-products")
    need(app == prepared["installed_app"], "matching-installed-app")
    need(runner == prepared["installed_runner"], "matching-installed-runner")
    return generated


def admit(args):
    # Resolve the sole trusted producer once. No caller token or older receipt
    # search grants preservation. Reading it identifies the PID to bracket.
    run = (args.root / "build/mg/run/latest").resolve(strict=True)
    products.contained(str(args.root / "build/mg/run"), str(run))
    receipt = read_json(run / "result.json")
    need(receipt.get("job") == "run" and receipt.get("status") == "pass" and
         receipt.get("dry_run") is False and receipt.get("root") == str(args.root) and
         receipt.get("placement") == "host" and not receipt.get("failures"), "trusted-preparation")
    prepared = read_json(run / "physical/preparation.json")
    need(prepared.get("version") == 2 and prepared.get("job") == "run" and
         prepared.get("configuration") == "Debug" and prepared.get("normal_launch_verified") is True and
         prepared.get("device") == args.device and prepared.get("source") == args.source, "matching-preparation")
    # This first selected observation precedes inventory, lease, artifact
    # verification and runner startup. Completion takes the other bracket.
    before = selected_process(args.device, args.output, prepared["process"]["processIdentifier"], prepared["installed_app"])
    write_json(args.output / "before.json", before)
    need(before == prepared["process"], "selected-process-changed")
    raw = read_bytes(run / "physical/install-witness.json", run / "physical")
    need(products.digest(raw) == prepared["install_witness_sha256"], "install-witness")
    need(json.loads(raw) == {"success": True, "device": args.device, "source": args.source,
         "artifacts": prepared["artifacts"], "installed_app": prepared["installed_app"],
         "installed_runner": prepared["installed_runner"]} and
         prepared["installed_test_bundle"] == prepared["artifacts"]["products"]["test"], "install-witness")
    generated = matching_artifacts(args, prepared)
    with (args.output / "destination.xctestrun").open("xb") as stream:
        plistlib.dump(destination_xctestrun(generated, RUNNER, "__TESTHOST__/PlugIns/MeguriUITests.xctest"), stream)
    write_json(args.output / "admission.json", {"prepared": prepared, "before": before})


def complete(args):
    # Always called after the drive, including failed selected tests, export,
    # capture or hierarchy checks. Retention never changes the drive's verdict.
    evidence = {"kept_running": True, "activated_existing": False,
                "continuity": {"status": "unknown", "reason": "completion-evidence-unavailable"},
                "installed_test_bundle_verified": False}
    try:
        admitted = read_json(args.output / "admission.json")
        prepared, before = admitted["prepared"], admitted["before"]
        evidence["before"] = before
        need(args.device == prepared["device"] and args.source == prepared["source"], "matching-preparation")
        matching_artifacts(args, prepared)
        after = selected_process(args.device, args.output, before["processIdentifier"], prepared["installed_app"])
        write_json(args.output / "after.json", after)
        evidence["after"] = after
        evidence["continuity"] = continuity(before, after)
        need(evidence["continuity"]["status"] == "verified", "selected-process-changed")
        # Missing diagnostics or observed transport loss/restart invalidates
        # even matching process brackets. An ordinary failed step does not.
        diagnostic = read_bytes(args.output.parent / "drive.log", args.output.parent).decode("utf-8", errors="replace")
        need(not re.search(r"(?:connection.{0,80}(?:lost|invalid|interrupt|closed)|lost.{0,40}connection|"
                           r"device.{0,80}(?:disconnected|unavailable)|disconnect|restarting|relaunching|"
                           r"runner.{0,80}(?:crashed|exited unexpectedly)|failed to establish.{0,40}connection)",
                           diagnostic, re.IGNORECASE), "transport-interruption")
        evidence["installed_test_bundle_verified"] = True
        log = read_json(args.output.parent / "drive-log.json")
        evidence["activated_existing"] = log.get("kept_running") is True and log.get("activation_completed") is True
        need(evidence["activated_existing"], "activation-unproved")
    except (Refused, products.Invalid, OSError, ValueError, TypeError, KeyError, AttributeError) as error:
        reason = str(error) if isinstance(error, Refused) else "completion-evidence-unavailable"
        # Keep changed brackets as a failure; missing/interrupted proof is unknown.
        evidence["continuity"] = {"status": "failed" if reason in
            ("selected-process-changed", "matching-products", "matching-installed-app", "matching-installed-runner")
            else "unknown", "reason": reason}
        write_json(args.output / "evidence.json", evidence)
        raise Refused(reason) from None
    write_json(args.output / "evidence.json", evidence)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "admit", "complete", "preview-seal", "preview",
                                           "real-seal", "real-install"))
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--source", required=True)
    parser.add_argument("--device", required=True)
    parser.add_argument("--xctestrun", type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        need(args.root.is_absolute() and args.output.is_absolute() and
             re.fullmatch(r"[a-f0-9]{64}", args.source) and
             re.fullmatch(r"(?:[a-fA-F0-9]{8}-[a-fA-F0-9]{16}|[a-fA-F0-9]{8}(?:-[a-fA-F0-9]{4}){3}-[a-fA-F0-9]{12})", args.device),
             "physical-arguments")
        products.contained(str(args.root / "build/mg"), str(args.output))
        if args.action in ("preview-seal", "preview"):
            need(args.xctestrun is None and args.output.name == "personal-preview" and
                 args.output.parent.parent == args.root / "build/mg/run", "preview-arguments")
        if args.action in ("real-seal", "real-install"):
            need(args.xctestrun is None and args.output.name == "real-copy" and
                 args.output.parent.parent == args.root / "build/mg/run", "real-arguments")
        if args.action not in ("complete", "preview", "real-install"):
            args.output.mkdir(mode=0o700)
        if args.action == "prepare":
            need(args.xctestrun is not None, "prepared-xctestrun-missing")
            prepare(args)
        elif args.action == "preview-seal":
            preview_seal(args)
        elif args.action == "preview":
            preview(args)
        elif args.action == "real-seal":
            real_seal(args)
        elif args.action == "real-install":
            real_install(args)
        elif args.action == "admit":
            admit(args)
        else:
            complete(args)
    except (Refused, products.Invalid, OSError, ValueError, TypeError, KeyError, AttributeError, UnicodeError, RecursionError) as error:
        # Paths, signing identities, device responses and exceptions stay private.
        # The shell supplies the fixed public refusal explanation.
        if args.output.is_dir():
            category = str(error) if isinstance(error, Refused) else "physical-evidence-unavailable"
            try:
                write_json(args.output / "refusal.json", {"reason": category,
                    "kept_running": args.action in ("admit", "complete"), "activated_existing": False, "continuity": "unknown"})
            except OSError:
                pass
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
