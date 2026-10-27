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
