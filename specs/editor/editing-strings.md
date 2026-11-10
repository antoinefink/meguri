# Note editing strings

Spec: editor/editing-strings
Requirements: R797, R798, R799, R804, R807, R815, R816, R817, R826, R827, R828, R890, R916, R918, R919, R922, R925, R926, R928, R929, R931, R932, R933, R934, R935, R936, R937, R939, R944, R945, R946, R948, R1000
Anki: 26.09.3 for E3 to E9 (the English of Anki desktop's add, editor, note type, card template, field, change note type and image occlusion screens, with E11's differences); none for the keys and E10's language rule
Scenarios: none (Meguri's tests, E12)

The interface text of the note editing screens that must be Anki's own words: labels users look up, questions whose wording a Product Contract requirement quotes, and two names that end up stored in the collection. U160 (E4), U190 to U197 and U200 to U207 take each such string only from this file (KTD1). Every other string of these screens is Meguri's own (R1959) and lives in the String Catalog with no Anki source; the screen contracts in `docs/design/screen-inventory.yaml` place every string and own appearance.

## E1. Keys, arguments and plurals

- Every string is an entry of the String Catalog of the feature that shows it, with the stable key given here: the Editor String Catalog (`app/Packages/MeguriKit/Sources/Features/Editor/Resources/Localizable.xcstrings`, the Editor feature's) for E3 and E5; the Settings String Catalog (`Features/Settings/Resources/Localizable.xcstrings`) for E4; the Note Types String Catalog (`Features/NoteTypes/Resources/Localizable.xcstrings`) for E6, E7, E8 and E9's Change Note Type rows; the Image Occlusion String Catalog (`Features/ImageOcclusion/Resources/Localizable.xcstrings`) for E9's mode names. Each key is in exactly one of them, and only that feature's views read it, from their own target's bundle. Browse and the review screen read none of these keys (E3, E9). The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped; a changed meaning gets a new key.
- `%lld` is an integer argument, `%@` a text argument; with two arguments they are numbered (`%1$@`, `%2$lld`). A plural entry has the catalog's plural variations on its integer argument: English uses **one** for exactly 1 and **other** for every other value, 0 included; other languages follow their CLDR rules. Every translation shows the integer in every form.
- Text arguments (note type, template and field names, stock kind names) are inserted as stored, never translated or changed (R194).
- Decided 2026-11-10, as `review/reviewer-strings` S1 and `sync/status-strings` T1 decide: quotation marks and apostrophes are typographic (‘ ’ “ ”) and three dots are one ellipsis character (…), the platform's typography; Anki writes ASCII `'` and `...`. A label that opens another step before anything changes ends with "…". Every other difference from Anki's English is listed in E11.

## E2. Strings listed elsewhere

These strings are fixed by their own specs; this file does not repeat them. The third column says whose wording each is.

| Strings | Where | Wording |
|---|---|---|
| undo step names: Add Note (R814), Update Note (R821), Update Note Type, Change Note Type, Image Occlusion | `specs/conformance/undo-ops.md` V4; the Undo and Redo labels are `review/reviewer-strings` S5 | Anki's |
| stock note type names, field names and template names (R909, R913, R916) | `notetypes/stock-kinds` K1 and its field and template tables | Anki's |
| a clone's default name `<name> copy` (R916) | `notetypes/records` N7 | Anki's |
| the preview's fill-empty value `(<field name>)`, the sample cloze sentence (R939) and the simulated type-in pair `example` and `sample` (R941) | `render/preview` V6, V7, V9 | Anki's |
| "Indent/unindent currently only works with lists." (R845) | `editor/formatting` F19 | Anki's |
| Fields…, Cards…, Shrink Images, Close HTML Tags (R847, R848) and the other toolbar and menu labels | `editor/formatting` F19 | Meguri's; F19 gives Anki's label beside each |
| the image controls and label, including Restore Original Size and ` (Original: w×h)` (R878 to R880), and the field context menu (R881) | `editor/paste` P21 | Meguri's; P21 gives Anki's label beside each |

## E3. Add screen (U195; R797, R807, R815 to R817)

| Key | English | Shown when |
|---|---|---|
| `add.type` | Type | label of the note type chooser row (R797) |
| `add.deck` | Deck | label of the deck chooser row (R797) |
| `add.showDuplicates` | Show Duplicates | the button of the duplicate state (R807; `notes/field-check` K4) |
| `add.history.noteDeleted` | (Note deleted) | in place of the first-field text of a History row whose note no longer exists; the row is disabled (R815) |
| `add.discard.title` | Discard current input? | closing with a draft that is not unchanged since the last add (R816, `notes/add-defaults` D4), and Create Copy over such a draft (`notes/add-defaults` D3) |
| `add.discard.discard` | Discard | the destructive choice of that question |
| `add.discard.keepEditing` | Keep Editing | the other choice; it keeps the draft |

- Create Copy (R817; `browser/bulk-actions` B13) is a command of Browse and of the review ⋯ menu, not of the Add screen, so it has no key here. Its words are R817's "Create Copy", fixed by that requirement; the unit that shows the command writes it in its own catalog: Browse's menu item (R1227) under `browser/browse-strings` W6, the review ⋯ menu's item under U183. Anki's reviewer item has the dots and its Browse item has none (E11); whether Meguri's command ends with "…" is decided under the spec of the unit that shows it.

## E4. The `addToCur` setting (R798, R799, R804)

The two values of the Settings control that U160 places on its Editing page (R140; `editor/config-keys` K4). The control is a choice of the two, with no other label of Anki's.

| Key | English | Value |
|---|---|---|
| `settings.addToCur.currentDeck` | When adding, default to current deck | `true` |
| `settings.addToCur.byNotetype` | Change deck depending on note type | `false` |

## E5. Tag editor (U193; R890)

| Key | English | Shown when |
|---|---|---|
| `editor.tags.add` | Add tag | the control that appends and focuses a new empty tag (⇧⌘T, R890); also its accessibility label |

## E6. Note Types list (U200; R916, R918, R919)

| Key | English | Shown when |
|---|---|---|
| `notetypes.add.stock` | `Add: %@` | a choice of the Add sheet for a stock kind; `%@` is its `notetypes/stock-kinds` K1 name (R916) |
| `notetypes.add.clone` | `Clone: %@` | a choice of the Add sheet for an existing note type; `%@` is its name (R916) |
| `notetypes.delete.used` | Delete this note type and all its cards? | the delete question for a note type with at least one note (R918; `notetypes/structure-changes` S12) |
| `notetypes.delete.unused` | Delete this unused note type? | the same for a note type with no note (R918) |
| `notetypes.restoreToDefault` | Restore to Default… | the card template editor's Options item (R932), Anki's label; also the row action (R919), which Anki's Note Types list does not have, so it reuses the same label; its question is Meguri's (R914) |

- The row actions Fields… and Cards… (R919) are Meguri's own labels, entries of the Note Types String Catalog that U200 writes with the same English as `editor/formatting` F19's Fields… and Cards…; F19 gives no keys, and this file lists none for them (E11).
- Delete is Anki's question in either form: which one shows depends only on whether the note type has a note when the question is asked.
- Delete is disabled while the note type is the only one (R918; `notetypes/structure-changes` S12), so no refusal message is shown (E11).

## E7. Card template editor and preview (U200, U202; R929, R931 to R937, R939)

| Key | English | Shown when |
|---|---|---|
| `templates.part.front` | Front Template | the first part of the part picker (⌘1, R929); also the code editor's accessibility label while it shows that part |
| `templates.part.back` | Back Template | the second part (⌘2) |
| `templates.part.styling` | Styling | the third part (⌘3) |
| `notetypes.numberedName` | `%1$lld: %2$@` | a row of the card type picker: the template's position from 1 and its name (R931); also a row of the field list (E8) |
| `templates.clozeCard` | `Card %lld` | a row of the card picker that a cloze note type shows in place of the card type picker (R931): one per cloze number of the previewed note, the argument being that number |
| `templates.affectsNotes` | one: `Changes below will affect the %lld note that uses this card type.` other: `Changes below will affect the %lld notes that use this card type.` | under the card type picker of a standard note type, or under the card picker of a cloze one (R931); the argument is the number of notes of the note type, the same for every card type of it (U29's note count of the note type, not a count per template) |
| `templates.addCardType` | Add Card Type… | Options item of a standard note type (R932) |
| `templates.addCardType.confirm` | one: `This will create %lld card. Proceed?` other: `This will create %lld cards. Proceed?` | after Add Card Type when the note type has at least one note; the argument is the number of notes of the note type (R932) |
| `templates.removeCardType` | Remove Card Type… | Options item of a standard note type (R932); disabled while the note type has one template (R933) |
| `templates.removeCardType.confirm` | one: `Delete the ‘%1$@’ card type, and its %2$lld card?` other: `Delete the ‘%1$@’ card type, and its %2$lld cards?` | the remove question (R933); `%1$@` is the template's name, `%2$lld` the number of its cards from `notetypes/structure-changes` S11, pluralized on that number |
| `templates.renameCardType` | Rename Card Type… | Options item of a standard note type (R932) |
| `templates.repositionCardType` | Reposition Card Type… | Options item of a standard note type (R932) |
| `templates.deckOverride` | Deck Override… | Options item of a standard note type (R932, R934) |
| `templates.copyInfo` | Copy Info | Options item (R932), for either kind of note type |
| `templates.browserAppearance` | Browser Appearance… | Options item (R932, R935), for either kind of note type |
| `templates.insertField` | Insert Field | the control that inserts a field reference at the cursor (R936), for either kind of note type |
| `templates.flip` | Flip | the Back Template's Flip control of a standard note type (R936) |
| `templates.fillEmpty` | Fill Empty Fields | the preview's toggle (R939, U202) |
| `notetypes.discardChanges.title` | Discard changes? | leaving the card template editor or the field editor with changes not saved (R937, R928) |
| `notetypes.discardChanges.discard` | Discard | the destructive choice |
| `notetypes.discardChanges.keepEditing` | Keep Editing | the other choice; the editor stays open with its draft |

- A cloze note type has one template, and the editors never add, remove or move it (`notetypes/structure-changes` S6). As in Anki, its Options menu therefore has only Restore to Default…, Copy Info and Browser Appearance…: no Add, Remove, Rename or Reposition Card Type and no Deck Override. Its Back Template has no Flip.
- The remove question is one entry: R933's line about notes left with no card is Meguri's own and shows as a separate line under it, never joined to it.
- The card count of `templates.removeCardType.confirm` is pluralized as a whole sentence: "Delete the ‘Card 2’ card type, and its 1 card?", "… and its 0 cards?".

## E8. Field editor (U201; R826 to R828, R922, R925, R926)

| Key | English | Shown when |
|---|---|---|
| `notetypes.numberedName` | (E7) | a row of the field list: the field's position from 1 and its name (R920) |
| `fields.lastField` | Notes require at least one field. | Delete Field… on the note type's only field (R922); also the message when the op refuses a draft for having no field (`notetypes/structure-changes` S1.2) |
| `fields.delete.confirm` | one: `Delete field from %lld note?` other: `Delete field from %lld notes?` | the delete question for a field that is not protected (R922); the argument is the number of notes of the note type |
| `fields.option.rtl` | Reverse text direction (RTL) | the option's toggle (R925, R828) |
| `fields.option.htmlEditor` | Use HTML editor by default | the option's toggle (R925, R827) |
| `fields.option.collapse` | Collapse by default | the option's toggle (R925, R826) |
| `fields.option.excludeFromSearch` | Exclude from unqualified searches (slower) | the option's toggle (R925) |
| `fields.option.description` | Description | the label of the description text field (R925) |
| `fields.option.sortField` | Sort by this field in the browser | the sort field toggle (R926), which R926 quotes; also its accessibility label |

- The question is asked with 0 too: "Delete field from 0 notes?".
- A protected field's refusal (R922) is Meguri's own line, not Anki's message (E11).

## E9. Change Note Type and image occlusion (U203, U205, U206; R944 to R948, R1000)

| Key | English | Shown when |
|---|---|---|
| `changeNotetype.title` | Change Note Type | the sheet's title (U203) |
| `changeNotetype.nothing` | (Nothing) | the picker choice that gives a new field no old field, or a new template no old template (R945, R946; `notetypes/change-notetype` M2, M3) |
| `changeNotetype.notesUpdated` | one: `%lld note updated.` other: `%lld notes updated.` | the toast after the change commits (R948); the argument is the op's result, the number of notes in the selection (`notetypes/change-notetype` M9) |
| `io.mode.hideAll` | Hide All, Guess One | the mask editor's mode menu, the default (R1000, R1007; the modes of `render/image-occlusion-format`) |
| `io.mode.hideOne` | Hide One, Guess One | the other mode (R1000) |

- Browse's menu item and toolbar action that open the sheet (⇧⌘M, R944, R1227) are Browse's command names, written under `browser/browse-strings` W6 with R944's words "Change Note Type"; this file has no key for them. The sheet's own text above, the field and template pickers' `(Nothing)` and the toast are U203's, in the Note Types String Catalog.
- The toast is this sheet's own entry. Browse's bulk result messages, such as Edit Field's "N notes updated" (R1211), are `browser/browse-strings` W6's and are separate keys, even where the English matches.

## E10. Names written into the collection

A name the screen writes into the collection is data shared with other clients, not interface text: it is the English given here in every language and is not a String Catalog entry (as `notetypes/stock-kinds` K1).

- **A new card type** (Add Card Type, R932): named `Card N`, a space between, N in ASCII digits. N starts at the number of templates the draft has before the add, plus one, and goes up by one while a template of the draft already has exactly that name (the same characters: a template named `card 3` does not block `Card 3`). The draft's save then normalizes every template name as `notetypes/records` N4 steps 4 and 5 say. Examples: templates `Card 1`, `Card 2` give `Card 3`; templates `Card 1`, `Card 3` give `Card 4`; templates `Front`, `card 2` give `Card 3`; templates `Front`, `card 3` give `Card 3`, which the save stores as `Card 3+` (N4 step 5: equal under unicase to the earlier `card 3`).
- **A clone's default name** is `notetypes/records` N7's `<name> copy`, prefilled in the Add sheet's Name field and stored as the user leaves it.

## E11. Differences from Anki

- Typography (E1): Anki's `...` is "…" in every label of E6 and E7 (Restore to Default…, Add Card Type…, Remove Card Type…, Rename Card Type…, Reposition Card Type…, Deck Override…, Browser Appearance…), and the remove question quotes the name with ‘ ’ where Anki writes `'`.
- Restore to Default… is Anki's card template editor item, whose `...` is "…" (E1). Anki's Note Types list has no Restore to Default; Meguri's row action (R919) is an addition with the same label.
- Anki's Deck Override item appends its state directly after the dots, "(on)" or "(off)" ("Deck Override...(off)"); Meguri's item has no state suffix, and its sheet shows the current deck (R934).
- Anki's Note Types list buttons read "Fields" and "Cards"; Meguri's row actions read Fields… and Cards… (E6), as the editor's (`editor/formatting` F19).
- R932's "Copy Info" is Anki's "Copy Info to Clipboard", shortened by the requirement; R936's "Insert Field" is the button Anki labels "Add Field". Both keep the requirement's words.
- R926's "Sort by this field in the browser" is Anki's field editor label exactly (E8).
- The Change Note Type sheet's title has no dots, as Anki's window title. Browse's command that opens it is Browse's (E9).
- Create Copy: Anki's reviewer menu reads "Create Copy..." and its Browse menu "Create Copy". This file lists neither; Meguri's commands are Browse's and U183's (E3).
- Anki's refusals that Meguri does not show: "Please add another note type first." (deleting the last note type, R918) and "At least one card type is required." (removing the last template, R933), because Meguri disables those actions instead (E6, E7); and "This is a required field, and can not be deleted." (a protected field, R922), for which Meguri shows its own line (E8). "Notes require at least one field." (R922) is kept, since R922 quotes it.
- Anki's card type picker row also lists the fields each side uses ("1: Card 1: Front -> Back"); R931 keeps the position and the name only.
- R939 quotes "Fill empty fields"; the toggle is a control label and keeps Anki's title case, "Fill Empty Fields".
- The discard questions of the card template editor and the field editor offer Discard and Keep Editing. Anki's card template editor offers Save, Discard and Cancel, and its field editor a yes or no question; Meguri's choices are the Add screen's (R816).
- Anki takes its `addToCur`, Add, Clone, Card N and question texts through its translations; Meguri ships English only (R195) and keeps E10's names English in every later language.

## E12. Tests

The oracle runs the collection library, not Anki's UI, so these strings are checked by Meguri's tests, each in a file its unit lists:

| Test file (unit) | Covers |
|---|---|
| `AddScreenModelTests`, `AddCloseGuardTests` (U195) | E3: the chooser labels, Show Duplicates on the duplicate state, `(Note deleted)` on a History row after the note's deletion, the discard question with its two choices from close and from Create Copy |
| `TagEditorModelTests` (U193) | E5 |
| `NoteTypesUITests` (U200) | E6: the Add sheet's choices for the six stock kinds and for each note type, both delete questions |
| `CardTemplateEditorModelTests` (U200) | E7: the three parts, the picker rows of a standard and of a cloze note type, the affects line at 1 and 5 notes for both kinds, the Options items and Flip of each kind, the add question only when notes exist, the remove question at 0, 1 and 3 cards with its separate cardless line, the discard question; E10's `Card N` examples |
| `TemplatePreviewModelTests` (U202) | E7's Fill Empty Fields toggle |
| `FieldsEditorModelTests` (U201) | E8: the rows, the last-field message, the delete question at 0, 1 and 12 notes, the six option labels |
| `ChangeNotetypeModelTests` (U203) | E9's title, `(Nothing)` in both pickers, the toast at 1 and 50 notes |
| `OcclusionDocumentTests` (U205) | E9's two mode names, Hide All, Guess One first |
