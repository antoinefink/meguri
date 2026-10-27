# Package ops and observations for scenarios

Spec: conformance/package-ops
Requirements: R1597, R1598, R1599, R1619, R1626 (stamps), R1628, R2005, R2009 (format half)
Anki: 26.09.3
Scenarios: conformance/scenarios/package/collection-package/

The scenario vocabulary for collection packages (U124a): the collection-package form of `export`, the collection-package forms of `import`, the observation `package_listing`, and the fixture packages under `conformance/fixtures/package/`. `export` and `import` belong to the package area (`dsl.md` D7.2); the forms this file does not fix stay draft for U120, U122, U123 and U124, and the legacy package form of `export` stays as `model-ops.md` L6 fixes it. The behavior observed is `specs/package/container.md` and `specs/engine/collection-replace.md`.

E6 to E11 fix, as draft, the deck-package forms and options that U120, U122, U123 and U124 need (`package/apkg-import#I18.1`, `package/apkg-export#A17`, `package/legacy-export#L14`): the deck-package `import` form (E6), the latest and legacy deck-package `export` forms (E7, E8), the collection package export options (E9), and the observations `package_media` (E10) and `package_db_dump` (E11). Draft means no executor has them yet (`dsl.md` D7.2): the oracle maintainer implements them with the scenarios of those sections and may correct a detail then, in this file, the schema and the typed model together.

## E1. `export`, collection package form

- **Argument** `{format: colpkg, name: <file name>}`, with no other key. `name` is a file name of ASCII letters, digits, `.`, `_` and `-` that starts with a letter or digit and ends in `.colpkg`. E1 is exactly this form, without `media` and `legacy`; a step that carries either key is E9's draft form (U124), even when the key is false.
- **Effect.** Writes the whole collection as a collection-only package in the latest format (`package/container#P11`) to the file `name` in the session directory, replacing a file of that name. The collection does not change.
  - Oracle: Anki's collection package export with media off and the option for older clients off. It needs the collection closed: the driver closes it, exports, and opens it again. That reopen empties the undo history and writes nothing; scenarios observe no undo state across an export.
  - Meguri: the collection-only writer on the live collection; nothing closes.
- **Output**: none; the step records `{}`.
- **Not observable**: the package's bytes (zip times, the placeholder's ids and random GUID, compression output). Observe the package with `package_listing` (E3), and its content by importing it (E2).
- In a live chain the package crosses a handoff with the session directory (`dsl.md` D9.2).

## E2. `import`, collection package forms

### E2.1 Argument

`{format: colpkg, fixture: <path>}` or `{format: colpkg, name: <file name>}`: exactly one of `fixture` and `name`, no other key.

- `fixture`: a path relative to `conformance/fixtures/`, of lower-case segments as `open`'s (`storage-ops.md` P1.1), ending in `.colpkg`. Executors find it as `storage-ops.md` P1.4 says (`ORACLE_FIXTURES` on the oracle; `fixture_error` when it is not a regular file there). Fixtures are read, never opened with SQLite in place, written or locked.
- `name`: a package in the session directory, written by an earlier `export` step (one before a handoff included); the E1 name rule. No such file: `unknown_ref`.

### E2.2 Effect

Replaces the scenario's collection with the package's, as a restore of a backup does.

- Oracle: Anki's collection package import into the session's `collection.anki2`, with the session's media folder and media database. It needs the collection closed: the driver closes it, imports, and opens `collection.anki2` again, after a refusal too.
- Meguri: inspect (`package/container#P12`), then the verified replace with the reason `Restore` (`engine/collection-replace`), which reopens the collection and restarts its day clock (`engine/lifecycle#L8`).
- On success the collection holds the package's rows, upgraded when older, with `col.scm` and `col.mod` set to the current instant in milliseconds (`engine/collection-replace#R7`); the undo history is empty. A collection-only package leaves the media folder unchanged on both sides.
- Package with media (draft, U124). Oracle: Anki writes each of the package's files into the media folder under its name, replacing a file of that name whose bytes differ, and leaves every other file in place. Meguri: the merge of `package/container#P16`, which also keeps the replaced bytes in the trash, so a scenario that replaces a file observes `media_trash` only with a deviation. Until U124 lands Meguri refuses such a package, so a scenario imports one only in a segment the oracle runs (a live chain after `handoff: oracle`, `package/apkg-export#A17`, `package/legacy-export#L14`); U124 lifts this rule here.
- For the runner's schema guard (`storage/schema-18#S6.2`) a successful import counts as an open: the baseline becomes `sqlite_master` of the imported file, upgrade included. A refused import keeps the baseline.
- **Output**: none; the step records `{}`.

### E2.3 Rejections

`{"import": {"error": <kind>}}`, from this closed list (`dsl.md` D7.3). After a rejection nothing has changed: the previous collection is open, with the same rows and files.

| Kind | Oracle | Meguri |
|---|---|---|
| `too_new` | Anki's import error for a package made by a newer version (an unknown `meta` version) | `TooNew` |
| `corrupt` | any other error Anki's collection package import raises before it replaces the collection: its "not a valid file" import error (a missing collection entry, a collection it cannot open or whose integrity check fails, an unsafe media name), a protobuf decode error, an archive that does not read as zip, an error while reading or decoding an entry | `Corrupt`, `SchemaUnsupported` |

- The oracle tells its two import errors apart through the backend's own translations (`oracle/AGENTS.md` rule for messages), never by literal text. Any other failure (the driver's own, the worker's) stays a run error.
- Meguri's `UnsupportedFormat` (a legacy package before U120), `InsufficientSpace`, `DiskFull` and `Io` are run errors. No scenario of this file imports a legacy-format package. Draft (U120, U124): Anki imports a legacy `.colpkg` (`package/container#P17`); until U124 gives Meguri's side, a scenario imports one only in a segment the oracle runs, as E2.2 says for a package with media.
- A v1-scheduler collection inside a package: Meguri refuses (`SchemaUnsupported`, recorded `corrupt`), Anki imports it. No scenario carries one; one that did would need a deviation like `storage-v1-scheduler-refused`.

### E2.4 Scenario rules

- A scenario with a collection-package `import` has no `setup`, starts with `open` (`storage-ops.md` P1), and neither creates nor names a logical id (no `add_note`, no `#nN` or `#cN` anywhere): an import replaces the notes those ids would name. Loaders refuse such a scenario with `invalid_scenario`.
- Day clock (`specs/sched/day-clock.md` D14.2): the scenario observes `timing` right after its `open`, and right after every `import` that succeeds, before the clock moves. Meguri's replace runs the day check at the swap; the oracle's reopen runs none until asked.

## E3. `package_listing` (observation kind)

The layout of a package: what an importer finds before it decodes anything.

- **Request** `{package_listing: {name: <file name>}}` (a package in the session directory, E2.1 `name` rules, `.colpkg` or `.apkg`) or `{package_listing: {fixture: <path>}}` (E2.1 `fixture` rules, ending in `.colpkg` or `.apkg`). At most one `package_listing` per observe step.
- **Value**:

  ```json
  {
    "entries": [{"name": "meta", "method": 0}, {"name": "collection.anki21b", "method": 0}, {"name": "collection.anki2", "method": 0}, {"name": "media", "method": 0}],
    "meta_version": 3,
    "sqlite": {"collection.anki2": {"ver": 11, "notes": 1, "cards": 1, "revlog": 0, "journal": "rollback", "opens": true}}
  }
  ```

  - `entries`: every central-directory entry in archive order, its name and its compression method number (0 stored, 8 deflated).
  - `meta_version`: field 1 of the `meta` entry read as `package/container#P3` says (an integer, 0 when missing); `null` when the archive has no `meta` entry; `"invalid"` when the entry does not decode.
  - `sqlite`: one member per entry whose content, after the zip method alone (no zstd), starts with the SQLite header (`package/container#P1`), keyed by the entry name. In a latest-format package that is the placeholder only; `collection.anki21b` is zstd and is not listed. Each holds:
    - `ver`: `col.ver` (integer), `notes`, `cards` and `revlog`: row counts, read from a scratch copy with plain SQLite, no collection open;
    - `journal`: `rollback` when header bytes 18 and 19 are both 1, `wal` when both are 2, `other` otherwise;
    - `opens`: true when the executor's own collection open accepts another scratch copy (oracle: Anki's collection open; Meguri: the open gate, `storage/open-and-connections#C2`, upgrade included) and `PRAGMA integrity_check` on it is `ok`; false on any failure.
- Never changes the scenario's collection, the package or the session directory; scratch copies live in the executor's scratch space and are removed. An archive that does not read as zip, and a raw SQLite entry whose `col`, `notes`, `cards` or `revlog` table does not read, are run errors; scenarios list only readable packages.
- In a live chain, a `package_listing` placed after `handoff: oracle` is Anki reading Meguri's package: its `opens` is Anki opening Meguri's placeholder.

## E4. Fixture packages

`conformance/fixtures/package/`, made once by the oracle maintainer (`oracle/fixtures/package/`) from the storage fixtures and Anki 26.9.3 in the oracle image; never by hand, by Meguri or from `local-data/`.

- **Placeholder.** Anki's own placeholder holds its stock note types (R1959), so no fixture carries it. Wherever a fixture has a `collection.anki2` entry, it holds the bytes of `storage/schema-11.anki2` (no stock note type, schema 11, rollback mode).
- **Archives the maker writes**: stored entries, entry times 1980-01-01 00:00:00, in the order given. "zstd of X" is one zstd frame (RFC 8878) holding X in raw blocks of at most 128 KiB, with the single-segment flag and the content size set and no checksum: a valid frame that needs no zstd library.
- **Backup entries** below are the entries of `backup/standard.colpkg`, each with its exact bytes.

| Fixture | Recipe |
|---|---|
| `package/backup/standard.colpkg` | Copy `storage/schema-18.anki2` to a scratch profile, open it with Anki 26.9.3 at 2030-01-14T12:00:00 UTC, make a forced backup with Anki's backup writer into a scratch folder, and take the package it wrote. Rewrite it with the same entries in the same order, each with its exact bytes, except `collection.anki2`, which becomes the placeholder. `collection.anki21b` and `media` stay Anki's own zstd frames. |
| `package/variants/schema-11-inside.colpkg` | `meta` `08 03`; `collection.anki21b` zstd of `storage/schema-11.anki2`; the placeholder; the backup's `media` |
| `package/variants/meta-version-4.colpkg` | the backup entries with `meta` = `08 04` |
| `package/variants/meta-version-0.colpkg` | the backup entries with an empty `meta` |
| `package/variants/meta-not-protobuf.colpkg` | the backup entries with `meta` = `ff` |
| `package/variants/no-collection-entry.colpkg` | the backup entries without `collection.anki21b` |
| `package/variants/collection-not-zstd.colpkg` | the backup entries with `collection.anki21b` = the raw bytes of `storage/schema-18.anki2` |
| `package/variants/not-a-zip.colpkg` | the 26 ASCII bytes `this is not a zip archive` and LF |
| `package/variants/corrupt-page.colpkg` | the backup entries with `collection.anki21b` = zstd of a copy of `storage/schema-18.anki2` in which the cell count of the root page of index `ix_cards_nid` (bytes 3 and 4 of its b-tree page header, big-endian) is 0. The maker checks on that copy with SQLite: the page is a leaf index page (type byte 10), `PRAGMA integrity_check` is not `ok`, `SELECT ver FROM col` gives 18. |
| `package/variants/schema-12.colpkg` | the backup entries with `collection.anki21b` = zstd of `storage/ver-12-delete.anki2` |
| `package/variants/media-traversal.colpkg` | the backup entries plus an entry `0` = zstd of the 3 bytes `abc`, placed before `media`; `media` = zstd of a media map with one entry: name `../x.jpg`, size 3, the 20-byte SHA-1 of `abc` |
| `package/variants/media-unnormalized.colpkg` | as `media-traversal`, name `con.jpg` |
| `package/variants/media-sha1-short.colpkg` | as `media-traversal`, name `x.jpg`, SHA-1 cut to its first 19 bytes |
| `package/variants/no-media-map.colpkg` | the backup entries without `media` |

The maker checks that no fixture holds stock note type text (the storage maker's `stock-check`, on every raw SQLite entry and on the collection of `backup/standard.colpkg` after Anki imports it into a scratch profile).

## E5. Scenarios and what runs them

| Scenario | Shows | Needs on Meguri |
|---|---|---|
| `export-import-roundtrip` (live, T1) | Meguri exports the standard collection; Anki lists the package (layout, placeholder that Anki opens) and imports it; the dump, `schema_objects`, integrity check and Check Database equal Anki's own round trip | `open`, `timing`, `db_dump`, `export.colpkg`, `handoff` |
| `export-listing` | Meguri's package layout and placeholder equal Anki's export | `open`, `timing`, `export.colpkg`, `package_listing` |
| `backup-listing` | the layout of Anki's backup | `open`, `package_listing` |
| `restore-backup` | edits, then the backup restored: Anki's rows back, with the R7 stamps | `open`, `timing`, `db_exec`, `import.colpkg`, `db_dump`, `schema_objects` |
| `upgrade-on-import` | a schema-11 collection in a latest-format package, upgraded on the staging copy | as `restore-backup` |
| `refused-meta`, `refused-entries`, `refused-collection`, `refused-media-map` | each refusal kind, collection unchanged | `open`, `timing`, `import.colpkg`, `db_dump` |
| `missing-media-map` | Meguri imports, Anki refuses (deviation `package-latest-missing-media-map`) | `open`, `timing`, `import.colpkg` |

- **Capabilities** (`dsl.md` D8). An `export` step needs the op `export` and the capability of its form: `export.colpkg` (E1), `export.apkg_legacy` (`model-ops.md` L6), `export.apkg` (E7, U122) or `export.apkg` and `export.apkg.legacy` (E8, U123: E8 is E7's form plus a key, so it needs both); a `colpkg` export with `media: true` also needs `export.colpkg.media` and one with `legacy: true` also needs `export.colpkg.legacy` (E9, U124). Capabilities compare by exact string: `export.apkg_legacy` (L6's whole-collection form) and `export.apkg.legacy` (`package/legacy-export#L14`'s deck form) are two capabilities that differ only in a separator, and the vocabulary scan's separator folding never applies to capability names. A collection-package `import` step needs `import` and `import.colpkg`; a deck-package one needs `import` and `import.apkg` (E6, U120). The observations `package_media` (E10) and `package_db_dump` (E11) are kinds of their own. Meguri declares `export`, `export.colpkg`, `import`, `import.colpkg` and the kind `package_listing` with U124a's adapter (`core/crates/meguri-testkit/src/exec/package.rs`). The oracle implements every stable form this file fixes (E1 to E3); E6 to E11 are draft, in neither executor yet.
- The oracle driver implements E1 to E3; the testkit model reads their shapes with the load-time rules above (E2.4 included). Every scenario has a golden, recorded with `tools/mg oracle record package/collection-package`.

## E6. `import`, deck package form (draft)

The deck package import of `package/apkg-import` (U120), as its sheet's Import runs it with the options given.

### E6.1 Argument

`{format: apkg, fixture: <path>, merge_notetypes, update_notes, update_notetypes, with_scheduling, with_deck_configs}`, or the same with `name: <file name>` in place of `fixture`: exactly one of `fixture` and `name`, every option, no other key.

- `fixture`: E2.1's rules, ending in `.apkg`. The deck-package fixtures live under `conformance/fixtures/package/apkg/` (`package/apkg-import#I18.1`), made as E4 says.
- `name`: a package in the session directory written by an earlier `export` step (E7, E8, `model-ops.md` L6), one before a handoff included; E1's name rule with `.apkg` in place of `.colpkg`. No such file: `unknown_ref`.
- The five options of `package/apkg-import#I5`, in its order: `merge_notetypes` (Merge note types), `update_notes` and `update_notetypes` (`if_newer`, `always` or `never`), `with_scheduling` (Import any learning progress) and `with_deck_configs` (Import any deck presets). The three others are booleans.

### E6.2 Effect

- Oracle: Anki's deck-package import of the file with those options, each update value mapped to Anki's update condition of that meaning, into the open session collection, its media folder and its media database. Nothing closes.
- Meguri: inspect, then apply (`package/apkg-import` I2, I14) with the five options, then close the session. When a merge has a preview to confirm (`ConfirmationRequired`, I8), the executor applies again at once with the token: the import runs as accepted, as schema changes do in scenarios (`notetype-ops.md` T9). A safety backup (I14.4) goes to the scenario's backups folder (`backup-ops.md` F1).
- On both sides the import is one undoable step, "Import" (`undo-ops.md` V4 `import`, `package/apkg-import#I14.3`), and writes the five config keys of I5.
- Notes and cards the import adds get no logical id, as the cards an edit adds (`note-ops.md` N3); `adopt` (`normalized-dump.md` N5) numbers them. Target notes keep theirs.
- Files the import writes are in the media folder (`media_dir`, `media-ops.md` F9).

### E6.3 Output

`{import: <log>}`, the log of `package/apkg-import#I13`, every member present:

```json
{"found_notes": 2, "new": [{"note": 1894701600123, "fields": ["sun", "soleil"]}], "updated": [{"note": "#n1", "fields": ["moon", "lune"]}], "duplicate": [], "conflicting": [], "missing_notetype": [], "missing_deck": [], "first_field_match": [], "empty_first_field": []}
```

- `found_notes`: an integer. Each list holds `{note, fields}` entries in I13's order.
- `note`: in `new`, `updated`, `duplicate` and `missing_deck`, the target note's logical id, or its real id as an integer when it has none (as `empty-cards-ops.md` Y1); in `conflicting` and `missing_notetype`, the package note's id as an integer. Meguri's `existing_id` (I13) is not observed.
- `fields`: I13's reduced field texts; the oracle takes them as Anki's log gives them.
- `first_field_match` and `empty_first_field` are always `[]` for a deck package.

### E6.4 Rejections

`{"import": {"error": <kind>}}`. After a rejection the collection, its media folder and its undo history are as they were.

| Kind | Oracle | Meguri |
|---|---|---|
| `too_new` | as E2.3: an unknown `meta` version | `TooNew` |
| `corrupt` | as E2.3, for Anki's deck-package import | `Corrupt`, `SchemaUnsupported`, `UnsupportedFile` |
| `notetype_kind_conflict` | Anki's refusal to merge a standard and a cloze note type (I8) | `NotetypeKindConflict` |

- The oracle tells them apart through the backend's own translations, as E2.3 does. Every other error on either side (`InsufficientSpace`, `DiskFull`, `Stale`, `SafetyBackupFailed`, `Io`, any other Anki error) is a run error.
- Where Meguri differs from Anki (`package/apkg-import#I17`), the scenario that shows it carries the deviation I17 names.

## E7. `export`, deck package form (draft)

The deck package export of `package/apkg-export` (U122), latest format.

- **Argument** `{format: apkg, name, limit, scheduling, presets, media}`, every key, no other key.
  - `name`: E1's rule, ending in `.apkg`.
  - `limit`: `collection`, `{deck: <name>}` (a deck by its stored human name; `unknown_ref` when none has it), `{notes: [#nN, ...]}` or `{cards: [#cN, ...]}` (non-empty). A reference to a note or card deleted since is passed by its real id, which `package/apkg-export#A2` ignores.
  - `scheduling`, `presets`, `media`: booleans, A2's options Include scheduling information, Include deck presets and Include media. Support older Anki versions is off (E8 turns it on).
- **Effect.** Writes the deck package (`package/apkg-export` A3 to A10) to the file `name` in the session directory, replacing a file of that name. The collection, the media folder and the undo history do not change.
  - Oracle: Anki's deck-package export with that limit and those options, the option for older clients off.
  - Meguri: the operation of A11 with the session directory as its output directory; since A11 publishes with `ExclusiveCreate`, the executor first removes a file of that name.
- **Output** `{notes: <count>}`: the number of notes in the package.
- **Rejections**: `invalid_ids` (A8 rule 1, an id more than a day ahead; Meguri `InvalidIds`) and `media_check_required` (A9.2 rule 5, a referenced file whose name Check Media would change; Meguri `MediaNeedsCheck`). The oracle tells them apart through the backend's own translations. A scenario gives a refused export a name that no earlier step wrote: after the refusal the session directory has no file of that name. Every other error is a run error.
- **Not observable**: the package's bytes. Observe it with `package_listing` (E3), `package_media` (E10) and `package_db_dump` (E11), or import it (E6). In a live chain it crosses a handoff with the session directory (`dsl.md` D9.2).

## E8. `export`, legacy deck package form (draft)

"Support older Anki versions" for a deck package (`package/legacy-export`, U123).

- **Argument** E7's, every key, plus `legacy: true`. The form `{format: apkg, name, legacy: true}` with no other key stays `model-ops.md` L6's whole-collection form; the two are told apart by their keys.
- **Effect.** As E7, but the package is the legacy 2 layout (`package/legacy-export` L2 to L7: `meta` version 2, `collection.anki21` at schema 11, the placeholder, raw media entries and a JSON media map). Oracle: Anki's deck-package export with the option for older clients on. Meguri: `package/legacy-export#L7`.
- **Output**, **rejections** and what is observable: as E7. Meguri's `LegacyConversion` and `VerificationFailed` (L11) are run errors.

## E9. `export`, collection package options (draft)

E1's form with the two options of `package/apkg-export#A14` (U124).

- **Argument** `{format: colpkg, name, media?, legacy?}`, at least one of the two present: `media` (Include media) and `legacy` (Support older Anki versions) are booleans, false when missing. A step with either key is this form, never E1's (E1). A scenario writes the options only when one is true; for a package with both off it uses E1's form.
- **Effect.** Writes the whole collection as a collection package with A14's writer for that combination, to `name` as E1 says:
  - `media` true, `legacy` false: the latest format with every regular file of the media folder, referenced or not (`package/container#P15`, `package/file-export#E7`);
  - `legacy` true: the legacy 2 layout of `package/legacy-export#L8`, with the media folder's files when `media` is true.
  - Oracle: Anki's collection package export with those options. It closes and reopens the collection as E1 says; scenarios observe no undo state across it.
  - Meguri: A14's writer on the live collection; nothing closes.
- **Output**: none (`{}`).
- **Rejections**: none. Every error on either side, Meguri's `LegacyConversion` and `VerificationFailed` (`package/legacy-export#L11`) included, is a run error; scenarios give the media folder only regular files whose names `package/container#P6` accepts.
- The package's bytes are not observable: observe it with E3, E10 and E11, or import it with E2 in a segment the oracle runs (E2.2, E2.3; the live chains of `package/apkg-export#A17` and `package/legacy-export#L14`). A scenario that observes the collection after a legacy export observes only the package, or carries the deviation of `package/legacy-export#L13`.

## E10. `package_media` (observation kind, draft)

The media list of a package: what an importer finds in its media map and numbered entries.

- **Request** `{package_media: {name: <file name>}}` or `{package_media: {fixture: <path>}}`, with E3's rules. At most one per observe step.
- **Format.** Read from the `meta` entry as `model-ops.md` L7 reads it: version 3 is the latest format, version 1 or 2, or no `meta` entry, a legacy one.
- **Value** `[{name, size, sha1, entry_size}, ...]`:
  - latest format (`package/container#P5`): one item per media map record, in map order; `name`, `size` and `sha1` (lower-case hex) as the record holds them; `entry_size` the byte count of zip entry `n` after its zstd frame is decoded, `null` when the archive has no entry `n`;
  - legacy format (`package/container#P17`): one item per map member, in ascending member number; `name` as the JSON stores it; `size` and `sha1` computed from entry `n`'s content after the zip method; `entry_size` equal to `size`; all three `null` when the archive has no entry `n`.
  - A package without a `media` entry gives `[]`.
- Anki writes media in an order of its own (`package/apkg-export#A16`), so scenarios that compare a package of each executor list this value under `compare.set` (`dsl.md` D5 allows it for an order a spec leaves to each executor).
- Never changes the collection, the package or the session directory. An archive that does not read as zip and a map that does not decode are run errors; scenarios list only readable packages.

## E11. `package_db_dump` (observation kind, draft)

The rows of the collection inside a package, without opening it as a collection.

- **Request** `{package_db_dump: {name: <file name>, tables?, raw?}}` or the same with `fixture: <path>` (E3's rules); `tables` and `raw` as `db_dump`'s (`observations.md` O8). At most one per observe step.
- **What is read.** The package's collection entry: `collection.anki21b` decoded from zstd in the latest format (`package/container#P4`); in a legacy one the entry `model-ops.md` L7 names. The executor writes it to a scratch copy, reads it with plain SQLite (no open gate, no upgrade) and removes the copy.
- **Value** `db_dump`'s value for that file (O8, `normalized-dump.md`; raw with `raw: true`, N4). A bare request reports every O8 table the file holds; a schema-11 file holds only `col`, `notes`, `cards`, `revlog` and `graves` (its other records are in `col`'s JSON columns, which `package_col` reads, `model-ops.md` L7). A listed table the file does not hold is a run error.
- Anki's package collection also holds rows Meguri's never writes (`package/apkg-export#A16`: its new collection's stock field and template rows, and `curModel`): a scenario that compares those tables, or `config`, carries A16's deviation, or lists `tables` without them.
- Never changes anything. A missing entry, an entry that does not decode, and a file SQLite cannot read are run errors.
