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
