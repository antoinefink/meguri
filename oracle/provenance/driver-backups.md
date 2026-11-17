# Oracle driver: backups (U126)

Read at tag 26.09.3 to choose the `anki` calls behind `add_backup_files`,
`force_backup`, `save_backup_settings` and the observations `backup_files` and
`backup_settings` (`driver/ops/backups.py`). Meanings are
`specs/conformance/backup-ops.md` F1 to F7; the folder, its files and their
listing are the driver's own file operations, and the series names follow
`specs/package/backups.md` B2.1. No code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/collection.py` (`create_backup`, `get_preferences`, `set_preferences`, the `Preferences` alias) | the forced backup with `wait_for_completion` returns false when it skips; preferences are read and saved as one message |
| `rslib/src/backend/collection.rs` (`create_backup`) | a waited backup joins its thread, so the write and the thinning are done when the call returns, and a failure of either raises |
| `rslib/src/collection/backup.rs`, `rslib/src/collection/timestamps.rs` | the folder must exist (no creation); the name comes from the process's local time, which the worker's frozen clock and `TZ` set; the skip when nothing changed since the last backup of this opening, reset by every open (open, reopen, an import's reopen, a handoff's reopen) |
| `rslib/src/preferences.rs`, `proto/anki/config.proto` (preferences message) | the save writes only the groups present in the message, so a message with the backup group alone changes only the `backups` key; the group must be present even when all four values are zero |

Probed in the oracle image (`driver/tests/test_backups.py` and the goldens of
`conformance/scenarios/package/backups/`): Python's protobuf marks the backup
group present for all-zero values, and the save then writes
`{"daily":0,"weekly":0,"monthly":0,"minimum_interval_mins":0}`; the save
leaves every other `config` row as it was; a second forced backup in one
opening is skipped even after the clock moved; the name follows the worker's
zone (Tokyo, New York, UTC); the copy of a backup imports with Anki's
collection package import.

## The backup op (U127)

`backup {force}` (`driver/ops/backups.py`, meanings
`specs/conformance/backup-trigger-ops.md` H1 and H3) makes the same call as
`force_backup` with the scenario's force flag, through the same helper: the
name is the one regular file the call added, and a backup Anki reports made
with no file added is `invalid_scenario`. It differs only where Anki makes no
backup, which it records as a null name instead of an error. The rules that
decide it are Anki's, read for `oracle/provenance/backup-triggers.md`. No code
was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/collection.py` (`create_backup`) | the force flag skips only the interval check; the boolean is false whenever no backup was made, a forced one with no change included |

Probed in the oracle image (`driver/tests/test_backup_triggers.py` and the
goldens of `conformance/scenarios/package/backup-triggers/`): a new collection
with no backups folder gets a periodic backup; a fresh foreign file blocks a
periodic backup and not a forced one; with no change both flags give null in
one opening; `backup` and `force_backup` steps mix in one opening and each
sets the reference; retention runs within the step for both flags.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| package/backups/three-stages and package/backups/retention-daily-one-year (specs/package/backups.md B5.1, B5.2, B6.1) | `rslib/src/collection/backup.rs` | Which retention behaviors the two thinning tests prove: no deletion for today, yesterday and later dates, the daily/weekly/monthly stage selections with Monday weeks, and the default limits over a year of daily backups. The three-stages limits, dates and cases are the scenario's own. The expectations come from the oracle goldens. |
