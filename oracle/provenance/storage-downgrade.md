# Storage: the schema-11 copy (U33)

Spec-writer notes for `specs/storage/schema11-downgrade.md` and the U33 parts
of `specs/conformance/storage-ops.md` (P1 file form, P4 to P8). Read at tag
26.09.3 (`refs/anki-26.09.3`); no code was copied. The scenario inputs were
checked against a scratch copy of `conformance/fixtures/storage/schema-18.anki2`
read with Python's sqlite3 (a casefold `unicase` collation registered), never
the fixture file itself.

| Spec section | Anki files read | Read for |
|---|---|---|
| schema11-downgrade D1, D3 | `rslib/src/storage/upgrades/mod.rs`, `rslib/src/storage/sqlite.rs` (`close`, `open_or_create`), `rslib/src/storage/mod.rs` (`SchemaVersion`), `rslib/src/collection/mod.rs` (`close`), `rslib/src/backend/collection.rs` (`close_collection`), `pylib/anki/collection.py` (`close`) | the steps of the downgrade in one exclusive transaction, the journal-mode switch after it, and that the backend's close logs a failed downgrade and returns normally |
| schema11-downgrade D3.1 (`conf`) | `rslib/src/storage/config/mod.rs` (all config read as JSON values and written back as one object) | values parsed and re-serialized (sorted members, serde_json number spelling), failure on unreadable values; serde_json 1.0.143's recursion limit (`Cargo.lock`, the crate's `de.rs` at that version) for the 128-level rule |
| schema11-downgrade D3.1 (`models`, `decks`, `dconf`, `tags`) | `rslib/src/storage/notetype/mod.rs`, `rslib/src/storage/deck/mod.rs`, `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/storage/tag/mod.rs`, `rslib/src/storage/tag/get.sql`, `rslib/src/tags/mod.rs` | what each column holds, keyed by id or tag name; presets read with the range fix; tag `collapsed`/`config` not carried |
| schema11-downgrade D3.2-D3.4, D4 | `rslib/src/storage/upgrades/schema18_downgrade.sql`, `schema11_downgrade.sql`, `schema11.sql`, `schema18_upgrade.sql`, `schema15_upgrade.sql` | removed tables and indexes, the old `graves` definition and column order, what stays |
| schema11-downgrade D5 | `rslib/src/deckconfig/mod.rs` (default preset values and the read-time range fix) | bounds, defaults, NaN handling, which fields are checked |
| schema11-downgrade D7 | `rslib/src/storage/upgrades/mod.rs`, `rslib/src/storage/config/mod.rs`, `rslib/src/storage/tag/mod.rs`, `rslib/src/tags/mod.rs` (`Tag::new`) | what the upgrade gives back: config stamps 0, tags collapsed |
| schema11-downgrade D8 (errors) | `rslib/src/error/mod.rs`, `rslib/src/error/db.rs` | JSON, protobuf and storage-class failures map to different error kinds |
| storage-ops P4 (oracle) | `rslib/src/import_export/package/colpkg/export.rs`, `rslib/src/backend/import_export.rs`, `rslib/src/import_export/package/meta.rs` | the legacy colpkg export closes with the downgrade and propagates its error; its `collection.anki21` entry is the downgraded file |

Driver hints for the oracle maintainer (U33 role work):

- `schema11_copy`: never `col.close(downgrade=True)` on the session collection (it downgrades the collection itself) and never rely on that call to raise: the backend's close logs a failed downgrade and returns, leaving the file at 18. One path that works: close the session collection without the downgrade, copy `collection.anki2` to scratch, open the copy as its own `Collection`, call its legacy collection-package export without media, take the `collection.anki21` entry out of the package (meta version 2, deflated, not zstd) as `<session dir>/<name>`, delete the scratch files, reopen the session collection. A JSON error kind from the export (read through the backend error kind, as `ops/model.py`'s probe does) is the rejection `invalid_json`; anything else is a run error. The scenarios observe `db_dump` and `schema_objects` before and after the step and need them equal.
- `open: {file}`: as `open: {fixture}` with the session file as the source; `unknown_ref` when the file is missing; never move or change the session file.
- `file_col`, `file_objects`, `file_query`: copy the session file to scratch and read it with Python's sqlite3 (with a `unicase` collation registered), as `package_col` reads a package entry; never through Anki. `file_col` parses JSON columns as `package_col` does; `file_objects` reuses `observe/schema.py`'s normalization; `file_query` writes values as `db_exec` does.

Port (spec writer, 2026-10-27): the spec, storage-ops P1's `file` form and
P4 to P8 were ported from `spec/U33` without the driver op. The driver for
these hints now lands with the scenarios under
`conformance/scenarios/storage/downgrade/`, their goldens and the
`dsl.md`/`observations.md` rows (oracle maintainer, below). Rechecked at 26.09.3 for the
port: `rslib/src/storage/upgrades/mod.rs` (`downgrade_to_schema_11`: one
transaction, the order of the steps), `schema18_downgrade.sql` and
`schema11_downgrade.sql` (the `graves` rename, the dropped tables and
indexes), `rslib/src/storage/config/mod.rs` (`downgrade_config_from_schema14`),
`rslib/src/deckconfig/mod.rs` (`ensure_deck_config_values_valid`: bounds as
in `specs/sched/presets.md` V2, which D5 now cites instead of its own table),
`Cargo.lock` (serde_json 1.0.143), and the schema-11 caller
(`rslib/src/import_export/package/colpkg/export.rs`). `rslib/src/sync/version.rs`
is server-side only: the server sends schema 11 to clients older than sync
version 11. Meguri is a client at version 11 and uploads schema 18
(`sync/one-way` O6.2), so no transfer calls the downgrade.

Driver (oracle maintainer, U33; ported to `oracle/held-scenario-ops`,
2026-10-27): `ops/schema11.py` (the op and the three observations) and the
`open {file}` form in `ops/storage_open.py` follow the hints above. The port
parses `file_col`'s JSON columns in `ops/schema11.py` itself (`ops/package.py`
is not on this branch), with the same number rules.
Read at 26.09.3 for them: `pylib/anki/collection.py` (`close`,
`close_for_full_sync`, `export_collection_package`), the wheel's
`_backend_generated.py` (`export_collection_package`), `rslib/src/backend/import_export.rs`,
`rslib/src/import_export/package/colpkg/export.rs` and `meta.rs` (the export
closes with the downgrade and propagates its error; legacy meta version 2,
`collection.anki21` deflated), `rslib/src/storage/sqlite.rs` (`close`),
`rslib/src/storage/upgrades/mod.rs`, `rslib/src/storage/config/mod.rs`
(`get_all_config`) and `rslib/src/error/mod.rs` (a serde_json failure is the
JSON error kind). Session file names are lower case only (storage-ops P8.1),
so no name reaches `collection.anki2` on a case-insensitive volume.
