# Writing a schema-11 copy of a collection

Spec: storage/schema11-downgrade
Requirements: R1481, R2013; supports R1464, R1466
Anki: 26.09.3
Scenarios: none yet; `conformance/scenarios/storage/downgrade/` once the oracle driver has the op (D11)

What Meguri writes when a collection must reach an Anki release older than schema 18: a separate file at schema 11, with the note types, decks, presets, config and tags back in the JSON columns of `col`, the old `graves` layout, the schema-18 tables and indexes removed, and the rollback-journal mode older releases need (R1481). Its only caller is the legacy package export (U123: `.apkg` and `.colpkg` with "Support older Anki versions"), which adds no conversion of its own. Manual Anki transfers (KTD26) send the collection at schema 18 (`sync/one-way#O6.2` keeps `col.ver`), so they need no downgrade.

The copy must equal, value for value, what Anki 26.09.3's own downgrade to schema 11 makes of the same collection (goldens under `conformance/goldens/storage/downgrade/` once recorded, D11). The record → object conversion is the model area's (`model/schema11-json` S3 to S12, U21); this spec adds what the storage does around it. Meguri never downgrades the collection it has open, and never writes the source at all (R2013).

## D1. Entry point and guarantees

- **Source**: either the open collection, or a closed schema-18 file that is not open as a live collection (U123's staging file, built with the storage-only builder of U23). Its `col.ver` must be 18 (else `SourceNotSchema18`, D8).
- **Destination**: a file path. The call either leaves a complete schema-11 copy there (D3 to D5) or leaves the path as it was before the call. A file already at the path is replaced only by a complete, verified copy (D8).
- **The source is never written.** The downgrade reads it only through a read-only connection (D2): no write lock, no transaction on the writer, no checkpoint. The writer keeps committing while the copy is made; its commits after the snapshot are not in the copy. With no operation running, the source file and its `-wal` keep their bytes and `PRAGMA data_version` does not move.
- A destination that is the source file, or the source's `-wal`, `-shm` or `-journal`, compared after resolving symbolic links and relative parts, is refused before anything is written (`DestinationIsSource`). This guard keeps a caller's mistake from replacing the live collection with a schema-11 file.
- Meguri never opens the copy as its collection. (Tests and scenarios open it on purpose to check the round trip, D7.)

## D2. Snapshot

1. Take the snapshot with SQLite's `VACUUM INTO` into a new temporary file in the destination's directory, so that the final rename (D8) stays within one file system. It is the empty temporary file of `storage/durability-and-files#F4` (purpose `export`), which `VACUUM INTO` accepts as its target, so the launch cleanup removes a leftover (R1493).
   - From the open collection: on the maintenance reader (`storage/open-and-connections` C6); never on the writer, never on a `query_only` reader (where `VACUUM INTO` fails).
   - From a closed file: on a read-only connection to it, initialized as every connection is (C5: `unicase` first).
2. The snapshot is one committed state of the source, including frames that are still in its `-wal`. It carries every table and index with their rows, SQLite's statistics tables and the source's foreign objects (D9), and none of the free pages, so deleted rows of the source never reach the copy. `VACUUM INTO` may renumber the rowids of a foreign table that has no `INTEGER PRIMARY KEY`; every collection table either has one or has no rowid.
3. It needs about as much free space as the source file. The caller checks space before (`storage/durability-and-files#F5`, R2015); a disk that fills up during the call fails it with `InsufficientSpace` (D8).

Everything after this step works on the temporary file through one read-write connection initialized as in C5 (`unicase` registered, since the tables D3.2 removes use it), with no other connection open on it.

## D3. Converting the copy

The end state of the temporary file. How it is reached is free, as long as no step depends on the source and a failure leaves no destination (D8).

### D3.1 The `col` row

| Column | Value in the copy |
|---|---|
| `id`, `crt`, `mod`, `scm`, `dty`, `usn`, `ls` | unchanged |
| `ver` | 11 |
| `conf` | a JSON object with one member per `config` row (below) |
| `models` | `model/schema11-json` S12 output: one member per note type, its id in decimal → the S4 object of the note type with its fields and templates (S5, S6) |
| `decks` | S12 output: one member per deck, normal and filtered, its id → the S7 object (S8 or S9) |
| `dconf` | S12 output: one member per preset, its id → the S10 object of the preset **after the range checks of D5** |
| `tags` | S12 output: one member per `tags` row, the `tag` text → its `usn` |

- Member order: ids in ascending numeric order, tags in the order of the `tags` table (its key order under `unicase`), config keys in ascending byte order (S12). Anki writes the members of each column in no set order; conformance compares parsed values (`specs/conformance/observations.md` O7).
- The objects carry the rows' own `mod` and `usn` (`mtime_secs`, `usn`). The downgrade stamps nothing: no `usn`, modification time or `col.mod` changes.
- `conf` member values: each `config` row's `val`, read as `storage/config-table` C3 reads JSON and written in its compact form (C5, the same as `storage/schema-upgrade#U9.2`): no whitespace, object members sorted by name at every depth, a repeated member name keeping its last value, numbers and strings spelled as C5 says. So the stored text `{"b": 1, "a": 2.50, "c": [1E2, -0]}` becomes `{"a":2.5,"b":1,"c":[100.0,-0.0]}`. Unknown keys and their values are all carried (R1466); only their spelling changes, as Anki's does.
- A `config` row the downgrade cannot carry refuses the whole copy (`UnconvertibleConfig`, naming the key, D8): a `KEY` that is not TEXT holding valid UTF-8; a `val` that is not a BLOB; a `val` that is not exactly one JSON value per `model/json-numbers` N9, with nothing but JSON white space before or after it; a number in it beyond the 64-bit float range (N10); an array or object nested 128 levels deep or more, counting the outermost as level 1 (127 levels are carried). Anki's downgrade fails on each of these. The 128-level limit is one level stricter than the config table's typed reader (U23), which accepts 128 levels; the downgrade uses this one.
- A note type, field, template, deck, preset or tag row that `model/row-records` R10 refuses (a blob that fails `model/protobuf-records` P9, a deck kind with neither member, a column of the wrong storage class) refuses the whole copy (`UnreadableRecord`, naming the table and the row's key, D8). Anki fails on these too. Rows of `notes`, `cards`, `revlog` and `graves` are never read as records; the downgrade copies them as SQLite values and never refuses because of their content.

### D3.2 Removed objects

These no longer exist in the copy:

- the tables `config`, `deck_config`, `tags`, `fields`, `templates`, `notetypes` and `decks`, with every index and trigger defined on them (their unique name indexes included);
- the indexes `idx_cards_odid` and `idx_notes_mid`. When the source lacks one of them (`storage/schema-18` S6.1 lets such a file open), there is nothing to remove and the copy goes on. (Anki's downgrade fails then; Meguri does not. No golden holds such a file.)
- the schema-18 `graves` table and its index `idx_graves_pending` (D3.3 replaces it).

SQLite removes the statistics rows of a dropped table or index with it; `sqlite_stat1` and `sqlite_stat4` themselves stay (D4).

### D3.3 `graves`

The copy's `graves` table is defined by

`CREATE TABLE graves ( usn integer NOT NULL, oid integer NOT NULL, type integer NOT NULL )`

in the normalized form of `specs/conformance/storage-ops.md` P3.2 (use exactly these tokens; layout is free). It has a rowid, no primary key and no index. It holds one row per row of the source's `graves`, with the same `usn`, `oid` and `type` values, inserted in ascending (`oid`, `type`) order. No scenario depends on that order; it only makes the copy deterministic.

Reach this state without renaming a table: keep the rows in SQLite's temp schema (qualified `temp.`, so no name of the file can collide), drop `graves`, create the new one and copy the rows back, as step 18 of the upgrade does the other way (`storage/schema-upgrade#U15`). Renaming would rewrite every view and trigger that names `graves` (D9).

### D3.4 Kept as they are

- The definitions and every row of `col` (apart from D3.1's values), `notes`, `cards` and `revlog`.
- The indexes `ix_notes_usn`, `ix_cards_usn`, `ix_revlog_usn`, `ix_cards_nid`, `ix_cards_sched`, `ix_revlog_cid` and `ix_notes_csum` (the ones the source has).
- `sqlite_stat1` and `sqlite_stat4` when the source has them. The downgrade runs no `ANALYZE`; their rows are SQLite's and never compared.
- Foreign objects, as D9 says.

### D3.5 Journal mode

The copy is in rollback-journal mode: `PRAGMA journal_mode` reads `delete`, the file header's bytes 18 and 19 are both 1, and no `-wal`, `-shm` or `-journal` file is left next to it. Older Anki releases cannot open a WAL-mode file. `VACUUM INTO` already writes its output in that mode, even from a WAL-mode source (probed with SQLite 3.45): never switch the copy to WAL, and close the conversion's connection before the verification's own read-only open (D8), which checks the mode.

## D4. The objects of the copy

For a source that Anki or Meguri created (no foreign object), the copy's `sqlite_master`, normalized and sorted as `specs/conformance/storage-ops.md` P3 reports it, is exactly:

| Type | Name | Table | Normalized `sql` |
|---|---|---|---|
| index | `ix_cards_nid` | `cards` | `CREATE INDEX ix_cards_nid ON cards (nid)` |
| index | `ix_cards_sched` | `cards` | `CREATE INDEX ix_cards_sched ON cards (did, queue, due)` |
| index | `ix_cards_usn` | `cards` | `CREATE INDEX ix_cards_usn ON cards (usn)` |
| index | `ix_notes_csum` | `notes` | `CREATE INDEX ix_notes_csum ON notes (csum)` |
| index | `ix_notes_usn` | `notes` | `CREATE INDEX ix_notes_usn ON notes (usn)` |
| index | `ix_revlog_cid` | `revlog` | `CREATE INDEX ix_revlog_cid ON revlog (cid)` |
| index | `ix_revlog_usn` | `revlog` | `CREATE INDEX ix_revlog_usn ON revlog (usn)` |
| table | `cards` | `cards` | as in the source (`storage/schema-18` S2) |
| table | `col` | `col` | as in the source |
| table | `graves` | `graves` | `CREATE TABLE graves ( usn integer NOT NULL, oid integer NOT NULL, type integer NOT NULL )` |
| table | `notes` | `notes` | as in the source |
| table | `revlog` | `revlog` | as in the source |
| table | `sqlite_stat1` | `sqlite_stat1` | `CREATE TABLE sqlite_stat1(tbl,idx,stat)` |
| table | `sqlite_stat4` | `sqlite_stat4` | `CREATE TABLE sqlite_stat4(tbl,idx,neq,nlt,ndlt,sample)` |

The schema-18 guard (`storage/schema-18` S6.2, R2016) is about the open collection; the copy is checked against this list instead (D8).

## D5. Preset values written

Anki applies the read bounds of `sched/presets#V3` whenever it reads a preset, and its downgrade writes what that read gives: a bounded field (the "Read bounds" column of `sched/presets#V2`) whose value is outside its bounds, or NaN, is written as its built-in default. The stored preset is not changed. The schema-11 output of S10 then converts the checked record.

- The bounds, defaults and float comparison are V3's: inclusive bounds, 32-bit floats compared with the 32-bit float nearest to each bound. A stored 1.31 is kept; 1.3 becomes 2.5.
- No other field is checked: learning steps, FSRS parameter lists, Easy Days percentages and the enums go out as stored.
- The downgrade uses the one preset read of V3 and keeps no bounds of its own.

Examples (scenario `storage/downgrade/contents`, D11): a preset storing new cards per day 10000, starting ease 1.3 and a NaN hard interval is written with `"perDay":20`, `"initialFactor":2500` and `"hardFactor":1.2`; one storing 9999, 1.31 and 0.5 is written with those values.

## D6. What a schema-11 copy cannot carry

Schema 11 has no place for these; they are lost in the copy, as in Anki's:

- unknown protobuf fields of every note type, field, template, deck and preset blob (`model/protobuf-records` P4; they have no JSON form);
- the `collapsed` and `config` columns of `tags`;
- the `usn` and `mtime_secs` of each config key (schema 11 keeps neither), and the stored spelling of config values (D3.1);
- preset values outside D5's bounds;
- what S4 to S10 output does not write: template `mtime_secs` and `usn`, `other` members under a reserved key or a key the object already writes (S3), and the spelling and member order of `other` values (Anki parses and rewrites them; Meguri keeps the stored text, `model/json-numbers` N7);
- a deck's desired-retention override beyond whole percent (S8: 0.875 is written as 87), and a preset's starting ease beyond thousandths (S10.1: 1.3105 is written as 1310);
- `idx_cards_odid`, `idx_notes_mid`, `idx_graves_pending` and the primary key of `graves`.

Everything else of the collection is in the copy: every note, card, review-log row and grave, every note type, deck and preset with its `other` members (R1464), every config key with its value (R1466), and every tag with its `usn`.

## D7. What upgrading the copy gives back

Opening the copy with the upgrade of `storage/schema-upgrade` (Meguri's or Anki's) gives a schema-18 collection equal to the source except for:

| Table | Difference from the source |
|---|---|
| `config` | every row `usn` 0 and `mtime_secs` 0; `val` in compact form (step 14) |
| `tags` | `collapsed` 1 and `config` NULL on every row (step 17); `usn` kept |
| `templates` | `mtime_secs` 0 and `usn` 0 |
| `notetypes`, `fields`, `templates`, `decks`, `deck_config` blobs | encoded again from the objects (`model/protobuf-records` P3): unknown fields gone, `other` rebuilt from the object's extra members (S3 input), preset values replaced per D5; the starting ease goes through S10.1 (×1000, truncated) and the upgrade's ×100 and ÷100 in 32-bit float arithmetic, so its last bits can move (1.31, 2.3 and 2.5 come back bit for bit; 1.311 comes back as the next float up) |
| `sqlite_stat1`, `sqlite_stat4` | rebuilt by the upgrade's `ANALYZE` (never compared) |

`col`, `notes`, `cards`, `revlog` and `graves` come back unchanged, row for row, and so do the row columns of note types, decks and presets (names, ids, `mtime_secs`, `usn`). An upgrade is not a change (`storage/schema-upgrade#U12`): nothing is stamped and `col.mod` keeps its value. Scenario `storage/downgrade/roundtrip` shows both dumps; `tests/downgrade.rs` asserts that the differences are exactly these.

## D8. Verification, failure and errors

Before the copy reaches the destination, open the temporary file again with a read-only connection and check:

1. `PRAGMA integrity_check` returns the single row `ok`;
2. `col` has one row, its `ver` is 11, and each of `conf`, `models`, `decks`, `dconf` and `tags` parses as a JSON object;
3. its objects are those of D4 (D9 for foreign objects), compared by type, name and table, with `graves` holding D3.3's definition;
4. `PRAGMA journal_mode` reads `delete`.

These checks are the caller verification of the atomic file of `storage/durability-and-files#F4`, and its sequence applies unchanged: full sync of the file, these checks with every handle closed before the rename, rename over the destination, full directory sync.

On any failure, cancellation (D10) or crash, no destination file appears and an existing one keeps its bytes; the temporary file is removed (after a crash, by the launch cleanup); the source is unchanged.

| Error | When | Data |
|---|---|---|
| `SourceNotSchema18` | the source's `col.ver` is not 18 | version found |
| `DestinationIsSource` | D1 | |
| `UnconvertibleConfig` | D3.1 | the key, and the reason: not text, not a BLOB, not JSON, out of range, too deep |
| `UnreadableRecord` | D3.1 | table, row key, reason (`model/row-records` R10, `model/protobuf-records` P9) |
| `InsufficientSpace` | the disk fills up (SQLite's full error, or no space on a write or rename) | |
| `Cancelled` | D10 | |
| `VerificationFailed` | a check of this section fails | which check |
| `Io` | any other SQLite or file-system failure | |

U123 shows the user a failed export; the downgrade has no UI of its own.

## D9. Foreign objects

Objects of the source that are not Anki's (`storage/schema-18` S6: an add-on's table, an index, a view, a trigger):

- A foreign table stays, with its rows (D2 for rowids).
- Foreign indexes and triggers defined on a table that D3.2 removes are removed with it, and so are those defined on the schema-18 `graves`: they go with the old table and are not created again. This is what Anki's downgrade does.
- Views, and triggers on kept tables, keep their stored text. Those that read `graves` read the new table (D3.3 renames nothing). Those that read a removed table stay and fail when used, as in Anki's copy.
- Every other foreign object stays unchanged.

So the copy holds D4's objects plus the source's foreign objects minus those on removed tables and on `graves`. The only difference from Anki: Anki renames `graves` before replacing it, so its copy's views and triggers that read `graves` name a table that no longer exists. No fixture holds a foreign object, so no golden shows this; `tests/downgrade.rs` asserts Meguri's result.

## D10. Fault points and cancellation

- Fault points (U21's `faultpoint!`; U250 owns the catalog): `storage.downgrade.after_snapshot` (after `VACUUM INTO`, before the conversion) and `storage.downgrade.before_rename` (after D8's checks, before the rename). Killed at either: no destination, the source unchanged, the temporary file present until the next launch's cleanup.
- Cancellation: the call takes U24's cancellation token and checks it after the snapshot, after the conversion and before the rename; it may also stop `VACUUM INTO` early through SQLite's progress handler. A cancel follows the failure path with `Cancelled`.

## D11. Scenarios and tests

The scenarios below are not in the suite yet: they wait for the oracle driver to carry `conformance/storage-ops` P1's `file` form and P4 to P7, and are ported with their goldens then. They start from the fixture `storage/schema-18.anki2` (`storage/schema-upgrade#U16`: no stock note type; note types `Pair` and `Gap`, decks `Default`, `Lang`, `Lang::Verbs` and the filtered `Cram`, presets `Default` and `Slow`, tags, review-log rows, graves and the config key `meguriFixture` stored as non-compact text). They add, with `db_exec`, one `other` member to the note type `Pair`, its field `Front`, its template `Forward`, the deck `Lang` and each preset, out-of-range values (D5) to the preset `Default`, values at their bounds to `Slow`, and a tag state schema 11 cannot hold. The ops and observations are `specs/conformance/storage-ops.md` P1, P4 to P7.

| Scenario | Covers | Golden |
|---|---|---|
| `storage/downgrade/contents` | D3 to D5: the copy's `col` row (objects with their `other` members, compact config values, preset range checks), objects, journal mode, integrity, graves, notes, cards and review log; the collection's own dump and objects unchanged by the copy | awaits the driver ops (oracle maintainer) |
| `storage/downgrade/roundtrip` | D7: the copy, opened (and so upgraded) in place of the collection, against the dump before the copy; `other` members survive the round trip in the blobs | awaits the driver ops |
| `storage/downgrade/oracle-opens` (live) | T1: Meguri writes the copy, the oracle reads it and opens it; its dump equals the golden of Anki's own copy and upgrade | awaits the driver ops |
| `storage/downgrade/config-invalid` | D3.1, D8: a config value with a number beyond the 64-bit float range, then one that is not JSON, refuse the copy (`invalid_json`) and leave the collection as it was; with an in-range value the copy succeeds | awaits the driver ops |

`core/crates/meguri-storage/tests/downgrade.rs` (Meguri only) covers what no scenario shows:

- the source file's SHA-256, its `-wal` size and `data_version` are unchanged after a copy made with no operation running; a copy taken while the writer commits holds the state of its start, and the writer's commits are never blocked;
- the destination equal to the source (and to its `-wal`) is refused with nothing written; an existing destination is replaced only by a verified copy, and keeps its bytes after a failure;
- a closed staging file as the source;
- kill at both fault points: no destination, source intact, the temporary file removed by the launch cleanup; cancellation at each check;
- `UnconvertibleConfig` for each reason (a TEXT value, a non-UTF-8 key, 128 nested levels refused and 127 carried), `UnreadableRecord` for a blob that fails P9 and for a deck kind with neither member;
- a source missing `idx_cards_odid` or `idx_notes_mid`;
- foreign objects (D9): a foreign table with rows, an index and a trigger on `notes` (kept), an index and a trigger on `graves` and on `decks` (removed), a view reading `graves` (reads the new table) and one reading `decks` (kept);
- D7's differences, and nothing else, between a source and the upgrade of its copy, over the fixture and over a collection built by the U11 generator.
