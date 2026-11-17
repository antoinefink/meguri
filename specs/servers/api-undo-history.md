# Undo history after local-server writes

Spec: servers/api-undo-history
Requirements: R1795, R1796 (the comparison with Anki and the test cases); supports R1799, R1889 (the deviation entry), AE80 (the undo part)
Anki: 26.09.3 (the undo history of Anki's collection library); the AnkiConnect add-on build of 2025-11-09 (U136's pin) for the action outcomes of Y4
Scenarios: `conformance/recordings/servers/ankiconnect/undo/` (oracle transcript recordings of Y4, recorded 2026-11-17). They live under `recordings/` until the DSL suite and goldens-check skip `servers/` (`conformance/AGENTS.md`, `servers/transcripts` T2).

What Anki's undo history does when a client changes the collection through AnkiConnect, how Meguri's rule for the same writes (`engine/undo#M7`) differs, the oracle recordings that settle Anki's side for each write action, and the cases Meguri's own tests take from M7 (U147, `core/crates/meguri-servers/tests/undo_truncation.rs`). This file changes no Meguri rule: `engine/undo` M2 and M7 are the rule for Meguri, and where an example here disagrees with them, they win and this file is corrected.

## Y1. Terms

- **History**: the undo history and the redo history of one open collection (`engine/undo`). A step is named by its id from `conformance/undo-ops` V4 (`update_note`, `add_note`, ...).
- **Write action**: an AnkiConnect action flagged mutating in `servers/ankiconnect-envelope#E12.3`, except `sync`. The reference's `sync` needs a signed-in AnkiWeb account and the network, and Meguri's runs a CloudKit exchange (`servers/ankiconnect-gui#G9`), so no recording compares them.
- **Baseline**: the `undo_status` value (`conformance/undo-ops` V2) read right after a scenario's setup, before the first exchange. The oracle's setup leaves Anki's own steps of the setup's operations (`conformance/undo-ops` V5): its note type additions as "Add Note Type" steps and its note additions as "Add Note" steps. So the baseline is `add_note` when the setup adds notes, and `add_note_type` when it adds note types and no notes.
- **Outcome**: what one write did to Anki's history, one of Y2's rows.

## Y2. Anki's outcomes for one library call

Anki's collection library has no notion of who asks for a write. Each call the add-on makes for an action ends in exactly one of these outcomes ("kept, untouched" is the case of "kept" in which no collection operation ran at all), decided by the kind of call and, for an undoable operation, by whether it changed a row and whether it succeeded (`engine/undo#A2`):

| Outcome | The call in Anki | Undo history afterwards | Redo history afterwards |
|---|---|---|---|
| **recorded** | an undoable operation that succeeded and changed at least one row | its step on top, every step below it kept; a note update can merge into an "Update Note" step on top as `engine/undo#A3` says | empty |
| **kept** | an undoable operation that succeeded and changed nothing; or an operation that records no step and keeps the steps (a write without an undo entry of the kind `conformance/record-ops` W4 makes) | as before | empty |
| **kept, untouched** | no collection operation at all: a read, a check the add-on makes before calling the library, or a write only to the media folder and its media database (adding and trashing media files use the media database's own transaction) | as before | as before |
| **emptied** | an operation saved without any undo record (for example a note saved without an undo entry), a database statement that does not begin with `select`, or **any operation that fails and rolls back**, undoable or not, also when the failure comes at commit (a library check refuses a note type or a note) | empty | empty |
| **custom** | the library's custom undo step, which a caller opens under a name of its own: it is recorded even when nothing changed, and later operations can be merged into it | that step on top | empty |

- The outcome depends on the kind of call, on whether it changed anything and on whether it succeeded, never on which rows it changed. In particular, an emptied call drops steps that have nothing to do with the rows it wrote, and a recorded call leaves every step below it undoable, also one that would restore a row the call changed.
- A write action that makes several library calls (`addNotes`, the reference's rollback of a failed `addNotes` (`servers/ankiconnect-actions` A18 `ankiconnect-addnotes-rollback`), `updateNote`, `changeDeck` to a new deck, ...) has the combined result of its calls, in order: several steps, a recorded step followed by an emptying call, or an emptying call followed by a recorded step. Y5 says what a recording can tell apart.
- A custom step carries the caller's name, which has no V4 id; Y5 and Y9 Q2 apply.
- Not verified by recording: whether the user can undo a recorded write at once from Anki's Edit menu. Anki's main window refreshes its Undo item only after the operations it runs itself or after a full refresh, so an add-on that calls the library directly can leave the item stale until the next refresh. The add-on source was read: `guiUndo` calls the main window's undo. Whether the Edit menu shows the step at once is still not verified by recording (Y9 Q6). Meguri does not implement `guiUndo` (`servers/ankiconnect-envelope#E12.4`).
- These rules are Anki's and hold for every client of the library; which calls the add-on makes for each action is what the recordings of Y4 find out.

## Y3. Meguri's rule, against Anki's

Meguri's writes through either local server are ops of source `LocalApi`. Each is never recorded. When it changed something and at least one step intersects it, the newest intersecting step and every older step are dropped, whether or not those older steps intersect it; only the steps newer than that one stay. The redo history is cut the same way (`engine/undo#M2`, `#M7`, R1795, R1796). A `LocalApi` op that changed nothing, and one that fails and rolls back, leave both histories as they are (M2). So, for the same write:

| Anki's outcome | Meguri's result | Same? |
|---|---|---|
| recorded | nothing recorded; ⌘Z never reverts the write (R1795) | differs |
| kept | the same when no step intersects; the cut of M7 when one does | differs when a step intersects; the redo history differs too, see below |
| kept, untouched | the same: a read changes neither history; a media-only write changes no collection row, so it intersects nothing | same |
| emptied, by a write that succeeded | the cut of M7: the newest intersecting step and everything older go, the newer steps stay (R1796) | differs unless the top step intersects |
| emptied, by a write that failed | both histories as they were: Meguri's op rolls back whole (`servers/ankiconnect-envelope#E14`) and leaves the history (M2) | differs whenever the history held a step |
| custom | nothing recorded | differs |

- **Redo.** Every Anki operation empties the redo history, even one that changed nothing (`engine/undo#A2`). Meguri's `LocalApi` op cuts the redo history only at the intersecting redo step nearest the top and below (M7), so an API write leaves an unrelated redo available. This differs from every Anki outcome but "kept, untouched".
- **What ⌘Z shows.** After a cut, Meguri's ⌘Z undoes the kept steps; once they are undone, `peek(Global)` reports the reason `ChangedExternally` (`engine/undo#M3`). R1796 gives the words the user sees, "Can't undo: changed by an external tool"; no spec gives them a string key or a place yet (Y9 Q5, edit request to `review/reviewer-strings` S5 and `review/commands` K5).
- The differences are deliberate (R1795, R1796). Y6 states the deviation entry that the replays of Y4's recordings need.

## Y4. The recordings

Each recording starts with `observe: [undo_status]` (the baseline), then one exchange with the write, then `observe: [undo_status, ...]` with the rows that show the write took effect. Three recordings chain a second write after the first and repeat the last two steps for it, because their second write needs state that setup cannot give without raw statements (setup's draft `cards` and `revlog` sections write rows directly and could empty the history). The two `createModel` recordings end with a `modelNames` exchange after the last observation: the note type rows hold ids and stamps that differ between the sides, and the reply shows the result. The setups are chosen so that the baseline's top step differs from every step id the write could record, so the outcomes can be told apart (Y5).

All requests carry no `Origin` header and use `version` 6. Every scenario has the tags `undo-reference` (this file's: it marks the recordings of this table, and the deviation entry of Y6 may select by it) and `no-origin` (`servers/transcripts` T12). `#n1`, `#c1` are the setup's entities (`servers/transcripts` T10.1).

| Scenario (`servers/ankiconnect/undo/...`) | Setup | Write | Anki outcome |
|---|---|---|---|
| `read-control` | common | `deckNames` | kept, untouched: A equals B (`add_note`), so the other recordings can be read (Y5) |
| `add-note` | no notes | `addNote` of one Pair note in `Lang` | recorded (top only), `add_note` |
| `add-note-with-media` | no notes | `addNote` with an `audio` attachment from `data`, appended to `Back` (`servers/ankiconnect-media` X10) | recorded (top only), `add_note` |
| `add-notes` | no notes | `addNotes` of two Pair notes in `Lang` | recorded (top only), `add_note` |
| `add-notes-one-fails` | no notes | `addNotes` of a Pair note and a note of note type `Nope` (error `['model was not found: Nope']`; the reference adds the first, then deletes it) | recorded (top only), `delete_note`: the add-on adds the first note, refuses the second note's unknown note type itself before it calls the library for that note, then deletes the first note as an undoable "Delete Note" step. Rows changed inside the action (`graves` not observed), so Y5 reads recorded (top only), not custom |
| `update-note-fields` | common | `updateNoteFields` `#n1`, `Back` to a new value | emptied: the note is saved without an undo entry |
| `update-note-fields-unchanged` | common | `updateNoteFields` `#n1`, `Back` to its stored value | emptied: the note is saved without an undo entry, also when nothing changed |
| `update-note-fields-with-media` | common | `updateNoteFields` `#n1`, `Back` changed, with an `audio` attachment appended to `Back` | emptied: the note is saved without an undo entry |
| `update-note` | common | `updateNote` `#n1` with fields and tags | recorded (top only), `update_tag`. By the add-on's order of calls this is emptied (the field save) and then recorded (a `remove_tag` step for each old tag, an `update_tag` step for each new tag) |
| `update-note-tags-only` | common | `updateNote` `#n1` with `tags` only | recorded (top only), `update_tag` |
| `update-note-tags` | common | `updateNoteTags` `#n1` | recorded (top only), `update_tag` |
| `delete-notes` | common | `deleteNotes` `[#n2]` | recorded (top only), `delete_note` |
| `add-tags` | common | `addTags` `[#n1]`, `"extra"` | recorded (top only), `update_tag` |
| `add-tags-remove` | common | `addTags` `[#n1]`, `"old"`, `add: false` | recorded (top only), `remove_tag` |
| `remove-tags-then-clear` | common | `removeTags` `[#n2]`, `"spare"`; then `clearUnusedTags`, which then has `spare` to remove | recorded (top only) for each write: `remove_tag`, then `clear_unused_tags` |
| `replace-tags` | common | `replaceTags` `[#n1]`, `old` to `new` | emptied: each note is saved without an undo entry |
| `replace-tags-in-all-notes` | common | `replaceTagsInAllNotes`, `old` to `new` | emptied: each note is saved without an undo entry |
| `create-deck` | common | `createDeck` `Lang::Fresh` | recorded (top only), `add_deck` |
| `change-deck` | common | `changeDeck` `[#c1]` to the existing deck `Other` | emptied: a raw update statement on `cards` |
| `change-deck-new-deck` | common | `changeDeck` `[#c1]` to `Lang::Moved`, which does not exist yet (created first) | emptied: the deck is created, then a raw update statement on `cards` |
| `delete-decks` | common | `deleteDecks` `[Spare]`, `cardsToo: true` (an empty deck) | recorded (top only), `delete_deck` |
| `delete-decks-with-cards` | common | `deleteDecks` `[Lang]`, `cardsToo: true` (its two notes go) | recorded (top only), `delete_deck` |
| `create-model` | common | `createModel` of a two-field standard note type | recorded (top only), `add_note_type` |
| `create-model-refused` | common | `createModel` whose only template has plain text on its front (`notetypes/validation` V3 `no_front_field`; an error reply on both sides) | emptied (Y2: a failing operation) |
| `suspend-then-unsuspend` | common | `suspend` `[#c1]`; then `unsuspend` `[#c1]` | recorded (top only) for each write: `suspend`, then `unbury_unsuspend` |
| `suspend-then-suspend-false` | common | `suspend` `[#c1]`; then `suspend` `[#c1]` with `suspend: false` | recorded (top only) for each write: `suspend`, then `unbury_unsuspend` |
| `forget-cards` | common, FSRS on (config `fsrs: true`) | `forgetCards` `[#c1]` | recorded (top only), `reset_card` |
| `set-due-date` | common, FSRS on (config `fsrs: true`) | `setDueDate` `[#c1]`, `"1"` | recorded (top only), `set_due_date` |
| `store-media-file` | common | `storeMediaFile` of a new file | kept, untouched (media only) |
| `delete-media-file` | common | `deleteMediaFile` `old.mp3` (placed by setup) | kept, untouched (media only) |
| `gui-add-cards` | common | `guiAddCards` with a note for deck `Other`, no attachment (reply `0`; the note is not saved, `servers/ankiconnect-gui#G6`) | emptied: before it replies, the add-on saves the chosen note type through a path without an undo record (the reference's Add window selects the deck and note type at once, `servers/ankiconnect-gui#G11`) |

- **Common setup**: decks `Lang`, `Other` and `Spare`; the note type `Pair` (fields `Front`, `Back`; one template); notes `#n1` (`alpha`/`first`, tag `old`) and `#n2` (`beta`/`second`, tags `old` and `spare`), both in `Lang`; the media file `old.mp3`. Its baseline is the step of the last setup note, `add_note` (as recorded). `forget-cards` and `set-due-date` use it with FSRS on (config `fsrs: true`, as Meguri always runs FSRS); that changes no baseline.
- **No-notes setup** (`add-note`, `add-note-with-media`, `add-notes`, `add-notes-one-fails`): the deck `Lang` and the note type only, so the baseline is `add_note_type`: the oracle's setup records its note type addition as an "Add Note Type" step.
- The observed tables avoid the rows that the action-level deviation entries of `servers/ankiconnect-actions` A18 cover: no `config` table (`ankiconnect-add-defaults`), no `graves` in `add-notes-one-fails` (`ankiconnect-addnotes-rollback`). The replies and observations that may still show such an entry's difference are named in Y6.
- The last column is read from the goldens of 2026-11-17 with Y5's reading. Implementers still rely on Y3 and Y7 only, which do not depend on that column. Of 30 write scenarios (33 writes), 19 scenarios (22 writes) are recorded (top only), 9 emptied and 2 kept, untouched.

## Y5. Reading a recording

Let B be the baseline and A the `undo_status` after the write. `undo_status` shows only the top step of each history, so a recording reads the top, not what lies below it.

| A against B | Reading |
|---|---|
| `A.undo` is an id that B's top was not, and the write's rows changed | recorded with that step id, **or** emptied and then recorded (two calls, Y2), which this recording cannot tell apart |
| `A.undo` is an id that B's top was not, and no row changed | custom (Y2), which the driver reports as a run error before this table applies (below) |
| `A.undo` equals `B.undo`, `A.redo` is null | kept, or several calls whose last left B's step on top (B's redo is null after setup, so "kept" and "kept, untouched" read alike here; the media actions are "kept, untouched" by Y2 whatever they show) |
| `A.undo` is null and `B.undo` was not | emptied (the last call emptied the history, or a failing call did) |
| `A.undo` and `B.undo` both null | no result: the setup recorded nothing, so the recording cannot be read |

- So Y2's "every step below a recorded step stays undoable" is not checked by these recordings. A reading "recorded" means only that the top step is the write's; Y4 records it as "recorded (top only)" until a recording can look below the top (Y9 Q4 proposes the step that would).
- `read-control` must show A equal to B. Otherwise the recorder's own work between exchanges (the snapshot from which it gives entity tokens, `servers/transcripts` T10.1, or Anki's main window reacting to the add-on) changes the history, and no other recording here can be read.
- An undo step whose name has no V4 id is a driver run error (`conformance/undo-ops` V2): the add-on recorded a step under its own name, as a custom step (Y2). The recording then fails, and Y9 Q2 applies.
- Merging (`engine/undo#A3`) cannot show here: the baseline is never an "Update Note" step.

## Y6. Meguri's replay and the deviation entry

- Meguri's transcript executor starts these scenarios with both histories empty, as the DSL's undo scenarios do after their first raw statement (`conformance/undo-ops` V5): what the setup leaves in the history is not part of the language (edit request to `servers/transcripts` T20). None of Y4's writes is recorded by Meguri, and an empty history has nothing to cut, so on Meguri's side every `undo_status` output of these scenarios is `{"undo": null, "redo": null}`.
- So the `undo_status` outputs differ for two reasons: Meguri never records a server write (R1795), and Meguri's executor starts with no setup steps where Anki's setup leaves its "Add Note" and "Add Note Type" steps (the baseline, and `read-control` and the media recordings throughout, where nothing else differs). One entry covers both, since both are this pointer in these scenarios:

  | Member | Value |
  |---|---|
  | `id` | `ankiconnect-undo-history` |
  | `requirement` | R1795 |
  | `summary` | Writes through the local servers are never recorded in Meguri's undo history and cut it only below the newest step they intersect (`engine/undo` M7); Anki records, keeps or empties its history by the calls the add-on makes (`servers/api-undo-history` Y2, Y3). Meguri's transcript executor also starts every scenario with both histories empty, while Anki's setup leaves its "Add Note" and "Add Note Type" steps (`servers/transcripts` T20). |
  | `test` | `core/crates/meguri-servers/tests/undo_truncation.rs api_writes_never_recorded` (U147; with the other cases of Y7 in that file) |
  | `scenarios` | every id of Y4 |
  | `tags` | `[undo-reference]` |
  | `pointer` | `/steps/*/undo_status` |
  | `transform` | `replace` with `{"undo": null, "redo": null}` |

- The entry lands in `conformance/deviations.yaml` once `conformance/schema/deviations.schema.json` and `conformance/dsl` D13 accept T12's `tags` member, together with the other transcript entries (`no-origin`, R1739). The goldens can be committed before it; until it lands, a Meguri replay of these scenarios differs at `/steps/*/undo_status`.
- The replies and the other observations compare as in any oracle recording, with the envelope's entries (`no-origin`, `servers/transcripts` T12). `create-model`'s reply holds random ids, which T10 normalizes as for the client recordings that create a note type.
- Action-level entries of `servers/ankiconnect-actions` A18 are scoped to their own scenarios, so they do not reach these recordings by themselves. Where a golden here shows such an entry's difference, that entry's `scenarios` gain this recording's id; a new entry is never added for it. Expected: `create-model-refused`'s error reply at `/steps/1/reply/body/json/error` (`ankiconnect-error-text`): the recorded text is "Card template ⁨1⁩ in note type '⁨Duo⁩' has a problem.<br>Expected to find a field replacement on the front of the card template.", against Meguri's A17 text, and possibly `change-deck` and `change-deck-new-deck`'s `cards` rows (`ankiconnect-changedeck-op`). Any other difference fails the replay as usual.

## Y7. Cases for Meguri's tests (examples of `engine/undo#M7`)

The test cases U147 takes, each a history before one `LocalApi` write and Meguri's history after it. Steps are listed deepest first, the top last. As M7 says, a step intersects a write when the write changed a row in the step's journal, or changed a note the step touched (M1: a note whose row the step changed, or one of whose cards it changed). A write that changes only a card does not intersect a step that touched only that card's note row. A row the write stores again unchanged is not a changed row (M1's journal holds changed rows only). These follow from M7; they add no rule.

| # | History before | Write | History after | ⌘Z then |
|---|---|---|---|---|
| 1 | Answer Card on a card of note N (top) (AE80) | `updateNoteFields` N | empty, reason `ChangedExternally` | refused, `ChangedExternally`; N keeps the API's values |
| 2 | Update Note N; Answer Card on a card of M; Answer Card on a card of K (top) | `updateNoteFields` N | Answer Card (M); Answer Card (K) | two undos succeed, the third is refused with `ChangedExternally` (R1796) |
| 3 | Update Note A (a field edit that generated no card) | `addNote` (a new note B) | unchanged | undoes the edit of A |
| 4 | Update Note A | `updateNoteFields` A with the values A already has | unchanged (the op changed nothing, M2) | undoes the edit of A |
| 5 | Update Note A, then undone: the undo history is empty with reason `None`, redo holds Update Note A | `updateNoteFields` A (changed) | redo empty; undo history empty, and `peek(Global)` still reports `None`: no undo step was dropped | ⌘Z does nothing (nothing to undo); redo refused: nothing to redo |
| 6 | Update Note A, then undone: redo holds Update Note A | `updateNoteFields` B (changed) | redo still holds Update Note A | redo applies the edit of A (Anki would have emptied redo: Y3) |
| 7 | Change Deck on card c1 (top) | `suspend` [c1] | empty, `ChangedExternally` | refused |
| 8 | Answer Card on card c2 of note N | `addTags` [N] (the note row changes) | empty, `ChangedExternally` | refused |
| 9 | Update Note A | `storeMediaFile` (a new file) | unchanged (no collection row changed) | undoes the edit of A |
| 10 | Update Note A | any write while read-only mode is on (`servers/ankiconnect-envelope#E13`) | unchanged (refused, nothing written) | undoes the edit of A |
| 11 | Update Note A; Update Note B (top) | `multi` with `updateNoteFields` A, then `updateNoteFields` B (two ops in order, `servers/ankiconnect-envelope#E11`, `#E14`) | after the first: Update Note B (the cut is at A, the newest and only intersecting step); after the second: empty, `ChangedExternally` | refused |
| 12 | Move Received Card (`servers/migaku-received-log#L13`) of a row whose note is now N' | `deleteNotes` [N'] | empty, `ChangedExternally` | refused |
| 13 | Update Note A (a field edit that generated no card) | Migaku `/receive` creating a new note (`servers/migaku-receive`, source `LocalApi`) | unchanged | undoes the edit of A |
| 14 | Rename Deck D | `deleteDecks` [D], `cardsToo: true` | empty, `ChangedExternally` | refused |
| 15 | Update Note A; Add Note B (top) | `addNote` (a new note C) | empty, `ChangedExternally`: both adds moved the collection's new-card position counter `nextPos` (`notes/note-records#A3`), so the config row is in B's journal and changed by the write | refused |
| 16 | Update Note A | `addNotes` of a valid note and a note of an unknown note type (error, nothing written: `servers/ankiconnect-actions#A7.6`) | unchanged: the op rolls back whole or writes nothing (M2) | undoes the edit of A (Anki's result: `add-notes-one-fails`, Y4: recorded (top only), a "Delete Note" step on top. The add-on adds the first note, refuses the second note's unknown note type itself before it calls the library for that note, then deletes the first note as an undoable "Delete Note" step; rows changed inside the action, `graves` not observed) |
| 17 | Update Note N (top) | `suspend` [a card of N] (only the card row changes) | unchanged: the step's journal holds N's note row, not the card | undoes the edit of N |

- Cases 2, 3, 6, 11, 13 and 17 assume that the `col` row, whose `mod` every write moves and every step's journal holds, never makes a write intersect a step by itself (Y9 Q3, edit request to `engine/undo` M7). Read literally, M7 would cut the whole history at every write that changed something. Cases 1, 7, 8, 12, 14 and 15 end empty either way; cases 4, 9, 10 and 16 change no row.
- Case 15 is M7 read as written for a bookkeeping row: the config row `nextPos` is a changed row in both journals. So a user's "Add Note" step, and an "Update Note" step that generated a card, are cut by any API or Migaku note add; M4.2 would refuse that undo anyway, since the row no longer holds the step's after-image (edit request to `engine/undo` M7 to say so).
- Case 5: M7's "the reason becomes `ChangedExternally`" applies when undo steps are dropped; a cut of redo steps alone leaves the reason `peek(Global)` reported (edit request to `engine/undo` M7).
- Case 16 holds for every write that fails: an op that fails and rolls back, and a check that refuses before any op opens, both leave the history (M2). In Anki a failure inside the library empties both histories (Y2), so the two differ whenever the history held a step.
- The editor's part of AE80 (the banner of R1793) is `editor/edit-flows` W5.3 and W9 (U196); U147's UI tests check it and the ⌘Z part in the running app.

## Y8. What this file asks of other specs

- `engine/undo` M7 points here for Anki's result of the same writes, states that the `col` row never makes a write intersect a step and that bookkeeping rows such as `nextPos` do (Y7 cases 15, 17), and that a cut of redo steps alone keeps the undo reason. A2 lists that a failing operation empties both histories, and A11 records that Meguri's failing op leaves them.
- `servers/ankiconnect-actions` A3 and `servers/ankiconnect-envelope` E14, which state that no action is recorded, point here for Anki's; E18 gains the undo-history row.
- `servers/transcripts` T20 states that Meguri's executor starts a scenario with both histories empty, and T12's table gains the R1795 row.
- `review/reviewer-strings` S5 and `review/commands` K5 give R1796's words a key and a place (Y9 Q5).
- `conformance/deviations.yaml` gains Y6's entry when the schema and D13 accept T12's `tags` (edit request to `conformance/dsl` D13 and the schema).
- `servers/ankiconnect-actions` A18: the `ankiconnect-error-text` row's scenarios gain `servers/ankiconnect/undo/create-model-refused` (Y6).

## Y9. Open questions

- **Q1.** Settled 2026-11-17. Of Y4's 30 write scenarios (33 writes), 19 scenarios (22 writes) are recorded (top only), 9 emptied (field and tag replacements saved without an undo entry, `changeDeck`'s raw statement, a refused `createModel`, `guiAddCards`'s note type save) and the 2 media writes kept, untouched. So Anki's library lets the user revert most API writes, and R1795 is a deliberate deviation rather than a match to AnkiConnect's practical behaviour (subject to Q6: whether Anki's Edit menu shows the step at once). "Top only" is Y5's limit: a recording shows the top step, not every step below it. Y3's rule does not change.
- **Q2.** If the add-on opens a custom step under a name of its own (Y2), `undo_status` fails on the oracle (Y5). `conformance/undo-ops` V4 would then need an id for that name, marked as an add-on step, before the recording can be made.
- **Q3.** `engine/undo#M7` says a write intersects a step when it changed a row whose key is in the step's journal, and every op's journal holds the `col` row (`engine/undo#M1`: the step restores `col.mod`). Read literally, every `LocalApi` write that changed something would cut the whole history, which contradicts R1796 and AE80's intent. Meguri's engine already leaves the `col` row out of the intersection, and M4.2 compares the `col` row only on the columns a step changed other than `mod`, so an undo whose other `col` columns the write changed is still refused then. Proposed for M7 (edit request): the `col` row never makes a write intersect a step. Rows that bookkeeping changes, such as the config row `nextPos`, intersect as any other row (Y7 case 15); a row stored again unchanged is not changed.
- **Q4.** Looking below the top. `undo_status` shows only the top step, so no recording here tells "recorded on a kept history" from "emptied, then recorded" (Y5). A transcript step `undo` that runs the DSL's `undo` op (`conformance/undo-ops` V1) on the server's collection, on both executors, would: after a recorded write, `undo` then `undo_status` shows B's top again only when the steps below were kept. On Meguri's side the `undo` step's output differs too (nothing to undo), so the Y6 entry would gain its pointer. Proposed for `servers/transcripts` T7.1 and the recorder; until it exists, Y4's "recorded" means the top step only.
- **Q5.** R1796's "Can't undo: changed by an external tool" has no string key and no place. The Edit menu's Undo item is disabled with plain "Undo" when there is no step (`review/reviewer-strings#S5`), ⌘Z with nothing to undo shows nothing (`review/commands#K5`), and the Mac review screen has no on-screen undo (`docs/design/native-interface-contract.md` N11.1). AE80's "⌘Z reports that the undo is unavailable" and U147's test of it need one. A proposal is in the edit request to S5 and K5; the place is a design decision (`docs/design/workflow.md`).
- **Q6.** Whether the reference's Edit menu shows a recorded API write at once (Y2) is not verified by recording. The add-on's `guiUndo` calls the main window's undo, so it reverts the top step the main window acts on. It decides only how far Anki's users could undo API writes in practice, not Meguri's rule.
- **Q7.** Whether the add-on reacts to a write by refreshing Anki's main window in a way that runs further library operations (and so changes the history after the write) is what `read-control` and the per-action recordings show together; if `read-control` passes and a write shows an unexpected step, the recording names it. `gui-add-cards` may race: the reference replies at once while its Add window opens, so its deck and note-type selection may land after the observation; the two recordings of T16 catch that as drift.
