#!/usr/bin/env python3
"""The name and icon each identity shows (xcode_project_check.sh).

Development builds (com.antoinefink.meguri.dev: Debug, Profile, the personal
preview) are "Meguri Dev" with the blue AppIcon-Dev; daily-driver builds
(com.antoinefink.meguri: Release, AppStore) are "Meguri" with the red
AppIcon (app/Config/Base.xcconfig). Stdlib only: it runs on the host.

  app_identity_check.py settings <showBuildSettings.json> --identity dev|daily
      the resolved settings of the Meguri target (and MeguriWidget when listed)
  app_identity_check.py product <Meguri.app> --identity dev|daily
      the built app: Info.plist and localized names, icon name, the colour of the icon the
      system shows (sips renders it; the dominant hue must be red or blue),
      the widget's name, and no other identity's icon in the asset catalog

Exit 0 when everything matches, 1 with one line per difference otherwise.
"""

import argparse
import json
import math
import os
import plistlib
import struct
import subprocess
import sys
import tempfile

IDENTITIES = {
    "dev": {"bundle": "com.antoinefink.meguri.dev", "name": "Meguri Dev", "icon": "AppIcon-Dev",
            "other_icon": "AppIcon", "strings": "Development", "other_strings": "DailyDriver", "hue": "blue"},
    "daily": {"bundle": "com.antoinefink.meguri", "name": "Meguri", "icon": "AppIcon",
              "other_icon": "AppIcon-Dev", "strings": "DailyDriver", "other_strings": "Development", "hue": "red"},
}
# OKLCH hue ranges (degrees). The red source sits near 25, the dev icon near 251.
HUES = {"red": [(340.0, 360.0), (0.0, 60.0)], "blue": [(225.0, 275.0)]}


def check_settings(path, identity):
    want = IDENTITIES[identity]
    with open(path) as f:
        entries = json.load(f)
    by_target = {e.get("target"): e.get("buildSettings", {}) for e in entries}
    problems = []
    # The scheme's settings list the app target; the widget's appear only when
    # it is resolved on its own, and its name is checked in products.
    app, widget = by_target.get("Meguri"), by_target.get("MeguriWidget")
    if app is None:
        return [f"{path}: no Meguri target"]
    expect = [
        (app, "Meguri", "PRODUCT_BUNDLE_IDENTIFIER", want["bundle"]),
        (app, "Meguri", "PRODUCT_NAME", "Meguri"),
        (app, "Meguri", "MEGURI_APP_NAME", want["name"]),
        (app, "Meguri", "INFOPLIST_KEY_CFBundleDisplayName", want["name"]),
        (app, "Meguri", "ASSETCATALOG_COMPILER_APPICON_NAME", want["icon"]),
    ]
    if widget is not None:
        expect.append((widget, "MeguriWidget", "INFOPLIST_KEY_CFBundleDisplayName", want["name"]))
    for settings, target, key, value in expect:
        if settings.get(key) != value:
            problems.append(f"{target} {key} is {settings.get(key)!r}, expected {value!r}")
    excluded = app.get("EXCLUDED_SOURCE_FILE_NAMES", "").split()
    srcroot = app.get("SRCROOT", "")
    strings = "/Meguri/Identity/{}/en.lproj/InfoPlist.strings"
    unused = {want["other_icon"] + ".icon", srcroot + strings.format(want["other_strings"])}
    used = {want["icon"] + ".icon", srcroot + strings.format(want["strings"])}
    if not unused <= set(excluded) or used & set(excluded):
        problems.append(f"Meguri EXCLUDED_SOURCE_FILE_NAMES {excluded} must hold {sorted(unused)} and not {sorted(used)}")
    return problems


def read_bmp(path):
    """(width, height, [(r, g, b)]) of an uncompressed 24/32-bit BMP."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:2] != b"BM":
        raise ValueError(f"{path}: not a BMP")
    offset, = struct.unpack_from("<I", data, 10)
    header, width, height = struct.unpack_from("<Iii", data, 14)
    bpp, compression = struct.unpack_from("<HI", data, 28)
    masks = (0x00FF0000, 0x0000FF00, 0x000000FF)
    if compression == 3:
        masks = struct.unpack_from("<III", data, 54)
    elif compression != 0 or bpp not in (24, 32):
        raise ValueError(f"{path}: unsupported BMP ({bpp} bpp, compression {compression})")
    step = bpp // 8
    row = (width * step + 3) & ~3
    shifts = [(m & -m).bit_length() - 1 for m in masks]
    pixels = []
    for y in range(abs(height)):
        base = offset + y * row
        for x in range(width):
            if step == 4:
                value, = struct.unpack_from("<I", data, base + 4 * x)
                pixels.append(tuple((value & m) >> s for m, s in zip(masks, shifts)))
            else:
                b, g, r = data[base + 3 * x: base + 3 * x + 3]
                pixels.append((r, g, b))
    return width, abs(height), pixels


def oklch(rgb):
    def lin(c):
        c /= 255.0
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = (lin(c) for c in rgb)
    l_ = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m_ = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s_ = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    lightness = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_
    a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_
    bb = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_
    return lightness, math.hypot(a, bb), math.degrees(math.atan2(bb, a)) % 360.0


def dominant_hue(image):
    """The chroma-weighted mean hue of the chromatic pixels, and their share."""
    with tempfile.TemporaryDirectory() as tmp:
        bmp = os.path.join(tmp, "icon.bmp")
        subprocess.run(["sips", "-s", "format", "bmp", "-z", "64", "64", image, "--out", bmp],
                       check=True, capture_output=True)
        _, _, pixels = read_bmp(bmp)
    x = y = 0.0
    chromatic = 0
    for rgb in pixels:
        _, chroma, hue = oklch(rgb)
        if chroma > 0.08:
            chromatic += 1
            x += chroma * math.cos(math.radians(hue))
            y += chroma * math.sin(math.radians(hue))
    return math.degrees(math.atan2(y, x)) % 360.0, chromatic / max(len(pixels), 1)


def hue_family(hue):
    for family, ranges in HUES.items():
        if any(lo <= hue < hi for lo, hi in ranges):
            return family
    return "neither"


def icon_images(app, info, mac):
    if mac:
        name = info.get("CFBundleIconFile", "")
        path = os.path.join(app, "Contents", "Resources", name if name.endswith(".icns") else name + ".icns")
        return [path] if name else []
    names = []
    for key in ("CFBundleIcons", "CFBundleIcons~ipad"):
        names += info.get(key, {}).get("CFBundlePrimaryIcon", {}).get("CFBundleIconFiles", [])
    found = []
    for entry in sorted(os.listdir(app)):
        if entry.endswith(".png") and any(entry.startswith(n) for n in names):
            found.append(os.path.join(app, entry))
    return found


def catalog_names(car):
    out = subprocess.run(["assetutil", "--info", car], check=True, capture_output=True).stdout
    return {e.get("Name") for e in json.loads(out) if isinstance(e, dict) and e.get("Name")}


def check_product(app, identity):
    want = IDENTITIES[identity]
    mac = os.path.isdir(os.path.join(app, "Contents"))
    contents = os.path.join(app, "Contents") if mac else app
    with open(os.path.join(contents, "Info.plist"), "rb") as f:
        info = plistlib.load(f)
    problems = []
    expect = [("CFBundleIdentifier", want["bundle"]), ("CFBundleExecutable", "Meguri"),
              ("CFBundleDisplayName", want["name"])]
    if mac:
        expect += [("CFBundleIconName", want["icon"]), ("CFBundleIconFile", want["icon"])]
    for key, value in expect:
        if info.get(key) != value:
            problems.append(f"Info.plist {key} is {info.get(key)!r}, expected {value!r}")
    if not mac:
        for key in ("CFBundleIcons", "CFBundleIcons~ipad"):
            name = info.get(key, {}).get("CFBundlePrimaryIcon", {}).get("CFBundleIconName")
            if name != want["icon"]:
                problems.append(f"Info.plist {key} primary icon is {name!r}, expected {want['icon']!r}")
    # The localized names: the Mac's app menu and About panel read the
    # localized CFBundleName (Info.plist keeps PRODUCT_NAME there).
    strings = os.path.join(contents, "Resources" if mac else "", "en.lproj", "InfoPlist.strings")
    names = json.loads(subprocess.run(["plutil", "-convert", "json", "-o", "-", strings],
                                      check=True, capture_output=True).stdout)
    if names != {"CFBundleName": want["name"], "CFBundleDisplayName": want["name"]}:
        problems.append(f"en.lproj/InfoPlist.strings is {names}, expected both names {want['name']!r}")
    widget_info = os.path.join(contents, "PlugIns", "MeguriWidget.appex", *(["Contents"] if mac else []), "Info.plist")
    with open(widget_info, "rb") as f:
        widget = plistlib.load(f)
    if widget.get("CFBundleDisplayName") != want["name"]:
        problems.append(f"widget CFBundleDisplayName is {widget.get('CFBundleDisplayName')!r}, expected {want['name']!r}")
    images = icon_images(app, info, mac)
    if not images:
        problems.append("no app icon image in the bundle")
    for image in images:
        if not os.path.isfile(image):
            problems.append(f"{os.path.relpath(image, app)} is missing")
            continue
        hue, share = dominant_hue(image)
        family = hue_family(hue)
        if share < 0.05 or family != want["hue"]:
            problems.append(f"{os.path.relpath(image, app)}: dominant hue {hue:.0f} ({family}, "
                            f"{share:.0%} chromatic), expected {want['hue']}")
    car = os.path.join(contents, "Resources", "Assets.car") if mac else os.path.join(app, "Assets.car")
    names = catalog_names(car)
    if want["icon"] not in names:
        problems.append(f"Assets.car has no {want['icon']}")
    if want["other_icon"] in names:
        problems.append(f"Assets.car carries the other identity's {want['other_icon']}")
    return problems


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("kind", choices=["settings", "product"])
    parser.add_argument("path")
    parser.add_argument("--identity", choices=sorted(IDENTITIES), required=True)
    args = parser.parse_args(argv)
    try:
        problems = (check_settings if args.kind == "settings" else check_product)(args.path, args.identity)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError, plistlib.InvalidFileException) as error:
        problems = [f"{args.path}: {error}"]
    for problem in problems:
        print(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
