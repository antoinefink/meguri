# SPDX-License-Identifier: AGPL-3.0-or-later
"""Media ops and observations (specs/conformance/media-ops.md F3-F12) through
real workers, and the file times through one in-process session. Expected
values come from the scenario's bytes (hashlib), its clock and the rules
media-ops.md and media-files.md state, never from the driver."""

from __future__ import annotations

import hashlib
import os
import tempfile
import unittest
from pathlib import Path

from driver import registry
from driver.clock import Clock
from driver.harness import WorkerProcess
from driver.session import Session
from driver.tests.test_worker import epoch, run_ok

START = "2030-01-15T10:00:00"


def scenario(steps, media_files=None, sid="t/media"):
    sc = {"id": sid, "clock": {"tz": "UTC", "start": START}, "steps": steps}
    if media_files is not None:
        sc["setup"] = {"media_files": media_files}
    return sc


def run(worker, steps, media_files=None):
    return worker.request({"cmd": "run", "scenario": scenario(steps, media_files)})


def sha1(data: bytes) -> str:
    return hashlib.sha1(data).hexdigest()


def entry(data: bytes) -> dict:
    return {"sha1": sha1(data), "size": len(data)}


def trashed(name: str, data: bytes) -> dict:
    return {"name": name, **entry(data)}


class MediaFiles(unittest.TestCase):
    def test_files_and_directories_land_in_the_folder_as_given(self):
        files = [
            {"name": "Upper.PNG", "text": "\u00e9"},
            {"name": "e\u0301.txt", "text": ""},
            {"name": "big.bin", "fill": {"size": 3_000_000, "byte": 7}},
            {"name": "zero.bin", "fill": {"size": 5}},
            {"name": "\u00e9" * 127 + "a", "text": "255 bytes"},
            {"name": "sub", "dir": True},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario([{"observe": ["media_dir", "media_trash"]}], files))
        self.assertEqual(out[0]["media_dir"], {
            "Upper.PNG": entry("\u00e9".encode("utf-8")),
            "e\u0301.txt": entry(b""),
            "big.bin": entry(bytes([7]) * 3_000_000),
            "zero.bin": entry(bytes(5)),
            "\u00e9" * 127 + "a": entry(b"255 bytes"),
            "sub": {"dir": True},
        })
        self.assertEqual(out[0]["media_trash"], [])

    def test_names_contents_and_variants_are_checked_before_the_steps(self):
        bad = [
            "x",
            {"name": "a", "text": "x"},
            [{"name": "a"}],
            [{"text": "x"}],
            [{"name": 1, "text": "x"}],
            [{"name": "", "text": "x"}],
            [{"name": ".", "text": "x"}],
            [{"name": "..", "dir": True}],
            [{"name": "a/b", "text": "x"}],
            [{"name": "a\x00b", "text": "x"}],
            [{"name": "\u00e9" * 128, "text": "256 bytes"}],
            [{"name": "a", "text": 1}],
            [{"name": "a", "text": "x", "fill": {"size": 1}}],
            [{"name": "a", "fill": {"size": 209_715_201}}],
            [{"name": "a", "fill": {"size": -1}}],
            [{"name": "a", "fill": {"size": True}}],
            [{"name": "a", "fill": {"size": 1.0}}],
            [{"name": "a", "fill": {"size": 1, "byte": 256}}],
            [{"name": "a", "fill": {"size": 1, "byte": -1}}],
            [{"name": "a", "fill": {"byte": 1}}],
            [{"name": "a", "fill": {"size": 1, "other": 1}}],
            [{"name": "a", "fill": 1}],
            [{"name": "a", "dir": False}],
            [{"name": "a", "dir": True, "text": "x"}],
            [{"name": "a", "remove": True}],
            # F3: names of one list differ after NFC under unicase.
            [{"name": "Cat.jpg", "text": "x"}, {"name": "cat.jpg", "text": "y"}],
            [{"name": "\u00e9", "text": "x"}, {"name": "e\u0301", "text": "y"}],
            [{"name": "thumbs.db", "text": "x"}, {"name": "thumb\u017f.db", "text": "y"}],
            [{"name": "Stra\u00dfe", "text": "x"}, {"name": "STRASSE", "dir": True}],
            [{"name": "a", "text": "x"}, {"name": "a", "text": "y"}],
        ]
        with WorkerProcess("UTC") as w:
            for files in bad:
                resp = run(w, [{"observe": ["media_dir"]}], files)
                with self.subTest(files=files):
                    self.assertFalse(resp["ok"], resp)
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)
                    self.assertNotIn("step", resp["error"], resp)


class MediaOutside(unittest.TestCase):
    def test_entries_are_created_rewritten_and_removed_in_order(self):
        files = [{"name": "a.jpg", "text": "one"}, {"name": "gone.jpg", "text": "x"}, {"name": "d", "dir": True}]
        steps = [
            {"media_outside": [
                {"name": "a.jpg", "text": "two"},
                {"name": "gone.jpg", "remove": True},
                {"name": "d", "remove": True},
                {"name": "new.jpg", "fill": {"size": 2, "byte": 1}},
                {"name": "e", "dir": True},
                {"name": "late.jpg", "text": "x"},
                {"name": "late.jpg", "remove": True},
                {"name": "LATE.JPG", "text": "y"},
            ]},
            {"observe": ["media_dir"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, files))
        self.assertEqual(out[0], {})
        self.assertEqual(out[1]["media_dir"], {
            "a.jpg": entry(b"two"),
            "new.jpg": entry(b"\x01\x01"),
            "e": {"dir": True},
            "LATE.JPG": entry(b"y"),
        })

    def test_variants_of_present_entries_and_missing_removals_fail_when_the_step_runs(self):
        bad = [
            [],
            "x",
            {"name": "x.jpg", "text": "x"},
            [{"name": "cat.jpg", "text": "x"}],
            [{"name": "CAT.JPG", "dir": True}],
            [{"name": "missing.jpg", "remove": True}],
            [{"name": "cat.jpg", "remove": True}],
            [{"name": "Cat.jpg", "remove": False}],
            [{"name": "Cat.jpg", "dir": True}],
            [{"name": "d", "text": "x"}],
            [{"name": "x.jpg", "text": "1"}, {"name": "X.jpg", "text": "2"}],
            [{"name": "a/b", "text": "x"}],
        ]
        files = [{"name": "Cat.jpg", "text": "c"}, {"name": "d", "dir": True}]
        with WorkerProcess("UTC") as w:
            for change in bad:
                resp = run(w, [{"observe": ["media_dir"]}, {"media_outside": change}], files)
                with self.subTest(change=change):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)
            same = run_ok(self, w, scenario([{"media_outside": [{"name": "Cat.jpg", "text": "new"}]},
                                             {"observe": ["media_dir"]}], files))
        self.assertEqual(same[1]["media_dir"], {"Cat.jpg": entry(b"new"), "d": {"dir": True}})


class AddMedia(unittest.TestCase):
    def test_the_output_is_the_stored_name(self):
        # media-files.md M3, AE36: same bytes reuse the file, other bytes get
        # the hash name, new names are lowercased.
        steps = [
            {"add_media": {"name": "Photo.JPG", "text": "photo bytes"}},
            {"add_media": {"name": "Photo.JPG", "text": "other"}},
            {"add_media": {"name": "Diagram.PNG", "text": "diagram"}},
            {"add_media": {"name": "big.bin", "fill": {"size": 10, "byte": 2}}},
            {"observe": ["media_dir"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, [{"name": "photo.jpg", "text": "photo bytes"}]))
        hashed = f"photo-{sha1(b'other')}.jpg"
        self.assertEqual([o["add_media"] for o in out[:4]],
                         [{"name": "photo.jpg"}, {"name": hashed}, {"name": "diagram.png"}, {"name": "big.bin"}])
        self.assertEqual(out[4]["media_dir"], {
            "photo.jpg": entry(b"photo bytes"),
            hashed: entry(b"other"),
            "diagram.png": entry(b"diagram"),
            "big.bin": entry(bytes([2]) * 10),
        })

    def test_bad_arguments_and_names_anki_refuses(self):
        bad = [
            "a.jpg",
            {"text": "x"},
            {"name": 1, "text": "x"},
            {"name": "a"},
            {"name": "a", "text": "x", "fill": {"size": 1}},
            {"name": "a", "text": None},
            {"name": "a", "fill": {"size": 209_715_201}},
            {"name": "a", "text": "x", "extra": 1},
        ]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, [{"add_media": args}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)
            # F5: the Meguri-only rejections are never recorded; Anki fails.
            empty = run(w, [{"add_media": {"name": "", "text": "x"}}])
        self.assertEqual((empty["error"]["kind"], empty["error"]["op"], empty["error"]["step"]),
                         ("anki_error", "add_media", 0), empty)


class Trash(unittest.TestCase):
    def test_trash_restore_and_empty(self):
        steps = [
            {"observe": ["media_trash"]},
            {"add_media": {"name": "a.jpg", "text": "aaa"}},
            {"add_media": {"name": "b.jpg", "text": "bbb"}},
            {"trash_media": {"names": ["a.jpg"]}},
            {"observe": ["media_dir", "media_trash"]},
            {"restore_media_trash": {}},
            {"observe": ["media_dir", "media_trash"]},
            {"trash_media": {"names": ["b.jpg", "a.jpg"]}},
            {"observe": ["media_dir", "media_trash"]},
            {"empty_media_trash": {}},
            {"observe": ["media_dir", "media_trash"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        self.assertEqual(out[0]["media_trash"], [])
        self.assertEqual([out[i] for i in (3, 5, 7, 9)], [{}, {}, {}, {}])
        self.assertEqual(out[4], {"media_dir": {"b.jpg": entry(b"bbb")}, "media_trash": [trashed("a.jpg", b"aaa")]})
        self.assertEqual(out[6], {"media_dir": {"a.jpg": entry(b"aaa"), "b.jpg": entry(b"bbb")}, "media_trash": []})
        self.assertEqual(out[8], {"media_dir": {},
                                  "media_trash": [trashed("a.jpg", b"aaa"), trashed("b.jpg", b"bbb")]})
        self.assertEqual(out[10], {"media_dir": {}, "media_trash": []})

    def test_trash_entries_sort_by_utf8_name(self):
        names = ["\u00e9.jpg", "b.jpg", "C.jpg", "a.jpg", "\uff21.jpg"]
        files = [{"name": n, "text": n} for n in names]
        steps = [{"observe": ["media_pending"]}, {"trash_media": {"names": names}}, {"observe": ["media_trash"]}]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, files))
        expected = sorted(names, key=lambda n: n.encode("utf-8"))
        self.assertEqual(expected, ["C.jpg", "a.jpg", "b.jpg", "\u00e9.jpg", "\uff21.jpg"])
        self.assertEqual(out[2]["media_trash"], [trashed(n, n.encode("utf-8")) for n in expected])

    def test_bad_arguments(self):
        files = [{"name": "a.jpg", "text": "a"}, {"name": "d", "dir": True}]
        bad = [
            {"trash_media": {"names": []}},
            {"trash_media": {"names": "a.jpg"}},
            {"trash_media": {"names": [1]}},
            {"trash_media": {"names": [""]}},
            {"trash_media": {}},
            {"trash_media": ["a.jpg"]},
            {"trash_media": {"names": ["a.jpg"], "other": 1}},
            # F6: only names of files present under exactly that name, once.
            {"trash_media": {"names": ["missing.jpg"]}},
            {"trash_media": {"names": ["A.JPG"]}},
            {"trash_media": {"names": ["a.jpg", "a.jpg"]}},
            {"trash_media": {"names": ["d"]}},
            {"restore_media_trash": {"x": 1}},
            {"restore_media_trash": None},
            {"restore_media_trash": []},
            {"empty_media_trash": {"x": 1}},
            {"empty_media_trash": None},
        ]
        with WorkerProcess("UTC") as w:
            for step in bad:
                resp = run(w, [{"observe": ["media_pending"]}, step], files)
                with self.subTest(step=step):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)


class Pending(unittest.TestCase):
    def test_a_full_registration_whatever_the_folder_marker_says(self):
        files = [
            {"name": "ok.png", "text": "kept"},
            {"name": "Thumbs.db", "text": "ignored"},
            {"name": "empty.png", "text": ""},
            {"name": "sub", "dir": True},
        ]
        steps = [
            {"observe": ["media_pending"]},
            {"observe": ["media_pending"]},
            # Same instant: the folder keeps the time the last registration
            # recorded, so only a full comparison finds the new file.
            {"media_outside": [{"name": "new.png", "text": "new"}]},
            {"observe": ["media_pending"]},
            {"media_outside": [{"name": "ok.png", "remove": True}]},
            {"add_media": {"name": "x.png", "text": "x"}},
            {"observe": ["media_pending"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, files))
        # media-files.md M7.1, M8: ignored names, empty files and directories
        # never register.
        first = {"ok.png": sha1(b"kept")}
        self.assertEqual(out[0]["media_pending"], first)
        self.assertEqual(out[1]["media_pending"], first)
        self.assertEqual(out[3]["media_pending"], {**first, "new.png": sha1(b"new")})
        self.assertEqual(out[6]["media_pending"], {"ok.png": None, "new.png": sha1(b"new"), "x.png": sha1(b"x")})

    def test_a_rewrite_is_seen_only_once_the_clock_moved(self):
        # F12: outside writes carry the clock's instant, and registration
        # tells a change by whole-second modification time and size.
        steps = [
            {"observe": ["media_pending"]},
            {"media_outside": [{"name": "f.jpg", "text": "f two"}]},
            {"observe": ["media_pending"]},
            {"advance": "1s"},
            {"media_outside": [{"name": "f.jpg", "text": "f 3rd"}]},
            {"observe": ["media_pending"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, [{"name": "f.jpg", "text": "f one"}]))
        self.assertEqual([out[i]["media_pending"] for i in (0, 2, 5)],
                         [{"f.jpg": sha1(b"f one")}, {"f.jpg": sha1(b"f one")}, {"f.jpg": sha1(b"f 3rd")}])

    def test_observations_take_no_arguments(self):
        with WorkerProcess("UTC") as w:
            for spec in ("media_dir(x)", {"media_trash": {}}, {"media_pending": []}, {"media_dir": "x"}):
                resp = run(w, [{"observe": [spec]}])
                with self.subTest(spec=spec):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)
            ok = run_ok(self, w, scenario([{"observe": [{"media_dir": None}, "media_trash", "media_pending"]}]))
        self.assertEqual(ok[0], {"media_dir": {}, "media_trash": [], "media_pending": {}})


class FileTimes(unittest.TestCase):
    """F3, F4, F12 through one in-process session: modification times are not
    observable through the protocol."""

    def setUp(self):
        registry.load_all()
        self.tmp = tempfile.TemporaryDirectory()
        files = [{"name": "a.jpg", "text": "a"}, {"name": "d", "dir": True}]
        self.s = Session(scenario([], files), "UTC", self.tmp.name, Clock(str(Path(self.tmp.name) / "clock")))
        self.folder = Path(self.s.col.media.dir())

    def tearDown(self):
        self.s.close()
        self.tmp.cleanup()

    def mtime(self, path: Path) -> int:
        return os.stat(path).st_mtime_ns

    def test_written_files_and_the_folder_carry_the_clock_instant(self):
        t = epoch(START, "UTC") * 10**9
        self.assertEqual([self.mtime(self.folder / "a.jpg"), self.mtime(self.folder)], [t, t])
        self.s.run_step(0, {"advance": "5s"})
        self.s.run_step(1, {"media_outside": [{"name": "b.jpg", "text": "b"}, {"name": "d", "remove": True}]})
        later = t + 5 * 10**9
        self.assertEqual([self.mtime(self.folder / "a.jpg"), self.mtime(self.folder / "b.jpg"), self.mtime(self.folder)],
                         [t, later, later])


if __name__ == "__main__":
    unittest.main()
