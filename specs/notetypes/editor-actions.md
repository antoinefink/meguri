# Note type editor actions

Spec: notetypes/editor-actions
Requirements: R915, R916, R917, R918 (the selection after a delete), R920, R922, R925, R926, R932, R933, R934, R935, R936, R938 (the card type an editor opens at), R944, R945, R946, R947 (the pickers and the loss list)
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/manage/
Status: Approved 2026-11-20 (R915) for the lines of A3, A6, A7, A10, A12 that begin "Changed 2026-11-20 (round 9)". Approved by Antoine in chat (round 9, decided 2026-11-20), after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`). Every other section is in force and unchanged.

What the actions of the Note Types list, the card template editor, the field editor and the Change Note Type sheet do to their draft, where that must match Anki: list order and counts, the Add sheet's choices and names, the text Flip, Add Card Type, Insert Field and Copy Info produce, the values Browser Appearance and Deck Override store, and how Reposition moves an item. Each action here changes only the editor's draft (`notetypes/structure-changes` S1.1, `notetypes/records` N3) until Save, except where a section says otherwise. Layout, controls and wording are the screen contracts' (`docs/design/screen-inventory.yaml`: `noteTypes.list`, `noteTypes.templates`, `noteTypes.fields`, `noteTypes.changeNoteType`); Anki's own words are `editor/editing-strings`. Validation is `notetypes/validation`, saves `notetypes/records` and `notetypes/structure-changes`, the preview `render/preview`.

Terms. **Trimmed** means with every character that has the Unicode White_Space property, and every character U+001C to U+001F, removed from both ends (Anki's editors trim with that set). **Blank** means empty once trimmed. A field name typed in the field editor is not judged by these two terms but by `notetypes/validation` V5 (A10). The **selected template** is the card type the template editor's picker shows; the **draft texts** are the editor's current front, back and styling, unsaved edits included. Positions shown to the user count from 1; ordinals from 0.

## A1. Only the draft

- Every action of A3 to A16 that edits a note type changes the draft and nothing in the collection, except the Add sheet (A3) and Rename (A4), which save at once as Anki's list does, and Deck Override's deck creation (A11), which happens when the draft is saved.
- An action the user cancels, or whose input A3 to A16 ignore, leaves the draft as it was.
- **Changed.** The editor counts as changed (the discard question, `editor/editing-strings` E7) exactly when its draft differs from the note type the draft was loaded from (the base whose token the draft carries, `notetypes/structure-changes` S1.1), not from the note type as now stored: a sync or another window that changes the stored note type while the editor is open does not make an untouched draft changed (its Save then meets `Stale`, S1.2 item 1). Opening and closing a sheet without a change, or an edit undone by hand, leaves the editor unchanged.
- Anki marks its editors changed per action instead: closing the Deck Override dialog always marks it, whatever was typed; Browser Appearance marks it on OK only, not on Cancel; Rename Card Type never marks it, so a card type rename alone gets no discard question there; Add Field, a successful Flip, Reposition (a field even at its current position, A9), any text edit and every field option change mark it.

## A2. The Note Types list (R915, R916, R917, R918)

- One entry per `notetypes` row: its id, its stored name and its note count, the number of `notes` rows whose `mid` is that id. A note whose `mid` names no note type is counted nowhere.
- Order: by stored name under the `unicase` comparison (`text/unicode-primitives#U2`), which is the order of the `notetypes.name` column's collation; ascending id breaks a tie, which only a damaged collection can have (names are unique under unicase, `storage/schema-18`). Anki reads the list with one query ordered by that column. Example: with the stock kinds, `_under`, `alpha`, `Zeta`, `Äbc`, `Straße` and `Strasse2`, the order is `_under`, `alpha`, `Basic`, `Basic (and reversed card)`, `Basic (optional reversed card)`, `Basic (type in the answer)`, `Cloze`, `Image Occlusion`, `Straße`, `Strasse2`, `Zeta`, `Äbc` (`_` comes before letters once case is folded; `ß` folds to `ss`; `ä` is not a folded `a`). By code points it would be `Basic` … `Strasse2`, `Straße`, `Zeta`, `_under`, `alpha`, `Äbc`.
- **Core reads (Meguri contract).** The list needs two reads that the core answers, since only the core compares names under unicase exactly as the column does: the **ordered list**, every note type's id and stored name in this order; and the **note counts**, the count of each note type id (a note type without notes is 0). Each read uses one snapshot. Meguri shows the names from the first read at once and the counts when the second read ends (R915); a count belongs to the entry with the same id, and an entry whose id the counts read does not hold (a note type added between the reads) shows no count until the next refresh. Anki reads names and counts in one query. U29's existing by-id name read is not this read and keeps its order; A3's clone choices and A16's target picker use the ordered list.
- Anki shows a row as `<name> [<count>]`, the count as "1 note" or "N notes". Meguri's row text is its own (`noteTypes.list`).
- **Selection** (Mac and iPad, where rows are selectable):
  - on opening: the note type Browse's sidebar named (Manage Note Type, `browser/sidebar` S9.7); otherwise, and when it no longer exists, the first entry. Anki does the same, except that it selects nothing when the named note type no longer exists;
  - after an add, a clone or a rename: the list is read again and the entry with the affected id is selected (R916, R917), as in Anki;
  - after a delete (R918, `notetypes/structure-changes` S12): the entry that now holds the deleted entry's position, or the new last entry when the deleted entry was last (Meguri's choice). Anki reads the list again and keeps the row index, so it selects the same entry in the first case and nothing in the second; when its list was opened from Browse's sidebar, it instead tries the note type the sidebar named, and selects nothing when that was the one deleted.

## A3. Add (R916)

Changed 2026-11-20 (round 9): Meguri's screen offers New Note Type… with the six stock kinds and a direct Duplicate instead of the Add sheet's Clone rows (`notetypes/workspace` W11).

- **Choices**, in this order: one `Add: <name>` per stock kind, in `notetypes/stock-kinds` K1's order, with K1's name; then one `Clone: <name>` per note type of the collection. The first choice is selected.
- **Clone order.** Meguri lists the clones in A2's order. Anki sorts them by the stored names' code points, letter case included, so `Zeta` comes before `alpha` there; nothing depends on that order.
- **Name.** After the choice the user gives a name, prefilled with the choice's default: the stock kind's K1 name, or `<name> copy` (`notetypes/records` N7, the clone's source name as stored). Meguri disables Add while the name is blank once every `"` is removed. Anki ignores a blank name; a name made only of `"` and white space reaches its add and is refused there with an error (`invalid_notetype`, N4 step 1). Otherwise the name goes to the add as typed, not trimmed, and the add's normalization applies (`notetypes/records` N4 steps 1, 2 and 7: `"` removed, NFC, `+` appended while another note type has an equal name under unicase). So in Meguri "Add: Basic" with the default name in a collection that has `Basic` adds `Basic+`.
- **Anki's collision rename.** Anki's Add sheet saves through its Python layer, which first looks for another note type whose name equals the typed name under unicase and, when one exists, appends a hyphen and five lowercase hexadecimal characters derived from the current time (`notetypes/records` N4's last paragraph). So Anki adds `Basic-` followed by five such characters in that example, and the `+` of step 7 appears there only when the collision comes from `"` removal or NFC. Meguri follows N4 step 7 (R907; A17).
- **Effect.** A stock choice adds that stock kind with the name (`notetypes/stock-kinds` K8, `notetypes/records` N5); a clone copies the source note type as stored, not any open editor's draft (N7). Either is a checked save (`notetypes/validation` V1) and sets `curModel` (N5). The list then selects the new note type (A2).

## A4. Rename a note type (R917)

The name is prefilled with the stored name. Meguri disables Rename while the name is blank once every `"` is removed (Anki: as in A3). Otherwise the typed name, not trimmed, is saved by `notetypes/records` N8 (N4 steps 1, 2 and 7; a case-only rename keeps the new spelling). It is a checked save (V1). The renamed note type stays selected (A2). Anki's collision rename of A3 applies to its Rename too: renaming `alpha` to `Basic` while `Basic` exists gives `Basic+` in Meguri and `Basic-` plus five characters in Anki; a case-only rename collides with nothing in either.

## A5. Where the template editor and the field editor open (R938)

- **Template editor, the card type it opens at:**
  - opened from an editor that shows a card (the review screen's Edit panel; Browse's editor pane, whose card is `editor/edit-flows` W2's: the selected card in cards mode, the note's current card in notes mode): that card's ordinal. For a standard note type it selects the template at that position; when the ordinal is not below the template count (a card without a template, `notes/empty-cards` E1) Meguri selects the first template (Anki fails to open there). For a cloze note type it is the card picker's target (`render/preview` V1: the card's cloze number minus one), and V6's card list adds that number when the note's text lacks it;
  - opened from Browse's sidebar, Edit Template on a card type child (`browser/sidebar` S9.7): that template, previewing a new note with every value empty and fill-empty on, whether or not the note type has notes (S9.7, as Anki does);
  - opened from anywhere else (the Note Types list's Cards…, the Add screen, an editor without a card): the first template, or cloze card 1.
- Browse's sidebar note type item opens neither editor: its Manage Note Type opens the Note Types list with that note type selected (S9.7, A2).
- The template editor opens on Front Template (`editor/editing-strings` E7). Which note it previews, except for the sidebar case above, is `render/preview` V5.
- **Field editor:** opened from Browse's sidebar, Edit Fields on a field child (S9.7): that field is selected; from anywhere else, the first field.

## A6. Add Card Type (R932)

Changed 2026-11-20 (round 9): no question when adding; the Save sheet gives the cards created (`notetypes/draft-save` D4, D8). The new template's texts stay A6's.

Standard note types only (`editor/editing-strings` E7).

- When the note type has at least one note, the user is asked first (`templates.addCardType.confirm`, its count the note type's note count); declining changes nothing. With no note nothing is asked.
- The new template is a new one in the sense of `notetypes/records` N5.3 (no origin, `notetypes/structure-changes` S1.1; a fresh random id when saved) with:
  - name: `editor/editing-strings` E10's `Card N`;
  - front and back: the selected template's draft texts, copied exactly, unsaved edits included;
  - everything else empty: no browser front or back, no deck override, browser font name empty and size 0, even when the selected template has them.
- It is added after the last template and becomes the selected template. Saving it is a structural save (S2 "templates changed"); the cards it needs are generated then (S7).
- Its front equals the copied template's front, so unless that front contains the `{{Card}}` reference that V3 item 1 exempts, the live checks report `duplicate_front` at once (`notetypes/validation` V3, V4) and Save is refused until one of the two fronts is edited. Anki behaves the same: its save refuses the duplicate.

## A7. Remove Card Type (R933)

Changed 2026-11-20 (round 9): no question when removing; the card type is crossed out in the draft and the Save sheet gives S11's counts (`notetypes/draft-save` D4, D8).

- Disabled while the draft has one template (`editor/editing-strings` E7, E11).
- The question gives the selected template's card count and the notes left with no card from `notetypes/structure-changes` S11, asked with the base ordinals of the templates the draft already removes plus this template's origin. A template the draft added has no origin and no cards: its count is 0, and it adds no ordinal to the query. Anki counts the cards of the template's stored ordinal in the same way.
- Accepting removes the template from the draft. The selected position stays the same, or moves to the last template when the removed one was last.

## A8. Rename Card Type (R932)

The name is prefilled with the template's name. Every `"` is removed from the typed text; a result that is blank changes nothing (Meguri disables Rename). Otherwise the result, not trimmed, becomes the template's name in the draft (`notetypes/validation` V5's last paragraph). The save then makes names unique (`notetypes/records` N4 step 5). Not a schema change (R950).

## A9. Reposition (R920, R932)

The same rule moves a template (Reposition Card Type, standard note types only) and a field (Reposition, and drag reordering in the field editor). In the saved note type a move is a structure change (`notetypes/structure-changes` S2: fields changed or templates changed).

- **Input.** A position from 1 to N, N being the number of items in the draft's list, prefilled with the item's current position. A position outside 1 to N, or the current position, changes nothing in Meguri (Meguri's control admits only 1 to N). Anki ignores text that is not an integer and a position outside 1 to N; it ignores the current position for a card type only: for a field at its current position it asks its schema-change question and marks the editor changed, and the list stays the same.
- **Effect.** The item is taken out of the list and put back so that it is at the chosen position; the items between its old and new positions shift by one toward the gap. It is a move, never a swap:

  | List | Move | Result |
  |---|---|---|
  | A B C D | A to 3 | B C A D |
  | A B C D | D to 1 | D A B C |
  | A B C D | B to 4 | A C D B |
  | A B | B to 2 | unchanged |

- The moved item keeps its origin (`notetypes/structure-changes` S1.1) and becomes the selected item. A field move keeps the sort field on the same field (A10), whatever its new position.
- A drag that drops an item between two others is the same move, to the position it lands at.

## A10. Field editor actions (R920, R922, R925, R926)

Changed 2026-11-20 (round 9): Delete asks nothing; the field is crossed out and the Save sheet gives its content count (`notetypes/draft-save` D8). The options shown are `notetypes/workspace` W4: Collapsed in Editor, Right-to-Left and Placeholder; font, size, HTML by default and exclude from search are kept but not shown.

- **Add Field.** The typed name goes through `notetypes/validation` V5: when V5 step 2 finds it empty nothing happens, a refusal is explained, and an accepted name (V5 step 6's text) becomes a new field (N5.2 defaults: font `Arial`, size 20, every option off, description empty) after the last field, and it is selected.
- **Rename.** The name is prefilled with the field's draft name. V5 on the typed name, with the same outcomes as Add Field; the field keeps its origin (S1.1) and only its name changes in the draft. The save rewrites template references (`notetypes/field-rename` F3).
- **Delete.** Refused for the last field and for a protected field (`notetypes/structure-changes` S1.2 items 2 and 3); otherwise the question of `editor/editing-strings` E8 with the note type's note count, asked even at 0. Accepting removes the field from the draft. The field that now holds the deleted field's position is selected, or the new last field when the deleted one was last (Meguri's choice); Anki selects the first field.
- **Sort field** (R926). Turning the toggle on makes the selected field the sort field; it cannot be turned off, since exactly one field is the sort field. The editor tracks the chosen field through later moves and submits its final position as the draft's sort field (S1.1); when the user chose none, the draft submits none and the save keeps the base's (S3 step 4). When the sort field is deleted and no other is chosen, the editor shows as sort field the field the save will use: `notetypes/records` N4 step 6 applied to the draft's current list (the base's sort index, lowered to the last position when past the end).
- **Anki's sort index.** Anki stores the selected row's current position as the sort index, but its save reads that index as the original ordinal of a field. After fields were moved or added in the same session, setting the sort field there can select another field, or clamp to the last one; Meguri records the field (A17).
- **Options** (R925; `notetypes/records` N2): the editor font name and size, RTL, HTML editor by default, collapse by default, exclude from unqualified searches, and the description. The description is stored as typed, not trimmed. The size control admits 5 to 300 (Anki's range). A stored value the control cannot show (a size outside 5 to 300, from another client, or a font the device lacks) is kept unless the user changes that control: the size control shows the stored size moved into 5 to 300, and the font control shows the stored name. Anki writes the clamped size back when the field's options are left.

## A11. Deck Override (R934)

Standard note types only; it acts on the selected template's `target_deck_id` (`notetypes/records` N2).

- **Shown.** The current override as the deck's full name (`::` between levels) when it is set and the deck exists, or as the typed name when the draft holds a name whose deck is not created yet; empty when it is not set. When it names a deck that no longer exists, Meguri shows the field empty with its own line saying so, and keeps the stored id unless the user edits the field. Anki shows the Default deck's name there, and closing its dialog then sets the override to the Default deck.
- **Blank.** A blank text clears the override: `target_deck_id` 0.
- **A name** sets the override to the deck that `decks/decks` D5 gives for that name: an existing deck by D2's equality (whatever its spelling or kind, a filtered deck included), or a deck D5 creates, with its missing ancestors. The template stores that deck's id, so renaming the deck later keeps the override.
- **Checked on confirm.** Meguri checks a name that names no deck against D3's refusals (`filtered_parent`, `too_deep`) on the current decks when the sheet is confirmed: a refused name keeps the sheet open with Meguri's own line under the field and changes nothing. An accepted name that names no deck stays in the draft as a name; the id is set when the deck is created.
- **When a missing deck is created.** Anki creates it when its override dialog closes, before the note type is saved and even if its edits are then discarded. Meguri creates it when the draft is saved, in this order:
  1. the draft's live checks (`notetypes/validation` V4): a draft they refuse is not submitted and creates no deck;
  2. one D5 "Add Deck" step per template whose override is still a name, in template order (a second template naming the same new deck finds the first one's deck, D5 step 2);
  3. the note type save (`notetypes/records` N3, `notetypes/structure-changes` S1), with the ids.
  When a D5 step refuses (the decks changed since the sheet was confirmed, for example another device made the parent a filtered deck), the save stops there: the note type is not saved, the draft keeps its texts and names, decks an earlier step created stay, and the editor shows the refusal on that template's override (Meguri's wording) so the user can change it. A deck created in step 2 also stays when the note type save that follows is refused (`Stale`, `SafetyBackupFailed`). Discarding the draft creates nothing.
- **Filtered decks.** Anki stores a filtered deck's id when the name is one; card placement then never uses it (`notes/deck-placement-and-position` D2, D3: not usable). Meguri stores it the same way; its suggestions list only normal decks.
- The override is never a schema change (R950) and changes no existing card; it places only cards created later (D2, D3).

## A12. Browser Appearance (R935)

Changed 2026-11-20 (round 9): not offered in this version (`notetypes/workspace` W5); stored values are kept.

Either kind of note type; it acts on the selected template's browser front and back (`q_format_browser`, `a_format_browser`; schema-11 `bqfmt`, `bafmt`) and browser font (`browser_font_name`, `browser_font_size`; `bfont`, `bsize`). How Browse uses them is `browser/columns` and `render/card-render-pipeline` K5 step 1.

- **Formats.** Two one-line texts, prefilled with the stored formats. On confirm each format the user edited is stored trimmed; an empty result means none (Browse then uses the template's own front or back). A format the user did not edit is kept as stored, so a format of only white space from another client stays and Browse uses it (K5 step 1). Anki stores both formats trimmed on every confirm, edited or not.
- **Font.** One toggle governs the font name and size together. It starts on when the stored font name is not empty. The family shows the stored name, or `Arial` when it is empty; the size control admits 6 to 99 and shows the stored size moved into that range, or 12 when it is 0. Both show whatever the toggle's state; the user can change them while it is on.
- **Writing the pair.** The name and the size are written together or not at all. When the user changed none of the toggle, the family and the size, both stored values stay as they are, so a stored size with an empty name (another client's) and a stored size above 99 both survive a confirm that touched neither. When the user changed any of the three, the confirm writes both: with the toggle on, the shown family and the shown size; with it off, an empty name and size 0. Example: stored name `X` with size 0; the toggle starts on and the size shows 12; the user changes only the family to `Y`: `Y` and 12 are stored. Anki writes both values, or clears both, on every confirm.
- Cancel changes nothing. Not a schema change (R950).

## A13. Copy Info (R932)

Copies the selected template's draft front and back and the draft styling to the pasteboard as plain text, in this exact form, each line ending with LF (U+000A), the last included. The draft is not changed.

````
## Front Template
```html
<front>
```
## Back Template
```html
<back>
```
## Styling
```css
<styling>
```
````

- `<front>`, `<back>` and `<styling>` are the three texts as the draft holds them, their own line breaks kept, with one change: going through each text from its start, every run of three consecutive backticks (U+0060) is replaced by the six characters `` \`\`\` `` (a backslash before each backtick); a replaced run is not looked at again. So ```` ``` ```` gives `` \`\`\` ``, four backticks give `` \`\`\`` `` and six give two replaced runs; one or two backticks stay.
- An empty text gives an empty line between its fences.
- The headings and fence labels are this English text in every interface language (Anki writes them untranslated). Anki then shows "Copied to clipboard"; Meguri's feedback is its own (`noteTypes.templates`).
- For a cloze note type the template is its one template.

## A14. Insert Field (R936)

- **Choices** (Meguri's): the draft's fields in order, then the special names `FrontSide`, `Tags`, `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag` and `CardID` in that order (`render/special-fields` P1, without the card-number key `c<N>`, which is not a name). Anki offers the note type's fields only.
- **Alternate mode** is the shown part's state under `render/template-syntax` T2: its text, after White_Space at its start, begins with the alternate-delimiter directive. Its **lead** is that leading White_Space, the directive and every immediately repeated copy that T2 removes.
- **Effect** (Meguri): the reference to the chosen name goes in at the cursor of the shown part, replacing the selection: `{{` + the name + `}}`, or in alternate mode `<%` + the name + `%>`. Nothing else is added. When the cursor, or the start of the selection, lies inside or before the lead in alternate mode, the reference goes in right after the lead instead, and a selection is not replaced; so an insertion never ends alternate mode.
- **Names that cannot be referenced.** Field names cannot hold `{`, `}` or `:` (`notetypes/validation` V5, `notetypes/records` N4), so `{{` + name + `}}` always reads back as that name. In alternate mode a field whose name contains `%>` would end the tag early: it is shown disabled among the choices while the shown part is in alternate mode. The special names hold neither.
- Insert Field is unavailable while Styling is shown, as Anki hides its Add Field there.
- **Anki** instead appends, at the end of the shown front or back, on a line of its own, a `div` element holding `{{` + the name + `}}` whatever the delimiters, whose inline style sets a font family (default `Arial`) and a size in pixels (default 20, 6 to 200) chosen in its dialog. R936 asks for the cursor insertion, so Meguri does not reproduce this.

## A15. Flip (R936)

Standard note types only; it rewrites the selected template's draft front and back at once.

- **The separator** is the exact 14-character text `<hr id=answer>`. Other spellings (`<hr id="answer">`, `<HR id=answer>`, `<hr id=answer/>`) are not it.
- **Which one.** Among the separator's occurrences in the draft back, Flip uses the last one that has at least one character before it and at least one character after it. When there is none (no separator, or every separator at the very start or the very end of the back), Flip changes nothing and the editor says that Flip needs the line with text on both sides (Meguri's wording; R936). Anki shows its message in both cases.
- **The new front** is the back's text after that occurrence, trimmed.
- **The new back** is `{{FrontSide}}`, two LF, `<hr id=answer>`, two LF, then the old front exactly as it was.
- The back's text before the occurrence is dropped, whatever it holds. The new back always uses `{{` and `}}`: an old front that started with the alternate-delimiter directive ends up after other text, where the directive does not apply (`render/template-syntax` T2), and its tags become plain text. Flip writes what Anki's Flip writes in every case.
- Not a schema change. The save that follows is a front change (`notetypes/structure-changes` S2, S7), and the live checks (`notetypes/validation` V4) report what the new texts break, for example an empty front (`no_front_field`).

| Front | Back | Front after | Back after |
|---|---|---|---|
| `{{Front}}` | `{{FrontSide}}` LF LF `<hr id=answer>` LF LF `{{Back}}` | `{{Back}}` | `{{FrontSide}}` LF LF `<hr id=answer>` LF LF `{{Front}}` |
| `Q` | `x<hr id=answer>` LF ` {{Back}} ` LF | `{{Back}}` | `{{FrontSide}}` LF LF `<hr id=answer>` LF LF `Q` |
| `Q` | `A<hr id=answer>B<hr id=answer>C` | `C` | as above, ending `Q` |
| `Q` | `A<hr id=answer>B<hr id=answer>` | `B<hr id=answer>` | as above |
| `Q` | `{{FrontSide}}<hr id=answer>` U+0020 U+0020 | empty | as above |
| `Q` | `<hr id=answer>{{Back}}` | unchanged (nothing before) | unchanged |
| `Q` | `{{FrontSide}}<hr id=answer>` | unchanged (nothing after) | unchanged |
| `Q` | `{{FrontSide}}<hr id="answer">{{Back}}` | unchanged (no separator) | unchanged |

## A16. Change Note Type pickers (R944 to R947)

The sheet's data and rules are `notetypes/change-notetype` (M1 to M6); this section adds the pickers' behavior.

- **New note type.** Every note type, the old one included, in A2's order (A2's ordered list). The sheet opens on the old note type, whose default maps are the identity (M2, M3), so Save starts disabled (M7). Choosing another one runs the info query again (M5) and resets both maps to its defaults. Anki reads the names with an unordered query that SQLite answers in the name index's order, which is A2's; Meguri sorts explicitly.
- **Fields** (R945): any old field, or `(Nothing)`, for each new field; an old field may be chosen for several.
- **Card types** (R946): an old template may serve one new template only. Anki, when the user chooses an old template that another row has, sets that other row to `(Nothing)`. Meguri shows the choices other rows use as disabled (`noteTypes.changeNoteType`), so the user first sets the other row to `(Nothing)`. Either way every map the sheet submits satisfies M1.
- **Loss summary** (R947): the discarded fields and dropped templates in ascending old ordinal (M6). Anki lists their names joined by `, `, shows the first three followed by `... (+N)` (three ASCII full stops) for the rest, and expands on request; Meguri's presentation is its own.

## A17. Differences from Anki

The oracle runs Anki's collection library, not its desktop screens, so it observes none of these; Meguri's tests check them against this file (A19). None needs a `conformance/deviations.yaml` entry.

| Action | Meguri | Anki |
|---|---|---|
| Add's clones (A3) | A2's order | code point order of the names |
| A name colliding under unicase in Add or Rename (A3, A4) | `+` appended (N4 step 7, R907) | a hyphen and five hexadecimal characters appended |
| A name made only of `"` and white space (A3, A4) | Add or Rename disabled | refused by the add or save with an error |
| Selection after a note type delete (A2) | the entry now at its position, or the new last | the entry now at its position, or none; the sidebar's note type when opened from there |
| Selection after a field delete (A10) | the field now at its position, or the new last | the first field |
| A dangling deck override (A11) | shown empty and kept | shown as Default and set to Default on close |
| Creating an override's deck (A11) | at save, after the live checks, nothing on discard | when the dialog closes |
| Browser formats not edited (A12) | kept as stored | trimmed on every confirm |
| Browser font values (A12), field sizes (A10) | kept unless edited | rewritten or cleared on every confirm, sizes clamped |
| Insert Field (A14) | the reference at the cursor, special names offered | a styled `div` appended at the end, fields only |
| Card type picker (A16) | used choices disabled | a used choice moves |
| An editor opened on a card without a template (A5) | first template | does not open |
| A field moved to its current position (A9) | nothing | schema question, editor marked changed |
| Sort field after moves in the same session (A10) | the chosen field | the field whose original ordinal equals the row's position |
| A typed field name's ends (A10) | White_Space trimmed (`notetypes/validation` V5 step 1), so a name starting with U+001C keeps it | U+001C to U+001F trimmed as well |
| Changed state (A1) | draft differs from its base | marked per action, some dialogs on close, card type renames never |

## A18. Scenarios

- `notetypes/manage/list-order` (recorded, with its golden; inactive until `remove_notetype`'s Meguri adapter, U34): A2's order and counts through `db_exec`, on scenario-defined note types whose names order differently under unicase and under code points, a note type without notes, a case-only rename (the order holds) and a note type removal (its notes leave the counts). Its `db_exec` reads the table ordered by the name column, as Anki's list does, so it checks the column's collation and the counts in both executors; it does not exercise Meguri's ordered list read (A2's core reads), which A19's engine tests cover. Its ops are all in the DSL; it waits only for that adapter.
- Nothing else here is observable through the oracle: A3 to A16 are Anki's desktop screens.

## A19. Tests

Meguri's tests, in the files the units list:

| Test file (unit) | Covers |
|---|---|
| U200's engine and FFI tests of A2's core reads | A2 order (the example list), counts, a note type without notes, the tie by id, a note whose note type is missing |
| `NoteTypesUITests` (U200) | A2's display of names before counts and the selection on opening (from Browse's sidebar too), after add, clone, rename and delete (middle and last entry); A3's choices and their order, defaults, disabled blank and `"`-only names, `+` names; A4 |
| `CardTemplateEditorModelTests` (U200) | A1 changed state (a sheet closed without change, a change undone by hand, a stored note type changed elsewhere); A5 targets (a card without a template, notes mode, the sidebar's card type child included); A6's copied texts, empty values and the live `duplicate_front`; A7's counts for a moved and an added template and the selection after removal; A8; A9's table for templates; A11's shown value, blank, existing, filtered, refused and new names, deck creation only at save and after the live checks, a D5 refusal at save; A12's trimming of edited formats, kept unedited formats, the font pair and kept values; A14's insertion in both delimiter styles, with a selection, at the lead and the disabled `%>` names; A15's table |
| `CopyInfoFormatterTests` (U200) | A13's exact text, backtick runs and empty texts |
| `FieldNameValidatorTests` (U201) | A10's use of V5 for Add Field and Rename |
| `FieldsEditorModelTests` (U201) | A9's table for fields and drag, the current position ignored; A10's add position and defaults, rename prefill, delete at 0 notes and the selection after it, the sort toggle through moves and after the sort field's deletion, kept sizes |
| `ChangeNotetypeModelTests` (U203) | A16's order, the opening state, the reset on a new target, disabled template choices |
