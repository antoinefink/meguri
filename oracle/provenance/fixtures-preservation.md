# Preservation fixture: the maker in `oracle/fixtures/preservation/` (U36)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to make
`conformance/fixtures/preservation/kitchen-sink.anki2` as
`specs/conformance/normalized-dump.md` N9.2 says; no code was copied. The
recipe's own sources are in `preservation.md`.

| Anki file | Read for |
|---|---|
| `rslib/src/storage/sqlite.rs` | a schema-18 collection opens in WAL mode with exclusive locking; closing without the downgrade keeps WAL (only a downgrade switches the journal mode) |
| `pylib/anki/collection.py` | `close(downgrade=False)`; `Collection(path)` as the session opens it |
| `pylib/anki/dbproxy.py`, `rslib/src/backend/dbproxy.rs` | `pragma secure_delete` through `col.db` runs on Anki's own connection; as a statement that is not a `select` it sets the modified flag, which writes `col.mod` only on a commit request (none is sent: no `transact` in pylib or the driver) |
| `rslib/src/dbcheck.rs` | Check Database clears the tags table, registers the notes' tags again at the collection's usn (-1 on a client) and restores the expanded ones: a synced collection's tags always change usn |

Black-box findings (on the made file and one-off runs in the oracle image):

- Closing leaves no `-wal`, `-shm` or `-journal` file; the header says WAL;
  integrity check `ok`; one free page, zeroed.
- The recipe run with secure_delete observes, after a fresh open, exactly the
  recipe golden's last step. Run without it, the stock check finds pieces of
  17 stock texts (every stock note type's CSS and most templates).
- Check Database at 2030-01-15T10:00Z: K5 counts all 0; it changes `col.mod`,
  `nextPos` (value, usn, mtime) and every tag's usn (9 to -1), nothing else.
