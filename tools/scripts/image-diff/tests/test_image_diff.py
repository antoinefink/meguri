"""Tests for image_diff (U10, R1973, R1974): the perceptual tolerance, diff
images, the approved-baseline guard and the settings self-test."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(os.path.dirname(HERE), "image_diff.py")
sys.path.insert(0, os.path.dirname(HERE))

import image_diff  # noqa: E402


class ImageDiffTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def png(self, name, pixels):
        path = os.path.join(self.dir, name)
        Image.fromarray(np.asarray(pixels, dtype=np.uint8), "RGB").save(path)
        return path

    def swatch(self, colour, size=(40, 60)):
        """A white image with a block of `colour`, like a token swatch."""
        pixels = np.full((size[0], size[1], 3), 255, dtype=np.uint8)
        pixels[10:30, 10:50] = colour
        return pixels

    def run_script(self, *args):
        return subprocess.run([sys.executable, SCRIPT, *args], capture_output=True, text=True)

    def test_identical_images_match(self):
        a = self.png("a.png", self.swatch((0, 122, 255)))
        result = image_diff.compare(a, a)
        self.assertTrue(result["match"])
        self.assertEqual(result["pixel_precision"], 1.0)

    def test_a_one_level_shift_is_under_the_tolerance(self):
        base = self.swatch((0, 122, 255))
        a = self.png("a.png", base)
        b = self.png("b.png", np.clip(base.astype(int) - 1, 0, 255))
        result = image_diff.compare(a, b)
        self.assertTrue(result["match"], result)
        self.assertLess(result["max_delta_e"], 2.0)

    def test_a_token_colour_change_fails_and_writes_a_diff(self):
        a = self.png("a.png", self.swatch((0, 122, 255)))
        b = self.png("b.png", self.swatch((255, 59, 48)))
        diff = os.path.join(self.dir, "out", "diff.png")
        result = image_diff.compare(a, b, diff_path=diff)
        self.assertFalse(result["match"])
        self.assertEqual(result["diff"], diff)
        with Image.open(diff) as image:
            self.assertEqual(image.size, (60, 40))
            self.assertEqual(image.getpixel((20, 20)), (255, 0, 255))
            self.assertNotEqual(image.getpixel((2, 2)), (255, 0, 255))
        proc = self.run_script("compare", a, b)
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertFalse(json.loads(proc.stdout)["match"])

    def test_a_size_change_never_matches(self):
        a = self.png("a.png", self.swatch((0, 0, 0), (40, 60)))
        b = self.png("b.png", self.swatch((0, 0, 0), (41, 60)))
        result = image_diff.compare(a, b)
        self.assertFalse(result["match"])
        self.assertIn("size", result["reason"])

    def test_precision_allows_a_share_of_differing_pixels(self):
        base = np.full((10, 10, 3), 255, dtype=np.uint8)
        changed = base.copy()
        changed[0, 0] = (0, 0, 0)
        a, b = self.png("a.png", base), self.png("b.png", changed)
        self.assertFalse(image_diff.compare(a, b)["match"])
        self.assertTrue(image_diff.compare(a, b, precision=0.99)["match"])

    def test_the_guard_flags_only_approved_baselines_changed_beyond_the_tolerance(self):
        approved = self.png("approved.png", self.swatch((0, 122, 255)))
        drift = self.png("drift.png", np.clip(self.swatch((0, 122, 255)).astype(int) + 1, 0, 255))
        changed = self.png("changed.png", self.swatch((52, 199, 89)))
        pairs = [
            {"path": "app/Snapshots/k/home.deckList/typical/a.png", "current": drift, "approved": approved},
            {"path": "app/Snapshots/k/home.deckList/typical/b.png", "current": changed, "approved": approved},
            {"path": "app/Snapshots/k/home.deckList/typical/c.png", "current": os.path.join(self.dir, "gone.png"), "approved": approved},
        ]
        report = image_diff.guard(pairs, diff_dir=os.path.join(self.dir, "diffs"))
        self.assertEqual(report["verdict"], "needs-human-review")
        self.assertEqual([e["path"] for e in report["needs_human_review"]], [pairs[1]["path"], pairs[2]["path"]])
        self.assertTrue(os.path.exists(report["needs_human_review"][0]["diff"]))

    def test_the_guard_passes_when_nothing_changed_beyond_the_tolerance(self):
        approved = self.png("approved.png", self.swatch((0, 122, 255)))
        pairs_path = os.path.join(self.dir, "pairs.json")
        with open(pairs_path, "w") as handle:
            json.dump([{"path": "p.png", "current": approved, "approved": approved}], handle)
        report_path = os.path.join(self.dir, "report.json")
        proc = self.run_script("guard", pairs_path, "--report", report_path)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        with open(report_path) as handle:
            self.assertEqual(json.load(handle)["verdict"], "pass")

    def test_differs_fails_when_a_setting_left_the_render_unchanged(self):
        reference = self.png("default.png", self.swatch((200, 200, 200)))
        changed = self.png("rt.png", self.swatch((120, 120, 120)))
        same = self.png("rm.png", self.swatch((200, 200, 200)))
        self.assertEqual(self.run_script("differs", reference, changed).returncode, 0)
        proc = self.run_script("differs", reference, changed, same)
        self.assertEqual(proc.returncode, 1)
        self.assertEqual(json.loads(proc.stdout)["unchanged"], [same])

    def test_unreadable_input_is_an_error(self):
        bad = os.path.join(self.dir, "bad.png")
        with open(bad, "w") as handle:
            handle.write("not a png")
        self.assertEqual(self.run_script("compare", bad, bad).returncode, 2)


if __name__ == "__main__":
    unittest.main()
