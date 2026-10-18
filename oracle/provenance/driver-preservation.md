# Oracle driver: adopt and the raw db_dump (U36)

Read at tag 26.09.3 to check that `adopt` (`driver/ops/preservation.py`) and
`db_dump {raw: true}` (`driver/observe/db_dump.py`) leave the collection as it
is; no code was copied. Their meanings are `specs/conformance/normalized-dump.md`
N4 and N5; `db_dump`'s other sources are in `driver-sync.md`.

| Anki file | Read for |
|---|---|
| `pylib/anki/dbproxy.py` | `col.db.list` sends one query and no begin or commit request |
| `rslib/src/backend/dbproxy.rs` | a statement that starts with `select` neither sets the modified flag nor clears the undo history and study queues, so `adopt`'s two id reads change no stamp, `col.mod` or undo entry |

Probed through workers (`driver/tests/test_preservation.py`): raw dumps and
`sqlite_master` are equal before and after two `adopt` steps and after a
reopen; a card that a note edit adds gets the next `#cN` from a later `adopt`.
The raw form is `dump(col, raw=True)`, the function `converge` already uses
(sync-ops.md S7.2), restricted to the requested tables.
