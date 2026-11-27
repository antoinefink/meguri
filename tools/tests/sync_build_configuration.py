#!/usr/bin/env python3
"""Check that every Apple product excludes the retired iCloud service."""
import argparse
import json
import plistlib
import subprocess
import sys
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def protected(key):
    return any(term in key.lower() for term in ("icloud", "ubiquity", "aps-environment"))


def check(settings, configuration, platform, signing, info=None, entitlements=None, widget=None,
          personal_preview=False, real_copy=False, target="Meguri"):
    require(configuration in ("Debug", "Profile", "Release", "AppStore"), "unknown configuration")
    require(platform in ("mac", "ios") and signing in ("adhoc", "team"), "unknown platform/signing")
    require(not personal_preview or (configuration == "Release" and signing == "team"), "preview requires team Release")
    require(not real_copy or (configuration == "Release" and signing == "team" and not personal_preview), "real copy requires team Release")
    require(isinstance(settings, dict), "settings must be a dictionary")
    suffix = ".dev" if configuration in ("Debug", "Profile") or personal_preview else ""
    require(settings.get("MEGURI_BUNDLE_ID_SUFFIX", "") == suffix, "unexpected bundle suffix")
    require(settings.get("PRODUCT_BUNDLE_IDENTIFIER") == "com.antoinefink.meguri" + suffix + (".widget" if target == "MeguriWidget" else ""), "unexpected target identity")
    for key, value in settings.items():
        require(not key.startswith("MEGURI_CLOUD") and "MeguriCloud" not in key, "retired cloud build setting")
        if key in ("OTHER_LDFLAGS", "OTHER_SWIFT_FLAGS") or key.startswith("INFOPLIST_KEY_"):
            require(not any(term in (key + str(value)).lower() for term in ("cloudkit", "meguricloud", "remote-notification", "icloud", "ubiquity", "aps-environment")), "retired framework or Info setting")
    for artifact, label in ((info, "Info"), (entitlements, "app entitlement"), (widget, "widget entitlement")):
        if artifact is not None:
            check_plist(artifact, label)


def check_plist(artifact, label):
    require(isinstance(artifact, dict), "plist must be a dictionary")
    require(not any(protected(key) or key.startswith("MeguriCloud") for key in artifact), "retired " + label)
    require("remote-notification" not in artifact.get("UIBackgroundModes", []), "retired remote notifications")


def check_binary(path):
    # otool does not recurse: explicitly inspect the implementation dylib too.
    require(path.is_file(), "missing executable")
    result = subprocess.run(["otool", "-L", str(path)], check=True, capture_output=True, text=True)
    require("cloudkit.framework" not in result.stdout.lower(), "product links CloudKit")
    # The retired app-injected agent linked XCTest into the app itself. Ordinary
    # XCTest runners are outside these app/widget executable paths.
    require(not any(name in result.stdout.lower() for name in ("xctest.framework", "libxctestbundleinject.dylib")), "stale app XCTest injection")


def product_binaries(app, platform):
    prefix = "Contents/MacOS" if platform == "mac" else ""
    widget = app / ("Contents/PlugIns" if platform == "mac" else "PlugIns") / "MeguriWidget.appex"
    for bundle, name in ((app, "Meguri"), (widget, "MeguriWidget")):
        yield bundle, bundle / prefix / name, False
        for dylib in sorted((bundle / prefix).glob("*.dylib")):
            yield bundle, dylib, True


def product_plist(path):
    result = subprocess.run(["plutil", "-convert", "json", "-o", "-", str(path)],
                            check=True, capture_output=True, text=True)
    return json.loads(result.stdout)


def check_product(app, platform, simulator=False):
    for bundle, binary, debug in product_binaries(app, platform):
        check_binary(binary)
        if not debug:
            check_plist(product_plist(bundle / ("Contents/Info.plist" if platform == "mac" else "Info.plist")), "Info")
        if simulator:
            # Simulator executables carry an embedded section. A dylib may have
            # none; establish absence from its load commands before skipping it.
            if debug:
                sections = subprocess.run(["otool", "-l", str(binary)], check=True, capture_output=True, text=True)
                if "__entitlements" not in sections.stdout:
                    continue
            import tempfile
            with tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "entitlements.plist"
                subprocess.run(["segedit", str(binary), "-extract", "__TEXT", "__entitlements", str(output)], check=True, capture_output=True)
                grants = plistlib.loads(output.read_bytes())
        else:
            # Inspect each signed executable; a provisioning profile can authorize
            # unused iCloud/push grants without those grants being signed here.
            result = subprocess.run(["codesign", "-d", "--entitlements", "-", "--xml", str(binary if debug else bundle)],
                                    check=True, capture_output=True)
            if result.stdout.strip():
                converted = subprocess.run(["plutil", "-convert", "json", "-o", "-", "-"], input=result.stdout,
                                           check=True, capture_output=True)
                grants = json.loads(converted.stdout)
            else:
                require(debug, "missing main executable grants")
                grants = {}
        check_plist(grants, "signed entitlement")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--settings", type=Path, required=True)
    parser.add_argument("--configuration", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--signing", required=True)
    parser.add_argument("--personal-preview", action="store_true")
    parser.add_argument("--real-copy", action="store_true")
    parser.add_argument("--app", type=Path)
    parser.add_argument("--simulator", action="store_true")
    parser.add_argument("--info", type=Path)
    parser.add_argument("--widget-info", type=Path)
    parser.add_argument("--entitlements", type=Path)
    parser.add_argument("--widget-entitlements", type=Path)
    parser.add_argument("--binary", action="append", type=Path, default=[])
    args = parser.parse_args()
    try:
        targets = json.loads(args.settings.read_text())
        for target in ("Meguri", "MeguriWidget"):
            entries = [entry["buildSettings"] for entry in targets if entry.get("target") == target]
            require(len(entries) == 1, "expected one of each product target")
            check(entries[0], args.configuration, args.platform, args.signing,
                  personal_preview=args.personal_preview, real_copy=args.real_copy, target=target)
        apps = [entry["buildSettings"] for entry in targets if entry.get("target") == "Meguri"]
        artifacts = [plistlib.loads(path.read_bytes()) if path else None
                     for path in (args.info, args.entitlements, args.widget_entitlements)]
        require(all(artifact is not None for artifact in artifacts) or all(artifact is None for artifact in artifacts), "provide all product plists together")
        check(apps[0], args.configuration, args.platform, args.signing, *artifacts,
              personal_preview=args.personal_preview, real_copy=args.real_copy)
        if args.widget_info:
            check_plist(plistlib.loads(args.widget_info.read_bytes()), "widget Info")
        require((args.info is None) == (args.widget_info is None), "provide both Info plists")
        if args.app:
            check_product(args.app, args.platform, args.simulator)
        for path in args.binary:
            check_binary(path)
    except (ValueError, KeyError, TypeError, OSError, subprocess.CalledProcessError, plistlib.InvalidFileException):
        print("Sync packaging check failed: iCloud must be absent and product evidence complete.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
