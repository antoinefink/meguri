# Editing, editor option, palette and week-start config keys (U160, U192, U195)

Spec-writer notes for `specs/editor/config-keys.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-10-27 by mapping each
section of the committed text to the files that back it. The storage rules the
spec builds on are `specs/storage/config-table.md`; typed reads of boolean keys
were probed for `specs/browser/config-keys.md` (`browser-config.md`).

Meguri's own design, with no Anki source: writing only the key the user
changed and never on open or close (K1), the skip-undo write (K1), the photo
import (K2), the name-order fallback for the note type (K4.1, decided),
offering only the four weekdays Anki reads (K5, decided), the strict boolean
reads and skip-undo writes of K8, and K9's read forms, `#ffffff` as an empty
slot (decided) and writes only on a palette change.

Follow-up: the K8 to K11 rows were mapped on 2026-10-27 against the
uncommitted working-tree text of those sections, which another track owns.
When they are committed, recheck the rows against the committed ids and text.

| Spec section | Anki files read | Read for |
|---|---|---|
| config-keys K1 | `rslib/src/config/bool.rs`, `rslib/src/config/mod.rs`, `rslib/src/backend/config.rs`, `rslib/src/storage/config/mod.rs`, `rslib/src/config/schema11.rs` | the four key names, their types and defaults (`addToCur` true in a new collection), typed reads falling back on any decode failure |
| config-keys K2 | `qt/aqt/editor_legacy.py` (image paste), `qt/aqt/preferences.py`, `qt/aqt/forms/preferences.ui` | PNG or JPEG for pasted image data, read at each paste |
| config-keys K3 | `qt/aqt/editor_legacy.py` (paste mode with and without Shift), `ftl/core/preferences.ftl` | the plain paste's mode, Shift inverting it, Anki's label |
| config-keys K4, K4.1 | `rslib/src/adding.rs` (defaults for adding: current deck, current note type, the deck's last note type, the note type's last deck), `rslib/src/config/deck.rs`, `rslib/src/config/notetype.rs`, `rslib/src/decks/current.rs`, `pylib/anki/collection.py` (the review card's current deck as input), `qt/aqt/addcards_legacy.py`, `ftl/core/preferences.ftl` | the two branches of `addToCur`, the fallbacks for a missing or filtered `curDeck`, the review deck from the card's `odid` or `did`, Anki's labels |
| config-keys K4.2 | `qt/aqt/addcards_legacy.py` (note type change), `rslib/src/adding.rs` (default deck for a note type) | the deck after a note type change for each value of `addToCur` |
| config-keys K4.3 | `rslib/src/notes/mod.rs` (add note), `rslib/src/config/deck.rs`, `rslib/src/config/notetype.rs` | the keys an add writes whatever `addToCur` holds |
| config-keys K5 | `rslib/src/config/mod.rs` (first day of week), `rslib/src/stats/graphs/mod.rs` (graph preferences), `proto/anki/stats.proto` (weekday enum), `ts/routes/graphs/calendar.ts`, `ts/routes/graphs/CalendarGraph.svelte` | the four stored values, any other read as Sunday, the calendar's week rows and its week-start menu |
| config-keys K6 | `rslib/src/preferences.rs` (preferences written as one undoable step), `qt/aqt/preferences.py`, `rslib/src/stats/graphs/mod.rs` (graph preferences write), `rslib/src/undo/mod.rs` | Anki writing every editing key when Preferences closes and every graph key on any graph setting change; which writes are undo steps |
| config-keys K8 | `qt/aqt/editor_legacy.py` (options on note load, options menu), `ts/routes/editor/NoteEditor.svelte`, `ts/routes/editor/editor-toolbar/OptionsButton.svelte`, `ts/routes/editor/plain-text-input/PlainTextInput.svelte` | the two key names and their `true` defaults; reads when a note loads; each menu item writing the opposite of the value read |
| config-keys K9 | `qt/aqt/editor.py`, `qt/aqt/editor_legacy.py` (palette loaded into the colour dialog), `qt/aqt/mediasrv.py` (palette save), `ts/routes/image-occlusion/Toolbar.svelte` | the array of colour strings, one per colour-dialog custom slot (16), unset slots saved as the dialog's white, the image occlusion toolbar reading and saving the same key |
| config-keys K10 | the files of K8 and K9, `ts/lib/tslib/profile.ts` (editor config writes), `pylib/anki/collection.py` (config set), `ts/routes/editor/editor-toolbar/ColorPicker.svelte`, `ts/routes/editor/editor-toolbar/TextColorButton.svelte`, `ts/routes/editor/editor-toolbar/HighlightColorButton.svelte` | the editor's script-value reads of the two options; its config writes marked undoable; the palette saved from the dialog's state when a picker opens or a colour is chosen; colour names accepted by the toolkit on load |
| config-keys K11 | none (2026-11-10: the K10 files, rechecked for a collection-library read of the three keys; there is none) | scenario list and the decision to cover the stored side with `editor/formatting/options/key-writes`, Meguri's own |
| config-keys K12 | `rslib/src/config/bool.rs` (the two keys default to true), `rslib/src/config/mod.rs` (typed read, any decode failure gives the default), `rslib/src/config/schema11.rs` (not in a new collection), `rslib/src/stats/graphs/mod.rs` (graph preferences read and write) | the two graph keys' names, boolean type and `true` defaults; absence from a new collection; Anki's write of all three graph keys together |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| conformance/scenarios/editor/config-keys/deck-key-curmodel-fallback (K4.1 true row, missing and stale deck key fall back to curModel), with notes/add-defaults/after-add and editor/config-keys/add-defaults-current-deck | `ts/tests/e2e/context-switching.spec.ts` | Which behavior to prove: with addToCur true, a missing or stale deck-to-note-type key falls back to the global note type and keeps the current deck. The expectations come from the oracle golden. |
