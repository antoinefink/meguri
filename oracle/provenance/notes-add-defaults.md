# Add-screen defaults and the next note (U191, U195)

Spec-writer notes for `specs/notes/add-defaults.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-10-27 by mapping each
section of the committed text to the files that back it. The rules that pick
the deck and the note type are `specs/editor/config-keys.md` K4, whose sources
are in `editor-config-keys.md`; the keys an add writes are
`specs/notes/note-records.md` A4 (`notes.md`).

Meguri's own design, with no Anki source: the `preset_deck` input and its table
(D1, decided), the prompts before Create Copy or a prefilled draft replaces a
draft (D3, decided), trimming Unicode White_Space only (D4, decided), the
selection rules of D5 (decided) and the Add-screen model's tests.

| Spec section | Anki files read | Read for |
|---|---|---|
| add-defaults D1 | `rslib/src/adding.rs` (defaults for adding, deck resolved first with `addToCur` on, note type first with it off), `proto/anki/notes.proto` (defaults request and result), `pylib/anki/collection.py` (the review card's current deck sent as home deck, 0 without a card) | the input and output, the errors when no note type or no deck is left and which comes first, the review deck taken without a kind check |
| add-defaults D2 | `rslib/src/adding.rs` (default deck for a note type), `rslib/src/config/notetype.rs`, `proto/anki/notes.proto` | the last-deck key read without looking up the note type, a missing, unreadable, missing-deck or filtered-deck value giving none, deck id 0 for none |
| add-defaults D3 | `qt/aqt/addcards_legacy.py` (open, reopen with a blank draft, set note), `qt/aqt/addcards.py`, `qt/aqt/reviewer.py` and `qt/aqt/browser/browser.py` (Create Copy), `qt/aqt/browser/sidebar/tree.py` (Add Notes from a deck), `qt/aqt/main.py` (Add entry points) | when Anki asks for the defaults, what each entry presets, a non-blank draft kept on reopen, Create Copy's note type and deck |
| add-defaults D4 | `qt/aqt/editor_legacy.py` (fields blank check), `qt/aqt/addcards_legacy.py` (close check) | `<br>` removed and white space trimmed, sticky fields compared with the last added note, tags not looked at; Anki's trim set |
| add-defaults D5, D8 | `qt/aqt/addcards_legacy.py` (operation hook: the draft rebuilt in place, the last added note forgotten; a replaced note type treated as a change), `qt/aqt/browser/sidebar/tree.py` | what Anki does when another screen changes decks or note types; the differences listed in D8 |
| add-defaults D6 | `qt/aqt/addcards_legacy.py` (add, new note after an add: sticky fields and tags carried over), `rslib/src/notes/mod.rs` (add with the update-defaults flag), `proto/anki/notes.proto` | the flag Add-screen adds send, the next draft's fields and tags, the selection kept |
| add-defaults D7 | `pylib/anki/collection.py`, `proto/anki/notes.proto` | the backend requests the requested observation kinds would call |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| editor/config-keys K4.1 and notes/add-defaults D1/D2 (classified covered-existing) | `rslib/src/adding.rs` | The case list of the add-defaults test, used only to check that existing scenarios cover it. Nothing new was written. |
| Harvest rows h57651ff721da and hab1f710886fe linked to the existing notes/add-defaults and editor/config-keys scenarios (D1, D2) | `rslib/src/notes/service.rs` | The harvested tests, to confirm that their obligations (fresh-collection add defaults; a note type's default deck, none and then set) are the ones the recorded add_defaults and notetype_last_deck scenarios already show. |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| notes/add-defaults#D8 (A replaced note type; Browse's sidebar Add) | `qt/aqt/notetypechooser.py`, `qt/aqt/browser/browser.py`, `qt/aqt/addcards_legacy.py`, `rslib/src/adding.rs` | Checking that replacing a missing note type and Browse's add-from-sidebar (set deck, then set note type) both go through the note type change path that applies the last deck for either addToCur value |
