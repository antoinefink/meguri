# Empty Cards (generation side)

Spec: notes/empty-cards
Requirements: R952, R1653; supports R1015, R1473, R1652, R1654, R2008
Anki: 26.09.3
Scenarios: conformance/scenarios/cardgen/empty/; U131's maintenance/empty-cards/ (E7)

Finding the cards whose template or cloze number no longer generates a card, and deleting them as one undoable step. The maintenance flow around it (the report screen data, paging, the safety backup before the delete) is U131's; the screen is U163's; the scenario ops are `specs/conformance/empty-cards-ops.md`. Which ordinals a note requires is `notes/card-generation` G2 to G4 (U27's evaluator).

## E1. Empty cards

A card is **empty** when its ordinal is not among the ordinals its note requires right now:

- standard note type: the ordinals of the templates whose front requires a card for the note (`notes/card-generation` G2, G3);
- cloze note type: min(n − 1, 499) for each cloze number n of the note's fields (G4).

The note's existing cards play no part, and G6 (the card of ordinal 0 a note gets when it requires none) does not apply: a note that requires nothing has all its cards empty. Consequences:

- A card whose template's front does not parse, or renders nothing for the note (a front of `{{^Deck}}{{Back}}{{/Deck}}`: `Deck` is never empty), is empty.
- A card whose ordinal has no template (a standard note type whose template count is not above the ordinal, left by another program or an older client) is empty.
- An image-occlusion note follows the cloze rule: the card of a mask that was removed is empty (R1015), unless another field of the note still holds a cloze with that number (an audio cloze `{{c6::[sound:…]}}` in an `A06` field): that card stays required (`editor/mask-audio` A5; scenario `editor/image-occlusion/audio-fields`).
- Cards of ordinal 499 are not empty while the note has a cloze number of 500 or more, whatever their count.

## E2. The scan

The scan looks at every note type and, for each, every note of that type that has at least one card. It writes nothing.

Result: per note type that has at least one note with an empty card, the list of those notes; note types without one are left out. For each note:

- the note id;
- its first field (index 0) as the scan's snapshot holds it, stripped by `text/strip-html` H4, the empty string for a note with no stored field: the text that identifies the note in U131's report and on U163's screen (R1652). Anki's report shows the note id instead, as a link to Browse. Like the labels below, it is display text;
- its empty cards, as (ordinal, card id), sorted by ordinal, then card id;
- its number of cards (all of them, empty or not);
- **all empty**: true when every card of the note is empty, so that deleting them would remove the note (R952 "marks notes that would lose all their cards").

Order: note types by name in the note type list's order (the `unicase` collation of `storage/schema-18`: `alpha Cloze` before `Beta Optional` before `Deckless`), notes by ascending id. An empty result means there are no empty cards (Anki's "No empty cards.").

Each empty card also carries what the report line shows for it (the wording and layout of the line are U131's and U163's):

- standard note type: its template's name as stored, or, when the note type has no template at that ordinal, the card number (ordinal + 1), which Anki shows as "Card <number>";
- cloze note type: the cloze number, ordinal + 1 (Anki: "Cloze 1, 3").

A note's label text is its labels joined with ", " in E2's card order (for a cloze note type, the word Cloze, a space, then the numbers so joined), then stripped by `text/strip-html` H4 as one string, as Anki does. So markup that spans two template names goes with the text between them: names `a<b` and `c>d` give `ad`. The note type's name in the report is stripped by H4 too. The stripped note type name and, for a standard note type whose empty ordinals all have templates, the stripped label text are user text that a scenario can observe (`empty-cards-ops` Y5, planned); the rest of the line is display wording. Anki's report line is "<empty count> of <card count> cards empty (<labels joined with ", ">)." under "Empty cards for <note type>:", with notes that are all empty shown in red.

## E3. Running the scan

- It runs as a job (`engine/jobs`) on a reader snapshot, off the writer, so reviews and edits go on meanwhile (U35's verification runs it on a 150,000-card collection).
- It reports progress as notes examined out of the notes with cards of the snapshot, throttled as `engine/jobs` J5 says.
- It can be cancelled at any time; a cancelled scan has no result.
- Each note type's templates are parsed once per scan; nothing is rendered to HTML (G3 decides from the template structure alone).

The scan's result is a plan for the delete (E4), which never trusts it blindly.

## E4. The delete

Input: the card ids the scan listed (all of them: the screen offers no per-card choice) and the "Keep notes with no valid cards" option (R952).

The option starts on every time the report opens and is never stored: Anki 26.9.3's report opens with it on and does not remember a change, and Meguri does the same (R1653). Its label is `maintenance/maintenance-strings#S4`.

Inside one write transaction, from the live rows (`engine/op-model` O7, re-derived):

1. **Gone.** A listed card that no longer exists is skipped. A card listed twice counts once.
2. **Still empty?** For each remaining listed card, its live note and that note's live note type decide E1 again. A card that is no longer empty (a field was filled, Add Reverse was set, a cloze number came back, the template changed) is **kept as valid** and counted.
3. **Keep notes.** With the option on, each note whose live cards are all listed cards that step 2 found still empty keeps one of them: the one with the lowest ordinal, the lowest card id among equal ordinals. This is the card Anki keeps ("keeps each such note's first card"; the first of the scan's list, sorted as E2 says). With the option off, nothing is kept here.
4. **Delete.** Every other listed card that step 2 found still empty is deleted with its card grave; each note left with no card is then deleted with its note grave (`notes/note-records` D2, the variant that removes the notes it leaves empty; `storage/graves` G3). Nothing is appended to the deleted-notes log (`notes/deleted-notes-log` L3). Review-log rows stay.
5. `col.mod` becomes `now_ms` when anything was deleted (`storage/ids-usn-mtime` S5). A delete that deletes nothing changes nothing and takes no undo step.

Result: the number of cards deleted (the cards step 4 removed; a skipped or kept card is not counted) and the number of listed cards kept as valid (step 2). Anki's screen then says "Deleted N cards.", with N the number of card ids it sent to the delete (the listed cards less the ones keep notes leaves), so it counts a card that no longer exists; its delete itself counts only the cards it removed, as Meguri's does. U131's message and U163's screen show the op's count, and also say how many cards were kept because they became valid.

The delete is one undo step, "Empty Cards": undo restores the cards and notes and removes the graves the delete added, redo deletes them again (`engine/undo`, `storage/graves` G6).

It is not a schema change (`notetypes/schema-changes` G2): it leaves `col.scm` alone, so it never by itself makes the next manual Anki transfer one-way, and CloudKit sync is never one-way (R1654). Changed 2026-10-27: this paragraph said that the graves make the next normal sync remove the cards on the other side; in Meguri an Anki sync is a manual transfer only (KTD26). The deletions reach other devices two ways, both as incremental changes:

- **CloudKit** (routine sync between Meguri devices): the deleted cards and notes are row deletions the change set carries like any other (`engine/change-stream` S1), which CloudKit sync sends to the other devices (`sync/cloudkit-records`). Nothing here depends on the graves.
- **Manual Anki transfer** (KTD26): the card and note graves (R1473) are what the next incremental transfer sends, so the server and other Anki clients remove the cards too (`sync/normal-sync`).

With no change between the scan and the delete, the result is Anki's: the same cards are deleted, the same card kept per note, the same notes removed.

| Scan (Optional: Card 1 `{{Front}}`, Card 2 `{{#Add Reverse}}{{Back}}{{/Add Reverse}}`) | Keep notes on | Keep notes off |
|---|---|---|
| card 2 empty, card 1 valid | card 2 deleted | card 2 deleted |
| the note's only card (card 2) empty | kept | deleted, then the note |
| both cards empty | card 1 kept, card 2 deleted | both deleted, then the note |
| cloze note, `{{c1::…}}` and `{{c2::…}}` removed from its text | the ordinal 0 card kept, ordinal 1 deleted | both deleted, then the note |
| card 2 listed, then Add Reverse set again before the delete | card 2 kept as valid, counted | same |
| card 2 listed, then its note deleted before the delete | skipped | skipped |

## E5. Difference from Anki: the delete looks again

Anki 26.09.3 deletes the cards its scan listed, as listed: a card that became valid between the scan and the delete is deleted with the rest, and "keep notes" keeps a card of every note the scan marked all empty. Meguri's delete re-evaluates each card against live rows (E4 steps 2 and 3) and keeps the cards that became valid (plan U35, the write-phase rule of U24). Scenario `cardgen/empty/recheck` shows it; its golden is transformed by the deviation entries `empty-cards-recheck`, `empty-cards-recheck-cards`, `empty-cards-recheck-review-log` and `empty-cards-recheck-graves` (R952), which `conformance/deviations.yaml` holds with that golden (`empty-cards-ops` Y4).

The message differs too (E4 Result): for a listed card that vanished before the delete, Anki's "Deleted N cards." counts it and Meguri's does not. No scenario observes the message: the ops' `deleted` is the delete's own count, which skips such a card on both sides, so this needs no deviation entry.

## E6. Running the delete

- The safety backup (R2008; `package/backup-triggers#T6.1` row "Empty Cards deletion", taken when the list of cards is not empty) is the executor's, as T6.2 says: the delete op's descriptor carries that safety class with the row's condition, and the executor takes U127's backup before the op's write phase is queued. Decided 2026-10-27: U35 gives the op that class when it adds the op, so no caller can reach the delete without the backup; U131's flow, which the plan gives the delete's backup, submits the op and does not call the backup itself, so each delete takes one backup. U131's tests check it.
- On a long list the write phase can pass ~200 ms; it then declares a long write (`engine/op-model` O8) through an op catalog entry added with the implementation. Every listed card is re-evaluated in step 2. Changed 2026-10-27: this bullet allowed skipping a card unchanged since the scan's snapshot (`engine/op-model` O7 `Untouched`); the input carries no snapshot `seq` to prove it, and the re-evaluation reads only the note and note type the delete reads anyway.

## E7. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `cardgen/empty/scan` | E1 (optional reverse, both empty, a special-name front, a card without a template, cloze numbers), E2 (order, all-empty flag, card count), an empty scan | recorded |
| `cardgen/empty/keep-notes` | E4 with keep on: the card kept per note, graves, notes kept, review log kept, a scan after the delete | recorded |
| `cardgen/empty/delete-notes` | E4 with keep off: notes left without cards deleted with note graves | recorded |
| `cardgen/empty/recheck` | E4 steps 1 and 2, E5: a note whose Add Reverse is set again and one whose Back is filled keep their card 2 and its review log, a deleted note's cards are skipped; deviation `empty-cards-recheck` | recorded, with E5's four deviation entries |
| `cardgen/empty/occlusion` | E1 for a cloze note and an image-occlusion note whose third mask was removed (R1015), E4 | recorded |
| `maintenance/empty-cards/report` | U131's report cases with its own note types: a two-way note whose Back is cleared lists card 2; a cloze note made with c1 to c3 whose text loses c2 lists the ordinal 1 card (E1, E2) | recorded |
| `maintenance/empty-cards/undo-redo` | E4's undo paragraph: one "Empty Cards" step; undo restores the cards and note with their stored `usn` and removes the graves, redo deletes them again (U131) | recorded |
| `maintenance/empty-cards/incremental-sync` | E4's sync paragraphs: after the delete `scm` still equals `ls`, the next manual Anki transfer is incremental and the other client loses the cards and the note (U131, R1654; actors, `conformance/sync-ops`) | recorded |
| `cardgen/empty/undo` | E4's undo paragraph with keep on: one "Empty Cards" step removing two cards with card graves and no note grave, the review log kept; undo restores both cards with their ids, stamps and scheduling and removes the graves; redo deletes them again; a scan after a second undo lists the same cards (U35) | recorded |
| `cardgen/empty/after-template-change` | E1, E2, E4 after a note type edit: a template added (G9) gives existing notes a card; changing its front so that one note no longer requires it keeps the card (G7) and makes it empty; the scan lists that note alone, the delete removes the card with a card grave and keeps the note, and an edit that fills the field creates the card again | recorded |
| `cardgen/empty/special-front-edit` | E1 with special names on fronts edited after the notes exist: an inverted `Deck` section (never renders), a `Tags` section (renders only for a tagged note), the same inverted `Deck` section in a note type with a field named `Deck` (reads the field), a `CardFlag` section (always renders), inverted `Subdeck` and `Type` sections (never render) | recorded |
| `maintenance/empty-cards/labels` | E2's stripped note type name and labels (`empty-cards-ops` Y5): plain names unchanged, script elements and event-handler attributes removed with the text around them kept, the cross-name case `a<b`, `c>d` giving `ad`, a decoded entity, a cloze note with no observed labels (U131) | planned: needs the `empty_cards_labels` observation in the language, schemas, testkit model and driver first (`empty-cards-ops` Y4 item 4) |

Decided 2026-10-27, done since: the `cardgen/empty/` scenarios entered `conformance/` with the two ops and have goldens at the current pin (`empty-cards-ops` Y4). They stay inactive until U35's adapters pass; the `maintenance/empty-cards/` scenarios until U131's flow.

The scan's progress and cancellation, the 150,000-card scan off the writer (U35's verification), undo and redo of the delete (also `maintenance/empty-cards/undo-redo`), and a delete whose list is entirely gone are tested in U35's Rust tests (`core/crates/meguri-engine/tests/empty_cards.rs`).
