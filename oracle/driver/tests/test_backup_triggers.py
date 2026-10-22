# SPDX-License-Identifier: AGPL-3.0-or-later
"""The backup op (specs/conformance/backup-trigger-ops.md H1, H3) through real
workers: its argument, its output ({name} or null), the periodic rules it
leaves to Anki (package/backup-triggers.md T2.1, T2.2), a forced backup with no
change, mixing with force_backup, retention within the step, and a replaced
file. Expected names and remaining files come from the specs (package/backups.md
B2.1, B5.3) and the scenario clock, never from the driver."""

from __future__ import annotations

import unittest

from driver.tests.test_backups import FILES, NEW_UTC, Backups, add

INTERVAL_0 = {"config": {"backups": {"daily": 12, "weekly": 10, "monthly": 9, "minimum_interval_mins": 0}}}
PERIODIC = {"backup": {"force": False}}
FORCED = {"backup": {"force": True}}
# A raw write that moves col.mod: a change for T2.1.
CHANGE = {"db_exec": {"sql": "UPDATE col SET mod = mod + 1"}}


def made(name):
    return {"backup": {"name": name}}


def at(clock):
    return f"backup-2030-01-15-{clock}.colpkg"


NONE = made(None)


class Trigger(Backups):
    def test_a_new_collection_and_an_empty_folder_get_a_periodic_backup(self):
        # T2.1: no reference is a change; T2.2: no folder, no entry. The op
        # creates the folder (F1).
        steps = self.ok([PERIODIC, FILES])
        self.assertEqual(steps, [made(NEW_UTC), {"backup_files": [NEW_UTC]}])

    def test_an_entry_blocks_a_periodic_backup_not_a_forced_one(self):
        # T2.2, default interval 30: a fresh foreign file blocks; force goes
        # ahead; the forced backup then blocks the next periodic one after a
        # change.
        steps = self.ok([add("notes.txt"), PERIODIC, FORCED, {"advance": "1s"}, CHANGE, PERIODIC, FILES])
        self.assertEqual((steps[1], steps[2], steps[5]), (NONE, made(NEW_UTC), NONE))
        self.assertEqual(steps[6], {"backup_files": [NEW_UTC, "notes.txt"]})

    def test_no_change_gives_null_forced_or_not(self):
        # T2.1 for both flags in one opening: null, never an error.
        steps = self.ok([FORCED, {"advance": "1m"}, FORCED, PERIODIC, FILES], setup=INTERVAL_0)
        self.assertEqual(steps, [made(NEW_UTC), {}, NONE, NONE, {"backup_files": [NEW_UTC]}])

    def test_a_change_lets_the_next_backup_through(self):
        steps = self.ok([PERIODIC, {"advance": "1s"}, PERIODIC, CHANGE, PERIODIC, FILES], setup=INTERVAL_0)
        self.assertEqual((steps[0], steps[2], steps[4]), (made(NEW_UTC), NONE, made(at("10.00.01"))))
        self.assertEqual(steps[5], {"backup_files": [NEW_UTC, at("10.00.01")]})

    def test_mixed_with_force_backup_in_one_opening(self):
        # H3: backup steps before and after a force_backup in one opening (F7
        # counts force_backup only), and every backup sets the reference.
        steps = self.ok([
            FORCED, {"advance": "1s"}, CHANGE, {"force_backup": {}}, PERIODIC,
            {"advance": "1s"}, CHANGE, FORCED, PERIODIC, {"advance": "1s"}, CHANGE, PERIODIC, FILES,
        ], setup=INTERVAL_0)
        self.assertEqual(steps[0], made(NEW_UTC))
        self.assertEqual((steps[3], steps[4]), ({"force_backup": {"name": at("10.00.01")}}, NONE))
        self.assertEqual((steps[7], steps[8]), (made(at("10.00.02")), NONE))
        self.assertEqual(steps[11], made(at("10.00.03")))
        self.assertEqual(steps[12]["backup_files"], [NEW_UTC, at("10.00.01"), at("10.00.02"), at("10.00.03")])

    def test_retention_runs_within_the_step(self):
        # package/backups.md B5.3: limits 1, 1, 1 on Tuesday 2030-01-15; the
        # interval 0 lets the seeded files through (T2.2).
        for step in (PERIODIC, FORCED):
            steps = self.ok([
                add("backup-2030-01-14-09.00.00.colpkg", "backup-2030-01-12-20.00.00.colpkg",
                    "backup-2030-01-12-08.00.00.colpkg", "backup-2030-01-08-12.00.00.colpkg",
                    "backup-2030-01-07-12.00.00.colpkg", "backup-2030-01-06-12.00.00.colpkg",
                    "backup-2030-01-02-12.00.00.colpkg", "backup-2029-12-20-12.00.00.colpkg",
                    "backup-2029-12-02-12.00.00.colpkg", "backup-2029-06-01-12.00.00.colpkg", "notes.txt"),
                step,
                FILES,
            ], setup={"config": {"backups": {"daily": 1, "weekly": 1, "monthly": 1, "minimum_interval_mins": 0}}})
            with self.subTest(step=step):
                self.assertEqual(steps[1], made(NEW_UTC))
                self.assertEqual(steps[2]["backup_files"], [
                    "backup-2029-12-20-12.00.00.colpkg", "backup-2030-01-06-12.00.00.colpkg",
                    "backup-2030-01-12-20.00.00.colpkg", "backup-2030-01-14-09.00.00.colpkg", NEW_UTC, "notes.txt",
                ])

    def test_a_backup_that_replaced_a_seeded_file_is_invalid(self):
        # H1, H3: Anki reports a backup but no name was added.
        for step in (PERIODIC, FORCED):
            err = self.error([add(NEW_UTC), step], setup=INTERVAL_0)
            with self.subTest(step=step):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)

    def test_malformed_arguments(self):
        for args in (None, [], {}, True, {"force": 1}, {"force": 0}, {"force": "true"}, {"force": None},
                     {"force": True, "x": 1}, {"x": True}):
            err = self.error([{"advance": "1m"}, {"backup": args}])
            with self.subTest(args=args):
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)


if __name__ == "__main__":
    unittest.main()
