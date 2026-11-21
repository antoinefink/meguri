#!/usr/bin/env python3
"""The development identity's app icon (`tools/mg dev-icon`), in image-tools.

Development builds (com.antoinefink.meguri.dev) use `AppIcon-Dev`, the
daily-driver icon `AppIcon` in another colour: same document, same layers,
same lightness, only the hue of the dominant red moves to the system blue
family. Each colour is converted to OKLCH; a pixel whose hue lies in the red
band and whose chroma is above the neutral threshold gets the band's hues
mapped onto the blue band, with lightness L and chroma C unchanged (chroma is
lowered only when the new colour is outside sRGB, never lightness). Neutral
and off-band pixels (greys, whites, other hues) keep their colour;
at the band's edges the change fades in OKLab, through the neutral axis,
never through purple or teal.

  dev_icon.py generate [--source app/Meguri/AppIcon.icon] [--out app/Meguri/AppIcon-Dev.icon]
          [--record tools/scripts/dev-icon/record.json]
      writes the dev document: icon.json copied (colours in it converted the
      same way), every layer image converted under its own name, and the
      record, which holds both documents' digests and the parameters
  dev_icon.py check [--source ...] [--out ...] [--record ...]
      exit 1 when the record no longer matches either document (the main
      icon changed: run generate again)
  dev_icon.py analyze <png>...
      prints each image's chroma-weighted hue histogram (OKLCH degrees)

Exit 2 on unusable input.
"""

import argparse
import hashlib
import json
import math
import os
import re
import shutil
import sys

try:
    import numpy as np
    from PIL import Image
except ImportError:  # `check` needs neither: it runs with the host's python3 too
    np = Image = None

# The red band of the source image (OKLCH hue, in degrees) and its target.
# The source's dominant red sits near 25 degrees; system blue (#0088FF, the
# icon document's own fill) near 252. Hues inside the band map linearly onto
# the blue band, compressed so the band's orange end does not reach violet
# and its other end stays short of green-blue.
PARAMETERS = {
    "version": 1,
    "red_band": [350.0, 65.0],  # full change inside (wrapping through 0)
    "fade": 15.0,  # degrees beyond each end over which the change fades out
    "blue_band": [236.0, 264.0],  # where red_band lands
    "chroma_neutral": 0.025,  # at or below: unchanged (whites, greys, pith)
    "chroma_full": 0.06,  # at or above: full change
}

COLOUR = re.compile(r"^(extended-srgb|srgb):([-0-9.]+),([-0-9.]+),([-0-9.]+),([-0-9.]+)$")


def srgb_to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


def linear_to_oklab(rgb):
    m1 = np.array([[0.4122214708, 0.5363325363, 0.0514459929],
                   [0.2119034982, 0.6806995451, 0.1073969566],
                   [0.0883024619, 0.2817188376, 0.6299787005]])
    m2 = np.array([[0.2104542553, 0.7936177850, -0.0040720468],
                   [1.9779984951, -2.4285922050, 0.4505937099],
                   [0.0259040371, 0.7827717662, -0.8086757660]])
    return np.cbrt(rgb @ m1.T) @ m2.T


def oklab_to_linear(lab):
    m1 = np.array([[1.0, 0.3963377774, 0.2158037573],
                   [1.0, -0.1055613458, -0.0638541728],
                   [1.0, -0.0894841775, -1.2914855480]])
    m2 = np.array([[4.0767416621, -3.3077115913, 0.2309699292],
                   [-1.2684380046, 2.6097574011, -0.3413193965],
                   [-0.0041960863, -0.7034186147, 1.7076147010]])
    return ((lab @ m1.T) ** 3) @ m2.T


def lch(lab):
    chroma = np.hypot(lab[..., 1], lab[..., 2])
    hue = np.degrees(np.arctan2(lab[..., 2], lab[..., 1])) % 360.0
    return lab[..., 0], chroma, hue


def band_position(hue, p):
    """Signed degrees past the red band's start, and the band's width."""
    start, end = p["red_band"]
    width = (end - start) % 360.0
    offset = (hue - start) % 360.0
    # Offsets in the upper part of the circle are before the band's start.
    offset = np.where(offset > (width + 360.0) / 2, offset - 360.0, offset)
    return offset, width


def weights(chroma, hue, p):
    offset, width = band_position(hue, p)
    outside = np.maximum(np.maximum(-offset, offset - width), 0.0)
    hue_w = np.clip(1.0 - outside / p["fade"], 0.0, 1.0)
    hue_w = 0.5 - 0.5 * np.cos(np.pi * hue_w)
    chroma_w = np.clip((chroma - p["chroma_neutral"]) / (p["chroma_full"] - p["chroma_neutral"]), 0.0, 1.0)
    chroma_w = 0.5 - 0.5 * np.cos(np.pi * chroma_w)
    return hue_w * chroma_w


def target_hue(hue, p):
    offset, width = band_position(hue, p)
    t = np.clip(offset / width, 0.0, 1.0)
    lo, hi = p["blue_band"]
    return lo + t * (hi - lo)


def in_gamut(lab):
    rgb = oklab_to_linear(lab)
    return np.all((rgb >= -1e-6) & (rgb <= 1.0 + 1e-6), axis=-1)


def convert_lab(lab, p):
    """The converted OKLab colours (any shape ... x 3)."""
    lightness, chroma, hue = lch(lab)
    w = weights(chroma, hue, p)
    new_h = np.radians(target_hue(hue, p))
    moved = np.stack([lightness, chroma * np.cos(new_h), chroma * np.sin(new_h)], axis=-1)
    # Fade in OKLab: a partial change passes through the neutral axis.
    out = lab + w[..., None] * (moved - lab)
    # Out of sRGB: lower chroma (bisection), never lightness.
    bad = ~in_gamut(out)
    if np.any(bad):
        sub = out[bad]
        lo = np.zeros(len(sub))
        hi = np.ones(len(sub))
        for _ in range(24):
            mid = (lo + hi) / 2
            trial = sub.copy()
            trial[:, 1:] *= mid[:, None]
            ok = in_gamut(trial)
            lo = np.where(ok, mid, lo)
            hi = np.where(ok, hi, mid)
        sub[:, 1:] *= lo[:, None]
        out[bad] = sub
    return out


def convert_rgb8(rgb8, p):
    lab = linear_to_oklab(srgb_to_linear(rgb8.astype(np.float64) / 255.0))
    out = linear_to_srgb(oklab_to_linear(convert_lab(lab, p)))
    return np.clip(np.rint(out * 255.0), 0, 255).astype(np.uint8)


def convert_image(source, dest, p):
    with Image.open(source) as image:
        image.load()
        mode = image.mode
        if mode not in ("RGB", "RGBA"):
            raise ValueError(f"{source}: unsupported mode {mode}")
        if image.info.get("icc_profile"):
            raise ValueError(f"{source}: embedded colour profiles are not supported (sRGB only)")
        data = np.asarray(image)
    rgb = convert_rgb8(data[..., :3], p)
    if mode == "RGBA":
        rgb = np.concatenate([rgb, data[..., 3:4]], axis=-1)
    # A fresh PNG: pixels only, no metadata carried over from the source.
    Image.fromarray(rgb, mode).save(dest, format="PNG", optimize=True)


def convert_colour_string(value, p):
    m = COLOUR.match(value)
    if not m:
        return value
    space = m.group(1)
    r, g, b, a = (float(x) for x in m.groups()[1:])
    lab = linear_to_oklab(srgb_to_linear(np.array([[r, g, b]])))
    out = linear_to_srgb(oklab_to_linear(convert_lab(lab, p)))[0]
    if np.allclose(out, [r, g, b], atol=1e-4):
        return value
    return f"{space}:{out[0]:.5f},{out[1]:.5f},{out[2]:.5f},{a:.5f}"


def convert_json(node, p):
    if isinstance(node, dict):
        return {k: convert_json(v, p) for k, v in node.items()}
    if isinstance(node, list):
        return [convert_json(v, p) for v in node]
    if isinstance(node, str):
        return convert_colour_string(node, p)
    return node


def image_names(node):
    if isinstance(node, dict):
        for k, v in node.items():
            if k == "image-name" and isinstance(v, str):
                yield v
            else:
                yield from image_names(v)
    elif isinstance(node, list):
        for v in node:
            yield from image_names(v)


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def manifest(source, out):
    files = {}
    for root in (source, out):
        for dirpath, _, names in os.walk(root):
            for name in sorted(names):
                full = os.path.join(dirpath, name)
                rel = os.path.relpath(full, os.path.dirname(root))
                if not name.startswith("."):
                    files[rel.replace(os.sep, "/")] = sha256(full)
    return {"generator": "tools/scripts/dev-icon/dev_icon.py", "parameters": PARAMETERS,
            "files": dict(sorted(files.items()))}


def generate(source, out, manifest_path):
    with open(os.path.join(source, "icon.json")) as f:
        doc = json.load(f)
    names = sorted(set(image_names(doc)))
    if not names:
        raise ValueError(f"{source}/icon.json references no image")
    os.makedirs(os.path.join(out, "Assets"), exist_ok=True)
    for stale in os.listdir(os.path.join(out, "Assets")):
        if stale not in names:
            os.remove(os.path.join(out, "Assets", stale))
    for name in names:
        convert_image(os.path.join(source, "Assets", name), os.path.join(out, "Assets", name), PARAMETERS)
    converted = convert_json(doc, PARAMETERS)
    if converted == doc:
        shutil.copyfile(os.path.join(source, "icon.json"), os.path.join(out, "icon.json"))
    else:
        with open(os.path.join(out, "icon.json"), "w") as f:
            json.dump(converted, f, indent=2, separators=(",", " : "))
    with open(manifest_path, "w") as f:
        json.dump(manifest(source, out), f, indent=2)
        f.write("\n")


def check(source, out, path):
    try:
        with open(path) as f:
            recorded = json.load(f)
    except (OSError, ValueError) as error:
        print(f"dev-icon: cannot read {path}: {error}", file=sys.stderr)
        return 1
    current = manifest(source, out)
    if recorded != current:
        changed = sorted(k for k in set(recorded.get("files", {})) | set(current["files"])
                         if recorded.get("files", {}).get(k) != current["files"].get(k))
        if recorded.get("parameters") != current["parameters"]:
            changed.append("parameters")
        print("dev-icon: stale, run tools/mg dev-icon generate; changed: " + ", ".join(changed), file=sys.stderr)
        return 1
    return 0


def analyze(paths):
    for path in paths:
        with Image.open(path) as image:
            rgb = np.asarray(image.convert("RGB"))
        lab = linear_to_oklab(srgb_to_linear(rgb.reshape(-1, 3).astype(np.float64) / 255.0))
        lightness, chroma, hue = lch(lab)
        hist, edges = np.histogram(hue, bins=36, range=(0, 360), weights=chroma)
        total = hist.sum() or 1.0
        mask = chroma > 0.08
        mean = math.degrees(math.atan2((chroma[mask] * np.sin(np.radians(hue[mask]))).sum(),
                                       (chroma[mask] * np.cos(np.radians(hue[mask]))).sum())) % 360
        print(f"{path}: L mean {lightness.mean():.4f} sd {lightness.std():.4f}; "
              f"chroma mean {chroma.mean():.4f}; chromatic (C>0.08) {mask.mean():.1%}, mean hue {mean:.1f}")
        for count, lo in zip(hist, edges):
            if count / total >= 0.005:
                print(f"  {lo:5.0f}-{lo + 10:3.0f}: {count / total:6.1%} {'#' * int(60 * count / total)}")
        bands = [(0.0, 0.025), (0.025, 0.06), (0.06, 0.12), (0.12, 1.0)]
        for lo, hi in bands:
            sel = (chroma > lo) & (chroma <= hi)
            print(f"  chroma {lo:.3f}-{hi:.3f}: {sel.mean():6.1%} of pixels")


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("generate", "check"):
        cmd = sub.add_parser(name)
        cmd.add_argument("--source", default="app/Meguri/AppIcon.icon")
        cmd.add_argument("--out", default="app/Meguri/AppIcon-Dev.icon")
        cmd.add_argument("--record", default="tools/scripts/dev-icon/record.json")
    cmd = sub.add_parser("analyze")
    cmd.add_argument("paths", nargs="+")
    args = parser.parse_args(argv)
    try:
        if args.command == "generate":
            generate(args.source, args.out, args.record)
            return check(args.source, args.out, args.record)
        if args.command == "check":
            return check(args.source, args.out, args.record)
        analyze(args.paths)
        return 0
    except (OSError, ValueError) as error:
        print(f"dev-icon: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
