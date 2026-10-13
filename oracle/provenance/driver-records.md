# Oracle driver: record ops (U23)

Read at tag 26.09.3 to choose the `anki` Python APIs behind `db_exec`,
`edit_note`, `delete_notes` and `set_config` (`driver/ops/storage.py`,
`driver/ops/notes.py`; meanings in `specs/conformance/record-ops.md`); no code
was copied. The specs' own sources are in `storage-records.md`.

| Anki file | Read for |
|---|---|
| `pylib/anki/dbproxy.py`, `pylib/anki/_backend.py` | `col.db.all` sends one query with positional arguments and no begin or commit request; how results come back (JSON from the backend) |
| `rslib/src/backend/dbproxy.rs` | value mapping in both directions (TEXT decoded lossily, BLOB as a byte list, untagged argument values), the undo and study-queue reset after a statement that does not start with `select`, `col.mod` set only on a commit request |
| `rslib/src/undo/mod.rs`, `rslib/src/collection/transact.rs` | what that reset clears; an op with no changes does not move `col.mod` |
| `pylib/anki/collection.py` | `get_note`, `update_note` (undoable by default), `remove_notes`, `set_config` (no undo entry by default), `undo_status` for the driver self-tests |
| `rslib/src/notes/mod.rs` | `remove_notes` skips ids with no stored note |
| `pylib/anki/utils.py` | `to_json_bytes` encodes the config value from the Python value, so a float stays a float |

Probed in the oracle image (`driver/tests/test_records.py` and one-off runs):
a statement list (`SELECT 1; SELECT 2`), an empty statement and a comment-only
statement are `DBError`; a trailing `;` or comment runs; a REAL that is not
finite (`9e999`) arrives as null, because the backend's JSON has no infinity.
