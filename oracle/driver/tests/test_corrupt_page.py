# SPDX-License-Identifier: AGPL-3.0-or-later
"""The corrupt_page op (specs/conformance/maintenance-ops.md M2).

The page arithmetic is checked on plain SQLite files against SQLite's own
rules: the page size is the big-endian 16-bit value at header offset 16, where
1 stands for 65,536, and page n starts at (n - 1) * page size. A WAL-mode file
whose rows are still in its log only reaches the main file through a
checkpoint, so a damage that skipped it would leave the rows readable. Through
real workers, Anki's Check Database must then find the file corrupt, twice."""

from __future__ import annotations

import os
import sqlite3
import tempfile
import unittest

from driver.errors import DriverError
from driver.harness import WorkerProcess
from driver.ops.maintenance import SCHEMA_TABLES, check_corrupt_page_placement, damage_root_page
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
CORRUPT = {"check_db": {"error": "corrupt"}}


def _file(page_size: int, wal: bool) -> tuple[str, sqlite3.Connection]:
    directory = tempfile.mkdtemp(prefix="corrupt-")
    path = os.path.join(directory, "collection.anki2")
    db = sqlite3.connect(path)
    db.execute(f"PRAGMA page_size = {page_size}")
    if wal:
        db.execute("PRAGMA journal_mode = wal").fetchall()
        db.execute("PRAGMA wal_autocheckpoint = 0").fetchall()
    db.execute("CREATE TABLE keep (v)")
    db.execute("CREATE TABLE revlog (id INTEGER PRIMARY KEY, v)")
    db.executemany("INSERT INTO revlog (v) VALUES (?)", [("x" * 50,)] * 20)
    db.execute("INSERT INTO keep VALUES ('kept')")
    db.commit()
    return path, db


class DamageRootPage(unittest.TestCase):
    def _check(self, page_size: int, wal: bool) -> None:
        path, db = _file(page_size, wal)
        root = db.execute("SELECT rootpage FROM sqlite_schema WHERE name = 'revlog'").fetchone()[0]
        if wal:
            # With no autocheckpoint the rows are only in the log. Copy both
            # files, close (which folds the log in), then put the copies back:
            # the main file lacks the rows again, as after an unclean close.
            log = open(path + "-wal", "rb").read()
            self.assertGreater(len(log), 0)
            snapshot = open(path, "rb").read()
        db.close()
        if wal:
            with open(path, "wb") as f:
                f.write(snapshot)
            with open(path + "-wal", "wb") as f:
                f.write(log)
        self.assertEqual(damage_root_page(path, "revlog"), root)
        self.assertFalse(os.path.exists(path + "-wal") and os.path.getsize(path + "-wal") > 0)
        data = open(path, "rb").read()
        header = int.from_bytes(data[16:18], "big")
        self.assertEqual(header, 1 if page_size == 65536 else page_size)
        start = (root - 1) * page_size
        self.assertEqual(data[start:start + page_size], b"\xff" * page_size)
        db = sqlite3.connect(path)
        try:
            self.assertEqual(db.execute("SELECT v FROM keep").fetchall(), [("kept",)])
            with self.assertRaises(sqlite3.DatabaseError):
                db.execute("SELECT count(*) FROM revlog").fetchall()
            try:
                verdict = db.execute("PRAGMA integrity_check").fetchall()
            except sqlite3.DatabaseError:
                verdict = None
            self.assertNotEqual(verdict, [("ok",)])
        finally:
            db.close()

    def test_rollback_journal_file(self):
        self._check(4096, wal=False)

    def test_wal_file_is_folded_in_first(self):
        self._check(4096, wal=True)

    def test_page_size_65536_is_read_from_the_value_1(self):
        self._check(65536, wal=False)

    def test_a_table_the_file_lacks_is_a_driver_error(self):
        path, db = _file(4096, wal=False)
        db.close()
        before = open(path, "rb").read()
        with self.assertRaises(DriverError) as cm:
            damage_root_page(path, "notes")
        self.assertEqual(cm.exception.kind, "internal_error")
        self.assertEqual(open(path, "rb").read(), before)


class Placement(unittest.TestCase):
    def test_actor_scenarios_and_later_handoffs_are_refused(self):
        bad = [
            ({"actors": ["a", "b"], "steps": [{"corrupt_page": {"table": "revlog"}}]}, 0),
            ({"steps": [{"advance": "1m"}, {"corrupt_page": {"table": "revlog"}},
                        {"check_db": {}}, {"handoff": "meguri"}]}, 3),
        ]
        for sc, step in bad:
            with self.subTest(sc=sc):
                with self.assertRaises(DriverError) as cm:
                    check_corrupt_page_placement(sc)
                self.assertEqual(cm.exception.kind, "invalid_scenario")
                self.assertEqual(cm.exception.fields.get("step"), step)

    def test_a_handoff_before_it_is_allowed(self):
        check_corrupt_page_placement({"steps": [{"handoff": "oracle"}, {"corrupt_page": {"table": "revlog"}}]})
        check_corrupt_page_placement({"actors": ["a", "b"], "steps": [{"handoff": "oracle"}]})


class CorruptPageOp(unittest.TestCase):
    def test_check_db_finds_the_file_corrupt_twice(self):
        steps = [
            {"answer": {"card": "#c1", "rating": "good"}},
            {"check_db": {}},
            {"corrupt_page": {"table": "revlog"}},
            {"advance": "1h"},
            {"check_db": {}},
            {"check_db": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/corrupt-page", "UTC", START, steps, notes=2))
        self.assertEqual(set(out[1]["check_db"].values()), {0})
        self.assertEqual(out[2], {})
        self.assertEqual(out[4], CORRUPT)
        self.assertEqual(out[5], CORRUPT)

    def test_every_schema_table_name_is_accepted(self):
        # Only the argument check: whether Anki opens the file again depends
        # on the table (M2.3 damages revlog), so an open failure is allowed
        # here, a refused argument is not.
        self.assertEqual(len(set(SCHEMA_TABLES)), 12)
        with WorkerProcess("UTC") as w:
            for table in SCHEMA_TABLES:
                with self.subTest(table=table):
                    resp = w.request({"cmd": "run", "scenario": scenario(
                        "t/corrupt-page", "UTC", START, [{"corrupt_page": {"table": table}}], notes=1)})
                    if not resp["ok"]:
                        self.assertNotEqual(resp["error"]["kind"], "invalid_scenario", resp)

    def test_arguments_outside_m2_1_are_invalid(self):
        with WorkerProcess("UTC") as w:
            for args in ({}, {"table": "nope"}, {"table": "Revlog"}, {"table": "sqlite_schema"},
                         {"table": "revlog", "page": 2}, {"table": 1}, {"table": ["revlog"]},
                         None, [], "revlog"):
                with self.subTest(args=args):
                    resp = w.request({"cmd": "run", "scenario": scenario(
                        "t/corrupt-page", "UTC", START, [{"corrupt_page": args}])})
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)

    def test_a_handoff_after_it_is_refused_before_any_step(self):
        steps = [{"corrupt_page": {"table": "revlog"}}, {"check_db": {}}, {"handoff": "meguri"}]
        with WorkerProcess("UTC") as w:
            resp = w.request({"cmd": "run", "scenario": scenario("t/corrupt-page", "UTC", START, steps)})
        self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)
        self.assertEqual(resp["error"].get("step"), 2, resp)


if __name__ == "__main__":
    unittest.main()
