# Deck package export and legacy package export (U122, U123, U124)

Spec-writer notes for `specs/package/apkg-export.md` and
`specs/package/legacy-export.md`. Read at tag 26.09.3 (`refs/anki-26.09.3`);
no code was copied. Written 2026-10-27 by mapping each section of the
committed text to the files that back it. The container rules both specs
build on (entries, `meta`, placeholder, media map, safe names) are
`specs/package/container.md`, whose sources are in `package.md`; the schema-11
conversion is `specs/storage/schema11-downgrade.md` (`storage-downgrade.md`);
the collection package and its media inventory are `specs/package/file-export.md`.

Meguri's own design, with no Anki source: the export operation's input and
result shape, progress, cancellation, staging, verification and atomic
publication (A11, L7 to L10), the typed errors (A12, L11), the broken-reference
and stale-deck refusals (A2, A8 rule 2), the capture check of media files, the
remembered options (A2), and the snapshot source of a legacy `.colpkg` (L4, L8).

| Spec section | Anki files read | Read for |
|---|---|---|
| apkg-export A1, A2 | `qt/aqt/import_export/exporting.py`, `qt/aqt/forms/exporting.ui`, `pylib/anki/collection.py` (deck package export), `proto/anki/import_export.proto` | the entry points, the options and their defaults (scheduling off from a deck's menu), the limit kinds, an empty package for a missing deck |
| apkg-export A3 | `rslib/src/import_export/gather.rs` (notes and cards of a limit), `rslib/src/import_export/package/apkg/export.rs`, `rslib/src/search/mod.rs` | which notes and cards a deck, note, card or whole-collection limit selects; an empty selection |
| apkg-export A4 | `rslib/src/import_export/package/apkg/export.rs` (minimal collection), `rslib/src/import_export/insert.rs`, `rslib/src/storage/sqlite.rs` (new collection), `rslib/src/config/mod.rs` (creation offset) | the package collection Anki builds: its new-collection rows, `crt` and `creationOffset` copied, `curModel`, and the rows inserted |
| apkg-export A5 | `rslib/src/import_export/gather.rs` (decks with parents, note types, review log, presets) | the rows that go in with scheduling: parent decks, the original decks of filtered cards, the review log |
| apkg-export A6 | `rslib/src/import_export/gather.rs` (reset cards and notes, system tags, decks, filtered cards), `rslib/src/scheduler/new.rs`, `rslib/src/card/mod.rs` | how scheduling is removed: cards back to new with their positions, filtered cards restored, filtered decks turned normal, the `marked` and `leech` tags dropped, `nextPos` |
| apkg-export A7 | `rslib/src/import_export/gather.rs` (deck presets), `rslib/src/deckconfig/mod.rs` | presets with and without the option, FSRS parameters kept or stripped, the not-found error for a missing preset |
| apkg-export A8 | `rslib/src/import_export/gather.rs` (id check) | the invalid-ids refusal before anything is written |
| apkg-export A9.1, A9.2 | `rslib/src/import_export/gather.rs` (media names of notes), `rslib/src/text.rs` (media references), `rslib/src/import_export/package/media.rs` (safe normalized names), `rslib/src/import_export/package/colpkg/export.rs` (media files) | the candidate names from fields, missing files dropped, unsafe names failing with the check-media error, following links, size 0 for files of 4 GiB or more |
| apkg-export A9.3 | `rslib/src/latex.rs` (LaTeX extraction, image names), `rslib/src/import_export/gather.rs` (the note type's SVG option) | the LaTeX image name rule and its PNG or SVG extension |
| apkg-export A9.4, A9.5 | `rslib/src/notetype/mod.rs` (media names of a note type), `rslib/src/text.rs` (underscored CSS imports, underscored template references) | the `_` names taken from CSS `url()` and `@import`, and from template texts; form 4 running to `>` |
| apkg-export A10 | `rslib/src/import_export/package/colpkg/export.rs`, `rslib/src/import_export/package/meta.rs`, `rslib/src/import_export/package/media.rs` | entries, their order and methods, zstd frames at the default level without size or checksum, the protobuf media map |
| apkg-export A11 (file name) | `qt/aqt/import_export/exporting.py` | the suggested file name and the characters it replaces |
| apkg-export A14 | `qt/aqt/import_export/exporting.py`, `rslib/src/import_export/package/colpkg/export.rs` | the collection package's media and legacy options |
| apkg-export A15 | `rslib/src/import_export/package/apkg/import/mod.rs`, `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/import_export/package/apkg/import/cards.rs` | what Anki's importer reads from the package (today from `crt` and `creationOffset`, the rows it merges) |
| apkg-export A16 | the files of A4, A6, A9 and A10 | the Anki column of each difference (stock note type rows and `curModel`, arbitrary media order, links, size 0, form 4) |
| legacy-export L1, L2 | `qt/aqt/import_export/exporting.py` (legacy support option), `rslib/src/import_export/package/meta.rs`, `rslib/src/import_export/package/colpkg/export.rs` | that the option writes legacy 2 for both package kinds and nothing else; no legacy 1 writer |
| legacy-export L3 | `rslib/src/import_export/package/colpkg/export.rs` (stored and deflated entries, the placeholder collection), `rslib/src/import_export/package/meta.rs` | entry names and methods of the legacy writer, the placeholder in a legacy package |
| legacy-export L4 | `rslib/src/import_export/package/colpkg/export.rs`, `rslib/src/import_export/package/apkg/export.rs`, `rslib/src/storage/sqlite.rs` (close with a schema version), `rslib/src/storage/upgrades/mod.rs` | the collection closed at schema 11 for the package; the live collection downgraded and reopened for a `.colpkg` |
| legacy-export L5 | `rslib/src/import_export/package/colpkg/export.rs` (media map), `rslib/src/import_export/package/media.rs` | raw media entries numbered from 0, the JSON map and its string escaping |
| legacy-export L6 | `rslib/src/import_export/package/colpkg/export.rs` | the zip writer's 4 GiB limit per entry |
| legacy-export L12 | `rslib/src/import_export/package/apkg/import/mod.rs`, `rslib/src/import_export/package/colpkg/import.rs`, `rslib/src/storage/upgrades/mod.rs` | what Anki's importers do with a legacy 2 package (upgrade on the staging copy) |
| legacy-export L13 | the files of L4 and L5 | the Anki column of each difference (map member order, live downgrade, generic error) |

## Session s-20261117T144450Z-f49841c9 (spec writer): deferred harvest rows, batch 1

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows and write their scenarios; no code, test text or fixture value was copied, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| package/apkg-export#A8; scenario package/apkg-export/selection/invalid-ids | `rslib/src/import_export/gather.rs` (id check after gathering, its tests) | Which ids the export checks: selected cards, notes and the review log only when scheduling gathers it. The comparison against now plus one day, and that a microsecond-scale note id is the malformed case to prove. |
| package/apkg-export#A2, #A3; scenarios package/apkg-export/selection/whole-collection, note-ids, card-ids, deck-children | `rslib/src/import_export/service.rs` (export limit to search mapping, its tests); `rslib/src/import_export/gather.rs` (gathering of notes then the cards of those notes, its tests) | Which behaviors the four limit kinds must prove: an absent limit is the whole collection, note ids and card ids become id searches, and a deck id includes its children. Also that gathering returns the stored notes. Expected values come from the oracle goldens. |
| package/apkg-export#A3, #A10, #A12; scenario package/apkg-export/selection/empty-collection; needs-op for invalid destination | `rslib/src/import_export/service.rs` (deck package export service tests for an empty collection and for an unwritable output path) | The obligations: an empty collection exports a package file without error, and an output path in a missing directory must fail rather than succeed. The second has no scenario form (E7 names are plain session file names). |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| package/apkg-export/media-unsafe-references and package/apkg-export/media-backslash-name (specs/package/apkg-export.md A9.2 rules 2 and 5) | `rslib/io/src/lib.rs`; `rslib/src/import_export/gather.rs` | Which media names count as one plain path component, and that the deck-package export drops gathered references that fail that check, which makes the export side observable. |
| Deck package scenario apkg-export/notetype-underscore-refs (A9.1, A9.4, A9.5) | `rslib/src/media/service.rs` (extract_static_media_files and extract_media_files service methods and the extract_check test) | That the static media extraction for a note type gathers `_` names from CSS and templates the same way the deck export does, and that field extraction is the plain media reference scan. The scenario inputs are an independent design (no fixture shape). Expectations come from the oracle. |
| Media entries of collection packages (compressed and legacy) and the colpkg media round trip and normalization refusal; scenario package/apkg-export/colpkg/media-entries | `rslib/src/import_export/package/colpkg/export.rs`; `rslib/src/import_export/package/colpkg/tests.rs` | The media_file_writing obligation (true size and digest in both modes), roundtrip with media, and the normalization_check_on_export refusal and cleanup (classified needs-op). |
| package/apkg-export A9.1 rule 3, A9.4, A9.5 (scenario package/apkg-export/notetype-underscore-forms) | `rslib/src/text.rs`, `rslib/src/notetype/mod.rs` | which CSS and template reference shapes the harvested extraction test covers, and which note type parts (CSS, both template sides) are scanned for _ names on export |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| package/apkg-export A4, A5, A6, A8 rule 4 (typed reads of notes, cards and revlog, and write-back of read values) | `rslib/src/import_export/gather.rs`; `rslib/src/import_export/insert.rs`; `rslib/src/storage/card/mod.rs`; `rslib/src/storage/card/get_card.sql`; `rslib/src/storage/card/data.rs`; `rslib/src/storage/note/mod.rs`; `rslib/src/storage/note/get.sql`; `rslib/src/notes/mod.rs`; `rslib/src/storage/revlog/mod.rs`; `rslib/src/storage/revlog/get.sql` | Checked that the deck export reads every card, note and revlog row as a typed record (strict columns fail the export; due, odue, csum, time and review kind use defaults) and writes those records back to the package, along with card data serialization |
| package/apkg-export A9.2 step 6 and A16 case-variant row | `rslib/src/import_export/package/media.rs`; `rslib/src/import_export/gather.rs` | Checked that export media selection joins each name to the folder and keeps it only if that exact path exists, with no case folding |
| package/file-export#E7.1 'Differences from Anki' note (symlinks skipped silently; OS metadata files included) | `rslib/src/import_export/package/colpkg/export.rs`; `rslib/src/import_export/package/media.rs`; `rslib/io/src/lib.rs` | Checked that the collection export with media walks the media folder without recursion, keeps only entries whose own metadata (links not followed) is a regular file, so symlinks are skipped, and that it does not exclude .DS_Store or Thumbs.db. |
