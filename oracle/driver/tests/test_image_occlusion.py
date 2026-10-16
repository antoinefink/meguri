# SPDX-License-Identifier: AGPL-3.0-or-later
"""The image_occlusion observation (specs/conformance/image-occlusion-ops.md):
request checks, value shape, numbers in ascending order, null for note types
without field roles, and that neither the scenario's collection nor the
scratch collection keeps anything. Expected values are the examples of
specs/render/image-occlusion-format.md F2, F4 and F5."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.errors import DriverError
from driver.observe import image_occlusion as io_module
from driver.session import Session


def io(text: str) -> str:
    return "{{c1::image-occlusion:%s}}" % text


def one(shape: str, *props: tuple[str, str], number: int = 1, hide_all: bool = False) -> dict:
    return {
        "hide_all": hide_all,
        "numbers": [{"number": number, "shapes": [{"shape": shape, "props": [list(p) for p in props]}]}],
    }


def roles(occlusions: int, image: int, header: int, back_extra: int) -> dict:
    return {"occlusions": occlusions, "image": image, "header": header, "back_extra": back_extra}


NONE = {"hide_all": False, "numbers": []}


class ImageOcclusionObservation(unittest.TestCase):
    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        self.clock = Clock(str(Path(self.tmp.name) / "clock"))

    def tearDown(self):
        # The scratch collection lives for the worker process; here it lives in
        # this test's directory, so close it before that goes.
        if io_module._scratch is not None:
            io_module._scratch.col.close()
            io_module._scratch = None
        self.tmp.cleanup()

    def run_steps(self, steps):
        sc = {"id": "t/image-occlusion", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": steps}
        s = Session(sc, "UTC", self.tmp.name, self.clock)
        try:
            out = s.run_all()["steps"]
            counts = [s.col.db.scalar(f"select count() from {t}") for t in ("notes", "cards", "notetypes")]
        finally:
            s.close()
        return out, counts

    def observe(self, *requests):
        out, _ = self.run_steps([{"observe": [{"image_occlusion": r} for r in requests]}])
        return out[0]["image_occlusion"]

    def test_parse_shape_texts(self):
        # F4's table, each shape text in a c1 IO cloze.
        cases = [
            ("rect:left=.2:top=.3", one("rect", ("left", ".2"), ("top", ".3"))),
            ("text:text=a\\:b\\\\c:left=1", one("text", ("text", "a:b\\c"), ("left", "1"))),
            ("rect:a=\\:\\::b=1", one("rect", ("a", "::"), ("b", "1"))),
            ("rect:left=", one("rect", ("left", ""))),
            ("rect:left=:top=1", one("rect")),
            ("rect:left=1\\x:top=2", one("rect")),
            ("rect:left=a:", one("rect", ("left", "a"))),
            ("rect:a=1:b", one("rect", ("a", "1"))),
            ("rect:foo:left=1:top=2", one("rect", ("foo:left", "1"), ("top", "2"))),
            ("rect:a=1::b=2", one("rect", ("a", "1"), (":b", "2"))),
            ("rect:a\\=1:b=2", one("rect", ("a\\", "1"), ("b", "2"))),
            ("rect:a=b=c", one("rect", ("a", "b=c"))),
            ("rect:left=1:left=2", one("rect", ("left", "1"), ("left", "2"))),
            (":left=1", one("", ("left", "1"))),
            ("image-occlusion:rect:a=1", one("image-occlusion", ("rect:a", "1"))),
            ("rect", NONE),
            ("", NONE),
        ]
        value = self.observe({"fns": ["parse"], "inputs": [io(text) for text, _ in cases]})
        self.assertEqual(value, {"parse": [expected for _, expected in cases]})

    def test_parse_fields_numbers_ascending_and_mode(self):
        # F5's table.
        cases = [
            (
                "{{c3::image-occlusion:rect:a=1}}<br>{{c1::image-occlusion:rect:a=2}}<br>"
                "{{c3::image-occlusion:ellipse:a=3}}<br>",
                {
                    "hide_all": False,
                    "numbers": [
                        {"number": 1, "shapes": [{"shape": "rect", "props": [["a", "2"]]}]},
                        {
                            "number": 3,
                            "shapes": [
                                {"shape": "rect", "props": [["a", "1"]]},
                                {"shape": "ellipse", "props": [["a", "3"]]},
                            ],
                        },
                    ],
                },
            ),
            (
                "{{c1,2::image-occlusion:rect:left=1}}{{c2::image-occlusion:ellipse:left=2}}"
                "{{c0::image-occlusion:text:text=x}}",
                {
                    "hide_all": False,
                    "numbers": [
                        {"number": 0, "shapes": [{"shape": "text", "props": [["text", "x"]]}]},
                        {"number": 1, "shapes": [{"shape": "rect", "props": [["left", "1"]]}]},
                        {
                            "number": 2,
                            "shapes": [
                                {"shape": "rect", "props": [["left", "1"]]},
                                {"shape": "ellipse", "props": [["left", "2"]]},
                            ],
                        },
                    ],
                },
            ),
            (
                "{{c0::image-occlusion:text:text=t:oi=1}}{{c1::image-occlusion:rect:a=1}}",
                {
                    "hide_all": True,
                    "numbers": [
                        {"number": 0, "shapes": [{"shape": "text", "props": [["text", "t"], ["oi", "1"]]}]},
                        {"number": 1, "shapes": [{"shape": "rect", "props": [["a", "1"]]}]},
                    ],
                },
            ),
            ("{{c1::image-occlusion:rect:oi=0:oi=1}}", one("rect", ("oi", "0"), ("oi", "1"))),
            ("{{c1::image-occlusion:rect}}", NONE),
            ("{{c2::x {{c1::image-occlusion:rect:left=1}}}}", NONE),
            ("{{c1::image-occlusion:rect:a=1{{c2::y}}z}}", one("rect", ("a", "1"))),
            ("{{c1::image-occlusion:rect:a=1", NONE),
            ("{{c1:: image-occlusion:rect:left=1}}", NONE),
            ("{{c1::image-occlusion:text:text=a}}b:scale=1}}", one("text", ("text", "a"))),
            ("{{c1::image-occlusion:rect:left=1::hint}}", one("rect", ("left", "1"))),
            ("{{c01::image-occlusion:rect:a=1}}", one("rect", ("a", "1"))),
        ]
        value = self.observe({"fns": ["parse"], "inputs": [text for text, _ in cases]})
        self.assertEqual(value, {"parse": [expected for _, expected in cases]})

    def test_parse_keeps_tab_and_line_feed(self):
        value = self.observe({"fns": ["parse"], "inputs": ["{{c0::image-occlusion:text:text=a\nb\tc}}"]})
        self.assertEqual(value, {"parse": [one("text", ("text", "a\nb\tc"), number=0)]})

    def test_fields_roles_from_tags(self):
        # F2's table; null where the note type has no field roles.
        cases = [
            ([0, 1, 2, 3], roles(0, 1, 2, 3)),
            ([3, 2, 1, 0], roles(3, 2, 1, 0)),
            ([4, 0, 1, 2, 3], roles(1, 2, 3, 4)),
            ([None, 0, 1, 2, 3], roles(1, 2, 3, 4)),
            ([1, 0, None, 2, 3], roles(1, 0, 3, 4)),
            ([0, 0, 1, 2, 3], roles(0, 2, 3, 4)),
            ([3, 1, None, 2, 0], roles(4, 1, 3, 0)),
            ([None, 0, 1, 2], roles(0, 1, 2, 3)),
            ([0, 1, 2, None], roles(0, 1, 2, 3)),
            ([None, None, None, None], roles(0, 1, 2, 3)),
            ([7, 7, 7, 7], roles(0, 1, 2, 3)),
            ([None] * 6, roles(0, 1, 2, 3)),
            ([0, 1, 2], None),
            ([None, None, None], None),
            ([0], None),
        ]
        value = self.observe({"fns": ["fields"], "inputs": [tags for tags, _ in cases]})
        self.assertEqual(value, {"fields": [expected for _, expected in cases]})

    def test_requests_merge_and_leave_no_trace(self):
        out, counts = self.run_steps(
            [
                {
                    "observe": [
                        {"image_occlusion": {"fns": ["parse"], "inputs": [io("rect:a=1"), ""]}},
                        {"image_occlusion": {"fns": ["fields"], "inputs": [[0, 1, 2, 3], [0]]}},
                    ]
                },
                {"observe": [{"image_occlusion": {"fns": ["parse"], "inputs": []}}]},
            ]
        )
        self.assertEqual(
            out[0]["image_occlusion"],
            {"parse": [one("rect", ("a", "1")), NONE], "fields": [roles(0, 1, 2, 3), None]},
        )
        self.assertEqual(out[1]["image_occlusion"], {"parse": []})
        _, base_counts = self.run_steps([{"observe": ["ids"]}])
        self.assertEqual(counts, base_counts)
        scratch = io_module._scratch.col
        self.assertEqual(scratch.db.scalar("select count() from notes"), 0)
        self.assertEqual(scratch.db.scalar("select count() from cards"), 0)
        self.assertEqual([n.name for n in scratch.models.all_names_and_ids() if "probe" in n.name], ["IO parse probe"])

    def test_bad_requests_are_invalid(self):
        for args in (
            {"fns": ["parse"]},
            {"fns": ["parse"], "inputs": [], "extra": 1},
            {"fns": [], "inputs": []},
            {"fns": "parse", "inputs": []},
            {"fns": ["nope"], "inputs": []},
            {"fns": ["parse", "parse"], "inputs": []},
            {"fns": ["fields", "parse"], "inputs": []},
            {"fns": ["parse"], "inputs": "x"},
            {"fns": ["parse"], "inputs": [1]},
            {"fns": ["parse"], "inputs": [[0, 1, 2, 3]]},
            {"fns": ["parse"], "inputs": ["a\rb"]},
            {"fns": ["parse"], "inputs": ["a\x00b"]},
            {"fns": ["parse"], "inputs": ["a\x1fb"]},
            {"fns": ["parse"], "inputs": ["é"]},
            {"fns": ["fields"], "inputs": ["x"]},
            {"fns": ["fields"], "inputs": [[]]},
            {"fns": ["fields"], "inputs": [[True, 1, 2, 3]]},
            {"fns": ["fields"], "inputs": [[1.0, 1, 2, 3]]},
            {"fns": ["fields"], "inputs": [[-1, 1, 2, 3]]},
            {"fns": ["fields"], "inputs": [[4294967296, 1, 2, 3]]},
            {"fns": ["fields"], "inputs": [["0", 1, 2, 3]]},
        ):
            with self.subTest(args=args), self.assertRaises(DriverError) as ctx:
                self.observe(args)
            self.assertEqual(ctx.exception.kind, "invalid_scenario")

    def test_largest_tag_is_valid(self):
        value = self.observe({"fns": ["fields"], "inputs": [[4294967295, 5, 4, None, None]]})
        self.assertEqual(value, {"fields": [roles(0, 1, 2, 3)]})


if __name__ == "__main__":
    unittest.main()
