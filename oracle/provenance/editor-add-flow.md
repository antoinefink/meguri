# Add flow (U195)

Spec-writer notes for `specs/editor/add-flow.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-11-10 (session
s-20261110T085429Z-165d83d3). Anki's default add window is the legacy one
(`qt/aqt/main.py`: the new one opens only with the experimental editor flag
or Shift), so the comparison reads `qt/aqt/addcards_legacy.py` and
`qt/aqt/editor_legacy.py`. Defaults, drafts and Create Copy are
`specs/notes/add-defaults.md` (`notes-add-defaults.md`).

Meguri's own design, with no Anki source: `unicase` note type order, the
default-deck exceptions of A2 (subdecks, only deck, selected deck), the
History kept since launch per collection with first-field labels and
grapheme counting (R815), the held flush around an add, the refusal line in
place of a dialog, stopping only the editor's own audio, keeping an unchanged
draft across a close, Discard dropping the draft, ⌥⌘P, the `editor.addScreen`
context, the default deck kept when selected or beside only filtered decks,
the add stopped by a replaced selection, the toast's Undo bound to the add's
own step, and the History label from the first shown field.

| Spec section | Anki files read | Read for |
|---|---|---|
| add-flow A2 | `qt/aqt/deckchooser.py`, `qt/aqt/notetypechooser.py` (Ctrl+D, Ctrl+N, choosing the same entry changes nothing), `qt/aqt/studydeck.py` (filter: lowercase, words split at spaces, substring), `pylib/anki/decks.py`, `rslib/src/decks/service.rs`, `rslib/src/decks/name.rs` and `rslib/src/storage/deck/mod.rs` (names ordered by stored name, filtered decks dropped, default deck dropped when no card has its id), `rslib/src/storage/upgrades/schema15_upgrade.sql` (deck name collation) | list contents, order, filter rule, the empty-default rule Anki applies |
| add-flow A3, A3.1 | `qt/aqt/addcards_legacy.py` (note type change: save first, deck from the note type when asked, last added note forgotten, fields copied by exact name when non-empty, then the rest by packed position, tags kept, focus index clamped) | the mapping and its edge cases (empty values skipped, extras dropped, case-sensitive names) |
| add-flow A4 | `qt/aqt/editor_legacy.py` (toggle one, toggle all: clear when any is sticky, set all otherwise; note type saved), `ts/routes/editor/StickyBadge.svelte` (F9 on the focused field), `ts/routes/editor/NoteEditor.svelte` (Shift+F9 in add mode), `qt/aqt/operations/notetype.py`, `rslib/src/notetype/service.rs` (the legacy update is the note type update op) | keys, the toggle-all rule, the save it makes and its undo step |
| add-flow A5 | `qt/aqt/addcards_legacy.py` (save first, double add guarded, check states: empty and cloze misuse refused, missing cloze asked, duplicates allowed; add op with the target deck; tooltip with the returned card count; audio stopped; history; next note with sticky fields), `qt/aqt/operations/note.py`, `ftl/core/adding.ftl`, `ftl/core/importing.ftl` (the count text), `ftl/core/notetypes.ftl` (image occlusion refusal) | the order of the steps and what each state does |
| add-flow A6 | `qt/aqt/addcards_legacy.py` (history: newest first, 15 kept, label from all fields cut at 30, deleted rows disabled, opening the browser on `nid:`), `ftl/core/adding.ftl` | Anki's history rules listed in A10 |
| add-flow A7 | `qt/aqt/addcards_legacy.py` (Esc and close through one check after a save, the discard question with Discard and Keep Editing), `qt/aqt/editor_legacy.py` (blank check against the last added note) | the guard's order |
| add-flow A9 | `qt/aqt/addcards_legacy.py` (Ctrl+Return, Ctrl+Enter, history key per platform), `qt/aqt/deckchooser.py`, `qt/aqt/notetypechooser.py`, `ts/routes/editor/StickyBadge.svelte`, `ts/routes/editor/NoteEditor.svelte` | the Anki column |
| add-flow A12 | `rslib/src/notetype/service.rs`, `rslib/src/notes/service.rs` and `rslib/src/notes/mod.rs` (the add reports the number of cards it created) | what the inactive scenarios exercise |
| add-flow A9, A10 keys (review round, 2026-11-10) | `qt/aqt/addcards_legacy.py` (Ctrl+Return, Ctrl+Enter, history key per platform), `qt/aqt/__init__.py` (macOS and Linux: the standard Close key closes the active window), `ts/lib/tslib/keys.ts` (the web editor maps Control to Meta on Apple platforms) | the "Anki on macOS" column and the keys bullet |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| editor/add-flow A12 scenarios notetype-deck-memory, deck-notetype-memory, last-deck-fallback (Add-screen context memory across adds and reopen) | `ts/tests/e2e/context-switching.spec.ts` | Read only to learn which behaviors the harvest rows ask for: a note type's last deck picked on a switch and on reopen; note type and deck kept after an add and a reopen; a missing or stale last deck falling back to the current deck; the deck's last note type with addToCur true; the selected deck in the add; the field list replaced on a note type switch. The goldens come from the oracle. The scenario contents (note types, decks, values) are original. |
| conformance/scenarios/editor/add-flow/selected-context (A5 step 5, notes/add-defaults D6) and the proposed A11 A5 line | `ts/tests/e2e/context-switching.spec.ts` | Which behavior to prove: an add carries the selected deck and note type, and both stay selected and are the defaults afterwards. The expectations come from the oracle golden. |
| editor/add-flow A5 (flush, field check refusal, the add), the proposed A11 change, and scenario editor/add-flow/add-leaves-other-notes | `ts/tests/e2e/note-add-roundtrip.spec.ts` | Which behaviors harvest rows h3632fc2d2bac, hd760eb8143a3 and hd9a760a2f259 require: the focused field flushed into the add, both fields and the deck committed with no update of existing notes, the next draft reset, and no add for an empty draft. The expected values come from the oracle golden only. |
| editor/add-flow A4 sticky pins and scenario editor/add-flow/sticky-pin-each-way; harvest rows h8c899c6506a2, hc01768203874 | `ts/tests/e2e/sticky-field.spec.ts` | Which behaviors the harvested e2e tests require: a single field's sticky toggle persisting on and off, and sticky values kept while other fields clear after an add. No assertions or values were taken; the golden comes from the oracle. |
