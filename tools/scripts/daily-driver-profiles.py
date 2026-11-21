#!/usr/bin/env python3
"""Installed provisioning profiles for the daily-driver identity (the real copy).

Read-only: it decodes the profiles Xcode installed (`security cms -D -i`,
which modifies no keychain) and reports, per platform, whether one authorizes
the app's signed claims: the identity `<team>.com.antoinefink.meguri`, its
App Group and Keychain group, the production CloudKit container
`iCloud.com.antoinefink.meguri` in the Production environment, and
development push (the real copy signs development push: standing decision
2026-11-21). It never requests, downloads or registers anything (one-time step
`daily-driver-account`, docs/process/one-time-setup.md).

  daily-driver-profiles.py [--require mac|ios] [--team TEAM] [--device UDID]
      prints {"mac": {...}, "ios": {...}} on stdout; with --require, exit 1
      unless that platform's app profile authorizes every claim. Each entry
      has state authorized | incomplete | missing, the missing grants of the
      closest profile, and its expiry (no names, devices or certificates).

Profile directories: MEGURI_PROFILE_DIRS (colon-separated), else Xcode's two.
Stdlib only: doctor and build-app run it with the host's python3.
"""

import argparse
import datetime
import fnmatch
import json
import os
import plistlib
import subprocess
import sys

BUNDLE = "com.antoinefink.meguri"
CONTAINER = "iCloud.com.antoinefink.meguri"
DEFAULT_DIRS = ["~/Library/Developer/Xcode/UserData/Provisioning Profiles",
                "~/Library/MobileDevice/Provisioning Profiles"]


def profiles():
    dirs = os.environ.get("MEGURI_PROFILE_DIRS")
    dirs = dirs.split(":") if dirs else [os.path.expanduser(d) for d in DEFAULT_DIRS]
    for directory in dirs:
        if not os.path.isdir(directory):
            continue
        for name in sorted(os.listdir(directory)):
            if not name.endswith((".provisionprofile", ".mobileprovision")):
                continue
            path = os.path.join(directory, name)
            try:
                decoded = subprocess.run(["security", "cms", "-D", "-i", path],
                                         check=True, capture_output=True).stdout
                yield plistlib.loads(decoded)
            except (subprocess.CalledProcessError, OSError, ValueError, plistlib.InvalidFileException):
                continue


def authorizes(grant, claim, choices=None):
    """Whether a profile grant covers one signed claim."""
    if grant is None:
        return False
    if grant == "*" or grant == claim:
        return True
    if isinstance(grant, list):
        return claim in grant or any(isinstance(g, str) and isinstance(claim, str) and fnmatch.fnmatchcase(claim, g)
                                     for g in grant)
    return isinstance(grant, str) and isinstance(claim, str) and fnmatch.fnmatchcase(claim, grant)


def missing_grants(profile, platform, team):
    grants = profile.get("Entitlements", {})
    identity = grants.get("com.apple.application-identifier", grants.get("application-identifier"))
    group = (team + "." + BUNDLE) if platform == "mac" else ("group." + BUNDLE)
    push = "com.apple.developer.aps-environment" if platform == "mac" else "aps-environment"
    checks = [
        ("identity", authorizes(identity, team + "." + BUNDLE)),
        ("app-group", authorizes(grants.get("com.apple.security.application-groups"), group)),
        ("keychain", authorizes(grants.get("keychain-access-groups"), team + "." + BUNDLE)),
        # Exactly the production container: the signed claim lists only it,
        # and the product check compares the two lists.
        ("icloud-container", grants.get("com.apple.developer.icloud-container-identifiers") == [CONTAINER]),
        ("icloud-services", authorizes(grants.get("com.apple.developer.icloud-services"), "CloudKit")),
        ("icloud-production", authorizes(grants.get("com.apple.developer.icloud-container-environment"), "Production")),
        ("push-development", authorizes(grants.get(push), "development")),
    ]
    return [name for name, ok in checks if not ok]


def summary(platform, team, device):
    wanted = "OSX" if platform == "mac" else "iOS"
    now = datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None)
    best = None
    for profile in profiles():
        if wanted not in profile.get("Platform", []) or team not in profile.get("TeamIdentifier", []):
            continue
        grants = profile.get("Entitlements", {})
        identity = grants.get("com.apple.application-identifier", grants.get("application-identifier"))
        # Only profiles naming this app (or the team wildcard) can sign it.
        if not authorizes(identity, team + "." + BUNDLE):
            continue
        expires = profile.get("ExpirationDate")
        missing = missing_grants(profile, platform, team)
        if not isinstance(expires, datetime.datetime) or expires <= now:
            missing.append("not-expired")
        # Development profiles list their devices (the product check requires it).
        if device and device not in profile.get("ProvisionedDevices", []):
            missing.append("this-device")
        entry = {"state": "authorized" if not missing else "incomplete", "missing": missing,
                 "expires": expires.strftime("%Y-%m-%d") if isinstance(expires, datetime.datetime) else None,
                 "exact_identity": identity == team + "." + BUNDLE}
        if best is None or len(entry["missing"]) < len(best["missing"]):
            best = entry
    return best or {"state": "missing", "missing": ["profile"], "expires": None, "exact_identity": False}


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--require", choices=["mac", "ios"])
    parser.add_argument("--team", default="DNQ34W43XX")
    parser.add_argument("--device", default="", help="require coverage of this provisioning UDID (Mac)")
    args = parser.parse_args(argv)
    report = {"mac": summary("mac", args.team, args.device), "ios": summary("ios", args.team, "")}
    print(json.dumps(report, sort_keys=True))
    if args.require and report[args.require]["state"] != "authorized":
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
