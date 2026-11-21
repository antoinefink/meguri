# Card and note bulk actions

Spec: browser/bulk-actions
Requirements: R1117, R1181, R1182, R1184, R1187, R1188, R1189, R1190, R1191, R1192, R1193, R1194, R1195, R1196, R1197, R1198, R1199, R1212, R1213, R1214, R1215, R1268, AE45; supports R645, R1183, R1185, R1831
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/bulk/, conformance/scenarios/notes/change-deck/
Status: Approved 2026-11-20 (R1232) for the lines of B3, B5, B11, B16 that begin "Changed 2026-11-20 (round 8)". Approved by Antoine in chat (round 8, decided 2026-11-20), after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`). The lines that carry the marker "changed 2026-11-21" with "implementation review", in any letter case or punctuation, are in force: what U86 (PR #63) and U87 (PR #77) built and the coordinator's rulings on them, decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/u86-spec-record-20261121.md`, `docs/process/review-log/spec-record-3-20261121.md`). Every other section is in force and unchanged.

What each of Browse's card and note actions does: which cards or notes it acts on (B1), which item decides a toggle (B2), the toggles (B3 to B6), Change Deck (B7), and how the other actions hand their targets to the operations other specs define (B8 to B14), with filtered decks (B15), the results and undo steps (B16) and the scenarios (B17). Most actions are operations of other areas, which this file names instead of restating; Change Deck's operation is specified here (B7.2). How a bulk action runs (job, phases, cancellation, progress, change set, one undo entry) is the Meguri design doc `browser/bulk-framework` (U86). Find and Replace is `browser/find-replace`; Edit Field is the Meguri design doc `browser/batch-edit` (not written yet). Changed 2026-11-21 (implementation review): the result message left the framework's scope (round 8 removed messages, B16), and `browser/batch-edit` is not written yet. The scenario op Change Deck needs is `specs/conformance/bulk-ops.md` X2.

Terms: card columns, home deck, filtered card and first card are `browser/columns` C1; `now_s`, `now_ms` the operation's instant.

## B1. Targets (R1181, R1117)

- **Card actions** (Toggle Suspend, Toggle Bury, Flag, Change Deck, Set Due Date, Reset, Grade Now) act on cards: in cards mode the selected rows' cards; in notes mode every card of every selected note, also cards the search did not match (R1117). A note without cards adds none.
- **Note actions** (Toggle Mark, Add Tags, Remove Tags, Change Note Type, Delete Notes, Export Notes, Find and Replace with Selected notes only) act on notes: in cards mode the distinct notes of the selected cards; in notes mode the selected notes. Changed 2026-11-21 (implementation review): the all-notes target (`browser/bulk-framework` K1) is Find and Replace's alone; every other note action refuses it with `InvalidArgument`.
- The ids and their order are `browser/result-sets` H12 (`resolve_selection`): cards mode lists cards in table order, notes mode lists each selected note in table order with its cards in ascending card id; notes come in the order of their first selected row. The order shows in: Reset and Grade Now, which handle cards in list order (`sched/manual-ops` M1, M6, M7); Change Note Type, whose old note type and `not_found` come from the first listed note (`notetypes/change-notetype` M4); and Delete Notes, which handles notes, and writes their graves and deleted-notes log lines, in list order (`notes/note-records` D1). The other actions do not depend on it.
- Anki passes the order in which the rows were selected (its selection model's order of selected ranges), which equals table order only when the rows were selected top to bottom; Meguri always passes table order (B18).
- **Enablement** (R1181): every action of this file except Clear Unused Tags and Add Notes needs a non-empty selection. Create Copy needs a current row (B2) and does nothing without a current card (B13). Change Note Type is enabled on any non-empty selection, also one whose notes use several note types (B8). Find and Replace opens without a selection, collection-wide (`browser/find-replace` F2).

## B2. The current card and note

A toggle (B3 to B6) decides its direction from one item, the **current** one: the focused row, which Browse keeps whether or not it is selected.

- In cards mode the current card is the focused row's card and the current note its note.
- In notes mode the current note is the focused row's note and the current card that note's card with the lowest `ord` (Anki lists a note's cards by template order here, not by id, so this can differ from C1's first card when a card was generated later; two cards of one note share an `ord` only in a damaged collection, and Meguri then takes the lower card id). A note without cards has no current card.
- With no current card, a toggle reads "not suspended", "not buried" and "unmarked"; Flag does nothing (B5). Toggle Mark too reads the current card (its note, B6), so a notes-mode note without cards reads "unmarked".
- **Where the direction is decided.** Meguri's toggle ops take the targets, the current card id (or none; Toggle Mark too takes the card, not a note) and, for Flag, N; they read the current item's state as stored inside their own transaction and then run the operation B3 to B6 name (`sched/manual-ops` M2 to M4, `tags/flags-and-marked` F2, `tags/tags` T10). So the direction follows the current item's state when the op runs, also when it changed after the menu was drawn; the checkmark below is display only. Anki decides from the checkmark the menu showed.

The menu items show the current item's state as a checkmark (R1193): Toggle Suspend when the current card's `queue` is -1, Toggle Bury when it is -2 or -3, Toggle Mark when the current card's note is marked (`tags/flags-and-marked` M1), and the flag item equal to the current card's flag.

## B3. Toggle Suspend (R1193, AE45)

Changed 2026-11-20 (round 8): Browse offers Suspend and Unsuspend as two commands and Bury and Unbury as two commands, shown by what the selection holds (`browser/workspace` V8.1); B3 and B4's toggles and B2's current-card direction are not used by Browse. M3, M4 and M2 stay the operations.

| Current card's `queue` | Operation on every target card |
|---|---|
| -1 | unsuspend and unbury (`sched/manual-ops` M4): suspended and buried targets get the queue of their type; others are untouched |
| anything else, or no current card | suspend (M3): targets already suspended are untouched |

So with three unsuspended cards selected, one Toggle Suspend suspends all three in one "Suspend" step and one undo restores them (AE45). With the current card suspended and two others not, all three end unsuspended; a buried one among them is also unburied.

## B4. Toggle Bury (R1194)

| Current card's `queue` | Operation on every target card |
|---|---|
| -2 or -3 | the same unsuspend and unbury operation (M4): buried targets come back, and so do suspended targets, since Anki restores both with one operation |
| anything else, or no current card | bury by hand (M2): queue -3; suspended targets are untouched |

## B5. Flag 1–7 and No Flag (R1195)

Changed 2026-11-20 (round 8): Browse's Flag menu sets the chosen flag on every target (No Flag clears it); it does not toggle (`browser/workspace` V8.1).

`tags/flags-and-marked` F3 with the targets of B1: Flag N sets N on every target card, or 0 when the current card's flag is N; No Flag sets 0. With no current card Flag N does nothing; No Flag still clears. Anki's Browse Flag menu has only the seven flags (clearing is choosing the current card's flag again); R1195 adds No Flag. The operation is F2 ("Set Flag").

## B6. Toggle Mark (R1192)

`tags/flags-and-marked` M2's Browse toggle on the target notes: remove the tag `marked` from all of them when the current card's note is marked, else add it to all of them. With no current card, add: also for a notes-mode note without cards, which is the current note but has no current card (B2). Anki reads the mark through the current card the same way. The operations are `tags/tags` T10 Add ("Update Tag") and Remove ("Remove Tag").

## B7. Change Deck (R1187, R1188, R1268)

### B7.1 The deck chooser

Changed 2026-11-21 (implementation review): Browse's Change Deck sheet is N20.1's deck search with its create row (`decks/decks` D18, `browser/workspace` V8.1), so U87 built no chooser list; the list, filter and opening deck below are not Browse's. The inline new deck is B7.2 step 0's typed name.

- The chooser lists every normal deck by its full name (`browser/columns` C1), in `unicase` order of the stored names. Filtered decks are never targets and are not listed, as in Anki; R1268's filtered decks shown as non-targets are those of the sidebar and the search lists (B15), not rows of this chooser. The default deck (id 1) is left out when no card has `did` 1, whatever its children and whatever cards have `odid` 1; Meguri does the same as Anki.
- It opens on the current card's deck: its `did`, or its home deck when it sits in a filtered deck (R1187). Anki opens on the `did` of the first selected card, which matches no entry when that card is in a filtered deck.
- **Filter.** The typed text is split at each U+0020; a deck stays in the list when every piece is contained in its full name, both lowercased (full Unicode lowercase mapping). Empty pieces match every name, so empty text lists every deck. When the chooser opens, the deck it opens on is selected (the first row when it is not listed). After each change of the text the first listed deck is selected, also when the opening deck is still listed. Meguri does the same as Anki.
- An inline new deck (R1187) is created as `decks/decks` D5 creates a deck.
  - Anki creates it as its own "Add Deck" step, then moves the cards in a second step.
  - Meguri creates it inside the Change Deck operation, one step "Change Deck" (U87): undo removes the deck with the move. Not observable through the oracle (B17). B7.2 gives the input and the refusals.

### B7.2 The operation

Inputs: the target cards (B1) and a target: a deck id, or a new deck name typed in the chooser (Meguri, B7.1).

0. **A new deck name.** The name is converted and looked up as `decks/decks` D5 steps 1 and 2 do. When a deck already has that name, it is the target and no deck is created (a blank name is `blank`, D1). Otherwise D3's refusals apply first (`filtered_parent`, `too_deep`, each refusing the whole operation, writing nothing), and then the deck and its missing ancestors are created as D5 step 3 does, inside this operation: they are D4's new decks with preset 1, so step 3.1 translates for preset 1 and step 4 uses its parameters and the new deck's desired retention. Undo of the step removes every deck row step 0 inserted, ancestors included, with the move.
1. **Refusals**, before anything is written, in this order: a list that is empty or names a card twice is refused with `InvalidArgument` (Meguri, as `sched/manual-ops` M9 does); step 0's refusals; no deck has that id, `NotFound`; the deck is filtered, also when a typed name resolves to an existing filtered deck, `FilteredDeck` (Anki shows "Cards can't be manually moved into a filtered deck."; Meguri's wording is U219's). A refusal leaves the undo and redo histories as they were (`engine/undo` M2), as Anki's refusals, made before its transaction, do.
2. **Cards.** Ids that name no card are skipped. A card whose `did` already is the target is skipped: not written, not counted. A card in a filtered deck whose home deck is the target is not skipped, since its `did` is the filtered deck: it goes home and counts.
3. **Each other card**, one write per card:
   1. its remaining steps are translated for the target's preset (`sched/step-remap` R4: learning and relearning cards only, the old list read from its home deck's preset, also when both presets are the same);
   2. it leaves its filtered deck when it is in one (`sched/card-data` K3, "Leaving a filtered deck": `due` from `odue` when that is not 0, the queue from its type unless negative, `odid` and `odue` 0);
   3. `s`, `d`, `dr` and `decay` are removed from its data (`sched/card-data` K5); `pos`, `lrt` and every other key stay;
   4. `did` becomes the target;
   5. `mod` = `now_s`, `usn` -1, and its `data` text is encoded again (`model/card-data-json` C4).

   Its `type`, `queue` (except as step 2 restores it), `due` (except as step 2 restores it), `ivl`, `factor`, `reps`, `lapses`, `flags` and the rest stay: moving never reschedules (R1188). A suspended or buried card stays suspended or buried.
4. **Memory states** (R1188, R645). When FSRS is on (the config key `fsrs`, read again in the commit; changed 2026-11-21, implementation review) and step 3 moved at least one card, the same operation recomputes memory states, without rescheduling, for one set of cards (`sched/memory-state` M7.2): the cards of the target deck itself, matched by `did` or by `odid`, whose stability, read as `search/card-extractors` X4 reads `s`, is NULL after step 3.
   - Cards of the target's subdecks and of every other deck are not in the set. It holds the moved cards (step 3 removed their `s`) and target cards that step 3 did not move: cards already there without `s`, and filtered cards whose home deck is the target, which stay in their filtered deck. A card that holds `d` but no `s` is in the set; one that holds `s` but no `d` is not.
   - The set goes through `sched/memory-state` M4.1 with the target preset's parameters and the target deck's desired retention. A new card, also one with review-log rows, and a card with no review-log row are not processed and keep their rows (a moved one keeps what step 3 wrote). A processed card whose review-log rows give no memory item gets `dr` and `decay` and no `s` or `d`. A processed card is written as M4.2 says (`mod` = `now_s`, `usn` -1). One whose `s`, `d`, `dr` and `decay` come out as stored (a target card step 3 did not move, with no `s` or `d`, review-log rows that give no item and the same `dr` and `decay` already) is not written in Meguri, where Anki stamps it (B18).
   - A card outside the set is not written: its `data`, `mod` and `usn` stay, also when the `dr` it holds differs from the target deck's desired retention.
   - With FSRS off, or when step 3 moved no card, nothing is computed.
5. **Count**: the number of cards step 3 moved. Cards that only step 4 wrote (target-deck cards step 3 did not move that got a memory state, or `dr` and `decay`) are not counted; they are in the change set with the moved cards, as rows the operation wrote. `col.mod` becomes `now_ms` when the count is not 0 or step 0 created a deck.
6. **Undo**: one step "Change Deck" (`specs/conformance/undo-ops.md` V4 `change_deck`); a count of 0 records no step (`engine/undo` A2) unless step 0 created a deck, which the step then holds alone.
7. **How it runs.** Change Deck is one engine op named `change_deck` (`engine/op-model` O1), source `UserUndoable`, run as a bulk op (`browser/bulk-framework`). Its compute phase, on a snapshot and cancellable, plans every row of steps 0 to 4, step 4's memory states included; its commit writes them in one transaction under the `bulk_op` catalog entry (O9) when large, and a cancel no longer interrupts it there. It does not use the `memory_recompute` entry, whose write phase checks for cancellation between batches. A run cancelled before its commit writes nothing and records no step.
   - Changed 2026-11-21 (implementation review): what the commit reads again before its first write, since answering can change it between the phases: every listed card and set card from its live row, each moved card's home deck and that deck's preset (step 3.1 reads the old list in the commit), the target's preset choice and desired retention, the deck a typed name names, the `fsrs` key, step 4's set, and each set card's review-log row count and highest id. Any difference makes the action plan again (`browser/bulk-framework` K3). Under review admission (K2) no answer comes between the phases; the re-reads guard a caller that is not admitted and every other writer. **Known limit** (accepted by the coordinator, 2026-11-21): a change that keeps a set card's row count and highest id, such as a review-log row rewritten in place, goes unseen, and that card's memory state is computed from the history the plan read (the register row U87 adds to `docs/process/verification-status.md`).

| Card before (target `Target`, preset of `Home` and `Target` the same) | After |
|---|---|
| new, `did` Home | `did` Target, stamped |
| any, `did` Target | untouched, not counted |
| review in filtered `Cram`, home Home, `odue` 6 | `did` Target, `due` 6, queue 2, `odid` and `odue` 0 |
| review in filtered `Cram`, home Target, `odue` 9 | `did` Target, `due` 9, queue 2: counted |
| suspended review in Other | `did` Target, queue -1 |
| learning, data `s`, `d`, `dr`, `decay`, `pos`, `lrt`, FSRS off | `did` Target, data `pos` and `lrt` only |

Anki refuses the move on a collection still on the v1 scheduler; Meguri never has one open (`specs/conformance/storage-ops.md` P1, `v1_scheduler`, R1482).

## B8. Change Note Type (R1189)

The target notes go to `notetypes/change-notetype`. The item is enabled on any non-empty selection, also one whose notes use several note types, as in Anki (`browser/command-map` M3); it is never disabled for a mixed selection. Before the screen opens, Browse runs M5's info query on the target notes: M4 refuses a selection whose notes use more than one note type with `multiple_notetypes` ("Please select notes from only one note type." is Anki's message) and one whose first note is missing with `not_found`. Browse then shows a warning naming the cause (the wording is U87's, M4) and does not open the screen, as Anki shows its warning and opens no dialog. M5 to M10 are the screen and the change; their `not_found` and `multiple_notetypes` refusals after the screen is open (a re-query on a target change, or M8's commit) are the screen's. Browse passes the notes in B1's order. A refusal writes nothing.

## B9. Add Tags, Remove Tags, Clear Unused Tags (R1190, R1191)

- **Add Tags**: `tags/tags` T10 Add on the target notes, with the text the tag entry gives (autocomplete from the registry, `tags/complete`). Anki's Browse dialog first turns every run of U+0020, LF, TAB and VT in the text into one space, so a TAB there separates two tags (T10 alone would remove it inside a name). Meguri's entry splits the same way. Empty text runs nothing. Changed 2026-11-21 (implementation review): the op refuses a text that gives no tag, for Add Tags and Remove Tags, with `InvalidArgument`, writes nothing and leaves the undo and redo histories as they were; the entry never sends one, and the screen does not show that refusal as an error.
- **Remove Tags**: T10 Remove on the target notes. Anki passes the text as typed, so a TAB or LF there is part of a name, which then matches no tag. Meguri's entry splits the text as for Add Tags first (R1190: both entries split on whitespace), so a TAB or LF separates two names (B18).
- **Clear Unused Tags**: T11 on the whole collection; enabled without a selection.
- Steps "Update Tag", "Remove Tag" and "Clear Unused Tags" (T15).

## B10. Set Due Date, Reset and Grade Now (R1196 to R1199)

| Action | Operation | From Browse |
|---|---|---|
| Set Due Date | `sched/set-due-date` S1 to S7 on the target cards | context browser: the dialog is prefilled from `setDueBrowser` (default `"0"`), and the text as typed is stored there in the same "Set Due Date" step (S6) |
| Reset (Forget) | `sched/manual-ops` M6 on the target cards in B1's order | context browser: the two choices are prefilled from `restorePositionBrowser` (default true) and `resetCountsBrowser` (default false) and stored there in the same "Reset Card" step (M6.4) |
| Grade Now | `sched/manual-ops` M7 on the target cards in B1's order | buttons Again, Hard, Good and Easy; in Meguri the keys 1 to 4 always work, and so do the user's own answer keys when they differ (R1199); nothing is remembered. Anki binds exactly one key per button, the profile's answer key for that rating (by default 1 to 4), so a rebound rating loses its digit there (B18) |

- The remembered keys are written inside their operation's step, never as a separate write without an undo step, so an earlier step stays undoable below them (`browser/config-keys` K6.6).
- A card in a filtered deck: Set Due Date and Reset take it out (S4, M6.2); Meguri's Grade Now refuses the whole selection with `InFilteredDeck` (M7), where Anki answers it with filtered-deck states.

## B11. Delete Notes (R1212)

Changed 2026-11-20 (round 8): Delete asks when more than one note goes or the one note has more than one card (`browser/workspace` V8.2); one note with one card is deleted at once.

- `notes/note-records` D1 on the target notes: every card of each note goes too, one step "Delete Note", fully undoable. No confirmation.
- Before deleting, Anki selects and focuses one row whose note is not a target, skipping rows of deleted items: the nearest such row below the bottom selected row, else the nearest above the top selected row, else none (the selection is cleared). Meguri does the same.
- Anki's operation returns the number of cards removed and its message counts cards ("`N` cards deleted."). Meguri's operation returns both counts, notes removed and cards removed; its message counts notes (R1212).
- Changed 2026-11-21 (implementation review): the operation takes no confirmed counts. Undo restores what it deletes, and no line of this file asks for `notetypes/draft-save` D8's confirmation here; the question of `browser/workspace` V8.2 is the screen's.
- What the editor pane, the preview and Card Info show after the move: `browser/inspector` I6 and `browser/preview` P9.

## B12. Export Notes (R1213)

Opens the `.apkg` export with the limit `Notes(ids)` of the target notes (`package/apkg-export` A2), with the export's own options and defaults. Nothing is written to the collection.

## B13. Add Notes and Create Copy (R1214)

- **Add Notes** opens the Add screen as `notes/add-defaults` D1 says, with no preset deck.
- **Create Copy** opens it with the current note's note type, fields and tags and the current card's home deck (`notes/add-defaults` D3, row Create Copy). In notes mode the current card is B2's. With no current row, or with no current card (a notes-mode note without cards), it does nothing; Anki fails an internal check there (B18).

## B14. Table copy (Meguri, R1215)

Anki has no table copy. Meguri's ⌘C on the table copies the selected rows as tab-separated text (U87's `copy_text`):

- one line per selected row, in table order, lines joined by LF with no final LF;
- one cell per visible column, in display order, joined by TAB;
- each cell is the text the table shows for it: the cell of `browser/columns` C7 in cards mode or C8 in notes mode, displayed as C12 says, with the strings of C14, with every TAB, LF and CR replaced by U+0020; an empty cell is the empty string;
- the values are read from the rows as stored in `copy_text`'s own read transaction when it runs, not taken from what the table last drew; they equal what the table shows once the change stream has refreshed its rows (R1155);
- a row whose card or note was deleted since the selection was taken is left out.

## B15. Filtered decks (R1268)

Meguri never creates or rebuilds a filtered deck (`decks/decks` D9), but cards that another client put into one are handled by every action as follows:

| Action | A card in a filtered deck |
|---|---|
| Toggle Suspend, Toggle Bury | only its queue changes; it stays in the deck (`sched/manual-ops` M1) |
| Flag, Toggle Mark, tags, Find and Replace | its deck is not touched |
| Change Deck | goes home, then to the target (B7.2 step 3); filtered decks are never targets (B7.1) |
| Set Due Date, Reset | taken out to its home deck (B10) |
| Grade Now | refused with `InFilteredDeck` (B10) |
| Change Note Type | stays where it is (`notetypes/change-notetype` M9) |
| Delete Notes | removed with its note |
| Export Notes | exported as `package/apkg-export` A5 and A6 say |
| Create Copy | the copy's deck is the card's home deck (B13) |

The Deck column shows such a card as `browser/columns` C7.4 says.

## B16. Results and undo steps (R1182, R1184)

Changed 2026-11-20 (round 8): no result message or toast; the Undo titles use Meguri's display names (`browser/workspace` V8.3). The step ids and counts here stay.

Each action is one operation and one undo step on the shared stack (`engine/undo`), named after Anki's operation. An operation that changes nothing records no step.

- **Step names.** R1182's "named after the action" is read as Anki's operation name, the step names of `specs/conformance/undo-ops.md` V4, which `undo_status` compares with the oracle. So the Undo item reads "Undo Change Deck" after Change Deck, but "Undo Update Tag" after Find and Replace in Tags, Add Tags or marking, "Undo Remove Tag" after unmarking, and "Undo Suspend" or "Undo Unbury/Unsuspend" after Toggle Suspend. Changed 2026-11-21 (implementation review): Browse's Unsuspend and Unbury (`browser/workspace` V8.1) both record the one step "Unbury/Unsuspend" (U87); the titles the user sees follow the command chosen ("Undo Unsuspend Cards", "Undo Unbury Cards", V8.3), and the step name stays V4's.
- **Counts.** Anki's column is what Anki's operation returns. Meguri's column is the bulk framework's `{affected, considered}` (`browser/bulk-framework` K5: kept with the started job; `JobFinished` carries only the outcome; changed 2026-11-21, implementation review) for every action. `considered` is always the number of target ids B1 gave (cards for card actions, notes for note actions; ids of items deleted since included), except for Clear Unused Tags, whose target is the registry, and for the all-notes Find and Replace (`browser/find-replace` F8). The message (R1184, U219) uses `affected`, and also `considered` where it reads "`N` of `M`".

| Action | Operation | Step (V4) | Anki returns | Meguri `affected` |
|---|---|---|---|---|
| Toggle Suspend, suspending | M3 | Suspend (`suspend`) | cards written | cards written |
| Toggle Suspend or Bury, restoring | M4 | Unbury/Unsuspend (`unbury_unsuspend`) | nothing | cards written |
| Toggle Bury, burying | M2 | Bury (`bury`) | cards written | cards written |
| Unsuspend, Unbury (`browser/workspace` V8.1; changed 2026-11-21, implementation review) | M4 on the suspended, or the buried, target cards only | Unbury/Unsuspend (`unbury_unsuspend`) | (no such Anki command) | cards written |
| Flag, No Flag | `tags/flags-and-marked` F2 | Set Flag (`set_flag`) | cards written | cards written |
| Toggle Mark, adding; Add Tags | `tags/tags` T10 Add | Update Tag (`update_tag`) | notes written | notes written |
| Toggle Mark, removing; Remove Tags | T10 Remove | Remove Tag (`remove_tag`) | notes written | notes written |
| Clear Unused Tags | T11 | Clear Unused Tags (`clear_unused_tags`) | rows removed | rows removed; `considered` the rows the registry held |
| Change Deck | B7.2 | Change Deck (`change_deck`) | cards moved | cards moved (B7.2 step 5) |
| Change Note Type | `notetypes/change-notetype` M9 | Change Note Type (`change_note_type`) | nothing | notes changed |
| Set Due Date | `sched/set-due-date` | Set Due Date (`set_due_date`) | nothing | target cards that exist, each of which it writes |
| Reset | M6 | Reset Card (`reset_card`) | nothing | target cards that exist, each of which it writes |
| Grade Now | M7 | Grade Now (`grade_now`) | nothing | target cards that exist, each of which it answers |
| Delete Notes | `notes/note-records` D1 | Delete Note (`delete_note`) | cards removed | notes removed, with the cards removed as a second count (B11) |
| Find and Replace | `browser/find-replace` | Find and Replace (`find_and_replace`); Update Tag in Tags | notes written (every matched note) | notes written (`browser/find-replace` F9) |
| Edit Field | `browser/batch-edit` (Meguri) | Meguri's | | notes written |

Anki's English messages, for comparison only (Meguri's text is U219's):

| Action | Anki's message |
|---|---|
| Flag, Change Deck | "`N` cards updated." (`N` its count) |
| Add Tags, Remove Tags, Toggle Mark | "`N` notes updated." (`N` its count) |
| Clear Unused Tags | "Removed `N` unused tags." |
| Set Due Date | "Set due date of `N` cards." (`N` the number of selected cards, not a returned count) |
| Reset | "Reset `N` cards." (`N` the number of selected cards) |
| Grade Now | "Graded `N` cards." (`N` the number of selected cards) |
| Delete Notes | "`N` cards deleted." |
| Find and Replace | `browser/find-replace` F9 |
| Toggle Suspend, Toggle Bury, Change Note Type | none |

Each has a singular form for a count of 1 ("`N` card updated.", "Removed 1 unused tag.").

## B17. Scenarios

| Scenario | Covers | Ops |
|---|---|---|
| `browser/bulk/toggle-suspend` | B3, AE45: both directions on a three-card selection, a buried card unburied by the restore, one step each | existing |
| `browser/bulk/toggle-bury` | B4: the restore also unsuspends; burying leaves a suspended card and re-buries a sibling-buried one by hand | existing |
| `browser/bulk/flag-notes-mode` | B1, B5: all cards of two notes flagged, then cleared by the toggle, then No Flag | existing |
| `browser/bulk/mark` | B6: marking adds `marked` to every target note, unmarking removes `Marked` and `marked` alike | existing |
| `browser/bulk/set-due-notes-mode` | B1, B10: `3-7!` on the three cards of two notes, manual review-log rows, `setDueBrowser` stored as typed | existing |
| `browser/bulk/set-due-after-op` | B10: the remembered text inside the "Set Due Date" step; after its undo the earlier step is still undoable | existing |
| `browser/bulk/reset-browser` | B10: Reset with restore on and counts kept, its marker rows, the browser keys | existing |
| `browser/bulk/grade-now` | B10: Grade Now Good on two cards, review-log rows with time 0, other cards untouched | existing |
| `browser/bulk/delete-notes` | B11: three notes and their cards removed in one step, graves; undo restores the rows | existing |
| `browser/bulk/change-notetype-mixed` | B8: a selection over two note types refused | existing |
| `notes/change-deck/remaining-steps` | B7.2 steps 3.1, 3.2 and 5: learning and relearning counts translated to the target preset, between two presets and between two decks that share one (a count above the list length becomes the length, a count of 0 becomes 1); an empty old list leaves the count unchanged; a filtered card sent home, with its home preset read; a review card's `left` kept; the count | `change_deck` (X2); held: recorded, kept on branch `held/harvest-batch-2` until the testkit parses it |
| `notes/change-deck/memory-states` | B7.2 steps 2 to 5 with FSRS on: a move of cards already in the target (count 0, nothing computed); moved cards and a card already in the target without a state computed with the target preset's desired retention; a new card left without a state; `due` and `ivl` kept; a card that holds a state recomputed when it moves again; the source deck untouched | `change_deck` (X2); held: recorded, kept on branch `held/harvest-batch-2` until the testkit parses it |
| `notes/change-deck/target-state-kept` | B7.2 step 4: a target card that holds a state not written although its `dr` differs from the target's desired retention; subdeck and other-deck cards untouched; a filtered card whose home deck is the target computed and left in its filtered deck; a card with only a set-due-date row given `dr` and `decay` only; new cards, one with a review-log row, untouched | `change_deck` (X2); held: recorded, kept on branch `held/harvest-batch-2` until the testkit parses it |
| `notes/change-deck/fsrs-off` | B7.2 steps 3.3 and 4 with FSRS off: memory keys removed from moved cards, `pos` kept, nothing computed, a card already in the target untouched | `change_deck` (X2); held: recorded, kept on branch `held/harvest-batch-2` until the testkit parses it |
| `search/state/memory-state-moves` | B7.2 step 4: the set read by stability: a target card with `d` but no `s` computed, one with `s` but no `d` left as stored | `change_deck` (X2); held: recorded, kept on branch `held/harvest-batch-2` until the testkit parses it |
| `notes/change-deck/basic` | B7.2 steps 1 to 3 with FSRS off: a selected card in filtered `Cram` whose home deck is the target (moved, counted, `due` from `odue`, `odid` and `odue` 0), a suspended and a buried card from another deck moved with their queues kept, a card already in the target untouched and not counted, the card of a note deleted earlier skipped and not counted; then a move into `Cram` refused with `filtered_deck`, nothing written and "Change Deck" still on top of the undo history | `change_deck` (X2); planned, not recorded: it needs the typed model to parse `change_deck` (X5) and X2 to state that a reference to a deleted card is passed on by its real id |

None is active. The `browser/bulk/` rows are in the tree with oracle goldens. The Change Deck rows use the draft op `change_deck` (`specs/conformance/bulk-ops.md` X2). Those marked held are recorded with goldens but are not in the tree: Meguri's typed scenario model does not parse `change_deck` yet, so in the tree they would be invalid and fail `tools/mg goldens-check`, not pending (X5). They land under `conformance/scenarios/notes/change-deck/` (and `search/state/`) with the model change (U87; changed 2026-11-21, implementation review: U86 did not add `change_deck`, `specs/conformance/bulk-ops.md` X5); from then on they are pending until Meguri's executor runs `change_deck`, and U87 makes them active. Changed 2026-11-21 (implementation review): U87 (PR #77) adds `change_deck` to the typed model and the executor; once it is on `master`, the spec side lands the held rows from `held/harvest-batch-2` and activates those that pass T0. The planned row covers B7.2 steps 1 to 3's refusals, ids that name no card, a selected filtered card whose home deck is the target, and a suspended or buried card kept so; until it is recorded, U87's tests cover them from B7.2 and its table. A Change Deck scenario that reaches step 4's unchanged case (a target card written by Anki with the values it held) also needs the deviations `memory-recompute-unchanged-mod` and `memory-recompute-unchanged-usn` extended to it; none of the rows above does.

The toggle directions (B2 to B6), the chooser (B7.1), the Add Tags text rule (B9), the Delete Notes focus move (B11), Create Copy (B13) and the table copy (B14) live in Anki's desktop UI, which the oracle does not run. Meguri's tests cover them from this file's tables (U87 `core/crates/meguri-engine/tests/bulk_actions.rs`, U219, U220): each toggle in both directions in both modes, with and without a current card, a notes-mode note without cards (Toggle Mark adds, Create Copy does nothing), a notes-mode current card that is not the note's lowest-id card, the direction read when the op runs after the current item changed, the inline new deck in one "Change Deck" step undone together with every deck row it created (B7.2 step 0: an existing name, an existing filtered deck, D3's refusals, a blank name), the chooser filter (the first row selected after each edit) and Default omission, Change Note Type on a mixed selection (enabled, a warning, no screen), `InvalidArgument` for repeated ids and an empty list, B16's `affected` and `considered` for every action, `copy_text` on two rows × [Sort Field, Due], and R1831's budget (10k notes in at most 2 s, U242). Changed 2026-11-21 (implementation review): U87 did not build the toggles of B2 to B6 (`browser/workspace` V8.1 supersedes them for Browse), Grade Now, Change Note Type's warning (B8) or `copy_text` (B14), and Browse has no B7.1 chooser list (the chooser filter and Default omission tests above are not Browse's); the tests of what was not built go with it to later units. U87's tests cover the rest of this list that it built, with V8.1's Unsuspend and Unbury subsets, the review admission (`browser/bulk-framework` K2) and an exact undo and redo of every action.

## B18. Differences from Anki

| Difference | Why | Where |
|---|---|---|
| A new deck made in the Change Deck chooser is part of the Change Deck step | U87 | B7.1 |
| The chooser opens on the current card's home deck | R1187 | B7.1 |
| Delete Notes reports notes and cards | R1212 | B11 |
| A No Flag item in Browse's Flag menu | R1195 | B5 |
| Grade Now refuses cards in filtered decks | `sched/manual-ops` M7 | B10, B15 |
| Grade Now always takes 1 to 4 besides the user's answer keys | R1199 | B10 |
| Remove Tags splits its text at whitespace runs as Add Tags does | R1190 | B9 |
| Targets in table order, not in the order the rows were selected | `browser/result-sets` H12 | B1 |
| A toggle reads the current item's state when it runs | U87 | B2 |
| Repeated ids or an empty list refused | engine invariant | B7.2 |
| A tag text that gives no tag refused (Add Tags, Remove Tags) | engine invariant (changed 2026-11-21, implementation review) | B9 |
| The all-notes target refused for every note action but Find and Replace | engine invariant (changed 2026-11-21, implementation review) | B1 |
| Every action reports `affected` and `considered` counts | U86 | B16 |
| Change Deck's memory-state step does not write a processed card whose `s`, `d`, `dr` and `decay` come out as stored | R687 (`sched/memory-state` M4.2) | B7.2 step 4 |
| Create Copy on a notes-mode note without cards does nothing | Anki fails an internal check there | B13 |
| Table copy | R1215 | B14 |
