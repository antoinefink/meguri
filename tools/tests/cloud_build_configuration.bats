#!/usr/bin/env bats
# Focused configuration/packaging guard regressions; native artifacts are
# checked by xcode-check. Fixtures here are not signed-account evidence.

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudKit artifact verifier covers configurations signing suffixes and typed readiness" {
  python3 - "$REPO_ROOT" <<'PY'
import importlib.util
import itertools
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
path = root / "tools/tests/cloud_build_configuration.py"
spec = importlib.util.spec_from_file_location("cloud_configuration", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

def rejected(settings, config, platform, signing, info, ent, widget, preview=False):
    try:
        module.check(settings, config, platform, signing, info, ent, widget, personal_preview=preview)
    except ValueError:
        return
    raise AssertionError((config, platform, signing, settings, info, ent, widget))

with tempfile.TemporaryDirectory() as temporary:
    directory = Path(temporary)
    for config, platform, signing, preview, setup in itertools.product(
            ("Debug", "Profile", "Release", "AppStore"), ("mac", "ios"),
            ("adhoc", "team"), (False, True), (False, True)):
        if preview and (config, platform, signing) != ("Release", "mac", "team"):
            rejected({}, config, platform, signing, None, None, None, preview=True)
            continue
        environment = "development" if config in ("Debug", "Profile") or preview else "production"
        suffix = ".dev" if environment == "development" else ""
        container = "iCloud.example.isolated" if environment == "development" else "iCloud.example.daily"
        ready = signing == "team" and setup
        settings = {
            "PRODUCT_BUNDLE_IDENTIFIER": "com.antoinefink.meguri" + suffix,
            "MEGURI_BUNDLE_ID_SUFFIX": suffix,
            "MEGURI_CLOUD_CONTAINER_development": "iCloud.example.isolated",
            "MEGURI_CLOUD_CONTAINER_production": "iCloud.example.daily",
            "MEGURI_CLOUD_CONFIGURATION": environment,
            "MEGURI_CLOUD_CONTAINER": container,
            "MEGURI_CLOUD_ENVIRONMENT": environment,
            "MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT": environment.capitalize(),
            "MEGURI_CLOUD_PUSH_ENVIRONMENT": environment,
            "INFOPLIST_KEY_MeguriCloudContainer": container,
            "INFOPLIST_KEY_MeguriCloudEnvironment": environment,
            "MEGURI_CLOUD_SCHEMA_CONFIGURED_development": "YES" if setup else "NO",
            "MEGURI_CLOUD_SCHEMA_CONFIGURED_production": "YES" if setup else "NO",
            "MEGURI_CLOUD_SCHEMA_CONFIGURED": "YES" if ready else "NO",
            "MEGURI_CLOUD_SCHEMA_BOOLEAN": "1" if ready else "0",
        }
        info = {"MeguriCloudContainer": container, "MeguriCloudEnvironment": environment,
                "MeguriCloudSchemaConfigured": ready}
        push = "aps-environment" if platform == "ios" else "com.apple.developer.aps-environment"
        grants = {
            "com.apple.developer.icloud-container-identifiers": [container],
            "com.apple.developer.icloud-services": ["CloudKit"],
            "com.apple.developer.icloud-container-environment": environment.capitalize(),
            push: environment,
        }
        ent = grants if platform == "ios" or signing == "team" else {}
        module.check(settings, config, platform, signing, info, ent, {}, personal_preview=preview)
        for key, value in (("MeguriCloudSchemaConfigured", "YES"),
                           ("MeguriCloudSchemaConfigured", "NO"),
                           ("MeguriCloudSchemaConfigured", 0),
                           ("MeguriCloudSchemaConfigured", not ready),
                           ("MeguriCloudContainer", "iCloud.wrong"),
                           ("MeguriCloudEnvironment", "production" if environment == "development" else "development")):
            rejected(settings, config, platform, signing, dict(info, **{key: value}), ent, {}, preview)
        missing = dict(info)
        del missing["MeguriCloudSchemaConfigured"]
        rejected(settings, config, platform, signing, missing, ent, {}, preview)
        for key in grants:
            changed = dict(ent)
            if key in changed:
                del changed[key]
            else:
                changed[key] = grants[key]
            rejected(settings, config, platform, signing, info, changed, {}, preview)
        for key in ("com.apple.developer.icloud-container-identifiers",
                    "com.apple.developer.icloud-services", "aps-environment",
                    "com.apple.developer.aps-environment"):
            rejected(settings, config, platform, signing, info, ent, {key: "unexpected"}, preview)
        for key in ("MEGURI_CLOUD_CONTAINER", "MEGURI_CLOUD_ENVIRONMENT",
                    "MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT", "MEGURI_CLOUD_PUSH_ENVIRONMENT",
                    "INFOPLIST_KEY_MeguriCloudContainer", "INFOPLIST_KEY_MeguriCloudEnvironment",
                    "MEGURI_CLOUD_CONFIGURATION", "MEGURI_BUNDLE_ID_SUFFIX", "PRODUCT_BUNDLE_IDENTIFIER"):
            rejected(dict(settings, **{key: "wrong"}), config, platform, signing, info, ent, {}, preview)
        bad = dict(settings, MEGURI_CLOUD_CONTAINER_development="iCloud.example.daily")
        rejected(bad, config, platform, signing, info, ent, {}, preview)

        # Keep development/test destinations isolated even with matching fields.
        # Personal preview needs an explicit development selection beyond .dev.
        redirected = dict(settings, MEGURI_CLOUD_CONFIGURATION="production",
                          MEGURI_CLOUD_CONTAINER="iCloud.example.daily",
                          MEGURI_CLOUD_ENVIRONMENT="production",
                          MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT="Production",
                          MEGURI_CLOUD_PUSH_ENVIRONMENT="production",
                          INFOPLIST_KEY_MeguriCloudContainer="iCloud.example.daily",
                          INFOPLIST_KEY_MeguriCloudEnvironment="production")
        if environment == "development":
            rejected(redirected, config, platform, signing, None, None, None, preview)
        if preview:
            rejected(settings, config, platform, signing, info, ent, {}, False)
        for bad_suffix in (".unexpected", "" if suffix else ".dev"):
            rejected(dict(settings, MEGURI_BUNDLE_ID_SUFFIX=bad_suffix,
                          PRODUCT_BUNDLE_IDENTIFIER="com.antoinefink.meguri" + bad_suffix),
                     config, platform, signing, info, ent, {}, preview)

        # Exercise the CLI with binary Info and XML signed/embedded entitlement
        # fixtures. This catches parsing/type errors beyond in-memory checks.
        (directory / "settings.json").write_text(json.dumps([{"target": "Meguri", "buildSettings": settings}]))
        for name, value in (("Info", info), ("app", ent), ("widget", {})):
            (directory / (name + ".plist")).write_bytes(plistlib.dumps(
                value, fmt=plistlib.FMT_BINARY if name == "Info" else plistlib.FMT_XML))
        args = [sys.executable, str(path), "--settings", str(directory / "settings.json"),
                "--configuration", config, "--platform", platform, "--signing", signing,
                "--info", str(directory / "Info.plist"), "--entitlements", str(directory / "app.plist"),
                "--widget-entitlements", str(directory / "widget.plist")]
        if preview:
            args.append("--personal-preview")
        subprocess.run(args, check=True)
    (directory / "Info.plist").write_bytes(plistlib.dumps(dict(info, MeguriCloudSchemaConfigured="NO")))
    result = subprocess.run(args, capture_output=True, text=True)
    assert result.returncode == 1 and "Boolean" in result.stderr, result
PY
}

@test "CloudKit source readiness is explicit and restricted grants stay out of Mac adhoc previews and widgets" {
  python3 - "$REPO_ROOT" <<'PY'
from pathlib import Path
import plistlib
import re
import sys

root = Path(sys.argv[1])
cloud = (root / "app/Config/Cloud.xcconfig").read_text()
for environment in ("development", "production"):
    assert re.search(r"^MEGURI_CLOUD_SCHEMA_CONFIGURED_" + environment + r" = (NO|YES)$", cloud, re.M)
assert "MEGURI_CLOUD_SCHEMA_CONFIGURED_adhoc = NO" in cloud
assert "MEGURI_CLOUD_SCHEMA_CONFIGURED_team = $(MEGURI_CLOUD_SCHEMA_CONFIGURED_$(MEGURI_CLOUD_CONFIGURATION))" in cloud
for config, environment in (("Debug", "development"), ("Profile", "development"), ("Release", "production")):
    assert f"MEGURI_CLOUD_CONFIGURATION = {environment}" in (root / f"app/Config/{config}.xcconfig").read_text()
for folder in ("Meguri", "MeguriWidget"):
    for path in (root / "app" / folder).glob("*.entitlements"):
        ent = plistlib.loads(path.read_bytes())
        restricted = {key for key in ent if key.startswith("com.apple.developer.icloud-") or "aps-environment" in key}
        if folder == "MeguriWidget" or "AdHoc" in path.name:
            assert not restricted, path
        else:
            push = "aps-environment" if path.name == "Meguri-iOS.entitlements" else "com.apple.developer.aps-environment"
            assert restricted == {push, "com.apple.developer.icloud-container-identifiers",
                                  "com.apple.developer.icloud-services",
                                  "com.apple.developer.icloud-container-environment"}, path
            assert ent["com.apple.developer.icloud-container-identifiers"] == ["$(MEGURI_CLOUD_CONTAINER)"], path
            assert ent[push] == "$(MEGURI_CLOUD_PUSH_ENVIRONMENT)", path
PY
}

@test "both actual Info inputs preprocess readiness into typed false and verified true" {
  python3 - "$REPO_ROOT" <<'PY_INPUT'
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys

root = Path(sys.argv[1])
config = (root / "app/Config/Meguri.xcconfig").read_text()
cloud = (root / "app/Config/Cloud.xcconfig").read_text()
assert "INFOPLIST_PREPROCESS = YES" in config
assert "INFOPLIST_OTHER_PREPROCESSOR_FLAGS = $(inherited) -traditional" in config
assert "INFOPLIST_PREPROCESSOR_DEFINITIONS = $(inherited) MEGURI_CLOUD_SCHEMA_BOOLEAN=$(MEGURI_CLOUD_SCHEMA_BOOLEAN)" in config
assert "INFOPLIST_KEY_MeguriCloudSchemaConfigured" not in config
compiler = shutil.which("clang") or shutil.which("cc")
assert compiler, "C preprocessor required for the source Info contract"
substitutions = {"MeguriCloudContainer": "$(MEGURI_CLOUD_CONTAINER)",
                 "MeguriCloudEnvironment": "$(MEGURI_CLOUD_ENVIRONMENT)"}
for name, sdk in (("Info-iOS.plist", "iphone*"), ("Info-macOS.plist", "macosx*")):
    assert f"INFOPLIST_FILE[sdk={sdk}] = Meguri/{name}" in config
    path = root / "app/Meguri" / name
    source = path.read_text()
    assert source.count("<key>MeguriCloudSchemaConfigured</key>") == 1
    for key, value in substitutions.items():
        assert source.count(f"<key>{key}</key>") == 1
        assert f"<key>{key}</key>\n\t<string>{value}</string>" in source
    for setting, expected in (("NO", False), ("YES", True)):
        macro = re.search(r"^MEGURI_CLOUD_SCHEMA_BOOLEAN_" + setting + r" = ([01])$", cloud, re.M)[1]
        result = subprocess.run([compiler, "-E", "-P", "-x", "c", "-traditional",
                                 "-DMEGURI_CLOUD_SCHEMA_BOOLEAN=" + macro, str(path)],
                                check=True, capture_output=True)
        # GCC may prefix blank lines from its implicit stdc-predef header.
        actual = plistlib.loads(result.stdout.lstrip())
        if name == "Info-iOS.plist":
            assert actual["UIBackgroundModes"] == ["remote-notification"]
        assert type(actual["MeguriCloudSchemaConfigured"]) is bool
        assert actual["MeguriCloudSchemaConfigured"] is expected
        for key, value in substitutions.items():
            assert actual[key] == value
        # Every other source key, including URL/path text, must survive CPP.
        literal = re.sub(r"#if MEGURI_CLOUD_SCHEMA_BOOLEAN\n.*?#endif",
                         "<true/>" if expected else "<false/>", source, flags=re.S)
        assert actual == plistlib.loads(literal.encode())
PY_INPUT
}
