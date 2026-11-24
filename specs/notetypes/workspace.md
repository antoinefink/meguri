# Note Types workspace (round 9)

Spec: notetypes/workspace
Requirements: R915 to R917, R919 to R921, R923 to R927, R929 to R938, R940; supersedes in part R915, R919, R920, R925, R929, R931, R932, R938, R940 (the Browse and Note Types plan §5 lists the notes)
Anki: none for the layout (Meguri design, decided by Antoine 2026-11-20, round 9: 1B 2B 3C 4B 5A 6B 7B 8B); the stored data and every save keep `notetypes/records`, `notetypes/structure-changes`, `notetypes/validation` and `render/*`
Scenarios: none; tests named in W16
Status: Approved 2026-11-20 (R915). Approved by Antoine in chat (round 9, decided 2026-11-20, `docs/design/decisions.md` the two `noteTypes` rows), after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`). The lines that carry the marker "changed 2026-11-21" with "implementation review", in any letter case or punctuation, are in force: the readings U267 (PR #60) and U268 (PR #67) built, decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/notetypes-reads-spec-record-20261121.md`). The line marked "changed 2026-11-23" in W13 records Flip's undo as PR #128 built it and N22.3 states it, decided by Claude (coordinator) under the same permission (`docs/design/decisions.md`, the `noteTypes` row of 2026-11-22 that begins "iPad Cards and Fields tabs"), recorded here after independent review. Every other section is in force and unchanged.

What the Note Types section shows and does: the list, the Fields tab, the Cards tab with the template editor and its live preview, the Card Text row over the styling, live issues, creating and duplicating, and the read-only touch layouts. Saving is `notetypes/draft-save`. Layout is `docs/design/native-interface-contract.md` N22. Where `notetypes/editor-actions` differs, this file wins for Meguri's screen; its sections that state Anki-matching results (A2's order, A6's copied texts, A10's sort-field tracking, A11, A14, A15) still hold.

## W1. Terms

- **Draft**: one per note type, holding every unsaved change to its fields, card types, templates, styling and names (`notetypes/draft-save` D1).
- **Card type**: a template (`notetypes/records` N2). Meguri says "card type" everywhere in the interface.
- **Notes with content** of a field: the notes of the type whose value of that field is not empty by `text/field-emptiness` E1.
- **Used On** of a field: the card types whose front or back refers to it (`render/template-syntax` T11.1: replacement keys and section keys, comments excluded), by the draft's texts.

## W2. The list

- Every note type with its note count (`notetypes/editor-actions` A2: order, and names first then counts). A filter field above narrows the list by simple case-folded substring of the name.
- A note type whose draft differs from its base shows "Edited" after its name.
- The + button opens New Note Type… (W11). The row's context menu: Duplicate (⌘D while the list has focus), Rename…, Restore to Default… (stock kinds only, `notetypes/restore-to-default`; its question is R914's, stating the losses with counts when the default has fewer fields or card types, D7), Delete… (W12).

## W3. The Fields tab

- A table, one row per draft field in order: a drag handle; **Name**; **Sort** (one radio per row: exactly one is on, `notetypes/editor-actions` A10 sort field); **Used On** (card type names joined with ", "; "Both" is not used: two names are listed; "Not Used" in the secondary colour when none); **Notes** (notes with content, W1, as a number with a bar of its share of the type's notes).
- The Notes column is computed by the core's `field_content_counts(notetype)` (W3.1) in the background; it is blank until known and is not recomputed while the draft changes (a new field shows 0).
- Actions: drag a row to reorder; double click (Return) on a name renames it in place (`notetypes/validation` V5 on Return; a refusal shows under the row and keeps the old name); + adds a field named "Field <n>" (the first free n) after the last, in rename mode; − or ⌫ marks the selected field deleted: the row is crossed out and stays in the draft list until Save; ⌘Z (the editor's own undo, W13) or Restore in the row's context menu takes it back. Deleting is disabled, with the reason as help text, for the last remaining field and for a protected field (`notetypes/structure-changes` S1.2 items 2 and 3).
- Renaming a field rewrites every template that names it, at save (`notetypes/field-rename` F3); it is not a loss and asks nothing.
- Sorting the table by Used On or Notes (a header click) sorts the view only; the field order is the drag order.
- Nothing on this tab writes before Save (`notetypes/draft-save`).

### W3.1 Notes with content (core read)

`field_content_counts(notetype id)` reads one snapshot and returns, for each stored field ordinal, the number of notes of the type whose value at that ordinal is not empty (E1), and the type's note count. Budget: 300 ms for 50,000 notes of the type; it runs off the main thread and is cancelled when the view leaves.

## W4. Field options shown

The inspector beside the table holds, for the selected field:

| Option | Stored (`notetypes/records` N2) | Why it is shown |
|---|---|---|
| Name | `name` | renaming is the most common edit |
| Collapsed in Editor, with the help line "Closed in Add Note until you open it" | `collapsed` | keeps a field closed in Add Note and Edit Note until it is opened (`editor/field-html` F3) |
| Right-to-Left | `rtl` | needed to type Arabic or Hebrew fields (`editor/field-html` F2) |
| Placeholder, with the help line "Grey text shown while the field is empty" | `description` | the hint shown in an empty field (`editor/field-html` F1) |

- Sort Field is the table's Sort column (W3).
- Not shown in this version: the editing font and size (`font_name`, `font_size`: editor settings, decided 2026-11-20), "Use HTML editor by default" (`plain_text`) and "Exclude from unqualified searches" (`exclude_from_search`) (decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day ("Carry on here and in general with your best jugment")). The core keeps honouring a stored `exclude_from_search` in search (`search/text-matching` T7). Their stored values are kept exactly: a draft carries every field option of the base unchanged (`notetypes/structure-changes` S1.1), and a new field takes N5.2's defaults.
- These per-field options are editor and search settings stored in the note type. None of them changes how a card is rendered in review; card appearance is the templates and the styling (W9, W10).

## W5. Card types (the Cards tab's list)

- A list of the draft's card types in order, each with the number of cards that exist for it (by base ordinal; 0 for a new one) and a red dot when an issue (W7) names it. + at the trailing end of the list's title bar; a More button on each row, shown on the row under the pointer and on the selected row on the Mac and on every row on iPad, opens the context menu's commands; there is no − (changed 2026-11-24, Antoine's decision in chat, `docs/design/decisions.md` the `noteTypes.templates` row of that date; before, + and − sat under the list); double click or Return renames in place (the template-name rule of `notetypes/records` N4); dragging reorders.
- **Add** (+) duplicates the selected card type as `notetypes/editor-actions` A6 does (its name `Card <n>`, A6's texts), in rename mode. Its help says which: `Duplicate “<name>”`. It asks nothing now; the Save sheet says how many cards it creates (`notetypes/draft-save` D2).
- **Delete** (the More and context menus, ⌫) crosses the card type out in the draft; it asks nothing now (A7's question moves to the Save sheet, D2). Disabled, with the reason, while one card type remains.
- Context menu: Rename (↩), Duplicate (⌘D while the card types list has focus), Deck for New Cards… (A11, with N20.1's deck search; standard note types only), Move Up (⌥⌘↑), Move Down (⌥⌘↓), Delete (⌘⌫).
- Cloze and Image Occlusion note types have one card type: Add, Duplicate and Delete are disabled with the reason, and the preview gains a Cloze Number menu (W9).
- Browser Appearance (A12) is not offered in this version; its stored values are kept.
- Under the list, the **Fields** list: every draft field; the ones the selected card type does not use are grey. Double click or drag inserts `{{<name>}}` at the caret (A14's text).

## W6. The template editor

- Three parts: Front, Back, Styling (⌘1, ⌘2, ⌘3; R929). While Styling is selected on Mac, the part picker presents “Shared by All Card Types” through its tooltip and accessibility hint. On iPad and the unfolded Duo, the editor shows the same text beside the picker. The preview keeps showing the selected card type.
- The text is the stored template, byte for byte: Meguri never reformats it (R930, plain quotes, no autocorrection, Tab indents, ⌘/ comments the line with `<!-- -->` in Front and Back and `/* */` in Styling, system find and replace ⌘F).
- **Colouring** from `render/template-syntax` T12's nodes: field replacements one colour, conditional section tags a second, comments and HTML tags the system's code colours; CSS properties and values in Styling, each colour value with a swatch that opens the system colour picker and rewrites only that value's characters.
- **Completion**: typing `{{` lists the draft's field names, then the special fields `FrontSide`, `Tags`, `Deck`, `Subdeck`, `Card`, `CardFlag`, `CardID`, `Type` (`render/special-fields` P1); after a filter prefix (`hint:`, `type:`, `cloze:`, `furigana:`, `text:`) the field names again. Accepting inserts the name and `}}`. Typing `{{#` or `{{^` and accepting a name also inserts the matching `{{/<name>}}` after the caret.
- **Insert menu** (and ⌃Space), each item inserting at the caret: Field ▸ (`{{<name>}}`), Special Field ▸ (`{{<name>}}`), Show Only If Field Has Content ▸ (`{{#<name>}}` … `{{/<name>}}`), Show Only If Field Is Empty ▸ (`{{^<name>}}` … `{{/<name>}}`), Hint ▸ (`{{hint:<name>}}`), Type-in Answer ▸ (`{{type:<name>}}`), Text Only ▸ (`{{text:<name>}}`), Furigana ▸ (`{{furigana:<name>}}`), Answer Divider (`<hr id=answer>`), and in Styling Dark Appearance Rule (`.card.nightMode {\n}` at the end). With text selected, the two conditionals wrap it.
- Flip (A15) is in the Back part's context menu.

## W7. Live issues

Shown under the editor for every card type of the draft, recomputed about 300 ms after typing pauses:

| Issue | Colour | Text | Fix (one click) |
|---|---|---|---|
| each failure of `notetypes/validation` V4 (V3's kinds) | red; blocks Save | "<card type> · <Front or Back>, line <n>." then the failure in words: "No field is named “<x>”.", "{{#<x>}} is opened but never closed.", "The front repeats “<card type>”.", "The front shows no field.", "A cloze note type needs a cloze field on the front and the back." | unknown field with a near name (W7.1): "Use “<name>”"; unclosed section: "Add {{/<x>}}" at the end of the part |
| no card for some notes (W8) | yellow; does not block | "<card type> · The front is blank for <k> notes." with the reason "Their <field> is empty, so the front is blank." when one field decides it (changed 2026-11-21, implementation review: one field, W8) | "Show Notes" (opens Browse on them, `browser/workspace` V15) |

- Issues are computed on the draft as it would be saved: field renames are applied to the templates first (`notetypes/field-rename` F3, as `notetypes/validation` V3 runs "after field renames rewrote the references"), so renaming `Front` to `Prompt` does not report `{{Front}}` as unknown. Changed 2026-11-21 (implementation review): W8's count also applies the rest of S4's rewrite and S5's remap (W8).
- Clicking an issue moves the caret to its line. Red issues name the line from T12's spans; an issue without a line names the part.
- The bottom line reads "Fix the <n> red issues to save." while any is red; Save is disabled.

### W7.1 Near names

Changed 2026-11-21 (implementation review): for an unknown field name x, the suggested name is the draft's field whose name equals x under simple case folding; else the draft's field at the smallest Levenshtein distance of at most 2, counted in Unicode scalar values between x and the field name, both simply case folded (ties: the first in field order); none otherwise. The suggestion shows the field's stored name.

## W8. No card for some notes

- For each card type of the draft (standard note types), `notes_without_card(notetype, draft templates)` counts the notes of the type for which that card type's front would not render something (`render/template-syntax` T11.2, strict form) with the note's non-empty names (`notes/card-generation` G2), and returns all their note ids (8 bytes each; Show Notes opens Browse on all of them). It reads one snapshot, off the main thread, cancellable, recomputed when a front changes (debounced as W7; changed 2026-11-21, implementation review: also when the draft's fields change, since they change what the save would store).
- Changed 2026-11-21 (implementation review): the fronts and values are those the save would store: after S4's rewrite and S5's remap of the draft.
- Changed 2026-11-21 (implementation review): a front that does not parse (`render/template-syntax` T6) gives no note a card (`notes/card-generation` G3): every note of the type is counted, and no field is named.
- Changed 2026-11-21 (implementation review): the reason line names a field f when the front, which must parse, refers (T11.1) to f and to no other name that can be non-empty for a note: a field, `Type`, `Deck`, `Subdeck`, `Card`, `CardFlag`, `CardID` or `Tags`. The front must also render something (T11.2, strict) when f alone is non-empty, and f must be empty in every counted note. Otherwise only the count shows.
- This is a warning about the draft, not a check of saved cards: existing cards are never removed by a save (`notetypes/structure-changes` S7), and the count includes notes that keep an older card.
- Budget: 1 s for 50,000 notes of the type.

## W9. The preview

- Beside the code on Mac, iPad and the Duo opened. It renders the draft (`render/preview` V2 to V11, unsaved texts included) for one note: the note Note Types was opened from (from Browse or Edit Note), else the type's newest note (largest note id); a note menu with Previous and Next steps through the type's notes in id order ("<i> of <k>").
- Question or Answer (a segmented control); Light or Dark (a sun and moon segmented control, the card's own appearance only, R940); a Cloze Number menu for cloze types (R931). No device or size picker (decision 2026-11-20; R940's platform classes are superseded).
- It redraws about 300 ms after the last keystroke, then within R1835's 100 ms, and keeps the insertion point in the code. Audio plays only from its button.
- A note type without notes previews with "Fill Empty Fields" on (R939's sample values).

## W10. Styling and the Card Text row (6B)

- The Styling part is the note type's CSS, saved as typed (`notetypes/records` N3; Meguri never rewrites it).
- Above the CSS, one **Card Text** row of five controls: Font (a menu of system font families and "System"), Size (points), Alignment (leading, centre, trailing), Text Colour, Background. The row is shown only when the CSS has exactly one **plain `.card` rule** (W10.1); otherwise it is absent, not disabled.
- Each control shows the value of its declaration in that rule (W10.2), or an empty state when the rule has none. Changing a control edits only that one declaration's value characters. When the rule has no such declaration, one declaration is added at the end of the rule's block. Changed 2026-11-21 (implementation review): when the block's last item, a declaration or not (an invalid declaration or an at-rule statement included, a nested rule with its block excluded), ends with neither its own `;` nor a `{}` block, a `;` is first put right after its last token other than white space and comments (the only other byte added); when that token is a string broken by a line break or a lone `\`, nothing is written. Then, when the closing `}` stands alone on its line, the line `  <property>: <value>;` is inserted before that line, otherwise ` <property>: <value>; ` is inserted just before `}`. Nothing else in the text changes: comments, order, spacing and every other rule stay byte for byte. Changed 2026-11-21 (implementation review): after an edit the result is read again; when the rule would not show the written value as its property's last declaration, nothing is written.
- The edited line is highlighted briefly in the code, and the preview updates (W9).
- The Card Text row is not shown on the iPhone or the folded Duo (templates are read-only there, W13).

### W10.1 The plain `.card` rule

Reading the CSS as CSS Syntax Level 3 tokens: a top-level qualified rule (not inside `@media`, `@supports` or any at-rule) whose prelude, with comments removed and White_Space trimmed, is exactly `.card`. "Exactly one" counts such rules. Changed 2026-11-21 (implementation review): the prelude is compared as text: `.ca/**/rd` counts as `.card` and `.c\61 rd` does not. The row is also absent when the CSS is not well formed: when its end falls inside an unclosed block, string, `url(` or comment, or when a `}` closes no block. Changed 2026-11-21 (implementation review): a `}` that does not close the innermost open block, for example one inside `(`, `[` or a function, is a `}` that closes no block. A `)` or `]` that closes nothing is an ordinary token.

### W10.2 Declarations

| Control | Property | Value written |
|---|---|---|
| Font | `font-family` | Changed 2026-11-21 (implementation review): the family name is written unquoted only when it is one CSS identifier made of letters, digits, `-`, `_` and non-ASCII characters that starts with a letter, `_`, non-ASCII, or `-` followed by one of those or `-`, and is not `inherit`, `initial`, `unset`, `revert`, `revert-layer`, `default`, `system-ui`, `-apple-system` or `BlinkMacSystemFont` (ASCII case-insensitive). Otherwise it is written as a double-quoted string, with `"` and `\` escaped by `\`. So a name with a space is quoted (`"Hiragino Sans"`). A name that is empty or holds a control character or a line separator is refused. "System" writes `system-ui, -apple-system, sans-serif`. Read: the control shows the first family of the list; it shows "System" when that family is `system-ui`, `-apple-system` or `BlinkMacSystemFont`; a generic family alone (`serif`, `sans-serif`, `monospace`) is shown by its name. Choosing a family replaces the whole list's value |
| Size | `font-size` | `<n>px`. Read: the control shows points-as-pixels: a `px` value as its number, a `pt` value × 4/3 rounded to a whole number; any other unit or keyword shows no value. A change writes `px` |
| Alignment | `text-align` | `left`, `center`, `right`: the control's leading and trailing are left and right in a left-to-right interface, right and left in a right-to-left one. Read: `start` and `end` show as leading and trailing; `justify` and other values show no segment selected |
| Text Colour | `color` | `#rrggbb` lower case (`#rrggbbaa` when the chosen colour is not opaque). Read: hex forms of 3, 4, 6 or 8 digits, `rgb()`, `rgba()`, `hsl()`, `hsla()` and CSS named colours show their colour; any other value (`var()`, `currentColor`, functions of later levels) shows the well empty, and choosing a colour replaces it. Changed 2026-11-21 (implementation review): an `rgb()`, `rgba()`, `hsl()` or `hsla()` value with a `none` component shows the well empty too, here and in Background |
| Background | `background-color` | `#rrggbb`. Changed 2026-11-21 (implementation review): when the rule has a `background-color`, its last one is read and edited. Otherwise, when the rule's last `background` shorthand has a value that is exactly one colour (one hex, named colour, or `rgb()`/`rgba()`/`hsl()`/`hsla()` function), the control shows that colour and edits only that value. Any other shorthand shows the well empty and is never edited: choosing a colour adds a `background-color` declaration (W10) |

The last declaration of a property in the rule is the one read and edited. Changed 2026-11-21 (implementation review): this holds whatever CSS would apply: an earlier `!important` declaration or a later shorthand (`font`, `background`) does not change which declaration is read and edited. `!important` stays where it is.

## W11. Creating and duplicating

- **New Note Type…** (the + button, File › New Note Type…, no key): a sheet listing the six stock kinds (`notetypes/stock-kinds` K1) with one line each (W15), the first selected, and a Name field following the chosen kind's name until edited (K1's name; when taken, "<name> 2", the first free number). Cancel, Create (disabled while the name is blank, or equal under unicase to an existing name). Create adds the kind (K8) and selects the new type. It asks nothing and is not a structure change (R950).
- **Duplicate** (⌘D, context menu): clones the note type at once (`notetypes/records` N7) named "<name> Copy" (then "<name> Copy 2"…), selects it and puts its name in rename mode. One undo step "Add Note Type". It replaces R916's "Clone: <name>" rows and "<name> copy".

## W12. Rename and Delete

- **Rename…**: in place in the list (Return commits; `notetypes/records` N8). Not a structure change.
- **Delete…**: disabled for the only note type; otherwise asks "Delete “<name>”?" with "Its <n> notes and their <c> cards will be deleted." (or "It has no notes." with 0), Delete (destructive) and Cancel (R918's question, reworded). With a ready Meguri Sync binding it adds `notetypes/draft-save` D4's sync paragraph and the button "Delete and Upload". The op is `notetypes/structure-changes` S12 through the save flow of D5 and D6. Changed 2026-11-21 (implementation review): the question's counts are the removal's own count (`notetypes/structure-changes` S12), read on one snapshot; its progress counts cards and has no total.

## W13. iPhone, the folded Duo, and the editor's own undo

- iPhone and the Duo folded or held tall (decision 2026-11-20, 8B): the list, then a note type's page with Card Types and Fields sections. Fields can be added, renamed, reordered (Edit), deleted and given W4's options, and the sort field set; the same draft and Save (D5, D6). A card type opens on its Preview (W9 without the note menu's size; Light and Dark in its More menu), with Front, Back and Styling as read-only coloured code that can be selected and copied, and one line: "To change this card type, unfold your iPhone Duo or use Meguri on an iPad or a Mac." On a plain iPhone the last words are "use Meguri on an iPad or a Mac." No Card Text row.
- Unfolding the Duo turns the same page into the full editor with the same draft.
- **The editor's own undo** (⌘Z while Note Types has focus and no text view owns the key) steps back through draft changes (a field marked deleted, a reorder, a rename, a card type added). It is not the collection's undo: the draft is not saved data. While Note Types has focus and its draft has steps, Edit › Undo is titled with the draft step ("Undo Delete Field", "Undo Move Field", "Undo Rename Field", "Undo Add Card Type", "Undo Delete Card Type", "Undo Move Card Type", "Undo Rename Card Type", "Undo Change Sort Field", "Undo Change Option") and undoes it; with no draft step it is the collection's Undo (round 6's one Undo, in Edit). After Save, the collection's step is "Undo Update Note Type" (D7); text edits in the code keep the text view's own undo. Flip (`notetypes/editor-actions` A15) is one step of the Back's own text view undo, "Undo Flip", that puts back both texts; a Front edited after the flip is kept, and only the Back is put back (changed 2026-11-23, recording PR #128 and `docs/design/native-interface-contract.md` N22.3).

## W14. Ways in and back

- Mac and iPad: the Note Types sidebar row (below Browse; with `rectangle.stack` as its system symbol, like Today and Browse; decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day ("Carry on here and in general with your best jugment")) and View › Note Types ⇧⌘T. iPhone and the Duo: Settings › Collection › Note Types, and Browse's Filters menu (N22.1).
- Edit Note Type… (Browse's Note menu, a note type's sidebar row in Browse, the inspector's Type menu, the status line's column set menu; the Add Note and Edit Note forms' Type menu): selects the Note Types section on that note type, its Cards tab, with the note it came from in the preview, and shows a Back button in the toolbar that returns to the screen it came from with its state.
- From review: More › Edit Note, then its Type menu's Edit Note Type…; it leaves the review (saved; it resumes).

## W15. Strings

| Text | Where |
|---|---|
| Note Types, Fields, Cards, Filter, Edited, Save, Save…, Revert, %1$lld notes · %2$lld cards | the section, tabs, list, toolbar and its subtitle |
| Undo Delete Field, Undo Move Field, Undo Rename Field, Undo Add Card Type, Undo Delete Card Type, Undo Move Card Type, Undo Rename Card Type, Undo Change Sort Field, Undo Change Option | W13's draft undo |
| Name, Sort, Used On, Notes, Not Used, Field %lld | Fields table |
| Collapsed in Editor, Closed in Add Note until you open it, Right-to-Left, Placeholder, Grey text shown while the field is empty | field inspector |
| Card Types, Card %lld, Deck for New Cards…, Move Up, Move Down, Duplicate, Rename, Delete, Restore | card types list |
| Front, Back, Styling, Shared by All Card Types, Insert, Field, Special Field, Show Only If Field Has Content, Show Only If Field Is Empty, Hint, Type-in Answer, Text Only, Furigana, Answer Divider, Dark Appearance Rule | editor |
| Question, Answer, Cloze Number, %1$lld of %2$lld, Fill Empty Fields | preview |
| Card Text, Font, System, Size, Alignment, Text Colour, Background | W10 |
| W7's issue texts ("The front is blank for %lld notes." among them), Use “%@”, Add {{/%@}}, Show Notes, Fix the %lld red issues to save., No Issues | W7 |
| New Note Type, Create, %@ Copy, and K1's six kinds' lines: "A front and a back. One card." (Basic), "Two cards: front to back, and back to front." (Basic (and reversed card)), "The reverse card only for notes where you ask for it." (Basic (optional reversed card)), "You type the answer and Meguri compares." (Basic (type in the answer)), "Hide parts of a sentence. One card per hidden part." (Cloze), "Hide parts of an image. One card per hidden part." (Image Occlusion) | W11; kind names stay Anki's (K1) |
| Delete “%@”?, Its %lld notes and their %lld cards will be deleted., It has no notes. | W12 |
| To change this card type, unfold your iPhone Duo or use Meguri on an iPad or a Mac., To change this card type, use Meguri on an iPad or a Mac. | W13 |

## W16. Tests

- Core: W3.1 counts (empty by E1, media-only values, a note with fewer values); W8 counts against `notes/card-generation` on a fixture (conditionals, negated sections, cloze excluded); W10.1 and W10.2 on CSS with comments, two `.card` rules, `@media` blocks, shorthand backgrounds, `!important`, and a byte-for-byte check that only the edited value changed; W7.1's near names.
- Swift model tests: W3's crossing out and restore, W5's disabled reasons, W6's completion lists and inserted texts, W13's draft undo.
- VM UI: the Mac Cards tab with a red and a yellow issue, the Fields tab, the iPhone read-only card type.
