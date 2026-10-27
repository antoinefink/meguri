# SPDX-License-Identifier: AGPL-3.0-or-later
"""The optimize op (specs/conformance/maintenance-ops.md M1) through real
workers.

Expected values come from SQLite's own rules, not from the driver: a dropped
table of a 1,000,000-byte blob leaves far more than 100 free pages and VACUUM
leaves none; ANALYZE's sqlite_stat1 row for an index is the table's row count
followed by the average number of rows per distinct key prefix, so three notes
of one note type with three different sort fields and one usn give "3 3" for
the note type index, "3 1" for the checksum index and "3 3" for the usn index.
A col stamp written at the optimize would carry the advanced clock."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
STAMPS = {"db_exec": {"sql": "SELECT mod, scm, usn, ls FROM col"}}
FREE = {"db_exec": {"sql": "SELECT * FROM pragma_freelist_count"}}
SCRATCH = [
    {"db_exec": {"sql": "CREATE TABLE scratch (payload BLOB)"}},
    {"db_exec": {"sql": "INSERT INTO scratch VALUES (zeroblob(1000000))"}},
    {"db_exec": {"sql": "DROP TABLE scratch"}},
]
NOTES_STAT1 = {"db_exec": {"sql": "SELECT idx, stat FROM sqlite_stat1 WHERE tbl = 'notes' ORDER BY idx"}}


class OptimizeOp(unittest.TestCase):
    def test_optimize_frees_every_page_and_stamps_nothing(self):
        steps = [
            *SCRATCH,
            {"db_exec": {"sql": "SELECT freelist_count > 100 FROM pragma_freelist_count"}},
            {"advance": "1h"},
            STAMPS,
            {"optimize": {}},
            FREE,
            {"db_exec": {"sql": "SELECT * FROM pragma_integrity_check"}},
            STAMPS,
            # Anki marked its proxy modified: a commit request would stamp
            # col.mod now. Closing and opening again sends none.
            {"advance": "1h"},
            {"reopen": {}},
            STAMPS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/optimize", "UTC", START, steps, notes=2))
        self.assertEqual(out[3], {"db_exec": {"rows": [[1]]}})
        before = out[5]["db_exec"]["rows"]
        self.assertEqual(len(before), 1)
        self.assertEqual(out[6], {})
        self.assertEqual(out[7], {"db_exec": {"rows": [[0]]}})
        self.assertEqual(out[8], {"db_exec": {"rows": [["ok"]]}})
        self.assertEqual(out[9], {"db_exec": {"rows": before}})
        self.assertEqual(out[12], {"db_exec": {"rows": before}})

    def test_optimize_writes_the_statistics_of_the_notes_stored(self):
        steps = [NOTES_STAT1, {"optimize": {}}, NOTES_STAT1]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/optimize-stat", "UTC", START, steps, notes=3))
        # A new collection holds no statistics for notes: the optimize wrote them.
        self.assertEqual(out[0], {"db_exec": {"rows": []}})
        self.assertEqual(out[1], {})
        self.assertEqual(out[2], {"db_exec": {"rows": [
            ["idx_notes_mid", "3 3"], ["ix_notes_csum", "3 1"], ["ix_notes_usn", "3 3"]]}})

    def test_optimize_takes_an_empty_mapping(self):
        with WorkerProcess("UTC") as w:
            for args in ({"vacuum": True}, None, [], "now", False, True, 0, 1, 1.0):
                with self.subTest(args=args):
                    resp = w.request({"cmd": "run", "scenario": scenario(
                        "t/optimize", "UTC", START, [{"optimize": args}])})
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)


if __name__ == "__main__":
    unittest.main()
