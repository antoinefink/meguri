# Package ops and observations for scenarios

Spec: conformance/package-ops
Requirements: R1597, R1598, R1599, R1619, R1626 (stamps), R1628, R2005, R2009 (format half)
Anki: 26.09.3
Scenarios: conformance/scenarios/package/collection-package/

The scenario vocabulary for collection packages (U124a): the collection-package form of `export`, the collection-package forms of `import`, the observation `package_listing`, and the fixture packages under `conformance/fixtures/package/`. `export` and `import` belong to the package area (`dsl.md` D7.2); the forms this file does not fix stay draft for U120, U122, U123 and U124, and the legacy package form of `export` stays as `model-ops.md` L6 fixes it. The behavior observed is `specs/package/container.md` and `specs/engine/collection-replace.md`.

## E1. `export`, collection package form

- **Argument** `{format: colpkg, name: <file name>}`, with no other key. `name` is a file name of ASCII letters, digits, `.`, `_` and `-` that starts with a letter or digit and ends in `.colpkg`. U124 adds the options for media and for older clients; each must default to the behavior below.
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
- On success the collection holds the package's rows, upgraded when older, with `col.scm` and `col.mod` set to the current instant in milliseconds (`engine/collection-replace#R7`); the undo history is empty. A collection-only package leaves the media folder unchanged on both sides (U124 adds packages with media).
- For the runner's schema guard (`storage/schema-18#S6.2`) a successful import counts as an open: the baseline becomes `sqlite_master` of the imported file, upgrade included. A refused import keeps the baseline.
- **Output**: none; the step records `{}`.

### E2.3 Rejections

`{"import": {"error": <kind>}}`, from this closed list (`dsl.md` D7.3). After a rejection nothing has changed: the previous collection is open, with the same rows and files.

| Kind | Oracle | Meguri |
|---|---|---|
| `too_new` | Anki's import error for a package made by a newer version (an unknown `meta` version) | `TooNew` |
| `corrupt` | any other error Anki's collection package import raises before it replaces the collection: its "not a valid file" import error (a missing collection entry, a collection it cannot open or whose integrity check fails, an unsafe media name), a protobuf decode error, an archive that does not read as zip, an error while reading or decoding an entry | `Corrupt`, `SchemaUnsupported` |

- The oracle tells its two import errors apart through the backend's own translations (`oracle/AGENTS.md` rule for messages), never by literal text. Any other failure (the driver's own, the worker's) stays a run error.
- Meguri's `UnsupportedFormat` (a legacy package before U120), `InsufficientSpace`, `DiskFull` and `Io` are run errors. No scenario of this file imports a legacy-format package.
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

- **Capabilities** (`dsl.md` D8). An `export` step needs the op `export` and the capability of its form: `export.colpkg` (E1) or `export.apkg_legacy` (`model-ops.md` L6); the package units add theirs (`export.apkg`, U122; the colpkg options, U124). A collection-package `import` step needs `import` and `import.colpkg`. Meguri declares `export`, `export.colpkg`, `import`, `import.colpkg` and the kind `package_listing` with U124a's adapter (`core/crates/meguri-testkit/src/exec/package.rs`). The oracle implements every form this file fixes.
- The oracle driver implements E1 to E3; the testkit model reads their shapes with the load-time rules above (E2.4 included). Every scenario has a golden, recorded with `tools/mg oracle record package/collection-package`.
