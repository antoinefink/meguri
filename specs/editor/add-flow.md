# Add flow

Spec: editor/add-flow
Requirements: R797, R802, R805, R809, R810, R811, R813, R814, R815, R816, R862, R245, R246, R247, R255, R259 (Add); supports R796, R800, R801, R806, R807, R808, R812, R817, R851, R1114; AE34, AE35
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/add-flow/ (recorded at the current oracle pin; inactive until Meguri's adapters land, A12)

What the Add screen (U195) does between opening and closing: its two choosers, a note type change and how the draft's content follows it, the sticky pins, the add itself, the History list, the close and the keys. The starting deck and note type, when the screen asks for them, blank and unchanged drafts, a selection that stops existing, Create Copy and the next draft after an add are `notes/add-defaults` D1 to D6; the `addToCur` rules are `editor/config-keys` K4. The field check is `notes/field-check`; the add op and the keys it writes are `notes/note-records` A1 to A5. The field editor, its commits and its flush are `editor/field-html`; the tag area is `editor/tag-editor`. The screen contract `editor.add` (`docs/design/screen-inventory.yaml`) owns layout, wording other than `editor/editing-strings` E3, states and VoiceOver. This file owns the behaviour it states: the contract's short summaries of it (which decks the deck chooser lists, what a History row shows) follow A2 and A6.

## A1. Terms and sources

- **Draft**: the Add screen's note being written: the selected note type and deck, one value per field (the fields' session targets in Add mode, `editor/field-html` S1) and the tag list (`editor/tag-editor` G11).
- **Empty value** (for A3 only): a field value that is the empty string. A value holding only `<br>` or white space is not empty here; `notes/add-defaults` D4's emptiness is for blank and unchanged drafts and is not used by A3.
- **Flush**: `editor/field-html` S1's flush, which also hands on the tag list. A **held flush** is one sent with `hold` (B3): input that arrives after it is refused until the host sends `release`.
- Anki's sources are its default (non-experimental) add window and editor. Its experimental editor is not compared.

## A2. The choosers (R797, R246)

Each chooser is a searchable list of names. ⌘N opens the note type chooser and ⌘D the deck chooser (R246); the Type and Deck rows open them too.

- **Note types.** Every note type of the collection, by stored name, in `unicase` order (`storage/schema-18`), ties by ascending id. When the collection has no image occlusion note type, the list also has the stand-in row of `editor/image-occlusion-notes` O10, sorted and filtered by its label with the stored names.
- **Decks.** Every normal deck by its full human name (`decks/decks` D17's names: stored components joined by `::`), in D17's order (`unicase` order of the stored name, ties by ascending id). Filtered decks are not listed. The default deck (id 1) is left out exactly when `decks/decks` D12 hides it from the deck tree (top-level, no child deck, no card with `did` 1, cards whose `odid` is 1 not counting, another top-level deck exists; R797), with two exceptions that keep it listed: it is the deck the draft has selected, or every other top-level deck is filtered (so the list is never empty). U195 reads the names from D17's snapshot and whether D12 hides the default deck from the deck tree query, which holds a node for id 1 exactly when D12 shows it; both are read when the chooser opens. Example: decks `Default` (no cards), `Spanish`, `Spanish::Mining`: the list is `Spanish`, `Spanish::Mining`; after a card is added to `Default`, `Default` is listed first.
- **Filter.** The search text is split at U+0020; empty pieces are ignored. A name is listed when every piece occurs in it as a substring, comparing under Unicode simple case folding. An empty search lists every name. Example: `sp min` lists `Spanish::Mining`; `mining sp` lists it too; `spm` does not.
- **Selection.** The list opens with the draft's current choice highlighted (when the filter leaves it listed; otherwise the first row). Choosing the current note type or deck again changes nothing. Choosing another note type is A3. Choosing another deck selects it and changes nothing else; the field check does not run again (it does not depend on the deck).
- The lists are read when a chooser opens and read again after any change set that lists decks or note types while it is open (`engine/change-stream` S1); the selection itself follows `notes/add-defaults` D5.

## A3. Changing the note type (R802, R800)

When the user picks another note type (A2), the Add screen:

1. makes a held flush (A1) and waits for the reply, so no input reaches the old draft after it;
2. builds the new draft's fields by A3.1 and keeps the tag list unchanged;
3. selects the deck by `editor/config-keys` K4.2 (with `addToCur` `false`, `notes/add-defaults` D2's deck when it gives one; otherwise the deck stays);
4. forgets the last added note (`notes/add-defaults` D4), so the close prompt then shows for any field that is not empty;
5. loads the new draft into the editor (`openNote`, which ends the hold) with focus in the field at the same position as the field that last had focus, or the new note type's last field when it has fewer fields (the first field when no field had focus yet);
6. runs the field check on the new draft (`editor/field-html` S3).

The note type change itself writes nothing to the collection.

### A3.1 Mapping the fields

Let the **old fields** be the draft's fields before the change, in order, and the **new fields** those of the chosen note type, in order. All new fields start empty.

1. **By name.** Each new field whose name equals, character for character (case counts), the name of an old field whose value is not empty (A1) takes that value. These new fields are **filled by name**, and those old fields are **used by name**.
2. **By position.** The old fields that are not empty and not used by name, in order, are put into the new fields not filled by name, in order: the first such old value into the first such new field, the second into the second, and so on. When the old values outnumber those new fields, the extra values are dropped. An empty old field takes no place, so later values move up.

Values are copied as they are, byte for byte. The new note type's sticky options play no part in a change: a field gets either a copied value or nothing.

| Old fields (values) | New fields | Result |
|---|---|---|
| Front `a`, Back `b` | Text, Back Extra | Text `a`, Back Extra `b` |
| Front `a`, Back `b` | Text, Back | Back `b` (by name), Text `a` |
| Front (empty), Back `b` | Text, Back Extra | Text `b`, Back Extra empty |
| Front `a`, Back `b`, Extra `c` | Text, Back Extra | Text `a`, Back Extra `b`; `c` is dropped |
| Front (empty), Extra `x` | Front, Back | Front `x` (Front's old value was empty, so it is filled by position) |
| Front `a`, Back `<br>` | Text, Back Extra | Text `a`, Back Extra `<br>` (`<br>` is not empty here) |
| Front `a`, Back `b` | front, back | front `a`, back `b` (by position: names differ in case) |
| Front `a`, Back `b` | Back, Front | Back `b`, Front `a` (both by name) |

A change into or between image occlusion note types maps the hidden Occlusion and Image fields as `editor/image-occlusion-notes` O10 says.

## A4. Sticky pins (R805, R255, R862)

- Each field of the Add screen shows a pin (`editor.field.<ord>.pin`) whose state is the field's `sticky` option in the stored note type (`notetypes/records` N2). Pins exist only in the Add screen.
- **Toggling one** (the pin, or F9 or ⌥⌘P while that field has keyboard focus): the note type is saved with that field's `sticky` flipped and every other value as stored. With focus in the tag editor or on a chooser, F9 and ⌥⌘P do nothing.
- **Toggling all** (⇧F9, wherever focus is in the Add screen): when at least one field of the note type is sticky, every field becomes not sticky; otherwise every field becomes sticky. "At least one" is read from the stored note type when the command runs. One save.
- The save is a plain note type save (`notetypes/records` N3, N4): no schema change, no note or card touched, the `notetypes` row stamped as N3 says, one undoable step "Update Note Type" (`conformance/undo-ops` V4). A refused or failed save leaves the pin as it was and shows the failure in one line.
- The pins of the screen follow the stored options: a change to the note type by another operation, an undo of a toggle and a sync update them (`engine/change-stream` S1, `notetypes` entry).
- A toggle changes what an add carries over (`notes/add-defaults` D6) and what counts as unchanged (D4) from that moment on.

## A5. Adding (R813, R245, R809 to R811, R814)

⌘↩ (R245) or the Add button:

1. **Once at a time.** While an add of this screen is running, a second ⌘↩ or press does nothing. In the exclusive phase (KTD17) Add is disabled with "Waiting for <operation>" (screen contract).
2. **Flush.** A held flush (A1), waited for, so the add takes every typed character (R813) and no input slips in while it runs.
3. **Selection.** The screen checks its selection (`notes/add-defaults` D5). When the check replaces the note type (the content mapped as A3.1) or the deck, the add stops before writing anything: the screen shows the replaced selection, the field check runs on the draft as it now is, `release` is sent, and the user presses Add again. A selection the check leaves valid goes on.
4. **Check.** The field check (`notes/field-check` K1) runs again on the flushed draft, and its state decides (K5):

   | State | What happens |
   |---|---|
   | `empty`, `notetype_not_cloze`, `field_not_cloze` | refused (R809, R810): nothing is written; the state's line shows under the fields and stays; focus moves to the first field; `release` is sent |
   | `missing_cloze` | the confirmation "Add without a cloze deletion?" (screen contract) asks first (R811). Cancel: nothing is written, `release` is sent. Add: go on |
   | `duplicate`, `normal` | go on (R808) |

   For an image occlusion note type, `editor/image-occlusion-notes` O7 step 1 runs first (R994: refused with "No occlusions created" when the Occlusion text the add would store holds no mask numbered 1 or more), then this table; `missing_cloze` then reaches the confirmation only when the note type's front does not use the Occlusion field with `cloze:`.
5. **The add.** `notes/note-records` A1 with the draft's note type, the flushed field values, the tag list, the selected deck and `update_add_defaults` true: one op, one undoable step "Add Note" (A5, R814), which also writes the keys of A4 (R801).
6. **On success**, in this order: audio that the editor started (R851) stops; a toast says how many cards the add created, the count the op returns (A2: at least 1), in the plural entry "1 card added" / "N cards added" with Undo (screen contract); the note's id goes to the top of the History (A6); the next draft replaces the draft as `notes/add-defaults` D6 says (sticky values kept, other fields empty, tags kept, focus in the first field; for an image occlusion note type with `editor/image-occlusion-notes` O7 step 4's additions: Occlusion and Image emptied whatever their sticky options, the picker shown again); `release` is sent; the field check runs on the next draft.
7. **On failure** (any error of the op): its message shows in one line above Add; the draft, its selection and the History are unchanged; `release` is sent.

Edit › Undo and ⌘Z undo the newest step of the collection history (`engine/undo` M3, M4), which right after an add is "Add Note": the note, its cards, the tags it registered and its config writes go (`notes/note-records` A5). After an add, focus is in the next draft's first field, whose own history is empty (the load cleared it, `editor/field-html` F5), so ⌘Z there reaches the collection undo; once the user has typed into a field, ⌘Z first undoes that typing (F5) and reaches the collection undo only when the field has nothing left to undo. The toast's Undo acts on the add's own step only: it undoes when the top step of the history is that add's step (its id is the add op's seq, `engine/op-model` O4), and otherwise (another step was recorded after it, such as a pin toggle's "Update Note Type", or the history was cleared) it is disabled and the toast closes. The next draft is not changed by the undo; the History row of that note turns into "(Note deleted)" (A6).

## A6. History (R815, R247)

- **Entries.** The ids of the notes the Add screen has added since the app launched, newest first, one per add. The list belongs to the open collection: it starts empty at launch and is emptied when the collection generation changes (`engine/change-stream` S3 `CollectionReplaced`) or another collection opens. Closing and reopening the Add screen keeps it. It has no length limit (R815).
- **Opening.** ⇧⌘H (R247) and the History button show the list (popover or pushed list, screen contract); with no entry it says "No notes added yet." (screen contract).
- **Row label.** The note's current first shown field (`editor/field-html` F1: the first field for every note type but image occlusion, whose occlusion and image fields are not shown, so its Header gives the label), read when the list is shown and while it shows, turned into a text line by `text/strip-html` H6 with media names kept. When that line is longer than 30 characters (extended grapheme clusters), the label is its first 30 followed by "…"; otherwise the whole line. A note whose line is empty shows an empty label.
- **Deleted notes.** A row whose note no longer exists shows "(Note deleted)" (`editor/editing-strings` E3) and is disabled. Existence is checked when the list is shown and kept current while it shows from change sets that remove the note (`engine/change-stream` S1, `notes` entry `Removed`); an undo of the add, a delete in Browse and a sync all count.
- **Choosing a row** opens Browse with the search `nid:<id>` (R1114: canonicalized and run as if committed; `search/names-and-ids`). The Add screen stays open with its draft.

Example: add `猫` then `<b>dog</b> and cat food for the week` then `<img src="cat.jpg">`. The list reads `cat.jpg`, `dog and cat food for the week`, `猫`, newest first; a first field `the quick brown fox jumps over the lazy dog` shows `the quick brown fox jumps over…`. After the `猫` note is deleted in Browse, its row reads "(Note deleted)" and is disabled.

## A7. Closing (R816, R259)

Esc, ⌘W, the close button, the iPhone swipe-down and a Mac quit go through one guard (R259):

1. a held flush (A1), waited for;
2. when the draft is unchanged since the last add (`notes/add-defaults` D4; tags are not looked at), the screen closes at once and keeps the draft for the next opening (`notes/add-defaults` D3);
3. otherwise "Discard current input?" asks (`editor/editing-strings` E3). Discard closes the screen and drops the draft: every field empty, no tags, no last added note; the next opening asks for the defaults (D3). Keep Editing sends `release` and leaves the screen as it was.

On iPhone the swipe-down is held back while the draft is not unchanged, and the same question shows. With suggestions shown in the tag editor, Esc first hides them (`editor/tag-editor` G4). A collection replacement goes through the same guard first (A7.1).

### A7.1 Before a collection replacement (KTD17)

A replacement (a one-way sync, a restore, a `.colpkg` import, the adoption of an iCloud collection) enters its exclusive phase only once open editors are closed (KTD17, `sync/one-way` O4). When the Add screen is open at that moment, the app runs A7's guard on it:

- **Closed** (the draft is unchanged, or the user chose Discard): the screen closes as A7 says, and the replacement goes on. From then until the phase ends, every Add entry point is disabled with "Waiting for <operation>" (KTD17, screen contract), so the Add screen cannot open again before `CollectionReplaced`, or before the phase ends without one (a cancel or a failure).
- **Keep Editing**: `release` is sent, the screen stays as it was, and the replacement does not start; nothing is written. The user can start it again later, which runs the guard again.
- **After `CollectionReplaced`**: the History is emptied (A6). Decided 2026-11-17: an unchanged draft that step 2 kept is dropped as well, since its note type and deck name the old collection's rows; the next opening asks for the defaults on the new collection (`notes/add-defaults` D3). A phase that ends without a replacement changes neither: the History and a kept draft stay, and Add is enabled again.

## A8. Create Copy and prefilled drafts (R817)

Create Copy (Browse, `browser/bulk-actions` B13; the review ⋯ menu, U183) and a draft prefilled by `guiAddCards` (U146) replace the draft as `notes/add-defaults` D3 says, including when they ask first. The copy's values are copied byte for byte, its tags as stored, its note type the note's, its deck the card's home deck; no field check of the source note is reused (S3 runs on the new draft). For an image occlusion note, the mask editor opens on the copy's shapes with the Rectangle tool (`editor/image-occlusion-notes` O5; `editor/mask-editor` M2).

## A9. Keys (R245, R246, R247, R255, R862, R259)

The keys belong to a context of their own, `editor.addScreen`: active while keyboard focus is inside the Add screen, that is, while the first responder is a view of the Mac panel (its fields, tag area, choosers, buttons), and while the sheet shows on iPad and iPhone. Focus elsewhere in the same window (Browse's table or its editor beside the Mac panel, the review behind it) leaves the context inactive, so there ⌘N is Add Note (R204), ⌘D Browse's Change Deck (`browser/command-map` M2) and ⌘W closes what R220 says. Each key is registered once with U158's registry:

| Command | Key | Other key | Context, and where focus must be | Anki on macOS |
|---|---|---|---|---|
| Add | ⌘↩ | | `editor.addScreen` | ⌘↩ (written Ctrl+Return and Ctrl+Enter) |
| Note type chooser | ⌘N | | `editor.addScreen` | ⌘N |
| Deck chooser | ⌘D | | `editor.addScreen` | ⌘D |
| History | ⇧⌘H | | `editor.addScreen` | ⇧⌘H (Ctrl+H on other systems) |
| Toggle the field's pin | F9 | ⌥⌘P | `editor.addScreen`, focus in a field (A4) | F9 |
| Toggle all pins | ⇧F9 | | `editor.addScreen` | ⇧F9 |
| Close | Esc | ⌘W | `editor.addScreen` | Esc, ⌘W |
| Add tag | ⇧⌘T | | `editor.addScreen` and `editor.any` (`editor/tag-editor` G9) | ⇧⌘T |

While `editor.addScreen` is active its keys take precedence over the window's other uses of the same keys; the registry test of `editor/formatting` F14 lists them for "Add panel over Browse" with focus in the panel and with focus in Browse's table, and fails on a duplicate (R200). While the mask editor shows, its own keys take precedence over these, and Esc and ⌘Z go to it only as `editor/mask-editor` M7.1 says.

## A10. Differences from Anki

Anki 26.09.3's add window differs as follows. None is visible to a scenario (the oracle runs the collection library, not the add window), so none has a `conformance/deviations.yaml` entry.

- **Choosers.** Anki lists note types in code-point order of their names; Meguri uses `unicase` order (A2). Anki leaves the default deck out whenever no card has `did` 1, even when it has subdecks, is not top-level, is the only deck, is the deck selected or has only filtered siblings; Meguri hides it only as `decks/decks` D12 hides it from the deck tree, with A2's two exceptions (R797). Anki's filter lowercases both texts and splits only at single spaces; Meguri's case folding and empty pieces give the same results for ordinary text. Anki's choosers also offer buttons to create a deck and to manage note types; Meguri's do not.
- **History.** Anki keeps the 15 newest notes of one add window and loses them when the window closes; Meguri keeps every note added since launch for the open collection (R815). Anki labels a row with all fields joined by ", ", cut at 30 code points with "...", inside `Edit "…"`; Meguri shows the first field only, counts grapheme clusters and keeps media names (A6). Anki checks deletions only when the menu opens.
- **Adding.** Anki shows its refusals in a warning dialog and stops audio of every source after an add; Meguri shows the refusal line in place (screen contract) and stops only audio the editor started. Anki's toast ends with a full stop ("1 card added."); Meguri's text is the screen contract's.
- **Closing.** Anki forgets the whole draft when its window closes, sticky values included; Meguri keeps an unchanged draft for the next opening (A7, `notes/add-defaults` D3).
- **Keys.** On macOS Anki's keys are the same as Meguri's: its Qt shortcuts and its editor write Ctrl, which is ⌘ there, and it closes its active window with ⌘W. ⌥⌘P is Meguri's addition.
- **Deck on a note type change.** Anki's add window selects the new note type's last deck (`notes/add-defaults` D2, when it gives one) on every note type change, whatever `addToCur` holds. Meguri does so only with `false` and keeps the selected deck with `true` (A3 step 3, `editor/config-keys` K4.2, R800). Anki's experimental editor, which A1 does not compare, checks `addToCur` as Meguri does.
- The rest of A3 (the field mapping, the focus rule, forgetting the last added note) and A4 match Anki, including name matching that counts case, positional filling that skips empty values and drops the extras, and toggle-all clearing when any field is sticky.

## A11. Meguri tests

`AddScreenModelTests`, `NotetypeSwitchMapperTests`, `AddCloseGuardTests` and `AddFlowUITests` (U195) cover what no oracle shows:

- every row of A3.1's table, A3's focus rule (a three-field type with focus in field 3 to a two-field type: focus in field 2) and the forgotten last added note (closing after a type change prompts for a non-empty field); after a change, the field list and its labels are the new note type's;
- A3 step 3 (`editor/config-keys` K4.2): with `addToCur` `false`, a change to a note type whose last deck exists selects that deck, and a change to one with no last deck or whose last deck was deleted keeps the selected deck; with `addToCur` `true` the selected deck stays;
- A2's deck list for each default-deck condition and both exceptions (selected, only filtered siblings), the filter examples, the note type order with names that differ in case, and a chooser refreshed by a deck added in Browse;
- A4: a pin toggled on and off, F9 in a field and in the tag editor, ⇧F9 with none, one and every field sticky, the pin following an undo of the toggle; after an add with the first and third of three fields pinned, the next draft holds those two values and an empty second field (`notes/add-defaults` D6);
- A5: with a note type of two fields, type into the first field, move focus to the second, type there, then within 100 ms press ⌘↩ (and, in a second case, the Add button) while focus is still in the second field: the added note holds both typed values, each in its own field (R813); during that add the screen sends no note-update op (only the add op of A5 step 5), and afterwards the collection history holds the one "Add Note" step above the steps it held before; a double ⌘↩ adds one note; each refusal state with its line and focus; the missing-cloze confirmation in both answers (confirming adds one card); the toast count for a note with one, two and three cards; ⌘Z after an add (focus in the first field, nothing typed) removes the note and the undo menu showed "Add Note"; ⌘Z after typing in the next draft undoes the typing first; the toast's Undo disabled after a pin toggle; a selection replaced at the add (its deck deleted in Browse) stops the add; a deck picked in the deck chooser just before ⌘↩ is the deck the card goes to; after a successful add with a note type and a deck that are neither default (a cloze kind and a deck other than the current one), both choosers still show that note type and deck, the next draft has that note type's fields, and D1 (`notes/add-defaults`) is not requested again; a failure keeps the draft;
- A6: newest first, the 30-character label and its ellipsis, a media-only first field, an image occlusion note labelled by its Header, "(Note deleted)" after a delete in Browse and after an undo, a row opening Browse on `nid:`, the list kept across close and reopen and emptied by a collection replacement;
- A7: AE35 (a sticky Sentence and tags `german::verb`: add, close at once without a prompt; type into a non-sticky field, close, "Discard current input?"); Discard then reopen gives the defaults; Keep Editing keeps the draft; A7.1: a one-way download confirmed while the draft holds typed input shows the prompt; after Discard the screen is closed and Add stays disabled until `CollectionReplaced`, after which the History is empty and the next opening asks for the defaults; Keep Editing keeps the draft and the download does not start; a cancelled phase leaves the History and a kept unchanged draft as they were;
- A9's keys through the menu commands; ⌘D and ⌘N with focus in the Mac panel and with focus in Browse's table beside it; and the registry's duplicate check.

AE34's duplicate state and its Show Duplicates search are `notes/field-check` K4 (scenario `notes/fields-check/duplicates`); the UI test checks that Add still adds the note.

## A12. Scenarios

Written in `conformance/scenarios/editor/add-flow/` and recorded at the current oracle pin (goldens in `conformance/goldens/editor/add-flow/`). They stay inactive until Meguri's adapters for their ops and observations land (U191, U195). They use only existing ops (`update_notetype` with `field_options`, `add_note`, `undo`, `undo_status`, `db_exec`, `notetype_structure`, `fields_check`, `set_config`, `set_current_deck`, `delete_decks`, `reopen`, the `card` observation, and the `add_defaults` and `notetype_last_deck` observations of `notes/add-defaults` D7). They show the collection half of A4 and A5, and the defaults an add leaves behind (`editor/config-keys` K4.1, K4.2; `notes/add-defaults` D2, D6); the UI half is A11's.

| Scenario | Covers |
|---|---|
| `sticky-toggle` | A4: one pin toggled on is a plain note type save (the `notetypes` row stamped, `col.scm` unchanged) recorded as "Update Note Type"; undo puts the option back; toggle-all on (every field sticky in one save, one step) and off (every field cleared in one save) |
| `add-count` | A5 step 6: the number of cards an add creates for a one-template note, a two-template note with both sides filled, one with an empty second side, a cloze note with clozes 1, 2 and 3, and a cloze note with none (the fallback card); each add one "Add Note" step |
| `sticky-pin-each-way` | A4: one field's pin toggled on and later off again, each toggle saving only that field's sticky option (a second pinned field stays on when the first is turned off); an add with pins on changes no option; the options persist across reopen |
| `add-leaves-other-notes` | A5 steps 4 and 5: a draft with every field empty is in the `empty` state; an add stores both field values in order and puts its card in the chosen deck as one "Add Note" step; a note and card already in the collection, set to a synced usn, keep their fields, mod and usn after the clock moves on |
| `selected-context` | A5 step 5 and `notes/add-defaults` D6: an add with a note type and a deck that are neither default (a cloze kind and a non-default deck) puts its card in that deck and its note under that note type. Afterwards the defaults are that note type and deck with `addToCur` `false`, and also with `true` once `curDeck` names that deck. With `true` and `curDeck` unchanged, the current deck's last note type stays. The other note type's last deck is unchanged. The selection kept on screen after the add is A11's (A5 bullet) |
| `notetype-deck-memory` | `editor/config-keys` K4.1 and K4.2 with `addToCur` `false`, using real adds only: each note type remembers the deck it was last added to. A later add of another note type leaves it as it was, it survives a reopen, and the note type's next add replaces it. The starting defaults restore a nondefault deck. Each card goes to the deck its add was given, whatever the defaults are |
| `deck-notetype-memory` | `editor/config-keys` K4.1 with `addToCur` `true`, using real adds only: the deck stays the current deck, and the note type is that deck's last note type even when `curModel` names another one. This holds for two decks and across a reopen. Then the contrast with `false` |
| `last-deck-fallback` | `editor/config-keys` K4.1 and `notes/add-defaults` D2 with `addToCur` `false` and a nondefault `curDeck`: a note type with no last deck gets the current deck. So does a note type whose last deck was removed by the deck deletion operation. That removal leaves the `_nt_<id>_lastDeck` key in place, and D2 reads it as none |
