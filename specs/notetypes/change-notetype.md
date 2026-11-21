# Change Note Type

Spec: notetypes/change-notetype
Requirements: R944, R945, R946, R947, R948; supports R949, R951, R1189, R1475, R1476, R2008, R2053
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/change/
Status: Approved 2026-11-20 (R915) for the lines of M8 that begin "Changed 2026-11-20 (round 9)". Approved by Antoine in chat (round 9, decided 2026-11-20), after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`). The lines that begin "Changed 2026-11-20 (round 9 review)", or that carry that marker inline, are in force: written and decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after the second independent review and its re-review (`docs/process/review-log/browse-note-types-specs-20261120.md`). The lines that carry the marker "changed 2026-11-21" with "implementation review", in any letter case or punctuation, are in force: decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/u35-spec-record-20261121.md`). Every other section is in force and unchanged.

Moving selected notes to another note type, or remapping the fields and templates of their own note type: the info query that gives the default mappings, the loss preview and the schema stamp, and the change itself. The screens are U203 (and the Browse entry, U87 and U210); the scenario ops are `specs/conformance/change-notetype-ops.md`. The sheet's pickers (target order, opening state, template choices another row uses, the collapsed loss list) are `notetypes/editor-actions` A16.

Terms. The **old note type** is the one the selected notes share; the **new note type** is the target, any note type of the collection, the old one included (R944). Fields and templates are numbered by their 0-based ordinal. "Standard" and "cloze" are the note type kinds (`notetypes/records`). Card generation terms are `notes/card-generation`'s; deck placement and new-card positions are `notes/deck-placement-and-position`'s; stamps are `storage/ids-usn-mtime`'s (`now_s`, `now_ms`, local change).

## M1. Mappings

- A **field map** has one entry per field of the new note type, in field order. An entry is the ordinal of the old field whose text the new field receives, or none ("(Nothing)": the new field starts empty). Several entries may name the same old field (R945). An old field that no entry names is **discarded**: its text is lost.
- A **template map** has one entry per template of the new note type, in template order. An entry is the ordinal of the old template whose cards become the new template's cards, or none. No old ordinal appears twice (R946). An old template that no entry names is **dropped**: its cards are deleted.
- A change has a template map exactly when both note types are standard. When either one is a cloze note type there is none: cards keep their ordinals (M9.2, M9.3), and the screen shows the note of R946 instead of the template mapping.

## M2. Default field map (R945)

1. Each new field whose name is exactly the name of an old field gets that old field. Names compare exactly, letter case included; stored names are already NFC (`notetypes/records` N4), and nothing else is folded.
2. The old fields step 1 left unused, in ascending ordinal, are given one each to the new fields still without one, in new field order, until either list runs out.
3. New fields still without one get none.

| Old fields | New fields | Default field map |
|---|---|---|
| Front, Back | Front, Back, Extra | 0, 1, none |
| Front, Back, Extra | Back, Notes, Front, More | 1, 2, 0, none |
| Front, Back, Extra | Front, Back | 0, 1 (Extra discarded) |
| Back, front, Extra | Extra, Front | 2, 0 (`front` is not `Front`; Back is the first unused old field) |
| Front, Back | Text, Back Extra | 0, 1 (no name matches) |
| Text | Front, Back, Text | none, none, 0 |

## M3. Default template map (R946)

Only when both note types are standard: the rule of M2 applied to template names.

| Old templates | New templates | Default template map |
|---|---|---|
| Card 1 | Card 1, Card 2 | 0, none |
| Card 1, Card 2 | Card 1 | 0 (Card 2 dropped) |
| Card 1, Card 2 | Recognition, Production | 0, 1 (no name matches) |
| the same note type | the same note type | 0, 1, … (identity) |

## M4. The selection

The info query and the change take a list of note ids, Browse's selection (R944, R1189).

- A note listed twice counts once, at its first position. The selection's order is the list order.

At the info query (M5), in this order:

- An empty list is an invalid argument (a caller error).
- The first listed note must exist, or the result is `not_found`.
- Every other listed note must exist and use the first note's note type, or the result is `multiple_notetypes` ("Please select notes from only one note type." is Anki's message; the wording is U87's). A missing note other than the first gives `multiple_notetypes`, as in Anki.
- The old note type is the first note's.

The change does not use these rules: M8 checks 1 and 4 check its selection, and there a missing note is `not_found` wherever it is listed (decided 2026-10-27: one answer per input).

## M5. The info query

Input: a selection (M4) and the new note type. It reads one snapshot and writes nothing. Refusals: M4's, then `not_found` when the new note type id names no note type (as in Anki, whose info query fails the same way for a missing old or new note type).

Result:

| Member | Content |
|---|---|
| old and new note type | id, stored name, kind |
| field names | the old and the new note type's, in ordinal order |
| template names | the old and the new note type's, in ordinal order (a cloze note type has its one template) |
| default field map | M2 |
| default template map | M3, or none when either note type is cloze (M1) |
| preview | M6 for the two default maps |
| field content | for each old field ordinal, the number of selected notes whose stored text for that field is not the empty string |
| card counts | for each ordinal that at least one card of the selected notes has, ascending, the number of those cards; ordinals without a template included |
| schema stamp | `col.scm` as read (`notetypes/schema-changes` G1), the staleness token of M8 check 3 (G4) |
| layout token | changed 2026-11-21 (implementation review): a digest of the two note types' ids, kinds, sort fields, and fields and templates by position (each part's id, or its name when it has none): the token of M8 check 1 |

- Field content counts any stored text, markup and white space included (`<br>`, a single space): the change discards a field's text without reading it. This is not the emptiness of card generation (`text/field-emptiness` E1), so a field holding `<br>` counts here although it requires no card. A note with fewer stored fields than its note type has (a damaged note) counts the missing ones as empty.
- The counts do not depend on any mapping, so the screen computes M6 for every mapping the user edits from this one result (U203, R947).

## M6. Loss preview (R947)

For a field map and a template map (or none), the selected notes would lose:

- **Discarded fields**: the old fields no field-map entry names, in ascending ordinal. Anki lists them under "Will discard content on the following fields:"; with the field content counts the screen can also say how many notes hold text in each.
- **Dropped templates**: with a template map, the old templates no entry names, in ascending ordinal (Anki: "Will remove the following cards:"); none without a template map.
- **Removed cards**: how many cards of the selected notes M9 deletes, from the card counts:
  - with a template map: the cards whose ordinal is a dropped template's ordinal, and the cards without a template (ordinal not below the old template count) whose ordinal is the position of an entry that names an old template (M9.1); other cards without a template are kept;
  - cloze to standard: the cards whose ordinal is not below the new note type's template count;
  - to cloze: none.

A note can lose every card and still keep one: M9 step 3 then creates a new card (`notes/card-generation` G6). The preview counts deletions only.

This preview is the change's destructive-mapping preview (`notetypes/schema-changes` G4): the user accepts it by saving, and the change asks nothing more.

## M7. Requests that change nothing

A change whose new note type is the old one, whose field map is the identity (entry i names old field i, for every i) and whose template map is the identity or absent is refused with `no_changes` and writes nothing. Meguri checks in the op so that every caller gets the same answer.

Anki's screen refuses such a save ("No changes to save") before its backend sees it only when the change has a template map (two standard note types). For a cloze note type to itself there is none, its test fails, and the screen sends the save: Anki's backend then sets `col.scm`, stamps every selected note and removes control characters from its fields (M9 step 3). Decided 2026-10-27 (data safety: a schema change that changes nothing would make the next manual Anki transfer one-way for no reason): Meguri refuses that case with `no_changes` too. The oracle driver does not yet reproduce the screen's test for this case: it also refuses a missing template map with `no_changes`, so a golden for it would record Meguri's answer, not Anki's. No scenario sends the case, and `conformance/deviations.yaml` has no entry for it. Before a scenario sends it, the driver must run Anki's change when the template map is absent, as `change-notetype-ops` H3 requires ("as Anki's screen decides it" is the screen's test above); that scenario then adds the deviation entry `change-notetype-cloze-unchanged` with its golden. Until then U35's tests cover it (M12).

## M8. The change: input, checks and safety backup

Changed 2026-11-20 (round 9; decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day): the input also carries the **confirmed losses**, what M6's preview showed and the user accepted: for each discarded old field, M5's field content count (the selected notes whose stored text for that field is not the empty string; M5's count, not `text/field-emptiness` E1, so `<br>` counts); for each dropped template, its card count; and, as one more count, the cards M9.1 deletes without a template plus the cards M9.2 deletes (M6's Removed cards less the dropped templates' cards). Each is recounted and compared on its own (`notetypes/draft-save` D8). Inside the write transaction, after checks 3 and 4 on live rows, the change counts them again; when any live count is larger it writes nothing, records no step and returns `LossesChanged { preview }` with a new M6 preview, and the sheet shows again. Callers pass what their M6 preview showed; a caller with no sheet passes the counts it reads at the call itself. This is a precondition like the schema stamp, not a confirmation input.

Changed 2026-11-20 (round 9 review): **Order and outcome** (the same for the four destructive note type ops, `notetypes/draft-save` D8): the confirmed losses are counted again twice, on the planning snapshot before the safety backup (so an out-of-date confirmation costs no backup) and again inside the write transaction on the live rows, right after the op's staleness checks and before its first write; a larger count at the first count refuses before the backup; passing it does not skip the second count, which alone guarantees the commit. When a refusal of the op's staleness checks (any of them: `Stale`, M8 checks 3 and 4) and `LossesChanged` both apply, the staleness refusal wins. A refusal writes nothing, records no undo step and leaves the undo and redo history as it was; a safety backup already taken stays (`package/backup-triggers#T6.2`).

Changed 2026-11-20 (round 9): with Meguri Sync as the service and a ready binding, Change Note Type's sheet carries `notetypes/draft-save` D4's sync paragraph and its button reads Save and Upload, and the save runs as `sync/meguri-provider` MP7's job; this replaces "asks nothing" beyond M6's preview for that case (`notetypes/schema-changes` G4 as changed). The op itself still takes no confirmation input.

Input: the selection (M4), the old note type the info query reported, the new note type, a field map, a template map exactly when M1 gives one, and the schema stamp the info query returned (plus the confirmed losses, round 9 line above; Changed 2026-11-20 (round 9 review)). There is no confirmation input (`notetypes/schema-changes#G4`). Changed 2026-11-21 (implementation review): the input also carries the info query's layout token (M5). `col.scm` is the clock's millisecond, so a second schema change in the same millisecond leaves the stamp equal; the token is compared as well.

Changed 2026-10-27: the input had a schema confirmation flag and the checks ended with a confirmation gate (`NeedsSchemaConfirmation`); both are gone (R948, R951, R1476). Saving asks nothing beyond the screen's own preview (M6), and the safety backup below protects the change.

The checks and the backup run in this order. A failed check writes nothing, takes no undo step and publishes nothing.

1. **Shape.** The list of note ids is not empty (otherwise an invalid argument). The old and the new note type ids name note types of the live collection, or the result is `not_found` (a note type deleted since the info query; Anki's change fails the same way on a missing new note type). Changed 2026-11-21 (implementation review): then the layout token is compared with the two live note types. When it differs, the result is `schema_changed`, not an invalid argument: what differs because the live note types changed is staleness, as in `notetypes/structure-changes` S1.2 (its whole order, step 3). The maps are then checked against that layout, the one the user saw; an entry it does not allow is an invalid argument. Every map check, a template ordinal named twice included, comes after the token, so a request that is both stale and malformed gets `schema_changed`, and a fresh info query then shows the error (as S1.2 step 3 does for a stale draft). Then the field map has one entry per new field and each entry is an old field ordinal or none; a template map is present exactly when both note types are standard, has one entry per new template, each entry an old template ordinal or none, no ordinal twice. Otherwise: an invalid argument (a caller error, not a refusal the user sees). Changed 2026-11-21 (implementation review): a field map may name one old field more than once to any note type (R945, M1), and the structure report carries it (M10, `sync/cloudkit-records#K19.2` "Repeated field origins"). U35 as merged (PR #46) builds this only to the same note type: between two note types it refuses such a map with `Unsupported` (`change_notetype.repeated_field_between_note_types`), right after the map checks and before check 2, before any backup, writing nothing. U35a removes that refusal (`docs/plans/2026-11-20-1700-feat-browse-and-note-types-implementation-plan.md` section 9); from then on the sentence before is the whole rule.
2. **Nothing to change**: M7, `no_changes`.
3. **Schema stamp**: when `col.scm` differs from the stamp, `schema_changed` (R948). The screen explains it and stays open (U203). Anki refuses with an invalid-input error here. Changed 2026-11-21 (implementation review): inside the write transaction, the layout token is compared again together with the stamp. A note type that planning found and that is gone there also gives `schema_changed`.
4. **Selection**: every listed note, the first included, still exists, or `not_found` (a note deleted since the info query; Anki fails the same way and keeps nothing); then every one still uses the old note type, or `multiple_notetypes`. M4's info-query rules do not apply: a missing note other than the first is `not_found` here, not `multiple_notetypes`. Anki does not check the note type here; without a schema change, which check 3 refuses, no note can have left the old note type. Changed 2026-11-21 (implementation review): then no listed note may store more field values than the old note type has fields; otherwise the result is `NoteFieldCountMismatch { note }`, naming the first such note in selection order. The change would drop the extra values without counting them, since M5 counts only the old fields. Only a damaged collection holds such a note, and Check Database repairs it (`notes/note-records` F1). Both runs of check 4 apply this, as the draft save's S1.2 item 4 does.
5. **Safety backup.** A change that passes checks 1 and 2 is a structural edit (`notetypes/schema-changes` G2, reason `notes_notetype_changed`; every such change rewrites notes or cards, so G3 always classifies it). It takes the safety backup of `package/backup-triggers#T6.1`'s structural-edit row, with that row's coalescing, after checks 1 to 4 and before its write transaction (T6.2). A failed backup refuses the change with `SafetyBackupFailed { cause }` and writes nothing.

Decided 2026-10-27: checks 3 and 4 run twice. Before the backup the change runs them on a read of the live collection, so a screen opened on a stale stamp or a deleted note gets its refusal without a backup being taken. Inside the write transaction it runs them again on live rows, and only that run decides: a schema change or deletion committed between the backup and the write transaction refuses the change there, and the backup already published stays (T6.2). Checks 1 and 2 need only the two note type records (their kinds and their field and template counts), read from the live collection, and run once, first. Inside the write transaction, check 3's stamp comparison guarantees that those records did not change since: an edit of a note type's fields or templates and a note type deletion set `col.scm` (`notetypes/schema-changes` G2). Changed 2026-11-21 (implementation review): the stamp alone does not guarantee it when two schema changes fall in one millisecond, so check 3 compares the layout token beside it, and check 1's maps are checked against the layout that token names.

Changed 2026-11-21 (implementation review): **The whole order.** The change checks its refusals in this order, and the first that applies is its result:

1. An invalid argument for an empty list.
2. `not_found` for a missing old or new note type.
3. `schema_changed` for a layout token that differs.
4. An invalid argument for maps that do not fit that layout.
5. `Unsupported` for a repeated field origin between two note types, until U35a (check 1).
6. `no_changes`.
7. `schema_changed` for the stamp.
8. `not_found` for a missing listed note, then `multiple_notetypes`, then `NoteFieldCountMismatch`.
9. `LossesChanged` from the first count.
10. The safety backup (check 5).
11. Inside the write transaction: the stamp, the layout token and both note types (`schema_changed`), then check 4 in the same order, then `LossesChanged` from the second count. Then the first write.

`Cancelled` can end the change before step 11 (M10). Steps 1 to 9 run on the planning snapshot and take no backup.

## M9. The change: effects

In one write transaction, after checks 3 and 4 passed on live rows, every value derived from the live rows (`engine/op-model` O7, re-derived), in this order:

1. `col.scm` is set to `now_ms` (`notetypes/schema-changes` G5).
2. The cards of the selected notes:
   - **M9.1 With a template map** (standard to standard): every card whose ordinal is a dropped template's ordinal is deleted with its card grave only (`notes/note-records` D2's plain removal, not its variant; `storage/graves` G2): its note stays, also when it is left with no card, and step 3 gives it one. A card whose ordinal is not below the old template count (a card without a template, `notes/empty-cards` E1) is deleted the same way when the entry at the position equal to its ordinal names an old template, because a renumbered card may take that ordinal. Then every card whose ordinal o (its ordinal before the change) is named by the entry at position n, with n ≠ o, gets ordinal n and is stamped as a local change (`mod` `now_s`, `usn` -1); its other columns stay, deck and scheduling included. A card whose ordinal is named by the entry at its own position, and any other card without a template, is not touched.

     Decided 2026-10-27 (data safety): Anki 26.09.3 keeps a card without a template at a renumbering's target, so a note with cards of ordinals 0, 1 and 2 on two templates, moved with the map none, 1, 0, ends with two cards of ordinal 2. Nothing refuses that (the `cards` table has no unique index on `nid` and `ord`, `storage/schema-18`), and the next Check Database deletes the one with the higher id, without a grave (`maintenance/check-database-notes` N6.2), which may be the user's renumbered card. Meguri deletes the card that has no template instead, with its grave, and the preview counts it (M6). No scenario has such a card, so no deviation entry is needed; U35's tests cover it (M12).
   - **M9.2 Cloze to standard**: every card whose ordinal is not below the new note type's template count is deleted with its card grave only, as in M9.1. The other cards keep their ordinals and are not touched, also when the new template at their ordinal requires no card for the note (Empty Cards finds them, `notes/empty-cards`).
   - **M9.3 To cloze** (from either kind): no card is deleted or changed. Each card's ordinal is now its cloze number minus one.
3. Each selected note, in selection order (M4):
   - **Fields.** New field i gets the stored text of the old field its field-map entry names (an ordinal beyond the note's stored fields reads as empty), or the empty string for none. The same text may go to several fields.
   - **Normalization.** Control characters other than TAB and LF are removed from every field (`text/checksum-and-sort-field` C1, first step). NFC is not applied, whatever `normalize_note_text` holds: text stays decomposed if it was (unlike an edit's save, `notes/note-records` N2).
   - **Row.** `mid` becomes the new note type's id; `sfld` and `csum` are computed from the new fields with the new note type's sort field (`text/checksum-and-sort-field` C2 to C4); `mod` `now_s` and `usn` -1. `guid`, `tags`, `flags` and `data` stay; tags are not registered again.
   - Every selected note is stamped and written, also one whose fields, `mid`, `sfld` and `csum` stay as they were (the new note type is the old one and only the template map changes), as Anki does. Changed 2026-10-27: such a note was left unwritten, which `notetypes/change/template-map` would show against Anki's stamped row. The row handed to `storage/ids-usn-mtime` S4 is already stamped, so S4 skips it only when the stored row already has that `mod` and `usn` -1.
   - **Cards.** The note's cards are then generated as for an updated note: `notes/card-generation` G7 over the note's live cards, and G6 when the note now has no card and requires none (the card of ordinal 0). Each new card is placed by `notes/deck-placement-and-position` D3, whose item 3 is the **new** note type's last deck. For a note that steps 2 left with no card, D3 has no item 2: a card its template generates may take item 1, and G6's card of ordinal 0 never does (G6), so that card goes to the new note type's last deck, else Default, as `maintenance/check-database-notes` N6.6 places such a card (Anki 26.09.3 does the same). New cards are positioned by P2 (one counter value per note that needs one). Every selected note is visited, also one with no card, unlike note type regeneration (G9, D4). New cards are created in note order, then ascending ordinal (G8).
4. `col.mod` becomes `now_ms` (`storage/ids-usn-mtime` S5).

The change writes notes, cards, graves, the `nextPos` config key when P2 draws a position (`notes/deck-placement-and-position` P1) and the `col` row; it never writes a note type row (`notetypes/records` N10).

Result: the number of notes in the selection, duplicates counted once. The screen says "N notes updated." (R948; Anki counts the notes it was given).

The change is one undo step, "Change Note Type" (R948): undo restores the cards, notes, graves and `col.scm` (G5) and redo applies them again (`engine/undo`). Its change set (`engine/change-stream#S1`) lists the notes and the cards it changed, `nextPos` under `config_keys` when step 3 wrote it, and `collection_row` true (`col.scm` changed). It lists no note type, and graves are not listed (S1).

| Situation (scenario) | Effect |
|---|---|
| Pair (Front, Back; Card 1) to Pair Reverse (Front, Back, Extra; Card 1 `{{Front}}`, Card 2 `{{Back}}`), default maps (`notetypes/change/to-reversed`) | fields 0, 1, none; card 1 untouched; card 2 created in card 1's deck at card 1's position; a note with an empty Back gets no card 2 |
| Pair Reverse to itself, field map 1, 0, 2 (`notetypes/change/swap-fields`) | `flds`, `sfld` and `csum` swapped, note stamped; no card touched |
| two fields to three, field map 0, 0, 1 (`notetypes/change/repeat-field`) | the old first field's text in both new fields 0 and 1; the second template, which asks for new field 1, gets a card for every note, the one with an empty old second field included |
| a raw first field holding a decomposed `e` + U+0301, a second holding U+0001, U+0007, CR, U+007F, TAB and LF, `normalize_note_text` absent (`notetypes/change/normalization`) | the four control characters removed, TAB and LF kept, the decomposed sequence kept (no NFC); `sfld` and `csum` from that text |
| Cloze with c1, c2, c3 to a standard type with two templates (`notetypes/change/cloze`) | ordinals 0 and 1 untouched, ordinal 2 deleted with a grave |
| Standard with two templates, fields holding c1 and c3, to cloze (`notetypes/change/cloze`) | ordinals 0 and 1 untouched, ordinal 2 created for c3 |
| Pair to Pair Reverse with template map none, 0 (`notetypes/change/template-map`) | the old card becomes ordinal 1 (same id, stamped), a new ordinal 0 card at its position |
| Pair Reverse to itself with template map 0, none (`notetypes/change/template-map`) | card 2 deleted with a grave, then created again as a new card: its scheduling is lost (its review-log rows stay); the note stamped although its fields stay |
| Duo (two templates) to itself with template map none, 0 (`notetypes/change/same-type-remap`) | old card 2 deleted with a grave, old card 1 becomes ordinal 1 with its scheduling, a new ordinal 0 card created |
| every template dropped, nothing required (`notetypes/change/placement`) | all cards deleted, then a new card of ordinal 0 (G6) in the new note type's last deck |
| cards split over two decks, a third template now required (`notetypes/change/placement`) | the new card goes to the new note type's last deck |

## M10. Running it

- The info query reads one snapshot on a reader, off the writer: one pass over the selected notes and their cards.
- The change selects and reads everything inside its write transaction, so edits, reviews and deletions made after the info query are seen and kept (the field remap is re-derived from the live note, `engine/op-model` O7). The only values carried from the info query are the selection, the old note type, the user's mappings, the schema stamp and the layout token, which checks 1 and 3 compare (changed 2026-11-21, implementation review), and the confirmed losses (M8; Changed 2026-11-20 (round 9 review)).
- Changed 2026-11-21 (implementation review): it reports progress during planning, as notes done out of notes selected. The write phase reports none: a report can reach the host's listener, which the writer never calls inside a transaction. On a large selection its write phase can pass ~200 ms: it declares a long write (`engine/op-model` O8) through an op catalog entry for Change Note Type, added with the implementation. Once the write starts it is not cancellable (O5). Cancelling while the safety backup runs cancels the backup and ends the change `Cancelled`, with nothing written (`package/backup-triggers` T6.2).
- It is schema-classified (`notetypes/schema-changes` G2, `notes_notetype_changed`): it sets `col.scm` (G5) and takes its safety backup (M8 check 5, R2008). It asks no confirmation (G4).
- Transfers. Setting `scm` makes the schema modified since the last Anki sync (G1), so the next manual Anki transfer is one-way and its preview says so before anything is sent or replaced (R1476, KTD26; `notetypes/schema-changes` G7). Routine CloudKit sync never becomes one-way: the change reaches the other devices as one coherent generation (R1189, R2053; `sync/cloudkit-records#K19`), built from the structure report it fills before it returns (`engine/change-stream#S2.5`):
  - `reasons`: 6 `notes_notetype_changed`;
  - `type_changes`: one entry with the old note type id, the new note type id, the field map of M1 (-1 for none; changed 2026-11-21, implementation review: one old field may appear at several entries, `sync/cloudkit-records#K19.2` "Repeated field origins", from U35a) and a template map: M1's (-1 for none) when both note types are standard; otherwise K19.2's prefix map ("No template map"), empty when the old note type is a cloze note type and the identity list of the old note type's template count (0, 1, …, n-1) when it is standard;
  - no `layouts` and no `removed`: the change writes no note type row.

  S2.5's guard rolls back a commit whose notes changed `mid` without that entry (`StructureNotReported`, nothing written), so every build needs the report, whether or not iCloud sync is on.

  Decided 2026-10-27 (data safety): a change whose new note type is the old one reports the same entry, with old id = new id and both maps, although no note's `mid` changes and the guard does not ask for it. Its maps move field contents (a swap of two fields) or cards (a renumbering) within the note type, so its notes move to a new layout as much as moved notes do: a receiver maps a pending local version of such a note through the entry as for any type change (K15.1, K19.5). Without it, a receiver's pending edit of field 0 would merge into the field that now holds what was field 1.

  Open (2026-11-21; changed 2026-11-21, implementation review): a change to the same note type reports reason 6 alone. The storage structure report holds `type_changes` entries only for pairs of different note types: it refuses old id = new id, and it keeps only pairs whose notes changed `mid`. So the entry decided above is not reported. iCloud Sync is paused and no receiver reads it (`docs/process/verification-status.md`, the paused-iCloud row). The point is decided when iCloud Sync work resumes; until then U35's test `a_change_to_the_same_note_type_reports_reason_six` pins the built behaviour.

## M11. Anki inputs Meguri refuses

Anki 26.09.3's backend takes any mapping its screen sends. Given other inputs (no screen sends them), it applies a template map that names an old template twice or one that does not exist as given, fails late on a field map of the wrong length, and changes a note listed twice twice (remapping its fields again); its selection check refuses a list that names a note twice (`multiple_notetypes`). Meguri refuses a wrong mapping (M8 check 1) and counts a note listed twice once (M4). No scenario sends such inputs.

## M12. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `notetypes/change/default-maps` | M2, M3, M4 refusals, M5, M6 for the default maps, M7 | recorded |
| `notetypes/change/info-counts` | M5 field content and card counts (10 notes, 7 with text in Back; `<br>` and a space count) | recorded |
| `notetypes/change/to-reversed` | M9 with default maps, D3 shared deck, P2 shared position, stamps, `col.scm`, result | recorded |
| `notetypes/change/swap-fields` | M9 note rewrite on the same note type, cards untouched | recorded |
| `notetypes/change/repeat-field` | M1 and M9 step 3: a field map naming one old field twice copies its text into both new fields; cards generated from the copied text; `flds`, `sfld` and `csum` | recorded |
| `notetypes/change/normalization` | M9 step 3 normalization: control characters other than TAB and LF removed, no NFC with `normalize_note_text` absent, `sfld` and `csum` from the normalized text; an unselected note keeps its raw value | recorded |
| `notetypes/change/cloze` | M9.2, M9.3, cloze generation after the change | recorded |
| `notetypes/change/template-map` | M9.1 renumbering, dropping, regeneration of a dropped template | recorded |
| `notetypes/change/same-type-remap` | M9.1 on the same note type: a template swap (1, 0) keeps both cards and their scheduling and exchanges their ordinals; with (none, 0) the dropped ordinal 1 card is deleted before the ordinal 0 card takes ordinal 1, and ordinal 0 is generated again; (none, 1) drops and regenerates ordinal 0; a field map entry of none empties that field (`sfld` and `csum` follow) without touching cards | recorded |
| `notetypes/change/cloze-round-trip` | M9.2, M9.3: a two-card standard note with no cloze markup keeps both cards untouched going to cloze and back to two templates; from cloze to a one-template note type its ordinal 1 card is deleted with a grave | recorded |
| `notetypes/change/placement` | M9 step 3 placement: the new note type's last deck, G6 after every card was dropped, P2 counter for a reviewed note | recorded |
| `notetypes/change/refusals` | M8 checks 3 and 4: `schema_changed` and `not_found` write nothing; a fresh info query then succeeds | recorded |
| `notetypes/change/undo` | M9's undo step: undo of a change that created a card (deleted without a grave, notes back with their note type, fields and stamps, `col.scm` back), redo, and undo of a change that dropped a card (the card back with its scheduling, the grave gone) | recorded (Meguri also needs `undo`, `redo` and `undo_status`) |

Every listed scenario and its golden are in `conformance/` (`change-notetype-ops` H4). All stay inactive until U35's adapters pass their required checks. Changed 2026-11-21 (implementation review): U35 activated every listed scenario but `notetypes/change/repeat-field`, which U35a activates once it passes T0 (`change-notetype-ops` H4).

U35's Rust tests (`core/crates/meguri-engine/tests/change_notetype.rs`) cover, against expectations built from this spec: undo and redo of the change, `col.scm` included; that the change takes no confirmation input and commits at once while `scm <= ls`; the safety backup (taken after the checks and before the write transaction, none for a refused `no_changes` or a stale stamp found by the first run of checks 3 and 4, T6.1's coalescing, `SafetyBackupFailed` writing nothing); the shape check, with an empty list and a missing old or new note type id (also at the info query, M5); a missing listed note other than the first (`multiple_notetypes` at the info query, `not_found` at the change); `multiple_notetypes` at the change; duplicates in the selection; a cloze note type to itself with the identity field map refused with `no_changes` (M7); a card without a template at a renumbering's target deleted with its grave and counted by M6, and one at another ordinal kept (M9.1); a note whose every card is dropped kept, with its G6 card in the new note type's last deck, and `nextPos` in the change set (M9 step 3); every selected note stamped when only the template map changes (M9 step 3); progress; and the structure report (M10; its `op_catalog.rs` test, `engine/change-stream#S2.5`): reason 6 and the `type_changes` entry between two standard note types, from a cloze note type (an empty template map), from a standard note type to a cloze one (the identity list of the old template count), and to the same note type (old id = new id; changed 2026-11-21, implementation review: reason 6 alone, M10's open point). Changed 2026-11-21 (implementation review): also the layout token (a stale layout with a now out-of-range ordinal is `schema_changed`; a fresh bad ordinal is an invalid argument); a note storing more values than fields, refused at planning and in the transaction; a repeated field map between two note types, refused before the backup (until U35a); and a note type deleted during the backup, which is `schema_changed`.

Changed 2026-11-21 (implementation review): U35a's tests replace the refusal test of a repeated field map between two note types. In `change_notetype.rs`: such a map (Duo with Word, Gloss to Twin with Word, Hint, Gloss, field map 0, 0, 1, as `notetypes/change/repeat-field`) commits on notes whose fields hold text, an empty string, `<br>`, decomposed text and a control character, and on a note storing fewer values than fields; the copied text is in both new fields, and cards are generated from it. Undo then gives back every selected note's row exactly as before the change (`mid`, `flds` byte for byte, `sfld`, `csum`, `mod`, `usn`, `tags`), and every card, grave and `col.scm`; redo gives the committed rows exactly again, and a second undo and redo cycle does the same. The commit's and the redo's structure report hold the entry with the repeated map, and the undo's holds the reversed entry with K19.2's inverse ([0, 2] for 0, 0, 1). Also a repeated map from a standard note type to a cloze one and from a cloze note type to a standard one, each with undo and redo. In the storage crate's tests: a `type_changes` field map with a repeated origin is admitted, authored and retained; a `layouts` field map or a template map with one is still refused; the inverse rule of K19.2 on [0, 0, 1], [1, 1, -1] and [-1, 0, 0]; and a joined report that composes a layout of the old note type, the repeated type change and a layout of the new note type, with its inverse.
