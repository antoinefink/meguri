#!/usr/bin/env python3
"""Snapshot image comparison (U10, R1973, R1974), in the image-tools container.

The same tolerance as the snapshot tests (MeguriSnapshotTests/Support/
HostedSnapshot.swift, SnapshotTolerance): a pixel matches when its CIE94 ΔE
from the other image's pixel is at most (1 - perceptual precision) x 100
(2.0 at 0.98), and the images match when at least `precision` of the pixels
match (all of them at 1.0).

  image_diff.py compare <a.png> <b.png> [--diff out.png] [--precision P] [--perceptual-precision Q]
      prints {"match": ..., "pixel_precision": ..., "max_delta_e": ...}; exit 0 match, 1 no match
  image_diff.py guard <pairs.json> --report <out.json> [--diff-dir DIR]
      the baseline guard: pairs is [{"path", "current", "approved"}]; a pair
      whose images do not match at the tolerance needs human review
  image_diff.py differs <reference.png> <other.png>... [--report out.json]
      the settings self-test: every other image must NOT match the reference
      (a setting that leaves the render unchanged was not applied); exit 1 if one matches

Exit 2 on unusable input.
"""

import argparse
import json
import os
import sys

import numpy as np
from PIL import Image

PRECISION = 1.0
PERCEPTUAL_PRECISION = 0.98


def load_rgb(path):
    """The image as float sRGB in 0..1, alpha composited onto white."""
    with Image.open(path) as image:
        rgba = image.convert("RGBA")
    data = np.asarray(rgba, dtype=np.float64) / 255.0
    rgb, alpha = data[..., :3], data[..., 3:4]
    return rgb * alpha + (1.0 - alpha)


def srgb_to_lab(rgb):
    """CIE L*a*b* (D65) of sRGB values in 0..1."""
    linear = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
    matrix = np.array([
        [0.4124564, 0.3575761, 0.1804375],
        [0.2126729, 0.7151522, 0.0721750],
        [0.0193339, 0.1191920, 0.9503041],
    ])
    xyz = linear @ matrix.T
    xyz = xyz / np.array([0.95047, 1.0, 1.08883])
    epsilon, kappa = 216 / 24389, 24389 / 27
    f = np.where(xyz > epsilon, np.cbrt(xyz), (kappa * xyz + 16) / 116)
    lightness = 116 * f[..., 1] - 16
    a = 500 * (f[..., 0] - f[..., 1])
    b = 200 * (f[..., 1] - f[..., 2])
    return np.stack([lightness, a, b], axis=-1)


def delta_e94(lab1, lab2):
    """CIE94 colour difference (graphic arts weights), per pixel."""
    l1, a1, b1 = lab1[..., 0], lab1[..., 1], lab1[..., 2]
    l2, a2, b2 = lab2[..., 0], lab2[..., 1], lab2[..., 2]
    dl = l1 - l2
    c1 = np.hypot(a1, b1)
    c2 = np.hypot(a2, b2)
    dc = c1 - c2
    da, db = a1 - a2, b1 - b2
    dh_sq = np.maximum(da * da + db * db - dc * dc, 0.0)
    sc = 1 + 0.045 * c1
    sh = 1 + 0.015 * c1
    return np.sqrt(dl * dl + (dc / sc) ** 2 + dh_sq / (sh * sh))


def compare(path_a, path_b, precision=PRECISION, perceptual_precision=PERCEPTUAL_PRECISION, diff_path=None):
    """Compares two images; the result says whether they match."""
    a, b = load_rgb(path_a), load_rgb(path_b)
    result = {
        "a": path_a,
        "b": path_b,
        "size_a": [a.shape[1], a.shape[0]],
        "size_b": [b.shape[1], b.shape[0]],
        "precision": precision,
        "perceptual_precision": perceptual_precision,
    }
    if a.shape != b.shape:
        result.update(match=False, pixel_precision=0.0, max_delta_e=100.0,
                      reason="the images differ in size: %dx%d and %dx%d" % (a.shape[1], a.shape[0], b.shape[1], b.shape[0]))
        return result
    delta = delta_e94(srgb_to_lab(a), srgb_to_lab(b))
    threshold = (1 - perceptual_precision) * 100
    failing = delta > threshold + 1e-9
    pixel_precision = 1.0 - float(failing.mean()) if delta.size else 1.0
    match = pixel_precision >= precision
    result.update(match=bool(match), pixel_precision=round(pixel_precision, 6), max_delta_e=round(float(delta.max()) if delta.size else 0.0, 4))
    if not match:
        result["reason"] = "%.4f%% of pixels differ by more than ΔE %.1f (largest %.2f)" % (
            100 * (1 - pixel_precision), threshold, result["max_delta_e"])
    if diff_path and not match:
        write_diff(b, failing, diff_path)
        result["diff"] = diff_path
    return result


def write_diff(new_rgb, failing, path):
    """The new image dimmed to a quarter, with differing pixels in magenta."""
    dimmed = 0.75 + 0.25 * new_rgb
    out = np.where(failing[..., None], np.array([1.0, 0.0, 1.0]), dimmed)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    Image.fromarray((out * 255).round().astype(np.uint8), "RGB").save(path)


def guard(pairs, diff_dir=None):
    """The baseline guard (R1974): approved signature baselines that changed
    beyond the tolerance need human review; changes within it do not."""
    report = {"verdict": "pass", "checked": [], "needs_human_review": []}
    for pair in pairs:
        # supersedes: the approved path this one replaced (an OS rebaseline).
        entry = {k: pair[k] for k in ("path", "supersedes") if k in pair}
        if not os.path.exists(pair["current"]):
            entry.update(match=False, reason="the approved baseline is gone")
        else:
            diff = None
            if diff_dir:
                diff = os.path.join(diff_dir, pair["path"].replace("/", "__"))
            entry.update(compare(pair["approved"], pair["current"], diff_path=diff))
        report["checked"].append(entry)
        if not entry["match"]:
            report["needs_human_review"].append(entry)
    if report["needs_human_review"]:
        report["verdict"] = "needs-human-review"
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    c = sub.add_parser("compare")
    c.add_argument("a")
    c.add_argument("b")
    c.add_argument("--diff")
    c.add_argument("--precision", type=float, default=PRECISION)
    c.add_argument("--perceptual-precision", type=float, default=PERCEPTUAL_PRECISION)
    g = sub.add_parser("guard")
    g.add_argument("pairs")
    g.add_argument("--report", required=True)
    g.add_argument("--diff-dir")
    d = sub.add_parser("differs")
    d.add_argument("reference")
    d.add_argument("others", nargs="+")
    d.add_argument("--report")
    args = parser.parse_args(argv)
    try:
        if args.command == "compare":
            result = compare(args.a, args.b, args.precision, args.perceptual_precision, args.diff)
            print(json.dumps(result, indent=2))
            return 0 if result["match"] else 1
        if args.command == "guard":
            with open(args.pairs) as handle:
                pairs = json.load(handle)
            report = guard(pairs, args.diff_dir)
            with open(args.report, "w") as handle:
                json.dump(report, handle, indent=2)
            print(json.dumps({"verdict": report["verdict"], "needs_human_review": [e["path"] for e in report["needs_human_review"]]}))
            return 0
        results = [compare(args.reference, other) for other in args.others]
        unchanged = [r["b"] for r in results if r["match"]]
        text = json.dumps({"unchanged": unchanged, "results": results}, indent=2)
        if args.report:
            with open(args.report, "w") as handle:
                handle.write(text)
        print(text)
        return 1 if unchanged else 0
    except (OSError, ValueError, KeyError) as error:
        print("image_diff: %s" % error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
