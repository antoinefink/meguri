# Model ops: schema-11 JSON and raw rows

Spec: conformance/model-ops
Requirements: R1883, R1463, R1464, R1460, R1461
Anki: 26.09.3
Scenarios: conformance/scenarios/model/

Two observation kinds and one op that the model scenarios need, plus how they use two draft ops. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The conversions they exercise are `specs/model/schema11-json.md`, `specs/model/protobuf-records.md` and `specs/model/card-data-json.md`.

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
- **`suspend`** (draft, scheduling area): `{cards: [#cN, ...]}`, no output. Suspends the listed cards with Anki's suspend operation; a card already suspended is left as is. Each suspended card's row is written again, so its `data` is rebuilt (`specs/model/card-data-json.md` C4). It is the simplest operation that rewrites a card row without changing any value stored in `data`.

## L4. `blob_decode` (observation kind)

Whether the collection can decode the protobuf blobs of named records (`specs/model/protobuf-records.md` P2, P9). It shows the blobs Anki refuses, which `legacy_json` cannot: a record Anki cannot read makes `legacy_json` a run error.

- **Request** `{blob_decode: {notetypes?: [<name>, ...], decks?: [<name>, ...], presets?: [<name>, ...]}}`: at least one list, each with at least one name, named as in L1. A record is found by its name column, without decoding any blob.
- **Value** `{notetypes?: {<name>: <status>}, decks?: {<name>: <status>}, presets?: {<name>: <status>}}`, with a member for each requested list. `<status>` is `"ok"` when every blob of the record decodes, and `"invalid_blob"` when one does not, for any reason of P9 except `no deck kind`. The blobs of a record: for a note type, `notetypes.config` and the `config` of each of its `fields` and `templates` rows; for a deck, `common` and `kind`; for a preset, `config`.
- **Oracle.** It reads each record the way `legacy_json` does, after reopening the collection, and reports `invalid_blob` when the read fails with Anki's protobuf decode error, `ok` when the read succeeds.
- **Meguri.** Its executor decodes each blob of the record with the codecs of `specs/model/protobuf-records.md` and reports `invalid_blob` for a P9 error other than `no deck kind`, `ok` when every blob decodes.
- **Errors.** A name that matches no record is a run error (`unknown_ref`). Any other failure to read the record, on either side, is a run error too (a deck kind with no member, a blob column holding TEXT or NULL); scenarios avoid them.
- Nothing in the collection changes.

## L5. Status

- The oracle driver implements `legacy_json` and `legacy_update` (stable in `observations.md` O4 and `dsl.md` D7.2), `db_exec` as `record-ops.md` W1 defines it, and `suspend` with the arguments above (its `dsl.md` row stays draft until the scheduling area fixes it). The model scenarios have goldens, except `model/proto/decode-limits`: it waits for the driver's `blob_decode` (L4), whose `observations.md` row is draft until then.
- The oracle reopens the collection before each `legacy_json`, `legacy_update` and `blob_decode` to read the file, which also empties Anki's undo history. Model scenarios never undo; a scenario that undoes across one of these steps is not portable.
- Meguri's executor implements `legacy_json`, `legacy_update` and `blob_decode` with U21's codecs once the record store (U23) and the setup adapters exist; `db_exec` comes with the storage area and `suspend` with the scheduling area. Until then the scenarios are `pending`.
