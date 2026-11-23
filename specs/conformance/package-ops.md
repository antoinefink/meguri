# Package ops and observations for scenarios

Spec: conformance/package-ops
Requirements: R1597, R1598, R1599, R1619, R1626 (stamps), R1627 (E2.2, U124), R1628, R2005, R2009 (format half)
Anki: 26.09.3
Scenarios: conformance/scenarios/package/collection-package/; planned, not yet written: conformance/scenarios/package/colpkg/ (E5 says what blocks them)

The scenario vocabulary for collection packages (U124a): the collection-package form of `export`, the collection-package forms of `import`, the observation `package_listing`, and the fixture packages under `conformance/fixtures/package/`. `export` and `import` belong to the package area (`dsl.md` D7.2); the forms this file does not fix stay draft for U120, U122, U123 and U124, and the legacy package form of `export` stays as `model-ops.md` L6 fixes it. The behavior observed is `specs/package/container.md` and `specs/engine/collection-replace.md`.

E6 to E11 fix the deck-package forms and options that U120, U122, U123 and U124 need (`package/apkg-import#I18.1`, `package/apkg-export#A17`, `package/legacy-export#L14`): the deck-package `import` form (E6), the latest and legacy deck-package `export` forms (E7, E8), the collection package export options (E9), and the observations `package_media` (E10) and `package_db_dump` (E11). E8 and E9 have stable typed scenario language; their execution adapters remain pending in this checkout (`dsl.md` D7.2). The oracle driver implements E6 to E11 (`oracle/driver/ops/apkg.py`, `ops/colpkg.py`, `observe/package_contents.py`), with the scenarios under `conformance/scenarios/package/apkg-export/`, `apkg-import/`, `notetype-merge/` and `legacy-export/`; E8 and E9 remain pending execution.

## E1. `export`, collection package form

- **Argument** `{format: colpkg, name: <file name>}`, with no other key. `name` is a file name of ASCII letters, digits, `.`, `_` and `-` that starts with a letter or digit and ends in `.colpkg`. E1 is exactly this form, without `media` and `legacy`; a step that carries either key is E9's options form (U124), even when the key is false.
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
- Meguri: the adapter picks its path from what inspection reads in the package. Before U124 only the first path exists (a legacy package is `UnsupportedFormat`, E2.3; a package with media is refused by P16's media-input refusal); once U124 lands:
  - a latest-format package whose media map has no entry, or that has no `media` entry (a collection-only package, the `collection-package/` scenarios, and `plain.colpkg` of the planned `colpkg/media-merge`): inspect (`package/container#P12`), then the verified replace with the reason `ColpkgImport`, as the product's collection-file import uses it (`engine/collection-replace#R1`); only a backup restore (`package/backup-triggers#T8`) uses `Restore`. U124a's adapter still passes `Restore`; U124 moves it to `ColpkgImport`;
  - a latest-format package whose map has at least one entry: the flow of `package/container#P16` with the reason `ColpkgImport`;
  - a legacy package (no `meta`, or version 1 or 2), with or without media: `package/container#P19` with the reason `ColpkgImport`.

  Each replace reopens the collection and restarts its day clock (`engine/lifecycle#L8`). Both reasons apply the same stamps (`engine/collection-replace#R7`) and set the publication fence, so the goldens do not tell the paths apart. P16's forced backup goes to the scenario's backups folder (`backup-ops.md` F1), which no `colpkg/` scenario observes.
- On success the collection holds the package's rows, upgraded when older, with `col.scm` and `col.mod` set to the current instant in milliseconds (`engine/collection-replace#R7`); the undo history is empty. A collection-only package leaves the media folder unchanged on both sides.
- Package with media (U124). Oracle: Anki writes each of the package's files into the media folder under its name, replacing a file of that name whose bytes differ (latest format: the SHA-1 its media database holds for the name after a folder scan; legacy: the file's size alone), and leaves every other file in place. Meguri: the merge of `package/container#P16` (latest) or `#P19` (legacy), with replace reason `ColpkgImport` (`engine/collection-replace#R1`), which also keeps the replaced bytes in the trash, so a scenario that replaces a file observes `media_trash` only with a deviation. Advance the clock before a `media_outside` step whose change the import must see (`media-ops.md` F12): Anki's scan finds a changed file by its time. The planned scenarios of `package/colpkg/` (U124's, E5, not yet written) import such packages on both executors and will stay inactive until U124's adapter lands; any other scenario imports one only in a segment the oracle runs (a live chain after `handoff: oracle`, `package/apkg-export#A17`, `package/legacy-export#L14`). The planned `colpkg/oracle-made-import` is the live chain the other way: Anki writes both formats with media and Meguri imports them, so Meguri's readers meet Anki's zstd frames, map encoding and JSON map spelling.
- For the runner's schema guard (`storage/schema-18#S6.2`) a successful import counts as an open: the baseline becomes `sqlite_master` of the imported file, upgrade included. A refused import keeps the baseline.
- **Output**: none; the step records `{}`.

### E2.3 Rejections

`{"import": {"error": <kind>}}`, from this closed list (`dsl.md` D7.3). After a rejection nothing has changed: the previous collection is open, with the same rows and files.

| Kind | Oracle | Meguri |
|---|---|---|
| `too_new` | Anki's import error for a package made by a newer version (an unknown `meta` version) | `TooNew` |
| `corrupt` | any other error Anki's collection package import raises before it replaces the collection: its "not a valid file" import error (a missing collection entry, a collection it cannot open or whose integrity check fails, an unsafe media name), a protobuf decode error, an archive that does not read as zip, an error while reading or decoding an entry, a media map naming a member the archive lacks (below) | `Corrupt`, `SchemaUnsupported` |

- The oracle tells its two import errors apart through the backend's own translations (`oracle/AGENTS.md` rule for messages), never by literal text. Any other failure (the driver's own, the worker's) stays a run error.
- Meguri's `UnsupportedFormat` (a legacy package before U124), `InsufficientSpace`, `DiskFull` and `Io` are run errors. Anki imports a legacy `.colpkg` (`package/container#P17`); Meguri's side is `package/container#P19` (U124). Only the planned `package/colpkg/legacy-import` and `package/colpkg/oracle-made-import` (E5) import one in a Meguri segment; any other scenario imports one only in a segment the oracle runs, as E2.2 says for a package with media.
- A media map that names a member the archive lacks (U124, fixture of E4.1) is `corrupt` too. Anki fails with its generic invalid-input error, not an import error, before it replaces the collection, having written no file for a one-entry map (with more entries, the files before the missing one may already be written, `package/container#P16.5`, so scenarios use a one-entry map). Before failing it opens its media database, which creates that file beside the collection when it is missing; no observation reads it. Given a media folder, that is the only invalid-input error of Anki's collection package import, so the oracle records the error kind as `corrupt` without reading its text; an invalid-input error of the deck-package import (E6.4) stays a run error. Meguri: `Corrupt` (`package/container#P16.1`). Scenario: the planned `package/colpkg/refused-media-member` (E5).
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

`conformance/fixtures/package/`, made once by the oracle maintainer (`oracle/fixtures/package/`) from the storage fixtures and Anki 26.9.3 in the oracle image (two deck packages with Anki 2.1.26, the storage maker's oldest pinned release); never by hand, by Meguri or from `local-data/`.

- **Placeholder.** Anki's own placeholder holds its stock note types (R1959), so no fixture carries it. Wherever a fixture has a `collection.anki2` entry, it holds the bytes of `storage/schema-11.anki2` (no stock note type, schema 11, rollback mode), except in the two legacy 1 packages (`apkg/legacy-1.apkg`, `apkg/v1-scheduler.apkg`), where `collection.anki2` is the package's collection.
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
| `package/apkg/meta-version-4.apkg` | `meta` `08 04`; `collection.anki21b` zstd of `storage/schema-11.anki2`; the placeholder; `media` zstd of no bytes |
| `package/apkg/legacy-traversal.apkg` | no `meta` (legacy 2): `collection.anki21` the raw bytes of `storage/schema-11.anki2`; the placeholder; an entry `0` = the 3 bytes `abc`; `media` the JSON map `{"0": "../x.jpg"}` |
| `package/apkg/legacy-map-not-json.apkg` | as `legacy-traversal` without the entry `0`, `media` = the 8 bytes `not json` |
| `package/apkg/not-a-zip.apkg` | as `variants/not-a-zip.colpkg` |
| `package/apkg/card-ids-ahead.apkg` | no `meta` (legacy 2): `collection.anki21` a copy of `storage/schema-11.anki2` whose card ids, in ascending order, become 1894960800000 (2030-01-18T10:00:00Z in ms) plus 0, 1, …, the review log's `cid` following; the placeholder; `media` the JSON map `{}` |
| `package/apkg/deck-tree.apkg` | as `card-ids-ahead`, the copy's decks `Lang` renamed `A::B` and `Lang::Verbs` renamed `F::x` instead (no deck `A` or `F`) |
| `package/apkg/v1-scheduler.apkg` | Anki 2.1.26 (the storage maker's legacy image) makes a v1-scheduler collection: note type `Pair` (`Front`, `Back`, one template `Forward`, its fields and template without ids), decks `Lang` and `Lang::Sub` with preset `Steps` (new delays 1 m and 1 d, lapse delays 10 m and 1 d), twelve notes, a media file `dot.png` one of them references; answers on day 0 (2026-09-01) and day 5 give review, learning (both queues) and relearning (both queues) cards; the filtered decks `Cram` (rescheduling, delays 5, 30, 60 m) and `Peek` (no rescheduling) each hold three cards, one of them answered there. 2.1.26's deck-package export of the whole collection with scheduling and media gives `collection.anki2` (schema 11), `0` and the JSON map `media`. Rewritten with the same entries, each with its exact bytes except that `col`'s JSON columns get their keys sorted (2.1.26 writes them in hash-map order) |
| `package/apkg/legacy-1.apkg` | the same export of the same collection without scheduling (all cards new, no review log, `marked` and `leech` removed by 2.1.26) |

The deck-package fixtures (`package/apkg/`) come from the same maker (`oracle/fixtures/package/apkg.py`, run by `make.sh` after the collection packages; the two 2.1.26 packages from `oldclient.py` in the storage maker's legacy image, with the clocks frozen and `random` seeded, so they too have the same bytes on every run). The maker checks that no fixture holds stock note type text (the storage maker's `stock-check`, on every SQLite entry of every package, raw or in one of its own frames, and on the collection of `backup/standard.colpkg` after Anki imports it into a scratch profile).

### E4.1 Media-map fixtures

Added on 2026-11-10 by the same maker (`oracle/fixtures/package/apkg.py` and the collection-package maker) with E4's archive rules; remaking the package fixtures left every older fixture's bytes unchanged.

| Fixture | Recipe | Scenario |
|---|---|---|
| `package/variants/media-member-missing.colpkg` | the backup entries with `media` = zstd of a media map with one entry: name `x.jpg`, size 3, the 20-byte SHA-1 of `abc`; no entry `0` | `package/colpkg/refused-media-member` (planned, E5; the fixture exists) |
| `package/apkg/media-subdir.apkg` | as `apkg/meta-version-4.apkg` with `meta` `08 03`, plus an entry `0` = zstd of the 3 bytes `abc` before `media`; `media` = zstd of a media map with one entry: name `a/b.jpg`, size 3, the SHA-1 of `abc` | `package/apkg-import/media-map-variants` |
| `package/apkg/legacy-subdir.apkg` | as `apkg/legacy-traversal.apkg` with `media` the JSON map `{"0": "a/b.jpg"}` | the same |
| `package/apkg/legacy-no-media-map.apkg` | as `apkg/legacy-traversal.apkg` without the entry `0` and without `media` | the same |
| `package/variants/media-absolute.colpkg` | as `media-traversal`, name `/x.jpg` | `package/collection-package/refused-media-map` (one more import step) |
| `package/apkg/legacy-absolute.apkg` | as `apkg/legacy-traversal.apkg` with `media` the JSON map `{"0": "/x.jpg"}` | `package/apkg-import/media-map-variants` (one more import step) |

The last two rows are not built yet: the fixture maker has not made them and no scenario step imports them. Once built, the import side refuses an absolute name directly, next to the `../x.jpg` and `a/b.jpg` cases (`package/container#P6`, and P17 for the legacy map). Until then the absolute-name import proof is open, and harvest row h434af44650c7 stays `deferred`.

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
| `colpkg/media-merge` (U124) | a latest package with media imported over an edited collection and changed folder: rows back with the R7 stamps; files whose bytes differ replaced, a removed one written, an equal and an unrelated one kept; then a collection-only import leaves the folder as it is (`package/container#P16.2`) | `open`, `timing`, `media_outside`, `db_exec`, `export.colpkg`, `export.colpkg.media`, `import.colpkg`, `db_dump`, `media_dir` |
| `colpkg/legacy-import` (U124) | a legacy package with media, upgraded on staging; a present file of the payload's length kept whatever its bytes (`package/container#P19`) | as `colpkg/media-merge`, with `export.colpkg.legacy` and `schema_objects` |
| `colpkg/oracle-made-import` (U124, live, T1) | Anki's latest and legacy packages with media, imported by Meguri over an edited collection and changed folder: the rows and files of `colpkg/media-merge` and `colpkg/legacy-import`, read from packages Anki wrote | `open`, `timing`, `media_outside`, `db_exec`, `import.colpkg`, `db_dump`, `schema_objects`, `media_dir`, `handoff` (Anki runs the exports) |
| `colpkg/refused-media-member` (U124) | a mapped member missing: `corrupt`, rows and folder unchanged (E2.3, E4.1) | `open`, `timing`, `media_outside`, `import.colpkg`, `db_dump`, `media_dir` |

- **Planned rows.** The four `colpkg/` rows are planned: no scenario file and no golden exists in this checkout yet, only the fixture `package/variants/media-member-missing.colpkg` (E4.1). They cannot land before U124's import adapter: the core suite pins the exact number of `package/` scenarios and requires each to load with its recorded golden, and they need E9's `media` and `legacy` export forms and the `ColpkgImport` paths of E2.2 on Meguri. Until they land, U124's oracle verification of `package/container` P16.2 and P19 is blocked on them.

- **Capabilities** (`dsl.md` D8). An `export` step needs the op `export` and the capability of its form: `export.colpkg` (E1), `export.apkg_legacy` (`model-ops.md` L6), `export.apkg` (E7, U122) or `export.apkg` and `export.apkg.legacy` (E8, U123: E8 is E7's form plus a key, so it needs both); a `colpkg` export with `media: true` also needs `export.colpkg.media` and one with `legacy: true` also needs `export.colpkg.legacy` (E9, U124). Capabilities compare by exact string: `export.apkg_legacy` (L6's whole-collection form) and `export.apkg.legacy` (`package/legacy-export#L14`'s deck form) are two capabilities that differ only in a separator, and the vocabulary scan's separator folding never applies to capability names. A collection-package `import` step needs `import` and `import.colpkg`; a deck-package one needs `import` and `import.apkg` (E6, U120). The observations `package_media` (E10) and `package_db_dump` (E11) are kinds of their own. Meguri declares `export`, `export.colpkg`, `import`, `import.colpkg` and the kind `package_listing` with U124a's adapter (`core/crates/meguri-testkit/src/exec/package.rs`), and `import.apkg` with U120's (same file; PR #172), whose `import` records the rejections `too_new`, `corrupt` and `notetype_kind_conflict` (E6.4). The oracle implements E1 to E3 and E6 to E11; E8 and E9 are stable typed language with execution pending. Changed 2026-11-23 (deck package import): Meguri's executor has E6 and none of E7 to E11 yet. It therefore runs the scenarios of `package/apkg-import/` and `package/notetype-merge/` that need no `export` step: `apkg-import/deck-tree`, `ids-cutoff`, `legacy-1`, `media-map-variants`, `refusals` and `v1-scheduler`, and `notetype-merge/by-name`, each matching its golden. Every other scenario of the two directories builds its package with an `export` step (E7, capability `export.apkg`; `legacy-2` also L6's `export.apkg_legacy`) and stays `pending` on Meguri (`dsl.md` D10) until U122 (and U123 for `legacy-2`).
- The testkit model reads E1 to E3's shapes with the load-time rules above (E2.4 included). Every `collection-package/` scenario of this table has a golden, recorded with `tools/mg oracle record package/collection-package`; the `colpkg/` ones (U124) are planned and will be recorded with `tools/mg oracle record package/colpkg` when they land, inactive until U124's adapter; the deck-package scenarios (E6 to E11) are recorded with `tools/mg oracle record <filter>` for `package/apkg-import`, `package/apkg-export`, `package/notetype-merge` and `package/legacy-export`.

## E6. `import`, deck package form

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
- Scenario rules. Anki's caches of note types, decks and presets survive `db_exec` (`model-ops.md` L1): a scenario that writes those rows runs `reopen` before an `import` or `export` reads them; the driver never reopens on its own, since that empties the undo history an export or a successful import keeps. Adding or removing a field or template empties Anki's undo history, so scenarios undo only plain edits (CSS, a front, the sort field) around an export and revert other edits with inverse edits. The new collection's stock note types are rows of `notetypes`, `fields` and `templates`: scenarios read their own note types with `db_exec`, `notetype_structure` or `legacy_json`, never by dumping those tables (R1959).

### E6.3 Output

`{import: <log>}`, the log of `package/apkg-import#I13`, every member present:

```json
{"found_notes": 2, "new": [{"note": 1894701600123, "fields": ["sun", "soleil"]}], "updated": [{"note": "#n1", "fields": ["moon", "lune"]}], "duplicate": [], "conflicting": [], "missing_notetype": [], "missing_deck": [], "first_field_match": [], "empty_first_field": []}
```

- `found_notes`: an integer. Each list holds `{note, fields}` entries in I13's order.
- `note`: in `new`, `updated`, `duplicate` and `missing_deck`, the target note's logical id, or its real id as an integer when it has none (as `empty-cards-ops.md` Y1); in `conflicting` and `missing_notetype`, the package note's id as an integer. Meguri's `existing_id` (I13) is not observed. A new note that comes back under the id of a note the scenario deleted is named by that note's logical id, which still names the id.
- `fields`: I13's reduced field texts; the oracle takes them as Anki's log gives them.
- `first_field_match` and `empty_first_field` are always `[]` for a deck package.

### E6.4 Rejections

`{"import": {"error": <kind>}}`. After a rejection the collection and its media folder are as they were. Undo differs (an exception to `dsl.md` D7.3): Anki refuses every package but one that does not read as zip inside its import transaction, whose rollback empties its undo history and study queues; Meguri refuses in its inspect phase and keeps them (`package/apkg-import` I14, I16). So a scenario observes `undo_status` after a refusal only when the refused file does not read as zip.

| Kind | Oracle | Meguri |
|---|---|---|
| `too_new` | as E2.3: an unknown `meta` version | `TooNew` |
| `corrupt` | as E2.3, for Anki's deck-package import; also a legacy media map that does not parse as JSON (Anki's JSON error) | `Corrupt`, `SchemaUnsupported`, `UnsupportedFile` |
| `notetype_kind_conflict` | Anki's refusal to merge a standard and a cloze note type (I8) | `NotetypeKindConflict` |

- The oracle tells them apart through the backend's own translations, as E2.3 does. Every other error on either side (`InsufficientSpace`, `DiskFull`, `Stale`, `SafetyBackupFailed`, `Io`, any other Anki error) is a run error.
- Where Meguri differs from Anki (`package/apkg-import#I17`), the scenario that shows it carries the deviation I17 names.

## E7. `export`, deck package form

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
- Anki's export computes the scheduler day, so in a collection without `rollover` it writes that key as any first day query does: scenarios set `rollover` in `setup.config` or observe `timing` first.

## E8. `export`, legacy deck package form

"Support older Anki versions" for a deck package (`package/legacy-export`, U123).

- **Argument** E7's, every key, plus `legacy: true`. The form `{format: apkg, name, legacy: true}` with no other key stays `model-ops.md` L6's whole-collection form; the two are told apart by their keys.
- **Effect.** As E7, but the package is the legacy 2 layout (`package/legacy-export` L2 to L7: `meta` version 2, `collection.anki21` at schema 11, the placeholder, raw media entries and a JSON media map). Oracle: Anki's deck-package export with the option for older clients on. Meguri: `package/legacy-export#L7`.
- **Output**, **rejections** and what is observable: as E7. Meguri's `LegacyConversion` and `VerificationFailed` (L11) are run errors.

## E9. `export`, collection package options

E1's form with the two options of `package/apkg-export#A14` (U124).

- **Argument** `{format: colpkg, name, media?, legacy?}`, at least one of the two present: `media` (Include media) and `legacy` (Support older Anki versions) are booleans, false when missing. A step with either key is this form, never E1's (E1). A scenario writes the options only when one is true; for a package with both off it uses E1's form.
- **Effect.** Writes the whole collection as a collection package with A14's writer for that combination, to `name` as E1 says:
  - `media` true, `legacy` false: the latest format with every regular file of the media folder, referenced or not (`package/container#P15`, `package/file-export#E7`);
  - `legacy` true: the legacy 2 layout of `package/legacy-export#L8`, with the media folder's files when `media` is true.
  - Oracle: Anki's collection package export with those options. It closes and reopens the collection as E1 says; scenarios observe no undo state across it.
  - Meguri: A14's writer on the live collection; nothing closes.
- **Output**: none (`{}`).
- **Rejections**: none. Every error on either side, Meguri's `LegacyConversion` and `VerificationFailed` (`package/legacy-export#L11`) included, is a run error; scenarios give the media folder only regular files whose names `package/container#P6` accepts.
- The package's bytes are not observable: observe it with E3, E10 and E11, or import it with E2 in a segment the oracle runs (E2.2, E2.3; the live chains of `package/apkg-export#A17` and `package/legacy-export#L14`), except in the planned `package/colpkg/` scenarios of E5, which will import it on both executors (E2.2). A scenario that observes the collection after a legacy export observes only the package, or carries the deviation of `package/legacy-export#L13`, until a successful collection-package import replaces the collection with the package's: from then on the collection is the package's on both sides, and observations are free again.

## E10. `package_media` (observation kind)

The media list of a package: what an importer finds in its media map and numbered entries.

- **Request** `{package_media: {name: <file name>}}` or `{package_media: {fixture: <path>}}`, with E3's rules. At most one per observe step.
- **Format.** Read from the `meta` entry as `model-ops.md` L7 reads it: version 3 is the latest format, version 1 or 2, or no `meta` entry, a legacy one. Any other version (0, 4, a negative one) and a `meta` entry that does not decode as `package/container#P3` reads it are `invalid_scenario`, as L7 makes an unknown version.
- **Value** `[{name, size, sha1, entry_size}, ...]`:
  - latest format (`package/container#P5`): one item per media map record, in map order; `name`, `size` and `sha1` (lower-case hex) as the record holds them; `entry_size` the byte count of zip entry `n` after its zstd frame is decoded, `null` when the archive has no entry `n`;
  - legacy format (`package/container#P17`): one item per map member, in ascending member number; `name` as the JSON stores it; `size` and `sha1` computed from entry `n`'s content after the zip method; `entry_size` equal to `size`; all three `null` when the archive has no entry `n`.
  - A package without a `media` entry gives `[]`.
- Anki writes media in an order of its own (`package/apkg-export#A16`), so scenarios that compare a package of each executor list this value under `compare.set` (`dsl.md` D5 allows it for an order a spec leaves to each executor).
- Never changes the collection, the package or the session directory. An archive that does not read as zip and a map that does not decode are run errors; scenarios list only readable packages.
- Oracle: the image has no zstd library, so the driver decodes zstd entries (the map, numbered entries, E11's `collection.anki21b`) with Anki's own decoder, through a collection package import of a scratch package into a scratch profile; nothing of the scenario is involved.

## E11. `package_db_dump` (observation kind)

The rows of the collection inside a package, without opening it as a collection.

- **Request** `{package_db_dump: {name: <file name>, tables?, raw?}}` or the same with `fixture: <path>` (E3's rules); `tables` and `raw` as `db_dump`'s (`observations.md` O8). At most one per observe step.
- **What is read.** The package's collection entry: `collection.anki21b` decoded from zstd in the latest format (`package/container#P4`); in a legacy one the entry `model-ops.md` L7 names. The format comes from the `meta` entry as in E10, with E10's `invalid_scenario` for any other version or an undecodable `meta`. The executor writes it to a scratch copy, reads it with plain SQLite (no open gate, no upgrade) and removes the copy.
- **Value** `db_dump`'s value for that file (O8, `normalized-dump.md`; raw with `raw: true`, N4). A bare request reports every O8 table the file holds; a schema-11 file holds only `col`, `notes`, `cards`, `revlog` and `graves` (its other records are in `col`'s JSON columns, which `package_col` reads, `model-ops.md` L7). A listed table the file does not hold is a run error.
- Anki's package collection also holds rows Meguri's never writes (`package/apkg-export#A16`: its new collection's stock field and template rows, and `curModel`): a scenario that compares those tables, or `config`, carries A16's deviation, or lists `tables` without them.
- Never changes anything. A missing entry, an entry that does not decode, and a file SQLite cannot read are run errors.
