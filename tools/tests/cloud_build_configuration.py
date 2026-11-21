#!/usr/bin/env python3
"""Check resolved CloudKit settings and, when supplied, built plist artifacts.

This proves packaging consistency only, never account/profile/schema readiness.
"""

import argparse
import json
import plistlib
import re
import sys
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def protected(key):
    return key.startswith("com.apple.developer.icloud-") or key in (
        "aps-environment", "com.apple.developer.aps-environment",
        "com.apple.developer.ubiquity-container-identifiers",
        "com.apple.developer.ubiquity-kvstore-identifier",
    )


def check(settings, configuration, platform, signing, info=None, entitlements=None, widget=None, personal_preview=False,
          real_copy=False):
    require(isinstance(settings, dict), "resolved settings must be a dictionary")
    for artifact in (info, entitlements, widget):
        require(artifact is None or isinstance(artifact, dict), "plist artifact must be a dictionary")
    require(configuration in ("Debug", "Profile", "Release", "AppStore"), "unknown configuration")
    require(platform in ("mac", "ios") and signing in ("adhoc", "team"), "unknown platform/signing")
    require(not personal_preview or (configuration == "Release" and signing == "team"),
            "personal preview requires Release with team signing")
    # The owner-requested Mac real copy (build-app --real-copy) keeps production
    # CloudKit but signs development push, so an ordinary macOS development
    # profile authorizes it (standing decision 2026-11-21, exception to KTD22).
    require(not real_copy or (configuration == "Release" and signing == "team" and platform == "mac"
                              and not personal_preview), "the real copy is the team-signed Mac Release")
    development = configuration in ("Debug", "Profile") or personal_preview
    environment = "development" if development else "production"
    push = "development" if real_copy else environment
    suffix = ".dev" if development else ""
    require(settings.get("MEGURI_BUNDLE_ID_SUFFIX", "") == suffix, "unexpected bundle suffix for cloud policy")
    require(settings.get("PRODUCT_BUNDLE_IDENTIFIER") == "com.antoinefink.meguri" + suffix,
            "unexpected app identity for cloud policy")
    containers = [settings.get("MEGURI_CLOUD_CONTAINER_" + name, "")
                  for name in ("development", "production")]
    require(all(re.fullmatch(r"iCloud\.[A-Za-z0-9.-]+", value) for value in containers),
            "both explicit CloudKit container identifiers are required")
    require(containers[0] != containers[1], "development and production containers must differ")
    container = containers[0 if environment == "development" else 1]
    expected = {
        "MEGURI_CLOUD_CONFIGURATION": environment,
        "MEGURI_CLOUD_CONTAINER": container,
        "MEGURI_CLOUD_ENVIRONMENT": environment,
        "MEGURI_CLOUD_ENTITLEMENT_ENVIRONMENT": environment.capitalize(),
        "MEGURI_CLOUD_PUSH_ENVIRONMENT": push,
        "INFOPLIST_KEY_MeguriCloudContainer": container,
        "INFOPLIST_KEY_MeguriCloudEnvironment": environment,
    }
    for key, value in expected.items():
        require(settings.get(key) == value, f"{key}: expected {value!r}, got {settings.get(key)!r}")
    for name in ("development", "production"):
        require(settings.get("MEGURI_CLOUD_SCHEMA_CONFIGURED_" + name) in ("NO", "YES"),
                f"{name} readiness must be explicit NO or YES")
    ready = signing == "team" and settings["MEGURI_CLOUD_SCHEMA_CONFIGURED_" + environment] == "YES"
    require(settings.get("MEGURI_CLOUD_SCHEMA_CONFIGURED") == ("YES" if ready else "NO"),
            "effective schema readiness differs from the selected signing/environment policy")
    require(settings.get("MEGURI_CLOUD_SCHEMA_BOOLEAN") == ("1" if ready else "0"),
            "schema readiness must expand to a plist Boolean element")
    if info is not None:
        require(info.get("MeguriCloudContainer") == container, "built Info container mismatch")
        require(info.get("MeguriCloudEnvironment") == environment, "built Info environment mismatch")
        value = info.get("MeguriCloudSchemaConfigured")
        require(type(value) is bool, "built schema readiness must be Boolean, not a string/number")
        require(value == ready, "built schema readiness mismatch")
    if entitlements is not None:
        actual = {key: value for key, value in entitlements.items() if protected(key)}
        grants = {}
        if platform == "ios" or signing == "team":
            grants = {
                "com.apple.developer.icloud-container-identifiers": [container],
                "com.apple.developer.icloud-services": ["CloudKit"],
                "com.apple.developer.icloud-container-environment": environment.capitalize(),
                ("aps-environment" if platform == "ios" else "com.apple.developer.aps-environment"): push,
            }
        require(actual == grants, f"built protected grants differ: expected {grants!r}, got {actual!r}")
    if widget is not None:
        require(not any(protected(key) for key in widget), "widget carries protected cloud/push grants")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--settings", type=Path, required=True)
    parser.add_argument("--configuration", choices=("Debug", "Profile", "Release", "AppStore"), required=True)
    parser.add_argument("--platform", choices=("mac", "ios"), required=True)
    parser.add_argument("--signing", choices=("adhoc", "team"), required=True)
    parser.add_argument("--personal-preview", action="store_true")
    parser.add_argument("--real-copy", action="store_true")
    parser.add_argument("--info", type=Path)
    parser.add_argument("--entitlements", type=Path)
    parser.add_argument("--widget-entitlements", type=Path)
    args = parser.parse_args()
    try:
        targets = json.loads(args.settings.read_text())
        require(isinstance(targets, list) and all(isinstance(entry, dict) for entry in targets),
                "resolved settings must be an Xcode target array")
        apps = [entry["buildSettings"] for entry in targets if entry.get("target") == "Meguri"]
        require(len(apps) == 1, "expected exactly one Meguri target in resolved settings")
        artifacts = []
        require(all((args.info, args.entitlements, args.widget_entitlements)) or
                not any((args.info, args.entitlements, args.widget_entitlements)),
                "artifact proof requires Info, app entitlements and widget entitlements together")
        for path in (args.info, args.entitlements, args.widget_entitlements):
            artifacts.append(plistlib.loads(path.read_bytes()) if path else None)
        check(apps[0], args.configuration, args.platform, args.signing, *artifacts, personal_preview=args.personal_preview,
              real_copy=args.real_copy)
    except (ValueError, KeyError, TypeError, OSError, plistlib.InvalidFileException) as error:
        print(f"CloudKit configuration: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
