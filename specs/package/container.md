# Package container and collection-only collection packages

Spec: package/container
Requirements: R1597, R1598, R1599, R1619 (the latest-format, collection-only part of each), R2004 (atomic package output), R2015 (disk preflight), R1635, R1852, R1638 (data side), R1626, R1627, R1628, R2018 (P16 local media import); supports R2005, R2009 (format half)
Anki: 26.09.3
Scenarios: conformance/scenarios/package/collection-package/

The zip container that collection packages (`.colpkg`) and deck packages (`.apkg`) share, in its latest format, for packages that hold a collection and no media file. It covers how Meguri reads such a package (bounded, into a staging directory), how it writes one (backups U126, server copies U109, and later U124's export without media), and how it inspects one before a restore (U127). The legacy formats are U120's, media entries U122's, the legacy writer U123's: each extends the sections that name it. P15 defines the narrow RF4 latest-format media writer/verifier used by user collection-file export, without accepting U122/U124 or changing the collection-only writers. P16 extends local collection-file inspection and activation to media; it is Meguri design, not a claim of new foreign behavior. Swapping an inspected package into place is `engine/collection-replace`. The scenario vocabulary is `specs/conformance/package-ops.md`.

## P1. Formats

A package is a zip archive (PKWARE APPNOTE 6.3.x, zip64 included). Three formats exist. The `meta` entry (P3) tells them apart:

| Format | `meta` | Collection entry | Encoding of the collection entry | Collection schema its writer uses | Media map (`media`) | Media files |
|---|---|---|---|---|---|---|
| legacy 1 | absent, or version 1 | `collection.anki2` | raw | 11 | JSON (U120) | raw (U120) |
| legacy 2 | absent with `collection.anki21` present, or version 2 | `collection.anki21` | raw | 11 | JSON (U120) | raw (U120) |
| latest | version 3 | `collection.anki21b` | zstd (P4) | 18 | protobuf in zstd (P5) | zstd (U122) |

- "Raw" means the bytes of a SQLite database file, which start with the 16 bytes `SQLite format 3` and a NUL.
- `.colpkg` and `.apkg` files use the same container. Which flow imports a file (R1596) is U120's and U124's.
- A **collection-only package** is a latest-format package whose media map lists no file. Anki's periodic backups are such packages, and so is its collection export with media turned off.
- Entry names are compared byte for byte, letter case included. All names this spec uses are ASCII.

## P2. The archive

- Compression methods: 0 (stored) and 8 (deflated) are read; any other method on an entry the reader needs makes the package `Corrupt`. Anki 26.9.3 reads the same two.
- An encrypted entry (general-purpose flag bit 0) that the reader needs: `Corrupt`.
- Zip64 records are read. Data before the first entry and the archive comment are ignored.
- The entries a reader needs are `meta`, the collection entry, `media` and, for U122, the media files. Other entries (directories, unknown names) are never read, and this unit never reads the media files.
- Two central-directory entries with the same name: `Corrupt` (Meguri's rule; no scenario observes what Anki does).
- A file that is not a zip archive (no end-of-central-directory record, or one that does not parse) and a CRC-32 mismatch at the end of an entry: `Corrupt`.
- Nothing outside the package is read (R1599). No entry name is ever turned into a path: extracted bytes go only to staging files with fixed names (P9).

## P3. The `meta` entry

- A stored entry that holds one protobuf message, read with the wire rules of `model/protobuf-records#P2`. Its one known field is field 1, the package version: a varint read as an `int32` (the low 32 bits, two's complement). Other fields are skipped by their wire type. When field 1 occurs more than once, the last value counts. A missing field 1, and an empty entry, give 0.
- Field 1 with a wire type other than varint, a truncated or otherwise invalid message, or an entry larger than its limit (P8): `Corrupt` (Anki: a protobuf decode error).
- A `meta` entry that cannot be read at all (a compression method other than 0 or 8, encryption, a CRC-32 mismatch): `Corrupt`. Anki 26.9.3 instead treats such an entry as absent and falls back to the legacy detection below, which in a latest-format package finds the placeholder (P7). Meguri never imports a placeholder in place of the collection. No scenario observes this case, and only a crafted file reaches it.
- Version to format:

  | Version | Format |
  |---|---|
  | 1 | legacy 1 |
  | 2 | legacy 2 |
  | 3 | latest |
  | anything else: 0, 4, any negative value | refused as `TooNew`, "made by a newer version" (R1597) |

- No `meta` entry: legacy 2 when the archive has an entry `collection.anki21`, else legacy 1.
- Until U120 adds the legacy reader, a legacy package is refused with `UnsupportedFormat` and nothing is written.
- Writer: the entry holds exactly the two bytes `08 03`.

Examples: `08 03` is the latest format; `08 04` and the empty entry are `TooNew`; `08 83 80 80 80 10` (field 1 = 2^32 + 3) reads as 3, the latest format; `0a 01 33` (field 1 length-delimited) and `ff` (a truncated varint) are `Corrupt`.

## P4. The collection entry (latest format)

- Name `collection.anki21b`. Anki writes it stored; a deflated one is read too. Missing: `Corrupt`.
- Payload: a zstd stream (RFC 8878). One or more frames back to back are decoded in order; skippable frames are allowed and contribute nothing; a frame's content checksum is checked when the frame has one. The decoded bytes are the collection file.
- A payload that is not a valid zstd stream, a checksum mismatch, a frame that needs a window above the limit (P8), or bytes after the last frame that are not a frame: `Corrupt`.
- The decoded file may be any collection file. The open gate (`storage/open-and-connections#C2`) decides, on the staging copy (P12): Anki's writers put a schema-18 file there, other writers may not, and a schema-11 file inside a latest-format package is upgraded like any other.
- A reader must not need a declared content size or a content checksum: Anki 26.9.3 writes this frame in streaming mode at the zstd library's default level and does not ask for either. Meguri's writer adds both (P11).

## P5. The media map (latest format)

- Entry `media`. Its payload is a zstd stream (the P4 rules) whose decoded content is a protobuf message (`model/protobuf-records#P2` wire rules):

  | Field | Wire type | Meaning |
  |---|---|---|
  | 1, repeated, length-delimited | 2 | one media entry message per media file, in order |

  A media entry message:

  | Field | Type | Meaning |
  |---|---|---|
  | 1 | `string` | the media file name |
  | 2 | `uint32` | the file's size in bytes |
  | 3 | `bytes` | the SHA-1 of the file's content |
  | 255 | `uint32` | an archive index Anki keeps only when it converts a legacy map in memory; Meguri ignores it and never writes it |

  Unknown fields are skipped. The n-th media entry (counting from 0) names the zip entry whose name is n in decimal without leading zeros (`0`, `1`, ...). Media files are U122's and U124's: this unit never reads or extracts them.
- Every entry is checked before the package is accepted (R1598): its name is valid UTF-8 (a protobuf `string`), its SHA-1 is exactly 20 bytes, and its name is safe (P6). Any failure: `Corrupt`. Anki checks the same three things.
- Decoded content of zero bytes is a map with no entry. The collection-only writer writes the zstd encoding of zero bytes.
- **No `media` entry.** Meguri reads it as a map with no entry and imports no media (R1598). Anki 26.9.3 refuses such a latest-format package: it substitutes the legacy format's empty JSON map, which is not a valid protobuf message. Deviation `package-latest-missing-media-map`; scenario `package/collection-package/missing-media-map`.

## P6. Safe media names (latest format)

A name is safe when it is not empty and the media file name transform `text/media-filenames#F1` leaves it unchanged. That transform removes `/` and `\` and turns `.` and `..` into `._` and `.._`, so a safe name is always one plain path component. Anki's check has the same result: one plain component that its own transform leaves unchanged.

| Name | Safe |
|---|---|
| `photo.jpg`, `.png`, `日本.mp3` | yes |
| (empty) | no |
| `../x.jpg`, `a/b.jpg`, `a\b.jpg`, `/abs.png` | no (separator) |
| `.`, `..` | no (F1 step 5 changes them) |
| `con.jpg`, `nul` | no (F1 step 4) |
| `e` U+0301 `.png` | no (NFC changes it) |
| `a:b.png`, `x?.png`, `tab` TAB `.png` | no (F1 step 2) |
| `name.` , `name ` | no (F1 step 5) |
| a name longer than 120 bytes in UTF-8 | no (F1 step 6) |

## P7. The placeholder collection (latest format)

- Clients older than the latest format (Anki before 2.1.50) look for `collection.anki21`, then `collection.anki2`, and import what they find. The placeholder gives them a collection with one card that asks the user to update. Readers of the latest format, Meguri's included, never open it; only the `package_listing` observation reads it.
- Entry `collection.anki2`, stored, raw (not zstd).
- Meguri's writer puts there a schema-11 collection file with these properties:
  - Tables `col`, `notes`, `cards` and `revlog` with the definitions of `storage/schema-18#S2` (the same text as at 11), `graves` as `CREATE TABLE graves ( usn integer NOT NULL, oid integer NOT NULL, type integer NOT NULL )`, and the seven indexes `ix_notes_usn`, `ix_cards_usn`, `ix_revlog_usn`, `ix_cards_nid`, `ix_cards_sched`, `ix_revlog_cid` and `ix_notes_csum` with the definitions of `storage/schema-18#S3`. No other object besides SQLite's own.
  - Rollback-journal mode (header bytes 18 and 19 are 1), no `-wal` or `-journal` file when it is packed. Page size is free (Anki uses 512 to keep the file small).
  - The `col` row: `id` 1, `crt` the writer's current time in epoch seconds rounded down to a whole UTC day, `mod` and `scm` the writer's current time in epoch milliseconds, `ver` 11, `dty` 0, `usn` 0, `ls` 0, `tags` `{}`, and `conf`, `models`, `decks` and `dconf` as below (`model/schema11-json#S12`).
  - `models`: one standard note type, Meguri's own (never Anki's stock note types, R1959): name `Notice`, one field `Text`, one template `Card 1` with front `{{Text}}` and back `{{Text}}`, CSS `.card { font-family: system-ui, sans-serif; font-size: 18px; margin: 2em; }`, sort field 0, deck 1. Its object is what `model/schema11-json#S4` to `#S6` write for that record; its id is any positive id.
  - `decks`: `{"1": <the Default deck object of model/schema11-json#S13>}`. `dconf`: `{"1": <the Default preset object of model/schema11-json#S13>}`.
  - `conf`: `{"activeDecks":[1],"addToCur":true,"collapseTime":1200,"curDeck":1,"curModel":<the note type id>,"dueCounts":true,"estTimes":true,"newSpread":0,"nextPos":2,"schedVer":2,"sortBackwards":false,"sortType":"noteFld","timeLim":0}`. These are the keys older clients read when they show the deck list and study; `schedVer` 2 also lets any current client, Meguri's open gate included, open the file.
  - One note: note type `Notice`, field text the P7.1 string, no tags, `guid` a new GUID (`text/guid#G3`), `sfld` and `csum` from `text/checksum-and-sort-field`, `id` the current time in epoch milliseconds, `mod` the current time in epoch seconds, `usn` 0, `flags` 0, `data` empty. One card: `id` and `mod` as the note's, `ord` 0, deck 1, new (`type` 0, `queue` 0, `due` 1), every other number 0, `data` empty. No review-log row, no grave.
- What this guarantees is checked in conformance (`specs/conformance/package-ops.md` E3): the placeholder is at version 11, rollback mode, holds one note and one card, and Anki opens it.

### P7.1 Placeholder text

Meguri's own words, exactly:

`This file needs a newer version of Anki. Please update to Anki 2.1.50 or later, then import the file again.`

## P8. Limits (R1599, R1852)

Meguri's own limits (Anki 26.9.3 has none). Each is checked while reading; the read stops at the first byte past a limit.

| What | Limit | Past it |
|---|---|---|
| entries in the central directory | 400,000 | `Corrupt` |
| bytes read from `meta` | 64 KiB | `Corrupt` |
| each scheduler/config value examined by the candidate open probe: schema-11 `col.conf`, or modern `config.val` for key `schedVer` | 4 MiB (4,194,304 bytes), inclusive | `Corrupt` |
| decoded bytes of `media` | 64 MiB | `Corrupt` |
| decoded bytes of `collection.anki21b` | 8 GiB | `Corrupt` |
| decoded bytes of `collection.anki21b` and of `media`, each | the larger of 16 MiB and 1,000 times the entry's compressed size | `Corrupt` |
| decoded bytes of all entries read from one package | 16 GiB (U122 counts its media files in it) | `Corrupt` |
| zstd window a frame may need | 2^27 bytes (the zstd library's default limit) | `Corrupt` |

- Reasons: real collections compress about 3 to 10 times, so 1,000 times is far above any real file; 8 GiB is many times the largest collection a sync server accepts (300 MiB decoded by default, `sync/transport`). The central directory costs the zip library a few hundred bytes per entry, so 400,000 entries stay near 120 MB, under R1852's 300 MB.
- A 1 KiB collection entry may therefore decode to at most 16 MiB. A crafted 1 KiB frame can expand to about 40 MiB, which this limit stops.
- Streaming: the collection entry is decoded in fixed-size chunks straight into its staging file and is never held in memory whole. `meta` and `media` may be held in memory up to their limits. Peak memory while reading does not grow with the collection's size.

Check each probed scheduler/config value's stored byte length before copying,
decoding or parsing it. The check must not materialize an oversized value,
including inside SQLite. Count bytes, not text characters. Apply this bound
before the ordinary candidate probe for both legacy and modern representations.
Existing handling of missing values, storage classes and invalid content stays.
Large unrelated config values are outside this probe-value bound.

This is a Meguri resource-safety limit, independent of the 8 GiB decoded
collection limit. A structurally valid collection exceeding this marker limit
is refused. R1852 still applies to total process memory. Refusal removes only
operation staging and preserves the selected source and live collection; it
must not reach replacement or safety-backup execution.

Focused tests cover both representations at the inclusive boundary and one
byte above it, large TEXT and BLOB values, unaffected unrelated config,
existing malformed/missing markers, and bounded memory for a large streamed
candidate whose marker exceeds the limit.

## P9. Staging and temporary files

Both kinds of name come from U32's namespace for unpublished work (`storage/durability-and-files#F4`). This unit invents no name and adds no leftover pattern of its own.

- **Staging directory.** Each operation (inspect, a writer's snapshot, a caller's download) gets its own fresh staging directory from U32's staging-directory primitive, made in the collection directory (U32 names it `.meguri-stage-` and a random suffix). A candidate collection there is always named `collection.anki2`; SQLite and the open gate put their own files next to it (`-wal`, `-shm`, `-journal`, `collection.anki2.pre-upgrade-v<V>`, `storage/schema-upgrade#U2`). A writer's snapshot is `snapshot.anki2`.
- The staging directory sits in the collection directory so that a staged file and the collection are on one file system and the swap is one rename (`engine/collection-replace#R5`).
- **Temporary package file.** A package being written is U32's atomic file in the destination's directory (U32 names it `.meguri-tmp-<purpose>-` and a random suffix). The caller names the purpose: `backup` for U126's backups, `export` for U109's server copy and U124's export.
- The operation removes its staging directory and its temporary file when it ends, whatever the outcome; U32's handles do so when they are dropped. After a kill, U32's launch sweep removes both at the next launch (R1493): it sweeps exactly these names in the collection and backups directories. Nothing ever reads a leftover as data. P16 transfers files needed for an admitted media replacement into owned media recovery storage before live publication; those files are not unpublished P9 leftovers and must survive this sweep.

## P10. Disk preflight (R2015)

Before an operation writes anything, U32's free-space check runs on the file system it writes to. Too little space: `InsufficientSpace{needed, available}`, and nothing is written.

| Operation | `needed` in bytes |
|---|---|
| write a package from the live collection | 2 × (size of `collection.anki2` + size of its `-wal`) + 4 MiB |
| write a package from a standalone file | 2 × the file's size + 4 MiB |
| inspect (P12) | E + 4 MiB, where E is the content size the first zstd frame of `collection.anki21b` declares, or, when it declares none, the larger of 4 × the entry's compressed size and the live collection file's size |
| inspect, after extraction, when the extracted file is below schema 18 | the extracted file's size again (the pre-upgrade copy) |

- A restore adds its safety backup to the first check (U127).
- A write that still fails for lack of space (another app filled the disk): `DiskFull`, with the same cleanup as any failure. U32 reports that failure as its own `InsufficientSpace` (`storage/durability-and-files#F4`, `#F5`). Once this preflight has passed, the package error for it is `DiskFull`, so a message can tell "not enough space to start" from "the disk filled during the write".

## P11. The collection-only writer

Writes a whole collection as a collection-only package (R1619's latest format without media).

- **Sources.**
  - The live collection: a snapshot made with `VACUUM INTO` on the shared maintenance reader (`storage/open-and-connections#C6`, KTD3). It is one read transaction: it never closes the collection, never holds the writer (answers keep committing, R1635), runs while a sync holds the sync reader, and contains every commit made before its first read.
  - A standalone file the caller has verified (U109's server copy): a closed collection file with no `-wal` or `-journal` file next to it, else `SourceNotFinalised`. It is snapshotted the same way through a read-only connection with `unicase` registered (`storage/open-and-connections#C5`).
- **The snapshot** is `snapshot.anki2` in a fresh staging directory (P9).
  Required header bytes: 18 = 2; 19 = 2 (WAL).
  Close it without `-wal` or `-shm` files.
  This matches the package format even when `VACUUM INTO` produces rollback mode.
- **Entries**, in this order, all stored:
  1. `meta`: `08 03` (P3).
  2. `collection.anki21b`: one zstd frame at level 3.
     Include its decoded size and a content checksum; the reader accepts both.
  3. `collection.anki2`: the placeholder (P7).
  4. `media`: the zstd encoding of zero bytes, with the same frame settings.

  Zip64 records are written when an entry or the archive needs them (4 GiB or more). Entry times are free.
- **Output.** Written to the temporary package file (P9) and published through U32's atomic-file sequence (`storage/durability-and-files#F4`): full sync of the file, the verification below, the rename, full sync of the directory. A full sync is `F_FULLFSYNC` on Apple platforms (`storage/open-and-connections#C9.2`). The verification reads the file back with the reader of this spec: latest format, the four entries, `collection.anki21b` decoding to exactly the snapshot's bytes (length and SHA-256, both computed while compressing), an empty media map, a placeholder that starts with the SQLite header. It closes every handle on the file before the rename.
- **Finalisation**, chosen by the caller: `ExclusiveCreate` (U126's backup names; U32's exclusive publication, which fails with `DestinationExists` when the name is taken and leaves that file alone) or `Replace` (U109's server copy name; U32's ordinary publication). The name and its rules are the caller's.
- **Failure or cancel** at any point: nothing appears at the destination (with `Replace`, a file already there keeps its bytes), the temporary file and the staging directory are removed, and a typed error is returned (P13). The collection itself never changes.

## P12. Inspect

Checks a collection package before a restore (U127), and later before a `.colpkg` import (U124), without touching anything but staging.

- Steps, in this order. The first failure ends the inspect with its error (P13):
  1. Read the archive (P2) and `meta` (P3), and find `collection.anki21b` (P4).
  2. Preflight (P10), then extract `collection.anki21b` into a fresh staging directory as `collection.anki2`, within the limits (P8).
  3. Run the open gate on it (`storage/open-and-connections#C2`): the probe, the refusals of R1479, R1482 and R2013, the upgrade of schemas 11 and 14 to 17 (R1478, on this staging copy only, after the second preflight of P10), and the expected-object check (R2016).
  4. Run `PRAGMA integrity_check` on it, with `unicase` registered: anything but the single row `ok` is `Corrupt`.
  5. Read the media map (P5).
- Result: a staged-collection handle and the facts `ver_found` (the `col.ver` read before any upgrade), the counts of `notes`, `cards` and `revlog` rows, `scm` and `mod` as the staged file holds them after the gate, and the number of media entries. The handle owns the staging directory: dropping it removes the directory; handing it to the replace primitive (`engine/collection-replace#R2`) passes the file on.
- Anki 26.9.3 also reads the media map after the collection, but it sets `scm` and `mod` on its copy before its integrity check; Meguri sets them in the replace (`engine/collection-replace#R3`, `#R7`), after its own checks. Only the error kind is observable, and every fault but an unknown version gives the same kind (P13), so neither difference can be told apart.
- Any failure removes the staging directory. Inspect writes nothing outside it: the live collection, its `-wal`, the backups and the media folder keep their bytes (R1638, data side). Inspect needs no exclusive phase and runs while the collection is open and used.

## P13. Errors

| Error | When |
|---|---|
| `TooNew` | the `meta` version is not 1, 2 or 3 (P3) |
| `UnsupportedFormat` | a legacy package, until U120 |
| `SchemaUnsupported{gate}` | the open gate refused the collection inside as `TooOld`, `TooNew` (version 12, 13 or above 18) or `V1Scheduler`; `gate` is that error, so the message can say which (U153) |
| `Corrupt{detail}` | everything else that is wrong with the package: P2, P3, P4, P5, P8, the gate's `NotACollection`, `UpgradeFailed` and `Corrupt`, a failed integrity check; `detail` names the check for logs |
| `InsufficientSpace{needed, available}` | P10 |
| `DiskFull` | a write failed for lack of space after the preflight passed |
| `SourceNotFinalised` | P11, standalone source with a `-wal` or `-journal` next to it |
| `DestinationExists` | P11, `ExclusiveCreate` and the name is taken |
| `Cancelled` | the caller cancelled |
| `Io` | any other file-system failure |

- Anki 26.9.3 reports an unknown `meta` version as "made by a newer version". Every other fault ends its import with an error and the collection unchanged, mostly its "not a valid file" error, also for a collection inside that it cannot open. Scenarios record two kinds, `too_new` and `corrupt` (`specs/conformance/package-ops.md` E2.3); Meguri's finer errors serve its own messages.
- A v1-scheduler collection inside a package is refused by Meguri and imported by Anki, as for an open (deviation `storage-v1-scheduler-refused`, R1482). No package scenario carries one.

## P14. Scenarios and tests

| What | Where |
|---|---|
| Meguri's package, imported by Anki: the dump equals the source's, layout and placeholder equal Anki's own (T1 live) | `package/collection-package/export-import-roundtrip` |
| Meguri's package layout equals Anki's export layout | `package/collection-package/export-listing` |
| Anki's backup: layout; restored by Meguri, the dump equals Anki's restore | `package/collection-package/backup-listing`, `restore-backup` |
| A schema-11 collection inside a latest-format package is upgraded on the staging copy | `package/collection-package/upgrade-on-import` |
| `meta` versions 4 and 0 refused as too new, a malformed `meta` as corrupt | `package/collection-package/refused-meta` |
| No collection entry, a collection entry that is not zstd, a file that is not a zip | `package/collection-package/refused-entries` |
| A collection that fails the integrity check, a schema-12 collection | `package/collection-package/refused-collection` |
| Media map with `../x.jpg`, `con.jpg`, a 19-byte SHA-1 | `package/collection-package/refused-media-map` |
| No `media` entry: Meguri imports, Anki refuses | `package/collection-package/missing-media-map` (deviation `package-latest-missing-media-map`) |
| Inspect of the backup fixture: counts equal those of the `restore-backup` golden's `db_dump`, `ver_found` 18 | `core/crates/meguri-package/tests/collection_package.rs` |
| Every refusal: no staging directory left, live collection and directory listing unchanged (SHA-256) | same file |
| A stored 1 KiB `collection.anki21b` that decodes past 16 MiB: `Corrupt`, peak memory bounded, staging removed | same file |
| Legacy package (no `meta`, or version 1 or 2): `UnsupportedFormat`, nothing written | same file |
| Writer on a standalone file: inspects and restores to that file's rows | same file |
| Writer during answers on the interactive lane: answers commit, the package passes the integrity check and holds every answer committed before the snapshot | same file |
| Disk full during the write (failpoint): no destination file, temporary file removed, `DiskFull`; injected low space: `InsufficientSpace` before any write | same file |
| `collection.anki21b` of the writer: header bytes 18 and 19 are 2, frame declares the size and has a checksum; placeholder: header bytes 1 and 1, P7's objects | same file |
| Reader on arbitrary bytes: no panic, no write outside staging, limits hold | `core/fuzz/fuzz_targets/package_reader.rs` (30 minutes) |


## P15. Latest-format media output for user collection-file export

This extension implements `package/file-export#E7`. P11 remains the explicit
collection-only writer for backups and retained server copies. Its layout and
verification do not change. No legacy or deck-subset behavior is added here.

The media-capable writer takes the closed collection snapshot and the immutable
captured media inventory of E7. Reuse P11's `meta`, collection and placeholder
entries and their frame settings. Add one stored zip entry per media file,
named by its zero-based map position in decimal with no leading zeros. Its
payload is one streaming zstd level-3 frame, including decoded content size and
content checksum. A zero-byte file still has a complete zstd frame and its own
map record. Write `media` as a zstd frame containing the P5 protobuf records;
write each record's exact UTF-8 name, uint32 decoded length and 20-byte SHA-1.
Never emit field 255. Zip64 remains available; no media payload is buffered
whole. Entry ordering is `meta`, `collection.anki21b`, `collection.anki2`, numeric
media entries in map order, then `media`.

Check available capacity on each filesystem before capture creates files.
Extend P10 to budget for the database snapshot and immutable media copies on
the collection filesystem, and the completed archive, map and zip-record
costs on the output filesystem. Allow for media that grows under compression.
Sum concurrent temporary and output requirements when they use one filesystem;
otherwise query each filesystem's capacity provider separately. All arithmetic
must detect overflow, and the P8 and uint32 bounds must pass before any write.
Do not start capture until every capacity check succeeds. The later native
save still follows `package/file-export#E4`, checking the selected destination
against the finished artifact's size. Exhausting capacity after preflight
returns `DiskFull`.

Before publication, reread the closed package under P2/P4/P5/P6/P8. Match the
snapshot's exact decoded collection length and SHA-256 and every captured map
record and numbered entry. Stream each media payload to verify its exact decoded
length and SHA-1, frame validity/checksum and aggregate bounds. Reject missing,
duplicate or extra numeric entries and mismatched records. A name never becomes
an arbitrary staging path. The existing atomic-file verifier/publication,
cleanup and cancellation rules apply to the complete media-inclusive package.

This verifier may validate nonempty media maps without enabling media install
in collection import or restore. P16 separately defines latest-format local
collection-file media input. Until that full path is implemented and verified,
its callers retain explicit refusal; no map may be accepted and discarded.
Legacy reading retains its separate contract. No unit completion follows from
this output extension.

## P16. Local collection-file import with media (Meguri design, U122/U124)

### P16.1 Scope and owned inspection

This extension admits latest-format whole-collection files through the existing local entry flow. It starts no Anki server request. Server work remains frozen. Legacy formats and deck-package merges retain their separate contracts.

Keep the existing coordinated source lifetime, owned source copy and opaque inspection handle. Bind confirmation to the source SHA-256, destination, engine, collection generation and committed operation sequence. The handle also owns the validated media inventory and bytes. A path or client-supplied count never replaces that authority. Check these guards again after participant flush and before activation; stale confirmation requires a new preview. Preserve the selected source.

Inspect all collection and media data before changing live database, media or journal state. Extend P12 with these checks:

- Decode the complete P5 map under P2–P8. Check every name, length, SHA-1 and numbered-member assignment, including entries not referenced by collection rows.
- Require exactly one payload for every map entry. Reject duplicate map names, missing, repeated or extra numeric members, including noncanonical numeric names. Unknown nonnumeric archive members keep P2's existing treatment.
- Stream each payload through ZIP CRC and P4's frame/checksum/window checks into fixed operation-owned staging files. Verify its decoded length against the uint32 declaration and its SHA-1 against the map. Stop at the first exceeded limit. Include every decoded payload in P8's 16-GiB total; no file is buffered whole.
- Refuse names that the shared store or destination volume cannot keep together without changing a supplied name or losing a payload. This includes conflicting incoming case variants under `media/media-files` M2/M6. Never silently drop, lowercase or rename one mapped file, or rewrite imported references to hide a conflict.

Zero-byte, underscore, unreferenced and greater-than-100-MiB files remain local media. Apply the package bounds, not the media-sync size limit. Required payloads are files, never directories or links; never use archive paths, modes or links as filesystem authority. Source, destination, staging and backup roots retain their existing isolation and no-link checks.

Preflight each affected filesystem before writes. Add incoming media, simultaneous staged and recovery copies, journal snapshot and index work, and the forced backup to P10's database budget. Use checked arithmetic and account for shared filesystems once with their combined peak requirement. Insufficient capacity refuses; later exhaustion is a disk-full failure, never successful media exclusion.

### P16.2 Merge and retained old media

Use the existing single entry activation owner, long-operation slot and initial/Ready replacement paths. An active review still refuses replacement. Enter the exclusive phase and drain accepted work. Settle prior media stages against the old collection before closing it. Hold shared media-operation ownership so local media writes and jobs cannot interleave with this merge. Run scans and streams off the collection writer.

Take the real forced old-database backup after the drain (`package/backup-triggers` T6; `package/backups` B4). Keep its receipt through every outcome. No old database means no fabricated backup. Backup failure prevents media publication and database replacement.

Check current destination files before publication. Reuse a file when its SHA-1 equals the incoming hash (R1627). For different bytes, apply the shared store's exact-name and trash operations (M6/M9/M10); do not choose a filename through M3. Write the incoming content under its supplied name. Save each replaced file in trash with its original name and durable identity. Leave other old media, prior trash entries and unused files intact. If the filesystem still reports a name collision, refuse before changing that name. A link or another nonregular entry must not be replaced. No foreign media index enters the imported state.

Before live media writes, checkpoint the store journal and copy it. Keep the copy unchanged and record its identity. For each affected name, save its old bytes, rows and trash IDs. Bind them to the T6/B4 forced-backup receipt and the R6 operation. Keep each old name's trash ID and version for that backup after import succeeds. Delete the journal copy only after durable completion. Cleanup and pruning old backups must not remove saved media files or their mapping. M10 still controls explicit trash deletion. Tell the user that restoring the backup's media needs those saved files.

### P16.3 Publication and recovery

This is a continuation of `engine/collection-replace` R5/R6, not another lifecycle or an atomic database-plus-media rename. The collection advisory lock and replacement owner cover these steps:

1. Validate and synchronize all staged payloads. Run R3/R4 on the staged database. Close live collection connections and finalize the old-file identity through R5. Do not admit ordinary writers during this sequence.
2. Extend R6's durable intent before publishing media. Bind the old/candidate identities, backup receipt, immutable media inventory, old journal snapshot and each planned exact-name/trash action. Record the native identities of the locked old and final staged database files alongside their byte identities. These distinguish an ordinary identical-database import whose media differs. Retain its recovery files under owned `media.staging` storage outside P9's unconditional sweep namespace. These are R6-owned files, not J4 note-postimage stages; ordinary media-stage recovery waits for their R6 resolution. An intent write failure prevents live media changes.
3. Merge media before the database swap, as U124 requires. Use the shared store's synchronized temp/rename, trash and journal operations. Record enough durable per-file progress to recover a kill between any two publications. Repeating recovery verifies expected bytes; it never overwrites unrelated content or discards a conflicting version.
4. Initialize the imported generation's own journal from the complete merged folder before the database swap. Register every locally supported record through the shared store; mark live syncable files pending, retain empty/oversized files for local lookup, clear pending-download state and reset the media USN to 0. Retain every existing media file. The old journal snapshot remains the rollback authority; foreign account/media baselines are not imported or reused. No network work starts.
5. Only after media and journal durability succeeds, use R5's existing database rename and directory sync. That rename is the database activation boundary. Complete R6's candidate activation and verify the merged files and journal before any new collection writer, mutating day check, reader, `Ready` or `CollectionReplaced` delivery. Emit generation change once through the existing lifecycle; do not add a second collection writer. Do not report candidate media ready before this completes.

On pre-swap failure or cancellation, prove that the old database remains active under R6. Restore the affected media names and the old journal state before returning the old collection to use. Retain newly published bytes in trash when undoing their publication. Report `Cancelled` or an unchanged-collection failure only after this rollback succeeds; keep any published safety backup. With no old database, restore the recorded absence and previous media state.

After the database rename, cancellation cannot report rollback. Finish the recorded activation, or return a typed candidate-active/recovery-required outcome with the backup and retained recovery data. Do not repeat import to repair a committed activation. Recovery retries only the recorded file/journal completion.

At reopen, reconcile R6 and these media actions before the ordinary staging sweep, collection open or media lookup. Proven old database means restore its media/journal view; proven candidate means finish its merge/journal activation. Use the recorded native identities or R6's durable activation witness when old and candidate database bytes are equal; byte hashes alone cannot select the media view. Missing/corrupt recovery data, conflicting identities or unprovable file contents block `Ready` and retain evidence. Never guess from equal timestamps or treat absent metadata as completion. Recovery is idempotent. Clear only completed operation ownership; do not sweep retained backup/media recovery data.

### P16.4 Facts, outcomes and checks

The preview states that the whole collection is replaced and media is merged. Show the validated file count and total decoded bytes, and state that old media is kept. Before commit, obtain and report current counts for reused files, added files and files moved to trash; otherwise return a typed media conflict. Old inventory data cannot justify these counts. A successful result contains the actual generation, any backup receipt and final media counts. Errors identify whether the old state is unchanged or restored, the old state needs recovery, or the candidate is active and needs recovery. Keep this information through FFI tracking errors, cancellation and restart. An unfinished import is not success; a committed replacement is not cancellation.

Until this entire path is implemented and verified, retain the explicit media-input refusal. Removing that refusal alone does not implement P16.

| Check | Minimum evidence |
|---|---|
| Package inspection | All map/member/name/hash/size/frame/CRC and total-limit failures; late invalid member; empty/large/static/unused media; bounded memory and disk-full cleanup; source and live state unchanged. |
| Merge and journal | First import and replacement; equal-hash skip, different-byte collision and retained old version; unrelated old media retained; incoming/volume name conflict; own journal initialization; backup/confirmation/session/generation failures. |
| Kill and recovery | Stop the process before intent, between each media, trash and journal publication, before and after database rename, directory sync and activation, and inside recovery. Include equal database bytes with different incoming assets. Reopen must establish the old or candidate view from recorded proof and keep all bytes. Repeating recovery has the same result. An unresolved state must not become Ready. |
| App and bridge | Exercise the existing owner on first import and Ready replacement. Drain cancellation and consume each opaque handle once. Check results on both sides of commit. Preserve those results, their backup receipts and recovery facts across delivery errors and restart. |
| Native chain | Mac, iPhone and iPad: Files/native import of generated media, rendered image/audio, accepted answer, relaunch without fixture reseed, native export with all media, then independent package comparison of saved reviews, media names and bytes. |

Keep `package/file-export` E7.4 and `fixtures/synthetic-collections` F11.1 independent expected-source rules. Freeze the generated input and any declared pre-launch preparation before the app runs. Derive expected post-import stamps and accepted review facts from their existing contracts and recorded inputs. Never make the app's later state or decoded export its own expected baseline. Existing local focused, kill and native checks remain required; neither this addendum nor export proof closes full U124/U162 or any milestone.
