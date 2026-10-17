# Model ops: schema-11 JSON and raw rows

Spec: conformance/model-ops
Requirements: R1883, R1463, R1464, R1460, R1461
Anki: 26.09.3
Scenarios: conformance/scenarios/model/

The observation kinds and ops that the model scenarios need, plus how they use two draft ops and fix one form of a third. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The conversions they exercise are `specs/model/schema11-json.md`, `specs/model/protobuf-records.md` and `specs/model/card-data-json.md`.

## L1. `legacy_json` (observation kind)

The schema-11 JSON objects of note types, decks and presets, as converted from what the collection holds at that moment.

- **Request** `{legacy_json: {notetypes?: [<name>, ...], decks?: [<name>, ...], presets?: [<name>, ...]}}`: at least one list, each with at least one name. Note type and preset names are as stored; deck names are in human form (`::`) as the collection holds them at that moment, so a deck renamed or inserted by an earlier step is named by its current name.
- **Value** `{notetypes?: {<name>: <object>}, decks?: {<name>: <object>}, presets?: {<name>: <object>}}`, with a member for each requested list. Each object is the record converted per `specs/model/schema11-json.md` (output direction), parsed as JSON: integers stay integers and numbers written with a fraction or exponent stay non-integers (an `f32` written `1.0` is 1.0, `observations.md` O6). The `other` members are part of the object.
- **What is read.** The executor reads the records from the collection file when the step runs, never from a cache, so rows that `db_exec` wrote are seen. Anki applies its preset range checks when it reads a preset (`specs/model/schema11-json.md` S13); the oracle reports what Anki gives, and Meguri's executor applies the same checks once the preset spec (U42) exists. Scenarios keep preset values in range.
- **Ids.** The objects hold real ids (`id`, `conf`, `did`, field and template `id`). They are part of the value by definition (`observations.md` O1); scenarios keep them deterministic (ids come from the frozen clock, and field and template ids are set by the scenario, since Anki gives new fields and templates random ids).
- **Errors.** A name that matches no record is a run error (`unknown_ref`).

## L2. `legacy_update` (op)

Saves a schema-11 object over an existing record, the way records arriving by sync or in a legacy package are saved.

- **Argument** `{notetype: <name>, patch: <object>}`, `{deck: <name>, patch: <object>}` or `{preset: <name>, patch: <object>}`: exactly one of `notetype`, `deck` and `preset`, named as in L1.
- **Effect.**
  1. Take the object that `legacy_json` would report for the record now.
  2. Apply `patch` to it as a JSON merge patch (RFC 7396): each member of `patch` replaces the member of the same name; a member whose value is `null` removes that key; a member whose value is an object is merged into the object's member the same way (into `{}` when the member is missing or not an object); any other value, arrays included, replaces the member whole, `null`s inside it kept. So a scenario cannot set a top-level key to `null`; it removes it instead.
  3. Convert the result to a record (`specs/model/schema11-json.md`, input direction), and save it under its id:
     - note types and decks keep the `usn` and `mod` the object holds (a scenario patches them to change them); a deck's name goes through S11;
     - a preset is saved as a local change: `usn` -1 and `mtime_secs` the clock's current second;
     - Anki runs part of its note type save logic on the converted note type: names normalized and card requirements (`req`) recomputed; template checks are skipped. Scenarios use note types for which this changes nothing beyond `req`.
- **Output** none (`{}`).
- **Rejection** `{legacy_update: {error: "invalid_json"}}` when the patched object does not convert (a key `specs/model/schema11-json.md` marks "refuse"); nothing changes then (`dsl.md` D7.3). Rejection kinds: `invalid_json`.
- **Errors.** An unknown name is a run error (`unknown_ref`). Other failures of the save (a duplicate name, a template check) are run errors; scenarios avoid them.

## L3. Other ops the model scenarios use

- **`db_exec`** (storage area, `record-ops.md` W1: a statement that returns no rows records `{rows: []}`, TEXT values are strings). Model scenarios use it:
  - to read a blob as text with SQLite's `hex()` (upper-case hex digits), and `cards.data` as stored text;
  - to write blobs with blob literals (`X'0a0408011003'`), or to append fields to a stored blob with `CAST(<column> || X'…' AS BLOB)`: SQLite's `||` gives TEXT, and a blob column holding TEXT makes the row unreadable (`specs/model/protobuf-records.md` P1). Appending works because a later occurrence of a field merges with, or replaces, the earlier one (P2);
  - to set `cards.data` text through a bound string parameter;
  - to insert a row (a filtered deck) that no setup section can create.

  Scenarios cannot name real ids, so they address rows by name, by their note type's name, or by position in `ORDER BY id` (cards are created in logical-id order, so `ORDER BY id LIMIT 1 OFFSET k` is `#c(k+1)`).
- **`suspend`** (`sched-ops.md` J3): `{cards: [#cN, ...]}`, no output. Suspends the listed cards with Anki's suspend operation; a card already suspended is left as is. Each suspended card's row is written again, so its `data` is rebuilt (`specs/model/card-data-json.md` C4). It is the simplest operation that rewrites a card row without changing any value stored in `data`.

## L4. `blob_decode` (observation kind)

Whether the collection can decode the protobuf blobs of named records (`specs/model/protobuf-records.md` P2, P9). It shows the blobs Anki refuses, which `legacy_json` cannot: a record Anki cannot read makes `legacy_json` a run error.

- **Request** `{blob_decode: {notetypes?: [<name>, ...], decks?: [<name>, ...], presets?: [<name>, ...]}}`: at least one list, each with at least one name, named as in L1. A record is found by its name column, without decoding any blob.
- **Value** `{notetypes?: {<name>: <status>}, decks?: {<name>: <status>}, presets?: {<name>: <status>}}`, with a member for each requested list. `<status>` is `"ok"` when every blob of the record decodes, and `"invalid_blob"` when one does not, for any reason of P9 except `no deck kind`. The blobs of a record: for a note type, `notetypes.config` and the `config` of each of its `fields` and `templates` rows; for a deck, `common` and `kind`; for a preset, `config`.
- **Oracle.** It reads each record the way `legacy_json` does, after reopening the collection, and reports `invalid_blob` when the read fails with Anki's protobuf decode error, `ok` when the read succeeds.
- **Meguri.** Its executor decodes each blob of the record with the codecs of `specs/model/protobuf-records.md` and reports `invalid_blob` for a P9 error other than `no deck kind`, `ok` when every blob decodes.
- **Errors.** A name that matches no record is a run error (`unknown_ref`). Any other failure to read the record, on either side, is a run error too (a deck kind with no member, a blob column holding TEXT or NULL); scenarios avoid them.
- Nothing in the collection changes.

## L5. Status

- The oracle driver implements `legacy_json`, `blob_decode`, `package_col` and `legacy_update` (stable in `observations.md` O4 and `dsl.md` D7.2), the legacy form of `export` (L6; a scenario that uses another form fails with `unsupported_op`, as for any draft op), `db_exec` as `record-ops.md` W1 defines it, and `suspend` with the arguments above (its `dsl.md` row stays draft until the scheduling area fixes it). Every model scenario has a golden, `model/proto/decode-limits` and `model/schema11/apkg-notetypes` included.
- The oracle reopens the collection before each `legacy_json`, `legacy_update` and `blob_decode` to read the file, which also empties Anki's undo history. Model scenarios never undo; a scenario that undoes across one of these steps is not portable.
- Meguri's executor implements `legacy_json`, `legacy_update` and `blob_decode` with U21's codecs once the record store (U23) and the setup adapters exist; `db_exec` comes with the storage area and `suspend` with the scheduling area. The legacy `export` form comes with the legacy package writer (U123) and `package_col` with the package reader (U120). Until then the scenarios are `pending`, and U21 uses their goldens as conversion pairs (`specs/model/schema11-json.md` S14).

## L6. `export`, legacy package form (op)

`export` belongs to the package area and is draft (`dsl.md` D7.2). The model scenarios need one form of it, a legacy package, so this section fixes that form. The package area adds the other forms and keeps this one as written; an option it adds later must default to the behavior below.

- **Argument** `{format: apkg, name: <file name>, legacy: true}`, with no other keys. `name` is a file name of letters, digits, `.`, `_` and `-` that starts with a letter or digit and ends in `.apkg`.
- **Effect.** Writes the whole collection as a legacy package to the file `name` in the session directory, replacing a file of that name. Scheduling information and presets are included; media files are not. A note type is in the package only when an exported note uses it, so a scenario gives each note type it observes a note. The collection does not change.
- **The package.** A zip archive. Anki 26.9.3 writes a `meta` entry naming package version 2 (L7), the exported collection at schema 11 as `collection.anki21`, a placeholder collection for older clients as `collection.anki2`, and a `media` entry. The package format is the package area's (U120, U123); the model scenarios rely only on L7 finding the exported collection, with its `col` row in the schema-11 form (`specs/model/schema11-json.md` S12).
- **Output** `{notes: <count>}`: the number of notes in the package.
- **Not observable.** The package's bytes: entry times, the compression and the placeholder collection's random values differ from run to run and between executors. Observe the content with L7.
- **Errors.** A malformed argument is `invalid_scenario`. An export that Anki refuses is a run error; scenarios avoid it. No rejection kinds.
- In a live chain the package crosses a handoff with the session directory (`dsl.md` D9.2).

## L7. `package_col` (observation kind)

The `col` row of the collection inside a legacy package that an `export` step wrote (L6).

- **Request** `{package_col: {name: <file name>, columns: [<column>, ...]}}`: `name` as in L6, the file of an earlier `export` step of the scenario (one before a handoff included); `columns` a non-empty list, without repeats, of `ver`, `conf`, `models`, `decks`, `dconf` and `tags`.
- **What is read.** The package's collection entry. The `meta` entry, when the archive has one, holds a protobuf message whose field 1 (varint) is the package version: version 1 means `collection.anki2` and version 2 `collection.anki21`. Without `meta`, the entry is `collection.anki21` when the archive has it, else `collection.anki2`. The entry is read as the SQLite file it is, never opened as a collection: opening converts it to schema 18, which empties the JSON columns.
- **Value** `{<column>: <value>, ...}`, one member per requested column. `ver` is the stored integer. Each other column is its stored text parsed as JSON, numbers as in L1: integers stay integers and numbers written with a fraction or an exponent stay non-integers. So `models` is `{"<note type id>": <note type object>, ...}` as `specs/model/schema11-json.md` S12 lays it out.
- **Comparison** as any value (`observations.md` O7): objects compare member by member, so the order in which Anki writes the members of a column and of its objects (S3, S12) never counts.
- **Errors.** A package version other than 1 or 2 is `invalid_scenario` (the current format holds a schema-18 collection, whose JSON columns are empty). A `name` with no such file in the session directory is a run error (`unknown_ref`). An archive that does not read, a missing entry and a column whose text is not JSON are run errors as well; scenarios avoid them.
- Nothing in the collection or the package changes.
- **Oracle.** It opens the archive and reads the entry with SQLite directly, never through Anki.
- **Meguri.** Its executor finds the entry with the package reader (U120) and reads it the same way.
