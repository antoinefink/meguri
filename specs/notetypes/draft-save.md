# One draft, one Save, one sheet (round 9, 3C)

Spec: notetypes/draft-save
Requirements: R918, R922, R926, R927, R928, R932, R933, R937, R947, R948, R949, R1475, R1476, R1477; supersedes in part R922's and R932's questions, R951's "no warning" and `notetypes/schema-changes` G4 (each noted where it stands)
Anki: none (Meguri design, decided by Antoine 2026-11-20, round 9 option 3C); the save itself is `notetypes/structure-changes` S1 and keeps Anki's data effects
Scenarios: none; tests named in D10
Status: Approved 2026-11-20 (R915). Approved by Antoine in chat (round 9 option 3C, decided 2026-11-20, `docs/design/decisions.md` row `noteTypes`; it changes the approved rule of `notetypes/schema-changes` G4 and `docs/design/native-interface-contract.md` N11.10), after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`). The lines that begin "Changed 2026-11-20 (round 9 review)", or that carry that marker inline, are in force: written and decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after the second independent review and its re-review (`docs/process/review-log/browse-note-types-specs-20261120.md`). The lines that carry the marker "changed 2026-11-21" with "implementation review", in any letter case or punctuation, are in force: the readings U267 (PR #60) built, decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/notetypes-reads-spec-record-20261121.md`). Every other section is in force and unchanged.

How the Note Types section saves: one draft per note type, one Save, and one sheet shown only when content would be lost or Meguri Sync would have its collection replaced. The sheet lists the changes with real counts and states the sync consequence once. With Meguri Sync, Meguri syncs first, then backs up, saves and uploads. The core op is `notetypes/structure-changes` S1 unchanged: it still takes no confirmation input; the question is the app's, asked before the op is submitted.

## D1. The draft

- One draft per note type, holding the Fields tab and the Cards tab together (fields, sort field, field options, card types, their names, order, texts, deck overrides, styling, the note type's name when renamed from the detail). It is made from the base when the user first changes something and carries the base token (S1.1).
- It survives leaving the section, switching to another note type and switching Fields and Cards; it lives while the app runs (device memory), and the list marks its note type "Edited" (W2). Quitting asks as N11.11 says ("Discard changes?" with Keep Editing and Discard).
- Revert (toolbar) discards it after "Discard changes?".
- A change made elsewhere (sync, another window) to a note type with a draft: the detail shows a row "This note type changed on another device." with Reload (discards the draft) and Keep Editing; Save then meets `Stale` (S1.2 item 1) and offers Reload.

## D2. The save summary (core read)

`draft_summary(draft)` reads one snapshot, writes nothing, runs off the main thread (cancellable, `notetypes/structure-changes` S10's planning), and returns:

- `reasons`: the schema reasons the save would carry (`notetypes/schema-changes` G2, by G3's effect);
- `changes`, in this order: each field added (name); each field removed with `notes_with_content` (W3.1's count for its base ordinal); each field renamed (old and new name); a field order change (once); a sort field change (new name); each card type added with `cards_created` (the cards S7 would generate for it); each card type removed with `cards_removed` and `notes_without_cards` (S11 over every removed base ordinal together); each card type renamed; a card type order change (once); the note type renamed; template text and styling changed (once, without detail);
- `loses_content`: true when a removed field has `notes_with_content` > 0, or a removed card type has `cards_removed` > 0.

Changed 2026-11-21 (implementation review): `draft_summary` refuses as the save's planning does before its loss comparison: `InvalidArgument`, `Stale`, `invalid_notetype`, `ProtectedField`, `NoteFieldCountMismatch` and `Cancelled`. It runs no template checks (`notetypes/validation` V3; those are W7's live issues) and compares no confirmed loss. A rename-only draft lists its renames and no reasons.

Counts come from the same snapshot; the counts S1.3 later reports can differ when the collection changed in between, and the op's own result is what is shown after the save.

## D3. When Save asks

The Save button reads "Save…" when D3 says the sheet will show, "Save" otherwise; ⌘S either way.

| The draft holds | Service is Meguri Sync | Service is iCloud (Beta) or Off |
|---|---|---|
| no structural change (`reasons` empty): template text, styling, renames, field options | no sheet; Save saves; sync stays normal | no sheet |
| structural changes, `loses_content` false (fields or card types added or moved, a new sort field, removed fields empty everywhere) | the sheet with the changes and the sync paragraph; button Save and Upload | no sheet: iCloud carries structure changes without replacing anything (G7) |
| `loses_content` true | the sheet with the losses and the sync paragraph; button Save and Upload, destructive | the sheet with the losses only; button Save, destructive |

- This table is for note type saves from the Fields and Cards tabs only. Three actions keep their own question and are not ruled by it: **Change Note Type** always shows its sheet (N22.8), whose M6 loss preview is the loss confirmation, with no second confirmation; **Delete Note Type** always asks W12's question; **Restore to Default** always asks R914's question. With a ready Meguri Sync binding, Change Note Type and Delete Note Type gain D4's sync paragraph and their confirm buttons read Save and Upload and Delete and Upload; otherwise they are unchanged. Restore to Default keeps its own question (R914); with a ready Meguri Sync binding and a structure-changing restore (MP7 runs only for schema-classified saves; changed 2026-11-20, round 9 review), that question gains D4's sync paragraph and its button reads "Restore and Upload".
- "Meguri Sync" in this table means Meguri Sync with a ready binding (MP5). Meguri Sync chosen but not set up counts as Off here: no sync paragraph; the next setup handles the collection as MP6.3 does.
- The service and its binding are read at Save, not when the editor opened (changed 2026-11-20, round 9 review). With a ready Meguri Sync binding at Save, a schema-classified save (the only kind that shows the sheet with the sync paragraph, D3 and D4) always runs as MP7's job from step 1 after the question (D6), never as D5's local save alone; a save that is not schema-classified shows no sheet, is D5's local save, and reaches the server by a normal sync round; the sheet D3 shows at that moment is the one for Meguri Sync. When the binding is gone at Save (the service changed to Off or iCloud, or setup lost its binding), the save is D5's local save and the sheet is the one without the sync paragraph; when that changes what the sheet shows, or whether one shows, the sheet is shown again before anything is submitted.
- Whether the sheet shows never depends on `scm` or `ls`; it depends on the service and the summary (G4 as changed).

## D4. The sheet

- Title: `Save Changes to “<note type>”?`
- One row per D2 change, each with a symbol and one line, the losses first and in red:

| Change | Line | Detail line |
|---|---|---|
| card type removed, cards > 0 | Delete Card Type “<name>” | "<c> cards and their progress are deleted." and, when > 0, "<k> notes will have no cards." (Their review log stays, `notetypes/structure-changes` S6 step 1, so the design's "and their review history" is not used: it would not be true.) |
| card type removed, no cards | Delete Card Type “<name>” | "No cards." (changed 2026-11-20, round 9 review: listed so D5 step 4 can see it gain cards) |
| field removed, content in n > 0 notes | Delete Field “<name>” | "Its content in <n> notes is deleted." |
| field removed, empty everywhere | Delete Field “<name>” | "Empty in every note." |
| card type added | Add Card Type “<name>” | "<c> cards will be created." when c > 0 |
| field added | Add Field “<name>” | |
| field or card type order changed | Change the Order of Fields / Change the Order of Card Types | |
| sort field changed | Sort by “<name>” | |
| renames, text and styling | not listed | |

- With Meguri Sync, one paragraph after the rows: "**Meguri Sync will be replaced.** These changes alter the structure of your collection, so they cannot be merged. Meguri syncs this <device> first, saves, then uploads this collection in place of the one on Meguri Sync." then "Sync your other devices before you continue. Changes on them that have not synced will be lost." then, in the secondary colour, "A backup of the collection is kept on this <device>." (<device> is "Mac", "iPad" or "iPhone").
- Without Meguri Sync, only the last line.
- When Meguri Sync cannot be reached (no network, the server not answering a reachability check made when the sheet opens): the first paragraph reads "**Meguri Sync cannot be reached.** Meguri saves on this <device> now and asks you to upload this collection when the server can be reached." and the button reads Save (destructive when `loses_content`).
- Buttons: Cancel (the cancellation action) and the button D3 names; the destructive one is never the default, so Return does nothing destructive.
- Presentation: a window sheet on Mac, a form sheet on iPad and the Duo opened, a sheet with a large detent on iPhone.

## D5. Saving without Meguri Sync

1. The live checks (`notetypes/validation` V4) pass, or Save is disabled (W7).
2. The sheet, when D3 shows one, is accepted.
3. Deck creations for deck overrides (A11's order).
4. Right before submitting, `draft_summary` (D2) is read again; when a loss count grew, or a loss not confirmed appeared (D8's comparison), or the service changed, D3 is applied again and its sheet is shown again (with the new counts) before anything is submitted.
5. The S1 op, which takes the **confirmed losses** as a precondition (`notetypes/structure-changes` S1.1, changed 2026-11-20; decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day): for each removed field, the number of notes with content the user saw; for each removed card type, the number of cards the user saw (zero when no sheet was shown). The op recounts them before its safety backup and again inside its write transaction (the order of `notetypes/structure-changes` S1.2's round 9 review line); when any live count is larger, it writes nothing, records no step and returns `LossesChanged { summary }` with the new D2 summary, and the sheet is shown again with those counts. This closes the window between the read and the commit. The op takes its safety backup when schema-classified (`package/backup-triggers#T6.1`) and saves in one transaction and one undo step "Update Note Type" (S1, S13). While it runs the toolbar shows its progress ("Updating <i> of <n> notes", S10) and the note type cannot be edited; the rest of the app stays usable.
6. On success the draft is gone and the detail shows the saved note type. On a refusal the draft stays with the refusal shown (`LossesChanged`: the sheet again, step 5; `Stale`: D1's row; `SafetyBackupFailed`: an alert that nothing was changed; a V3 failure cannot occur after step 1 unless the stored note type changed, and then shows as an issue).

## D6. Saving with Meguri Sync

When the service is Meguri Sync and the summary is schema-classified, Save and Upload runs one job (`sync/meguri-provider` MP7):

1. **Sync first**: one normal round (MP5) so this device holds everything the server has. When the round brings a change to this note type, the job stops before saving and the draft meets `Stale` (D1). When the round fails or needs the user, the job stops, nothing is saved and the sync status shows why.
2. **Back up and save**: D5 steps 3 to 5 (the safety backup of `package/backup-triggers#T6.1`).
3. **Upload**: the collection replaces the server's (MP7's upload, the MP4 safety mechanics). Its progress shows in the sync status.
4. When step 3 cannot run or fails after the save committed, the save stays; the sync status shows the needs-you line "Upload this collection to finish your note type changes." with Upload… (N15.4's needs-you kind). Upload… runs MP7 step 5: admission, a read of the server's facts and the upload, with no new question (the sheet was the consent), except in MP7 step 5's two cases: when the server's facts differ from the binding's last proved baseline, or no baseline is recorded, Upload… shows MP6.3's collection choice instead.

Offline (D4's unreachable case): D5 only, then step 4's needs-you line.

A schema-classified save never runs a normal round that would discover the full-sync requirement first: the upload is the decided outcome (MP4's "never run normal sync to discover", applied after the save).

## D7. Undo

- The save is one undo step, "Update Note Type", until the upload of D6 step 3 has run: Edit › Undo restores the note type, its notes and cards and `scm` (`notetypes/schema-changes` G5).
- A completed upload clears the undo and redo history (the server now holds the saved structure; an undo would leave this device's structure behind the server's). The backup stays either way.
- Undo after an offline save, before the upload: allowed; it also withdraws step 4's needs-you line when no other structural change is pending (`scm <= ls` again, G6's exception).

## D8. Questions that move into the sheet

**Rule for every destructive note type operation** (decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day): each confirmed destructive operation carries what the user confirmed, and its op refuses inside its write transaction, writing nothing and returning `LossesChanged` with the new counts, when the live loss is larger; the question is then shown again. The four operations: the draft save's field and card type removals (`notetypes/structure-changes` S1.2), Delete Note Type (S12), Restore to Default (`notetypes/restore-to-default` D7) and Change Note Type (`notetypes/change-notetype` M8). No other note type operation deletes content.

Changed 2026-11-20 (round 9 review): Confirmed losses are counts, one per loss category of the op (per removed field, per removed template, per extra category such as M9.1/M9.2 or D4.1 cards, per notes and cards for a removal). Each live count is compared with its own confirmed count, never a total. A category that appears live but is absent from the confirmed losses has a confirmed count of 0. A count that shrinks or stays equal does not refuse, even if different notes now hold the content: the confirmation is by count, not by note. `notes_without_cards` is shown but not compared (no content is deleted). `LossesChanged` is one refusal kind whose payload is the op's own read: D2's summary for the draft save, `{ notes, cards }` for S12, the restore summary for `notetypes/restore-to-default` D7.1, and an M6 preview for M8. Callers with no question (the conformance executors, any later programmatic caller) pass the counts they read at the call itself. No local API exposes these four ops today: AnkiConnect's `modelFieldRemove`, `modelTemplateRemove`, `updateModelTemplates` and `updateNoteModel` answer `unsupported action` (`servers/ankiconnect-envelope`), and the collection MCP tools are read-only for note types; a later action that adds one passes the counts it reads at the call.

These questions are no longer asked at the moment of the edit; D4's rows replace them: Add Card Type's "This will create N cards. Proceed?" (R932, `notetypes/editor-actions` A6), Remove Card Type's question (R933, A7), the field delete question "Delete field from N notes?" (R922, A10). Restore to Default (R914) and Delete Note Type (R918, W12) keep their own questions; Change Note Type keeps its sheet (N22.8) with D4's sync paragraph.

## D9. Strings

`Save Changes to “%@”?`, Save…, Save and Upload, Delete and Upload, Delete Card Type “%@”, %lld cards and their progress are deleted., %lld notes will have no cards., Delete Field “%@”, No cards., Its content in %lld notes is deleted., Empty in every note., Add Card Type “%@”, %lld cards will be created., Add Field “%@”, Change the Order of Fields, Change the Order of Card Types, Sort by “%@”, the D4 paragraphs with %@ the device name, Meguri Sync cannot be reached. and its sentence, Upload this collection to finish your note type changes., Upload…, This note type changed on another device., Reload, Keep Editing, Discard changes?, Discard, Revert, Restore and Upload, Updating %1$lld of %2$lld notes, and the undo step names Update Note Type and Add Note Type (existing core names, shown as "Undo Update Note Type"). All are new except the step names.

## D10. Tests

- Core: `draft_summary` against a fixture (fields removed with and without content, card types removed leaving notes without cards, added card types' counts, reasons equal to the op's), and that it writes nothing; the D6 job with a fake Meguri server: the round runs first, a round bringing a change to the note type stops the job with `Stale`, the save then the upload, a failed upload keeps the save and records the needs-you state, the history cleared after the upload.
- Swift model tests: D3's table (service × summary) for the button title and the sheet; D4's rows and paragraphs; D7's Undo availability before and after the upload.
- VM UI: the Mac sheet with losses and Meguri Sync; the iPhone field delete with Save.
