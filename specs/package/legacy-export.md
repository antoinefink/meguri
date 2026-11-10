# Legacy package export ("Support older Anki versions")

Spec: package/legacy-export
Requirements: R1619 (legacy part), R1625 (legacy option), R1618 (the legacy option of `.apkg` export), R1481 (caller side); supports R1464, R1466, R1922, R2004, R2013, R2015
Anki: 26.09.3
Scenarios: conformance/scenarios/package/legacy-export/ (all recorded; L14 lists them)

What Meguri writes when the user turns on "Support older Anki versions" for a deck package (`.apkg`, U123) or a collection package (`.colpkg`, U124): the legacy 2 container, a schema-11 collection in it, raw media entries and a JSON media map. The selection, scrub and media list of a deck package are `package/apkg-export` A2 to A9; the whole-collection media inventory is `package/file-export#E7`; the schema-11 copy is `storage/schema11-downgrade` (D1 to D11); reading these packages is `package/container#P17`. This spec adds the container, the order of steps around the downgrade, the preflight, the verification and the errors. The scenario vocabulary is `specs/conformance/package-ops.md` and `specs/conformance/model-ops.md` L6 and L7.

## L1. What the option changes

- Offered with every `.apkg` export (A2's table) and every `.colpkg` export (A14), from every entry point, first-use default off, remembered per package kind as A2 says.
- It changes three things only: the package format (L2), the schema of the collection inside (11 instead of 18, L4) and the encoding of the media entries and map (L5). With the option on and off, a deck package holds the same notes, cards, decks, note types, presets, review log, config keys and media files, and a collection package holds the whole collection and the same media inventory.
- No content of the collection forces the option on or blocks it. FSRS presets and parameters, per-deck desired retention, newer note type kinds and options, Easy Days, `other` members and unknown config keys all go out in the schema-11 objects (`model/schema11-json` S4 to S12); what schema 11 has no place for is lost in the file only (`storage/schema11-downgrade#D6`). Anki 26.9.3 has no check of its own either: its option only selects the legacy format.
- Decided 2026-10-27: Meguri adds no warning or confirmation for the option beyond its label (wording U160/U162). The export never changes the collection, so what the file cannot carry is never lost from the collection (data safety holds); the label already says the file is for older versions.
- The live collection, its `-wal`, the media folder, the media journal, backups and sync state never change (`package/file-export#E1`, `storage/schema11-downgrade#D1`).

## L2. Format

The option always writes the legacy 2 format of `package/container#P1`. Anki 26.9.3 writes the same for both package kinds.

| Package | Option off | Option on |
|---|---|---|
| `.apkg` | latest (`meta` version 3, A10) | legacy 2 (`meta` version 2) |
| `.colpkg` | latest (P11 or P15, A14) | legacy 2 (`meta` version 2) |

- Meguri never writes legacy 1 (no `meta`, schema-11 `collection.anki2`). Anki 26.9.3 has no writer for it.
- Readers find the format from `meta` (`package/container#P3`): version 2 names `collection.anki21`. Clients that predate `meta` find `collection.anki21` by name (P3, last rule); clients older still read `collection.anki2` and find the placeholder (L3).

## L3. Entries

In this order, one central-directory record each, with no other entry:

| # | Name | Method | Content |
|---|---|---|---|
| 1 | `meta` | 0 (stored) | exactly the two bytes `08 02` (field 1 = 2, `package/container#P3`) |
| 2 | `collection.anki21` | 8 (deflated) | the schema-11 collection file, raw (L4) |
| 3 | `collection.anki2` | 0 | the placeholder of `package/container#P7`, unchanged |
| 4 … n+3 | `0`, `1`, … `n-1` | 0 | the n media files, raw, in L5 order |
| n+4 | `media` | 0 | the JSON media map (L5) |

- Methods are those of Anki 26.9.3's legacy writer, so `package_listing` (`package-ops.md` E3) of Meguri's package equals Anki's. The deflate level is free (Meguri uses 6); readers depend only on the method.
- Media entry names are the decimal position without leading zeros, as in P5.
- Zip64 records are written when an entry or the archive needs them (an entry or offset of 4 GiB or more, more than 65,535 entries). Entry times, the "version made by" fields, data descriptors and the archive comment are free; no entry is encrypted. Every entry's CRC-32 is that of its uncompressed content.
- The placeholder is P7's, also in a legacy package: Anki 26.9.3 writes its own placeholder there too. A client older than legacy 2 reads it and is told to update; P7.1's text names a version that reads legacy 2 as well.

## L4. The collection entry

- The content is a schema-11 copy made by `storage/schema11-downgrade`, byte for byte as D8 verified it: `col.ver` 11, the JSON columns of D3.1, D4's objects, rollback-journal mode (header bytes 18 and 19 are 1).
- **`.apkg`**: the source is the closed package collection that `package/apkg-export#A4` builds in staging (A11 step 5), the closed-file source of D1. It is built exactly as with the option off, A5 to A7 included; only its closing differs: since this file is never packed, its journal mode is free (D2 reads either). The downgrade's destination is a new name in the same staging directory (`legacy.anki2`). Anki 26.9.3 builds its package collection the same way and downgrades it as it closes it.
- **`.colpkg`**: the source is the open collection (D1), snapshotted by D2 on the maintenance reader. This is the export's only snapshot: no other `VACUUM INTO` runs, and the copy holds every commit made before its first read (R1625, `package/file-export#E2`). The destination is `legacy.anki2` in a fresh P9 staging directory. Anki 26.9.3 instead closes its collection, downgrades the live file in place, packs it and reopens it (L13).
- The entry is the copy's bytes deflated; SHA-256 and length of the uncompressed bytes are computed while deflating, for L10.

## L5. Media entries and the JSON map

**Which files.**

- `.apkg`: A9's list, unchanged: the same candidates, the same refusals (A9.2) and the same name order.
- `.colpkg` with Include media on: `package/file-export#E7.1`'s inventory, unchanged (every regular file of the media folder, its names and its order). Off: no media file.

Each file is captured as an immutable staging copy first (A11 step 8 for `.apkg`, `package/file-export#E7.2` for `.colpkg`), with the same change checks; entries are written from the copies.

**Entries.** Entry `i` holds the exact bytes of the `i`-th file, stored (no zstd, no deflate). A zero-byte file has a zero-byte entry and its map member.

**Map.** The `media` entry holds one JSON object (RFC 8259) in UTF-8, written compact (no white space), with one member per media entry, in ascending entry number:

- key: the entry number in decimal without leading zeros, as a JSON string;
- value: the file's name as a JSON string, the NFC name of the media list (A9.2 rule 6, E7.1), which is always a safe name (P6). No two members have the same name: A9.1 counts names equal after NFC once, and E7.1 refuses duplicates after NFC, so P17's reread (L10 check 4) never meets two equal kept names.

Strings are written with only `"`, `\` and the characters U+0000 to U+001F escaped (`\"`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`, else `\u00XX` with lower-case hex); every other character, `/` and non-ASCII included, is written as itself in UTF-8. This is how Anki 26.9.3 writes a JSON string.

| Files | `media` entry |
|---|---|
| none (option off, nothing selected, empty folder) | `{}` |
| `a.mp3`, `日本.png` | `{"0":"a.mp3","1":"日本.png"}` |

The map has no size and no SHA-1; a reader hashes the payload when it needs one (P17).

## L6. Limits

The writer checks these before any media file is copied (the copy's own size once D8 has verified it, which is before capture in L7 and L8; the ratio after deflating), and refuses with `LimitExceeded{what}` (A12), so that every package it writes passes Meguri's reader (`package/container#P8`, `#P17`):

- at most 400,000 zip entries (so at most 399,996 media files);
- each media file at most 4,294,967,295 bytes (`MediaTooLarge{name}`, as A9.2 and E7.3: one rule for both formats, and Anki 26.9.3's zip writer cannot write an entry of 4 GiB or more);
- the map's encoded size at most 64 MiB;
- the schema-11 copy at most 8 GiB, and every entry L10 decodes together at most 16 GiB: the copy + all media files + the map's encoded size + the placeholder's size + 2 (`meta`), since P8's 16 GiB counts every entry the reread reads;
- the deflated collection entry at least one thousandth of the copy's size when the copy is larger than 16 MiB (P8's ratio rule, which P17 applies to a deflated member). This is checked after deflating; only a copy made almost entirely of one repeated byte can fail it.

## L7. The `.apkg` operation

`package/apkg-export#A11` with these differences:

- **Steps 1 to 5** unchanged: admission, snapshot and selection, A8's checks, preflight 1, build.
- **Step 6, media**: A9.2 to an inventory, then L6's limits.
- **Step 7, preflight 2**: L9's check 1 instead of A11's preflight 2 table.
- **Step 7a, convert** (new): `storage/schema11-downgrade` from the closed built file to `legacy.anki2` in the same staging directory. Step 7 covers its space (D2 step 3). Once D8 has verified the copy, the built file may be removed; then L6's copy limits and L9's check 2.
- **Step 8, capture and write**: capture as A11 says, then the legacy writer (L3 to L5) writes the package into the temporary package file (P9, purpose `export`).
- **Step 9, verify and publish**: L10, then A11's publication (`ExclusiveCreate` into the operation's output directory) and E2's generation recheck.

- Progress stages: A11's, with `converting` (indeterminate) between `building` and `media`.
- Output, counts, suggested name (`.apkg`), cancellation, temporary files, concurrency: as A11. A cancel during step 7a stops the downgrade as D10 says.

## L8. The `.colpkg` operation (U124)

The whole-collection export of `package/file-export` with the option on:

1. **Admission** as E2 (review flush, source generation and admission sequence, one active export per flow).
2. **Media inventory**: with Include media on, E7.1's listing (names and sizes; nothing copied yet), then L6's limits on it. Off: no file.
3. **Preflight**: L9's check 1.
4. **Convert**: `storage/schema11-downgrade` from the open collection to `legacy.anki2` in a fresh P9 staging directory (L4). E2's engine and maintenance reader leases are held as E2 says, through writing and verification; the writer keeps committing. Once D8 has verified the copy: L6's copy limits and L9's check 2.
5. **Capture**: E7.2's capture of the listed files, with its change checks.
6. **Write**: the legacy writer (L3 to L5) into the temporary package file (P9, purpose `export`).
7. **Verify and publish**: L10, then E3's owned artifact (`ExclusiveCreate`) and E2's generation recheck, with E7.2's final inventory check before Ready.

- Progress: E2's stages, with `converting` (indeterminate) in place of `snapshot`, one stage for the whole downgrade call (D2 to D8, step 4): the downgrade reports no stage of its own (D10 has only cancellation checks), so the operation needs no callback from it.
- The E2 to E6 job, artifact, destination, cancel and restart rules apply unchanged; so does E3's suggested `.colpkg` name.
- The collection stays open and usable throughout. Nothing closes it, and no undo history is lost.
- Like every `.colpkg`, it is the whole collection as it is: scheduling, presets, review log, graves, the default deck, every config key (A14). A6 never applies.

## L9. Preflight (R2015)

Two checks, each one per file system, with checked arithmetic (overflow refuses with `InsufficientSpace`). When the two file systems are one, the sums add up.

Let `S` be the size of the downgrade's source: for `.apkg` the built file (`snapshot.anki2`, known after A11 step 5); for `.colpkg` `collection.anki2` plus its `-wal`. Let `C` be the exact size of the schema-11 copy once D8 has verified it. Let `M` be the sum of the media files' sizes, `n` their number, `J` the map's encoded size and `H` the placeholder's size. The output need of a copy of size `x` is `out(x)` = `x` + `x`/1000 + 64 KiB (the deflated entry) + `H` + `M` + `J` + 512 × (`n` + 4) + 64 KiB (headers, central directory, zip64 records).

**Check 1**, after the media list is known and before the downgrade writes a file:

| File system | `needed` in bytes |
|---|---|
| the collection's (staging) | `S` + 4 MiB (the copy, D2) + `M` (the captured copies) |
| the output's (temporary package file) | `out(S)` |

**Check 2**, after D8 has verified the copy and before any capture or write, with the copy in place:

| File system | `needed` in bytes |
|---|---|
| the collection's (staging) | `M` |
| the output's (temporary package file) | `out(C)` |

- For `.apkg`, A11's preflight 1 still runs before the build; check 1 is its preflight 2, and check 2 follows step 7a.
- Check 1 is an estimate: the copy can be larger than its source (D3 puts the note type, deck, preset and config JSON in `col`, and how D3 reaches its end state is free), so a disk that fills during the downgrade fails it with `DiskFull` (L11) and the usual cleanup. Check 2 uses the copy's exact size.
- Deflate output never exceeds `C` + `C`/1000 + 64 KiB, so once check 2 has passed, a write that still fails for space is `DiskFull` (P10).

## L10. Verification before publication

Before the rename, close every write handle and reread the temporary package file with Meguri's reader (`package/container#P2`, `#P3`, `#P8`, `#P17`). The expected values are those recorded while writing, never the staging files read again:

1. Exactly L3's entries, in that order, with those names and methods; `meta` is `08 02`; every CRC-32 matches.
2. `collection.anki21` inflates to exactly the copy: the length and SHA-256 recorded in L4. It starts with the SQLite header and its header bytes 18 and 19 are 1.
3. `collection.anki2` starts with the SQLite header.
4. `media` parses under P17's map rules into exactly the written members: the same numbers and names, in the same order.
5. Each numbered entry has the length and SHA-1 recorded when its staging copy was captured.
6. P8's limits hold during the reread.

Any failure: `VerificationFailed{check}` and the failure path of A12. Meguri never publishes a package its own reader refuses. Publication follows: `storage/durability-and-files#F4`'s sequence (full sync, these checks, rename, directory sync), then the A11 or E3 artifact rules.

## L11. Errors

The errors of `package/apkg-export#A12` (`.apkg`), or for `.colpkg` the outcomes `package/file-export` states, under A12's names: `NoCollection` and `CollectionUnavailable` (E1: no open Ready collection, a failed or exclusive-busy state); `InsufficientSpace{needed, available}` (here L9's checks) and `DiskFull` (E2, P10); `Stale` (E2's generation recheck before Ready); `Cancelled` (E2, E5); `MediaChanged{name}` (E7.2's changed-source result, including the final inventory check); `MediaUnreadable{name}` (E7.2's unreadable entry, link, special file or changed binding); E7.1's refusals: `MediaNeedsCheck{name}` for a name that cannot be represented safely (invalid UTF-8, with `name` its bytes decoded with U+FFFD for each invalid sequence; an unsafe name under P6; a non-NFC name off Apple platforms; two names equal after NFC), whose message suggests Check Media (U130), and `MediaUnreadable{name}` with `name` `collection.media` for a media folder that is a link, is replaced during capture or cannot be safely inventoried; `LimitExceeded{what}` and `MediaTooLarge{name}` (E7.2's walk limits, E7.3); `Io` (any other file-system failure). Plus:

| Error | When |
|---|---|
| `LegacyConversion{error}` | the downgrade refused the collection: `UnconvertibleConfig` (with the key) or `UnreadableRecord` (with the table and key) of `storage/schema11-downgrade#D8`. The message suggests exporting without "Support older Anki versions" and running Check Database (wording U160/U162). For `.apkg`, an unreadable source record is refused before this, by `package/apkg-export#A8` rule 4 (`NeedsCheckDatabase{reason: UnreadableRecord{table, id}}`), as with the option off, so both formats agree; there `UnreadableRecord` from D8 means a fault in the built file. |
| `VerificationFailed{check}` | A12's kind: L10, or D8's own `VerificationFailed` |
| `LimitExceeded{what}`, `MediaTooLarge{name}` | L6 |

The downgrade's other errors map as follows: `InsufficientSpace` during the call is `DiskFull` (the preflight passed, P10); `Cancelled` is `Cancelled`; `Io`, `SourceNotSchema18` and `DestinationIsSource` are `Io` with their detail (the operation never passes such a source or destination, so they mean a fault).

Every error leaves no artifact, no staging directory and no temporary file, and changes nothing else. The live collection is never written (D1).

## L12. What an importer finds

- `meta` version 2; `collection.anki21`, a schema-11 collection whose `col.ver` is 11 and whose JSON columns hold the objects of `model/schema11-json` S12; Anki 26.9.3's importer, and Meguri's (P17), upgrade it on their staging copy.
- In a deck package: the A3 selection, with A5 to A7 applied, plus `crt` and `creationOffset` (A4) in `col.crt` and in `conf`, so the importer's day shift (R1609) is the same as with the latest format.
- The importer reads names from the JSON map (P17 normalizes them; Meguri's are already safe) and takes sizes and hashes from the payloads. For `.colpkg` import, the legacy media merge compares sizes (U124).
- What upgrading the schema-11 copy gives back, and what it cannot carry, is D6 and D7.

## L13. Differences from Anki

| Difference | Observable through |
|---|---|
| A16 of `package/apkg-export` applies to a legacy deck package, except its first row's field and template rows: the downgrade drops those tables in both. Its `curModel` remains: Anki's `conf` names a deleted note type, Meguri's has no `curModel`. | `package_col` of `conf`; no import reads it |
| JSON map members in ascending entry number; Anki writes them in no set order | the map's bytes only (readers parse it) |
| `.colpkg`: Meguri copies from a snapshot and never closes, downgrades or reopens the live collection. Anki 26.9.3 downgrades its live collection in place and upgrades it again when it reopens, so after its export the live collection shows D7's differences (unknown protobuf fields dropped, tag `collapsed` and `config` reset, config `usn` and `mtime_secs` 0, preset values outside the read bounds replaced) and the undo history is empty. | a dump of the live collection after a legacy `.colpkg` export |
| A collection entry of 4 GiB or more gets zip64 records; Anki's zip writer fails on it | synthetic tests only |
| `LegacyConversion` names the key or record that cannot be converted; Anki fails with its generic error | error kind only |
| Deflate output, entry times and the placeholder's ids differ | the bytes only |

No ported scenario observes any of them. A scenario that observes `conf` of a legacy deck package (`package_col`) needs a `conformance/deviations.yaml` entry for `curModel`, and one that observes the live collection after a legacy `.colpkg` export needs one for the third row, until a successful collection-package import replaces that collection (`conformance/package-ops#E9`; `package/colpkg/legacy-import` and `oracle-made-import` observe only after their import).

## L14. Scenarios and tests

Verification of U123 also includes `conformance/scenarios/model/schema11/apkg-notetypes` (recorded; `export` legacy form `conformance/model-ops#L6` and `package_col` `conformance/model-ops#L7`), on Meguri's executor on the macOS and Linux lanes.

Status (2026-11-10): every scenario of the table below is recorded, `unknown-keys-roundtrip` (added 2026-11-10, inactive) included; the oracle driver implements E8 and E9, which stay draft until Meguri's typed model reads them (`conformance/dsl#D7.2`). `apkg-listing` and `apkg-columns` use only ops and observations that exist in the runner and the oracle driver (`export` legacy form, `conformance/model-ops#L6`; `package_col`, `conformance/model-ops#L7`; `package_listing`, `conformance/package-ops#E3`). The others use these forms of `specs/conformance/package-ops.md`, which the oracle implements and Meguri's executor does not yet:

- `export`, deck-package form with `legacy: true`: `conformance/package-ops#E8` (capabilities `export.apkg` and `export.apkg.legacy`).
- `export`, collection-package options `{format: colpkg, name, media?, legacy?}`: `conformance/package-ops#E9` (capability `export.colpkg.legacy`, and `export.colpkg.media` with media).
- `import`, deck-package form `conformance/package-ops#E6`, and E2's `colpkg` import of a session package (exists), for the live chains.
- `package_media`: `conformance/package-ops#E10`; for a legacy package, the JSON map's members in ascending number, `size` and `sha1` computed from each numbered entry, `entry_size` equal to `size`.
- `package_db_dump`: `conformance/package-ops#E11`; for a legacy package, the raw `collection.anki21`.

Scenarios to record (`conformance/scenarios/package/legacy-export/`):

| Scenario | Shows | Needs |
|---|---|---|
| `apkg-listing` | a collection with notes of two note types, decks, a filtered deck and review log, exported with `conformance/model-ops#L6`'s legacy form: `package_listing` equals Anki's (entries `meta` 0, `collection.anki21` 8, `collection.anki2` 0, `media` 0; `meta_version` 2; `collection.anki21` `ver` 11, rollback, the counts, `opens` true; the placeholder) | existing ops; recording |
| `apkg-columns` | the same export: `package_col` of `ver`, `decks`, `dconf` and `tags` equals Anki's (D3.1 through A3 to A7: the default deck, a filtered deck, both presets, `tags` `{}`) | existing ops; recording. Adding `conf` needs the `curModel` deviation (L13) |
| `apkg-options` | legacy deck export of one deck without scheduling, presets on: `package_db_dump` of `notes`, `cards`, `revlog` and `graves` equals Anki's (A6's scrub in the schema-11 file), and `package_col` of `ver`, `decks`, `dconf` and `tags` | new `export` form, `package_db_dump` |
| `apkg-media` | A17's `media-selection` source, legacy: `package_media` equals Anki's set (names, sizes, SHA-1), and `package_listing` shows the entries stored | new `export` form, `package_media` |
| `apkg-oracle-import` (live, T1) | Meguri exports the legacy deck package of deck `A` with subdecks, scheduling and presets, then deletes `A` and its subdecks; after the handoff Anki imports the package into that collection with scheduling and presets; the dump (graves of the deletion included) equals Anki's own legacy export, deletion and import of the same deck | new `export` and `import` forms |
| `colpkg-listing` | legacy `.colpkg` with media off, then with media on and two files: `package_listing` equals Anki's (raw media entries, `media` stored) | new `colpkg` form |
| `unknown-keys-roundtrip` (added 2026-11-10) | a note type with add-on members in the note type, a field and the template (ids pinned), exported with `conformance/model-ops#L6`'s legacy form, removed, and imported back: `legacy_json` and the stored records hold every member (R1464); `package_col` of `models` shows them in the package | existing ops |
| `colpkg-oracle-import` (live, T1) | Meguri's legacy `.colpkg` with media; Anki removes the two files, imports the package; collection dump and media folder equal the source's, except D7's documented fields (here every `config` row's `usn` and `mtime_secs` 0 and `val` in compact form) and `col.scm` and `col.mod`, which the import sets to its own instant (`conformance/package-ops#E2.2`, R1626) | new `colpkg` form |

Goldens are recorded with `tools/mg oracle record package/legacy-export` at the pin of `conformance/oracle.json`. A scenario that observes the collection after a legacy `.colpkg` export observes nothing but the package (L13), or carries the deviation, until a successful collection-package import replaces the collection with the package's; the `package/colpkg/` scenarios import such a package on both executors (`conformance/package-ops#E2.2`, `#E9`).

Meguri's own tests (`core/crates/meguri-package/tests/legacy_export.rs`, U123; the `.colpkg` legs in `tests/colpkg.rs`, U124):

- The live collection file's SHA-256, `col.ver`, journal mode, `-wal` size and `data_version` are unchanged after a legacy `.apkg` and a legacy `.colpkg` export with no other op running; a legacy `.colpkg` taken while answers commit holds every answer committed before its snapshot, and no answer waits on it.
- The package, reread by an independent zip and JSON reader: L3's entries, order and methods, `08 02`, the map's exact bytes for L5's two examples, raw payloads equal to the source files, the collection entry inflating to a schema-11 file that D8's checks accept.
- Round trip: the package imported by Meguri (P17, upgrade on staging) gives the source subset (`.apkg`) or the source (`.colpkg`), except D7's fields (and, for `.colpkg`, the import's `col.scm` and `col.mod`); a note type with unknown `other` keys from the U11 fixture keeps them in the package's JSON and after the round trip (R1464); an unknown config key keeps its value (R1466).
- Each L11 error: `LegacyConversion` for an unconvertible config value, `MediaTooLarge` and `LimitExceeded` from injected sizes, `VerificationFailed` from a corrupted entry injected before the reread, `DiskFull` from a failpoint during the downgrade and during the write, `InsufficientSpace` from injected low space at L9: no artifact, staging and temporary file removed, the collection unchanged.
- Cancel during the downgrade, during capture and during the write: `Cancelled`, the same cleanup; a kill at D10's fault points and during the write: nothing published, leftovers swept at the next launch.
- A media file rewritten or replaced during capture: `MediaChanged` (`.apkg`) or E7.2's changed-source result (`.colpkg`).
