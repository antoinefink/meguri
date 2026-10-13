# SPDX-License-Identifier: AGPL-3.0-or-later
"""open, schema_objects and the storage reads of db_exec and db_dump
(specs/conformance/storage-ops.md P1-P3) through real workers. The fixtures
are made here with Anki and sqlite3; the expected DDL text is schema-18.md S2
and S3 as written there, not what the driver reads."""

from __future__ import annotations

import os
import shutil
import sqlite3
import tempfile
import unittest

from anki.collection import Collection

from driver.harness import WorkerProcess
from driver.observe.schema import normalize

START = "2030-01-15T10:00:00"
DECKS_SQL = (
    "CREATE TABLE decks ( id integer PRIMARY KEY NOT NULL, name text NOT NULL COLLATE unicase, "
    "mtime_secs integer NOT NULL, usn integer NOT NULL, common blob NOT NULL, kind blob NOT NULL )"
)
GRAVES_SQL = (
    "CREATE TABLE graves ( oid integer NOT NULL, type integer NOT NULL, usn integer NOT NULL, "
    "PRIMARY KEY (oid, type) ) WITHOUT ROWID"
)
ODID_SQL = "CREATE INDEX idx_cards_odid ON cards (odid) WHERE odid != 0"


def make_fixtures(root: str) -> None:
    """storage/v18.anki2 (WAL), v11.anki2 (rollback journal) and refused
    copies at 10, 12, 13 and 19."""
    base = os.path.join(root, "storage")
    os.makedirs(base)
    for name, downgrade in (("v18", False), ("v11", True)):
        work = tempfile.mkdtemp(dir=root)
        col = Collection(os.path.join(work, "collection.anki2"))
        col.decks.id("Lang")
        col.close(downgrade=downgrade)
        shutil.copyfile(os.path.join(work, "collection.anki2"), os.path.join(base, name + ".anki2"))
        shutil.rmtree(work)
    for ver, source in ((10, "v11"), (12, "v11"), (13, "v11"), (19, "v18")):
        path = os.path.join(base, "v%d.anki2" % ver)
        shutil.copyfile(os.path.join(base, source + ".anki2"), path)
        con = sqlite3.connect(path)
        con.execute("update col set ver = ?", (ver,))
        con.commit()
        con.close()
    os.mkdir(os.path.join(base, "dir.anki2"))
    # config as releases at 14 to 16 created it: the key column spelled `key`.
    path = os.path.join(base, "lower-key.anki2")
    shutil.copyfile(os.path.join(base, "v18.anki2"), path)
    con = sqlite3.connect(path)
    # The rename re-reads the schema; no unicase index changes, so any
    # comparison serves.
    con.create_collation("unicase", lambda a, b: (a > b) - (a < b))
    con.executescript(
        "alter table config rename to config_old;"
        "create table config (key text not null primary key, usn integer not null,"
        " mtime_secs integer not null, val blob not null) without rowid;"
        "insert into config select * from config_old; drop table config_old;"
    )
    con.close()


def sc(sid, steps, **extra):
    return {"id": sid, "clock": {"tz": "UTC", "start": START}, "steps": steps, **extra}


class StorageOpen(unittest.TestCase):
    def setUp(self):
        # The TAP runner runs each test on its own, so no class fixtures.
        self.root = tempfile.mkdtemp(prefix="fixtures-", dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        make_fixtures(self.root)
        self.handoff = tempfile.mkdtemp(prefix="handoff-", dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        self.worker = WorkerProcess("UTC", extra_env={"ORACLE_FIXTURES": self.root, "ORACLE_HANDOFF": self.handoff})

    def tearDown(self):
        self.worker.close()
        shutil.rmtree(self.root, ignore_errors=True)
        shutil.rmtree(self.handoff, ignore_errors=True)

    def run_steps(self, steps, **extra):
        return self.worker.request({"cmd": "run", "scenario": sc("t/storage", steps, **extra)})

    def ok(self, steps, **extra):
        resp = self.run_steps(steps, **extra)
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"]

    def test_open_upgrades_a_schema_11_file_and_reports_its_objects(self):
        out = self.ok([
            {"open": {"fixture": "storage/v11.anki2"}},
            {"db_exec": {"sql": "SELECT ver FROM col"}},
            {"db_exec": {"sql": "SELECT name FROM decks WHERE name = 'Lang'"}},
            {"observe": ["schema_objects"]},
        ])
        self.assertEqual(out[0], {})
        self.assertEqual(out[1], {"db_exec": {"rows": [[18]]}})
        self.assertEqual(out[2], {"db_exec": {"rows": [["Lang"]]}})
        objects = out[3]["schema_objects"]
        keys = [(o["type"], o["name"]) for o in objects]
        self.assertEqual(keys, sorted(keys))
        by_name = {o["name"]: o for o in objects}
        self.assertEqual(by_name["decks"], {"type": "table", "name": "decks", "tbl_name": "decks", "sql": DECKS_SQL})
        self.assertEqual(by_name["graves"]["sql"], GRAVES_SQL)
        self.assertEqual(by_name["idx_cards_odid"]["sql"], ODID_SQL)
        self.assertEqual(by_name["idx_cards_odid"]["tbl_name"], "cards")
        self.assertTrue(all(o["sql"] is None or "\n" not in o["sql"] for o in objects))

    def test_refused_versions_record_their_kind_and_the_next_open_replaces_them(self):
        out = self.ok([
            {"open": {"fixture": "storage/v10.anki2"}},
            {"open": {"fixture": "storage/v12.anki2"}},
            {"open": {"fixture": "storage/v13.anki2"}},
            {"open": {"fixture": "storage/v19.anki2"}},
            {"open": {"fixture": "storage/v18.anki2"}},
            {"db_exec": {"sql": "SELECT ver FROM col"}},
            {"db_exec": {"sql": "SELECT * FROM pragma_journal_mode"}},
            {"db_exec": {"sql": "SELECT * FROM pragma_integrity_check"}},
        ])
        self.assertEqual(
            out,
            [
                {"open": {"error": "file_too_old"}},
                {"open": {"error": "file_too_new"}},
                {"open": {"error": "file_too_new"}},
                {"open": {"error": "file_too_new"}},
                {},
                {"db_exec": {"rows": [[18]]}},
                {"db_exec": {"rows": [["wal"]]}},
                {"db_exec": {"rows": [["ok"]]}},
            ],
        )

    def test_only_an_open_may_follow_a_refused_open(self):
        for step in ({"db_exec": {"sql": "SELECT 1"}}, {"observe": ["schema_objects"]}, {"handoff": "meguri"}):
            resp = self.run_steps([{"open": {"fixture": "storage/v19.anki2"}}, step])
            self.assertFalse(resp["ok"], step)
            self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 1), step)

    def test_an_open_first_scenario_starts_with_no_collection(self):
        begin = self.worker.request({"cmd": "begin", "scenario": sc("t/storage", [{"open": {"fixture": "storage/v18.anki2"}}])})
        self.assertTrue(begin["ok"], begin)
        try:
            early = self.worker.request({"cmd": "step", "step": {"db_exec": {"sql": "SELECT 1"}}})
            self.assertEqual(early["error"]["kind"], "invalid_scenario", early)
            opened = self.worker.request({"cmd": "step"})
            self.assertEqual(opened["observation"], {}, opened)
            late = self.worker.request({"cmd": "step", "step": {"db_exec": {"sql": "SELECT ver FROM col"}}})
            self.assertEqual(late["observation"], {"db_exec": {"rows": [[18]]}}, late)
        finally:
            self.worker.request({"cmd": "end"})

    def test_open_is_only_the_first_step_of_a_scenario_without_setup(self):
        opening = {"open": {"fixture": "storage/v18.anki2"}}
        for steps, extra in (
            ([{"advance": "1d"}, opening], {}),
            ([opening], {"setup": {"decks": ["A"]}}),
        ):
            resp = self.run_steps(steps, **extra)
            self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)

    def test_a_missing_fixture_is_a_fixture_error_that_changes_nothing(self):
        begin = self.worker.request({"cmd": "begin", "scenario": sc("t/storage", [{"open": {"fixture": "storage/v18.anki2"}}])})
        self.assertTrue(begin["ok"], begin)
        try:
            self.assertTrue(self.worker.request({"cmd": "step"})["ok"])
            marker = {"db_exec": {"sql": "CREATE TABLE marker (x)"}}
            self.assertTrue(self.worker.request({"cmd": "step", "step": marker})["ok"])
            for fixture in ("storage/none.anki2", "storage/dir.anki2"):
                resp = self.worker.request({"cmd": "step", "step": {"open": {"fixture": fixture}}})
                self.assertEqual(resp["error"]["kind"], "fixture_error", resp)
            read = self.worker.request({"cmd": "step", "step": {"db_exec": {"sql": "SELECT count(*) FROM marker"}}})
            self.assertEqual(read["observation"], {"db_exec": {"rows": [[0]]}}, read)
        finally:
            self.worker.request({"cmd": "end"})
        for fixture in ("../v18.anki2", "/storage/v18.anki2", "storage//v18.anki2", "storage/v18.db", 7):
            resp = self.run_steps([{"open": {"fixture": fixture}}])
            self.assertEqual(resp["error"]["kind"], "invalid_scenario", (fixture, resp))

    def test_a_worker_without_fixtures_fails_the_open(self):
        with WorkerProcess("UTC") as w:
            resp = w.request({"cmd": "run", "scenario": sc("t/storage", [{"open": {"fixture": "storage/v18.anki2"}}])})
        self.assertEqual(resp["error"]["kind"], "fixture_error", resp)

    def test_open_and_handoff_keep_the_other_files_of_the_session(self):
        handed = os.path.join(self.handoff, "in")
        os.makedirs(handed)
        shutil.copyfile(os.path.join(self.root, "storage/v18.anki2"), os.path.join(handed, "collection.anki2"))
        con = sqlite3.connect(os.path.join(handed, "collection.anki2"))
        con.execute("create table marker (x)")
        con.close()
        with open(os.path.join(handed, "collection.anki2.pre-upgrade-v11"), "w") as f:
            f.write("kept")
        steps = [
            {"open": {"fixture": "storage/v18.anki2"}},
            {"handoff": "oracle"},
            {"open": {"fixture": "storage/v11.anki2"}},
            {"handoff": "meguri"},
        ]
        resp = self.worker.request({
            "cmd": "run", "scenario": sc("t/storage", steps), "steps": [2, 3],
            "resume": {"dir": "in", "now": 1894701600, "notes": [], "cards": []}, "handoff": "out",
        })
        self.assertTrue(resp["ok"], resp)
        out = os.path.join(self.handoff, "out")
        with open(os.path.join(out, "collection.anki2.pre-upgrade-v11")) as f:
            self.assertEqual(f.read(), "kept")
        con = sqlite3.connect("file:%s?mode=ro" % os.path.join(out, "collection.anki2"), uri=True)
        self.assertEqual(con.execute("select ver from col").fetchone(), (18,))
        self.assertEqual(con.execute("select count(*) from sqlite_master where name = 'marker'").fetchone(), (0,))
        con.close()

    def test_db_dump_reads_an_opened_file(self):
        out = self.ok([{"open": {"fixture": "storage/v11.anki2"}}, {"observe": [{"db_dump": {"tables": ["col", "config"]}}]}])
        dump = out[1]["db_dump"]
        self.assertEqual(sorted(dump), ["col", "config"])
        self.assertEqual(dump["col"]["1"]["ver"], 18)
        self.assertEqual(dump["col"]["1"]["conf"], "")
        self.assertIn("schedVer", dump["config"])

    def test_db_dump_keys_rows_by_the_column_as_the_file_spells_it(self):
        out = self.ok([{"open": {"fixture": "storage/lower-key.anki2"}}, {"observe": [{"db_dump": {"tables": ["config"]}}]}])
        row = out[1]["db_dump"]["config"]["schedVer"]
        self.assertEqual(sorted(row), ["key", "mtime_secs", "usn", "val"])
        self.assertEqual(row["key"], "schedVer")


class Normalize(unittest.TestCase):
    def test_comments_and_white_space(self):
        sql = "CREATE TABLE notes\n(\n    id integer primary key, -- note id\n\tsfld integer not null\r\n)"
        self.assertEqual(normalize(sql), "CREATE TABLE notes ( id integer primary key, sfld integer not null )")
        self.assertEqual(normalize("  a  --x\n--y\nb  "), "a b")
        self.assertIsNone(normalize(None))
