# SPDX-License-Identifier: AGPL-3.0-or-later
"""Backups (specs/conformance/backup-ops.md F1-F7) through real workers: the
backups folder and its handoff, add_backup_files, force_backup with its
retention and copy_as, the one-backup-per-opening rule, save_backup_settings
and the backup_files and backup_settings observations. Expected names,
remaining files, texts and limits come from the specs (package/backups.md
B2.1, B3, B5.3, B6; container.md P11) and the scenario clock, never from the
driver."""

from __future__ import annotations

import glob
import os
import shutil
import tempfile
import unittest
import zipfile

from anki.collection import Collection

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch

START = "2030-01-15T10:00:00"
NEW_UTC = "backup-2030-01-15-10.00.00.colpkg"
OPEN = {"open": {"fixture": "storage/v18.anki2"}}
LATEST_ENTRIES = ["meta", "collection.anki21b", "collection.anki2", "media"]
DEFAULTS = {"daily": 12, "weekly": 10, "monthly": 9, "minimum_interval_mins": 30}
FILES = {"observe": ["backup_files"]}
SETTINGS = {"observe": ["backup_settings"]}


def sc(steps, tz="UTC", start=START, **extra):
    return {"id": "t/backups", "clock": {"tz": tz, "start": start}, "steps": steps, **extra}


def add(*names):
    return {"add_backup_files": {"names": list(names)}}


def series(start, until, every):
    return {"add_backup_files": {"series": {"from": start, "until": until, "every": every}}}


def config_text(where="key = 'backups'", columns="CAST(val AS TEXT)"):
    return {"db_exec": {"sql": f"SELECT {columns} FROM config WHERE {where} ORDER BY key"}}


class Backups(unittest.TestCase):
    tz = "UTC"

    def setUp(self):
        # The TAP runner runs each test on its own, so no class fixtures.
        scratch = os.environ.get("ORACLE_SCRATCH_ROOT")
        self.handoff = tempfile.mkdtemp(prefix="handoff-", dir=scratch)
        self.fixtures = tempfile.mkdtemp(prefix="fixtures-", dir=scratch)
        self.worker = WorkerProcess(
            self.tz, extra_env={"ORACLE_HANDOFF": self.handoff, "ORACLE_FIXTURES": self.fixtures}
        )

    def tearDown(self):
        self.worker.close()
        shutil.rmtree(self.handoff, ignore_errors=True)
        shutil.rmtree(self.fixtures, ignore_errors=True)

    def run_steps(self, steps, **extra):
        return self.worker.request({"cmd": "run", "scenario": sc(steps, tz=self.tz, **extra)})

    def ok(self, steps, **extra):
        resp = self.run_steps(steps, **extra)
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"]

    def error(self, steps, **extra):
        resp = self.run_steps(steps, **extra)
        self.assertFalse(resp["ok"], resp)
        return resp["error"]

    def handed(self, steps, rel="out", **extra):
        """Run steps, then hand the session directory over to rel; its path
        and the outputs."""
        resp = self.worker.request({
            "cmd": "run", "scenario": sc(steps + [{"handoff": "meguri"}], tz=self.tz, **extra),
            "steps": [0, len(steps)], "handoff": rel,
        })
        self.assertTrue(resp["ok"], resp)
        return os.path.join(self.handoff, rel), resp["observations"]["steps"]

    def make_collection_fixture(self):
        """storage/v18.anki2: a schema-18 collection with one note."""
        work = tempfile.mkdtemp(dir=self.fixtures)
        col = Collection(os.path.join(work, "collection.anki2"))
        note = col.new_note(col.models.by_name("Basic"))
        note["Front"], note["Back"] = "q", "a"
        col.add_note(note, col.decks.id("Default"))
        col.close(downgrade=False)
        os.makedirs(os.path.join(self.fixtures, "storage"))
        shutil.copyfile(os.path.join(work, "collection.anki2"), os.path.join(self.fixtures, "storage", "v18.anki2"))
        shutil.rmtree(work)


class Files(Backups):
    tz = "America/New_York"

    def test_names_become_empty_regular_files_listed_by_utf8_bytes(self):
        names = ["日本", "b.txt", "~", "😀", "a b", "ö", "B", "ｚ", "z", "é", "." * 3, "-x", ".meguri"]
        out, steps = self.handed([FILES, add(*names), FILES])
        self.assertEqual(steps[0], {"backup_files": []})
        self.assertEqual(steps[1], {})
        # F5: UTF-8 byte order ("ｚ" is EF BD 9A, "😀" F0 9F 98 80).
        self.assertEqual(steps[2]["backup_files"], [
            "-x", "...", ".meguri", "B", "a b", "b.txt", "z", "~", "é", "ö", "日本", "ｚ", "😀",
        ])
        folder = os.path.join(out, "backups")
        self.assertEqual(sorted(os.listdir(folder)), sorted(names))
        for name in names:
            path = os.path.join(folder, name)
            with self.subTest(name=name):
                self.assertTrue(os.path.isfile(path) and not os.path.islink(path))
                self.assertEqual(os.path.getsize(path), 0)

    def test_series_step_plain_calendar_time(self):
        # F2: from, from + every, ... up to and including until, without any
        # zone rule: 02:30 on 2029-03-11 is in New York's skipped hour.
        steps = self.ok([
            series("2029-03-11T00:30:00", "2029-03-11T03:30:00", "1h"),
            series("2029-12-31T22:00:00", "2030-01-01T01:00:00", "1h"),
            series("2028-02-28T12:00:00", "2028-03-01T12:00:00", "1d"),
            series("2030-01-02T00:00:00", "2030-01-02T00:00:10", "3s"),
            series("2030-01-03T00:00:00", "2030-01-03T00:00:00", "1d"),
            series("2030-01-04T00:00:00", "2030-01-05T01:00:01", "1d1h"),
            FILES,
        ])
        self.assertEqual(steps[:6], [{}] * 6)
        self.assertEqual(steps[6]["backup_files"], [
            "backup-2028-02-28-12.00.00.colpkg", "backup-2028-02-29-12.00.00.colpkg",
            "backup-2028-03-01-12.00.00.colpkg",
            "backup-2029-03-11-00.30.00.colpkg", "backup-2029-03-11-01.30.00.colpkg",
            "backup-2029-03-11-02.30.00.colpkg", "backup-2029-03-11-03.30.00.colpkg",
            "backup-2029-12-31-22.00.00.colpkg", "backup-2029-12-31-23.00.00.colpkg",
            "backup-2030-01-01-00.00.00.colpkg", "backup-2030-01-01-01.00.00.colpkg",
            "backup-2030-01-02-00.00.00.colpkg", "backup-2030-01-02-00.00.03.colpkg",
            "backup-2030-01-02-00.00.06.colpkg", "backup-2030-01-02-00.00.09.colpkg",
            "backup-2030-01-03-00.00.00.colpkg",
            "backup-2030-01-04-00.00.00.colpkg", "backup-2030-01-05-01.00.00.colpkg",
        ])

    def test_twenty_thousand_files_in_one_step(self):
        steps = self.ok([series("2030-01-01T00:00:00", "2030-01-01T05:33:19", "1s"), FILES])
        listed = steps[1]["backup_files"]
        self.assertEqual(len(listed), 20000)
        self.assertEqual((listed[0], listed[-1]),
                         ("backup-2030-01-01-00.00.00.colpkg", "backup-2030-01-01-05.33.19.colpkg"))

    def test_malformed_steps_are_invalid_before_any_collection(self):
        good = {"from": "2030-01-01T00:00:00", "until": "2030-01-01T01:00:00", "every": "1h"}
        bad = [
            None, {}, [], {"names": []}, {"names": "x"}, {"names": ["a", "a"]}, {"names": [""]}, {"names": ["."]},
            {"names": [".."]}, {"names": ["a/b"]}, {"names": ["/"]}, {"names": ["a\x00b"]},
            {"names": [".meguri-tmp-1"]}, {"names": [".meguri-"]}, {"names": ["é" * 128]}, {"names": ["a" * 256]},
            {"names": [1]}, {"names": [None]}, {"names": ["a"], "series": good},
            {"other": ["a"]}, {"names": [f"f{i}" for i in range(20001)]},
            {"series": {**good, "every": "0s"}}, {"series": {**good, "every": "1x"}}, {"series": {**good, "every": 60}},
            {"series": {k: v for k, v in good.items() if k != "every"}}, {"series": {**good, "extra": 1}},
            {"series": {**good, "until": "2029-12-31T23:59:59"}}, {"series": {**good, "from": "2030-01-01T00:00:00Z"}},
            {"series": {**good, "from": "2030-02-30T00:00:00"}}, {"series": {**good, "from": "2030-1-01T00:00:00"}},
            {"series": {**good, "from": "2030-01-01 00:00:00"}}, {"series": {**good, "from": "2030-01-01T24:00:00"}},
            {"series": {**good, "until": "2030-01-01T05:33:20", "every": "1s"}}, {"series": "x"},
        ]
        for args in bad:
            err = self.error([{"advance": "1m"}, {"add_backup_files": args}])
            with self.subTest(args=str(args)[:80]):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)
        # Names at the limits.
        edge = ["é" * 127 + "a", "a" * 255, "x.meguri-", "..x", " "]
        steps = self.ok([add(*edge), FILES])
        self.assertEqual(steps[1]["backup_files"], [" ", "..x", "a" * 255, "x.meguri-", "é" * 127 + "a"])

    def test_a_name_already_in_the_folder_is_invalid_at_its_step(self):
        begin = self.worker.request({"cmd": "begin", "scenario": sc([add("a", "b"), add("c", "b")], tz=self.tz)})
        self.assertTrue(begin["ok"], begin)
        try:
            self.assertTrue(self.worker.request({"cmd": "step"})["ok"])
            resp = self.worker.request({"cmd": "step"})
            self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)
            # Nothing of the refused step was created.
            listing = self.worker.request({"cmd": "step", "step": FILES})
            self.assertEqual(listing["observation"], {"backup_files": ["a", "b"]}, listing)
        finally:
            self.worker.request({"cmd": "end"})

    def test_the_folder_crosses_a_handoff(self):
        steps = [add("x"), {"handoff": "oracle"}, FILES]
        first = self.worker.request({"cmd": "run", "scenario": sc(steps, tz=self.tz), "steps": [0, 1], "handoff": "a"})
        self.assertTrue(first["ok"], first)
        folder = os.path.join(self.handoff, "a", "backups")
        self.assertEqual(os.listdir(folder), ["x"])
        # What the other side left there: a file and a directory.
        os.mkdir(os.path.join(folder, "backup-2029-06-01-12.00.00.colpkg"))
        with open(os.path.join(folder, "y"), "wb") as f:
            f.write(b"y")
        second = self.worker.request({"cmd": "run", "scenario": sc(steps, tz=self.tz), "steps": [2, 3],
                                      "resume": first["state"]})
        self.assertTrue(second["ok"], second)
        self.assertEqual(second["observations"]["steps"], [{"backup_files": ["x", "y"]}])

    def test_only_regular_files_are_listed(self):
        begin = self.worker.request({"cmd": "begin", "scenario": sc([add("x")], tz=self.tz)})
        self.assertTrue(begin["ok"], begin)
        try:
            self.assertTrue(self.worker.request({"cmd": "step"})["ok"])
            # The session directory is in the worker's scratch (one session).
            (folder,) = glob.glob(os.path.join(self.worker.scratch, "col-*", "backups"))
            os.mkdir(os.path.join(folder, "backup-2029-06-01-12.00.00.colpkg"))
            os.symlink("x", os.path.join(folder, "backup-2029-06-02-12.00.00.colpkg"))
            os.symlink("missing", os.path.join(folder, "dangling"))
            os.mkfifo(os.path.join(folder, "fifo"))
            listing = self.worker.request({"cmd": "step", "step": FILES})
            self.assertEqual(listing["observation"], {"backup_files": ["x"]}, listing)
        finally:
            self.worker.request({"cmd": "end"})

    def test_observations_take_no_argument(self):
        for spec in ({"backup_files": {}}, "backup_files(#c1)", {"backup_settings": {}}, {"backup_settings": 1}):
            resp = self.run_steps([{"observe": [spec]}])
            with self.subTest(spec=spec):
                self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)
        steps = self.ok([{"observe": ["backup_files()", {"backup_settings": None}]}])
        self.assertEqual(steps, [{"backup_files": [], "backup_settings": DEFAULTS}])


class Force(Backups):
    def test_the_backup_is_named_after_the_clock_and_listed(self):
        out, steps = self.handed([{"force_backup": {}}, FILES])
        self.assertEqual(steps, [{"force_backup": {"name": NEW_UTC}}, {"backup_files": [NEW_UTC]}])
        # container.md P11: Anki's collection-only package layout.
        with zipfile.ZipFile(os.path.join(out, "backups", NEW_UTC)) as z:
            self.assertEqual([i.filename for i in z.infolist()], LATEST_ENTRIES)
            self.assertEqual(z.read("meta"), b"\x08\x03")

    def test_copy_as_copies_the_backup_into_the_session_directory(self):
        steps = [{"export": {"format": "colpkg", "name": "x.colpkg"}}, {"advance": "1s"},
                 {"force_backup": {"copy_as": "x.colpkg"}}]
        out, outputs = self.handed(steps)
        name = "backup-2030-01-15-10.00.01.colpkg"
        self.assertEqual(outputs[2], {"force_backup": {"name": name}})
        with open(os.path.join(out, "x.colpkg"), "rb") as copy, open(os.path.join(out, "backups", name), "rb") as src:
            self.assertEqual(copy.read(), src.read())
        self.assertEqual(sorted(os.listdir(os.path.join(out, "backups"))), [name])

    def test_retention_runs_within_the_step(self):
        # package/backups.md B5.3: limits 1, 1, 1 on Tuesday 2030-01-15.
        steps = self.ok([
            add("backup-2030-01-14-09.00.00.colpkg", "backup-2030-01-12-20.00.00.colpkg",
                "backup-2030-01-12-08.00.00.colpkg", "backup-2030-01-08-12.00.00.colpkg",
                "backup-2030-01-07-12.00.00.colpkg", "backup-2030-01-06-12.00.00.colpkg",
                "backup-2030-01-02-12.00.00.colpkg", "backup-2029-12-20-12.00.00.colpkg",
                "backup-2029-12-02-12.00.00.colpkg", "backup-2029-06-01-12.00.00.colpkg", "notes.txt"),
            {"force_backup": {}},
            FILES,
        ], setup={"config": {"backups": {"daily": 1, "weekly": 1, "monthly": 1, "minimum_interval_mins": 30}}})
        self.assertEqual(steps[1], {"force_backup": {"name": NEW_UTC}})
        self.assertEqual(steps[2]["backup_files"], [
            "backup-2029-12-20-12.00.00.colpkg", "backup-2030-01-06-12.00.00.colpkg",
            "backup-2030-01-12-20.00.00.colpkg", "backup-2030-01-14-09.00.00.colpkg", NEW_UTC, "notes.txt",
        ])

    def test_one_backup_per_opening(self):
        force = {"force_backup": {}}
        for steps in (
            [force, {"advance": "1s"}, force],
            [force, {"advance": "1s"}, {"export": {"format": "colpkg", "name": "x.colpkg"}}, force],
            [force, {"reopen": {}}, {"advance": "1s"}, force, {"advance": "1s"}, force],
        ):
            err = self.error(steps)
            with self.subTest(steps=steps):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", len(steps) - 1), err)
        for opening in ({"reopen": {}}, {"handoff": "oracle"}):
            steps = self.ok([force, opening, {"advance": "1s"}, force, FILES])
            with self.subTest(opening=opening):
                self.assertEqual(steps[3], {"force_backup": {"name": "backup-2030-01-15-10.00.01.colpkg"}})
                self.assertEqual(steps[4]["backup_files"], [NEW_UTC, "backup-2030-01-15-10.00.01.colpkg"])

    def test_import_resets_the_forced_backup_guard(self):
        self.make_collection_fixture()
        notes = {"db_exec": {"sql": "SELECT count() FROM notes"}}
        steps = self.ok([
            OPEN, {"force_backup": {"copy_as": "b.colpkg"}}, {"db_exec": {"sql": "DELETE FROM notes"}}, notes,
            {"advance": "1m"}, {"import": {"format": "colpkg", "name": "b.colpkg"}}, notes,
            {"force_backup": {}}, FILES,
        ])
        self.assertEqual(steps[1], {"force_backup": {"name": NEW_UTC}})
        self.assertEqual((steps[3], steps[5], steps[6]), (notes_rows(0), {}, notes_rows(1)))
        self.assertEqual(steps[7], {"force_backup": {"name": "backup-2030-01-15-10.01.00.colpkg"}})
        self.assertEqual(steps[8]["backup_files"], [NEW_UTC, "backup-2030-01-15-10.01.00.colpkg"])

    def test_a_backup_anki_skips_is_anki_error(self):
        # F3: nothing changed since this opening's backup, even after the
        # clock moved.
        begin = self.worker.request({"cmd": "begin", "scenario": sc([{"force_backup": {}}])})
        self.assertTrue(begin["ok"], begin)
        try:
            self.assertTrue(self.worker.request({"cmd": "step"})["ok"])
            self.assertTrue(self.worker.request({"cmd": "step", "step": {"advance": "1h"}})["ok"])
            resp = self.worker.request({"cmd": "step", "step": {"force_backup": {}}})
            self.assertEqual((resp["error"]["kind"], resp["error"].get("op")), ("anki_error", "force_backup"), resp)
            listing = self.worker.request({"cmd": "step", "step": FILES})
            self.assertEqual(listing["observation"], {"backup_files": [NEW_UTC]}, listing)
        finally:
            self.worker.request({"cmd": "end"})

    def test_a_seeded_file_with_the_backups_name_is_invalid(self):
        err = self.error([add(NEW_UTC), {"force_backup": {}}])
        self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)

    def test_malformed_arguments(self):
        for args in (None, [], {"x": 1}, {"copy_as": "x.apkg"}, {"copy_as": "a/b.colpkg"}, {"copy_as": ""},
                     {"copy_as": None}, {"copy_as": "x.colpkg", "x": 1}):
            err = self.error([{"advance": "1m"}, {"force_backup": args}])
            with self.subTest(args=args):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)


def notes_rows(n):
    return {"db_exec": {"rows": [[n]]}}


class Zones(Backups):
    """B3: the name is the wall time of the instant in the worker's zone."""

    def name_at(self, tz, start):
        with WorkerProcess(tz) as w:
            resp = w.request({"cmd": "run", "scenario": sc([{"force_backup": {}}], tz=tz, start=start)})
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"][0]["force_backup"]["name"]

    def test_offsets(self):
        self.assertEqual(self.name_at("Asia/Tokyo", "2030-01-15T20:00:00Z"), "backup-2030-01-16-05.00.00.colpkg")
        self.assertEqual(self.name_at("America/New_York", "2030-01-15T20:00:00Z"), "backup-2030-01-15-15.00.00.colpkg")


class Settings(Backups):
    def test_defaults_then_the_saved_text(self):
        limits = {"daily": 3, "weekly": 2, "monthly": 1, "minimum_interval_mins": 45}
        zeros = dict.fromkeys(limits, 0)
        top = dict.fromkeys(limits, 4294967295)
        query = config_text(columns="CAST(val AS TEXT), typeof(val), usn, mtime_secs")
        steps = self.ok([
            SETTINGS, {"save_backup_settings": limits}, query, SETTINGS,
            {"advance": "1m"}, {"save_backup_settings": zeros}, query, SETTINGS,
            {"advance": "1m"}, {"save_backup_settings": top}, query, SETTINGS,
        ])
        now = epoch(START, "UTC")
        self.assertEqual(steps[0], {"backup_settings": DEFAULTS})
        self.assertEqual(steps[1], {})
        # package/backups.md B6.2: Anki's text, a BLOB, usn -1, mtime now.
        self.assertEqual(steps[2], {"db_exec": {"rows": [
            ['{"daily":3,"weekly":2,"monthly":1,"minimum_interval_mins":45}', "blob", -1, now]]}})
        self.assertEqual(steps[3], {"backup_settings": limits})
        # All four zero still writes the group.
        self.assertEqual(steps[6], {"db_exec": {"rows": [
            ['{"daily":0,"weekly":0,"monthly":0,"minimum_interval_mins":0}', "blob", -1, now + 60]]}})
        self.assertEqual(steps[7], {"backup_settings": zeros})
        self.assertEqual(steps[10], {"db_exec": {"rows": [[
            '{"daily":4294967295,"weekly":4294967295,"monthly":4294967295,"minimum_interval_mins":4294967295}',
            "blob", -1, now + 120]]}})
        self.assertEqual(steps[11], {"backup_settings": top})

    def test_only_the_backups_key_changes(self):
        others = config_text(where="key != 'backups'", columns="key, val, usn, mtime_secs")
        steps = self.ok([others, {"advance": "1h"},
                         {"save_backup_settings": {"daily": 1, "weekly": 1, "monthly": 1, "minimum_interval_mins": 5}},
                         others])
        self.assertTrue(steps[0]["db_exec"]["rows"])
        self.assertEqual(steps[3], steps[0])

    def test_malformed_arguments(self):
        good = {"daily": 3, "weekly": 2, "monthly": 1, "minimum_interval_mins": 45}
        bad = [None, [], {}, {k: v for k, v in good.items() if k != "weekly"}, {**good, "extra": 1}]
        bad += [{**good, "daily": v} for v in (-1, 4294967296, True, 1.0, "3", None)]
        for args in bad:
            err = self.error([{"advance": "1m"}, {"save_backup_settings": args}])
            with self.subTest(args=args):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)


if __name__ == "__main__":
    unittest.main()
