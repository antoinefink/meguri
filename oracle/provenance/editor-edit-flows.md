# Edit flows (U196)

Spec-writer notes for `specs/editor/edit-flows.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-11-10 (session
s-20261110T085429Z-165d83d3). The commit pipeline and the field patch are
`specs/editor/field-html.md` (`editor-field-html.md`).

Meguri's own design, with no Anki source: own-patch matching with held
events (W1), release after a Browse change that keeps the note, flushing before a reload, the
source-keyed policy and the `LocalApi` banner (R1793, W5.3), options-only
refresh, the failed-flush rule in Browse, the deleted-note notice, the sync
coordinator hand-off (R1550) and two hosts on one note.

| Spec section | Anki files read | Read for |
|---|---|---|
| edit-flows W2 | `qt/aqt/browser/browser.py` (editor shown for a single selected card, saved before a row change and before actions, preview shortcut Ctrl+Shift+P, reload on another handler's note text change, empty editor when the note is gone) | when the pane shows and saves; Anki's reload without saving |
| edit-flows W3 | `qt/aqt/editcurrent_legacy.py` (close after saving, Ctrl+Return and Ctrl+Enter on the close button, no prompt), `qt/aqt/editor_legacy.py` (save before callback) | closing semantics |
| edit-flows W5, W8 | `qt/aqt/editcurrent_legacy.py` (note reloaded from the collection on another handler's note text change, window closed when the note is not found), `qt/aqt/browser/browser.py`, `qt/aqt/editor_legacy.py` (late input for another note ignored) | Anki's reload and delete behavior listed in W8 |
| edit-flows W2, W8 (review round, 2026-11-10) | `qt/aqt/browser/browser.py` (the editor's card is the single selected row's card, which may differ from the current card; Esc in the window closes Browse after saving), `qt/aqt/browser/table/table.py` and `qt/aqt/browser/table/state.py` (in notes mode a row's card is the note's first card, the note's cards listed by template order), `rslib/src/storage/card/mod.rs` (a note's card ids ordered by `ord`), `qt/aqt/__init__.py` (macOS: the standard Close key closes the active window) | the notes-mode card rule, the Esc and keys bullets |
