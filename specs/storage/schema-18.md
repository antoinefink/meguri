# Collection file at schema 18

Spec: storage/schema-18
Requirements: R1449, R1451, R2016
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/open/, conformance/scenarios/storage/upgrade/

What a collection file at schema 18 contains: the SQLite file properties, the twelve tables and sixteen indexes with the exact text SQLite stores for them, the statistics tables Anki's own files carry, and the expected-object check that keeps Meguri from adding anything (R2016). Opening and refusing files is `storage/open-and-connections`; upgrading older files is `storage/schema-upgrade`. Row values (protobuf blobs, legacy JSON) are the model area's (`specs/model/`, U21).

## S1. The file

- One SQLite 3 database file, named `collection.anki2` inside Meguri's collection directory (KTD12). All collection data lives in it; there is no second database for collection data (derived data lives in `meguri-derived.db`, U26).
- The schema version is the integer in `col.ver`, the only row of table `col` (`id` 1). SQLite's `user_version` and `application_id` stay 0 and mean nothing.
- Header facts of the files Anki 26.09.3 writes, which a file Meguri creates (U28 bootstrap) also has: page size 4096, text encoding UTF-8, schema format 4, auto-vacuum off. While the collection is open in Meguri, and after Meguri closes it, the file is in WAL mode (header bytes 18 and 19 are both 2).
- An existing file keeps its page size. Setting `page_size` on an open WAL-mode file changes nothing, and Meguri never leaves WAL mode to change it.

## S2. Tables

The table definitions, in the normalized form of `specs/conformance/storage-ops.md` P3.2 (SQL comments removed, each run of whitespace one space). This is the text `sqlite_master.sql` holds, after that normalization, for every table that Anki 26.09.3 or Meguri creates. Use exactly these tokens, with the same letter case; layout (line breaks, indentation, comments) is free.

| Table | Normalized `sqlite_master.sql` |
|---|---|
| `col` | `CREATE TABLE col ( id integer PRIMARY KEY, crt integer NOT NULL, mod integer NOT NULL, scm integer NOT NULL, ver integer NOT NULL, dty integer NOT NULL, usn integer NOT NULL, ls integer NOT NULL, conf text NOT NULL, models text NOT NULL, decks text NOT NULL, dconf text NOT NULL, tags text NOT NULL )` |
| `notes` | `CREATE TABLE notes ( id integer PRIMARY KEY, guid text NOT NULL, mid integer NOT NULL, mod integer NOT NULL, usn integer NOT NULL, tags text NOT NULL, flds text NOT NULL, sfld integer NOT NULL, csum integer NOT NULL, flags integer NOT NULL, data text NOT NULL )` |
| `cards` | `CREATE TABLE cards ( id integer PRIMARY KEY, nid integer NOT NULL, did integer NOT NULL, ord integer NOT NULL, mod integer NOT NULL, usn integer NOT NULL, type integer NOT NULL, queue integer NOT NULL, due integer NOT NULL, ivl integer NOT NULL, factor integer NOT NULL, reps integer NOT NULL, lapses integer NOT NULL, left integer NOT NULL, odue integer NOT NULL, odid integer NOT NULL, flags integer NOT NULL, data text NOT NULL )` |
| `revlog` | `CREATE TABLE revlog ( id integer PRIMARY KEY, cid integer NOT NULL, usn integer NOT NULL, ease integer NOT NULL, ivl integer NOT NULL, lastIvl integer NOT NULL, factor integer NOT NULL, time integer NOT NULL, type integer NOT NULL )` |
| `graves` | `CREATE TABLE graves ( oid integer NOT NULL, type integer NOT NULL, usn integer NOT NULL, PRIMARY KEY (oid, type) ) WITHOUT ROWID` |
| `deck_config` | `CREATE TABLE deck_config ( id integer PRIMARY KEY NOT NULL, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL )` |
| `config` | `CREATE TABLE config ( KEY text NOT NULL PRIMARY KEY, usn integer NOT NULL, mtime_secs integer NOT NULL, val blob NOT NULL ) without rowid` |
| `tags` | `CREATE TABLE tags ( tag text NOT NULL PRIMARY KEY COLLATE unicase, usn integer NOT NULL, collapsed boolean NOT NULL, config blob NULL ) without rowid` |
| `fields` | `CREATE TABLE fields ( ntid integer NOT NULL, ord integer NOT NULL, name text NOT NULL COLLATE unicase, config blob NOT NULL, PRIMARY KEY (ntid, ord) ) without rowid` |
| `templates` | `CREATE TABLE templates ( ntid integer NOT NULL, ord integer NOT NULL, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL, PRIMARY KEY (ntid, ord) ) without rowid` |
| `notetypes` | `CREATE TABLE notetypes ( id integer NOT NULL PRIMARY KEY, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL )` |
| `decks` | `CREATE TABLE decks ( id integer PRIMARY KEY NOT NULL, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, common blob NOT NULL, kind blob NOT NULL )` |

Notes for implementers:

- `notes.sfld` is declared `integer` on purpose: a sort field that looks like a number is stored with INTEGER affinity and sorts numerically (`text/checksum-and-sort-field`).
- In `col`, `notes`, `cards`, `revlog`, `deck_config`, `decks` and `notetypes` the `id` column is the rowid. `graves`, `config`, `tags`, `fields` and `templates` are WITHOUT ROWID tables; none of the twelve tables gets an automatic index.
- At schema 18 the `col` columns `conf`, `models`, `decks`, `dconf` and `tags` hold the empty string. Their data lives in the `config`, `notetypes`/`fields`/`templates`, `decks`, `deck_config` and `tags` tables.
- `tags.collapsed` holds the INTEGER 0 or 1, never text.
- Tables Anki created with older releases keep the text those releases stored (other spacing or letter case). Meguri never rewrites a stored definition.

## S3. Indexes

| Index | Table | Normalized `sqlite_master.sql` |
|---|---|---|
| `ix_notes_usn` | `notes` | `CREATE INDEX ix_notes_usn ON notes (usn)` |
| `ix_cards_usn` | `cards` | `CREATE INDEX ix_cards_usn ON cards (usn)` |
| `ix_revlog_usn` | `revlog` | `CREATE INDEX ix_revlog_usn ON revlog (usn)` |
| `ix_cards_nid` | `cards` | `CREATE INDEX ix_cards_nid ON cards (nid)` |
| `ix_cards_sched` | `cards` | `CREATE INDEX ix_cards_sched ON cards (did, queue, due)` |
| `ix_revlog_cid` | `revlog` | `CREATE INDEX ix_revlog_cid ON revlog (cid)` |
| `ix_notes_csum` | `notes` | `CREATE INDEX ix_notes_csum ON notes (csum)` |
| `idx_fields_name_ntid` | `fields` | `CREATE UNIQUE INDEX idx_fields_name_ntid ON fields (name, ntid)` |
| `idx_templates_name_ntid` | `templates` | `CREATE UNIQUE INDEX idx_templates_name_ntid ON templates (name, ntid)` |
| `idx_templates_usn` | `templates` | `CREATE INDEX idx_templates_usn ON templates (usn)` |
| `idx_notetypes_name` | `notetypes` | `CREATE UNIQUE INDEX idx_notetypes_name ON notetypes (name)` |
| `idx_notetypes_usn` | `notetypes` | `CREATE INDEX idx_notetypes_usn ON notetypes (usn)` |
| `idx_decks_name` | `decks` | `CREATE UNIQUE INDEX idx_decks_name ON decks (name)` |
| `idx_notes_mid` | `notes` | `CREATE INDEX idx_notes_mid ON notes (mid)` |
| `idx_cards_odid` | `cards` | `CREATE INDEX idx_cards_odid ON cards (odid) WHERE odid != 0` |
| `idx_graves_pending` | `graves` | `CREATE INDEX idx_graves_pending ON graves (usn)` |

The unique indexes on `name` take the column's `unicase` collation, so two deck, note type, field (per note type) or template (per note type) names that are equal under S4 cannot both be stored; neither can two such tags (the `tags` primary key). An insert that would break one fails with SQLite's unique-constraint error (extended code 2067) and stores nothing (scenario `storage/open/collation`).

## S4. Collation

- Every connection to the file, Meguri's and any other client's, must provide the collation `unicase`; SQLite cannot use the indexes of S3 or the `tags` key without it. Its comparison is `text/unicode-primitives#U2` (full case folding of `unicase` 2.6.0: `Straße` equals `STRASSE`, `ä` equals `Ä`, `Ω` equals `ω`).
- Columns with it: `deck_config.name`, `decks.name`, `notetypes.name`, `fields.name`, `templates.name`, `tags.tag`. Every other text comparison in the schema is SQLite's built-in `BINARY`.
- No schema object (table, index, view, trigger) Meguri creates may use any other collation or any application-defined SQL function. Functions Meguri registers for queries (`storage/open-and-connections` C5) are only ever called from statements, never stored in the file, or Anki and the sync server could no longer open it.
- Order through the collation, ties broken by plain code-point order (`ORDER BY x COLLATE unicase, x`): `_x`, `b`, `STRASSE`, `Straße`, `Z`, `Ä`, `ä`, `Ω`, `ω` (golden: `storage/open/collation`).

## S5. Statistics tables

- Files Anki writes also hold SQLite's statistics tables `sqlite_stat1` and `sqlite_stat4`, made by `ANALYZE`. Anki runs `ANALYZE` in the step to schema 15 (`storage/schema-upgrade#U5`), which also runs whenever Anki creates a collection, and in its optimize (`VACUUM`, `REINDEX`, `ANALYZE`), which Check Database and the preparation of a full upload run. Their rows are SQLite's own and are never compared.
- Meguri runs `ANALYZE` at the same points: in the schema-15 upgrade step, when it creates a collection (U28), and in optimize (U128, U132, and the full-upload preparation of U109). Nowhere else. The bundled SQLite is built with STAT4, so `ANALYZE` creates both tables, as Anki's does.
- Normalized text: `CREATE TABLE sqlite_stat1(tbl,idx,stat)` and `CREATE TABLE sqlite_stat4(tbl,idx,neq,nlt,ndlt,sample)` (SQLite writes it).

## S6. Expected objects and the schema guard (R2016)

The **required set** is the twelve tables of S2 and the sixteen indexes of S3, each with its table (`tbl_name`). **SQLite's own objects** are `sqlite_stat1` to `sqlite_stat4`, `sqlite_sequence` and indexes named `sqlite_autoindex_*`. Every other object in `sqlite_master` is **foreign** (an add-on's table, a view, a trigger).

### S6.1 Expected-object check

A function of the open gate (`storage/open-and-connections` C2.5) that reads `sqlite_master` (no table scans) and returns:

- `missing_tables`: required tables that are absent. Not empty: the file is refused with `NotACollection`.
- `missing_indexes`: required indexes that are absent. The open still succeeds (Anki never checks its indexes and opens such a file); the list goes to the diagnostics that Check Database reports (U128). Meguri does not recreate them on open.
- `foreign`: the foreign objects. They are kept as they are: Meguri never drops, renames or rewrites them. Their presence never refuses a file.

Objects are compared by type and name (and table, for indexes), never by their SQL text, because older clients stored other spacing and letter case.

### S6.2 The guard

After any Meguri operation on a collection (open, upgrade, close, any op, sync, import, maintenance), the set of `(type, name)` in `sqlite_master` equals the set before the operation, plus only:

- the objects that an upgrade step of `storage/schema-upgrade` creates (and minus those it drops: the old `graves` and `tags` tables);
- `sqlite_stat1` and `sqlite_stat4`, when the operation is one of the `ANALYZE` points of S5.

For a file that Anki or Meguri created, the set is exactly the required set plus `sqlite_stat1` and `sqlite_stat4`. Meguri's own temporary objects (TEMP tables of the writer, U23) live in SQLite's temp schema, never in `sqlite_master` of the file. The conformance runner asserts the guard after every scenario (U4), and `tests/schema_objects.rs` asserts it after ten open/close cycles, an upgrade and a 1,000-operation run.

Scenarios: every storage scenario that ends with an open collection observes `schema_objects` (`specs/conformance/storage-ops.md` P3) after its last change; the refusal scenarios end with none.
