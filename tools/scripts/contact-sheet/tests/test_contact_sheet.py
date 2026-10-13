"""Tests for contact_sheet (U8, R1976), on fixture videos made with ffmpeg.

Each fixture's expected frames, cuts and stalls follow from how it is built
(frame rate, durations, where the picture changes), not from the code under
test. Run in the image-tools container (`tools/mg image-tools-selftest`).
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(os.path.dirname(HERE), "contact_sheet.py")
sys.path.insert(0, os.path.dirname(HERE))

import contact_sheet  # noqa: E402


def ffmpeg(*args):
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args], check=True)


def lossless(graph, path, *extra):
    ffmpeg("-f", "lavfi", "-i", graph, *extra, "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv420p", path)


def gray_pixels(png):
    """(width, height, bytes) of a PNG as 8-bit gray."""
    probe = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height",
                            "-of", "json", png], check=True, capture_output=True, text=True)
    stream = json.loads(probe.stdout)["streams"][0]
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", png, "-f", "rawvideo", "-pix_fmt", "gray", "-"],
                         check=True, capture_output=True).stdout
    return stream["width"], stream["height"], raw


def mean(raw, width, x0, y0, x1, y1):
    total = count = 0
    for y in range(y0, y1):
        row = raw[y * width + x0:y * width + x1]
        total += sum(row)
        count += len(row)
    return total / count


class Fixtures(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        d = cls.tmp.name
        # 3 s at 30 fps; frame n has luma 16 + 2n, so later frames are brighter.
        cls.gradient = os.path.join(d, "gradient.mp4")
        lossless("color=c=black:s=160x120:r=30:d=3,geq=lum='16+2*N':cb=128:cr=128", cls.gradient)
        # 1 s of gray, then the test pattern: an abrupt cut at frame 30 (t = 1.0).
        cls.cut = os.path.join(d, "cut.mp4")
        lossless("color=c=gray:s=160x120:r=30:d=1[a];testsrc=s=160x120:r=30:d=1[b];[a][b]concat=n=2:v=1:a=0", cls.cut)
        # A 1 s fade from black: every frame changes a little, none abruptly.
        cls.fade = os.path.join(d, "fade.mp4")
        lossless("color=c=white:s=160x120:r=30:d=2,fade=t=in:st=0.5:d=1", cls.fade)
        # A box moving right, still from t = 0.5 to 1.0, then moving again:
        # the last change before the stall is frame 15 (0.5 s), the first after
        # it frame 31 (1.0333 s).
        cls.stall = os.path.join(d, "stall.mp4")
        lossless("color=c=black:s=320x120:r=30:d=2[bg];color=c=white:s=20x20:r=30:d=2[box];"
                 "[bg][box]overlay=x='if(lt(t,0.5),t*200,if(lt(t,1),100,100+(t-1)*200))':y=50:shortest=1", cls.stall)
        # Still for 1 s, a box moving from t = 1.0 to 1.5, still again until
        # 3 s: the first changed frame is 31 (1.0333 s), the last 45 (1.5 s).
        cls.move = os.path.join(d, "move.mp4")
        lossless("color=c=black:s=320x120:r=30:d=3[bg];color=c=white:s=20x20:r=30:d=3[box];"
                 "[bg][box]overlay=x='if(lt(t,1),0,if(lt(t,1.5),(t-1)*400,200))':y=50:shortest=1", cls.move)
        # Variable frame rate, as simctl records: the gradient without frames
        # 10-19, so frame 9 (0.3 s) stays on screen until frame 20 (0.6667 s).
        cls.vfr = os.path.join(d, "vfr.mp4")
        ffmpeg("-i", cls.gradient, "-vf", "select='not(between(n,10,19))'", "-fps_mode", "vfr",
               "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv420p", cls.vfr)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def out(self):
        d = tempfile.mkdtemp(dir=self.tmp.name)
        return d


class ContactSheet(Fixtures):
    def test_a_window_gives_a_sheet_of_twelve_tiles_in_time_order(self):
        out = self.out()
        report = contact_sheet.run(self.gradient, out, windows=[("reveal", 0.5, 2.5)])
        window = report["windows"][0]
        self.assertEqual(window["name"], "reveal")
        self.assertEqual(len(window["tiles"]), 12)
        times = [t["time"] for t in window["tiles"]]
        self.assertEqual(times[0], 0.5)
        self.assertAlmostEqual(times[-1], 2.5, places=3)
        self.assertEqual(times, sorted(times))
        # The frame on screen at t is the last one at or before t (30 fps).
        self.assertEqual([t["frame"] for t in window["tiles"]], [int(t * 30 + 1e-6) for t in times])

        sheet = window["sheet"]
        self.assertEqual(os.path.dirname(sheet), out)
        width, height, raw = gray_pixels(sheet)
        cols, rows = 4, 3
        tile_w = (width - contact_sheet.MARGIN * 2 - contact_sheet.PADDING * (cols - 1)) // cols
        tile_h = (height - contact_sheet.MARGIN * 2 - contact_sheet.PADDING * (rows - 1)) // rows
        self.assertEqual(tile_w, contact_sheet.TILE_WIDTH)
        self.assertEqual(tile_h, contact_sheet.TILE_WIDTH * 120 // 160)
        brightness = []
        for i in range(12):
            r, c = divmod(i, cols)
            x = contact_sheet.MARGIN + c * (tile_w + contact_sheet.PADDING)
            y = contact_sheet.MARGIN + r * (tile_h + contact_sheet.PADDING)
            # The tile's middle, away from its time label.
            brightness.append(mean(raw, width, x + tile_w // 4, y + tile_h // 3, x + 3 * tile_w // 4, y + 2 * tile_h // 3))
        for a, b in zip(brightness, brightness[1:]):
            self.assertLess(a, b, "tiles out of time order: %s" % brightness)

    def test_without_a_window_the_whole_video_is_one(self):
        report = contact_sheet.run(self.gradient, self.out())
        window = report["windows"][0]
        self.assertEqual((window["name"], window["start"]), ("video", 0))
        self.assertAlmostEqual(window["end"], 3.0, places=1)
        self.assertEqual(len(window["tiles"]), 12)

    def test_frame_count_and_columns_are_settings(self):
        report = contact_sheet.run(self.gradient, self.out(), windows=[("w", 0, 1)], frames=6, columns=3)
        self.assertEqual(len(report["windows"][0]["tiles"]), 6)
        width, height, _ = gray_pixels(report["windows"][0]["sheet"])
        self.assertEqual(width, contact_sheet.MARGIN * 2 + contact_sheet.PADDING * 2 + contact_sheet.TILE_WIDTH * 3)

    def test_variable_frame_rate_shows_the_frame_still_on_screen(self):
        report = contact_sheet.run(self.vfr, self.out(), windows=[("gap", 0.2, 0.75)], frames=12)
        tiles = report["windows"][0]["tiles"]
        on_screen = {round(t["time"], 4): round(t["frame_time"], 4) for t in tiles}
        # Every time from 0.3 to just before 0.6667 shows frame 9 (0.3 s);
        # other times show the 30 fps frame at or before them.
        for time, frame_time in on_screen.items():
            if 0.3 <= time < 0.6666:
                self.assertEqual(frame_time, 0.3, on_screen)
            else:
                self.assertEqual(frame_time, round(int(time * 30 + 1e-6) / 30, 4), on_screen)
        self.assertIn(0.3, on_screen.values())


class Fit(Fixtures):
    def test_fit_narrows_a_window_to_the_frames_that_change(self):
        report = contact_sheet.run(self.move, self.out(), windows=[("move", 0, 3)], fit=True)
        window = report["windows"][0]
        self.assertEqual(window["searched"], {"start": 0, "end": 3})
        self.assertAlmostEqual(window["start"], 1.0333 - 1 / 60, places=3)
        self.assertAlmostEqual(window["end"], 1.5, places=3)
        frames = [t["frame"] for t in window["tiles"]]
        # From the still frame before the motion to its last frame.
        self.assertEqual((frames[0], frames[-1]), (30, 45))
        self.assertEqual(frames, sorted(frames))
        self.assertGreaterEqual(len(set(frames)), 10)

    def test_fit_keeps_a_window_without_change(self):
        report = contact_sheet.run(self.move, self.out(), windows=[("still", 2, 3)], fit=True)
        window = report["windows"][0]
        self.assertEqual((window["start"], window["end"]), (2, 3))


class Detection(Fixtures):
    def test_an_abrupt_cut_is_reported_at_its_frame(self):
        report = contact_sheet.run(self.cut, self.out(), windows=[("around", 0.5, 1.5), ("before", 0, 0.9)])
        self.assertEqual([(c["frame"], c["time"]) for c in report["jump_cuts"]], [(30, 1.0)])
        around, before = report["windows"]
        self.assertEqual([c["frame"] for c in around["jump_cuts"]], [30])
        self.assertEqual(before["jump_cuts"], [])

    def test_a_fade_is_no_cut(self):
        report = contact_sheet.run(self.fade, self.out())
        self.assertEqual(report["jump_cuts"], [])

    def test_a_pause_in_the_middle_of_motion_is_a_stall(self):
        report = contact_sheet.run(self.stall, self.out(), windows=[("move", 0, 2)])
        window = report["windows"][0]
        self.assertEqual(window["jump_cuts"], [])
        self.assertEqual(len(window["stalls"]), 1, window["stalls"])
        stall = window["stalls"][0]
        self.assertAlmostEqual(stall["start"], 0.5, places=3)
        self.assertAlmostEqual(stall["end"], 1.0333, places=3)
        self.assertAlmostEqual(stall["duration"], 0.5333, places=3)

    def test_stillness_before_the_first_change_or_after_the_last_is_no_stall(self):
        report = contact_sheet.run(self.cut, self.out(), windows=[("w", 0, 2)])
        self.assertEqual(report["windows"][0]["stalls"], [])

    def test_a_gap_between_frames_with_changes_on_both_sides_is_a_stall(self):
        report = contact_sheet.run(self.vfr, self.out(), windows=[("w", 0, 1.5)])
        stalls = report["windows"][0]["stalls"]
        self.assertEqual([(round(s["start"], 4), round(s["end"], 4)) for s in stalls], [(0.3, 0.6667)])


class CommandLine(Fixtures):
    def run_cli(self, *args):
        return subprocess.run([sys.executable, SCRIPT, *args], capture_output=True, text=True)

    def test_marks_name_the_windows_and_the_report_lands_in_the_output_directory(self):
        out = self.out()
        marks = os.path.join(out, "marks.json")
        with open(marks, "w") as f:
            json.dump({"windows": [{"name": "dismiss-sheet", "start": 0.2, "end": 1.2}, {"name": "home", "start": 1.5, "end": 2.5}]}, f)
        result = self.run_cli(self.gradient, "--out", out, "--marks", marks)
        self.assertEqual(result.returncode, 0, result.stderr)
        with open(os.path.join(out, "report.json")) as f:
            report = json.load(f)
        self.assertEqual([w["name"] for w in report["windows"]], ["dismiss-sheet", "home"])
        for name in ["dismiss-sheet", "home"]:
            self.assertTrue(os.path.isfile(os.path.join(out, name + ".png")))

    def test_window_arguments_and_their_errors(self):
        out = self.out()
        ok = self.run_cli(self.gradient, "--out", out, "--window", "a=0.5-1.5", "--window", "b=2-3")
        self.assertEqual(ok.returncode, 0, ok.stderr)
        for bad in ["nameless", "x=2-1", "x=4-5", "x=a-b", "../x=0-1"]:
            with self.subTest(window=bad):
                self.assertEqual(self.run_cli(self.gradient, "--out", self.out(), "--window", bad).returncode, 2)

    def test_a_missing_video_exits_2(self):
        self.assertEqual(self.run_cli(os.path.join(self.tmp.name, "none.mp4"), "--out", self.out()).returncode, 2)


if __name__ == "__main__":
    unittest.main()
