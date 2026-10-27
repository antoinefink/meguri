# SPDX-License-Identifier: AGPL-3.0-or-later
"""schema11_copy, open {file} and the session-file observations file_col,
file_objects and file_query (specs/conformance/storage-ops.md P1, P4-P8)
through real workers.

The reference for a copy is Anki's own in-place downgrade (close with the
downgrade) of another copy of the same file, read here with sqlite3; literal
DDL is Anki's schema-11 graves definition as storage-ops.md and
schema11-downgrade.md give it, not what the driver reads."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import sqlite3
import tempfile
import unittest

from anki.collection import Collection

from driver.harness import WorkerProcess

START = "2030-01-15T10:00:00"
OPEN = {"open": {"fixture": "storage/v18.anki2"}}
COPY = {"schema11_copy": {"name": "legacy.anki2"}}
ALL_COLUMNS = ["crt", "mod", "scm", "ver", "dty", "usn", "ls", "conf", "models", "decks", "dconf", "tags"]
INT_COLUMNS = ["crt", "mod", "scm", "ver", "dty", "usn", "ls"]
GRAVES_11 = "CREATE TABLE graves ( usn integer NOT NULL, oid integer NOT NULL, type integer NOT NULL )"
SCHEMA_18_ONLY = ("config", "deck_config", "tags", "fields", "templates", "notetypes", "decks",
                  "idx_cards_odid", "idx_notes_mid")
DUMP = {"observe": ["schema_objects", "db_dump"]}


def file_col(name, columns):
    return {"file_col": {"name": name, "columns": columns}}


def file_query(name, sql):
    return {"observe": [{"file_query": {"name": name, "sql": sql}}]}


def make_fixture(root: str) -> None:
    """storage/v18.anki2: a schema-18 file Anki wrote, with a deck, a tagged
    note and the graves of a deleted note."""
    base = os.path.join(root, "storage")
    os.makedirs(base)
    work = tempfile.mkdtemp(dir=root)
    col = Collection(os.path.join(work, "collection.anki2"))
    did = col.decks.id("Lang")
    basic = col.models.by_name("Basic")
    for front in ("kept", "gone"):
        note = col.new_note(basic)
        note["Front"] = front
        note.tags = ["vocab"]
        col.add_note(note, did)
    col.remove_notes([col.find_notes("Front:gone")[0]])
    col.close(downgrade=False)
    shutil.copyfile(os.path.join(work, "collection.anki2"), os.path.join(base, "v18.anki2"))
    shutil.rmtree(work)


def anki_downgrade(source: str, scratch: str) -> str:
    """Anki's in-place downgrade of a copy of source: the reference file."""
    work = tempfile.mkdtemp(dir=scratch)
    path = os.path.join(work, "collection.anki2")
    shutil.copyfile(source, path)
    Collection(path).close(downgrade=True)
    return path


def read_db(path: str, sql: str) -> list:
    con = sqlite3.connect("file:%s?mode=ro" % path, uri=True)
    try:
        con.create_collation("unicase", lambda a, b: (a.casefold() > b.casefold()) - (a.casefold() < b.casefold()))
        return con.execute(sql).fetchall()
    finally:
        con.close()


def reference_col(path: str) -> dict:
    [row] = read_db(path, "SELECT %s FROM col" % ", ".join(ALL_COLUMNS))
    return {c: (v if c in INT_COLUMNS else json.loads(v)) for c, v in zip(ALL_COLUMNS, row)}


def sc(steps, sid="t/downgrade", **extra):
    return {"id": sid, "clock": {"tz": "UTC", "start": START}, "steps": steps, **extra}


class Downgrade(unittest.TestCase):
    def setUp(self):
        # The TAP runner runs each test on its own, so no class fixtures.
        scratch = os.environ.get("ORACLE_SCRATCH_ROOT")
        self.root = tempfile.mkdtemp(prefix="fixtures-", dir=scratch)
        make_fixture(self.root)
        self.fixture = os.path.join(self.root, "storage/v18.anki2")
        self.handoff = tempfile.mkdtemp(prefix="handoff-", dir=scratch)
        self.work = tempfile.mkdtemp(prefix="work-", dir=scratch)
        self.worker = WorkerProcess("UTC", extra_env={"ORACLE_FIXTURES": self.root, "ORACLE_HANDOFF": self.handoff})

    def tearDown(self):
        self.worker.close()
        for d in (self.root, self.handoff, self.work):
            shutil.rmtree(d, ignore_errors=True)

    def run_steps(self, scenario_steps, **req):
        """A run of the scenario; req adds a segment's steps, resume and handoff."""
        return self.worker.request({"cmd": "run", "scenario": sc(scenario_steps), **req})

    def ok(self, scenario_steps, **req):
        resp = self.run_steps(scenario_steps, **req)
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"]

    def failed(self, steps, kind, step):
        resp = self.run_steps(steps)
        self.assertFalse(resp["ok"], resp)
        self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), (kind, step), resp)
        return resp["error"]

    def handed(self, rel, name="legacy.anki2"):
        return os.path.join(self.handoff, rel, name)

    # --- schema11_copy ------------------------------------------------------------

    def test_the_copy_is_ankis_schema_11_file_and_the_collection_does_not_change(self):
        out = self.ok([
            OPEN,
            DUMP,
            COPY,
            DUMP,
            {"observe": [file_col("legacy.anki2", ALL_COLUMNS), {"file_objects": {"name": "legacy.anki2"}}]},
            file_query("legacy.anki2", "SELECT * FROM pragma_journal_mode"),
            file_query("legacy.anki2", "SELECT * FROM pragma_integrity_check"),
            file_query("legacy.anki2", "SELECT usn, oid, type FROM graves ORDER BY oid, type"),
            file_query("legacy.anki2", "SELECT id, tags, flds FROM notes ORDER BY id"),
            {"db_exec": {"sql": "SELECT * FROM pragma_journal_mode"}},
            {"db_exec": {"sql": "SELECT ver FROM col"}},
            COPY,
            {"observe": [file_col("legacy.anki2", ALL_COLUMNS)]},
        ])
        self.assertEqual(out[2], {})
        self.assertEqual(out[1], out[3])
        reference = anki_downgrade(self.fixture, self.work)
        self.assertEqual(out[4]["file_col"], reference_col(reference))
        self.assertEqual(out[4]["file_col"]["ver"], 11)
        objects = out[4]["file_objects"]
        listed = [(o["type"], o["name"], o["tbl_name"]) for o in objects]
        self.assertEqual(listed, sorted(read_db(reference, "SELECT type, name, tbl_name FROM sqlite_master")))
        self.assertFalse(set(SCHEMA_18_ONLY) & {o["name"] for o in objects})
        by_name = {o["name"]: o for o in objects}
        self.assertEqual(by_name["graves"]["sql"], GRAVES_11)
        self.assertTrue(all(o["sql"] is None or ("\n" not in o["sql"] and "  " not in o["sql"]) for o in objects))
        self.assertEqual(out[5], {"file_query": {"rows": [["delete"]]}})
        self.assertEqual(out[6], {"file_query": {"rows": [["ok"]]}})
        graves = read_db(reference, "SELECT usn, oid, type FROM graves ORDER BY oid, type")
        self.assertTrue(graves)
        self.assertEqual(out[7], {"file_query": {"rows": [list(r) for r in graves]}})
        notes = read_db(reference, "SELECT id, tags, flds FROM notes ORDER BY id")
        self.assertEqual(out[8], {"file_query": {"rows": [list(r) for r in notes]}})
        # The collection stays open, at 18 and in WAL mode.
        self.assertEqual((out[9], out[10]), ({"db_exec": {"rows": [["wal"]]}}, {"db_exec": {"rows": [[18]]}}))
        # A second copy replaces the file with the same content.
        self.assertEqual(out[11], {})
        self.assertEqual(out[12]["file_col"], out[4]["file_col"])

    def test_the_copy_takes_the_collection_as_it_is_now(self):
        out = self.ok([
            OPEN,
            {"db_exec": {"sql": "UPDATE notes SET tags = ' other ' WHERE id = (SELECT min(id) FROM notes)"}},
            {"set_config": {"key": "zzBig", "value": 18446744073709551615}},
            {"set_config": {"key": "zzFloat", "value": 1.0}},
            {"set_config": {"key": "zzNeg", "value": -9223372036854775808}},
            {"db_exec": {"sql": "SELECT crt, mod, scm, dty, usn, ls FROM col"}},
            COPY,
            {"observe": [file_col("legacy.anki2", ["conf", "crt", "mod", "scm", "dty", "usn", "ls"])]},
            file_query("legacy.anki2", "SELECT tags FROM notes ORDER BY id"),
        ])
        crt, mod, scm, dty, usn, ls = out[5]["db_exec"]["rows"][0]
        value = out[7]["file_col"]
        self.assertEqual({k: value[k] for k in INT_COLUMNS if k != "ver"},
                         {"crt": crt, "mod": mod, "scm": scm, "dty": dty, "usn": usn, "ls": ls})
        conf = value["conf"]
        self.assertEqual((conf["zzBig"], conf["zzFloat"], conf["zzNeg"]), (18446744073709551615, 1.0, -9223372036854775808))
        self.assertEqual([type(conf[k]) for k in ("zzBig", "zzFloat", "zzNeg")], [int, float, int])
        self.assertEqual(out[8], {"file_query": {"rows": [[" other "]]}})

    def test_a_config_value_anki_cannot_read_refuses_the_copy_and_writes_nothing(self):
        steps = [
            OPEN,
            {"db_exec": {"sql": "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('zzWide', -1, 0, CAST(? AS BLOB))",
                         "params": ['{"zz": [1, 1e400]}']}},
            {"observe": [{"db_dump": {"tables": ["col", "config"]}}, "schema_objects"]},
            COPY,
            {"observe": [{"db_dump": {"tables": ["col", "config"]}}, "schema_objects"]},
            {"handoff": "meguri"},
            {"db_exec": {"sql": "UPDATE config SET val = CAST(? AS BLOB) WHERE KEY = 'zzWide'", "params": ['{"zz": [1, 1e300]}']}},
            COPY,
            {"handoff": "oracle"},
            {"db_exec": {"sql": "UPDATE config SET val = CAST(? AS BLOB) WHERE KEY = 'zzWide'", "params": ['{"zz": [1, ']}},
            COPY,
            {"handoff": "meguri"},
            {"observe": [file_col("legacy.anki2", ["ver", "conf"])]},
        ]
        whole = self.ok(steps)
        self.assertEqual(whole[3], {"schema11_copy": {"error": "invalid_json"}})
        self.assertEqual(whole[2], whole[4])
        self.assertEqual((whole[7], whole[10]), ({}, {"schema11_copy": {"error": "invalid_json"}}))
        self.assertEqual(whole[12]["file_col"]["ver"], 11)
        self.assertEqual(whole[12]["file_col"]["conf"]["zzWide"], {"zz": [1, 1e300]})
        # In segments: no file after the first refusal, and the second one
        # leaves the file a successful copy wrote with its bytes.
        first = self.run_steps(steps, steps=[0, 5], handoff="a")
        self.assertTrue(first["ok"], first)
        self.assertFalse(os.path.exists(self.handed("a")))
        second = self.run_steps(steps, steps=[6, 8], resume=first["state"], handoff="b")
        self.assertTrue(second["ok"], second)
        with open(self.handed("b"), "rb") as f:
            written = f.read()
        third = self.run_steps(steps, steps=[9, 11], resume=second["state"], handoff="c")
        self.assertTrue(third["ok"], third)
        with open(self.handed("c"), "rb") as f:
            self.assertEqual(f.read(), written)
        last = self.run_steps(steps, steps=[12, 13], resume=third["state"])
        self.assertTrue(last["ok"], last)
        segments = first["observations"]["steps"] + second["observations"]["steps"] + third["observations"]["steps"]
        self.assertEqual(segments + last["observations"]["steps"], whole[:5] + whole[6:8] + whole[9:11] + whole[12:])

    def test_other_copy_failures_are_run_errors_that_leave_the_collection_open(self):
        begin = self.worker.request({"cmd": "begin", "scenario": sc([OPEN])})
        self.assertTrue(begin["ok"], begin)
        try:
            self.assertTrue(self.worker.request({"cmd": "step"})["ok"])
            text = {"db_exec": {"sql": "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('zzText', -1, 0, '{}')"}}
            self.assertTrue(self.worker.request({"cmd": "step", "step": text})["ok"])
            resp = self.worker.request({"cmd": "step", "step": COPY})
            self.assertFalse(resp["ok"], resp)
            self.assertEqual((resp["error"]["kind"], resp["error"].get("op")), ("anki_error", "schema11_copy"), resp)
            read = self.worker.request({"cmd": "step", "step": {"db_exec": {"sql": "SELECT count(*) FROM config WHERE KEY = 'zzText'"}}})
            self.assertEqual(read["observation"], {"db_exec": {"rows": [[1]]}}, read)
            missing = self.worker.request({"cmd": "step", "step": {"observe": [file_col("legacy.anki2", ["ver"])]}})
            self.assertEqual(missing["error"]["kind"], "unknown_ref", missing)
        finally:
            self.worker.request({"cmd": "end"})

    def test_malformed_copy_requests(self):
        for args in (None, "legacy.anki2", {}, {"name": "collection.anki2"}, {"name": "Collection.anki2"},
                     {"name": "COLLECTION.anki2"}, {"name": "Legacy.anki2"}, {"name": "x.ANKI2"},
                     {"name": "x.apkg"}, {"name": "../x.anki2"},
                     {"name": "a/x.anki2"}, {"name": ".x.anki2"}, {"name": "x.anki2\n"}, {"name": 1},
                     {"name": "x.anki2", "extra": 1}):
            with self.subTest(args=args):
                self.failed([OPEN, {"schema11_copy": args}], "invalid_scenario", 1)

    # --- open {file} --------------------------------------------------------------

    def test_open_file_opens_a_copy_of_the_session_file_and_keeps_it(self):
        read = {"observe": [file_col("legacy.anki2", ["ver", "conf", "models", "decks", "dconf", "tags"]),
                            {"file_objects": {"name": "legacy.anki2"}}]}
        out = self.ok([
            OPEN,
            COPY,
            read,
            {"db_exec": {"sql": "CREATE TABLE marker (x)"}},
            {"open": {"file": "legacy.anki2"}},
            {"db_exec": {"sql": "SELECT ver FROM col"}},
            {"db_exec": {"sql": "SELECT count(*) FROM sqlite_master WHERE name = 'marker'"}},
            {"db_exec": {"sql": "SELECT * FROM pragma_journal_mode"}},
            {"db_exec": {"sql": "SELECT name FROM decks WHERE name = 'Lang'"}},
            read,
            {"open": {"file": "legacy.anki2"}},
        ])
        self.assertEqual(out[4], {})
        self.assertEqual(out[5:9], [{"db_exec": {"rows": [[18]]}}, {"db_exec": {"rows": [[0]]}},
                                    {"db_exec": {"rows": [["wal"]]}}, {"db_exec": {"rows": [["Lang"]]}}])
        self.assertEqual(out[9], out[2])
        self.assertEqual(out[10], {})

    def test_open_file_errors(self):
        err = self.failed([OPEN, {"open": {"file": "nope.anki2"}}], "unknown_ref", 1)
        self.assertEqual(err["ref"], "nope.anki2")
        # The first open names a fixture (storage-ops.md P1.3).
        self.failed([{"open": {"file": "legacy.anki2"}}], "invalid_scenario", 0)
        for args in ({"fixture": "storage/v18.anki2", "file": "legacy.anki2"}, {}, {"file": "collection.anki2"},
                     {"file": "Collection.anki2"}, {"file": "x.apkg"}, {"file": "../legacy.anki2"}, {"file": 1}):
            with self.subTest(args=args):
                self.failed([OPEN, COPY, {"open": args}], "invalid_scenario", 2)

    # --- the session file crosses a handoff -----------------------------------------

    def test_the_session_file_crosses_a_handoff_and_opens_on_the_other_side(self):
        steps = [
            OPEN,
            COPY,
            {"handoff": "oracle"},
            {"observe": [file_col("legacy.anki2", ALL_COLUMNS), {"file_objects": {"name": "legacy.anki2"}}]},
            file_query("legacy.anki2", "SELECT * FROM pragma_journal_mode"),
            {"open": {"file": "legacy.anki2"}},
            DUMP,
        ]
        whole = self.ok(steps)
        first = self.run_steps(steps, steps=[0, 2], handoff="h")
        self.assertTrue(first["ok"], first)
        reference = anki_downgrade(self.fixture, self.work)
        self.assertEqual(reference_col(self.handed("h")), reference_col(reference))
        second = self.run_steps(steps, steps=[3, 7], resume=first["state"])
        self.assertTrue(second["ok"], second)
        self.assertEqual(first["observations"]["steps"] + second["observations"]["steps"], whole[:2] + whole[3:])
        self.assertEqual(whole[4], {"file_query": {"rows": [["delete"]]}})

    # --- file_col, file_objects, file_query on files the test writes ----------------

    def write_and_read(self, edit, request):
        """Copy, hand over, let edit(path) change the handed file, then
        observe request on the other side; the response."""
        steps = [OPEN, COPY, {"handoff": "meguri"}, {"observe": [request]}]
        first = self.run_steps(steps, steps=[0, 2], handoff="w")
        self.assertTrue(first["ok"], first)
        edit(self.handed("w"))
        return self.run_steps(steps, steps=[3, 4], resume=first["state"])

    def test_file_col_on_files_that_do_not_read_is_a_run_error(self):
        def sql(*statements):
            def edit(path):
                con = sqlite3.connect(path)
                for s in statements:
                    con.execute(s)
                con.commit()
                con.close()
            return edit

        def junk(path):
            with open(path, "wb") as f:
                f.write(b"not a database at all, not even close" * 200)

        cases = [
            ("two col rows", sql("INSERT INTO col SELECT id + 1, crt, mod, scm, ver, dty, usn, ls, conf, models, decks, dconf, tags FROM col"), ["ver"]),
            ("no col row", sql("DELETE FROM col"), ["ver"]),
            ("no col table", sql("DROP TABLE col"), ["ver"]),
            ("ver as text", sql("UPDATE col SET ver = 'eleven'"), ["ver"]),
            ("crt as real", sql("UPDATE col SET crt = 1.5"), ["crt"]),
            ("conf not JSON", sql("UPDATE col SET conf = '{'"), ["conf"]),
            ("conf as blob", sql("UPDATE col SET conf = X'7b7d'"), ["conf"]),
            ("repeated member", sql("""UPDATE col SET models = '{"5": {"a": 1, "a": 2}}'"""), ["models"]),
            ("NaN", sql("""UPDATE col SET dconf = '{"1": {"x": NaN}}'"""), ["dconf"]),
            ("beyond f64", sql("""UPDATE col SET decks = '{"1": {"x": 1e400}}'"""), ["decks"]),
            ("not a database", junk, ["ver"]),
        ]
        for label, edit, columns in cases:
            with self.subTest(label):
                resp = self.write_and_read(edit, file_col("legacy.anki2", columns))
                self.assertFalse(resp["ok"], resp)
                self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("internal_error", 3), resp)
                shutil.rmtree(os.path.join(self.handoff, "w"))

    def test_file_col_parses_json_columns_with_number_kinds_kept(self):
        models = '{"5": {"a": 1.0, "b": 2, "c": 1e3, "d": [0.5, -3], "e": {"z": null}, "f": 18446744073709551615}}'

        def edit(path):
            con = sqlite3.connect(path)
            con.execute("UPDATE col SET models = ?, tags = ?", (models, '{"b": 0, "a": 1}'))
            con.commit()
            con.close()

        resp = self.write_and_read(edit, file_col("legacy.anki2", ["models", "tags", "ver"]))
        self.assertTrue(resp["ok"], resp)
        value = resp["observations"]["steps"][0]["file_col"]
        self.assertEqual(value, {"models": {"5": {"a": 1.0, "b": 2, "c": 1000.0, "d": [0.5, -3], "e": {"z": None},
                                                  "f": 18446744073709551615}},
                                 "tags": {"a": 1, "b": 0}, "ver": 11})
        nt = value["models"]["5"]
        self.assertEqual([type(nt[k]) for k in "abcf"], [float, int, float, int])

    def test_file_query_values_by_storage_class_and_the_unicase_collation(self):
        sql = "SELECT 1, 1.0, 2.5e-7, 'x', X'00ff', NULL, 'a' = 'A' COLLATE unicase, 'ß' = 'SS' COLLATE unicase, 'b' < 'A' COLLATE unicase"
        out = self.ok([OPEN, COPY, file_query("legacy.anki2", sql),
                       file_query("legacy.anki2", "SELECT * FROM pragma_table_info('graves') ORDER BY cid"),
                       file_query("legacy.anki2", "SELECT id FROM revlog")])
        self.assertEqual(out[2], {"file_query": {"rows": [[1, 1.0, 2.5e-7, "x", {"sha256": hashlib.sha256(b"\x00\xff").hexdigest()},
                                                           None, 1, 1, 0]]}})
        self.assertEqual(type(out[2]["file_query"]["rows"][0][1]), float)
        self.assertEqual([r[1] for r in out[3]["file_query"]["rows"]], ["usn", "oid", "type"])
        self.assertEqual(out[4], {"file_query": {"rows": []}})

    def test_file_query_text_that_is_not_utf8_reads_as_replacement_characters(self):
        def edit(path):
            con = sqlite3.connect(path)
            con.execute("UPDATE notes SET tags = CAST(X'61ff62' AS TEXT)")
            con.commit()
            con.close()

        resp = self.write_and_read(edit, {"file_query": {"name": "legacy.anki2", "sql": "SELECT tags FROM notes"}})
        self.assertTrue(resp["ok"], resp)
        self.assertEqual(resp["observations"]["steps"][0], {"file_query": {"rows": [["a�b"]]}})

    def test_file_query_statements_that_write_or_are_not_one_query_are_invalid(self):
        for sql in ("DELETE FROM revlog", "UPDATE col SET ver = 12", "CREATE TABLE x (a)", "DROP TABLE revlog",
                    "PRAGMA journal_mode = wal", "PRAGMA journal_mode", "ATTACH ':memory:' AS other",
                    "BEGIN", "SELECT 1; SELECT 2", "", "   "):
            with self.subTest(sql=sql):
                self.failed([OPEN, COPY, file_query("legacy.anki2", sql), file_query("legacy.anki2", "SELECT 1")],
                            "invalid_scenario", 2)
        # A refused statement changed nothing: the session file still reads.
        out = self.ok([OPEN, COPY, file_query("legacy.anki2", "SELECT * FROM pragma_journal_mode"),
                       file_query("legacy.anki2", "SELECT count(*) FROM revlog")])
        self.assertEqual(out[2], {"file_query": {"rows": [["delete"]]}})

    def test_file_query_failures_are_run_errors(self):
        self.failed([OPEN, COPY, file_query("legacy.anki2", "SELECT * FROM notetypes")], "invalid_scenario", 2)
        self.failed([OPEN, COPY, file_query("legacy.anki2", "SELEC 1")], "invalid_scenario", 2)

    def test_reads_change_neither_the_file_nor_the_collection(self):
        reads = [{"observe": [file_col("legacy.anki2", ALL_COLUMNS), {"file_objects": {"name": "legacy.anki2"}}]},
                 file_query("legacy.anki2", "SELECT * FROM pragma_integrity_check"),
                 file_query("legacy.anki2", "SELECT * FROM notes ORDER BY id")]
        steps = [OPEN, COPY, DUMP] + reads + reads + [DUMP]
        first = self.run_steps(steps, steps=[0, 2], handoff="before")
        self.assertTrue(first["ok"], first)
        second = self.run_steps(steps, steps=[2, len(steps)], resume=first["state"], handoff="after")
        self.assertTrue(second["ok"], second)
        out = second["observations"]["steps"]
        self.assertEqual(out[1:4], out[4:7])
        self.assertEqual(out[0], out[7])
        with open(self.handed("before"), "rb") as a, open(self.handed("after"), "rb") as b:
            self.assertEqual(a.read(), b.read())

    def test_unknown_files_and_malformed_observation_requests(self):
        for request in (file_col("nope.anki2", ["ver"]), {"file_objects": {"name": "nope.anki2"}},
                        {"file_query": {"name": "nope.anki2", "sql": "SELECT 1"}}):
            with self.subTest(request=request):
                err = self.failed([OPEN, COPY, {"observe": [request]}], "unknown_ref", 2)
                self.assertEqual(err["ref"], "nope.anki2")
        ok_col = {"name": "legacy.anki2", "columns": ["ver"]}
        bad = [
            {"file_col": None}, {"file_col": "legacy.anki2"}, {"file_col": {"name": "legacy.anki2"}},
            {"file_col": dict(ok_col, columns=[])}, {"file_col": dict(ok_col, columns=["ver", "ver"])},
            {"file_col": dict(ok_col, columns=["id"])}, {"file_col": dict(ok_col, columns="ver")},
            {"file_col": dict(ok_col, name="collection.anki2")}, {"file_col": dict(ok_col, name="Collection.anki2")},
            {"file_col": dict(ok_col, name="../legacy.anki2")},
            {"file_col": dict(ok_col, name="x.apkg")}, {"file_col": dict(ok_col, extra=1)},
            {"file_objects": None}, {"file_objects": {}}, {"file_objects": {"name": "collection.anki2"}},
            {"file_objects": {"name": "legacy.anki2", "extra": 1}},
            {"file_query": {"name": "legacy.anki2"}}, {"file_query": {"name": "legacy.anki2", "sql": 1}},
            {"file_query": {"name": "collection.anki2", "sql": "SELECT 1"}},
            {"file_query": {"name": "legacy.anki2", "sql": "SELECT 1", "params": []}},
        ]
        for request in bad:
            with self.subTest(request=request):
                self.failed([OPEN, COPY, {"observe": [request]}], "invalid_scenario", 2)


if __name__ == "__main__":
    unittest.main()
