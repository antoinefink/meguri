# Package specs: note type merging on deck package import (U121)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/package/notetype-merge.md`
(J1 to J15, 2026-10-27). No code was copied; the rules were re-derived in the
spec writer's words. Nothing was probed against the oracle yet: the driver has
no deck-package `import` form with `merge_notetypes`, so every J15.1 scenario
must confirm these readings once recorded. Points the spec marks for
confirmation: the merged field order with two or more inserted fields
(`apkg-merge-layout`), the remap of the stage-2 note type's own notes
(`apkg-merge-conflict-remap`), the card pair test on the package ordinal
(`apkg-merge-card-ordinal`), and that `Always` governs with equal `mtime_secs`.

| Spec section | Anki file | Read for |
|---|---|---|
| notetype-merge J1, J2 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/import_export/package/apkg/import/mod.rs` | which note types merge (same id; siblings by original id; note-GUID conflicts after the first stage); the kind check and the whole import ending on a mismatch; names and stock kind not consulted |
| notetype-merge J3, J4.1 | `rslib/src/notetype/merge.rs` | field and template matching (id when both have one, otherwise name); where unmatched fields and templates are placed; that one entry can be matched twice when ids or names repeat |
| notetype-merge J4.2, J4.3, J4.4 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/notetype/merge.rs` | which side's values the merged note type keeps by update condition (`Always` without the mtime test); original id; the sort index kept as a number |
| notetype-merge J4.5 | `rslib/src/notetype/mod.rs`, `rslib/src/notetype/schemachange.rs`, `rslib/src/notetype/fields.rs`, `rslib/src/notetype/templates.rs` | the save over the existing rows with origins, normalization, template reference rewrite on rename, mtime and usn stamped on every merged note type |
| notetype-merge J5 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/notetype/schemachange.rs`, `rslib/src/notetype/notetypechange.rs`, `rslib/src/notetype/cardgen.rs` | the per-note-type order of the first stage; rewrite of the existing notes for a changed field list; card generation for changed fronts or new templates; siblings' notes moved with change-note-type maps |
| notetype-merge J6 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/notetype/notetypechange.rs` | the second stage on target notes whose note type differs from the package note's; the merge direction; the maps applied |
| notetype-merge J7 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/import_export/package/apkg/import/cards.rs` | the recorded field and template ord maps; the card map looked up through the target note; the pair test on the package ordinal |
| notetype-merge J8 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/notetype/mod.rs` | removal of merged note types left without notes after the notes step; current note type reset on removal |
| notetype-merge J9 | `rslib/src/notetype/schemachange.rs`, `rslib/src/notetype/notetypechange.rs`, `rslib/src/import_export/package/apkg/import/notes.rs` | which effects mark the schema (field list change, template ordinal change, note type change, removal) and that a front change alone does not |
| notetype-merge J11 | `rslib/src/import_export/package/apkg/import/notes.rs`, `rslib/src/notetype/schemachange.rs` | the note mtime compared by the update rule after the first stage's rewrite |
| notetype-merge J12 | `rslib/src/import_export/package/apkg/import/mod.rs`, `rslib/src/notetype/undo.rs` | the merge inside the one undoable "Import" op |
| notetype-merge J13 | `rslib/src/import_export/package/apkg/import/notes.rs` | the kind conflict as the merge's only error |
| notetype-merge J14 | the files above | each listed difference, read from the behaviors above |

## Session s-20261117T144450Z-f49841c9 (spec writer): deferred harvest rows, batch 1

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows and write their scenarios; no code, test text or fixture value was copied, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| package/notetype-merge/field-identity, package/notetype-merge/stage-two-composed, package/notetype-merge/copy-own-field, package/notetype-merge/template-shift (notetype-merge J1, J3, J4.1, J5, J6, J7, J8) | `rslib/src/import_export/package/apkg/import/notes.rs` | Which merge behaviors the harvested tests target: matching fields by identity while keeping a target-only field; keeping a target-only template while renaming a matched one and shifting the template map; merging earlier copies by original id; the GUID-driven second stage on a note type with no original id; how the field maps of the two stages chain. Used only to choose what to observe; expectations come from oracle goldens. |
| package/notetype-merge/align/field-order, template-order, new-fields, new-templates, field-renamed (J3, J4.1, J4.3, J7) | `rslib/src/notetype/merge.rs`; `rslib/src/import_export/package/apkg/import/notes.rs` | Which matching behaviors the harvested merge tests cover: matching by id or by name, aligning order, appending new entries with or without ids, keeping a renamed entry by its id. Also which direction the import merges in and which record it writes, so the scenarios exercise the import path. Expected values come only from the oracle goldens. |
| package/notetype-merge/align/repeated-field-id, repeated-template-id, repeated-field-id-always, repeated-template-id-always (J3, J4.3, J14 repeated ids) | `rslib/src/notetype/merge.rs`; `rslib/src/import_export/package/apkg/import/notes.rs`; `rslib/src/notetype/mod.rs` | The robustness obligation for an incoming record that repeats an id past the end of the target's list. Which record the import writes when T governs and when the incoming governs, which is why both variants are recorded. That a note type save does not reject repeated field ids, so a scenario can build such a package with legacy_update. The outcomes themselves come only from the four goldens. |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| package/notetype-merge J1, J5 (one ascending pass of adds and stage-1 merges, name uniquing against live state), J12 write order; package/apkg-import I8 "Called" | `rslib/src/import_export/package/apkg/import/notes.rs` (import_notetypes, add_notetype, update_notetype, update_or_merge_notetype); `rslib/src/notetype/mod.rs` (ensure_notetype_name_unique, add_or_update_notetype_with_existing_id_inner); `rslib/src/storage/notetype/mod.rs` (get_notetypes_for_search_notes, rowid order) | Confirming that Anki takes the note types in a single ascending pass, with adds and merges mixed, and that each one's name is made unique against live storage, so renames by earlier merges are visible. |
