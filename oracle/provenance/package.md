# Package specs: container, collection-only packages, replace, package ops (U124a)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/package/container.md`,
`specs/engine/collection-replace.md` (R7 only; the rest is Meguri design) and
`specs/conformance/package-ops.md`. No code was copied; the rules were
re-derived in the spec writer's words, then checked by black-box probes of
the pinned `anki` wheel in the oracle image (2026-10-17, image
`meguri-oracle:26.9.3-7c3e7da2abdbcd8b`, network off): Anki's export and
backup layouts (four stored entries in the P11 order, `meta` `08 03`, the
empty media map as a 9-byte single-segment frame, inner collection with a WAL
header and no frame content size or checksum, placeholder at 11 in rollback
mode with page size 512, one note, one card, `sqlite_stat1`/`sqlite_stat4`);
export leaves the `col` row unchanged; import of the export changes only
`col.scm` and `col.mod`, both to the same current millisecond; and every
refusal of the scenarios (meta 4 and 0 too new; `ff` a protobuf decode error;
meta 2^32+3 read as 3; no collection entry, schema 12, the damaged index page
and the three media-map faults "not a valid file"; a raw SQLite collection
entry an I/O error; a non-zip file a zip error; no `media` entry a protobuf
decode error), each with the collection's rows unchanged and no file left. A
frame with a declared content size and a checksum (zstd CLI) and the maker's
raw-block frames both import. The goldens of
`conformance/scenarios/package/collection-package/` record the same once the
oracle maintainer has added the ops and fixtures.

| Spec section | Anki file | Read for |
|---|---|---|
| container P1, P3 | `proto/anki/import_export.proto` (package metadata message), `rslib/src/import_export/package/meta.rs` | the three versions, their entry names and schemas; legacy detection without `meta`; an unknown version (prost's enum getter maps any other number to the unknown variant) refused as too new; `meta` decoded with prost (malformed: protobuf decode error) |
| container P3 (a `meta` entry that cannot be read) | `rslib/src/import_export/package/meta.rs` | read errors on `meta` (lookup or read) are dropped, so the package goes through the no-`meta` legacy detection; spec-side reading only, not probed (Meguri refuses it as corrupt) |
| container P2 | `Cargo.toml` (workspace `zip` 4.1 with `deflate` and `time` only), `rslib/src/error/network.rs` (zip errors become sync errors) | methods Anki reads; what a non-zip file raises |
| container P4 | `rslib/src/import_export/package/colpkg/import.rs` (collection copy), `colpkg/export.rs` (stored entry, streaming encoder at level 0, multithreading above 10 MB) | stored zstd collection entry; missing entry refused as corrupt |
| container P5, P6 | `proto/anki/import_export.proto` (media entries message, fields 1, 2, 3, 255), `rslib/src/import_export/package/media.rs` | the media map, its zstd payload, the per-entry checks (20-byte SHA-1, safe and already-normalized name); a missing `media` entry replaced by the legacy `{}` text even for the latest format, which prost cannot decode |
| container P6 | `rslib/io/src/lib.rs` (file name safety), `rslib/src/media/files.rs` (normalization, already covered by `specs/text/media-filenames.md`) | one normal path component, and normalization must not change the name |
| container P7 | `rslib/src/import_export/package/colpkg/export.rs` (placeholder: new collection, one note in the stock basic note type, page size 512, delete journal mode, vacuum, close at schema 11), `rslib/src/storage/schema11.sql`, `rslib/src/storage/upgrades/schema11_downgrade.sql`, `rslib/src/storage/mod.rs` (journal mode delete only at 11) | what Anki's placeholder is (Meguri's own content replaces Anki's stock note type and text) |
| container P11 | `rslib/src/import_export/package/colpkg/export.rs` (entry order: meta, collection, placeholder, media files, media map; all stored; temp file beside the output, atomic rename), `rslib/src/collection/backup.rs` (backups: raw file read after a TRUNCATE checkpoint, so a WAL-mode header; same writer), `rslib/src/storage/sqlite.rs` (close at 18 keeps WAL mode) | layout and the WAL-mode header of Anki's packages |
| container P12, P13; package-ops E2.3 | `rslib/src/import_export/package/colpkg/import.rs`, `rslib/src/import_export/mod.rs` (import error kinds), `rslib/src/backend/import_export.rs` (collection must be closed) | order of Anki's checks (meta, collection copy, open and integrity check, media map, rename); every collection open failure folded into "corrupt" |
| collection-replace R7; package-ops E2.2 | `rslib/src/import_export/package/colpkg/import.rs`, `rslib/src/collection/timestamps.rs`, `rslib/src/storage/collection_timestamps.rs`, `qt/aqt/main.py` (restore backup), `qt/aqt/import_export/importing.py` (collection package import op) | restore and `.colpkg` import are one backend call; it sets `scm` and `mod` to now in milliseconds on the copy, nothing else |
| package-ops E1 | `pylib/anki/collection.py` (collection package export closes the collection), `rslib/src/backend/import_export.rs` | the driver must reopen after an export |

Harvest rows classified with this unit (`conformance/catalog/harvest.csv`):
the colpkg round-trip test (latest-format collection part covered by
`export-import-roundtrip`; media and the legacy option wait for U124) and the
media-map normalization test (latest-format half covered by
`refused-media-map`; the legacy half waits for U120).
