# Upgrading schemas 11 to 17 to schema 18

Spec: storage/schema-upgrade
Requirements: R1478, R1480, R2013, R2016
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/upgrade/

What happens to a collection file at schema 11, 14, 15, 16 or 17 when Meguri opens it: a copy of the original first (R1480), then the steps 14, 15, 16, 17 and 18 in one transaction, with Anki's semantics (R1478). The result must equal, row for row and byte for byte where the observation compares bytes, what Anki 26.09.3 makes of the same file (goldens under `conformance/goldens/storage/upgrade/`). The gate that calls the upgrade is `storage/open-and-connections` C2; the schema-18 layout is `storage/schema-18`; the conversion of schema-11 JSON objects into rows and protobuf blobs is the model area's (`specs/model/schema11-json.md`, `specs/model/protobuf-records.md`, U21), and this spec adds only what the upgrade does around it.

## U1. Steps

A file at version V runs every step above V, in order, inside one write transaction on the writer:

| Step | Runs when V is | In short |
|---|---|---|
| 14 | 11 | new tables `deck_config`, `config`, `tags`; presets, config and tags move out of `col` |
| 15 | 11, 14 | new tables `fields`, `templates`, `notetypes`, `decks`, five indexes, `ANALYZE`; note types and decks move out of `col`; presets become protobuf with the starting ease ×100 |
| 16 | 11, 14, 15 | starting ease ÷100; presets at or below 1.3 reset to 2.5 and their low card eases raised |
| 17 | 11, 14, 15, 16 | `tags` table rebuilt with `collapsed` and `config` |
| 18 | 11, 14, 15, 16, 17 | `graves` rebuilt with a primary key, duplicates dropped |

Each step ends by setting `col.ver` to its number. Only the state after step 18 is observable (one transaction), so an implementation may merge steps as long as the end state is the same, `ANALYZE` included (U5).

Statement texts below are given in the normalized form of `storage/schema-18` S2; use exactly those tokens.

## U2. Pre-upgrade copy (R1480)

Before the writer opens the file (gate phase 3), Meguri copies it:

- Name: the collection file's name plus `.pre-upgrade-v<V>`, in the same directory: `collection.anki2.pre-upgrade-v11` for a file at 11.
- Made with SQLite's online backup from a read-only connection to the original (opened as the probe opens it, `storage/open-and-connections` C2.1), into `<name>.tmp`, which is then synced and renamed over the final name, replacing an older copy of the same name. The copy therefore holds everything committed, including frames still in a `-wal` file.
- If the copy cannot be made (no space, I/O error), the open fails with `UpgradeFailed` before anything touches the original.
- The storage layer never deletes the copy; after a successful upgrade it stays as the user's pre-upgrade state. Backups (U126/U127) own its retention, and callers that upgrade a staging copy (U124) delete it with their staging directory.
- Anki makes no such copy; nothing the oracle observes depends on it.

## U3. One transaction

- The writer begins one `IMMEDIATE` transaction before step 14 (or the first step that runs) and commits after step 18. Anki does the same: a crash at any point leaves the original file as it was, and the next open upgrades again from the start.
- The journal-mode switch to WAL (C4) happens before the transaction and is not undone by a rollback; it changes no data.
- Any failure inside rolls back and fails the open with `UpgradeFailed { version found, step }` (U10). The original is intact, the copy of U2 exists, and the next open tries again.
- Fault points (`faultpoint!`, U21 contract; U250 owns the catalog): `storage.upgrade.after_copy` (after U2, before the writer opens) and `storage.upgrade.in_transaction` (after step 15, before commit). `tests/upgrade.rs` kills the process at the second: the original is unchanged (SHA-256), the copy is present, and the next open upgrades successfully.
- Progress: the gate reports each step as it starts (`14` … `18`) to its caller, which shows the lifecycle state `Upgrading` (U24).

## U4. Step 14

1. Create the tables:
   - `CREATE TABLE deck_config ( id integer PRIMARY KEY NOT NULL, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL )`
   - `CREATE TABLE config ( KEY text NOT NULL PRIMARY KEY, usn integer NOT NULL, mtime_secs integer NOT NULL, val blob NOT NULL ) without rowid`
   - `CREATE TABLE tags ( tag text NOT NULL PRIMARY KEY COLLATE unicase, usn integer NOT NULL ) without rowid`

   and set `col.ver` to 14.
2. Presets: `col.dconf` holds a JSON object whose member names are preset ids and whose values are presets in the schema-11 layout. Each becomes a `deck_config` row: `id` the member name (the preset's own `id` member is ignored; buggy clients left it different), `name` from `name`, `mtime_secs` from `mod`, `usn` from `usn`, `config` the preset in its schema-11 JSON form. At schema 14 `config` holds JSON, not protobuf; step 15 converts it, so for an upgrade from 11 only the values matter, not that intermediate text. Then `col.dconf` becomes the empty string.
3. Config: `col.conf` holds a JSON object. Each member becomes a `config` row: `KEY` the member name, `val` the member's value written as compact JSON (U9.2), `usn` 0, `mtime_secs` 0. Then `col.conf` becomes the empty string.
4. Tags: `col.tags` holds a JSON object whose member names are tags and whose values are their usn (integers). Each becomes a `tags` row (`tag`, `usn`), except a tag equal under `unicase` to one already stored, which is dropped (the earlier one keeps its spelling and usn; order: U11). Then `col.tags` becomes the empty string.

## U5. Step 15

1. Create, in this order, and set `col.ver` to 15:
   - `CREATE TABLE fields ( ntid integer NOT NULL, ord integer NOT NULL, name text NOT NULL COLLATE unicase, config blob NOT NULL, PRIMARY KEY (ntid, ord) ) without rowid`
   - `CREATE UNIQUE INDEX idx_fields_name_ntid ON fields (name, ntid)`
   - `CREATE TABLE templates ( ntid integer NOT NULL, ord integer NOT NULL, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL, PRIMARY KEY (ntid, ord) ) without rowid`
   - `CREATE UNIQUE INDEX idx_templates_name_ntid ON templates (name, ntid)`
   - `CREATE INDEX idx_templates_usn ON templates (usn)`
   - `CREATE TABLE notetypes ( id integer NOT NULL PRIMARY KEY, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, config blob NOT NULL )`
   - `CREATE UNIQUE INDEX idx_notetypes_name ON notetypes (name)`
   - `CREATE INDEX idx_notetypes_usn ON notetypes (usn)`
   - `CREATE TABLE decks ( id integer PRIMARY KEY NOT NULL, name text NOT NULL COLLATE unicase, mtime_secs integer NOT NULL, usn integer NOT NULL, common blob NOT NULL, kind blob NOT NULL )`
   - `CREATE UNIQUE INDEX idx_decks_name ON decks (name)`
   - `CREATE INDEX idx_notes_mid ON notes (mid)`
   - `CREATE INDEX idx_cards_odid ON cards (odid) WHERE odid != 0`

   then run `ANALYZE` (whole database; it creates or refreshes `sqlite_stat1` and `sqlite_stat4`, `storage/schema-18#S5`).
2. Note types (U6), then decks (U7), then presets (U8).

## U6. Note types (step 15)

`col.models` holds a JSON object whose member names are note type ids and whose values are note types in the schema-11 layout. For each note type, in the order of U11:

1. Convert it with the schema-11 note type conversion (U21): note type row values and config blob, the field list and the template list.
2. Id 0: Anki has met note types stored under id 0. When the member name is `0`, the note type gets a new id: a uniformly random integer from 1 to 4294967295 (2^32 − 1) not used by another note type of the collection (Meguri's random source, KTD18; Anki draws it at random too, so goldens never contain it). Notes whose `mid` is 0 keep it: the upgrade does not touch `notes`.
3. Names in NFC (`text/unicode-primitives#U3`): the note type name, every field name and every template name.
4. Template names unique within the note type under `unicase`: going through the templates in list order, while a template's name equals (under `unicase`) the name of an earlier template as already renamed, append `+` to it. Then the same for field names. Names `A`, `a`, `a+` become `A`, `a+`, `a++`.
5. Note type name unique within the collection under `unicase`: while the name equals the name of a note type already written by this step, append `_`.
6. Write the rows:
   - `notetypes`: `id` (the member name, or the new id of 2), `name`, `mtime_secs` from `mod`, `usn` from `usn`, `config` the converted config blob. A rename by 3 or 5 does not change `mtime_secs` or `usn`.
   - `fields`: one row per field, `ord` its position in the stored field list from 0 (not the field's own `ord` member), `name`, `config` the converted blob.
   - `templates`: one row per template, `ord` its position in the stored template list from 0, `name`, `mtime_secs` 0, `usn` 0, `config` the converted blob.
7. Then `col.models` becomes the empty string.

The member name is the note type's id for all three tables. (When a note type's own `id` member differs from its member name, Anki writes the `notetypes` row under the `id` member but the field and template rows under the member name. No file Anki wrote has that; Meguri uses the member name for all three.)

## U7. Decks (step 15)

`col.decks` holds a JSON object of decks in the schema-11 layout (normal decks and filtered decks, `dyn`). For each deck, in the order of U11:

1. Convert it with the schema-11 deck conversion (U21): `common` and `kind` blobs, `mtime_secs` from `mod`, `usn` from `usn`. The row id is the deck's own `id` member. A deck whose `id` is 0 fails the upgrade (U10).
2. Name: split the stored name at every `::`, normalize each part, and join the parts with U+001F (the stored form at schema 18). Normalizing a part:
   1. NFC;
   2. remove every ASCII control character (U+0000–U+001F and U+007F);
   3. remove leading and trailing characters that are white space (Unicode `White_Space`) or `:`;
   4. if nothing is left, the part is `blank`.

   | Stored at 11 | Stored at 18 (U+001F shown as `␟`) |
   |---|---|
   | `Lang::Verbs` | `Lang␟Verbs` |
   | `  Spaced :: Out ` | `Spaced␟Out` |
   | `a::::b` | `a␟blank␟b` |
   | `a:::b` | `a␟b` |
   | `x` U+0007 `y` | `xy` |
   | `fo` U+001F `o::ba` U+000A `r` | `foo␟bar` |

3. When the normalized name, with U+001F written back as `::`, differs from the stored name, the deck is stamped modified: `mtime_secs` = now in seconds (the scenario clock), `usn` = −1.
4. Deck name unique within the collection under `unicase` (comparing the U+001F-joined names): while the name equals the name of a deck already written by this step, append `_` to it (so to its last part), stamping the deck modified as in 3 each time.
5. Write the row (`id`, `name`, `mtime_secs`, `usn`, `common`, `kind`). A later deck with the same `id` replaces an earlier one.
6. Then `col.decks` becomes the empty string.

## U8. Presets (step 15)

For every `deck_config` row: read its schema-11 JSON (made by step 14, or stored by a schema-14 client), convert it with the schema-11 preset conversion (U21), then multiply the starting ease (protobuf field `initial_ease`, field 11 of the preset config, a 32-bit float) by 100 in 32-bit float arithmetic. Clients at schema 15 stored the starting ease that way (250 for 2.5), and step 16 divides it back. Write the row: `name`, `mtime_secs`, `usn` from the JSON's `name`, `mod`, `usn`, and `config` the protobuf blob, for the row whose id is the JSON's `id`.

## U9. Values and formats used by several steps

### U9.1 Usn

The upgrade runs as a client: every usn it stamps is −1 (decks renamed by U7, cards fixed by U13). Meguri never upgrades in a server role.

### U9.2 Compact JSON

The JSON written by step 14 into `config.val` is the parsed value written back without any whitespace: object members sorted by name (code point order) at every depth, whatever their stored order; an integer that fits a signed or unsigned 64-bit integer written as an integer; every other number written as the shortest decimal that reads back as the same 64-bit float, with `.0` on whole values and an exponent outside the plain range (exactly what `serde_json` writes for an `f64`; table below); strings with only the escapes JSON requires (`\"`, `\\`, and the characters below U+0020 as `\b`, `\f`, `\n`, `\r`, `\t` or `\u00xx` with lower-case hex), every other character as itself. Duplicate member names keep the last occurrence. Examples:

| In `col.conf` | In `config.val` |
|---|---|
| `{"b": 1, "a": 2.50, "c": [1.0, 2]}` | `{"a":2.5,"b":1,"c":[1.0,2]}` |
| `{"z": {"b": 1, "a": 2}}` | `{"z":{"a":2,"b":1}}` |
| `2` | `2` |
| `12345678901234567890` | `12345678901234567890` |
| `123456789012345678901` | `1.2345678901234567e20` |
| `123.0` | `123.0` |
| `1E2` | `100.0` |
| `-0` | `-0.0` |
| `1e15` | `1000000000000000.0` |
| `1e16` | `1e16` |
| `0.0001` | `0.0001` |
| `0.000001` | `1e-6` |
| `1e300` | `1e300` |
| `"a/b\u00e9\u0001\""` | `"a/bé\u0001\""` |
| `true`, `null` | `true`, `null` |

## U10. Inputs the upgrade cannot read

The upgrade fails (U3: rollback, `UpgradeFailed`, original intact) when:

- `col.conf`, `col.tags`, `col.dconf`, `col.models` or `col.decks` is not a JSON object of the layout its step expects (U21 defines the layouts and which members may be missing or of another type);
- a deck has id 0;
- a statement fails (for example a unique index hit that the renaming rules of U6 and U7 do not prevent).

Duplicate member names in any of these JSON texts keep the last occurrence, at every depth.

## U11. Order where Anki has none

Anki goes through note types, decks and tags in an unspecified order that changes from run to run, so which of two names equal under `unicase` gets the suffix (or, for tags, survives) varies between Anki runs. Meguri uses a fixed order, one of those Anki can produce:

- note types (U6) and decks (U7): ascending numeric id (the member name for note types, the `id` member for decks), so the older id keeps its name;
- tags (U4.4): the order of the members in the stored JSON text, so the first one written keeps its spelling.

Goldens never depend on this choice: scenarios observe these cases only through queries whose result is the same for every order (`storage/upgrade/names-11`).

Two other values are unstable in Anki for the same reason and are kept out of goldens by the fixtures (U16): the `other` bytes of a note type, field, template, deck or preset converted from JSON with more than one unknown legacy key at its top level (Anki writes them in a random order; Meguri keeps the stored order, U21), and the new id of a note type stored under id 0.

## U12. What the upgrade leaves alone

- `col.crt`, `col.mod`, `col.scm`, `col.dty`, `col.usn` and `col.ls` keep their values: an upgrade is not a change that needs syncing, and it never forces a full sync.
- `notes` and `revlog` are not touched; `cards` only by U13. Rows keep their `usn` and `mtime_secs` except where a step says otherwise.
- The upgrade is not an op: no undo entry, no change notification of its own (the opened collection is new to the engine anyway), no `col.mod` bump (U23's rule applies to ops only).
- Foreign objects (`storage/schema-18#S6`) are kept.

## U13. Step 16

1. For every `deck_config` row: decode the protobuf (no clamping of out-of-range values), divide the starting ease by 100 in 32-bit float arithmetic, and if the result is at most 1.3 (compared as 32-bit floats), set it to 2.5 and remember the preset. Write the row's `config` back (name, `mtime_secs` and `usn` unchanged). Clients at schema 15 created new presets with the ease already divided, which this catches.
2. For the remembered presets: the decks that use one of them are the normal decks whose preset id is in the set. Every card with `factor` not 0 and at most 2000 whose `did` or `odid` is one of those decks gets `factor` 2500 and `usn` −1; its `mod` does not change.
3. Set `col.ver` to 16.

| Stored at 15 | After step 16 | Cards of decks using it |
|---|---|---|
| 250.0 | 2.5 | unchanged |
| 230.0 | 2.3 | unchanged |
| 131.0 | 1.31 | unchanged |
| 130.0 | 2.5 (1.3 is at most 1.3) | factor 1 to 2000 → 2500, usn −1 |
| 2.5 | 2.5 (0.025 is at most 1.3) | factor 1 to 2000 → 2500, usn −1 |

The rewrite encodes the preset again (U21 canonical encoding). Protobuf fields Meguri does not know are kept (R1486; Anki would drop them, which no golden fixture contains).

## U14. Step 17

Read every `tags` row (`tag`, `usn`), drop the table, create

`CREATE TABLE tags ( tag text NOT NULL PRIMARY KEY COLLATE unicase, usn integer NOT NULL, collapsed boolean NOT NULL, config blob NULL ) without rowid`

and insert every tag again with its name and usn, `collapsed` 1 and `config` NULL. Set `col.ver` to 17.

## U15. Step 18

Rename `graves` to `graves_old`, create

`CREATE TABLE graves ( oid integer NOT NULL, type integer NOT NULL, usn integer NOT NULL, PRIMARY KEY (oid, type) ) WITHOUT ROWID`

copy the rows over in the old table's rowid order, keeping only the first row for each `(oid, type)`, drop `graves_old`, create `CREATE INDEX idx_graves_pending ON graves (usn)` and set `col.ver` to 18. (Before schema 18 nothing stopped a client from writing the same grave twice; the first one written wins.)

## U16. Fixtures

The oracle maintainer makes these collection files once and commits them under `conformance/fixtures/storage/`. Each is made by the Anki release named, through its own Python API, closed without the downgrade to schema 11 unless the recipe says otherwise, and finished with a few direct SQL edits where a recipe says so (with SQLite, after Anki has closed the file). Releases per schema: 11 (26.09.3 closing with the downgrade to 11), 14 (2.1.24–2.1.26), 15 (2.1.28–2.1.33), 16 (2.1.34–2.1.40), 17 (2.1.41–2.1.44), 18 (26.09.3). Never edit a fixture by hand afterwards and never let Meguri write one.

**Rules for every fixture** (they keep the goldens stable across oracle runs, U11, and keep Anki's assets out of the repository, R1959):

- No stock note type: add the note types below, then delete every note type the release created by itself.
- Where JSON is converted (fixtures at 11 and 14): at most one unknown legacy key at the top level of any note type, field, template, deck or preset; a preset's `new`, `rev` and `lapse` objects each count as one such key when they hold unknown keys of their own. Anki writes several in a random order (U11). Old releases write legacy keys of their own (for example in `rev`): remove them by SQL until the rule holds. Check by recording each upgrade golden three times: the recordings must be identical.
- No two note types, decks, fields of one note type, templates of one note type or tags equal under `unicase`; no note type or deck id 0; deck names already normalized (U7). Only `schema-11-names.anki2` breaks these, on purpose.
- Config key `schedVer` = 2. When the release knows `sched2021` and it is true, `creationOffset` must be present too: the oracle's Python layer otherwise writes `creationOffset` when it opens the file, which is no part of the upgrade (the releases at 14 to 17 have no `sched2021`, so their files need nothing).

**Standard content** (each `schema-N.anki2`, as far as the release allows):

- Note types: `Pair` (standard; fields `Front`, `Back`; templates `Forward` with front `{{Front}}` and back `{{Front}} = {{Back}}`, `Reverse` with front `{{Back}}` and back `{{Back}} = {{Front}}`; CSS `.card { font-size: 18px; color: navy; }`) and `Gap` (cloze; fields `Text`, `Extra`; template `Gap` with front `{{cloze:Text}}` and back `{{cloze:Text}} / {{Extra}}`; CSS `.cloze { color: teal; }`).
- Decks: `Default` (id 1), `Lang`, `Lang::Verbs`, and a filtered deck `Cram` built from `deck:Lang` holding at least one card.
- Presets: `Default` (id 1, the release's defaults) and `Slow` (new cards per day 5, starting ease 2.3, learning steps 2 m and 15 m), used by `Lang::Verbs`.
- Notes: four `Pair` notes (two in `Lang`, two in `Lang::Verbs`) and two `Gap` notes in `Lang` (one with `{{c1::…}}` and `{{c2::…}}`, one with `{{c1::…}}` only). Tags `vocab`, `vocab::verbs`, `Mixed_Case` spread over them; one note without tags. Field text is the maintainer's own words.
- Card states: new, learning, review (interval 10 days or more), relearning, suspended and buried cards, after reviews done through the release's scheduler, so `revlog` has at least five rows.
- One card in `Lang` (preset `Default`) whose `factor` is 1400 (SQL edit). It must keep it after the upgrade.
- Graves: delete one note (its card graves and note grave) and one empty deck through the API. In fixtures below 18, add by SQL one more `graves` row repeating an existing `(oid, type)` with another usn (the first row must survive, U15).
- Config: one extra key `meguriFixture` with the value `{"b": 1, "a": 2.50, "c": [1.0, 2, 1E2, -0, 0.000001, 1e16, 12345678901234567890, 123456789012345678901], "d": "a/b\u00e9\u0001\""}` written as that exact text (in `col.conf` at 11, where step 14 rewrites it per U9.2; in `config.val` from 14 on, where nothing rewrites it).
- At 11 only: the note type `Pair` and the deck `Lang` each carry one unknown key `meguriExtra` with the value `[1]`.

**Files:**

| File | Recipe |
|---|---|
| `schema-11.anki2` | standard content made by 26.09.3, then closed with the downgrade to schema 11 (rollback-journal file) |
| `schema-14.anki2` | standard content, release at 14 |
| `schema-15.anki2` | standard content, release at 15, plus the ease cases below |
| `schema-16.anki2` | standard content, release at 16 |
| `schema-17.anki2` | standard content, release at 17 |
| `schema-18.anki2` | standard content made by 26.09.3 (control: nothing to upgrade) |
| `schema-11-names.anki2` | `schema-11.anki2` plus the name cases below |

**Ease cases in `schema-15.anki2`.** Presets and the starting ease stored in their blob (check by decoding the blob; through that release's API, the legacy value `initialFactor` F is stored as F ÷ 10):

| Preset | Stored starting ease | Deck using it | Cards (`factor`) |
|---|---|---|---|
| `Fresh` | 2.5 (`initialFactor` 25) | `EaseLow` | 0 (a new card), 1300, 2000, 2001, 2500 |
| `Edge` | 130.0 (`initialFactor` 1300) | `EaseEdge` | 1500 |
| `Above` | 131.0 (`initialFactor` 1310) | `EaseAbove` | 1500 |
| `Default` | 250.0 | `Lang` | 1400 (from the standard content) |

Each Ease deck holds `Pair` notes enough for the cards listed; each listed factor is on one card, the other cards of those notes stay new (factor 0). Plus one more card of `EaseLow` with `factor` 1800, moved into the filtered deck `EaseCram` (so its `odid` is `EaseLow`). Card factors are set by SQL after the reviews.

**Name cases in `schema-11-names.anki2`**, written into the JSON of `col` by SQL:

- note types `pair` and `PAIR`: copies of `Pair` with new ids above every other id;
- a copy of `Pair` stored under the member name `0` (and `id` 0), named `Zero`, and one `Pair` note of `Lang` whose `mid` is set to 0;
- a copy of `Pair` named `Dupes` whose fields are `Front` and `front` and whose templates are `Card` and `CARD`;
- a copy of `Pair` named `Cafe` + U+0301 (decomposed);
- decks `Foo` and `foo` (ids 10 and 11), `  Spaced :: Out ` (id 12), `a::::b` (id 13), `x` U+0007 `y` (id 14), `Cafe` + U+0301 (id 15), all copies of `Lang` with `mod` 1000 and `usn` 5;
- `col.tags` replaced by `{"Tag": 3, "tag": 4, "other": -1}`.

**Refused files** (`storage/open-and-connections` C1, C2.2):

| File | Recipe |
|---|---|
| `ver-10-delete.anki2`, `ver-12-delete.anki2`, `ver-13-delete.anki2` | `schema-11.anki2` with `col.ver` set to 10, 12 or 13 |
| `ver-19-delete.anki2` | `schema-18.anki2` switched to `journal_mode=DELETE`, `col.ver` set to 19 |
| `ver-10-wal.anki2`, `ver-12-wal.anki2`, `ver-13-wal.anki2`, `ver-19-wal.anki2` | the matching `-delete` file switched to `journal_mode=WAL` and closed (no `-wal` file left) |
| `v1-scheduler-18.anki2` | `schema-18.anki2` with config `schedVer` set to 1 |
| `v1-scheduler-11.anki2` | `schema-11.anki2` with the member `schedVer` removed from `col.conf` |

No fixture may come from `local-data/` or contain anything from the user's collection.

## U17. Scenarios and tests

| What | Where |
|---|---|
| Schema 11, 14, 15, 16, 17 → 18: schema objects and the normalized dump equal the oracle's upgrade of the same file; schema 18 opens unchanged | `storage/upgrade/from-11` … `from-17`, `storage/upgrade/at-18` |
| Starting ease ×100 at 15 and the reset at or below 1.3 with the card factor fix | `storage/upgrade/initial-ease` |
| `Pair`/`pair`/`PAIR`, note type id 0, duplicate field and template names, NFC, deck name normalization and duplicates, case-duplicate tags | `storage/upgrade/names-11` |
| Card factor 1400 survives (preset not reset) | `storage/upgrade/from-11` |
| Duplicate graves: first row kept | `storage/upgrade/from-*` (dump of `graves`) |
| Pre-upgrade copy written first; killed mid-transaction → original intact, copy present, next open upgrades | `tests/upgrade.rs` (Meguri only) |
| Expected objects after each upgrade, then after ten open/close cycles and 1,000 ops | scenarios (`schema_objects`) and `tests/schema_objects.rs` |
