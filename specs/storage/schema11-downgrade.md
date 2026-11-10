# Writing a schema-11 copy of a collection

Spec: storage/schema11-downgrade
Requirements: R1481, R2013; supports R1464, R1466
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/downgrade/

What Meguri writes when a collection must reach an Anki release older than schema 18: a separate file at schema 11, with the note types, decks, presets, config and tags back in the JSON columns of `col`, the old `graves` layout, the schema-18 tables and indexes removed, and the rollback-journal mode older releases need (R1481). Its only callers are the legacy package exports with "Support older Anki versions" (U123 for `.apkg`, from a closed staging file; U124 for `.colpkg`, from the open collection; `package/legacy-export` L4, L7 and L8), which add no conversion of their own. Manual Anki transfers (KTD26) send the collection at schema 18 (`sync/one-way#O6.2` keeps `col.ver`), so they need no downgrade.

The copy must equal, value for value, what Anki 26.09.3's own downgrade to schema 11 makes of the same collection (goldens under `conformance/goldens/storage/downgrade/`, D11). The record → object conversion is the model area's (`model/schema11-json` S3 to S12, U21); this spec adds what the storage does around it. Meguri never downgrades the collection it has open, and never writes the source at all (R2013).

## D1. Entry point and guarantees

- **Source**: either the open collection (U124's `.colpkg` export), or a closed schema-18 file that is not open as a live collection (U123's staging file, built with the storage-only builder of U23). Its `col.ver` must be 18 (else `SourceNotSchema18`, D8).
- **Destination**: a file path, in a directory that the launch cleanup of `storage/durability-and-files#F4` covers: in practice a `.meguri-stage-*` staging directory (`package/container#P9`), as both callers use (`package/legacy-export` L4). The call accepts any other directory, but a temporary file left there by a crash is not removed by the next launch. The call either leaves a complete schema-11 copy there (D3 to D5) or leaves the path as it was before the call. A file already at the path is replaced only by a complete, verified copy (D8).
- A `-journal`, `-wal` or `-shm` file next to the destination path refuses the call with `Io` before anything is written, and again just before D8's rename. Decided 2026-10-27: SQLite would play a stale `-journal` of the replaced file back into the new copy at its next read-write open, and removing it is not safe while the file it belongs to may still need it. The callers write into fresh staging directories, where no such file exists.
- **The source is never written.** The downgrade reads it only through a read-only connection (D2): no write lock, no transaction on the writer, no checkpoint. The writer keeps committing while the copy is made; its commits after the snapshot are not in the copy. With no operation running, the source file and its `-wal` keep their bytes and `PRAGMA data_version` does not move.
- A destination that names the source file, or the source's `-wal`, `-shm` or `-journal`, is refused before anything is written (`DestinationIsSource`). This guard keeps a caller's mistake from replacing the live collection with a schema-11 file. Decided 2026-11-10: the test is by file identity, not by path text, because Meguri's volumes (APFS) are usually case-insensitive and a hard link gives a file a second name:
  - When the destination's parent directory, with symbolic links and relative parts resolved, is the source's parent directory so resolved, the destination's name is compared with the four names after NFC and full Unicode case folding, whatever the volume's own rules: `COLLECTION.anki2` and `collection.anki2-WAL` are refused next to `collection.anki2`, also on a case-sensitive volume.
  - When the destination exists, it is refused if it is the same file (device and inode) as the source or as one of its existing `-wal`, `-shm` or `-journal` files.
  - Both tests run again just before D8's rename.
- Meguri never opens the copy as its collection. (Tests and scenarios open it on purpose to check the round trip, D7.)

## D2. Snapshot

1. Take the snapshot with SQLite's `VACUUM INTO` in the destination's directory, so that the final rename (D8) stays within one file system. First create the temporary file of `storage/durability-and-files#F4` (purpose `export`). `VACUUM INTO` then writes to a name that does not exist yet: that temporary file's name with `-snapshot` appended, which is in the same `.meguri-tmp-export-*` namespace, so the launch cleanup removes a leftover of either (R1493). Right after `VACUUM INTO`, rename the snapshot over the F4 temporary file; D3 to D8 work on that file. When `VACUUM INTO` fails or is cancelled (D10), the call removes the partial `-snapshot` file and any `-journal`, `-wal` or `-shm` file next to it, then the F4 temporary file, before it returns. Decided 2026-10-27: SQLite releases differ on whether `VACUUM INTO` accepts an existing empty file as its target (3.45.3 and the bundled 3.53.2 do, 3.54.0 refuses it with "output file already exists"), so the target never exists before the call. A target that exists anyway fails the call with `Io`.
   - From the open collection: on the maintenance reader (`storage/open-and-connections` C6); never on the writer, never on a `query_only` reader (where `VACUUM INTO` fails).
   - From a closed file: on a connection opened with the URI parameters `mode=ro&immutable=1`, initialized as every connection is (C5: `unicase` first). No other connection exists on a closed file, so it needs no lock, and SQLite then creates no file next to it, even when the file is in WAL mode (a plain `mode=ro` open of a WAL-mode file creates `-wal` and `-shm` and cannot remove them; probed with SQLite 3.53 and 3.45.3). An immutable open does not read a `-wal`, so a `-wal` or `-journal` next to the closed file, checked before the open, fails the call with `Io` and nothing is written: such a file was not closed cleanly (`package/apkg-export#A4` closes it with neither).
2. The snapshot is one committed state of the source, including frames that are still in its `-wal`. It carries every table and index with their rows, SQLite's statistics tables and the source's foreign objects (D9), and none of the free pages, so deleted rows of the source never reach the copy. `VACUUM INTO` may renumber the rowids of a foreign table that has no `INTEGER PRIMARY KEY`; every collection table either has one or has no rowid.
3. It needs about as much free space as the source file. The caller checks space before (`storage/durability-and-files#F5`, R2015); a disk that fills up during the call fails it with `InsufficientSpace` (D8).
4. A foreign object that needs a collation or SQL function Meguri does not register (C5 registers `unicase` and Meguri's own functions only), for example an index declared with an add-on's collation, an expression index or a `CHECK` constraint that calls an add-on's function, makes `VACUUM INTO` fail, since it rebuilds every index and table: the call fails with `Io` and SQLite's message ("no such collation sequence: …", "no such function: …") and nothing is written. Meguri registers nothing for such objects (D12 `downgrade-foreign-function`; probed with SQLite 3.54.0).

Everything after this step works on the temporary file through one read-write connection initialized as in C5 (`unicase` registered, since the tables D3.2 removes use it), with triggers and foreign keys turned off (D3), `secure_delete` on (D3.2), and with no other connection open on it.

## D3. Converting the copy

The end state of the temporary file. How it is reached is free, as long as no step depends on the source and a failure leaves no destination (D8).

Decided 2026-10-27: no trigger fires during the conversion. The conversion connection runs with SQLite's triggers turned off (`SQLITE_DBCONFIG_ENABLE_TRIGGER` set to 0 before its first statement), so a foreign trigger on `col` or on another kept table neither changes the copy nor makes it fail, whatever the order of the steps. (`DROP TABLE` fires no trigger in any case.) Anki writes `col` with triggers on, seven times in its one transaction: once while it replaces `graves`, once for each of the five JSON columns, and a last time, setting `ver` to 11, after the tables and indexes of D3.2 are dropped. So a foreign trigger on `col` runs in Anki's copy, and one that reads a removed table fails at that last write, which makes Anki's whole downgrade fail (D12 `downgrade-triggers-off`).

Decided 2026-11-10: no foreign key acts during the conversion. Meguri's bundled SQLite, like Anki's, enforces foreign keys by default (`storage/open-and-connections` C4), and a foreign table may declare one that references a table D3.2 removes or `graves`. With enforcement on, dropping the referenced table first deletes its rows, which deletes or changes the referencing rows (`ON DELETE CASCADE`, `SET NULL`, `SET DEFAULT`) or fails (`NO ACTION`, `RESTRICT`, while such rows exist). The conversion connection therefore sets `PRAGMA foreign_keys` to 0 before its first transaction (the pragma has no effect inside one) and reads it back as 0 before going on, else `Io`. Foreign rows then stay exactly as the snapshot holds them (D9, D12 `downgrade-foreign-keys`).

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
- `conf` member values: each `config` row's `val`, read as `storage/config-table` C3 reads JSON and written in its compact form (C5, the same as `storage/schema-upgrade#U9.2`): no whitespace, object members sorted by name at every depth, a repeated member name keeping its last value, numbers and strings spelled as C5 says. So the stored text `{"b": 1, "a": 2.50, "c": [1E2, -0]}` becomes `{"a":2.5,"b":1,"c":[100.0,-0.0]}`. Unknown keys and their values are all carried (R1466); only their spelling changes, as Anki's does. This rule, not the stored-text rule that `model/schema11-json` S12 gives for `conf` values (N7), makes the `conf` column of the copy.
- A `config` row the downgrade cannot carry refuses the whole copy (`UnconvertibleConfig`, naming the key, D8): a `KEY` that is not TEXT holding valid UTF-8; a `val` that is not a BLOB; a `val` that is not exactly one JSON value per `model/json-numbers` N9, with nothing but JSON white space before or after it; a number in it beyond the 64-bit float range (N10); a value nested too deep (below). Anki's downgrade fails on each of these, except a value nested exactly 127 levels (below). D3.1 may read values with the config table's JSON parser (C3) only when it adds the depth check below and N9 and N10.
- Depth. Decided 2026-11-10: every JSON column of the copy stays within the nesting limit of Anki's schema-11 upgrade, which reads each column as one JSON text and fails on an array or object nested 128 levels deep or more, counting the column's own outer object as level 1 (the column inputs of `storage/schema-upgrade` U4 to U8). Meguri's upgrade must open every copy this call writes (D7). A value read alone counts its own outermost array or object as level 1; in its column it sits one level deeper, so:
  - a `conf` value nested 127 levels or more, counting its own outermost array or object as level 1, refuses the copy (`UnconvertibleConfig`, too deep); 126 levels are carried. (Anki parses each value alone with a 128-level limit, so it fails on 128 levels and writes 127, which its own upgrade then cannot read.)
  - a note type, field, template, deck or preset whose object would put an array or object at level 128 or deeper of its column refuses the copy (`UnreadableRecord`, the table, the row's key and reason `too deep`). In practice only `other` members (`model/schema11-json` S3) go that deep. Counting the `other` object itself as level 1, its contents sit one level deeper in `models`, `decks` and `dconf` for a note type, deck or preset (a preset's `new`, `rev` and `lapse` parts included, S10.5), and three levels deeper for a field or template (inside `flds` or `tmpls`). Anki writes these copies, which no upgrade opens, and drops an `other` of 128 levels or more whole (D12 `downgrade-deep-json`).
- Repeated member names. The copy's JSON columns repeat no member name at any depth. A `conf` value keeps the last of repeated names (above). In an `other` that repeats a top-level member name, only the last occurrence is written; an `other` member whose value repeats a member name at any depth is written in the compact form of `storage/config-table` C5, which keeps the last value; every other member keeps its stored text (N7). Anki's parse of `other` keeps the last occurrence too, so the parsed values agree.
- The records read are those Anki's downgrade reads: every `notetypes` row, each with the `fields` and `templates` rows whose `ntid` is its id, in `ord` order (`model/row-records` R7); every `decks` row; every `deck_config` row; and the `tag`, `usn` and `collapsed` columns of every `tags` row. A `fields` or `templates` row whose `ntid` matches no note type is neither read nor refused, and is lost in the copy (D6). The `config` column of `tags` is never read.
- A record read this way that `model/row-records` R10 refuses refuses the whole copy (`UnreadableRecord`, naming the table and the row's key, D8). Anki fails on the same records in these cases: a blob that fails `model/protobuf-records` P9, a deck kind with neither member, a column of the wrong storage class, a field or template `ord` outside the 32-bit unsigned range, and a TEXT name (`notetypes.name`, `fields.name`, `templates.name`, `decks.name`, `deck_config.name`, `tags.tag`) that is not valid UTF-8. A name is never converted lossily.
- Two columns differ from R1 for this read. A `usn` (of a note type, template, deck, preset or tag) is an INTEGER that must fit 32 bits signed under R1; Anki keeps only its low 32 bits instead (4294967297 becomes 1 in its copy). Meguri refuses such a row (`UnreadableRecord`, D12 `downgrade-usn-range`). `tags.collapsed` is read but never written: any INTEGER passes, 2 and −1 included, and only a value of another storage class refuses the copy (`UnreadableRecord`, table `tags`), as in Anki.
- A deck stored under id 0 refuses the copy (`UnreadableRecord`, table `decks`, key 0, reason `deck id 0`). Decided 2026-11-10: both upgrades refuse a deck with id 0 (`storage/schema-upgrade` U7 step 1, U10), so no release that reads schema 11 through them can open such a copy; Anki writes it anyway (D12 `downgrade-deck-id-0`). A note type under id 0 is carried; the upgrade gives it a new id (D7).
- The `col` row is not read as a record: apart from `ver` and the five JSON columns, its columns keep their SQLite values, whatever they hold, and never refuse the copy (D3.4). Rows of `notes`, `cards`, `revlog` and `graves` are never read as records either; the downgrade copies them as SQLite values and never refuses because of their content.

### D3.2 Removed objects

These no longer exist in the copy:

- the tables `config`, `deck_config`, `tags`, `fields`, `templates`, `notetypes` and `decks`, with every index and trigger defined on them (their unique name indexes included);
- the indexes `idx_cards_odid` and `idx_notes_mid`. When the source lacks one of them (`storage/schema-18` S6.1 lets such a file open), there is nothing to remove and the copy goes on. (Anki's downgrade fails then; Meguri does not, D12 `downgrade-missing-index`. No golden holds such a file.)
- the schema-18 `graves` table and its index `idx_graves_pending` (D3.3 replaces it).

SQLite removes the statistics rows of a dropped table or index with it; `sqlite_stat1` and `sqlite_stat4` themselves stay (D4).

Decided 2026-11-10: the conversion connection runs with `PRAGMA secure_delete` on, set before its first statement, so the pages these drops free hold zeros (probed with SQLite 3.54.0) and no byte of what D6 lists (unknown protobuf fields, tag states, config stamps) stays in the file. The copy's size is not reduced; no `VACUUM` runs after the conversion (it could renumber the rowids of a foreign table, D2). Anki's copy keeps those bytes in its free pages; no observation reads free pages, so no golden shows this.

### D3.3 `graves`

The copy's `graves` table is defined by

`CREATE TABLE graves ( usn integer NOT NULL, oid integer NOT NULL, type integer NOT NULL )`

in the normalized form of `specs/conformance/storage-ops.md` P3.2: the stored text must normalize to exactly that string. P3.2 only collapses runs of white space, so white space is required wherever the string has a space (after `(`, before `)`, after `graves`) and forbidden where it has none (before each `,`); each such run may be any white space. It has a rowid, no primary key and no index. It holds one row per row of the source's `graves`, with the same `usn`, `oid` and `type` values, inserted in ascending (`oid`, `type`) order. No scenario depends on that order; it only makes the copy deterministic.

Reach this state without renaming a table: keep the rows in SQLite's temp schema (qualified `temp.`, so no name of the file can collide), drop `graves`, create the new one and copy the rows back, as step 18 of the upgrade does the other way (`storage/schema-upgrade#U15`). Renaming would rewrite every view and trigger that names `graves` (D9).

### D3.4 Kept as they are

- The definitions and every row of `col` (apart from D3.1's values), `notes`, `cards` and `revlog`.
- The indexes `ix_notes_usn`, `ix_cards_usn`, `ix_revlog_usn`, `ix_cards_nid`, `ix_cards_sched`, `ix_revlog_cid` and `ix_notes_csum` (the ones the source has).
- `sqlite_stat1` and `sqlite_stat4` when the source has them. The downgrade runs no `ANALYZE`; their rows are SQLite's and never compared.
- Foreign objects, as D9 says.

### D3.5 Journal mode

The copy is in rollback-journal mode: `PRAGMA journal_mode` reads `delete`, the file header's bytes 18 and 19 are both 1, and no `-wal`, `-shm` or `-journal` file is left next to it. Older Anki releases cannot open a WAL-mode file. `VACUUM INTO` already writes its output in that mode, even from a WAL-mode source (probed with SQLite 3.45 and 3.54.0). The conversion connection still reads `PRAGMA journal_mode` before its first transaction and, when it does not read `delete`, sets `PRAGMA journal_mode=DELETE` and reads it back (else `Io`), so a SQLite release that writes the snapshot otherwise changes nothing. Never switch the copy to WAL, and close the conversion's connection before the verification's own read-only open (D8), which checks the mode.

## D4. The objects of the copy

For a complete source (every object of `storage/schema-18` S2, `sqlite_stat1` and `sqlite_stat4` present, no foreign object), the copy's `sqlite_master`, normalized and sorted as `specs/conformance/storage-ops.md` P3 reports it, is exactly:

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

A source that lacks one of these indexes or statistics tables gives a copy that lacks it too (D3.4); D8 check 3 derives the expected objects of any source from its snapshot. The schema-18 guard (`storage/schema-18` S6.2, R2016) is about the open collection; the copy is checked by D8 instead.

## D5. Preset values written

Anki applies the read bounds of `sched/presets#V3` whenever it reads a preset, and its downgrade writes what that read gives: a bounded field (the "Read bounds" column of `sched/presets#V2`) whose value is outside its bounds, or NaN, is written as its built-in default. The stored preset is not changed. The schema-11 output of S10 then converts the checked record.

- The bounds, defaults and float comparison are V3's: inclusive bounds, 32-bit floats compared with the 32-bit float nearest to each bound. A stored 1.31 is kept; 1.3 becomes 2.5.
- No other field is bounded here: learning and relearning steps, FSRS parameter lists, Easy Days percentages and the Auto Advance times go out as stored, apart from the refusal below. The enums get no range check here either; S10 maps the values it does not know (`answer_action` outside 1 to 4 and `question_action` other than 1 become 0, `leech_action` other than 1 becomes 0, an unknown new-card insert order becomes the default) and writes the others (review order, new mix, sort and gather orders, interday mix) as stored.
- The bounds compare as IEEE floats: −0.0 is not below a lower bound of 0.0, so a lapse multiplier of −0.0 is kept and written as `"mult":-0.0`. A total order that puts −0.0 below 0.0 is wrong here.
- The downgrade uses the one preset read of V3 (the model-level bounded read, `core/crates/meguri-model/src/deck_config.rs`) and keeps no bounds of its own.

Non-finite values. Decided 2026-10-27: when an unbounded `f32` of a preset (`learn_steps`, `relearn_steps`, `fsrs_params_4`, `fsrs_params_5`, `fsrs_params_6`, `easy_days_percentages`, `seconds_to_show_question` or `seconds_to_show_answer`, any element of a list) is NaN, +∞ or −∞, the whole copy is refused with `UnreadableRecord` (table `deck_config`, the preset id, reason `non-finite value` with the field's name, D8). S10 would write such a value as `null` (`model/json-numbers`), which no reader takes back: the strict readers of the Auto Advance times, the FSRS parameter lists and the Easy Days list refuse the whole `dconf` column, so neither Anki's schema-11 upgrade nor Meguri's (`storage/schema-upgrade`) can open the copy, and a `null` in a step list makes that list read back as something else (Anki reads it as empty). Anki writes that file anyway; Meguri refuses rather than publish a copy that older releases cannot open or that silently changes the steps (D12 `downgrade-non-finite-preset`). The stored preset is not changed; `package/legacy-export` L11 reports the refusal as `LegacyConversion` with the table and the preset id, and the user corrects that option of the preset and exports again.

The same holds for the two `f32` values of decks (`model/protobuf-records`; decided 2026-11-10): a NaN, +∞ or −∞ element of a filtered deck's `delays`, or a normal deck's desired-retention override that is NaN, +∞ or −∞, refuses the copy with `UnreadableRecord` (table `decks`, the deck id, reason `non-finite value` with the field's name). S9 would write such a delay as `null`, and the upgrade then reads the whole list as empty; S8's truncation would write a NaN retention as 0 and +∞ as 4294967295 (D12 `downgrade-non-finite-deck`). A finite retention is always written as S8 makes it, also when negative or huge (D6). The stored deck is not changed; L11 reports the refusal with the table and the deck id.

Examples (scenario `storage/downgrade/contents`, D11): a preset storing new cards per day 10000, starting ease 1.3 and a NaN hard interval is written with `"perDay":20`, `"initialFactor":2500` and `"hardFactor":1.2`; one storing 9999, 1.31 and 0.5 is written with those values.

## D6. What a schema-11 copy cannot carry

Schema 11 has no place for these; they are lost in the copy, as in Anki's:

- unknown protobuf fields of every note type, field, template, deck and preset blob (`model/protobuf-records` P4; they have no JSON form);
- the `collapsed` and `config` columns of `tags`;
- the `usn` and `mtime_secs` of each config key (schema 11 keeps neither), and the stored spelling of config values (D3.1);
- preset values outside D5's bounds;
- what S4 to S10 output does not write: template `mtime_secs` and `usn`, `other` members under a reserved key or a key the object already writes (S3), the whole of an `other` that is not a valid JSON object (S3 output rule 1: not JSON, not an object, or holding a number beyond the 64-bit float range), and the spelling and member order of `other` values (Anki parses and rewrites them; Meguri keeps the stored text, `model/json-numbers` N7);
- `fields` and `templates` rows whose `ntid` matches no note type (D3.1);
- what the truncations of S8 and S10.1 drop: they multiply in 32-bit float arithmetic and truncate the product, so a deck's desired-retention override keeps only whole percent, sometimes one less (0.875 is written as 87, 0.53 as 52 and 0.59 as 58; every whole percent from 0.70 to 0.99 is written as itself; a negative value as 0, and one whose product reaches 2^32, about 42949673, as 4294967295), and a preset's starting ease keeps only thousandths, sometimes one less (1.3105 is written as 1310, 2.001 as 2000; 35 of the thousandths from 1.310 to 5.000 lose one this way, 2.008, 2.011 and 4.002 among them);
- columns that an add-on added to the source's `graves`: the copy's `graves` has only `usn`, `oid` and `type` (D3.3), as Anki's has; D8 check 3 compares objects by type, name and table only, so it does not notice them;
- `idx_cards_odid`, `idx_notes_mid`, `idx_graves_pending` and the primary key of `graves`;
- the foreign indexes and triggers that D9 removes with the tables they are defined on.

Everything else of the collection is in the copy: every note, card, review-log row and grave, every note type, deck and preset with its `other` members (R1464; S3 output rule 1 excepted), every config key with its value (R1466), and every tag with its `usn`.

## D7. What upgrading the copy gives back

Opening the copy with the upgrade of `storage/schema-upgrade` (Meguri's or Anki's) gives a schema-18 collection equal to the source except for:

| Table | Difference from the source |
|---|---|
| `config` | every row `usn` 0 and `mtime_secs` 0; `val` in compact form (step 14) |
| `tags` | `collapsed` 1 and `config` NULL on every row (step 17); `usn` kept |
| `templates` | `mtime_secs` 0 and `usn` 0 |
| `notetypes`, `fields`, `templates`, `decks`, `deck_config` blobs | encoded again from the objects (`model/protobuf-records` P3): unknown fields gone, `other` rebuilt from the object's extra members (S3 input), preset values replaced per D5; the starting ease goes through S10.1 (×1000 in 32-bit float arithmetic, truncated) and the upgrade's ÷1000, ×100 and ÷100 in the same arithmetic, so it can come back one thousandth lower (2.001 as 2.0, D6) or with its last bits moved (1.311 as the next float up); 1.31, 2.3 and 2.5 come back bit for bit; a deck's desired-retention override comes back as the whole percent S8 wrote, divided by 100 (0.875 as 0.87, 0.53 as 0.52, a negative value as 0.0, one beyond the 32-bit range as 42949672.0) |
| `sqlite_stat1`, `sqlite_stat4` | created or rebuilt by the upgrade's `ANALYZE` (never compared) |
| `fields`, `templates` | rows whose `ntid` matched no note type in the source are absent (D3.1, D6); `ord` comes back as the position in the list from 0 (`storage/schema-upgrade` U6 step 6), so a note type whose fields or templates had gaps in `ord` (0 and 2) comes back renumbered (0 and 1), and a card whose `ord` named a template after a gap no longer matches that template |
| `notetypes`, `fields`, `templates` of a note type stored under id 0 | the note type comes back under a new random id (U6 step 2); its notes keep `mid` 0 and so have no note type in the upgraded collection |
| `idx_cards_odid`, `idx_notes_mid` | present, as `storage/schema-upgrade#U5` creates them, also when the source lacked them; the `ix_*` indexes a source lacked stay absent |
| foreign objects | the indexes and triggers that D9 removed are absent |
| names | a note type, field, template or deck name that `storage/schema-upgrade` U6 steps 3 to 5 or U7 steps 2 to 4 would change comes back changed, a deck so renamed with the stamp of U7 step 3 |

`col`, `notes`, `cards`, `revlog` and `graves` come back unchanged, row for row, and so do the row columns of note types, decks and presets (names, apart from the renames above, ids, apart from a note type's id 0, `mtime_secs`, `usn`). A deck stored under id 0 never reaches a copy (D3.1). An upgrade is not a change (`storage/schema-upgrade#U12`): nothing is stamped and `col.mod` keeps its value. Scenario `storage/downgrade/roundtrip` shows both dumps; `tests/downgrade.rs` asserts that the differences are exactly these.

## D8. Verification, failure and errors

Before the copy reaches the destination, open the temporary file again with a read-only connection initialized as in C5 (`unicase` registered first, since a kept foreign table or index may use it) and check:

1. `PRAGMA integrity_check` returns the single row `ok` (as `storage/durability-and-files#F5` `integrity_check(path)` checks it);
2. `col` has one row, its `ver` is 11, and each of `conf`, `models`, `decks`, `dconf` and `tags` parses as a JSON object (`model/json-numbers` N9 and N10) with no array or object nested 128 levels deep or more, counting the column's object as level 1 (D3.1);
3. its objects are the expected ones, compared by type, name and table, with `graves` holding D3.3's definition. The expected objects are those of the snapshot (read from the temporary file after D2, before the conversion), minus D3.2's removed objects and what goes with them (their indexes, triggers and `sqlite_autoindex_*` indexes), minus the schema-18 `graves` with its index, triggers and `sqlite_autoindex_*` index, plus D3.3's `graves`. So each `ix_*` index, each `sqlite_stat*` table and every other object SQLite makes itself (`sqlite_sequence`, the `sqlite_autoindex_*` indexes of a kept table) is expected exactly when the source has it, and nothing the source lacks is expected. For a complete source this is D4's list; foreign objects follow D9;
4. `PRAGMA journal_mode` reads `delete`.

These checks are the caller verification of the atomic file of `storage/durability-and-files#F4`, and its sequence applies unchanged: full sync of the file, these checks with every handle closed before the rename, D1's check for a `-journal`, `-wal` or `-shm` next to the destination and its `DestinationIsSource` tests, rename over the destination, full directory sync.

On any failure, cancellation (D10) or crash, no destination file appears and an existing one keeps its bytes; on normal unwinding the call removes the temporary file, the `-snapshot` file of D2 if it still exists, and the `-journal`, `-wal` and `-shm` files next to either (after a crash, the launch cleanup removes them, D1); the source is unchanged.

| Error | When | Data |
|---|---|---|
| `SourceNotSchema18` | the source's `col.ver` is not 18 | version found |
| `DestinationIsSource` | D1 | |
| `UnconvertibleConfig` | D3.1 | the key, and the reason: not text, not a BLOB, not JSON, out of range, too deep |
| `UnreadableRecord` | D3.1, D5 | table, row key, reason (`model/row-records` R10, `model/protobuf-records` P9, `too deep` or `deck id 0` (D3.1), or `non-finite value` with the field, D5) |
| `InsufficientSpace` | the disk fills up (SQLite's full error, or no space on a write or rename) | |
| `Cancelled` | D10 | |
| `VerificationFailed` | a check of this section fails | which check |
| `Io` | a `-wal` or `-journal` next to a closed source (D2), a `-journal`, `-wal` or `-shm` next to the destination (D1), a collation or function a foreign object needs (D2 step 4), a connection setting that does not read back (D3, D3.5), or any other SQLite or file-system failure | the detail |

U123 (`.apkg`) and U124 (`.colpkg`, `package/legacy-export` L8) show the user a failed export; the downgrade has no UI of its own.

## D9. Foreign objects

Objects of the source that are not Anki's (`storage/schema-18` S6: an add-on's table, an index, a view, a trigger):

- A foreign table stays, with its rows (D2 for rowids), unchanged by the drops of D3.2 and D3.3 even when it declares a foreign key on a removed table or on `graves` (foreign keys are off, D3). Such a foreign key keeps its stored text; in the copy it names a table that no longer exists, or the new `graves`, which has no key on its columns, so a later write that enforces it fails, as for views below. This is not what Anki's downgrade does (D12 `downgrade-foreign-keys`).
- Foreign indexes and triggers defined on a table that D3.2 removes are removed with it, and so are those defined on the schema-18 `graves`: they go with the old table and are not created again. This is what Anki's downgrade does.
- Views, and triggers on kept tables, keep their stored text; the triggers never fire during the conversion (D3). Those that read `graves` read the new table (D3.3 renames nothing). Those that read a removed table stay and fail when used, as in Anki's copy. One that does not resolve in the source (it names a table or column that does not exist) stays as stored, and the copy is made.
- Every other foreign object stays unchanged. One that needs a collation or function Meguri does not register fails the call before the conversion (D2 step 4).

So the copy holds D4's objects plus the source's foreign objects minus those on removed tables and on `graves` (D8 check 3). Differences from Anki (D12): Anki renames `graves` to `graves_old` before replacing it, so its copy's views and triggers that read `graves` name a table that no longer exists (`downgrade-graves-views`), and a foreign object already named `graves_old` makes that rename, and so its downgrade, fail (`downgrade-graves-old-name`; probed with SQLite 3.54.0: "there is already another table or index with this name: graves_old"); SQLite checks the whole schema when it renames a table, so a view or trigger that does not resolve makes Anki's rename, and so its downgrade, fail (`downgrade-broken-schema`; probed with SQLite 3.45.3: "error in view v: no such table: main.missing"). Meguri renames nothing, so neither case stops it. No fixture holds a foreign object, so no golden shows this; `tests/downgrade.rs` asserts Meguri's result.

## D10. Fault points and cancellation

- Fault points (U21's `faultpoint!`; U250 owns the catalog, and `storage/durability-and-files#F6`'s list of named points gains these two): `storage.downgrade.after_snapshot` (after `VACUUM INTO` and the rename of the snapshot over the temporary file, before the conversion) and `storage.downgrade.before_rename` (after D8's checks and D1's repeated tests). The atomic file's own points (`storage.atomic.*`, F6) fire as well, in their place in its sequence: `storage.downgrade.before_rename` fires after `storage.atomic.after_verify` and before `storage.atomic.before_rename`. Killed at either: no destination, the source unchanged, the temporary file present until the next launch's cleanup.
- Cancellation: the storage entry point takes a cancellation predicate, as the storage's other long calls (vacuum, analyze) do, and the engine adapts U24's cancellation token to it (the storage crate cannot depend on the engine's token). The call checks it after the snapshot, after the conversion and before the rename; it may also stop `VACUUM INTO` early through SQLite's progress handler. A cancel follows the failure path with `Cancelled`.

## D11. Scenarios and tests

The scenarios start from the fixture `storage/schema-18.anki2` (`storage/schema-upgrade#U16`: no stock note type; note types `Pair` and `Gap`, decks `Default`, `Lang`, `Lang::Verbs` and the filtered `Cram`, presets `Default` and `Slow`, tags, review-log rows, graves and the config key `meguriFixture` stored as non-compact text). They add, with `db_exec`, one `other` member to the note type `Pair`, its field `Front`, its template `Forward`, the deck `Lang` and each preset, out-of-range values (D5) to the preset `Default`, values at their bounds to `Slow`, and a tag state schema 11 cannot hold. The ops and observations are `specs/conformance/storage-ops.md` P1, P4 to P7.

| Scenario | Covers | Golden |
|---|---|---|
| `storage/downgrade/contents` | D3 to D5: the copy's `col` row (objects with their `other` members, compact config values, preset range checks), objects, journal mode, integrity, graves, notes, cards and review log; the collection's own dump and objects unchanged by the copy | recorded |
| `storage/downgrade/roundtrip` | D7: the copy, opened (and so upgraded) in place of the collection, against the dump before the copy; `other` members survive the round trip in the blobs | recorded |
| `storage/downgrade/oracle-opens` (live) | T1: Meguri writes the copy, the oracle reads it and opens it; its dump equals the golden of Anki's own copy and upgrade | recorded |
| `storage/downgrade/config-invalid` | D3.1, D8: a config value with a number beyond the 64-bit float range, then one that is not JSON, refuse the copy (`invalid_json`) and leave the collection as it was; with an in-range value the copy succeeds | recorded |

`core/crates/meguri-storage/tests/downgrade.rs` (Meguri only) covers what no scenario shows:

- the source file's SHA-256, its `-wal` size and `data_version` are unchanged after a copy made with no operation running; a copy taken while the writer commits holds the state of its start, and the writer's commits are never blocked;
- the destination equal to the source (and to its `-wal`) is refused with nothing written, and so are a case variant (`COLLECTION.anki2`, `collection.anki2-WAL`) in the source's directory and a hard link to the source elsewhere; an existing destination is replaced only by a verified copy, and keeps its bytes after a failure; a `-journal`, a `-wal` and a `-shm` next to the destination each give `Io` with nothing written and the existing file unchanged;
- a closed staging file as the source, in rollback mode and in WAL mode: its directory holds the same names and the file the same bytes afterwards; with a `-wal` or a `-journal` next to it, `Io` and nothing written;
- the snapshot target: `VACUUM INTO` writes to a name that did not exist, and a target that exists gives `Io`;
- kill at both fault points, with the destination in a `.meguri-stage-*` staging directory of the collection's directory (D1): no destination, source intact, the temporary file removed by the launch cleanup; cancellation at each check and during `VACUUM INTO`, after which neither the temporary file nor the `-snapshot` file nor a sibling of either remains;
- `UnconvertibleConfig` for each reason (a TEXT value, a non-UTF-8 key, a value of 127 nested levels refused and one of 126 carried, and the copy then opened by Meguri's upgrade), `UnreadableRecord` for a blob that fails P9, for a deck kind with neither member, for a non-UTF-8 name in each of the six name columns, for a deck `usn` and a tag `usn` of 4294967297, for a deck under id 0 (reason `deck id 0`), and for an `other` member of a note type and of a field whose deepest array sits at column level 128 (reason `too deep`), with one level less carried;
- a tag with `collapsed` 2 and one with −1: not refused; one with `collapsed` 0.5 (REAL): refused;
- a note type under id 0 with notes: carried; its upgrade gives it a new id and the notes keep `mid` 0 (D7); a note type whose fields have `ord` 0 and 2 and whose templates have `ord` 0 and 2: carried; the upgrade gives `ord` 0 and 1 (D7);
- an `other` member whose value repeats a member name: the copy's column has it once, with the last value (D3.1);
- after a copy, no byte of a dropped table's unknown protobuf field (a marker value placed in a note type blob) is found anywhere in the file (`secure_delete`, D3.2); a source in WAL mode gives a copy whose header bytes 18 and 19 are 1;
- `UnreadableRecord` with reason `non-finite value` (D5) for NaN in `seconds_to_show_question`, in an element of `fsrs_params_6` and in an element of `learn_steps`, for +∞ in `easy_days_percentages`, for NaN in an element of a filtered deck's `delays` and for +∞ in a deck's desired retention; a NaN in a bounded field is written as its default, not refused; a lapse multiplier of −0.0 is written as `-0.0`; a deck retention of −0.5 is written as 0;
- a `fields` and a `templates` row whose `ntid` matches no note type, with a blob that fails P9: not refused, absent from the copy and from its upgrade; a `tags.config` value of any storage class: not refused;
- a source missing `idx_cards_odid` or `idx_notes_mid`, one missing `ix_notes_csum`, and one without `sqlite_stat4` (and one without either statistics table): each copy passes D8 check 3 and lacks what the source lacked;
- a foreign index declared `COLLATE unicase`: D8 check 1 passes; a foreign index with an unregistered collation, an expression index calling an unregistered function and a `CHECK` constraint calling one: each gives `Io` with nothing written (D2 step 4);
- foreign objects (D9): a foreign table with rows, one with `AUTOINCREMENT` (`sqlite_sequence`) and one with a `UNIQUE` column (`sqlite_autoindex_*`), an index and a trigger on `notes` (kept), an index and a trigger on `graves` and on `decks` (removed), a view reading `graves` (reads the new table) and one reading `decks` (kept), a view reading a table that does not exist (kept, the copy made), an `AFTER UPDATE` trigger on `col` that reads `decks` and one that inserts into a foreign table (neither fires: the copy is made and the foreign table keeps its rows; Anki's downgrade fails on the first, D12), a foreign table whose foreign key references `decks(id)` with `ON DELETE CASCADE` and one referencing `graves` with `ON DELETE RESTRICT`, both with rows (the copy is made and both keep every row, D3), and a foreign table named `graves_old` (kept, the copy made);
- D7's differences, and nothing else, between a source and the upgrade of its copy, over the fixture and over a collection built by the U11 generator; a preset with starting ease 2.001 comes back as 2.0.

## D12. Differences from Anki

Decided 2026-10-27 (the rows from `downgrade-foreign-keys` to `downgrade-deep-json`, `downgrade-non-finite-deck` and the split of `downgrade-snapshot`: 2026-11-10), data safety first. `tests/downgrade.rs` (D11) asserts each one. No golden shows any of them: no fixture or scenario holds such a source, and `conformance/deviations.yaml` holds golden transforms only (`specs/conformance/dsl.md` D13), so none has an entry there yet. A scenario that comes to show one adds its entry under the id below.

| Id | Source | Anki 26.09.3 | Meguri |
|---|---|---|---|
| `downgrade-missing-index` | no `idx_cards_odid` or `idx_notes_mid` (D3.2) | the downgrade fails | the copy is made |
| `downgrade-graves-views` | a view or trigger that reads `graves` (D9) | it names `graves_old`, which no longer exists | it reads the new `graves` |
| `downgrade-broken-schema` | a view or trigger that does not resolve (D9) | the rename of `graves` fails, and so does the downgrade | the copy is made; the object stays as stored |
| `downgrade-triggers-off` | a foreign trigger on `col` (D3) | it fires at each of the seven writes of `col`, the last after the drops; one that reads a removed table fails there, and so does the downgrade | it never fires; the copy is made |
| `downgrade-foreign-keys` | a foreign table with a foreign key on a table D3.2 removes or on `graves`, with referencing rows (D3, D9) | foreign keys are enforced: the drop deletes or changes those rows (`CASCADE`, `SET NULL`, `SET DEFAULT`) or fails, and with it the downgrade (`NO ACTION`, `RESTRICT`) | foreign keys are off: the rows are kept as they are; the copy is made |
| `downgrade-graves-old-name` | a foreign object named `graves_old` (D9) | the rename of `graves` fails, and so does the downgrade | the copy is made; the object stays |
| `downgrade-foreign-function` | a foreign index, expression or `CHECK` constraint needing a collation or function neither registers (D2 step 4) | its downgrade rebuilds no index of the kept tables, so one that calls an unknown function does not stop it (an unknown collation in an index makes its rename of `graves` fail; probed with SQLite 3.54.0) | `VACUUM INTO` fails: `Io`, nothing written |
| `downgrade-usn-range` | a `usn` beyond 32 bits signed in a note type, template, deck, preset or tag row (D3.1) | its low 32 bits are written | refused with `UnreadableRecord` |
| `downgrade-deck-id-0` | a deck stored under id 0 (D3.1) | the copy holds it; neither upgrade opens the copy | refused with `UnreadableRecord`, reason `deck id 0` |
| `downgrade-deep-json` | a config value nested exactly 127 levels, or a record whose object would reach level 128 of its column (D3.1) | the copy holds it and its upgrade cannot read the column; an `other` of 128 levels or more is dropped whole | refused (`UnconvertibleConfig` or `UnreadableRecord`, too deep) |
| `downgrade-non-finite-preset` | NaN or ±∞ in an unbounded `f32` of a preset (D5) | the copy holds `null`; no upgrade opens it, or a step list reads back empty | refused with `UnreadableRecord` |
| `downgrade-non-finite-deck` | NaN or ±∞ in a filtered deck's `delays` or a deck's desired retention (D5) | the delay is written as `null` and the list reads back empty; the retention is written as 0 or 4294967295 | refused with `UnreadableRecord` |
| `downgrade-snapshot` | `.colpkg` (D1, D2) | the live collection is closed, downgraded in place, packed and upgraded again when it reopens (`package/legacy-export` L13) | a snapshot of the open collection is converted; the collection is never written (R2013) |
| `downgrade-snapshot` | `.apkg` (D1, D2) | the temporary export collection it built is downgraded in place as it is closed; the live collection is not touched | a snapshot of U123's closed staging file is converted; that file is never written |
