"""Tests of dev_icon.py (tools/mg image-tools-selftest)."""

import json
import os
import shutil
import sys
import tempfile
import unittest

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import dev_icon  # noqa: E402

P = dev_icon.PARAMETERS


def lab_of(rgb8):
    return dev_icon.linear_to_oklab(dev_icon.srgb_to_linear(np.array(rgb8, dtype=np.float64) / 255.0))


class ConvertTests(unittest.TestCase):
    def convert(self, rgb):
        return dev_icon.convert_rgb8(np.array([[rgb]], dtype=np.uint8), P)[0, 0]

    def test_red_becomes_blue_with_the_same_lightness(self):
        for red in [(150, 20, 35), (200, 40, 40), (110, 15, 20), (220, 90, 70)]:
            out = self.convert(red)
            l0, _, _ = dev_icon.lch(lab_of(red))
            l1, c1, h1 = dev_icon.lch(lab_of(out))
            self.assertAlmostEqual(float(l0), float(l1), delta=0.01, msg=f"{red} -> {out}")
            self.assertTrue(225 <= float(h1) <= 275, f"{red} -> {out}: hue {float(h1):.0f}")
            self.assertGreater(float(c1), 0.05)

    def test_neutral_and_off_band_colours_are_unchanged(self):
        for colour in [(255, 255, 255), (0, 0, 0), (128, 128, 128), (236, 232, 230), (40, 160, 60), (0, 136, 255)]:
            self.assertEqual(tuple(self.convert(colour)), colour)

    def test_document_colour_strings(self):
        blue = "extended-srgb:0.00000,0.53333,1.00000,1.00000"
        self.assertEqual(dev_icon.convert_colour_string(blue, P), blue)
        moved = dev_icon.convert_colour_string("srgb:0.80000,0.10000,0.10000,1.00000", P)
        r, g, b, a = (float(x) for x in moved.split(":")[1].split(","))
        self.assertGreater(b, r)
        self.assertEqual(a, 1.0)


class DocumentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.source = os.path.join(self.tmp, "AppIcon.icon")
        self.out = os.path.join(self.tmp, "AppIcon-Dev.icon")
        self.manifest = os.path.join(self.tmp, "dev-icon.json")
        os.makedirs(os.path.join(self.source, "Assets"))
        pixels = np.zeros((8, 8, 3), dtype=np.uint8)
        pixels[:4] = (180, 30, 40)
        pixels[4:] = (240, 238, 235)
        Image.fromarray(pixels, "RGB").save(os.path.join(self.source, "Assets", "art.png"))
        with open(os.path.join(self.source, "icon.json"), "w") as f:
            json.dump({"groups": [{"layers": [{"image-name": "art.png"}]}]}, f)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def test_generate_then_check_then_stale(self):
        dev_icon.generate(self.source, self.out, self.manifest)
        self.assertEqual(dev_icon.check(self.source, self.out, self.manifest), 0)
        with open(os.path.join(self.source, "icon.json")) as a, open(os.path.join(self.out, "icon.json")) as b:
            self.assertEqual(a.read(), b.read())
        with Image.open(os.path.join(self.out, "Assets", "art.png")) as image:
            out = np.asarray(image)
            self.assertNotIn("icc_profile", image.info)
        self.assertEqual(tuple(out[6, 0]), (240, 238, 235))
        self.assertGreater(int(out[0, 0, 2]), int(out[0, 0, 0]))
        # The main icon changes: the dev icon is stale until generated again.
        Image.fromarray(np.full((8, 8, 3), 90, dtype=np.uint8), "RGB").save(
            os.path.join(self.source, "Assets", "art.png"))
        self.assertEqual(dev_icon.check(self.source, self.out, self.manifest), 1)


if __name__ == "__main__":
    unittest.main()
