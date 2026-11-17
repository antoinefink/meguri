# Storage specs: open, schema 18, upgrade, storage ops (U22)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/storage/schema-18.md`,
`specs/storage/open-and-connections.md`, `specs/storage/schema-upgrade.md` and
`specs/conformance/storage-ops.md`. No code was copied; the rules were
re-derived in the spec writer's words and checked by black-box probes of the
pinned `anki` wheel in the oracle image (new-collection objects, refusals of
versions 10/12/13/19 with header bytes before and after, upgrade of a crafted
schema-11 file run three times, collation order, read-only and immutable
opens of WAL and hot-journal files, `VACUUM INTO` and `query_only`).

| Spec section | Anki file | Read for |
|---|---|---|
| schema-18 S1, S2, S3 | `rslib/src/storage/schema11.sql`, `rslib/src/storage/upgrades/schema14_upgrade.sql`, `schema15_upgrade.sql`, `schema17_upgrade.sql`, `schema18_upgrade.sql` | table and index definitions; the stored text was taken from a probe of `sqlite_master`, normalized |
| schema-18 S4 | `rslib/src/storage/sqlite.rs`, `Cargo.lock` (`unicase` 2.6.0) | the collation every connection registers |
| schema-18 S5 | `rslib/src/storage/upgrades/schema15_upgrade.sql`, `rslib/src/storage/sqlite.rs` (create path, optimize) | where `ANALYZE` runs |
| open C1, C2.2 | `rslib/src/storage/sqlite.rs`, `rslib/src/storage/upgrades/mod.rs`, `rslib/src/error/db.rs`, `rslib/src/config/mod.rs`, `pylib/anki/collection.py` | accepted and refused versions and their kinds; how the scheduler version is read and defaulted |
| open C3 | `rslib/src/storage/sqlite.rs` | Anki sets journal mode before checking the version (probe confirmed the header change) |
| open C4, C9.1 | `rslib/src/storage/sqlite.rs` | Anki's pragmas (exclusive locking, WAL, busy timeout 0, page size, cache size, statement cache) and that it sets none of `synchronous`, `fullfsync`, `checkpoint_fullfsync` (re-read at 26.09.3 for the C9 decision, spec/U24), so it runs with SQLite's defaults |
| open C7, C10 | `rslib/src/storage/sqlite.rs`, `rslib/src/collection/mod.rs`, `rslib/src/backend/collection.rs` | Anki's TRUNCATE checkpoint and its close |
| open C2.6 | `rslib/src/collection/mod.rs`, `rslib/src/sync/collection/upload.rs`, `download.rs` | integrity checks only on full-sync staging files |
| upgrade U1, U3 | `rslib/src/storage/upgrades/mod.rs`, `rslib/src/storage/sqlite.rs` | step order, one exclusive transaction for the whole upgrade |
| upgrade U4 | `rslib/src/storage/deckconfig/mod.rs` (`add.sql`), `rslib/src/storage/config/mod.rs` (`add.sql`), `rslib/src/storage/tag/mod.rs`, `Cargo.lock` (`serde_json` without `preserve_order`) | presets, config and tags moved out of `col`; key taken over the inner id; insert-or-ignore of tags; compact JSON with sorted members |
| upgrade U5, U6 | `rslib/src/storage/notetype/mod.rs`, `rslib/src/notetype/mod.rs`, `rslib/src/notetype/schema11.rs`, `rslib/src/text.rs` | id-0 replacement, NFC names, `+` and `_` suffixes, row ords from list positions, template stamps 0, member name versus inner id |
| upgrade U7 | `rslib/src/storage/deck/mod.rs`, `rslib/src/decks/name.rs`, `rslib/src/decks/schema11.rs`, `rslib/src/decks/mod.rs` | name component normalization, modified stamp on rename, `_` suffix, insert-or-replace by id, id 0 refused |
| upgrade U8, U13 | `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/storage/card/mod.rs` (`fix_low_ease.sql`), `rslib/src/deckconfig/schema11.rs` at 26.09.3, 2.1.33 and 2.1.34 | ease ×100 at 15, ÷100 and the 1.3 reset at 16, the card factor fix by `did`/`odid`, no clamping in that read |
| upgrade U9.1 | `rslib/src/storage/sync.rs` | client usn −1 |
| upgrade U11 | `rslib/src/storage/notetype/mod.rs`, `rslib/src/storage/deck/mod.rs`, `rslib/src/storage/tag/mod.rs`, `rslib/src/notetype/schema11.rs`, `rslib/src/deckconfig/schema11.rs` | hash-map iteration order and hash-map `other` serialization (probe: three runs gave three different outcomes) |
| upgrade U14, U15 | `rslib/src/storage/tag/mod.rs`, `rslib/src/tags/mod.rs`, `rslib/src/storage/upgrades/schema17_upgrade.sql`, `schema18_upgrade.sql` | tags rebuilt collapsed; graves deduplicated with insert-or-ignore |
| upgrade U12 (foreign objects), U14 and U15 restated as end states (2026-10-14) | `rslib/src/storage/upgrades/schema17_upgrade.sql`, `schema18_upgrade.sql`, `rslib/src/storage/tag/mod.rs`, `Cargo.toml` (rusqlite `bundled`); no `legacy_alter_table` anywhere in `rslib/` or `pylib/` | Anki drops `tags` (its indexes and triggers go with it) and renames `graves` to `graves_old` before dropping it; with SQLite's default `legacy_alter_table` off, the rename rewrites the views and triggers that read `graves` to name `graves_old` (checked with a SQLite 3.54 probe, pragma off), so Anki loses them. Meguri keeps them: a deviation no golden sees, asserted by `tests/upgrade.rs` |
| open C3, hot journal decided on a copy (2026-10-14) | none | Meguri-only procedure (R1479); Anki writes before it checks the version (row "open C3" above) |
| upgrade U16 (releases) | `rslib/src/storage/upgrades/mod.rs` and `rslib/src/storage/sqlite.rs` at tags 2.1.24 to 2.1.50, `pylib/anki/collection.py` at 2.1.26 to 2.1.44 | which releases write which schema; `close(downgrade=False)` keeps it |
| storage-ops P1 | `pylib/anki/collection.py` (`__init__`, `_load_scheduler`) | the Python layer's `creationOffset` write on open |
| storage-ops P2 | `rslib/src/backend/dbproxy.rs` | how the driver's SQL reaches Anki's connection |

The deck-config reserved field numbers of `proto/anki/deck_config.proto` at
26.09.3 were compared with `proto/backend.proto` at 2.1.33 to confirm that the
schema-16 rewrite of a schema-15 preset drops no field.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| Classification of h69f4adbd5970 (reopen stamps; refusal to create a collection at an unwritable place) as needs-op against storage-ops P1 | `pylib/tests/test_collection.py` (test_create_open) | The obligation to classify: col.mod unchanged after reopen, and refusal of an unwritable directory or a mode-0 file. No scenario written. |
| Classification of the per-deck studied-count rows h1586af627542, h5a31d6105048, hb30e802d8f2f as not observable | `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/revlog/studied_today_by_deck.sql`, `rslib/src/scheduler/fsrs/rescheduler.rs`, `rslib/src/scheduler/fsrs/memory_state.rs` | Whether the per-deck studied count exists at the tag and which callers use it (only the FSRS reschedule-on-change pass). |
| storage/ids-usn-mtime I4 (review-log rows with given ids) and the schema-18 new collection (harvest rows hf7664c5a108b, h294205b39ddd) | `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/upgrades/mod.rs`, `rslib/src/storage/sqlite.rs`, `rslib/src/sync/collection/chunks.rs` | Confirming that the behavior of the main-branch tests exists at the tag: insert-or-ignore of revlog rows, and new collections created at the maximum schema version 18 |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| storage/config-table C3, C5 and storage/schema-upgrade U9.2, U17 (N11 reading of config values) | `Cargo.lock` (serde_json 1.0.143 without float_roundtrip) | config.val and col.conf values are parsed by serde_json into a JSON value with its non-roundtrip float reading. The golden evidence is the meguriFixture row in storage/upgrade/from-11 (123456789012345678901 -> 1.2345678901234567e20) and in storage/downgrade/roundtrip (1.2345678901234567e20 -> 1.2345678901234568e20). |
