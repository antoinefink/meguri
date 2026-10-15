# Oracle driver: open, schema_objects and the storage reads (U22)

Read at tag 26.09.3 to choose the `anki` Python APIs behind `open`
(`driver/ops/storage_open.py`) and `schema_objects` (`driver/observe/schema.py`),
and to check that `db_exec` (`driver/ops/storage.py`, unit/U21's file, the same
code as U23's) and `db_dump` (`driver/observe/db_dump.py`, U13's file) read
upgraded files; meanings in `specs/conformance/storage-ops.md` and
`observations.md` O8. No code was copied.

| Anki file | Read for |
|---|---|
| `rslib/src/storage/sqlite.rs` (`open_or_create`, `schema_version`) | the version refusals happen before any upgrade, as a DB error with an empty info |
| `rslib/src/error/db.rs` | the DB error message is the debug text of the error, so it names the kind (`FileTooOld`, `FileTooNew`) without localization |
| `pylib/anki/collection.py` (`__init__`, `reopen`, `_load_scheduler`, `close`) | `Collection(path)` opens through the backend and then loads the scheduler, which writes only for v3 without `creationOffset`; `close(downgrade=False)` |
| `rslib/src/config/bool.rs` | `sched2021` defaults to false, so files from releases without it load the dummy scheduler and stay unwritten |
| `pylib/anki/dbproxy.py` | `col.db.all` for the `sqlite_master` read |

Probed in the oracle image (`driver/tests/test_storage_open.py` and the
storage goldens): versions 10, 12, 13 and 19 are refused with those kinds in
rollback-journal and WAL files; a refused open leaves no collection; the
table-valued `pragma_integrity_check` and `pragma_journal_mode` come back as
rows through `db_exec`. Files made by releases at 14 to 16 spell config's key
column `key` (and 2.1.44 names a table `FIELDS`): `db_dump` now finds its key
columns whatever their letter case, which U13's version did not (KeyError).
