# Undo history after local-server writes (integrations-ui pass, U144/U145/U147)

Spec-writer notes for `specs/servers/api-undo-history.md` and the recordings
`conformance/recordings/servers/ankiconnect/undo/*.yaml`.
Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. The Y4
recordings were run on 2026-11-17; their goldens are under
`conformance/recordings/servers/ankiconnect/undo/`. The AnkiConnect add-on
25.11.9.0 inside the recorder image (the build pinned in
`oracle/ankiconnect/ADDON_PIN`) was then read for which library calls each
write action makes (Y4 outcomes) and for `guiUndo` (Y2, Y9 Q6): see
"Transcript recordings (2026-11-17)" below.

| Spec section | Anki files read | Read for |
|---|---|---|
| Y2 (recorded, kept) | `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs` | an operation with an undo kind records a step only when it changed something, and every operation that begins in normal mode empties the redo history; the skip-undo kind keeps the undo steps |
| Y2 (emptied) | `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs`, `rslib/src/notes/mod.rs`, `rslib/src/notes/service.rs`, `pylib/anki/notes.py`, `pylib/anki/collection.py`, `rslib/src/config/mod.rs`, `rslib/src/backend/config.rs` | a transaction begun without an undo kind empties both histories; a note update saved without an undo entry runs as such a transaction (also the legacy note save); config has both a skip-undo write and a write that empties |
| Y2 (emptied, failing operation) | `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs`, `rslib/src/notetype/mod.rs` | on an error from the operation's work or from its commit, the transaction rolls back and discards undo: a step begun with no kind, which empties both histories, whatever kind the operation had; adding a note type validates inside its transaction |
| Y2 (emptied, raw statements) | `rslib/src/backend/dbproxy.rs`, `rslib/src/undo/mod.rs` | a statement not beginning with `select` empties both histories and drops the study queues |
| Y2 (custom) | `rslib/src/undo/mod.rs` | a step counts as having changes when it holds changes or is of the custom kind; the collection can add an empty custom step and merge later operations into it |
| Y2 (kept, untouched) | `pylib/anki/collection.py` (`save` and `autosave` do nothing at this tag); `rslib/src/media/service.rs`, `rslib/src/media/mod.rs` | the legacy save calls clients still make do not touch the history; adding and trashing media files run in the media database's own transaction, never the collection's |
| Y2 (Edit menu, unverified) | `qt/aqt/main.py` | the main window sets its Undo and Redo items from the collection's undo status only after its own operations or a full refresh; nothing read shows that the add-on triggers one |
| Y3 | none: Meguri's rule is `specs/engine/undo.md` M2 and M7 | the comparison |
| Y4 to Y7 | none: recordings and Meguri test cases derived from Y2 and `engine/undo` M7; `core/crates/meguri-engine/src/undo/entry.rs` and `stack.rs` (Meguri's engine) for how the `col` row and the redo-only cut behave today | |

The general rules of Y2 restate `specs/engine/undo.md` A2 (provenance
`oracle/provenance/undo.md`) for writes made by a client; new against A2 are the
note update without an undo entry, the failing operation and the custom step,
which A2 does not list (edit request for A2).

Recording hints for the oracle maintainer:

- The `undo_status` observation is the driver's (`oracle/driver/ops/undo.py`),
  run through the recorder's `observe` control op on `mw.col`; it reopens
  nothing, so the recorder accepts it.
- Record `read-control` first. If its two `undo_status` outputs differ, the
  recorder or Anki's main window changes the history between exchanges and the
  other recordings cannot be read (Y5).
- A step name with no V4 id fails the observation as a driver run error
  (`specs/conformance/undo-ops.md` V2); report the action (Y9 Q2) rather than
  adding a name to the driver.
- `create-model-refused`: if the add-on refuses the template in Python before
  calling the library, the history is kept; if the library refuses it, both
  histories are emptied. Either is a valid result; record what it is.
- `gui-add-cards` opens the Add window; its selection writes may land after the
  reply (Y9 Q7). Two recordings that differ are drift, never settled by keeping
  one.
- Record twice and compare before committing, as for every transcript golden.

## Transcript recordings (2026-11-17)

Session s-20261117T160047Z-35f65b90 (spec writer, servers transcripts).
Pins: Anki tag 26.09.3 (`refs/anki-26.09.3`) and the AnkiConnect add-on build
pinned in `oracle/ankiconnect/ADDON_PIN` (tag 25.11.9.0, commit `532a3c8`,
build 2025-11-09), installed as `addons21/2055492159` in the recorder image
`meguri-oracle-ankiconnect:recorder`. No code was copied into a spec or a
scenario.

| Subject | Sources | Read for |
|---|---|---|
| Y4 outcomes, the Y1/Y4 baseline corrections, and the Y2/Q6 `guiUndo` note | AnkiConnect add-on 25.11.9.0 inside the recorder image (`/opt/ankiconnect/addons21/2055492159/__init__.py`: the write actions `updateNoteFields`, `updateNote`, `updateNoteTags`, `addTags`, `replaceTags`, `changeDeck`, `deleteDecks`, `createModel`, `suspend`, `forgetCards`, `setDueDate`, `deleteNotes`, `guiAddCards`, `addNotes`, `storeMediaFile`, `deleteMediaFile`, and `guiUndo`). Anki 26.09.3: `pylib/anki/models.py` (legacy update), `rslib/src/notetype/mod.rs` (existing-id save without undo), `rslib/src/notetype/service.rs`, `rslib/src/decks/current.rs` (Select Deck op). Goldens `conformance/recordings/servers/ankiconnect/undo/*.json`. | Why each write records, empties or keeps the history: library calls saved without an undo entry, the raw update statement, the add-on's own note type check on the second note of `addNotes` and its rollback of the first, `guiAddCards`' note type save without undo, and `guiUndo` calling the main window's undo. |
| Y4 outcome column (recorded) | Goldens `conformance/recordings/servers/ankiconnect/undo/*.json`, each read with Y5 | recorded (top only): `add-note`, `add-note-with-media`, `add-notes` (`add_note`); `add-notes-one-fails` (`delete_note`: the first note's add, then the add-on's rollback delete); `update-note`, `update-note-tags`, `update-note-tags-only`, `add-tags` (`update_tag`); `add-tags-remove` (`remove_tag`); `remove-tags-then-clear` (`remove_tag`, then `clear_unused_tags`); `delete-notes` (`delete_note`); `create-deck` (`add_deck`); `delete-decks`, `delete-decks-with-cards` (`delete_deck`); `create-model` (`add_note_type`); `suspend-then-unsuspend`, `suspend-then-suspend-false` (`suspend`, then `unbury_unsuspend`); `forget-cards` (`reset_card`); `set-due-date` (`set_due_date`). Emptied: `update-note-fields`, `update-note-fields-unchanged`, `update-note-fields-with-media`, `replace-tags`, `replace-tags-in-all-notes`, `change-deck`, `change-deck-new-deck`, `create-model-refused` (the library refused the template), `gui-add-cards`. Kept, untouched: `read-control`, `store-media-file`, `delete-media-file`. Every redo was null. |
| Y1, Y4 baseline (corrected) | The same goldens' first `undo_status` | The common setup's baseline is `add_note`, as expected. The no-notes setup's baseline is `add_note_type`: the setup's note type addition also leaves its step, which Y1's baseline text (only the "Add Note" steps) omits. `read-control` showed the same value before and after its exchange, so the other recordings can be read (Y5). |
