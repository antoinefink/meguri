#!/usr/bin/env python3
"""Installed provisioning profiles for the daily-driver identity (the real copy).

Read-only: it decodes the profiles Xcode installed (`security cms -D -i`,
which modifies no keychain) and reports, per platform, whether one authorizes
the app's signed claims: the identity `<team>.com.antoinefink.meguri`, its
App Group and Keychain group. It never requests, downloads or registers anything (one-time step
`daily-driver-account`, docs/process/one-time-setup.md).

  daily-driver-profiles.py [--require mac|ios] [--team TEAM] [--device UDID] [--ios-device UDID]
      prints {"mac": {...}, "ios": {...}, "ios_widget": {...}} on stdout; with
      --require, exit 1 unless that platform's app profile (and on iOS the
      widget's) authorizes every claim and covers the given device. Each entry
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
import re
import subprocess
import sys

BUNDLE = "com.antoinefink.meguri"
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


def missing_grants(profile, platform, team, bundle):
    grants = profile.get("Entitlements", {})
    identity = grants.get("com.apple.application-identifier", grants.get("application-identifier"))
    group = (team + "." + BUNDLE) if platform == "mac" else ("group." + BUNDLE)
    checks = [
        ("identity", authorizes(identity, team + "." + bundle)),
        ("app-group", authorizes(grants.get("com.apple.security.application-groups"), group)),
    ]
    if bundle != BUNDLE:
        # The widget signs only its identity and the App Group.
        return [name for name, ok in checks if not ok]
    checks += [
        ("keychain", authorizes(grants.get("keychain-access-groups"), team + "." + BUNDLE)),
    ]
    return [name for name, ok in checks if not ok]


def summary(platform, team, device, bundle=BUNDLE):
    wanted = "OSX" if platform == "mac" else "iOS"
    now = datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None)
    best = best_rank = None
    for profile in profiles():
        if wanted not in profile.get("Platform", []) or team not in profile.get("TeamIdentifier", []):
            continue
        grants = profile.get("Entitlements", {})
        identity = grants.get("com.apple.application-identifier", grants.get("application-identifier"))
        # Only profiles naming this bundle (or the team wildcard) can sign it.
        if not authorizes(identity, team + "." + bundle):
            continue
        expires = profile.get("ExpirationDate")
        missing = missing_grants(profile, platform, team, bundle)
        if not isinstance(expires, datetime.datetime) or expires <= now:
            missing.append("not-expired")
        # Development profiles list their devices (the product check requires it).
        if device and device not in profile.get("ProvisionedDevices", []):
            missing.append("this-device")
        # Manual signing refuses Xcode-managed profiles.
        if profile.get("IsXcodeManaged") is True:
            missing.append("manual-profile")
        uuid = profile.get("UUID")
        if not isinstance(uuid, str) or not re.fullmatch(r"[0-9A-Fa-f-]{36}", uuid):
            missing.append("uuid")
        # The route signs manually with this UUID: automatic signing would use
        # only Xcode-managed profiles, and never without provisioning updates.
        entry = {"state": "authorized" if not missing else "incomplete", "missing": missing,
                 "expires": expires.strftime("%Y-%m-%d") if isinstance(expires, datetime.datetime) else None,
                 "exact_identity": identity == team + "." + bundle,
                 "uuid": uuid if isinstance(uuid, str) else None}
        rank = (len(entry["missing"]), not entry["exact_identity"])
        if best is None or rank < best_rank:
            best, best_rank = entry, rank
    return best or {"state": "missing", "missing": ["profile"], "expires": None, "exact_identity": False, "uuid": None}


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--require", choices=["mac", "ios"])
    parser.add_argument("--team", default="DNQ34W43XX")
    parser.add_argument("--device", default="", help="require coverage of this provisioning UDID (Mac)")
    parser.add_argument("--ios-device", default="", help="require coverage of this iPhone or iPad UDID")
    args = parser.parse_args(argv)
    report = {"mac": summary("mac", args.team, args.device),
              "ios": summary("ios", args.team, args.ios_device),
              # iOS widgets need their own profile; Mac widgets may go without.
              "ios_widget": summary("ios", args.team, args.ios_device, BUNDLE + ".widget")}
    print(json.dumps(report, sort_keys=True))
    required = {"mac": ["mac"], "ios": ["ios", "ios_widget"]}.get(args.require, [])
    if any(report[key]["state"] != "authorized" for key in required):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
