# Note editing strings (U190 to U197, U200 to U207)

Spec-writer notes for `specs/editor/editing-strings.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-11-10 by checking each
Product Contract requirement of U190 to U207 for a quoted Anki label, message
or name, then mapping each section to the files that back it. The English values
are Anki's interface wording; Anki's string ids are not used as keys (the keys
are Meguri's).

Meguri's own design, with no Anki source: the keys, the catalogs that hold
them, argument forms and plural variations, the typography rule (E1), the
Settings placement (E4), the split of Create Copy and Browse's Change Note
Type command to their units (E3, E9), the Note Types list's Restore to Default
row action (E6), the choices
of the discard questions (E7, E11) and the English-only rule for stored names
(E10).

| Spec section | Anki files read | Read for |
|---|---|---|
| editing-strings E3 | `ftl/core/notetypes.ftl`, `ftl/core/decks.ftl`, `ftl/core/editing.ftl`, `ftl/core/adding.ftl`, `ftl/core/actions.ftl` (create-copy, with-ellipsis), `qt/aqt/addcards.py` (close question and its buttons), `qt/aqt/addcards_legacy.py` (history menu), `ts/routes/editor/DuplicateLink.svelte`, `qt/aqt/reviewer.py` (the More menu's Create Copy item, wrapped with dots), `qt/aqt/forms/browser.ui` (Browse's Create Copy item, plain) | the chooser labels, the duplicate link, the deleted-note history entry, the discard question with Discard and Keep Editing; the two Create Copy labels E11 compares |
| editing-strings E4 | `ftl/core/preferences.ftl` | the two `addToCur` choices |
| editing-strings E5 | `ftl/core/editing.ftl`, `ts/lib/tag-editor/tag-options-button/TagAddButton.svelte` | the add-tag label |
| editing-strings E6 | `ftl/core/notetypes.ftl`, `qt/aqt/models.py` (row buttons, add choices, last-type refusal, delete questions) | Add and Clone choices, the two delete questions and when each shows; that the list's buttons are Add, Rename, Delete, Fields, Cards and Options with no Restore to Default and no dots on Fields and Cards (E11) |
| editing-strings E7 | `ftl/core/card-templates.ftl`, `ftl/core/actions.ftl` (with-ellipsis), `ftl/core/fields.ftl` (add-field), `qt/aqt/clayout.py` (part buttons, card type picker rows, cloze card picker, affects line and its count, Options menu and which items a cloze type gets, the deck override state suffix, the Add Field and Flip buttons, add, remove and discard questions, last-template refusal) | the part labels, the cloze picker rows `Card N`, the affects line counting the note type's notes and shown for both kinds, the add question only with notes, the remove question with a pluralized card count, the Options labels (Restore to Default wrapped with dots; Add, Remove, Rename, Reposition and Deck Override for standard types only; Copy Info to Clipboard and Browser Appearance for both), Flip for standard types only, Add Field, Fill Empty Fields, Discard changes? |
| editing-strings E8 | `ftl/core/fields.ftl`, `ftl/core/browsing.ftl`, `qt/aqt/fields.py` (list rows, delete checks and question) | the numbered rows, the last-field message, the protected-field message E11 drops, the delete question counting the note type's notes, the option labels including the sort field label |
| editing-strings E9 | `ftl/core/browsing.ftl`, `ftl/core/change-notetype.ftl`, `ftl/core/notetypes.ftl`, `qt/aqt/changenotetype.py` (window title; the toast and its count, the number of note ids the screen was given), `ts/routes/image-occlusion/store.ts` and `ts/routes/image-occlusion/Toolbar.svelte` (the mode store's starting value and the mode menu) | Change Note Type title and Browse's menu item E9 leaves to Browse, (Nothing), the notes-updated plural and what it counts, the two occlusion mode names and Hide All, Guess One as the default |
| editing-strings E10 | `qt/aqt/clayout.py` (new card type name), `ftl/core/card-templates.ftl` | `Card N` from the template count plus one, raised while the exact name exists |
| editing-strings E11 | the files of E3 and E6 to E9 | the Anki column of each difference |

Revised 2026-11-10 after review: the Card N example, which Options items and
Flip a cloze type gets, the affects line for both kinds, the Restore to Default
and Deck Override labels, the Create Copy and Change Note Type command split,
the catalogs, R926, Copy Info, Insert Field and the dropped refusals were
checked again against the files above.
