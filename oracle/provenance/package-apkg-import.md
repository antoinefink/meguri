# Package specs: deck package import (U120), legacy reader and classification

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/package/apkg-import.md`
and `specs/package/container.md` P17 and P18 (2026-10-27). No code was copied;
the rules were re-derived in the spec writer's words. Nothing was probed
against the oracle yet: the driver has no deck-package `import` form, so every
I18 scenario must confirm these readings once recorded. Points the spec marks
for confirmation: the stored form of `updateNotes`/`updateNotetypes` (serde
unit-variant strings of the backend enum), the position counter used without
scheduling (the package collection's), and the import order of same-level
decks (Anki sorts unstably; Meguri fixes package id order).

| Spec section | Anki file | Read for |
|---|---|---|
| apkg-import I3, I3.1, I3.2 | `rslib/src/import_export/package/apkg/import/mod.rs`, `rslib/src/dbcheck.rs` (future-id repair), `rslib/src/storage/dbcheck/mod.rs` and its `invalid_ids_*.sql`, `rslib/src/scheduler/upgrade.rs`, `rslib/src/storage/revlog/v2_upgrade.sql`, `rslib/src/scheduler/filtered/card.rs` (queue from type) | order on the package copy (open, id repair, scheduler upgrade only when a card is outside the new queue); the id cutoff of one day; the repair's start value (its CASE compares a quoted literal with an integer, so it always starts at now); v1 conversion of revlog eases, relearning type, filtered-deck step capping and queues, `schedVer` and creation offset |
| apkg-import I3.3 | `rslib/src/notes/mod.rs` (note preparation requires the field count) | a field-count mismatch fails the whole import in Anki |
| apkg-import I5 | `rslib/src/import_export/package/apkg/import/mod.rs` (options written inside the import transaction), `rslib/src/import_export/service.rs` (defaults), `rslib/src/config/bool.rs`, `rslib/src/config/mod.rs`, `rslib/proto/rust.rs` (serde on the update-condition enum), `proto/anki/import_export.proto` | the five keys, their defaults, their write in the undoable op |
| apkg-import I6 | `rslib/src/import_export/gather.rs` | what is gathered (all notes, their cards, decks of cards and parents, original decks with scheduling, default deck skipped without), reset without scheduling (source position counter), filtered-deck handling, presets and FSRS parameter clearing |
| apkg-import I7 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/notetype/mod.rs` (name uniqueness with `+`, preparation, add with current note type), `rslib/src/notetype/merge.rs` (field/template match by id then name), `rslib/src/notetype/schemachange.rs` (sort-field change marks the schema and rewrites notes) | note type outcomes, update conditions (`Always` means mtimes differ without merge), original id, schema equality, the sort-field side effect; an update whose template fronts differ regenerates cards for the target's notes during the note type step (last-deck placement, target position counter, no schema mark when ordinals are unchanged) |
| apkg-import I8 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/import_export/package/apkg/import/cards.rs` | what the merge hands on (field and template ord maps, sibling removal, kind conflict aborting the import); the pair test on the package ordinal |
| apkg-import I9 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/notes/mod.rs` | GUID matching, update rule, incoming mtime kept, id bump by 999, no card generation, conflicting rule |
| apkg-import I10 | `rslib/src/import_export/package/apkg/import/decks.rs`, `rslib/src/decks/addupdate.rs`, `rslib/src/decks/name.rs`, `rslib/src/deckconfig/undo.rs` | level order, timestamp suffix, renamed parents, filtered ancestors, the deck update merge, today-limit remap, presets added only when absent |
| apkg-import I11 | `rslib/src/import_export/package/apkg/import/cards.rs`, `rslib/src/card/undo.rs`, `rslib/src/revlog/undo.rs` | cards for every mapped note, pair skip, id bump, deck remap, day shift columns, revlog only for added cards and only when the id is free |
| apkg-import I12 | `rslib/src/import_export/package/apkg/import/media.rs`, `rslib/src/import_export/package/media.rs`, `rslib/src/media/files.rs` (hash suffix), `rslib/src/text.rs` (reference rewrite, entity re-escaping), `rslib/io/src/lib.rs` (name safety), htmlescape 0.3.1 (minimal entity set) | static names, checked names, use tracking from new and updated notes, hash names, latest-format SHA-1 trusted, legacy hashed |
| apkg-import I13 | `rslib/src/import_export/mod.rs` (log note reduction), `proto/anki/import_export.proto` (log members) | log members and the 80-byte field reduction |
| container P17 | `rslib/src/import_export/package/meta.rs`, `rslib/src/import_export/package/media.rs` | legacy entry names, raw collection and media, JSON map keyed by member number, legacy names normalized after a safety check, missing map read as `{}` |
| container P18 | `qt/aqt/import_export/importing.py` | lowercased file name routing: `.colpkg`, `collection.apkg`, `backup-*.apkg` to the collection import; `.apkg` and `.zip` to the deck import |

## Session s-20261117T144450Z-f49841c9 (spec writer): deferred harvest rows, batch 1

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows and write their scenarios; no code, test text or fixture value was copied, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| package/apkg-import/new-notetype, guid-other-id, conflicting-copy, media-new-note (apkg-import I7, I9, I11.1, I12.2, I12.3) | `rslib/src/import_export/package/apkg/import/notes.rs` (tests module only) | Which behaviors the harvested tests require: new id for a unique GUID with a taken id, an older incoming note kept as duplicate, a GUID match with another id updated, a GUID match under another or remapped note type kept as conflicting, a media reference rewritten to the media map's destination name, and a note type the target lacks added under its own id. The golden values come from oracle runs only. |
| harvest rows h8ee8470f9917, hb92ed5858653, h8672411fcc3d, h522f5887f377 (apkg-import I5, I13, I14.5, I16; package-ops E6.3, E6.4) | `rslib/src/import_export/service.rs` (the import service, the presets read and the response conversion, plus its tests) | What the service tests require: presets read on a fresh collection gives the defaults, a missing package path is an error, a valid package returns a log, and the import response carries both its changes and its log. Used to classify the rows as covered-existing, needs-op or (the response's change flags, h522f5887f377) not-observable. |
| package/notetype-merge/copy-reuse-update, package/notetype-merge/no-merge-update (apkg-import I7, I9) | `rslib/src/import_export/package/apkg/import/notes.rs` | Which no-merge note type rules the harvested tests target: a copy added for a schema mismatch, then found again by original id and schema and updated; the IfNewer update and no-update cases for a matching schema; a field rename with matching ids that is not a schema change; a remapped note type used for new notes. |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| package/apkg-import/media-map-variants (specs/package/container.md P6, P17; apkg-import I16; package-ops E4.1) | `rslib/io/src/lib.rs`; `rslib/src/import_export/package/media.rs` | That media-map names in both latest and legacy maps are refused as corrupt when they are not one plain component, and that a legacy package without a media map has no media to extract. |
| apkg-import I10.2 deck order, parent spelling and today-limit shift; scenarios package/apkg-import/deck-parent-spelling and deck-day-limits | `rslib/src/import_export/package/apkg/import/decks.rs` | Which behaviors the parents and day_limits_should_carry_over_correctly tests require. Goldens come from the oracle. |
| Harvest row hceea58e626d5 mapped to existing package/apkg-import/v1-scheduler and storage/open/v1-scheduler | `rslib/src/scheduler/upgrade.rs` | Which v1-to-v2 card conversions the test requires: relearning reclassification, step capping in filtered decks, learning cards reset without rescheduling, relearning cards kept without rescheduling (not proved by the fixture), and reviews moved out of the new queue. I checked these against the fixture's state before conversion and the existing golden. |
| I11.3 review-log rows with a taken id on apkg import; scenario package/apkg-import/revlog-id-taken | `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/revlog/add.sql`, `rslib/src/import_export/package/apkg/import/cards.rs`, `rslib/src/revlog/undo.rs` | Which callers insert review-log rows without uniquify (import, sync merge, undo restore), and confirming the skip-on-taken-id behavior exists at the pin; golden values come from the oracle |
| package/apkg-import I12.2 (scenario package/apkg-import/media-rename-in-field) | `rslib/src/text.rs`, `rslib/src/import_export/package/apkg/import/notes.rs` | that the harvested replacement test rewrites some media and sound references of one field and keeps others, and that import applies it to the fields of new and updated notes |
