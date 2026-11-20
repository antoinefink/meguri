# Note type structure changes

Spec: notetypes/structure-changes
Requirements: R789 (template edits), R918 (core side), R920, R922, R923, R924, R926, R927, R932, R933, R934, R949, R950, R1475, R1477, R2008 (core side); supports AE34
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/structure/
Status: Under review (R915; round 9, decided 2026-11-20) for the lines of S1.2 that begin "Changed 2026-11-20 (round 9)": do not implement them until approved. Every other section is in force and unchanged.

What a save of a note type does to its notes and cards when it adds, removes, moves or renames fields or templates, changes the sort field or a front template; the preview of a template removal; and removing a note type. Restore to Default is `notetypes/restore-to-default`. The note type's own rows, stamps and save normalization are `notetypes/records` (N3, N4, N6); the template checks `notetypes/validation`; which saves are schema changes and how `col.scm` is set `notetypes/schema-changes` (G2, G3, G5, G6); the safety backup of a structural edit `package/backup-triggers` (T6.1, T6.2). Field renames are `notetypes/field-rename` (F1 to F5); this file adds what a save with removals, moves, additions or template changes does on top of them. This file builds on: the note save's field normalization, sort field and checksum (`notes/note-records` N2, `text/checksum-and-sort-field` C1 to C4), card generation for a whole note type (`notes/card-generation` G9) with its placement and positions (`notes/deck-placement-and-position` D4, P2), card and note removal with graves (`notes/note-records` D2, `storage/graves` G3), the template parse result with source spans (`render/template-syntax` T3 to T6, T12), and the plan/commit rules of `engine/op-model` O6 and O7.

Terms: the **base** is the stored note type a draft was loaded from (its `notetypes` row, its `fields` rows and its `templates` rows); a **base ordinal** is a field's or template's position in the base; `now_s`, `now_ms` and local stamps are `storage/ids-usn-mtime`'s.

## S1. The draft apply op

The field editor (U201), the template editor (U200) and the local API's note type edits (U141, U142) save through one op that takes a whole draft of one note type. One draft, however many changes it holds, is one transaction and one undo step named "Update Note Type" (U25 step id `update_note_type`).

Decided 2026-10-27: a draft whose only differences from the base are field renames has exactly the result of the field rename op (`notetypes/field-rename` F2 to F4), and the draft apply op may hand it to that op; either way the result, the journal and the undo step are the same. A draft that renames fields and also makes plain-save changes (field options, the note type's or a template's name, styling, template texts, deck overrides, browser fonts) is not handed on, since F2 refuses unrelated changes: the draft apply op itself applies F2 to F4's rename (rewrite, requirements, generation) together with the plain save of the other changes (`notetypes/records` N3), in one transaction and one undo step, with the result of S1.3.

### S1.1 Input

- The note type id and the **base token**: the U29 `AggregateDigest` of the base aggregate, taken when the draft was loaded; the same token and comparison as `notetypes/field-rename#F2` and the plain save (an `engine/op-model` O7 precondition). It is one digest over the note type's id, name and config and every `fields` and `templates` row with the ordinal it was read with (the template rows' `mtime_secs` and `usn` included); the `notetypes` row's own `mtime_secs` and `usn` are left out. It is not an O7 `RowDigest`.
- The draft's fields, in their new order. Each holds its **origin**, the base ordinal it comes from, or none for a field the draft adds; its name; its options (`model/protobuf-records` P6.2: sticky, RTL, font name and size, description, plain text, collapsed, exclude from search). A field with an origin keeps the base field's id, tag, protection, `other` bytes and unknown protobuf fields; a field without one is new (`notetypes/records` N5.2: fresh random id).
- The draft's templates, in their new order, each with its origin or none, its name, front, back, browser front, browser back, deck override, browser font name and size. A template with an origin keeps the base template's id, `other` bytes, unknown fields and the row's `mtime_secs` and `usn` (`notetypes/records` N3.3); one without is new (N5.3).
- The note type's name and styling.
- The sort field: a position in the draft's field list, or absent to keep the base's sort field (S3 step 4).
- `skip_checks`: as `notetypes/validation` V1 sets it per caller: true for the field editor's saves, false for the template editor's. Decided 2026-10-27: false for the local API's note type edits (U141, U142), field edits included, since Anki's own note type saves outside its field editor are checked by default; a local API template update that names an unknown field is refused by V3.

There is no confirmation input (`notetypes/schema-changes#G4`). A destructive-mapping preview (the fields, templates or cards the save removes; S11 for templates) is the caller's, accepted before the op is submitted.

Fields and templates are matched to the base by origin only, never by name or id: ids are optional and clones repeat them (`notetypes/records` N2, N7), and names change in the same draft. For a field rename the origin is the ordinal `notetypes/field-rename#F2` captures with the field's id. Each base ordinal appears at most once as an origin; a base field or template whose ordinal appears nowhere is **removed**. A draft whose origins repeat or lie outside the base is an `InvalidArgument`.

The note type's kind (standard or cloze) is not part of the draft; it changes only through Restore to Default with a forced kind and through Change Note Type (U35).

### S1.2 Refusals

Changed 2026-11-20 (round 9): the draft also carries **confirmed losses** (`notetypes/draft-save` D5 step 5): for each removed base field, the count of notes with content (`text/field-emptiness` E1) the user confirmed, and for each removed base template, the count of cards the user confirmed (zero for each when no question was shown). After refusal 5, inside the write transaction, the op counts them again on the live rows; when any live count is larger than the confirmed one it writes nothing, records no undo step and returns `LossesChanged { summary }`, the summary `notetypes/draft-save` D2 defines, read in that transaction. This is a precondition, like the base token, not a confirmation input (`notetypes/schema-changes` G4). Callers with no question (the local API) pass the counts they read.

Checked in this order; each writes nothing and records no undo step. Before them all, a draft whose origins repeat or lie outside the base (S1.1) is refused with `InvalidArgument`, which also writes nothing and records no undo step.

1. **`Stale`**: the live aggregate's digest differs from the base token (F2). The draft was made against another version of the note type (a sync, the local API or another window changed it); the caller reloads. The op never re-applies an old draft to a changed note type on its own. Decided 2026-10-27: the token is checked twice: on the planning snapshot, and again as the `engine/op-model` O7 precondition inside the write transaction, after the safety backup (S3 step 5). A mismatch at either check returns `Stale` with no replan; a safety backup already taken stays (T6.2), and the caller's next submission, from the reloaded note type, is a new op that T6.1's coalescing may cover.
2. **`invalid_notetype`** (`notetypes/validation` V2): no field, no template, or a name the normalization refuses. Removing the last field is refused this way (R922: "Notes require at least one field."), and so is removing the last template (R933). Such a draft is never rewritten (S3 step 2). Scenario `notetypes/structure/remove-last`.
3. **`ProtectedField { field }`**: the draft removes a field whose base `prevent_deletion` is true (R922). Anki's core does not refuse this; its editor never offers it. Meguri refuses it in the core so that no caller (an integration included) removes a Cloze or Image Occlusion field that its notes depend on. No scenario covers it.
4. **`NoteFieldCountMismatch { note }`**: the save rewrites notes (S5) and a note of the type stores more field values than the base has fields (only a damaged collection holds one; Check Database repairs it, `notes/note-records` F1). Rewriting would drop the extra values, so Meguri refuses instead. A note with fewer values is rewritten, its missing values read as empty. Anki pads and truncates when the field list changes, and refuses the whole save when only the sort field changes; no scenario covers either.
5. The template checks of `notetypes/validation` V3 when `skip_checks` is false, run on the templates as S4 leaves them.
6. **`SafetyBackupFailed { cause }`** (`package/backup-triggers` T6.2, T9): the save is schema-classified (S2), so it is a structural edit (`notetypes/schema-changes` G2) and takes a safety backup after planning and the checks above, before its first write (T6.1, T6.2, with T6.1's coalescing); the backup failed. A save that is not schema-classified takes no backup.

Planning can also end with `Cancelled` (S10).

### S1.3 Result

On success: the note type id; `notes_rewritten` (S5); `cards_added` (S7); `cards_removed` and `cards_renumbered` (S6); `notes_without_cards`, the notes of the type that had at least one card before the save and have none after it (R933); and the schema reasons the save carried (G2). A save that changes nothing writes nothing (`notetypes/records` N3.1), takes no backup and reports zeros.

## S2. What the draft changes

After the save normalization of `notetypes/records` N4 (names, uniqueness, NFC; renames as `notetypes/field-rename#F2` computes them), the op compares the draft with the base:

| Change | When | Schema reason (`notetypes/schema-changes` G2) |
|---|---|---|
| fields changed | the draft's origins by position are not exactly 0, 1, …, n−1 for a base of n fields: a field added, removed or moved (`notetypes/schema-changes#G3`). A draft that removes one field and adds another at the same position changes the fields | `fields_changed` |
| sort field changed | the field at the sort field's final position (S3 step 4) is not the base's sort field: its origin differs from the base's `sort_field_idx`, or it has none. So the base sort field removed counts, and a move that keeps the same field as sort field does not (`notetypes/schema-changes#G2`: another field made the sort field) | `sort_field_changed` |
| templates changed | the draft's origins by position are not exactly 0, 1, …, n−1 for a base of n templates: a template added, removed or moved. A draft that removes one template and adds another at the same position changes the templates | `templates_changed` |
| fields renamed | a field with an origin whose final name differs from the base field's name (F2: code points compared exactly, so a case-only rename counts) | none |
| fields removed | a base field that is no origin | (part of fields changed) |
| templates removed | a base template that is no origin | (part of templates changed) |
| fronts changed | a template was added, or the list of front texts by position differs from the base's (compared exactly) | none by itself |

The schema reasons decide `col.scm` (G3, G5) and the safety backup (S1.2 item 6). A save with renames, options, names, styling, back or browser texts, deck overrides or browser fonts and nothing else is not a schema change (R950); without renames and front changes it touches no note and no card and is a plain save (`notetypes/records` N3, N8).

How the editors move a field or a template (a move, never a swap; a move to the current position changes nothing) is `notetypes/editor-actions` A9.

## S3. Order of the save

1. Normalize the draft (`notetypes/records` N4 steps 1 to 5, 7) and classify it (S2).
2. Rewrite template references for renamed and removed fields (S4). A draft with no field or no template skips this step and steps 3 and 4: nothing is rewritten, and step 5 refuses it (S1.2 item 2). S4.3 therefore always has a first field.
3. Compute the card requirements from the templates S4 leaves (`notetypes/records` N6; S8).
4. Sort field: the draft's sort field at its final position. When the draft keeps the base's sort field and that field was removed, `notetypes/records` N4 step 6 applies: the index keeps the base value, lowered to the last position when past the end.
5. Plan (S10) and run the checks (S1.2 items 1 to 5). For a schema-classified save, take the safety backup (S1.2 item 6). Then, in one write transaction: check the base token again (S1.2 item 1; a mismatch rolls back and returns `Stale`), rewrite the notes (S5), remove and renumber cards (S6), generate cards (S7), write the note type rows (`notetypes/records` N3.2) and, for a schema-classified save, set `col.scm` (G5). `col.mod` moves once.

The order inside the transaction is not observable except that notes are rewritten before cards are generated, so generation reads the new field layout.

## S4. Rewriting template references (R923, R924)

Runs when the save renames or removes at least one field.

### S4.1 Renames

Renamed fields are rewritten by `notetypes/field-rename#F3`: the simultaneous rename map, the four texts of every template, empty browser texts and texts with a structural error left as they are, the matching of keys, the append of the first field, the text's own delimiters, and everything else left byte for byte (deviation `template-rewrite-keeps-text`, S14). The precise terms F3 relies on:

- A key is compared after `render/template-syntax` T4's trimming and T5's split and nothing else: `{{ Front }}`, `{{{Front}}}` and `{{ text:Front }}` name `Front`; `{{text: Front}}` names ` Front` and is not edited.
- A rewritten replacement is written over its whole source span (T12) as the canonical replacement: the opening delimiter, each filter piece in source order followed by `:`, the new name, the closing delimiter. A rewritten conditional, negated conditional or close tag is the opening delimiter, `#`, `^` or `/`, the new name and the closing delimiter.
- The delimiters are `{{` and `}}`, or `<%` and `%>` in a text that starts with the alternate-delimiter directive (T2). Names inside comments are never edited.

### S4.2 Removals

A save that removes fields extends F3's pass. Each removed field's base name enters the map as "removed", together with the renames; each tag is still looked up once, so a rename to a removed field's old name rewrites only the references that named the renamed field. In each text F3 processes, outside comments:

- a replacement whose key names a removed field is deleted over its whole span;
- a section whose key names a removed field loses its opening and its matching closing tag; its contents stay in place.

F3's skips (empty browser texts, structural errors) apply unchanged.

### S4.3 The appended first field

F3's append rule runs on every save that renames or removes at least one field, after the map is applied. Let F be the first field of the saved note type (position 0 after the save's moves).

- Standard note types: a front or browser front left with no replacement tag at all (of any key, special names, unknown names and the empty key included; sections do not count) gets the replacement of F appended at its end, in the text's delimiters (`{{F}}`, or `<%F%>`).
- Cloze note types: a front, back, browser front or browser back left with no replacement whose filter chain holds a `cloze` piece gets `{{cloze:F}}` appended at its end, in the text's delimiters.
- Nothing else is added: no space or line break. The rule applies to every template's texts, also those the map did not touch: a front made only of sections and text gets F appended on any save that renames or removes a field. Backs of standard note types never get a reference.

### S4.4 Examples

Standard note type with fields `Front`, `Back`, `Extra`; the rows that say cloze use a cloze note type with fields `Intro`, `Text`, `Extra`. The rename rows show F3; the removal rows show S4.2 and S4.3.

| Text | Save | Result |
|---|---|---|
| `{{Front}}<br>{{text:Front}}` | `Front` renamed `Question` | `{{Question}}<br>{{text:Question}}` |
| `{{#Front}}{{Back}}{{/Front}}{{^Front}}-{{/Front}}` | `Front` renamed `Question` | `{{#Question}}{{Back}}{{/Question}}{{^Question}}-{{/Question}}` |
| `{{FrontSide}}<!-- {{Front}} -->` | `Front` renamed `Question` | unchanged |
| `{{ Front }} {{{Back}}} {{ text:Extra }}` | `Extra` renamed `Note` | `{{ Front }} {{{Back}}} {{text:Note}}` (Anki: `{{Front}} {{Back}}} {{text:Note}}`) |
| `{{=<% %>=}}<%Back%> {{x}} <% Extra %>` | `Extra` renamed `Note` | `{{=<% %>=}}<%Back%> {{x}} <%Note%>` (Anki: `{{Back}} {{x}} {{Note}}`) |
| `{{Front}} {{Back}}` | `Front` and `Back` swap names | `{{Back}} {{Front}}` |
| front `{{Front}}` | `Front` removed | `{{Back}}` (emptied, then F = `Back` appended) |
| front `{{Back}}{{#Front}} ({{Front}}){{/Front}}` | `Front` removed | `{{Back}} ()` |
| front `{{#Extra}}hint{{/Extra}}{{Back}}` | `Back` removed | `{{#Extra}}hint{{/Extra}}{{Front}}` (no replacement left) |
| front `{{#Back}}hello{{/Back}}{{Tags}}` | `Extra` renamed | unchanged (`{{Tags}}` is a replacement) |
| back `{{#Back}}{{Back}} {{ Front }}` (unclosed) | `Front` renamed | unchanged (structural error) |
| empty browser front | any rename or removal | stays empty (Anki: `{{Front}}`, the first field) |
| cloze front `{{Intro}}<br>{{cloze:Text}}` | `Text` removed | `{{Intro}}<br>{{cloze:Intro}}` |
| cloze back `{{cloze:Text}}<br>{{Extra}}` | `Text` removed | `<br>{{Extra}}{{cloze:Intro}}` |

Scenarios `notetypes/structure/rename-field`, `remove-field`, `cloze-fields` and `rewrite-format` (S16).

## S5. Notes (R926, R927)

Runs when fields changed or the sort field changed (S2). Renames, option changes and template changes alone never touch a note (F4).

- **Which notes.** Every note whose `mid` is the note type, selected inside the write transaction, so a note added after planning began is rewritten too (`engine/op-model` O7, predicate sets).
- **New values.** For each field of the saved note type in order: the value the live note holds at the field's origin, or empty for a field without one or an origin past the note's stored values. Values of removed fields are dropped. The values are read from the live row inside the transaction (O7 re-derivation), so a field edit committed while the op was planning survives, moved to its field's new position.
- **Normalization and derived columns.** Every value goes through the note save's normalization (`notes/note-records` N2: control characters, NFC per `normalize_note_text`), then `sfld` is computed from the saved sort field and `csum` from the saved first field by the same functions the note save uses (`text/checksum-and-sort-field` C2 to C4). Moving another field to position 0 changes `csum`, so the duplicate check (AE34, `notes/field-check`) keeps finding duplicates by the new first field.
- **Stamps.** Every note of the type gets `mod` = `now_s` and `usn` −1, whether or not its row's values changed (an empty field moved among empty fields, a sort field change that leaves a note's `sfld` as it was). R927 rewrites every note of the type; Anki stamps them all.
- `tags`, `guid`, `flags`, `data` and the note's cards are not changed here.

| Base fields; note | Save | `flds` | `sfld` | `csum` from |
|---|---|---|---|---|
| `A B C`, sort `A`; `<b>私</b>`, `x`, `007` | `C` moved to 0 | `007`, `<b>私</b>`, `x` | `私` (sort field `A`, now at 1) | `007` |
| same | sort field `C` | unchanged | integer 7 | `<b>私</b>` |
| same | `D` added | `<b>私</b>`, `x`, `007`, (empty) | `私` | `<b>私</b>` |
| same | `A` removed, sort was `A` | `x`, `007` | `x` (index 0 kept) | `x` |
| `A B C`, sort `C`; `a`, `b`, `c` | `C` removed | `a`, `b` | `b` (index 2 lowered to 1) | `a` |

Scenarios `notetypes/structure/reorder-fields`, `combined-draft`, `remove-field`; `first-field-duplicates` shows the duplicate check finding notes by the new first field after a move.

## S6. Cards when templates change

Runs when templates changed (S2). The cards are the note type's cards, selected inside the write transaction by their note's `mid`.

1. **Removed templates.** Every card whose `ord` is the base ordinal of a removed template is removed with its card grave (`notes/note-records` D2, `storage/graves` G3). Its note stays, also when it is left with no card (R933 counts such notes); the note's next save or Check Database gives it a card again (`notes/card-generation` G6). Its review log stays.
2. **Moved templates.** Every card whose `ord` is the base ordinal of a kept template now at another position gets that position as `ord`, `mod` = `now_s` and `usn` −1. Cards of templates whose position did not change are not touched.
3. Removal comes before renumbering. Cards whose `ord` is past the base's last template are left alone: a standard note type has them only in a damaged collection (Check Database removes them), and a cloze note type's cards past ord 0 are cloze numbers, which a draft never changes. Restore to Default with a kind change handles such cards itself (`notetypes/restore-to-default#D4.1`).

Cloze note types have one template; their card ordinals are cloze numbers, so the editors never add, remove or move a cloze note type's template (U200). The op does not guard against it.

| Base templates; a note's cards | Save | Cards after |
|---|---|---|
| `One Two Three`; ords 0, 1, 2 | `Three` moved to 0 | ords 1, 2, 0 (all three stamped) |
| `Three One Two`; ords 0, 1, 2 | `One` removed | ord 1 removed with a grave; ord 2 becomes 1 (stamped); ord 0 untouched |
| `Three Two`; a note with ord 0 only | `Three` removed | its card removed; the note stays with no card |
| `One Two`; ords 0, 1 | `Two` removed and `Three` added at position 1 (the count stays 2) | ord 1 removed with a grave; ord 0 untouched; S7 then gives the note a `Three` card at ord 1 when its front requires one |

Scenario `notetypes/structure/remove-template`.

## S7. Card generation (R789)

When the fronts changed (S2: a template added, or any front text differs by position, which includes fronts S4 rewrote and template removals and moves), the save generates the cards the saved templates require, by `notes/card-generation` G9 against the saved note type and the rewritten notes:

- Only notes that have at least one card after S6 are visited (G9 step 1). A note left with no card by a template removal, or one that a raw write left without cards, gets none from a later template add or front edit.
- Existing cards are never removed or changed by generation (G7, R788): a front edit that stops rendering for some notes leaves their cards; Empty Cards (U35) removes them.
- Placement is `notes/deck-placement-and-position` D4: D3's list (override, the note's shared home deck, the note type's last deck `_nt_<id>_lastDeck`, Default). Deviation `deck-placement-regeneration`: Anki skips the last deck here, so a new card of a note whose cards are split across decks goes to Default.
- Positions are P2: a new card takes an existing new sibling's position, otherwise one counter value per note.
- The plan half runs on the draft before the save (G9's plan with the in-memory note type); the commit half writes the cards inside the save's transaction, re-deriving each note's existing cards and field values from the live rows (U27). So the cards and the note type are one journal and one undo step.

Example (scenario `notetypes/structure/add-template`): template `Reverse` (`{{Back}}`) added to a note type with Forward cards: a note whose Forward card is new gets a Reverse card at the same position; a note whose Forward card is in review gets the next counter value; a note with an empty Back gets none; a note with no card gets none, whatever its fields.

Scenarios `notetypes/structure/add-template`, `add-template-placement`, `edit-front`, `remove-field`.

## S8. Card requirements after renames and removals

The saved `reqs` are computed from the templates as S4 leaves them, with the saved field list (`notetypes/records` N6; for renames `notetypes/field-rename#F3`). Deviations `notetype-reqs-after-field-change` (a rename) and `notetype-reqs-after-field-change-removal` (a removal): Anki computes them from the fronts before its rewrite, so right after a field rename or removal its stored requirement for a front that named the renamed or removed field is `none` (or misses the appended field), until a later save of the note type recomputes it. Scenario `notetypes/structure/reqs-after-rename`.

## S9. The note type rows and stamps

- The note type's rows are written by `notetypes/records` N3.2: changed `fields` and `templates` rows by `ord`, the `notetypes` row stamped (`mtime_secs` = `now_s`, `usn` −1) whenever anything in the aggregate differs. A template with an origin keeps its row's `mtime_secs` and `usn` wherever it moves; a new one gets 0 and 0. Restore to Default is the exception: its origins only map notes, cards and the report, and every restored field and template takes the stock values, ids and row stamps of `notetypes/restore-to-default` D3 (D4).
- A schema-classified save sets `col.scm` to `now_ms` (`notetypes/schema-changes` G5) and so passes the storage guard (G6). A save that only renames leaves `scm` (F4).
- Deck overrides are written as values of the draft and are never a schema change (R934, R950); creating the deck an override names is the template editor's job before it saves (U200): `notetypes/editor-actions` A11, one `decks/decks` D5 "Add Deck" step per template whose override is still a name, after the draft's live checks and just before the save; a D5 refusal stops the save with the note type not saved (decks that earlier steps created stay); nothing is created when the draft is discarded.
- The save reports its structure as `engine/change-stream#S2.5` requires (approved with R2066; its guard refuses an unreported change with `StructureNotReported`): its reasons, and maps built from the origins: entry i of `field_map` is the origin of saved field i, or −1 when it has none; `template_map` likewise. Every save that adds, removes, moves or renames a field or a template, a template rename alone included, or changes the sort field reports a `layouts` entry; a save that only renames reports identity maps and no reason code. A note type removal (S12) reports the id in `removed` with reason 4 `notetype_removed`; Restore to Default reports its layout from `notetypes/restore-to-default` D4's origins, with reason 5 `notetype_restored` only when D5 classifies it.

## S10. Planning, progress and cancel (R927)

- The op plans on a reader snapshot (`engine/op-model` O6): the diff, the rewritten templates, the per-note field remap, the card removals and renumbering, and the generation plan. Planning reports progress (notes planned out of notes of the type, at most about ten times a second) and can be cancelled; a cancelled plan writes nothing, takes no backup and returns `Cancelled`.
- The safety backup of a schema-classified save runs after planning and before the write phase (`package/backup-triggers` T6.2). The draft apply op never replans after `Stale` (S1.2 item 1): it returns `Stale`, and the backup already taken stays. Restore to Default replans on its own (`notetypes/restore-to-default#D6`) and takes no second backup then.
- The write phase re-derives what it writes from live rows (S5, S7) and selects notes and cards by `mid` inside the transaction, so notes and cards added or edited while planning are handled; the base token (S1.2) covers the note type's own rows. A save whose write phase would exceed O6's 200 ms aim declares the long write `notetype_structure_change` (O8, O9), so answers queued meanwhile wait in the session's overlay. Reordering the fields of a 50,000-note type completes with progress within the U12 baseline.
- An injected failure anywhere in the write phase rolls everything back: the note type, its notes and cards stay as they were. The safety backup already published stays.

## S11. Removal preview (R933)

A read-only query for the template editor's removal confirmation: "Delete the 'X' card type, and its N cards?" plus how many notes would be left with no cards (deviation from Anki's wording, which gives only the card count).

- **Input**: the note type id and a set of base template ordinals proposed for removal (the template being removed, plus those the draft already removes).
- **Output**: for each listed ordinal, the number of cards of the note type with that `ord`; and `notes_without_cards`, the number of notes of the type that have at least one card and whose cards all have a listed ordinal.
- It reads one snapshot and writes nothing. Notes that already have no card are not counted.

Example: a note type with templates `Forward` and `Reverse` where five notes have only their `Reverse` card: the preview for `{1}` gives the `Reverse` card count and 5 notes left with no cards.

## S12. Removing a note type (R918)

Changed 2026-11-20 (round 9; decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day): the input also carries the **confirmed counts** the delete question showed (W12: the note type's notes and cards). Inside the write transaction the removal counts them again; when either live count is larger it writes nothing, takes no step and returns `LossesChanged { notes, cards }`, and the question shows again with the new counts. A precondition, not a confirmation input.

- **Input**: the note type id. There is no confirmation input: the delete question of R918 ("Delete this note type and all its cards?", or "Delete this unused note type?") is the caller's, accepted before the op is submitted (`notetypes/schema-changes` G4, which supersedes R918's schema-change confirmation).
- **Refusals**, writing nothing: `LastNotetype` when it is the collection's only note type (R918; Anki's core instead removes it and adds a new stock Basic note type, which Meguri's callers never want); `SafetyBackupFailed` when the safety backup fails (`package/backup-triggers` T6.1 row "deleting a note type", T6.2: taken after planning, before the first write; R2008).
- Removing a note type that no longer exists succeeds, changes nothing and takes no backup. Decided 2026-10-27: T6.1's "always" applies to a removal that removes something; with nothing to remove there is nothing to protect.
- Decided 2026-10-27: T6.1's coalescing does not apply to a removal. Removal has its own T6.1 row ("always"), apart from the structural-edit row the coalescing serves, and it deletes every note of the type, so each removal that removes something takes its own backup, also within 10 minutes of another structural edit.
- **Effect**, one transaction and one undo step named "Remove Note Type" (U25 step id `remove_note_type`):
  1. Every note whose `mid` is the note type, in ascending id order: its cards are removed with card graves, then the note with its note grave (`storage/graves` G3). No line goes to the deleted-notes log (`notes/note-records` D1). Review-log rows stay.
  2. `col.scm` = `now_ms` (reason `notetype_removed`, G2, G5).
  3. Every config key that begins with `_nt_<note type id>_` is removed (`storage/config-table` C10): its last deck and any per-template keys add-ons keep there.
  4. The note type's `notetypes`, `fields` and `templates` rows are deleted. No grave is written for a note type.
  5. `curModel` is set to the id of the remaining note type whose name comes first under the `unicase` comparison (`text/unicode-primitives` U2; names are unique under it, so there is no tie), written only when it differs (`storage/config-table` C4), whichever note type it named before. Anki takes the first row of an unordered read of the note types, which SQLite answers from the name index in that order; Meguri sorts explicitly. With the stock kinds present that is usually `Basic`, but a note type named `Alpha` comes first.
  `col.mod` moves once. Keys `_deck_<id>_lastNotetype` that name the removed note type stay; decks, presets and other note types are untouched.
- The removal lists the note type in the structure report's `removed`, with reason 4 `notetype_removed` (`engine/change-stream#S2.5`, required; S9).
- **Undo** restores the rows, removes the graves the removal added, and restores `col.scm`, the removed config keys and `curModel`.

Scenarios `notetypes/structure/remove-notetype` and, for the undo, `undo-removals` (S16).

## S13. Undo

- A draft apply, a removal and a restore are each one undo step (U25): undo restores every row from its before-image (the note type rows with their stamps, notes with their `mod` and `usn`, renumbered cards, removed cards and notes) and removes the graves the step added; cards the step added are deleted without graves, since they were never synced; `col.scm` returns to its value before the step (G5). Redo applies the after-images. Undo and redo take no safety backup of their own (`package/backup-triggers` T6.1 lists the ops, not their undo).
- An undo or redo of a large step runs as a job with the same declared write (U25).
- Anki's undo gives the same results (`Update Note Type`, `Remove Note Type`), `col.scm`, the removed `_nt_<id>_` keys and `curModel` included. Scenarios `notetypes/structure/undo-add-template` (a template add: generated cards removed without graves, `nextPos` and `col.scm` back; then redo), `undo-removals` (a template removal and a note type removal) and `undo-restore` (`notetypes/restore-to-default` D6) compare undo and redo with Anki's (`specs/conformance/undo-ops.md`). Meguri's tests cover the rest (S16).

## S14. Deviations

| Id | Requirement | Meguri | Anki |
|---|---|---|---|
| `template-rewrite-keeps-text` | R923 | S4 and F3: only the tags that name a renamed or removed field are rewritten; empty browser texts stay empty; the alternate-delimiter directive stays | rewrites every tag of every text in canonical form (white space inside tags and extra leading `{` dropped, close tags written with their open tag's key), drops the directive and writes every tag with `{{` and `}}` (an alternate-delimiter text holding a literal `{{` breaks there), and appends the first field to empty browser fronts (and `{{cloze:F}}` to empty cloze browser backs) |
| `notetype-reqs-after-field-change`, `notetype-reqs-after-field-change-removal` | R903 | S8: `reqs` from the final fronts | `reqs` from the fronts before the rewrite |
| `deck-placement-regeneration` | R792 | S7: D3's full list, the note type's last deck included | no last deck: such cards go to Default |

`conformance/deviations.yaml` holds the entries of the scenarios that have goldens here: `template-rewrite-keeps-text` (`rewrite-format`), `notetype-reqs-after-field-change`, `notetype-reqs-after-field-change-removal` and `deck-placement-regeneration`. Not in `conformance/deviations.yaml` because no scenario shows them: the protected-field and extra-values refusals (S1.2 items 3 and 4), `LastNotetype` (S12), the safety backup (`notetypes/schema-changes` G9).

## S15. Differences from the Product Contract's wording

- R924 says the first remaining field is inserted when a front is left "with no field reference and no conditional". Anki inserts it whenever no replacement tag is left, sections notwithstanding, and on every save that renames or removes a field, not only into fronts that lost a reference; Meguri follows Anki (S4.3).
- R922's protection refusal is enforced in the core as well (S1.2 item 3).
- R918's "requires the schema-change confirmation" is superseded (S12; `notetypes/schema-changes` G4).

## S16. Scenarios

All under `conformance/scenarios/notetypes/structure/`. Schema changes run as accepted (`specs/conformance/notetype-ops.md` T9). Every listed scenario is present and recorded. Keep them inactive until their ops and the capabilities of `notetype-ops.md` T10.1 exist. U34 makes active those that need only its own ops and U27's and U29's adapters, after their required checks pass.

| Scenario | Covers | Golden | Meguri also needs |
|---|---|---|---|
| `reorder-fields` | S2, S5 (moves, sort field, `csum` from a new first field), G2, G5 | recorded | |
| `combined-draft` | S1, S5: a rename, an added field and two moves in one save | recorded | |
| `reqs-after-rename` | S8; deviations `notetype-reqs-after-field-change` and `-removal` | recorded | |
| `add-template` | S7, G9, P2, notes with no card skipped | recorded | |
| `edit-front` | S2 fronts changed, S7, G7: cards added, none removed, no schema change | recorded | |
| `remove-template` | S6: move, removals with graves, a note left with no card | recorded | |
| `add-template-placement` | S7 placement; deviation `deck-placement-regeneration` | recorded | `setup.decks` (U28) |
| `rename-field` | F3 in all four texts, renames swapped, S5 (notes untouched), no schema change | recorded | |
| `remove-field` | S4.2 removal and S4.3 append, S5, S7 (a card the rewritten front now requires) | recorded | |
| `cloze-fields` | S4 on a cloze note type: rename inside sections, removal with the cloze append on all four texts | recorded | |
| `rewrite-format` | F3's skips and text preservation; deviation `template-rewrite-keeps-text` | recorded | |
| `remove-notetype` | S12 | recorded | |
| `undo-add-template` | S13: undo of a template add removes its cards without graves and returns the note type, `nextPos` and `col.scm`; redo gives the same cards back | recorded | `undo`, `redo`, `undo_status` (U38's undo adapter) |
| `undo-removals` | S6, S12, S13: undo of a template removal (cards back, graves gone) and of a note type removal (notes, cards, rows, `_nt_<id>_` keys back; `curModel` moved from the removed note type to `Alpha` and back) | recorded | the same |
| `undo-restore` | `notetypes/restore-to-default` D6, S13: undo and redo of a structural restore | recorded | the same |
| `remove-last` | S1.2 item 2: removing the last field, then the last template, refused with `invalid_notetype`, nothing written | recorded | |
| `first-field-duplicates` | S5: `csum` from the new first field, so the duplicate check (AE34, `notes/field-check` K4) finds notes by it | recorded | `fields_check` (U27) |
| `field-sequence` | S2, S4 (F3), S5: a chain of single-edit saves (a rename, an added field, a rename of the added field, removal of the second field, five moves); each note's values follow their field and the sort field is followed by identity; a rename is not a schema change and an add is | recorded | |
| `field-ord-changes` | S1.1, S2, S5: a save that keeps every position is not a schema change and rewrites no note; removing the first field and adding a new one at that position keeps the count but changes the fields (`scm` set, every note rewritten with an empty first value and `csum` from it); removing the last field shortens every note | recorded | |
| `template-swap-replace` | S2, S6, S7: a template moved down swaps the card ordinals; a removal renumbers the kept cards, which still render from their template; replacing the only template in one save removes every card with a grave and generates none, since notes left with no card are not visited | recorded | |
| `template-draft-mix` | S1.1, S2, S6, S7: one draft that removes, moves and adds templates together (origins none, 2, none, 4, then 2, none, 0): removals with graves, renumbering with stamps, new cards only where the fields allow | recorded | |
| `remove-crucial-fields` | S4.2, S4.3: two fields removed in one save, with the first remaining field appended to a standard front and nothing to a back; on a cloze note type, `{{cloze:F}}` (F the first field) appended only to the texts left without a cloze replacement | recorded | |
| `rename-then-remove` | S4.1, S4.2, S4.3: a rename, then removal of the renamed field: plain and filtered replacements deleted, section tags dropped, their contents kept, nothing appended while a replacement remains | recorded | |

The scenarios above that observe template text after a rename or removal (`field-sequence`, `remove-crucial-fields`, `rename-then-remove`) give every template non-empty browser texts, so deviation `template-rewrite-keeps-text` does not show there; `field-ord-changes` leaves its browser texts empty but observes no template text. The listed scenarios, the recorded goldens and the stated deviations are in `conformance/`. The language, schemas and Meguri parser recognize the new interfaces (`specs/conformance/notetype-structure-ops.md` M5). Their adapters and U34 acceptance remain outstanding. Restore to Default's scenarios are listed in `notetypes/restore-to-default` D8. U34 also makes U29's gate scenarios (`notetypes/gate/after-sync`, `notetypes/gate/structure`) active, since they wait for its `update_notetype` edits (`notetypes/schema-changes` G8).

Meguri's own tests (`core/crates/meguri-engine/tests/notetype_structure.rs`) cover what no scenario shows: the refusals of S1.2 and S12; `Stale` against a note type changed after the draft was loaded (a local API styling edit committed in the same second as the previous note type write included), and against one changed while the safety backup runs (`Stale` from the write transaction, the backup kept, nothing written); `InvalidArgument` for repeated origins before `Stale`; a draft that removes one template and adds another at the same count (S2 templates changed, S6 removal with graves, `scm` set, a backup); the sort field reason by identity (a move that keeps the sort field reports none, the removal of the sort field reports it); a local API template update naming an unknown field refused by V3; that no op takes a confirmation input; the safety backup: one before the first write of a schema-classified save, a removal and a structural restore; none for a second schema-classified save or structural restore submitted less than 10 minutes after a safety backup when every commit since it can still be undone (T6.1's coalescing), and one again at 10 minutes or after a commit that left the undo history; one for every removal, coalescing notwithstanding (S12); none for a rename-only, text-only or unchanged save, none for a removal of a missing note type, none on undo and redo, and `SafetyBackupFailed` with an injected disk full leaving the collection unchanged (`package/backup-triggers` T10); one transaction and one undo step per draft with an injected write-phase failure leaving everything unchanged; a rename-only draft giving the same result as the field rename op (S1); undo of a template add on 5,000 notes removing the cards without graves and a kill between plan and commit leaving neither; progress and cancel; notes added and fields edited by the local API while a 50,000-note reorder, a template removal or a note type removal is planning (S5, S6, S12 membership and remap); the AE34 duplicate check after moving a field to position 0 on 5,000 notes; and the removal preview (S11).
