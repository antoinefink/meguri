# Storage records: ids, USN and mtimes, graves, config table (U23)

Spec-writer notes for `specs/storage/ids-usn-mtime.md`, `specs/storage/graves.md`,
`specs/storage/config-table.md` and `specs/conformance/record-ops.md`. Read at
tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. Black-box probes of the
config encoding ran in the oracle image (`meguri-oracle:26.9.3-*`) through the
`anki` Python API with no network.

| Spec section | Anki files read | Read for |
|---|---|---|
| ids-usn-mtime I1-I2 | `rslib/src/storage/note/add.sql`, `rslib/src/storage/note/mod.rs`, `rslib/src/storage/card/add_card.sql`, `rslib/src/storage/card/mod.rs`, `rslib/src/storage/deck/alloc_id.sql`, `rslib/src/storage/deck/mod.rs`, `rslib/src/storage/deckconfig/add.sql`, `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/storage/notetype/add_notetype.sql`, `rslib/src/storage/notetype/mod.rs`, `rslib/src/storage/revlog/add.sql`, `rslib/src/storage/revlog/mod.rs`, `rslib/src/revlog/mod.rs`, `rslib/src/revlog/undo.rs`, `rslib/src/scheduler/answering/mod.rs` | candidate id (clock ms, answer instant for review rows) and the collision rule per table |
| ids-usn-mtime I4 | `rslib/src/storage/revlog/add.sql`, `rslib/src/revlog/undo.rs` | review-log rows with a given id are skipped when the id exists |
| ids-usn-mtime S1 | `rslib/src/storage/upgrades/schema14_upgrade.sql`, `schema15_upgrade.sql`, `schema17_upgrade.sql`, `schema18_upgrade.sql` | which tables carry `usn` and a modification time |
| ids-usn-mtime S2-S3 | `rslib/src/card/mod.rs`, `rslib/src/notes/mod.rs`, `rslib/src/config/mod.rs`, `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/sync/collection/changes.rs` | local stamping (`usn` -1, seconds), unbury at rollover without stamping, marking sent rows |
| ids-usn-mtime S4 | `rslib/src/notes/mod.rs` (note comparison before update), `rslib/src/decks/addupdate.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/notetype/mod.rs`, `rslib/src/config/undo.rs` | which updates Anki skips when nothing changed |
| ids-usn-mtime S5 | `rslib/src/collection/transact.rs`, `rslib/src/collection/timestamps.rs`, `rslib/src/collection/undo.rs`, `rslib/src/storage/collection_timestamps.rs` | when `col.mod` moves, and its undo and redo |
| ids-usn-mtime S6 | `rslib/src/sync/collection/finish.rs` | `col.usn`, `ls` and `mod` after a sync |
| graves G1-G2 | `rslib/src/storage/graves/add.sql`, `rslib/src/storage/graves/mod.rs`, `rslib/src/storage/upgrades/schema18_upgrade.sql` | table layout, insert-or-ignore |
| graves G3 | `rslib/src/notes/mod.rs`, `rslib/src/notes/undo.rs`, `rslib/src/card/mod.rs`, `rslib/src/card/undo.rs`, `rslib/src/decks/remove.rs`, `rslib/src/decks/undo.rs`, `rslib/src/notetype/mod.rs` | graves per deletion kind and their order |
| graves G4-G5 | `rslib/src/sync/collection/graves.rs`, `rslib/src/storage/graves/mod.rs` | incoming graves, pending graves |
| graves G6 | `rslib/src/card/undo.rs`, `rslib/src/notes/undo.rs`, `rslib/src/decks/undo.rs` | undo of an added grave |
| config-table C1-C4 | `rslib/src/storage/config/mod.rs`, `rslib/src/storage/config/add.sql`, `rslib/src/storage/config/get.sql`, `rslib/src/config/mod.rs`, `rslib/src/config/undo.rs`, `rslib/src/backend/config.rs`, `pylib/anki/collection.py`, `pylib/anki/utils.py`; `Cargo.toml`, `Cargo.lock` (serde_json 1.0.143 with no `preserve_order` or `arbitrary_precision`, ryu 1.0.20) | storage class, typed reads and defaults, byte comparison on write, generic JSON path |
| config-table C5 | probes in the oracle image | member order, string escapes, float spelling thresholds, integer range, `-0` |
| config-table C1, C10 | `rslib/src/config/deck.rs`, `rslib/src/config/notetype.rs`, `rslib/src/notes/mod.rs` (keys written when adding a note), `rslib/src/decks/remove.rs`, `rslib/src/notetype/mod.rs` | key families and their removal |
| config-table C9 | `rslib/src/sync/collection/changes.rs`, `rslib/src/storage/config/mod.rs` | whole-table send and replace |
| record-ops W1 | `rslib/src/backend/dbproxy.rs`, `rslib/src/undo/mod.rs`, `pylib/anki/dbproxy.py` | raw SQL: undo and study queues cleared after a non-select statement, `col.mod` set only on an explicit commit request |
| record-ops W2-W4 | `pylib/anki/collection.py`, `rslib/src/notes/mod.rs`, `rslib/src/backend/config.rs` | the note update and removal the driver should call, the config API without an undo entry |

Driver hints for the oracle maintainer (U23 role work):

- `db_exec`: call the collection's raw query API with positional arguments and no begin or commit request; a commit request would move `col.mod` after any non-select statement.
- `edit_note`: load the note with the collection's note getter when the step runs, change fields and tags, and save with the collection's note update (undoable).
- `delete_notes`: the collection's note removal with the ids of the notes that still exist.
- `set_config`: the collection's config setter with its default (no undo entry); the value goes through Python's JSON encoder, so the scenario's floats stay floats.
