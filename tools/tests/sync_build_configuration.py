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
          personal_preview=False, real_copy=False):
    require(configuration in ("Debug", "Profile", "Release", "AppStore"), "unknown configuration")
    require(platform in ("mac", "ios") and signing in ("adhoc", "team"), "unknown platform/signing")
    require(not personal_preview or (configuration == "Release" and signing == "team"), "preview requires team Release")
    require(not real_copy or (configuration == "Release" and signing == "team" and not personal_preview), "real copy requires team Release")
    require(isinstance(settings, dict), "settings must be a dictionary")
    suffix = ".dev" if configuration in ("Debug", "Profile") or personal_preview else ""
    require(settings.get("MEGURI_BUNDLE_ID_SUFFIX", "") == suffix, "unexpected bundle suffix")
    require(settings.get("PRODUCT_BUNDLE_IDENTIFIER") == "com.antoinefink.meguri" + suffix, "unexpected app identity")
    for key, value in settings.items():
        require(not key.startswith("MEGURI_CLOUD") and "MeguriCloud" not in key, "retired cloud build setting")
        if key in ("OTHER_LDFLAGS", "OTHER_SWIFT_FLAGS"):
            require("cloudkit" not in str(value).lower(), "retired framework linker setting")
    for artifact, label in ((info, "Info"), (entitlements, "app entitlement"), (widget, "widget entitlement")):
        require(artifact is None or isinstance(artifact, dict), "plist must be a dictionary")
        if artifact is not None:
            require(not any(protected(key) or key.startswith("MeguriCloud") for key in artifact), "retired " + label)
    if info is not None:
        require("remote-notification" not in info.get("UIBackgroundModes", []), "retired remote notifications")


def check_binary(path):
    # -L lists each Mach-O slice's dependencies; includes Swift autolinking.
    result = subprocess.run(["otool", "-L", str(path)], check=True, capture_output=True, text=True)
    require("CloudKit.framework" not in result.stdout, "product links CloudKit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--settings", type=Path, required=True)
    parser.add_argument("--configuration", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--signing", required=True)
    parser.add_argument("--personal-preview", action="store_true")
    parser.add_argument("--real-copy", action="store_true")
    parser.add_argument("--info", type=Path)
    parser.add_argument("--entitlements", type=Path)
    parser.add_argument("--widget-entitlements", type=Path)
    parser.add_argument("--binary", action="append", type=Path, default=[])
    args = parser.parse_args()
    try:
        targets = json.loads(args.settings.read_text())
        apps = [entry["buildSettings"] for entry in targets if entry.get("target") == "Meguri"]
        require(len(apps) == 1, "expected one app target")
        artifacts = [plistlib.loads(path.read_bytes()) if path else None
                     for path in (args.info, args.entitlements, args.widget_entitlements)]
        require(all(artifact is not None for artifact in artifacts) or all(artifact is None for artifact in artifacts), "provide all product plists together")
        check(apps[0], args.configuration, args.platform, args.signing, *artifacts,
              personal_preview=args.personal_preview, real_copy=args.real_copy)
        for path in args.binary:
            check_binary(path)
    except (ValueError, KeyError, TypeError, OSError, subprocess.CalledProcessError, plistlib.InvalidFileException):
        print("Sync packaging check failed: iCloud must be absent and product evidence complete.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
