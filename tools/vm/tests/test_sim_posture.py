"""Synthetic only: no VNC connection, native execution or workstation GUI.

Run through tools/mg image-tools-selftest -k posture (Pillow image-tools image).
Transport/inspection regressions also remain in tools/tests/vnc_session.bats.
"""
import base64
from contextlib import ExitStack
import copy
import ctypes
import importlib.util
import io
import json
import os
import pathlib
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("posture", ROOT / "tools/vm/sim-posture.py")
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
NAME = "Meguri-Pool-foldable-1"
UDID = "11111111-2222-3333-4444-555555555555"


def window():
    return dict(id=23, pid=123, lookup_pid=123, active=True, terminated=False, finished_launching=True,
                title=NAME, bundle="/Selected/DeviceHub.app",
                bundle_id="com.apple.dt.Devices",
                executable="/Selected/DeviceHub.app/Contents/MacOS/DeviceHub",
                rect=[100, 50, 400, 700], screen=[0, 0, 800, 900], pointer=[0, 0])


def native_window(owner):
    return dict(front={k: owner[k] for k in ("pid", "active", "terminated", "finished_launching",
                                           "bundle", "bundle_id", "executable")},
                normal=[{k: v for k, v in owner.items() if k not in ("screen", "pointer")}],
                screen=owner["screen"], pointer=owner["pointer"])


def png(color):
    output = io.BytesIO()
    Image.new("RGB", (100, 200), color).save(output, format="PNG")
    return base64.b64encode(output.getvalue()).decode()


def synthetic_toolbar(w, geometry, controls, variant=None):
    """Invented geometric fixtures, never captured or traced Apple glyphs."""
    scale = int(geometry["scale"])
    image = Image.new("RGB", tuple(int(v * scale) for v in w["screen"][2:]), "#303030")
    draw = ImageDraw.Draw(image)
    for pose, (px, py) in p.toolbar_points(controls, geometry).items():
        shape = pose
        if variant == "swapped" and pose in ("open", "closed"):
            shape = "closed" if pose == "open" else "open"
        if variant == "middle" and pose == "partial":
            shape = "open"
        if (variant in ("missing", "tooltip-missing") and pose == "partial"
                or variant == "missing-closed" and pose == "closed"
                or variant == "missing-open" and pose == "open"):
            continue
        cx, cy = w["rect"][0] + px, w["rect"][1] + py
        # Invented strokes in window points, rendered at the bound RFB scale.
        def line(points):
            draw.line([(int((cx + x) * scale), int((cy + y) * scale)) for x, y in points],
                      fill="white" if variant == "tint" else "#cccccc", width=round(2 * scale))
        if variant == "tint":
            # Pillow rectangles include the far endpoint; the calibrated crop
            # is half-open. Cover all 22 points at both 1x and 2x, without a
            # default-background stripe along the bottom/right crop border.
            draw.rectangle([(int((cx - 11) * scale), int((cy - 11) * scale)),
                            (int((cx + 11) * scale) - 1, int((cy + 11) * scale) - 1)], fill="#3478bd")
        if shape == "closed":
            line([(-4, -7), (4, -7), (4, 7), (-4, 7), (-4, -7)])
        elif shape == "partial":
            line([(0, -7), (7, 0), (0, 7), (-7, 0), (0, -7)])
        else:
            line([(-7, -7), (-7, 7)])
            line([(7, -7), (7, 7)])
            line([(-7, 0), (7, 0)])
        if (variant == "mutated" and pose == "closed"
                or variant == "mutated-middle" and pose == "partial"):
            line([(-4, -7), (4, 7)])
    return image


class ToolbarTests(unittest.TestCase):
    def fixture(self, folder, scale=1):
        w = window()
        geometry = p.window_binding(w, (800 * scale, 900 * scale), NAME)
        controls = dict(closed=[170, 650], open=[230, 650])
        path = Path(folder) / "neutral.png"
        synthetic_toolbar(w, geometry, controls).save(path)
        record = dict(schema=3, binding=dict(name=NAME), geometry=geometry,
                      viewport="compact", controls=controls,
                      glyphs=p.toolbar_glyphs(path, w, geometry, controls))
        return w, geometry, path, record

    def test_posture_private_glyphs_require_three_distinct_shapes_at_exact_positions(self):
        for scale in (1, 2):
            with self.subTest(scale=scale), tempfile.TemporaryDirectory() as folder:
                w, geometry, path, record = self.fixture(folder, scale)
                p.validate_calibration(record, record["binding"], geometry)
                p.toolbar_evidence(path, w, geometry, record)
                self.assertLess(len(json.dumps(record)), 8192)
                for variant in ("tint", "mutated", "mutated-middle", "swapped", "middle", "missing",
                                "missing-closed", "missing-open"):
                    with self.subTest(variant=variant):
                        synthetic_toolbar(w, geometry, record["controls"], variant).save(path)
                        if variant == "tint":
                            p.toolbar_evidence(path, w, geometry, record)
                        else:
                            with self.assertRaises(RuntimeError):
                                p.toolbar_evidence(path, w, geometry, record)
                for bad in (dict(record, glyphs=dict(record["glyphs"], partial=record["glyphs"]["open"])),
                            dict(record, glyphs=dict(record["glyphs"], partial="0" * 121)),
                            dict(record, glyphs=dict(record["glyphs"], partial="f" * 121)),
                            dict(record, controls=dict(closed=[170, 400], open=[230, 400])),
                            dict(record, controls=dict(closed=[3, 650], open=[63, 650])),
                            dict(record, controls=dict(closed=[170, 650], open=[200, 650])),
                            dict(record, controls=dict(closed=[170, 650], open=[290, 650])),
                            dict(record, controls=dict(closed=[170, 650], open=[230, 655])),
                            dict(record, schema=2)):
                    with self.assertRaises(RuntimeError):
                        p.validate_calibration(bad, record["binding"], geometry)
                Image.new("RGB", (800 * scale, 900 * scale), "#303030").save(path)
                with self.assertRaises(RuntimeError):
                    p.toolbar_evidence(path, w, geometry, record)

    def test_posture_calibration_glyph_sampling_preserves_neutral_pointer_and_zero_clicks(self):
        for failure in (None, "missing", "pointer", "window", "title", "tooltip", "crop-tooltip",
                        "unknown", "crop-unknown"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as folder:
                w, geometry, approved, record = self.fixture(folder)
                fresh = Path(folder) / "fresh.png"
                synthetic_toolbar(w, geometry, record["controls"], failure).save(fresh)
                if failure == "pointer":
                    w["pointer"] = [300, 700]
                guest = mock.Mock()
                guest.call.return_value = dict(w, id=24) if failure == "window" else w
                holder = mock.Mock()
                holder.batch.return_value = fresh
                boxes = [] if failure == "title" else [(NAME, (130, 60, 250, 18))]
                if failure == "tooltip":
                    boxes.append(("Open", (315, 720, 30, 14)))
                if failure == "unknown":
                    boxes.append(("Ouvrir", (315, 720, 30, 14)))
                with mock.patch.object(p, "text_boxes", return_value=boxes), \
                        mock.patch.object(p, "tooltip_text_boxes", return_value=
                                          [("Open" if failure == "crop-tooltip" else "Ouvrir",
                                            (315, 720, 30, 14))]
                                          if failure in ("crop-tooltip", "crop-unknown") else []), \
                        mock.patch.object(p.time, "sleep"):
                    if failure:
                        with self.assertRaises(RuntimeError) as caught:
                            p.calibrate_toolbar(guest, holder, w, geometry, NAME, record["controls"], approved, None)
                        self.assertNotIn("Ouvrir", str(caught.exception))
                    else:
                        self.assertEqual(p.calibrate_toolbar(guest, holder, w, geometry, NAME,
                                                           record["controls"], approved, None), record["glyphs"])
                        self.assertEqual(holder.batch.call_args_list, [mock.call(), mock.call()])
                holder.click.assert_not_called()
                self.assertTrue(all(not call.args and not call.kwargs for call in holder.batch.call_args_list))

    def test_posture_glyph_fallback_cannot_erase_any_observed_pose_label(self):
        box = (315, 720, 30, 14)
        good, wrong = [("Open", box)], [("Closed", box)]
        for case in ("unlabelled", "fresh-tooltip", "wrong-then-none", "ambiguous-then-none",
                     "partial-then-none", "full-wrong", "full-correct-then-none", "disagree", "baseline-correct",
                     "baseline-wrong", "baseline-ambiguous", "stale-correct", "scope-then-none", "late",
                     "unknown-crop", "unknown-full", "unknown-with-correct", "unknown-broad",
                     "baseline-unknown", "baseline-full-unknown", "canvas", "new-canvas", "empty", "outside"):
            for scale in (1, 2):
                with self.subTest(case=case, scale=scale), tempfile.TemporaryDirectory() as folder:
                    w, geometry, baseline, record = self.fixture(folder, scale)
                    first, unlabelled = Path(folder) / "first.png", Path(folder) / "unlabelled.png"
                    image = synthetic_toolbar(w, geometry, record["controls"])
                    image.save(unlabelled)
                    if case == "fresh-tooltip":
                        ImageDraw.Draw(image).rectangle([300 * scale, 720 * scale, 380 * scale, 735 * scale],
                                                       fill="white")
                    image.save(first)
                    def scaled(rows):
                        return [(text, tuple(v * scale for v in rect)) for text, rect in rows]
                    title = [(NAME, (130 * scale, 60 * scale, 250 * scale, 18 * scale))]
                    unknown = [("Ouvrir", box)]
                    canvas = [("Canvas", (211, 640, 10, 10))]
                    baseline_rows = {"baseline-correct": good, "baseline-wrong": wrong,
                                     "baseline-ambiguous": good * 2, "baseline-unknown": unknown,
                                     "canvas": canvas}.get(case, [])
                    crop_rows = {"fresh-tooltip": good, "wrong-then-none": wrong,
                                 "ambiguous-then-none": good * 2,
                                 "partial-then-none": [("Partially", box), ("Open", box)],
                                 "disagree": good, "stale-correct": good,
                                 "scope-then-none": [("Open", (211, 640, 10, 10))],
                                 "unknown-crop": unknown, "unknown-with-correct": good + unknown,
                                 "unknown-broad": [("Ouvrir", (211, 640, 10, 10))],
                                 "new-canvas": canvas, "canvas": canvas,
                                 "empty": [(" \t", box)],
                                 "outside": [("Ouvrir", (5, 5, 30, 14))]}.get(case, [])
                    ticks = dict(now=0, samples=0)
                    def full(ocr, path):
                        if path == baseline and case == "baseline-full-unknown":
                            return title + scaled(unknown)
                        if case == "canvas":
                            return title + scaled(canvas)
                        rows = (wrong if case in ("full-wrong", "disagree") else
                                good if case == "full-correct-then-none" else
                                unknown if case == "unknown-full" else []) if path == first else []
                        return title + scaled(rows)
                    def crop(ocr, path, *args):
                        if path == baseline:
                            return scaled(baseline_rows)
                        ticks["samples"] += 1
                        if case == "late":
                            ticks["now"] = p.HOVER_ACQUISITION_SECONDS + 10
                        return scaled(crop_rows) if path == first or case == "canvas" else []
                    holder = mock.Mock()
                    holder.batch.side_effect = [baseline, baseline, first] + [unlabelled] * 50
                    # Virtual sleep ends the bounded wait. A later clean frame
                    # is available, but bad observations must stop before it.
                    def sleep(seconds):
                        ticks["now"] += seconds
                    with mock.patch.object(p, "stable_window"), \
                            mock.patch.object(p, "text_boxes", side_effect=full), \
                            mock.patch.object(p, "tooltip_text_boxes", side_effect=crop), \
                            mock.patch.object(p.time, "monotonic", side_effect=lambda: ticks["now"]), \
                            mock.patch.object(p.time, "sleep", side_effect=sleep):
                        if case in ("unlabelled", "fresh-tooltip", "canvas", "empty", "outside"):
                            proof = p.hover(mock.Mock(), holder, w, geometry, NAME,
                                            record["controls"]["open"], "Open", None, record=record)
                            self.assertIn("tooltip" if case == "fresh-tooltip" else "three-glyph", proof)
                        else:
                            with self.assertRaises(RuntimeError) as caught:
                                p.hover(mock.Mock(), holder, w, geometry, NAME,
                                        record["controls"]["open"], "Open", None, record=record)
                            self.assertNotIn("Ouvrir", str(caught.exception))
                            self.assertNotIn("Canvas", str(caught.exception))
                    holder.click.assert_not_called()
                    if case in ("wrong-then-none", "ambiguous-then-none", "partial-then-none",
                                "full-wrong", "disagree", "stale-correct", "late", "unknown-crop", "unknown-full",
                                "unknown-with-correct", "unknown-broad", "new-canvas"):
                        self.assertEqual(ticks["samples"], 1)
                    if case.startswith("baseline-"):
                        self.assertEqual(ticks["samples"], 0)
                        self.assertEqual(holder.batch.call_args_list,
                                         [mock.call("move %d %d" % (300 * scale, 62 * scale)), mock.call()])

    def test_posture_glyph_admission_keeps_single_click_and_display_receipts(self):
        for pose in ("open", "closed"):
            for failure in (None, "tint", "mutated", "mutated-middle", "swapped", "middle", "missing", "title", "pointer",
                            "window", "wrong", "ambiguous", "legacy1", "legacy2", "tooltip", "stale-tooltip",
                            "tooltip-missing", "legacy2-tooltip", "unknown-full", "unknown-crop",
                            "unknown-with-correct", "baseline-unknown-full", "baseline-unknown-crop", "canvas"):
                with self.subTest(pose=pose, failure=failure), tempfile.TemporaryDirectory() as folder:
                    folder = Path(folder)
                    w, geometry, _, record = self.fixture(folder)
                    binding = record["binding"]
                    if failure in ("legacy1", "legacy2", "legacy2-tooltip"):
                        record.pop("glyphs")
                        record["schema"] = 1 if failure == "legacy1" else 2
                        if record["schema"] == 1:
                            record.pop("viewport")
                            record["compact"] = True
                    state = dict(clicks=0, pointed=False, frames=0, displays=0)

                    class Guest:
                        def __init__(self, *args):
                            self.ready, self.child = dict(binding=binding), mock.Mock()
                        def call(self, action):
                            if action == "window":
                                value = copy.deepcopy(w)
                                if state["pointed"] and failure == "pointer":
                                    value["pointer"][0] += 3
                                if state["pointed"] and failure == "window":
                                    value["id"] += 1
                                return value
                            if action == "displays":
                                state["displays"] += 1
                                inner = pose == "closed" if not state["clicks"] else pose == "open"
                                return {"1": png("black" if inner else "white"),
                                        "3": png("white" if inner else "black")}
                            raise AssertionError(action)

                    self_test = self
                    class Holder:
                        def __init__(self, *args):
                            self.folder, self.log, self.pending = args[3], io.BytesIO(), False
                        def batch(self, command=None):
                            if command:
                                _, x, y = command.split()
                                w["pointer"] = [int(x), int(y)]
                                state["pointed"] = int(y) == 700
                            state["frames"] += 1
                            path = self.folder / (str(state["frames"]) + ".png")
                            image = synthetic_toolbar(w, geometry, record["controls"],
                                                      failure if state["pointed"] else None)
                            if state["pointed"] and failure in ("tooltip", "legacy2-tooltip"):
                                ImageDraw.Draw(image).rectangle([200, 720, 400, 730], fill="white")
                            image.save(path)
                            return path
                        def click(self, original, point):
                            self_test.assertEqual({k: v for k, v in original.items() if k != "pointer"},
                                                  {k: v for k, v in w.items() if k != "pointer"})
                            self_test.assertEqual(point, [w["rect"][0] + record["controls"][pose][0],
                                                         w["rect"][1] + record["controls"][pose][1]])
                            state["clicks"] += 1

                    def boxes(ocr, path, *args):
                        result = [] if state["pointed"] and failure == "title" else [(NAME, (130, 60, 250, 18))]
                        crop = bool(args)
                        x = w["rect"][0] + record["controls"][pose][0]
                        if (failure == ("baseline-unknown-crop" if crop else "baseline-unknown-full")
                                or state["pointed"] and failure in
                                (("unknown-crop", "unknown-with-correct") if crop else ("unknown-full",))):
                            result.append(("Ouvrir", (x - 15, 720, 30, 14)))
                        if failure == "canvas":
                            result.append(("Canvas", (x - 110, 640, 10, 10)))
                        if failure == "stale-tooltip" or (state["pointed"] and failure in
                                                          ("wrong", "ambiguous", "tooltip", "tooltip-missing",
                                                           "legacy2-tooltip", "unknown-with-correct")):
                            label = "Partially Open" if failure == "wrong" else pose.title()
                            result += [(label, (x - 15, 720, 30, 14))] * (2 if failure == "ambiguous" else 1)
                        return result

                    # This fixture exercises glyph identity, not OCR latency.
                    # Advance only on explicit waits; the separate late-sample
                    # regression models evidence work crossing the deadline.
                    clock = dict(now=0)
                    def sleep(seconds):
                        clock["now"] += seconds
                    with mock.patch.object(p.session, "status", return_value=dict(connected=True)), \
                            mock.patch.object(p, "Guest", Guest), mock.patch.object(p, "Holder", Holder), \
                            mock.patch.object(p, "OffscreenText"), mock.patch.object(p, "text_boxes", side_effect=boxes), \
                            mock.patch.object(p, "tooltip_text_boxes", side_effect=boxes), \
                            mock.patch.object(p, "calibration_path", return_value=folder / "record"), \
                            mock.patch.object(p, "read_calibration", return_value=record), \
                            mock.patch.object(p, "close_guest", return_value=(True, True)), \
                            mock.patch.object(p.time, "sleep", side_effect=sleep), \
                            mock.patch.object(p.time, "monotonic", side_effect=lambda: clock["now"]):
                        if failure in (None, "tint", "tooltip", "legacy2-tooltip", "canvas"):
                            receipt = p.run(folder, "ssh", "guest", str(ROOT), UDID, "tart", pose, folder)
                            self.assertEqual(receipt["click_count"], 1)
                            self.assertIn("tooltip" if failure in ("tooltip", "legacy2-tooltip") else "three-glyph",
                                          receipt["control_evidence"])
                            self.assertEqual(set(receipt["before"]), {"1", "3"})
                            self.assertEqual(set(receipt["after"]), {"1", "3"})
                            self.assertEqual(state["displays"], 3)
                            self.assertNotIn("Ouvrir", json.dumps(receipt))
                            self.assertNotIn("Canvas", json.dumps(receipt))
                        else:
                            with self.assertRaises(RuntimeError) as caught:
                                p.run(folder, "ssh", "guest", str(ROOT), UDID, "tart", pose, folder)
                            self.assertNotIn("Ouvrir", str(caught.exception))
                    self.assertEqual(state["clicks"],
                                     1 if failure in (None, "tint", "tooltip", "legacy2-tooltip", "canvas") else 0)


class EvidenceTests(unittest.TestCase):
    def test_posture_overlay_rows_require_high_confidence_nonempty_local_text(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "frame.png"
            Image.new("RGB", (800, 900)).save(path)
            ocr = mock.Mock()
            for confidence, text, expected in ((.899, "Ouvrir", False), (.9, "Ouvrir", True),
                                                (1, "", False), (1, " \t", False)):
                with self.subTest(confidence=confidence, empty=not text.strip()):
                    ocr.recognize.return_value = [dict(text=text, confidence=confidence,
                                                     box=[.375, .8, .075, .02])]
                    boxes = p.text_boxes(ocr, path)
                    self.assertEqual(bool(p.nearby_text(boxes, (330, 700), 1)), expected)
                    self.assertEqual(bool(p.nearby_text(boxes, (330, 700), 1, control_local=True)), expected)

    def test_posture_ocr_unavailable_or_unqualified_never_supplies_evidence(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "image.png"
            Image.new("RGB", (800, 900)).save(path)
            ocr = mock.Mock()
            title = dict(text=NAME, confidence=.899, box=[.16, .07, .3, .02])
            tooltip = dict(text="Open", confidence=.899, box=[.36, .8, .04, .02])
            for rows in ([], [title, tooltip]):
                ocr.recognize.return_value = rows
                boxes = p.text_boxes(ocr, path)
                with self.assertRaises(RuntimeError):
                    p.title_evidence(boxes, window(), 1, NAME)
                with self.assertRaises(RuntimeError):
                    p.tooltip_evidence([], boxes, (300, 700), 1, "Open")
            ocr.recognize.return_value = [dict(title, confidence=.9), dict(tooltip, confidence=.9)]
            boxes = p.text_boxes(ocr, path)
            p.title_evidence(boxes, window(), 1, NAME)
            p.tooltip_evidence([], boxes, (300, 700), 1, "Open")
            ocr.recognize.side_effect = RuntimeError("posture visual evidence unavailable")
            with self.assertRaises(RuntimeError):
                p.text_boxes(ocr, path)

    def test_posture_calibration_only_reads_bounded_authenticated_terminal_data(self):
        answers = [b"COMPACT " + NAME.encode() + b"\n", b"170 650\n", b"230 650\n"]
        with mock.patch.object(p.os, "open", return_value=99) as opened, \
                mock.patch.object(p.os, "close") as closed, \
                mock.patch.object(p.os, "tcgetpgrp", return_value=7), \
                mock.patch.object(p.os, "getpgrp", return_value=7), \
                mock.patch.object(p.termios, "tcgetattr", return_value=[0, 0, 0, p.termios.ICANON]), \
                mock.patch.object(p.select, "select", return_value=([99], [], [])), \
                mock.patch.object(p.os, "write", side_effect=lambda fd, data: len(data)), \
                mock.patch.object(p.os, "read", side_effect=answers) as read:
            self.assertEqual(p.operator_controls(Path("/private/capture.png"), NAME, dict(scale=2)),
                             ("compact", dict(closed=[170, 650], open=[230, 650])))
            opened.assert_called_once_with("/dev/tty", os.O_RDWR | os.O_NOCTTY)
            self.assertEqual(read.call_args_list, [mock.call(99, 256), mock.call(99, 128), mock.call(99, 128)])
            closed.assert_called_once_with(99)
        with mock.patch.object(p.os, "open", return_value=99), mock.patch.object(p.os, "close"), \
                mock.patch.object(p.os, "tcgetpgrp", return_value=-1), \
                mock.patch.object(p.os, "read") as read:
            with self.assertRaises(RuntimeError):
                p.operator_controls(Path("/private/capture.png"), NAME, dict(scale=1))
            read.assert_not_called()

    def test_posture_expanded_confirmation_names_the_target_and_keeps_a_distinct_schema(self):
        w = dict(window(), rect=[140, 80, 960, 760], screen=[0, 0, 1500, 1000])
        geometry = p.window_binding(w, (1500, 1000), NAME)
        controls = dict(closed=[700, 715], open=[760, 715])
        for confirmation, accepted in (("EXPANDED " + NAME, True), ("COMPACT " + NAME, True),
                                       ("EXPANDED Library", False), ("EXPANDED", False),
                                       ("EXPANDED " + NAME + " other", False)):
            with self.subTest(confirmation=confirmation), mock.patch.object(p, "operator_terminal", return_value=99), \
                    mock.patch.object(p.os, "close"), \
                    mock.patch.object(p.select, "select", return_value=([99], [], [])), \
                    mock.patch.object(p.os, "write", side_effect=lambda fd, data: len(data)), \
                    mock.patch.object(p.os, "read", side_effect=[(confirmation + "\n").encode(),
                                                               b"700 715\n", b"760 715\n"]):
                if not accepted:
                    with self.assertRaises(RuntimeError):
                        p.operator_controls(Path("/private/capture.png"), NAME, geometry)
                    continue
                viewport, measured = p.operator_controls(Path("/private/capture.png"), NAME, geometry)
                record = dict(schema=2, viewport=viewport, binding=dict(name=NAME),
                              geometry=geometry, controls=measured)
                self.assertEqual(p.validate_calibration(record, record["binding"], geometry), controls)
                # The expanded canvas still needs the exact top-40pt title,
                # not its selected sidebar row, and the existing tooltip proof.
                p.title_evidence([(NAME, (400, 90, 250, 18))], w, 1, NAME)
                with self.assertRaises(RuntimeError):
                    p.title_evidence([(NAME, (155, 230, 250, 18))], w, 1, NAME)
                for changed in (dict(record, schema=1), dict(record, compact=True),
                                dict(record, viewport="library"), dict(record, viewport=True)):
                    with self.assertRaises(RuntimeError):
                        p.validate_calibration(changed, record["binding"], geometry)

    def test_posture_geometry_handles_scale_and_movement_and_refuses_mismatch(self):
        w = window()
        geometry = p.window_binding(w, (800, 900), NAME)
        w["rect"][:2] = [250, 75]
        self.assertEqual(p.window_binding(w, (800, 900), NAME), geometry)
        self.assertEqual(p.window_binding(w, (1600, 1800), NAME)["scale"], 2)
        for dimensions in ((800, 1800), (1200, 1350)):
            with self.assertRaises(RuntimeError):
                p.window_binding(w, dimensions, NAME)
        for field, value in (("screen", [-800, 0, 800, 900]), ("rect", [500, 0, 400, 700]),
                             ("title", "Library")):
            other = dict(w, **{field: value})
            with self.assertRaises(RuntimeError):
                p.window_binding(other, (800, 900), NAME)

    def test_posture_title_requires_exact_high_confidence_titlebar_match(self):
        good = [(NAME, (130, 60, 250, 18))]
        p.title_evidence(good, window(), 1, NAME)
        for boxes in ([], [(NAME, (130, 200, 250, 18))], good * 2,
                      [(NAME + " other", (130, 60, 250, 18))]):
            with self.assertRaises(RuntimeError):
                p.title_evidence(boxes, window(), 1, NAME)

    def test_posture_two_hub_windows_use_only_the_top_canvas_title(self):
        top = dict(window(), rect=[140, 80, 480, 760])
        previous = ("Previous iPhone", (20, 30, 110, 18))
        target = (NAME, (160, 90, 250, 18))
        p.window_binding(top, (800, 900), NAME)
        p.title_evidence([previous, target], top, 1, NAME)
        for boxes in ([previous], [(NAME, (20, 30, 110, 18))],
                      [previous, (NAME, (160, 200, 250, 18))]):
            with self.subTest(boxes=boxes), self.assertRaises(RuntimeError):
                p.title_evidence(boxes, top, 1, NAME)
        # A previous device on top cannot borrow the requested title below it.
        with self.assertRaises(RuntimeError):
            p.window_binding(dict(top, title="Previous iPhone"), (800, 900), NAME)

    def test_posture_title_refusal_reports_only_bounded_requested_geometry(self):
        w = dict(window(), rect=[140, 80, 960, 760])
        title = (300, 170, 500, 36)
        outside = [(20 + i * 10, 500, 5, 10) for i in range(5)]
        unrelated = "PRIVATE unrelated OCR content"
        for boxes, reason, count, exact in (
                ([], "no-exact-row", 0, []),
                ([(NAME + " suffix", title)], "no-exact-row", 1, []),
                ([(NAME, box) for box in outside] + [(unrelated, title)],
                 "outside-titlebar", 1, outside[:4]),
                ([(NAME, title)] * 2, "multiple-qualified-labels", 2, [title, title]),
                ([(NAME, title), (unrelated, title)], "multiple-qualified-labels", 2, [title])):
            with self.subTest(reason=reason, count=count), self.assertRaises(RuntimeError) as caught:
                p.title_evidence(boxes, w, 2, NAME)
            self.assertEqual(str(caught.exception),
                             "exact leased window title lacks qualified visual evidence"
                             f"; reason={reason}; front_rect_pt=[140, 80, 960, 760]; scale=2"
                             f"; qualified_titlebar_rows={count}; exact_row_boxes_px={exact}")
        # Exact requested-name rows elsewhere on the frame do not change success.
        self.assertIsNone(p.title_evidence([(NAME, title)] + [(NAME, box) for box in outside]
                                          + [(unrelated, outside[0])], w, 2, NAME))

    def test_posture_tooltip_requires_new_exact_localized_control_at_candidate(self):
        for scale in (1, 2):
            point = (300 * scale, 700 * scale)
            box = (285 * scale, 720 * scale, 30 * scale, 14 * scale)
            good = [("Open", box)]
            p.tooltip_evidence([], good, point, scale, "Open")
            for before, after in ((good, good), ([], []), ([], good * 2),
                                  ([], [("Partially Open", box)]), ([], [("Closed", box)]),
                                  ([], [("Partially", box), ("Open", box)]),
                                  ([], [("Ouvrir", box)]), ([], [("Open", (5, 5, 30, 14))])):
                with self.assertRaises(RuntimeError):
                    p.tooltip_evidence(before, after, point, scale, "Open")

    def test_posture_tooltip_crop_maps_clamped_origins_to_desktop_pixels(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "frame.png"
            Image.new("RGB", (800, 900)).save(path)
            for point, scale, bounds in (
                    ((300, 700), 1, (180, 635, 420, 765)),
                    ((600, 700), 2, (360, 570, 800, 830)),
                    ((5, 7), 1, (0, 0, 125, 72)),
                    ((5, 7), 2, (0, 0, 245, 137)),
                    ((795, 895), 1, (675, 830, 800, 900)),
                    ((795, 895), 2, (555, 765, 800, 900))):
                with self.subTest(point=point, scale=scale):
                    self.assertEqual(p.tooltip_crop_box(point, scale, (800, 900)), bounds)
                    crops = []
                    def recognize(crop):
                        crops.append(crop)
                        self.assertNotEqual(crop, path)
                        self.assertEqual(crop.stat().st_mode & 0o777, 0o600)
                        self.assertEqual(crop.parent.stat().st_mode & 0o777, 0o700)
                        with Image.open(crop) as image:
                            self.assertEqual(image.size, (bounds[2] - bounds[0], bounds[3] - bounds[1]))
                        return [dict(text="Closed", confidence=.9, box=[.25, .25, .5, .5]),
                                dict(text="Open", confidence=.899, box=[.25, .25, .5, .5])]
                    ocr = mock.Mock(recognize=mock.Mock(side_effect=recognize))
                    width, height = bounds[2] - bounds[0], bounds[3] - bounds[1]
                    self.assertEqual(p.tooltip_text_boxes(ocr, path, point, scale),
                                     [("Closed", (bounds[0] + width / 4, bounds[1] + height / 4,
                                                  width / 2, height / 2))])
                    self.assertTrue(all(not crop.parent.exists() for crop in crops))
                    self.assertTrue(path.exists())
            ocr.recognize.side_effect = RuntimeError("fixed OCR failure")
            with self.assertRaises(RuntimeError):
                p.tooltip_text_boxes(ocr, path, (300, 700), 1)
            self.assertEqual(list(Path(folder).iterdir()), [path])

    def test_posture_hover_local_ocr_proves_tooltip_omitted_by_full_frame(self):
        for failure in (None, "identical", "wrong-label", "before-pose", "conflict", "weak", "late"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as folder:
                folder = Path(folder)
                baseline, after = folder / "baseline.png", folder / "after.png"
                image = Image.new("RGB", (3840, 2400))
                image.save(baseline)
                if failure != "identical":
                    ImageDraw.Draw(image).rectangle((510, 1440, 570, 1468), fill="white")
                image.save(after)
                w = dict(window(), screen=[0, 0, 1920, 1200], pointer=[270, 700])
                geometry = p.window_binding(w, (3840, 2400), NAME)
                holder = mock.Mock()
                holder.batch.side_effect = [baseline, baseline, after]
                guest = mock.Mock()
                guest.call.side_effect = lambda *args: dict(w, pointer=[300, 62] if holder.batch.call_count < 3 else [270, 700])
                recognized = []
                ticks = dict(now=0)
                def recognize(path):
                    recognized.append(path)
                    if path in (baseline, after):
                        # Accurate full-frame OCR finds only the signed top title.
                        return [dict(text=NAME, confidence=1, box=[260/3840, 120/2400, 500/3840, 36/2400])]
                    self.assertEqual(path.name, "crop.png")
                    with Image.open(path) as crop:
                        self.assertEqual(crop.size, (480, 260))
                        hovered = crop.getpixel((210, 170)) != (0, 0, 0)
                    # An identical image must still refuse even with a bogus
                    # fresh OCR result; changed-region proof remains independent.
                    hovered = hovered or (failure == "identical" and len(recognized) == 4)
                    if not hovered and failure != "before-pose":
                        return []
                    label = "Open" if failure == "wrong-label" else "Closed"
                    rows = [dict(text=label, confidence=.899 if failure == "weak" else 1,
                                 box=[210/480, 170/260, 60/480, 28/260])]
                    if failure == "conflict":
                        rows.append(dict(rows[0], text="Partially Open"))
                    if hovered and failure in ("wrong-label", "before-pose", "conflict", "weak", "late"):
                        ticks["now"] = p.HOVER_ACQUISITION_SECONDS + 9
                    return rows
                ocr = mock.Mock(recognize=mock.Mock(side_effect=recognize))
                with mock.patch.object(p.time, "sleep"), \
                        mock.patch.object(p.time, "monotonic", side_effect=lambda: ticks["now"]):
                    if failure:
                        with self.assertRaises(RuntimeError):
                            p.hover(guest, holder, w, geometry, NAME, [170, 650], "Closed", ocr)
                    else:
                        p.hover(guest, holder, w, geometry, NAME, [170, 650], "Closed", ocr)
                expected = [mock.call("move 600 124"), mock.call()]
                if failure != "before-pose":
                    expected += [mock.call("move 540 1400")]
                self.assertEqual(holder.batch.call_args_list, expected)
                holder.click.assert_not_called()
                self.assertEqual([path for path in recognized if path in (baseline, after)],
                                 [baseline] if failure == "before-pose" else [baseline, after])
                self.assertTrue(all(not path.parent.exists() for path in recognized if path not in (baseline, after)))
                self.assertTrue(baseline.exists() and after.exists())

    def test_posture_hover_rejection_reports_only_fixed_control_facts(self):
        box = (285, 720, 30, 14)
        private = "arbitrary device app desktop content"
        for label in ("Closed", "Open"):
            for case, before, after, reason in (
                    ("baseline", [("Partially Open", box), (private, box)], [(label, box)],
                     "fresh localized pose tooltip absent or ambiguous"),
                    ("missing", [], [(private, box), ("Open", (1, 1, 10, 10))],
                     "fresh localized pose tooltip absent or ambiguous"),
                    ("partial", [], [(label, box), ("partial " + private, box)],
                     "fresh localized pose tooltip absent or ambiguous"),
                    ("scope", [], [(label, (181, 640, 10, 10))],
                     "tooltip is not scoped to candidate control"),
                    ("unexpected", [], [(label, box)], "tooltip predicate rejected")):
                with self.subTest(label=label, case=case):
                    clock = dict(now=0, crops=0)
                    # Baseline OCR precedes the target move. Its joined frame
                    # supplies sample 1 (blank); a later capture supplies the
                    # diagnostic rows in sample 2. Keep room for both complete
                    # five-second samples, then refuse another acquisition.
                    rows = iter((before, [], after))
                    def crop(*args):
                        clock["crops"] += 1
                        if clock["crops"] > 1:
                            clock["now"] += 5
                        return next(rows)
                    def sleep(seconds):
                        clock["now"] += seconds
                    with mock.patch.object(p, "HOVER_ACQUISITION_SECONDS", 14), \
                            mock.patch.object(p, "stable_window"), \
                            mock.patch.object(p, "text_boxes", return_value=[]), \
                            mock.patch.object(p, "title_evidence"), \
                            mock.patch.object(p, "tooltip_text_boxes", side_effect=crop), \
                            mock.patch.object(p.time, "sleep", side_effect=sleep), \
                            mock.patch.object(p.time, "monotonic", side_effect=lambda: clock["now"]), \
                            mock.patch.object(p, "tooltip_evidence", wraps=p.tooltip_evidence) as predicate:
                        if case == "unexpected":
                            predicate.side_effect = RuntimeError(private)
                        with self.assertRaises(RuntimeError) as caught:
                            p.hover(mock.Mock(), mock.Mock(), window(), {"scale": 1},
                                    NAME, [200, 650], label, mock.Mock())
                    prefix = "tooltip hover rejected; "
                    self.assertTrue(str(caught.exception).startswith(prefix))
                    self.assertEqual(json.loads(str(caught.exception)[len(prefix):]), dict(
                        requested=label, baseline_known_pose=case == "baseline",
                        after_labels=[] if case in ("baseline", "missing") else [label],
                        samples=0 if case == "baseline" else 2, reason=reason))
                    self.assertEqual(clock["crops"], 1 if case == "baseline" else 3)
                    # Unknown/private rows and conflicting partial labels veto
                    # before the positive tooltip predicate can admit them.
                    self.assertEqual(predicate.call_count,
                                     0 if case == "baseline" else
                                     1 if case in ("missing", "partial") else 2)
                    self.assertNotIn(private, str(caught.exception))
                    self.assertNotIn(NAME, str(caught.exception))

    def test_posture_ocr_rejects_low_confidence_and_bounds_data(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "image.png"
            Image.new("RGB", (800, 900)).save(path)
            ocr = mock.Mock()
            ocr.recognize.return_value = [dict(text=NAME, confidence=.89, box=[.1, .1, .1, .1])]
            self.assertEqual(p.text_boxes(ocr, path), [])
            ocr.recognize.return_value[0]["confidence"] = .99
            self.assertEqual(p.text_boxes(ocr, path)[0][1], (80, 90, 80, 90))
            ocr.recognize.return_value[0]["box"][0] = float("nan")
            with self.assertRaises(RuntimeError):
                p.text_boxes(ocr, path)
            ocr.recognize.return_value[0]["box"][0] = .1
            for confidence in (float("nan"), float("inf"), True, -1, 1.01, "1.0"):
                ocr.recognize.return_value[0]["confidence"] = confidence
                with self.subTest(confidence=confidence), self.assertRaises(RuntimeError):
                    p.text_boxes(ocr, path)

    def test_posture_calibration_is_exact_bounded_private_data(self):
        geometry = p.window_binding(window(), (800, 900), NAME)
        record = dict(schema=1, compact=True, binding=dict(build="24B1", vm="guest", udid=UDID),
                      geometry=geometry, controls=dict(closed=[170, 650], open=[230, 650]))
        p.validate_calibration(record, record["binding"], geometry)
        for field, value in (("compact", False), ("binding", dict(build="different")),
                             ("geometry", dict(geometry, scale=2)), ("controls", dict(open=[170, 650], closed=[230, 650])),
                             ("controls", dict(open=[float("nan"), 650], closed=[170, 650]))):
            with self.assertRaises(RuntimeError):
                p.validate_calibration(dict(record, **{field: value}), record["binding"], geometry)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "record.json"
            path.write_text(json.dumps(record))
            path.chmod(0o600)
            self.assertEqual(p.read_calibration(path), record)
            link = path.with_name("link")
            link.symlink_to(path)
            with self.assertRaises(OSError):
                p.read_calibration(link)
            path.chmod(0o644)
            with self.assertRaises(RuntimeError):
                p.read_calibration(path)
            path.chmod(0o600)
            path.write_text(" " * 8193)
            with self.assertRaises(RuntimeError):
                p.read_calibration(path)

    def test_posture_display_proof_fails_sleeping_dim_both_lit_and_partial_information(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "display.png"
            evidence = {}
            for name, color in (("black", (0, 0, 0)), ("lit", (0, 0, 255)), ("ambiguous", (12, 12, 12))):
                Image.new("RGB", (100, 200), color).save(path)
                evidence[name] = p.display_evidence(path)
                self.assertEqual(evidence[name]["state"], name)
                self.assertEqual(set(evidence[name]), {"state", "width", "height", "black_percent", "lit_percent"})
            self.assertEqual(p.display_state({"1": evidence["lit"], "3": evidence["black"]}), "outer")
            self.assertEqual(p.display_state({"1": evidence["black"], "3": evidence["lit"]}), "inner")
            for a, b in (("lit", "lit"), ("black", "black"), ("ambiguous", "lit")):
                with self.assertRaises(RuntimeError):
                    p.display_state({"1": evidence[a], "3": evidence[b]})
            with self.assertRaises(KeyError):
                p.display_state({"3": evidence["lit"]})  # Inner alone never proves Open.

    def test_posture_foreground_and_pointer_are_rechecked(self):
        original = window()
        guest = mock.Mock()
        guest.call.return_value = dict(original, pointer=[300, 700])
        p.stable_window(guest, original, [300, 700])
        for changed in (dict(original, rect=[101, 50, 400, 700]), dict(original, pid=124),
                        dict(original, id=24), dict(original, pointer=[400, 700])):
            guest.call.return_value = changed
            with self.assertRaises(RuntimeError):
                p.stable_window(guest, original, [300, 700])


class HolderTests(unittest.TestCase):
    def test_posture_guest_protocol_bounds_writes_and_handles_partial_publication(self):
        guest = p.Guest.__new__(p.Guest)
        guest.sequence = 0
        guest.child = mock.Mock()
        guest.child.stdin.fileno.return_value = 99
        guest.read = mock.Mock(return_value=dict(id=1, ok=True, result="joined"))
        writes = []
        def partial(fd, data):
            self.assertEqual(fd, 99)
            count = min(7, len(data))
            writes.append(bytes(data[:count]))
            return count
        with mock.patch.object(p.select, "select", return_value=([], [guest.child.stdin], [])), \
                mock.patch.object(p.os, "write", side_effect=partial):
            self.assertEqual(guest.call("window"), "joined")
        self.assertEqual(json.loads(b"".join(writes)), dict(id=1, action="window"))
        with mock.patch.object(p.select, "select", return_value=([], [], [])), \
                mock.patch.object(p.os, "write") as write:
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                guest.call("window")
            write.assert_not_called()

    def test_posture_guest_rejects_wrong_extra_boolean_ack_ids_and_bad_frames(self):
        guest = p.Guest.__new__(p.Guest)
        guest.sequence = 0
        guest.child = mock.Mock()
        for reply in (dict(id=True, ok=True, result=True), dict(id=2, ok=True, result=True),
                      dict(id=1, ok=True, result=True, extra=True), dict(ok=True, result=True),
                      dict(id=1, ok=1, result=True), [True]):
            guest.sequence = 0
            guest.failed = False
            with mock.patch.object(p.select, "select", return_value=([], [guest.child.stdin], [])), \
                    mock.patch.object(p.os, "write", side_effect=lambda fd, data: len(data)), \
                    mock.patch.object(guest, "read", return_value=reply):
                with self.assertRaises(RuntimeError):
                    guest.call("arm")
        for data in (b'{}\n{}\n', b'{}\ntrailing', b'[]\n', b'', b'{"error":"window"}\n', b'{"id":1,"id":1}\n', b'{"value":NaN}\n'):
            with self.subTest(data=data), mock.patch.object(p.select, "select", return_value=([guest.child.stdout], [], [])), \
                    mock.patch.object(p.os, "read", return_value=data):
                with self.assertRaises((RuntimeError, ValueError)):
                    guest.read(1)

    def fixture(self, folder):
        state = Path(folder)
        os.mkfifo(state / "commands", 0o600)
        (state / "holder.log").write_text("connected\nposture-click-v1\n")
        (state / "holder.log").chmod(0o600)
        keepalive = os.open(state / "commands", os.O_RDWR | os.O_NONBLOCK)
        self.addCleanup(os.close, keepalive)
        guest = mock.Mock()
        guest.request = dict(guard="synthetic-guard")
        guest.call.side_effect = lambda action: (
            dict(armed=True, remaining=600) if action == "arm" else
            dict(published=True, remaining=600) if action == "publish" else
            window() if action == "click-window" else True)
        holder = p.Holder(state, lambda: True, guest, state, state)
        self.addCleanup(holder.log.close)
        self.addCleanup(Path(str(state) + ".mutex").unlink, missing_ok=True)
        return state, guest, holder

    def test_posture_readonly_shot_error_and_fifo_open_failure_never_arm(self):
        for failure in ("shot", "open", "lost"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                def publish(fd, data):
                    with (state / "holder.log").open("a") as log:
                        log.write("error shot private detail\n")
                    return len(data)
                if failure == "open":
                    (state / "commands").unlink()
                elif failure == "lost":
                    holder.connected = lambda: False
                with mock.patch.object(p.os, "write", side_effect=publish):
                    with self.assertRaises((OSError, RuntimeError)):
                        holder.batch()
                self.assertFalse(holder.pending)
                self.assertFalse((state / "sim-posture-pending").exists())
                guest.call.assert_not_called()

    def test_posture_click_terminal_error_and_success_join_without_a_screenshot(self):
        for error in (None, "input", "expired"):
            with self.subTest(error=error), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                publications = []
                def publish(fd, data):
                    publications.append(bytes(data))
                    command, deadline, path = data.decode().strip().split(" ", 2)
                    self.assertEqual(command, "posture-click")
                    self.assertGreater(float(deadline), time.monotonic())
                    result = dict(joined=True, executed=error != "expired")
                    if error:
                        result["error"] = error
                    Path(path).write_text(json.dumps(result))
                    return len(data)
                with mock.patch.object(p.os, "write", side_effect=publish):
                    if error:
                        with self.assertRaisesRegex(RuntimeError, "refused or failed"):
                            holder.click(window(), [0, 0])
                    else:
                        holder.click(window(), [0, 0])
                self.assertFalse(holder.pending)
                self.assertFalse((state / "sim-posture-pending").exists())
                self.assertEqual(len(publications), 1)
                self.assertEqual(guest.call.call_args_list, [mock.call("arm"), mock.call("publish"), mock.call("click-window"), mock.call("disarm")])

    def test_posture_short_lease_and_lost_ssh_before_publication_never_click(self):
        for failure in ("short", "arm-ssh", "publish-ssh", "fifo", "old"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                if failure == "short":
                    guest.call.side_effect = lambda action: dict(armed=True, remaining=1) if action == "arm" else True
                elif failure in ("arm-ssh", "publish-ssh"):
                    original = guest.call.side_effect
                    def call(action):
                        if action == ("arm" if failure == "arm-ssh" else "publish"):
                            raise RuntimeError("guest disconnected")
                        return original(action)
                    guest.call.side_effect = call
                elif failure == "fifo":
                    (state / "commands").unlink()
                else:
                    holder.capable = False
                with mock.patch.object(p.os, "write") as write:
                    with self.assertRaises((RuntimeError, OSError)):
                        holder.click(window(), [0, 0])
                    write.assert_not_called()
                self.assertFalse(holder.pending)
                self.assertFalse((state / "sim-posture-pending").exists())

    def test_posture_atomic_fifo_error_is_nonexecution_but_partial_publication_is_uncertain(self):
        for outcome in ("error", "zero", "partial"):
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                def publish(fd, payload):
                    if outcome == "error":
                        raise BlockingIOError("pipe full")
                    return 0 if outcome == "zero" else 1
                with mock.patch.object(p.os, "write", side_effect=publish):
                    with self.assertRaises((RuntimeError, OSError)):
                        holder.click(window(), [0, 0])
                self.assertEqual(holder.pending, outcome == "partial")
                self.assertEqual((state / "sim-posture-pending").exists(), outcome == "partial")
                if outcome != "partial":
                    self.assertIn(mock.call("disarm"), guest.call.call_args_list)

    def test_posture_unjoined_actual_click_retains_evidence_after_disconnect(self):
        with tempfile.TemporaryDirectory() as folder:
            state, guest, holder = self.fixture(folder)
            ticks = iter(range(1000))
            # A submitted callback has no terminal receipt. Never infer a join
            # from caller/SSH/VNC termination, timeout, or elapsed input deadline.
            with mock.patch.object(p.time, "monotonic", side_effect=lambda: next(ticks)), \
                    mock.patch.object(p.time, "sleep"), \
                    mock.patch.object(p.os, "write", side_effect=lambda fd, data: len(data)):
                with self.assertRaisesRegex(RuntimeError, "completion unproven"):
                    holder.click(window(), [0, 0])
            self.assertTrue(holder.pending)
            self.assertTrue((state / "sim-posture-pending").exists())
            self.assertNotIn(mock.call("disarm"), guest.call.call_args_list)

    def test_posture_lost_transport_and_unjoined_guard_retain_only_actual_fences(self):
        guest = mock.Mock(admitted=True)
        guest.child.wait.side_effect = [subprocess.TimeoutExpired("synthetic", 1), -15]
        self.assertEqual(p.close_guest(guest, True), (False, False))
        guest.call.assert_not_called()
        guest.terminal.assert_not_called()
        for code in (0, 1, 255, -15):
            guest = mock.Mock(admitted=True)
            guest.child.wait.return_value = code
            self.assertEqual(p.close_guest(guest, False), (True, code == 0))
        guest = mock.Mock(admitted=True)
        guest.call.side_effect = RuntimeError("SSH lost before button input")
        guest.child.wait.return_value = 255
        self.assertEqual(p.close_guest(guest, False), (True, False))
        guest.terminal.assert_called_once()

    def test_posture_native_holder_callback_refuses_expired_and_joins_terminal_errors(self):
        # Load only against synthetic vncdotool boundary classes; no dependency
        # installation, VNC connection, credentials or native acceptance implied.
        import types
        client_module = types.ModuleType("vncdotool.client")
        client_module.VNCDoToolClient = type("Client", (), {})
        client_module.VNCDoToolFactory = type("Factory", (), {})
        package = types.ModuleType("vncdotool")
        package.api = mock.Mock()
        with mock.patch.dict(sys.modules, {"vncdotool": package, "vncdotool.client": client_module}):
            spec = importlib.util.spec_from_file_location("native_holder", ROOT / "tools/vm/vnc-holder.py")
            native = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(native)
        for scenario in ("expired", "ok", "error"):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as folder:
                client = native.ResizingVNCClient()
                client.mouseDown = mock.Mock(side_effect=RuntimeError("private") if scenario == "error" else None)
                client.mouseUp = mock.Mock()
                receipt = Path(folder) / "receipt.json"
                with mock.patch.object(native.time, "monotonic", return_value=10):
                    client.postureClick(9 if scenario == "expired" else 11, str(receipt))
                proof = json.loads(receipt.read_bytes())
                self.assertTrue(proof["joined"])
                self.assertEqual(proof["executed"], scenario != "expired")
                if scenario == "expired":
                    client.mouseDown.assert_not_called()
                    client.mouseUp.assert_not_called()
                else:
                    client.mouseDown.assert_called_once_with(1)
                    client.mouseUp.assert_called_once_with(1)

    def test_posture_calibration_terminal_refusal_precedes_all_guest_work(self):
        with mock.patch.object(p, "operator_terminal", side_effect=RuntimeError("operator terminal")), \
                mock.patch.object(p, "Guest") as guest, mock.patch.object(p.session, "status") as status:
            with self.assertRaisesRegex(RuntimeError, "operator terminal"):
                p.run(Path("/unused"), "ssh", "guest", str(ROOT), UDID, "tart", "calibrate", Path("/lease"))
            guest.assert_not_called()
            status.assert_not_called()


class ProductionFlowTests(unittest.TestCase):
    def test_posture_local_ocr_compile_refusal_is_quiet_and_precedes_guest_work(self):
        for timeout in (False, True):
            with self.subTest(timeout=timeout), tempfile.TemporaryDirectory() as folder:
                stdout, stderr = io.StringIO(), io.StringIO()
                def compile_ocr(args, **kwargs):
                    self.assertEqual(args[:2], ["/usr/bin/xcrun", "swiftc"])
                    self.assertEqual(kwargs["stdout"], subprocess.DEVNULL)
                    self.assertEqual(kwargs["stderr"], subprocess.DEVNULL)
                    if timeout:
                        raise subprocess.TimeoutExpired(args, 60, stderr=b"PRIVATE compiler diagnostics")
                    return subprocess.CompletedProcess(args, 1)
                with mock.patch.object(p.session, "status", return_value=dict(connected=True)), \
                        mock.patch.object(p, "Guest") as guest, \
                        mock.patch.object(p.subprocess, "run", side_effect=compile_ocr), \
                        mock.patch.object(p.signal, "signal"), mock.patch.object(p.os, "umask"), \
                        mock.patch.object(sys, "argv", ["posture", folder, "ssh", "guest", str(ROOT), UDID,
                                                       "tart", "open", folder]), \
                        mock.patch.object(sys, "stdout", stdout), mock.patch.object(sys, "stderr", stderr):
                    self.assertEqual(p.main(), 1)
                guest.assert_not_called()
                self.assertEqual(stdout.getvalue(), "")
                self.assertEqual(stderr.getvalue(), "sim-posture: " +
                                 ("private posture evidence unavailable" if timeout else
                                  "local offscreen OCR compilation unavailable") + "\n")

    def test_posture_one_click_after_tooltip_and_two_readonly_display_proofs(self):
        for pose in ("open", "closed"):
            for failure in (None, "delayed", "transient", "stale", "partial", "sleep", "proof", "calibration", "moved",
                            "arm-window", "arm-pointer", "reserve-window", "reserve-pointer", "final-unavailable",
                            "ocr-unavailable", "ocr-timeout", "ocr-malformed", "weak-title", "wrong-title", "weak-tooltip"):
                with self.subTest(pose=pose, failure=failure), tempfile.TemporaryDirectory() as folder:
                    folder = Path(folder)
                    lease = folder / "lease"
                    lease.mkdir()
                    w = window()
                    state = dict(clicks=0, frames=0, pointed=False, displays=0, compiles=0, recognized=[])
                    binding = dict(name=NAME, vm="guest", udid=UDID, build="selected")
                    record = dict(schema=1, compact=True, binding=binding,
                                  geometry=p.window_binding(w, (800, 900), NAME),
                                  controls=dict(closed=[170, 650], open=[230, 650]))
                    if failure == "calibration":
                        record["binding"] = dict(binding, build="foreign")

                    class FakeGuest:
                        def __init__(self, *args):
                            request = args[1]
                            source = (ROOT / "tools/vm/sim-posture-visual.swift").read_text()
                            # The M5 controller supplies the current observer;
                            # no deployment or resident replacement is involved.
                            if request["visual"] != source:
                                raise AssertionError("guest observer source mismatch")
                            self.ready = dict(binding=binding)
                            self.child = mock.Mock()
                            self.request = request
                        def call(self, action, **kwargs):
                            if action in ("arm", "publish", "disarm"):
                                if action == "arm" and failure == "arm-window":
                                    w["id"] += 1  # Another signed Hub canvas comes forward.
                                if action == "arm" and failure == "arm-pointer":
                                    w["pointer"][0] += 20
                                if action == "disarm":
                                    state["disarmed"] = True
                                    return True
                                return {"armed" if action == "arm" else "published": True, "remaining": 600}
                            if action in ("window", "click-window"):
                                if action == "click-window" and failure == "final-unavailable":
                                    raise RuntimeError("native proof unavailable")
                                result = copy.deepcopy(w)
                                if failure == "moved" and state["frames"] > 1:
                                    result["rect"][0] += 1
                                return result
                            if action == "displays":
                                state["displays"] += 1
                                if failure == "sleep":
                                    return {"1": png("black"), "3": png("black")}
                                if failure == "proof" and state["clicks"]:
                                    return {"1": png("white"), "3": png("white")}
                                if failure == "transient" and state["clicks"] and 2 < state["displays"] < 6:
                                    color = {3: "black", 4: "#0c0c0c", 5: "white"}[state["displays"]]
                                    return {"1": png(color), "3": png(color)}
                                inner = pose == "closed" if not state["clicks"] else pose == "open"
                                return {"1": png("black" if inner else "white"),
                                        "3": png("white" if inner else "black")}
                            raise AssertionError(action)

                    # Keep the actual Holder.click and scheduler fence. Only the
                    # transport/capture boundaries are synthetic; run() must pass
                    # its original window and guest-point hover position.
                    class FakeHolder(p.Holder):
                        def __init__(self, *args):
                            self.pending = self.armed = False
                            self.capable = True
                            self.connected, self.guest = args[1:3]
                            self.log = io.BytesIO()
                            self.folder = args[3]
                            self.lease = args[4]
                            info = self.lease.stat()
                            self.lease_identity = (info.st_dev, info.st_ino)
                        def open_publication(self):
                            return None
                        def publication(self, fd, latest, receipt):
                            if failure == "reserve-window":
                                w["id"] += 1
                            if failure == "reserve-pointer":
                                w["pointer"][0] += 20
                            return min(time.monotonic() + 5, latest), b"synthetic-click"
                        def publish(self, fd, payload):
                            state["clicks"] += 1
                            return len(payload)
                        def click_receipt(self, receipt):
                            return dict(joined=True, executed=True)
                        def batch(self, command=None):
                            if command:
                                _, x, y = command.split()
                                w["pointer"] = [int(x), int(y)]
                                state["pointed"] = int(y) == 700
                            state["frames"] += 1
                            path = self.folder / (str(state["frames"]) + ".png")
                            image = Image.new("RGB", (800, 900))
                            if state["pointed"] and failure != "stale":
                                ImageDraw.Draw(image).rectangle([200, 715, 400, 735], fill="white")
                            image.save(path)
                            return path

                    def native(args, **kwargs):
                        # Fake only the compiler/Vision subprocess. run(), hover()
                        # and text_boxes() must route every actual saved frame.
                        self.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
                        self.assertEqual(kwargs["stderr"], subprocess.DEVNULL)
                        if args[:2] == ["/usr/bin/xcrun", "swiftc"]:
                            state["compiles"] += 1
                            source = Path(args[2])
                            state["ocr"] = str(source.parent / "ocr")
                            self.assertEqual(args[3:], ["-o", state["ocr"]])
                            self.assertEqual(source.parent.stat().st_mode & 0o777, 0o700)
                            self.assertEqual(source.read_bytes(), (ROOT / "tools/vm/sim-posture-ocr.swift").read_bytes())
                            self.assertNotIn("DEVELOPER_DIR", kwargs["env"])
                            self.assertEqual(kwargs["stdout"], subprocess.DEVNULL)
                            self.assertEqual(kwargs["timeout"], 60)
                        else:
                            self.assertEqual(args[0], state["ocr"])
                            path = Path(args[1])
                            self.assertTrue(path.is_file())
                            cropped = path.name == "crop.png"
                            if cropped:
                                self.assertEqual(path.parent.parent, Path(state["ocr"]).parent)
                                with Image.open(path) as image:
                                    self.assertEqual(image.size, (240, 130))
                            else:
                                self.assertEqual(path, Path(state["ocr"]).parent / (str(state["frames"]) + ".png"))
                            state["recognized"].append(path)
                            self.assertEqual(kwargs["timeout"], 30)
                            if failure == "ocr-unavailable":
                                return subprocess.CompletedProcess(args, 1)
                            if failure == "ocr-timeout":
                                raise subprocess.TimeoutExpired(args, 30, stderr=b"PRIVATE native content")
                            if failure == "ocr-malformed":
                                kwargs["stdout"].write(b"PRIVATE malformed native content")
                                return subprocess.CompletedProcess(args, 0)
                            result = [] if cropped else [dict(
                                text="Other target" if failure == "wrong-title" else NAME,
                                confidence=.5 if failure == "weak-title" else 1.0,
                                box=[130/800, 60/900, 250/800, 18/900])]
                            # The full frame deliberately omits the small label.
                            if cropped and state["pointed"] and (failure != "delayed" or state["frames"] > 7):
                                label = "Partially Open" if failure == "partial" else pose.title()
                                result.append(dict(text=label, confidence=.5 if failure == "weak-tooltip" else .9,
                                                   box=[105/240, 85/130, 30/240, 14/130]))
                            kwargs["stdout"].write(json.dumps(result).encode())
                        return subprocess.CompletedProcess(args, 0)

                    ticks = iter(i / 20 for i in range(100000))
                    with mock.patch.object(p.time, "monotonic", side_effect=lambda: next(ticks)), \
                            mock.patch.object(p.session, "status", return_value=dict(connected=True)), \
                            mock.patch.object(p, "Guest", FakeGuest), mock.patch.object(p, "Holder", FakeHolder), \
                            mock.patch.object(p.subprocess, "run", side_effect=native), \
                            mock.patch.object(p, "read_calibration", return_value=record), \
                            mock.patch.object(p, "calibration_path", return_value=folder / "record"), \
                            mock.patch.object(p, "close_guest", return_value=(True, True)), mock.patch.object(p.time, "sleep"):
                        if failure not in (None, "delayed", "transient"):
                            with self.assertRaises((RuntimeError, ValueError, subprocess.TimeoutExpired)):
                                p.run(folder, "ssh", "guest", str(ROOT), UDID, "tart", pose, lease)
                        else:
                            receipt = p.run(folder, "ssh", "guest", str(ROOT), UDID, "tart", pose, lease)
                            self.assertEqual(receipt["click_count"], 1)
                            self.assertIn(pose.title(), receipt["control_evidence"])
                            self.assertEqual(state["displays"], 7 if failure == "transient" else 3)
                    self.assertEqual(state["compiles"], 1)
                    self.assertGreaterEqual(len(state["recognized"]), 1)
                    self.assertTrue(all(not path.parent.exists() for path in state["recognized"]))
                    self.assertEqual(state["clicks"], 1 if failure in (None, "delayed", "transient", "proof") else 0)
                    self.assertFalse((lease / "sim-posture-pending").exists())
                    if failure in ("arm-window", "arm-pointer", "reserve-window", "reserve-pointer", "final-unavailable"):
                        self.assertTrue(state["disarmed"])


    def test_posture_neutral_calibration_and_first_frame_proof_use_offscreen_and_hosted_paths(self):
        hosted = p.sibling("vnc-hosted")
        for remote in (False, True):
            for action, failures in (("calibrate", (None, "initial-title", "initial-window", "neutral-pointer", "neutral-window", "neutral-title", "approval-pointer")),
                                     ("open", (None, "late", "wrong", "pointer", "glyph", "legacy-delay", "legacy-expired"))):
                for failure in failures:
                    with self.subTest(remote=remote, action=action, failure=failure), tempfile.TemporaryDirectory() as directory:
                        root = Path(directory)
                        lease = root / "lease"
                        lease.mkdir()
                        w, geometry, _, record = ToolbarTests().fixture(root)
                        # Normal use and old hover-only calibration can leave
                        # the guest pointer on Open. No separate input route.
                        w["pointer"] = [330, 700]
                        legacy = failure in ("legacy-delay", "legacy-expired")
                        success = failure in (None, "legacy-delay")
                        if legacy:
                            record = dict(record, schema=2)
                            record.pop("glyphs")
                        clock = dict(now=100.0)
                        state = dict(clicks=0, frames=0, moves=[], pointed=False, approved=False, compiles=0)
                        events = []
                        test = self

                        def capture(folder, number, point=None):
                            if point is not None:
                                w["pointer"] = list(point)
                                state["moves"].append(list(point))
                                state["pointed"] = point[1] == 700
                                events.append("target-move" if state["pointed"] else "neutral-move")
                            state["frames"] += 1
                            # A second target frame would exceed the budget.
                            # Returning the move's complete frame must avoid it.
                            if state["pointed"]:
                                test.assertIn(state["frames"], (4, 5) if legacy else (4,))
                                clock["now"] += 12  # PNG capture + large transfer.
                            events.append("capture")
                            image = synthetic_toolbar(w, geometry, record["controls"],
                                                      "missing" if failure == "glyph" and state["pointed"] else None)
                            if legacy and state["pointed"] and state["frames"] == 5:
                                ImageDraw.Draw(image).rectangle([300, 720, 380, 735], fill="white")
                            path = folder / ("synthetic-%d.png" % number)
                            image.save(path)
                            return path

                        class Guest:
                            def __init__(self, ssh, request, log):
                                self.request, self.ready, self.child = request, dict(binding=record["binding"]), mock.Mock()
                            def call(self, operation):
                                events.append(operation)
                                if operation in ("window", "click-window"):
                                    value = copy.deepcopy(w)
                                    if (failure == "initial-window" and state["frames"] == 1
                                            or failure == "neutral-window" and state["moves"]):
                                        value["id"] += 1
                                    if failure == "neutral-pointer" and state["moves"]:
                                        value["pointer"][0] += 3
                                    if failure == "pointer" and state["pointed"]:
                                        value["pointer"][0] += 3
                                    return value
                                if operation == "displays":
                                    return {"1": png("black" if state["clicks"] else "white"),
                                            "3": png("white" if state["clicks"] else "black")}
                                if operation in ("arm", "publish"):
                                    return {"armed" if operation == "arm" else "published": True, "remaining": 600}
                                test.assertEqual(operation, "disarm")
                                return True

                        class LocalHolder(p.Holder):
                            def __init__(self, state_path, connected, guest, folder, lease):
                                self.connected, self.guest, self.folder, self.lease = connected, guest, folder, lease
                                self.log, self.pending, self.armed, self.capable = io.BytesIO(), False, False, True
                                info = lease.stat()
                                self.lease_identity = (info.st_dev, info.st_ino)
                            def batch(self, command=None):
                                point = None if command is None else [int(n) for n in command.split()[1:]]
                                return capture(self.folder, state["frames"] + 1, point)
                            def open_publication(self):
                                return None
                            def publication(self, fd, latest, receipt):
                                return min(time.monotonic() + 5, latest), b"synthetic-click"
                            def publish(self, fd, payload):
                                state["clicks"] += 1
                                return len(payload)
                            def click_receipt(self, receipt):
                                return dict(joined=True, executed=True)

                        def rpc(tart, message):
                            if message["op"] == "status":
                                return dict(connected=True, session="a" * 64)
                            op = message["operation"]
                            if op["op"] in ("capture", "move-capture"):
                                path = capture(root, state["frames"] + 1, op.get("point"))
                                return dict(png=base64.b64encode(path.read_bytes()).decode())
                            if op["op"] == "reserve":
                                clock["now"] += .1
                                return dict(reserved=True)
                            if op["op"] == "click":
                                state["clicks"] += 1
                                return {}
                            test.assertEqual(op["op"], "receipt")
                            return dict(session=op["session"], request=op["request"], guard=op["guard"],
                                        receipt=dict(joined=True, executed=True))

                        def native(args, **kwargs):
                            test.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
                            test.assertEqual(kwargs["stderr"], subprocess.DEVNULL)
                            if args[:2] == ["/usr/bin/xcrun", "swiftc"]:
                                state["compiles"] += 1
                                test.assertEqual(Path(args[2]).read_bytes(),
                                                 (ROOT / "tools/vm/sim-posture-ocr.swift").read_bytes())
                                test.assertEqual(kwargs["timeout"], 60)
                            else:
                                test.assertEqual(kwargs["timeout"], 30)
                                path = Path(args[1])
                                test.assertTrue(path.is_file())
                                events.append("crop-ocr" if path.name == "crop.png" else "full-ocr")
                                clock["now"] += 4  # Two OCR calls per target frame.
                                if (path.name == "crop.png" and
                                        (failure == "late" and state["pointed"]
                                         or failure == "legacy-expired" and state["frames"] == 5)):
                                    clock["now"] += p.HOVER_ACQUISITION_SECONDS
                                bad_title = (failure == "initial-title" and state["frames"] == 1
                                             or failure == "neutral-title" and state["frames"] > 1)
                                rows = [] if path.name == "crop.png" else [dict(
                                    text="Other target" if bad_title else NAME, confidence=1,
                                    box=[130/800, 60/900, 250/800, 18/900])]
                                if failure == "wrong" and state["pointed"] and path.name == "crop.png":
                                    rows.append(dict(text="Partially Open", confidence=1,
                                                     box=[105/240, 85/130, 30/240, 14/130]))
                                if legacy and state["pointed"] and state["frames"] == 5 and path.name == "crop.png":
                                    rows.append(dict(text="Open", confidence=1,
                                                     box=[105/240, 85/130, 30/240, 14/130]))
                                kwargs["stdout"].write(json.dumps(rows).encode())
                            return subprocess.CompletedProcess(args, 0)

                        def approve(path, name, actual_geometry):
                            test.assertEqual(w["pointer"], [300, 62])
                            test.assertEqual(state["frames"], 3)
                            test.assertEqual(state["moves"], [[300, 62]])
                            test.assertEqual(events[-1], "window")
                            test.assertTrue(path.is_file())
                            test.assertEqual(name, NAME)
                            test.assertEqual(actual_geometry, geometry)
                            events.append("approve")
                            state["approved"] = True
                            if failure == "approval-pointer":
                                w["pointer"][0] += 3
                            return "compact", record["controls"]

                        def sleep(seconds):
                            clock["now"] += seconds

                        with ExitStack() as stack:
                            stack.enter_context(mock.patch.object(p.session, "status", return_value=dict(connected=True)))
                            stack.enter_context(mock.patch.object(p.session, "hosted_state", return_value=remote))
                            stack.enter_context(mock.patch.object(p, "Guest", Guest))
                            stack.enter_context(mock.patch.object(p, "Holder", LocalHolder))
                            stack.enter_context(mock.patch.object(p, "sibling", return_value=hosted))
                            stack.enter_context(mock.patch.object(hosted, "sibling", return_value=p))
                            stack.enter_context(mock.patch.object(hosted, "endpoint", return_value=("wrapper", "guest")))
                            configuration = stack.enter_context(mock.patch.object(hosted.wrapper, "configuration",
                                return_value=("fake-m3-alias", "/private-kit")))
                            stack.enter_context(mock.patch.object(hosted, "rpc", side_effect=rpc))
                            stack.enter_context(mock.patch.object(p.subprocess, "run", side_effect=native))
                            stack.enter_context(mock.patch.object(p, "operator_terminal", side_effect=lambda: os.open(os.devnull, os.O_RDONLY)))
                            stack.enter_context(mock.patch.object(p, "operator_controls", side_effect=approve))
                            stack.enter_context(mock.patch.object(p, "calibration_path", return_value=root / "calibration.json"))
                            stack.enter_context(mock.patch.object(p, "read_calibration", return_value=record))
                            stack.enter_context(mock.patch.object(p, "close_guest", return_value=(True, True)))
                            stack.enter_context(mock.patch.object(p.time, "monotonic", side_effect=lambda: clock["now"]))
                            stack.enter_context(mock.patch.object(p.time, "sleep", side_effect=sleep))
                            if not success:
                                with self.assertRaises(RuntimeError):
                                    p.run(root, "ssh", "guest", str(ROOT), UDID, "wrapper", action, lease)
                            else:
                                result = p.run(root, "ssh", "guest", str(ROOT), UDID, "wrapper", action, lease)
                                test.assertEqual(result["click_count"], int(action == "open"))
                        if remote and action == "open" and success:
                            configuration.assert_called_once_with("wrapper")
                        else:
                            configuration.assert_not_called()
                        self.assertEqual(state["compiles"], 1)
                        self.assertEqual(state["clicks"], int(action == "open" and success))
                        if action == "calibrate":
                            self.assertEqual(state["approved"], failure in (None, "approval-pointer"))
                            self.assertEqual((root / "calibration.json").exists(), failure is None)
                            self.assertNotIn("arm", events)
                            self.assertNotIn("publish", events)
                            self.assertNotIn("target-move", events)
                            if success:
                                self.assertEqual(state["moves"], [[300, 62]])
                                self.assertEqual(state["frames"], 5)
                            if failure in ("initial-title", "initial-window"):
                                self.assertEqual(state["moves"], [])
                        elif success:
                            self.assertEqual(state["frames"], 5 if legacy else 4)
                            self.assertLess(events.index("target-move"), events.index("arm"))
                            self.assertEqual(events.count("displays"), 3)
                        else:
                            self.assertNotIn("arm", events)
                        self.assertFalse((lease / "sim-posture-pending").exists())


class GuardTests(unittest.TestCase):
    def test_posture_final_native_admission_reuses_only_unchanged_signed_facts(self):
        with tempfile.TemporaryDirectory() as folder:
            hub = Path(folder) / "DeviceHub.app"
            binaries = hub / "Contents/MacOS"
            binaries.mkdir(parents=True)
            for name in ("DeviceHub", "DevicesTrampoline"):
                (binaries / name).write_bytes(b"synthetic")
            info = hub / "Contents/Info.plist"
            info.write_bytes(plistlib.dumps(dict(CFBundleIdentifier="com.apple.dt.Devices",
                                                 CFBundleExecutable="DevicesTrampoline")))
            resources = hub / "Contents/_CodeSignature/CodeResources"
            resources.parent.mkdir()
            resources.write_bytes(b"synthetic signature resources")
            signed = mock.Mock()
            scope = dict(hub=hub, hub_requirement="synthetic requirement", pathlib=pathlib,
                         plistlib=plistlib, require=p.require, run=signed)
            exec(p.WINDOW_ADMISSION, scope)
            scope["kernel_executable"] = mock.Mock(return_value=str(binaries / "DeviceHub"))
            owner = dict(window(), bundle=str(hub), executable=str(binaries / "DeviceHub"))
            evidence = native_window(owner)
            evidence["front"].update(executable=str(binaries / "DevicesTrampoline"))
            admit = scope["admit_window"]
            with self.assertRaises(RuntimeError):
                admit(evidence, final=True)  # No prior signature proof.
            self.assertEqual(admit(evidence), owner)
            signed.reset_mock()
            self.assertEqual(admit(evidence, final=True), owner)
            for part, field, value in (("front", "active", False), ("owner", "pid", 456),
                                       ("owner", "bundle", "/Other.app"), ("owner", "lookup_pid", 456)):
                changed = copy.deepcopy(evidence)
                row = changed["front"] if part == "front" else changed["normal"][0]
                row[field] = value
                with self.subTest(part=part, field=field), self.assertRaises(RuntimeError):
                    admit(changed, final=True)
            signed.assert_not_called()
            for path in (info, resources, binaries / "DevicesTrampoline", binaries / "DeviceHub"):
                with self.subTest(changed_file=path.name):
                    admit(evidence)  # Establish new full proof outside the ticket.
                    signed.reset_mock()
                    path.write_bytes(path.read_bytes() + b" ")
                    with self.assertRaises(RuntimeError):
                        admit(evidence, final=True)
                    signed.assert_not_called()  # Fail closed; never redo codesign here.

    def test_posture_kernel_path_lookup_is_bounded_and_uses_only_the_positive_cg_pid(self):
        scope = dict(require=p.require)
        exec(p.WINDOW_ADMISSION, scope)
        lookup = scope["kernel_executable"]
        path = b"/Selected/DeviceHub.app/Contents/MacOS/DeviceHub"
        libproc = mock.Mock()
        def proc_pidpath(pid, buffer, size):
            self.assertEqual(pid, 123)
            self.assertEqual(size, 4096)
            buffer.value = path
            return len(path)
        libproc.proc_pidpath.side_effect = proc_pidpath
        with mock.patch.object(ctypes, "CDLL", return_value=libproc) as load:
            self.assertEqual(lookup(123), path.decode())
            load.assert_called_once_with("/usr/lib/libproc.dylib")
            self.assertEqual(libproc.proc_pidpath.argtypes,
                             [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32])
            self.assertIs(libproc.proc_pidpath.restype, ctypes.c_int)
            for pid in (-1, 0, True, 2147483648):
                with self.assertRaises(RuntimeError):
                    lookup(pid)
            for result in (0, -1, 4097):
                libproc.proc_pidpath.side_effect = None
                libproc.proc_pidpath.return_value = result
                with self.assertRaises(RuntimeError):
                    lookup(123)
            def unterminated(pid, buffer, size):
                buffer.raw = b"x" * size
                return size
            libproc.proc_pidpath.side_effect = unterminated
            with self.assertRaises(RuntimeError):
                lookup(123)

    def test_posture_split_front_launcher_and_window_owner_require_the_exact_signed_installation(self):
        with tempfile.TemporaryDirectory() as folder:
            hub = Path(folder) / "Selected/DeviceHub.app"
            binaries = hub / "Contents/MacOS"
            binaries.mkdir(parents=True)
            for name in ("DeviceHub", "DevicesTrampoline", "OtherAppleTool"):
                (binaries / name).write_bytes(b"synthetic")
            (hub / "Contents/Info.plist").write_bytes(plistlib.dumps(dict(
                CFBundleIdentifier="com.apple.dt.Devices", CFBundleExecutable="DevicesTrampoline")))
            requirement = '=anchor apple and identifier "com.apple.dt.Devices"'
            signed = mock.Mock()
            scope = dict(hub=hub, hub_requirement=requirement, pathlib=pathlib, plistlib=plistlib,
                         require=p.require, run=signed)
            exec(p.WINDOW_ADMISSION, scope)
            lookup = mock.Mock(return_value=str(binaries / "DeviceHub"))
            scope["kernel_executable"] = lookup
            admit = scope["admit_window"]
            owner = dict(window(), bundle=str(hub), executable=str(binaries / "DeviceHub"))
            evidence = native_window(owner)
            evidence["front"].update(pid=456, executable=str(binaries / "DevicesTrampoline"))
            original = admit(evidence)
            self.assertEqual(original, owner)  # Bind the actual window owner, not the launcher.
            lookup.assert_called_once_with(owner["pid"])
            expected = [mock.call("/usr/bin/codesign", "--verify", "--strict", "-R", requirement, str(path))
                        for path in (hub, binaries / "DevicesTrampoline", binaries / "DeviceHub")]
            self.assertEqual(signed.call_args_list, expected)
            # Actual observed combination: separate launcher, sentinel lookup
            # for the real CG owner, active/finished and exact kernel binary.
            sentinel = copy.deepcopy(evidence)
            sentinel["front"]["pid"] = -1
            sentinel["normal"][0]["lookup_pid"] = -1
            admitted = admit(sentinel)
            self.assertEqual(admitted["pid"], 123)
            self.assertEqual(admitted["lookup_pid"], -1)
            self.assertTrue(admitted["active"])
            self.assertFalse(admitted["terminated"])
            self.assertTrue(admitted["finished_launching"])
            guest = mock.Mock()
            guest.call.side_effect = lambda *args: admit(sentinel)
            p.stable_window(guest, original)  # The verified lookup PID isn't the CG binding.
            for wrong in (str(binaries / "DevicesTrampoline"), str(binaries / "OtherAppleTool"),
                          str(hub.parent / "Other.app/Contents/MacOS/DeviceHub"), ""):
                lookup.return_value = wrong
                with self.assertRaises(RuntimeError):
                    admit(sentinel)
                self.assertEqual(scope["stage"], "window-owner-kernel")
            lookup.return_value = str(binaries / "DeviceHub")
            for lookup_pid in (456, 0, -2, True, None):
                wrong = copy.deepcopy(sentinel)
                wrong["normal"][0]["lookup_pid"] = lookup_pid
                with self.assertRaises(RuntimeError):
                    admit(wrong)
                self.assertEqual(scope["stage"], "window-owner-lookup")
            missing = copy.deepcopy(sentinel)
            del missing["normal"][0]["lookup_pid"]
            with self.assertRaises(RuntimeError):
                admit(missing)
            self.assertEqual(scope["stage"], "window-owner-lookup")
            missing = copy.deepcopy(sentinel)
            missing["normal"][0] = {k: missing["normal"][0][k] for k in ("pid", "id", "rect", "title")}
            with self.assertRaises(RuntimeError):
                admit(missing)
            for field in ("active", "terminated", "finished_launching"):
                wrong = copy.deepcopy(sentinel)
                wrong["normal"][0][field] = not wrong["normal"][0][field]
                with self.assertRaises(RuntimeError):
                    admit(wrong)
                self.assertEqual(scope["stage"], "window-owner-identity")
            lower = dict(pid=789, id=24, rect=[20, 20, 300, 200], title="")
            foreign_path = str(hub.parent / "Other.app/Contents/MacOS/Other")
            foreign = dict(lower, lookup_pid=789, bundle=str(hub.parent / "Other.app"),
                           executable=foreign_path)
            # Missing secondary AppKit lookup still counts every selected
            # internal binary, including the launcher and other companions.
            cases = [(lower, str(binaries / name), False)
                     for name in ("DeviceHub", "DevicesTrampoline", "OtherAppleTool")]
            cases += [
                (lower, None, False),  # An unresolved PID cannot prove uniqueness.
                (dict(lower, pid=0), foreign_path, False),
                (dict(lower, pid=-1), foreign_path, False),
                (dict(lower, pid=True), foreign_path, False),
                (lower, foreign_path, True),  # Kernel-only unrelated owner.
                (foreign, foreign_path, True),  # Known unrelated lower window.
                (foreign, str(binaries / "DeviceHub"), False),  # Conflicting bundle/path.
                (foreign, foreign_path + "Changed", False),  # Conflicting executables.
                (dict(foreign, lookup_pid=456), foreign_path, False),
                (dict(lower, pid=owner["pid"]), foreign_path, False),  # Owned/unknown overlap.
                (dict(lower, executable=str(binaries / "DeviceHub")), foreign_path, False),
                (dict(lower, bundle=str(hub)), foreign_path, False),
            ]
            for row, path, accepted in cases:
                with self.subTest(row=row, path=path, accepted=accepted):
                    def kernel(pid):
                        p.require(type(pid) is int and 0 < pid <= 2147483647)
                        if pid == owner["pid"]:
                            return str(binaries / "DeviceHub")
                        self.assertEqual(pid, 789)
                        p.require(path is not None)
                        return path
                    lookup.reset_mock()
                    lookup.side_effect = kernel
                    changed = dict(evidence, normal=evidence["normal"] + [row])
                    if accepted:
                        self.assertEqual(admit(changed), owner)
                        self.assertEqual(lookup.call_args_list,
                                         [mock.call(789), mock.call(owner["pid"])])
                    else:
                        with self.assertRaises(RuntimeError):
                            admit(changed)
            lookup.side_effect = None
            # Native two-window layout: the exact leased compact canvas is
            # globally first; the same signed Hub's previous device/management
            # window remains behind, even with a different title and geometry.
            lower_hub = dict(evidence["normal"][0], id=24, title="Previous iPhone",
                             rect=[20, 20, 700, 800])
            for lower_pid, lower_lookup in ((123, 123), (123, -1), (456, 456), (456, -1)):
                other = dict(lower_hub, pid=lower_pid, lookup_pid=lower_lookup)
                two = dict(evidence, normal=evidence["normal"] + [other])
                self.assertEqual(admit(two), owner)
                guest = mock.Mock()
                guest.call.side_effect = lambda *args: admit(two)
                p.stable_window(guest, original)
            two = dict(evidence, normal=evidence["normal"] + [lower_hub])
            # A change of active canvas refuses even if both owners verify.
            guest.call.side_effect = lambda *args: admit(dict(two, normal=list(reversed(two["normal"]))))
            with self.assertRaisesRegex(RuntimeError, "foreground window changed"):
                p.stable_window(guest, original)
            with self.assertRaises(RuntimeError):
                p.window_binding(admit(dict(two, normal=list(reversed(two["normal"])))), (800, 900), NAME)
            # Every selected lower row needs complete actual-owner admission.
            for field, value in (("pid", 0), ("pid", -1), ("pid", True), ("pid", None),
                                 ("pid", 2147483648), ("lookup_pid", 456), ("lookup_pid", 0),
                                 ("lookup_pid", True), ("lookup_pid", None),
                                 ("bundle", ""), ("bundle", str(hub.parent / "Other.app")),
                                 ("bundle_id", "com.apple.other"), ("executable", ""),
                                 ("executable", str(binaries / "DevicesTrampoline")),
                                 ("executable", str(binaries / "OtherAppleTool")),
                                 ("active", False), ("terminated", True), ("finished_launching", False)):
                with self.subTest(lower_field=field, value=value):
                    other = dict(lower_hub, **{field: value})
                    with self.assertRaises(RuntimeError):
                        admit(dict(two, normal=[two["normal"][0], other]))
            for field in ("pid", "lookup_pid", "bundle", "bundle_id", "executable",
                          "active", "terminated", "finished_launching"):
                other = dict(lower_hub)
                del other[field]
                with self.subTest(missing_lower_field=field), self.assertRaises((RuntimeError, KeyError)):
                    admit(dict(two, normal=[two["normal"][0], other]))
            # Missing/duplicate/noninteger IDs never establish CG order or a
            # unique top binding, including a duplicate on an unrelated owner.
            for index in (0, 1):
                for value in (None, 0, -1, True, 23.0, 23 if index == 1 else 24):
                    changed = copy.deepcopy(two)
                    changed["normal"][index]["id"] = value
                    with self.subTest(index=index, window_id=value), self.assertRaises(RuntimeError):
                        admit(changed)
                changed = copy.deepcopy(two)
                del changed["normal"][index]["id"]
                with self.assertRaises(RuntimeError):
                    admit(changed)
            with self.assertRaises(RuntimeError):
                admit(dict(evidence, normal=evidence["normal"] + [dict(foreign, id=23)]))
            # Correct target evidence behind foreign or unknown top windows
            # cannot select it. Kernel identity for unrelated rows stays checked.
            for top in (foreign, lower):
                lookup.side_effect = lambda pid: (str(binaries / "DeviceHub") if pid == 123 else foreign_path)
                with self.subTest(top=top), self.assertRaises(RuntimeError):
                    admit(dict(evidence, normal=[top] + evidence["normal"]))
            lookup.side_effect = lambda pid: (str(binaries / "DeviceHub") if pid == 123 else foreign_path)
            with self.assertRaises(RuntimeError):
                admit(dict(evidence, normal=evidence["normal"] +
                           [dict(lower_hub, pid=789, lookup_pid=789)]))
            lookup.side_effect = None
            # The lower actual owner must also pass strict code verification.
            signed.reset_mock()
            signed.side_effect = [None, None, None, RuntimeError("lower signature failed")]
            with self.assertRaisesRegex(RuntimeError, "lower signature failed"):
                admit(two)
            signed.side_effect = None
            for front in (dict(evidence["front"], pid=-1), dict(evidence["front"], pid=789),
                          native_window(owner)["front"]):
                fresh = dict(evidence, front=front)
                guest = mock.Mock()
                guest.call.side_effect = lambda *args: admit(fresh)
                p.stable_window(guest, original)  # Freshly admitted front PID may change.
            bad = []
            for part in ("front", "owner"):
                for field, value in (("pid", 0), ("pid", -2), ("pid", True),
                                     ("active", False), ("terminated", True), ("finished_launching", False),
                                     ("bundle", str(hub.parent / "Other/DeviceHub.app")),
                                     ("bundle_id", "com.apple.other"),
                                     ("executable", str(binaries / "OtherAppleTool"))):
                    changed = copy.deepcopy(evidence)
                    target = changed["front"] if part == "front" else changed["normal"][0]
                    target[field] = value
                    bad.append(changed)
                    if part == "front":
                        sentinel = copy.deepcopy(changed)
                        sentinel["front"]["pid"] = -1
                        if field != "pid":
                            bad.append(sentinel)
            bad.append(dict(evidence, normal=[dict(evidence["normal"][0], pid=-1)]))
            bad.append(dict(evidence, normal=[dict(evidence["normal"][0], bundle="/Other.app"),
                                             evidence["normal"][0]]))  # Obscured.
            for changed in bad:
                with self.assertRaises(RuntimeError):
                    admit(changed)
            # The native helper's observed -1 front entry never supplies the
            # bound owner, and never admits an initially unrelated top window.
            ready = copy.deepcopy(evidence)
            ready["front"]["pid"] = -1
            obscured = dict(ready, normal=[dict(ready["normal"][0], bundle="/Other.app"),
                                           ready["normal"][0]])
            check_lease = mock.Mock()
            clock = mock.Mock()
            clock.monotonic.return_value = 0
            read = mock.Mock(side_effect=[json.dumps(obscured), json.dumps(ready)])
            def observe_run(*args):
                return read(*args) if args == ("visual", "window") else signed(*args)
            scope.update(visual="visual", json=json, time=clock, check_lease=check_lease, run=observe_run)
            observe = scope["observe_window"]
            self.assertEqual(observe(True), owner)
            self.assertEqual(read.call_count, 2)
            self.assertEqual(check_lease.call_count, 2)
            clock.sleep.assert_called_once_with(.25)
            for initial in (False, True):
                with self.subTest(initial=initial):
                    read.reset_mock(side_effect=True)
                    read.side_effect = [json.dumps(obscured)] * 2
                    clock.reset_mock(side_effect=True)
                    clock.monotonic.side_effect = [0, 0, 5]
                    with self.assertRaises(RuntimeError):
                        observe(initial)
                    self.assertEqual(read.call_count, 2 if initial else 1)
                    self.assertEqual(clock.sleep.call_count, 1 if initial else 0)
            read.reset_mock(side_effect=True)
            read.side_effect = [json.dumps(obscured), json.dumps(ready)]
            clock.reset_mock(side_effect=True)
            clock.monotonic.return_value = 0
            check_lease.side_effect = [None, RuntimeError("lease changed during initial transition")]
            with self.assertRaisesRegex(RuntimeError, "lease changed"):
                observe(True)
            self.assertEqual(read.call_count, 1)
            for invalid in (hub, binaries / "DevicesTrampoline", binaries / "DeviceHub"):
                def verify(*args):
                    if args[-1] == str(invalid):
                        raise RuntimeError("invalid signature or code identifier")
                signed.side_effect = verify
                with self.assertRaises(RuntimeError):
                    admit(evidence)

    def test_posture_guest_mutex_holds_exact_lease_through_input_and_proof(self):
        self.check_guard_scenarios(("read-only", "armed-only", "short", "error", "joined", "pending", "timeout", "bad-frame"))

    def test_posture_guest_refuses_lease_expired_during_compile_before_open(self):
        self.check_guard_scenarios(("expired-before-open",))

    def test_posture_terminal_cancellation_orders_with_paused_real_publish(self):
        self.check_guard_scenarios(("cancel-before-lock", "cancel-at-marker", "foreign-at-marker"))

    def check_guard_scenarios(self, scenarios):
        for scenario in scenarios:
            end_pending = scenario in ("pending", "foreign-at-marker")
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                developer = root / "Selected Xcode.app/Contents/Developer"
                developer.mkdir(parents=True)
                hub = developer.parent / "Applications/DeviceHub.app"
                (hub / "Contents/MacOS").mkdir(parents=True)
                (hub / "Contents/MacOS/DevicesTrampoline").write_bytes(b"synthetic")
                (hub / "Contents/MacOS/DeviceHub").write_bytes(b"synthetic")
                (hub / "Contents/Info.plist").write_bytes(plistlib.dumps(dict(
                    CFBundleIdentifier="com.apple.dt.Devices", CFBundleExecutable="DevicesTrampoline")))
                (developer.parent / "Info.plist").write_bytes(plistlib.dumps(dict(CFBundleShortVersionString="27.2")))
                (developer.parent / "version.plist").write_bytes(plistlib.dumps(dict(ProductBuildVersion="selected")))
                pool = root / "pool"
                lease = pool / "leases" / UDID / "lease.json"
                lease.parent.mkdir(parents=True)
                lease.write_text(json.dumps(dict(udid=UDID, vm="guest", kind="foldable", worktree="/task",
                                                 expires_epoch=time.time() + (1 if scenario == "short" else 600))))
                original = lease.read_bytes()
                pin = json.loads((ROOT / "tools/sim-devices.json").read_text())["foldable"]["runtime"]
                request = dict(vm="guest", checkout="/task", udid=UDID, pool=str(pool), pin=pin, visual="fixed source", guard="test-guard")
                actions = {
                    "read-only": ["window", "finish"], "armed-only": ["arm", "finish"],
                    "short": ["arm"], "error": ["arm", "disarm", "finish"],
                    "joined": ["window", "arm", "publish", "click-window", "displays", "disarm", "finish"],
                    "pending": ["arm", "publish"], "timeout": [], "bad-frame": [], "expired-before-open": [],
                    "cancel-before-lock": ["arm", "publish"],
                    "cancel-at-marker": ["arm", "publish"], "foreign-at-marker": ["arm", "publish"],
                }[scenario]
                wire = b"".join((json.dumps(dict(id=i, action=a)) + "\n").encode() for i, a in enumerate(actions, 1))
                if scenario == "bad-frame":
                    wire = b'{"id":true,"action":"arm"}\n'
                source = io.TextIOWrapper(io.BytesIO(wire))
                if scenario == "timeout":
                    source = mock.Mock()
                    source.buffer.readline.side_effect = TimeoutError("read-only guard alarm")
                calls = []
                def output(args, **kwargs):
                    calls.append(args)
                    self.assertTrue((pool / "pool.lock/pid").exists())
                    self.assertEqual(lease.read_bytes(), original)
                    if args[0] == "/usr/bin/xcode-select":
                        return str(developer).encode()
                    if args[0] in ("/usr/bin/codesign", "/usr/bin/open"):
                        return b""
                    if args[0].endswith("/visual"):
                        evidence = native_window(dict(window(), bundle=str(hub),
                            executable=str(hub / "Contents/MacOS/DeviceHub")))
                        evidence["front"]["pid"] = -1
                        evidence["normal"][0]["lookup_pid"] = -1
                        return json.dumps(evidence).encode()
                    self.assertEqual(args[0], str(developer / "usr/bin/simctl"))
                    if args[1] == "io":
                        self.assertEqual(args[2:4], (UDID, "screenshot"))
                        self.assertTrue((pool / "pool.lock/sim-posture-pending").is_dir())
                        Path(args[-1]).write_bytes(b"synthetic PNG transport")
                        return b""
                    inventories = dict(devices={pin["identifier"]: [dict(udid=UDID, name=NAME,
                                       state="Booted", isAvailable=True, deviceTypeIdentifier="duo")]},
                                       runtimes=[dict(identifier=pin["identifier"], version=pin["version"],
                                                      buildversion=pin["build"], isAvailable=True)],
                                       devicetypes=[dict(identifier="duo", name="iPhone Duo", productFamily="iPhone")])
                    return json.dumps({args[2]: inventories[args[2]]}).encode()
                original_stat = Path.stat
                def path_stat(path, *args, **kwargs):
                    return type("Console", (), dict(st_uid=501))() if str(path) == "/dev/console" else original_stat(path, *args, **kwargs)
                # Run the real terminal script against the real publish path.
                # Exercise both orders: cancellation wins before serialization,
                # or it arrives just before mkdir and must wait for publication.
                racing = scenario in ("cancel-before-lock", "cancel-at-marker", "foreign-at-marker")
                terminal_requested, terminal_done = threading.Event(), threading.Event()
                terminal_errors = []
                def terminal():
                    try:
                        terminal_request = dict(request, guard="foreign" if scenario == "foreign-at-marker" else request["guard"])
                        with mock.patch.object(sys, "argv", ["terminal", json.dumps(terminal_request)]):
                            exec(p.TERMINAL, {})
                    except BaseException as error:
                        terminal_errors.append(error)
                    finally:
                        terminal_done.set()
                worker = threading.Thread(target=terminal, daemon=True)
                main_thread = threading.current_thread()
                real_flock, real_mkdir = p.fcntl.flock, os.mkdir
                first_lock = True
                def flock(fd, operation):
                    nonlocal first_lock
                    if operation == p.fcntl.LOCK_EX:
                        if threading.current_thread() is worker:
                            terminal_requested.set()
                        elif first_lock:
                            first_lock = False
                            if scenario == "cancel-before-lock":
                                worker.start()
                                self.assertTrue(terminal_done.wait(5))
                    return real_flock(fd, operation)
                def mkdir(path, *args, **kwargs):
                    if racing and scenario != "cancel-before-lock" and path == "sim-posture-pending":
                        self.assertIs(threading.current_thread(), main_thread)
                        worker.start()
                        self.assertTrue(terminal_requested.wait(5))
                        self.assertFalse(terminal_done.is_set())  # Publish still holds the inode lock.
                    return real_mkdir(path, *args, **kwargs)
                def reply(value, **kwargs):
                    if racing and '"published": true' in value:
                        self.assertTrue(terminal_done.wait(5))
                def proc_pidpath(pid, buffer, size):
                    self.assertEqual(pid, 123)
                    self.assertEqual(size, 4096)
                    buffer.value = str(hub / "Contents/MacOS/DeviceHub").encode()
                    return len(buffer.value)
                libproc = mock.Mock()
                libproc.proc_pidpath.side_effect = proc_pidpath
                now = [time.time()]
                def compile_visual(*args, **kwargs):
                    if scenario == "expired-before-open":
                        now[0] += 601
                with mock.patch.object(sys, "argv", ["guard", json.dumps(request)]), \
                        mock.patch.object(ctypes, "CDLL", return_value=libproc), \
                        mock.patch.object(sys, "stdin", source), mock.patch.object(os, "getuid", return_value=501), \
                        mock.patch.object(Path, "stat", path_stat), \
                        mock.patch.object(subprocess, "check_output", side_effect=output), \
                        mock.patch.object(subprocess, "run", side_effect=compile_visual), \
                        mock.patch.object(time, "time", side_effect=lambda: now[0]), mock.patch.object(p.signal, "signal"), \
                        mock.patch.object(p.signal, "alarm"), mock.patch("builtins.print", side_effect=reply), \
                        mock.patch.object(p.fcntl, "flock", side_effect=flock), mock.patch.object(os, "mkdir", side_effect=mkdir):
                    if scenario in ("pending", "short", "timeout", "bad-frame", "expired-before-open") or racing:
                        with self.assertRaises(SystemExit) as stopped:
                            exec(p.GUEST, {})
                        self.assertEqual(stopped.exception.code, 1)
                    else:
                        exec(p.GUEST, {})
                if racing:
                    worker.join(5)
                    self.assertFalse(worker.is_alive())
                    self.assertEqual(terminal_errors, [])
                self.assertEqual((pool / "pool.lock/sim-posture-pending").exists(), end_pending)
                self.assertEqual((pool / "pool.lock").exists(), end_pending)
                self.assertEqual(lease.read_bytes(), original)
                self.assertFalse(any("boot" in c or "reset" in c for c in calls))
                opens = [c for c in calls if c[0] == "/usr/bin/open"]
                self.assertEqual(opens, [] if scenario == "expired-before-open" else [
                    ("/usr/bin/open", "-a", str(hub), "devices://device/open?id=" + UDID)])
                if scenario.startswith("cancel-"):
                    # No residual token files or fences can block the next owner.
                    (pool / "pool.lock").mkdir()
                    (pool / "pool.lock").rmdir()
                    self.assertEqual(list(pool.iterdir()), [pool / "leases"])

    def test_posture_terminal_disarm_removes_only_the_exact_owned_guard(self):
        for token, replaced in (("owned", False), ("foreign", False), ("owned", True)):
            with self.subTest(token=token, replaced=replaced), tempfile.TemporaryDirectory() as folder:
                pool = Path(folder)
                lock = pool / "pool.lock"
                lock.mkdir()
                (lock / "posture-owner").write_text(token + "\nP")
                (lock / "sim-posture-pending").mkdir()
                request = dict(pool=str(pool), guard="owned")
                original_flock = p.fcntl.flock
                def flock(fd, operation):
                    if replaced and operation == p.fcntl.LOCK_EX:
                        # Delay cleanup after it has opened its exact owner. A
                        # replacement must survive even when the old token matches.
                        lock.rename(pool / "retired")
                        lock.mkdir()
                        (lock / "posture-owner").write_text("replacement\nP")
                        (lock / "sim-posture-pending").mkdir()
                    return original_flock(fd, operation)
                with mock.patch.object(sys, "argv", ["terminal", json.dumps(request)]), mock.patch("builtins.print"), \
                        mock.patch.object(p.fcntl, "flock", side_effect=flock):
                    exec(p.TERMINAL, {})
                original = pool / "retired" if replaced else lock
                self.assertEqual((original / "sim-posture-pending").exists(), token != "owned")
                self.assertEqual((original / "posture-owner").read_text(), token + ("\nT" if token == "owned" else "\nP"))
                if replaced:
                    self.assertTrue((lock / "sim-posture-pending").exists())
                    self.assertEqual((lock / "posture-owner").read_text(), "replacement\nP")
                self.assertTrue(lock.exists())  # No foreign/live lock reclamation.


if __name__ == "__main__":
    unittest.main()
