# Browse workspace (round 8)

Spec: browser/workspace
Requirements: R1080, R1111, R1115 to R1117, R1132 to R1136, R1139 to R1145, R1152 to R1156, R1181 to R1187, R1190, R1193 to R1198, R1200 to R1207, R1212, R1221 to R1224, R1232, R1233, R1235 to R1240, R1241 to R1247, R1825 to R1832; supersedes in part R1131, R1134, R1135, R1157, R1184, R1193, R1212, R1222, R1226 to R1228, R1234 (the Browse and Note Types plan §5 lists the notes)
Anki: none (Meguri design, decided by Antoine 2026-11-20, round 8: 1A 2A 3A 4B 5D 6A 7A 8A 9A 10A 11A); the operations it calls keep their own specs and Anki behaviour
Scenarios: none; tests named in V18
Status: Under review (R1232; round 8, decided 2026-11-20, `docs/design/decisions.md` rows `browser` and `home.deckList`). Do not implement until approved.

The behaviour of Browse as round 8 decided it: columns and column sets, field columns, the deck and date cells, selection, the commands and when they ask, Find and Replace's preview and scope, Find Duplicates, the status line, the inspector's two tabs, the touch layouts' list, Select mode and swipes, the entry points, and the speed budgets. Layout and presentation are `docs/design/native-interface-contract.md` N21; the search field and the sidebar's meaning are `search/filter-tokens`. Where an older `browser/*` section differs, this file wins and that section carries a "Changed 2026-11-20" note.

## V1. Terms and state

- **Rows**: cards in Cards mode, notes in Notes mode (`search/query-model` Q4). **Results**: the rows of the current query (`browser/result-sets` handle). **Total**: the number of cards (Cards) or notes (Notes) in the collection.
- **Selection** and **current row**: `browser/result-sets` H12 and R1152; the current row drives the inspector.
- **Kept while the app runs** (device, never in the collection): the query, the mode's results, the selection, the current row and the scroll position. Leaving Browse for Today, a deck or review and coming back restores them; a relaunch starts as `search/filter-tokens` T14 says.

## V2. Column sets

### V2.1 Cards and Notes

- A Cards / Notes control (N21.3; ⌥⌘T toggles, R1226) switches the mode. It writes `browserTableShowNotesMode` (`browser/config-keys` K6.1), which syncs; Cards is the default (absent key).
- The selection follows (`browser/result-sets` H11, H12): a selected card selects its note; a selected note selects its cards. The query stays (`search/filter-tokens` T11).

### V2.2 Which set is shown

- The **standard set** is used for every list, except: when the query has a Note Type token with exactly one value, set to **is**, the table uses that note type's **own set**.
- Each set has one column list per mode. A note type has no own set until the user first changes the columns or the sort while that note type's set is in use: until then the standard set is used, live. The first such change writes the own set as a copy of the standard set with the change applied; from then on the two are separate.
- The status line's trailing menu names the set ("Standard Columns", "<note type name> Columns") and offers Use Standard Columns (shown in an own set: the note type's set is reset to a copy of the standard set) and Edit Note Type… (in an own set; it opens Note Types on that type, `notetypes/workspace` W14).

### V2.3 Columns

- A list holds, in order, any of: the 19 columns of `browser/columns` C2 by their keys, and **field columns** `fld:<field name>` (V3). The state column (V2.5) is always first and is not in the list.
- **Default standard set** (when nothing is stored): Cards `noteFld`, `template`, `deck`, `cardDue`, `cardMod`; Notes `noteFld`, `note`, `template` (which shows the card count in Notes, C8), `noteTags`, `noteCrt`. R1134's defaults are superseded.
- The header's context menu (Mac; touch and hold on iPad) lists every column with a check: the list's columns first in their order, then the others by label; then **Fields ›**, a submenu of field names grouped by note type (a field name held by several note types appears under each and adds the same column); then Size Columns to Fit. In an own set the menu instead leads with that note type's fields, in field order, the first 8 then "<N> More Fields ›", then Standard ›. Checking adds the column at the end; unchecking removes it; at least one column stays (R1133).
- Dragging a header reorders the list (R1135). Dragging a header edge sets a width; a double click on the edge fits it; the Sort Field column takes the remaining width. Widths are per device, per set and per mode (R1136, K11).

### V2.4 Where sets are stored

- One synced collection config key, `meguriBrowserColumnSets` (`storage/config-table` C8): a JSON object. Member `std` is the standard set; member `<note type id in decimal>` is that note type's own set. Each set is an object with members `cards` and `notes` (arrays of column keys, V2.3) and `sort` (V4.3). A missing member reads as its default (V2.3); a member of another shape reads as missing and is replaced at the next write of that set; members Meguri does not know are kept.
- A write changes only the set it concerns, through `storage/config-table` C4, and only on a user change (R1138): showing, hiding or moving a column, changing the sort, Use Standard Columns. Undo class: the current-deck selection's skip-undo class (R42, `engine/undo` M2; the alternative `browser/config-keys` K10.2 describes): a write records no undo step, keeps the undo steps and empties the redo history, so changing a column never removes "Undo Suspend Cards". This differs from K10.2's first row on purpose: these writes are frequent display choices.
- Meguri never reads or writes `activeCols`, `activeNoteCols`, `sortType`, `sortBackwards`, `noteSortType` or `browserNoteSortBackwards` from Browse: other Anki clients keep their own columns and sort (supersedes R1134, R1135, R1141 for Meguri's display; `browser/config-keys` K4 and K5 stay the definition of those keys).
- A set whose note type no longer exists is ignored and kept as it is (no op removes it; a later note type never reuses the id).

Example: `{"std":{"cards":["noteFld","template","deck","cardDue","cardMod"],"sort":{"cards":["noteFld",false]}},"1700000000001":{"notes":["fld:Front","fld:Back","fld:Extra","deck","cardDue"]}}`.

### V2.5 The state column

The first, narrow column of every row shows, in this order of precedence, one symbol: suspended `pause.circle`, buried `moon`, flagged (the flag's `flag.fill` in its colour, `tags/flags-and-marked` F1), else nothing. A suspended or buried row's text is secondary (grey). No row is tinted (R1147's tints are superseded; R1149's glyphs stand, without the marked glyph: marking is not offered, V8). The symbol's accessibility label is the state ("Suspended", "Buried", "Red Flag").

## V3. Field columns

- A field column `fld:f` shows, for a row whose note's note type has a field named f (exact name), that field's value as one line: HTML removed and white space collapsed as the Sort Field cell does (`browser/columns` C7.1), except media: a field whose value holds a `[sound:…]` reference shows a speaker symbol and the first sound's length (from the media file; blank until read), a click plays it; a field holding an image shows the first image as a thumbnail the row's height; never the reference text or HTML.
- A row whose note type has no field f shows an en dash in the secondary colour; an empty field shows nothing.
- In Cards mode a note's cards repeat its values.
- Cells are computed by the core with the other row cells (`browser/result-sets` H7): H7's column set is extended to accept `fld:` keys (Changed 2026-11-20 note in H7). A field name holding `:` can occur only in a damaged collection (`notetypes/records` N4 removes it); its key is still `fld:` + the stored name.
- The first column of the list stays in place while the table scrolls sideways.

## V4. Sorting

### V4.1 Columns

Clicking a sortable header sorts by it in its first direction (`browser/sorting` S1); clicking again reverses (S8, R1142). A field column is sortable in both modes; its first direction is ascending.

### V4.2 The field sort key

For `fld:f`, rows compare by, in order:

1. rows whose note type has field f before rows whose note type has none;
2. among those, non-empty values before empty ones (`text/field-emptiness` E1);
3. non-empty values: when both values, after V3's HTML removal and trimming, match `^-?[0-9]+(\.[0-9]+)?$` (ASCII digits; so `5.` and `.5` are texts), as binary64 numbers, numerically (equal numbers then by text); a number before a text; texts by the `unicase` comparison (`text/unicode-primitives` U2), then by code point;
4. then the S8 final key (card id or note id), so the order is total.

Reversing reverses the whole order (rows without the field come first). The key reads stored values, as the other keys do (`browser/sorting` S2). Budget: R1828 (V17).

### V4.3 Where the sort is stored

In the set's `sort` member: `{"cards": [<column key>, <descending>], "notes": [...]}`; absent means `noteFld` ascending (R1141's `noteCrt` fallback does not apply to Meguri's own key). A stored column that the list no longer shows still sorts; one that cannot sort in that mode (`browser/sorting` S1) falls back to `noteFld` ascending without a write.

## V5. Cells that changed

### V5.1 Deck

The deck's last name component, then, in the secondary colour, its parent path joined with " › " (no `::`). When the column is too narrow, the parent path is cut first, from its end, with "…"; then the name, from its end. Help tag (Mac) and accessibility label: the full path joined with " › ". A filtered card shows its filtered deck so, with "(" + its home deck's last component + ")" after, as `browser/columns` C7.4 says in words. Notes mode with cards in several decks: C8's text. R1131's middle truncation is superseded.

### V5.2 Dates

Every date cell (Due, Modified, Created) names the day in the device's locale and calendar: Today and Tomorrow by name, other days as day and short month name, with the year added only for another year. No time of day is shown. Sort order is not affected (V4 sorts stored values). The other texts of the Due cell (New #N, (filtered), learning) stay `browser/columns` C7.5's; no brackets mark a suspended or buried card, since V2.5's symbol does.

### V5.3 Numbers

Numbers and dates use tabular figures; numbers align at the trailing edge, text at the leading edge.

## V6. The Browse sidebar

- Rows at the top, always: Today (`calendar`) and Browse (`magnifyingglass`); each with its system symbol, deck rows without one (decision `home.deckList`, 2026-11-20). They navigate; they are not filters.
- With Browse selected, the rest of the sidebar lists, in this order, each section folding: Saved Searches (T13's built-ins, then the user's), Card State (New, Learning, Review, Suspended, Buried), Flags (No Flag, then flags 1 to 7 by shown name; folded by default), Decks (the deck tree, `decks/decks` D12, with the same expansion as Today's tree), Note Types (by name, `notetypes/editor-actions` A2 order; no children), Tags (the tag tree; folded by default).
- Each value row shows its count (`search/filter-tokens` T12.3). The Tags section, when it has more than 12 top-level tags, shows the 8 with the most notes (descendants counted), in tree order, then "Show All <N>…", which shows the whole tree (remembered on the device).
- The sidebar's search field (N20.1's field) filters the rows as `browser/sidebar` S6 does while Browse is selected; with Today selected it is the deck search of N20.1.
- Section folding is device state (K11). The synced `collapse*` keys are not read or written by this sidebar (superseded R1157's storage; they stay untouched).
- Clicks: `search/filter-tokens` T9.
- Context menus: a note type row: Edit Note Type… (`notetypes/workspace` W14); a user saved search: Rename…, Delete (T13); a deck row: Browse Only This Deck (same as a click) is not added; no other management action in this version (renaming, deleting and dragging decks and tags inside Browse wait; `browser/sidebar` S9 and S10 stay the reference for later).
- Replaces: R1157's sections and order, R1159 (Today section), R1162's root and Current deck rows, R1163's children, R1166 to R1180 for this version (later work).

## V7. Selection and the inspector

- Exactly one row selected: the inspector shows its note (V12, V13). In Notes mode the card shown is the note's card with the lowest template ordinal, then the lowest card id (the rule `docs/design/screen-inventory.yaml` `cardInfo.card` gives).
- Two or more rows: the inspector shows the selection summary and the actions (N21.5): the title "<n> Cards Selected" (Cards) or "<n> Notes Selected" (Notes); one line: the number of notes (Cards mode only, "<k> notes"), the deck when all selected cards share one home deck, else "<d> decks", and "<s> suspended" when s > 0, joined with " · "; then V8's commands for the selection as buttons, each with its key.
- No row selected: the inspector shows "No Selection" in its content-unavailable style.
- Select All (⌘A in the table; ⌥⌘A anywhere, R1153) selects every row of the results, including rows not fetched yet. Select Sibling Cards (Edit menu, Cards mode): selects every card of the selected cards' notes that the results hold; it adds no row the query does not find.
- After an action rows stay (R1155) and update; deleted notes' rows go (R1156, `browser/bulk-actions` B11's move of the current row).

## V8. Commands

### V8.1 The commands and their titles

Targets are `browser/bulk-actions` B1's (card commands act on every card of a selected note in Notes mode). n is the number of target cards or notes as the title says. Titles take the singular when n is 1.

| Command | Title | Shown when | Key (Mac) | Operation |
|---|---|---|---|---|
| Change Deck… | Change Deck… | always | ⌘D | `browser/bulk-actions` B7 (the sheet: N20.1's deck search with its create row, `decks/decks` D18) |
| Add Tags… | Add Tags… | always | ⇧⌘A | B9 Add |
| Remove Tags… | Remove Tags… | always; its field suggests only tags the target notes have | ⌥⇧⌘A | B9 Remove |
| Flag | Flag ▸ with No Flag and the seven flags by shown name, each with its colour | always | ⌘1 to ⌘7; No Flag ⌘0 | `tags/flags-and-marked` F2 set (not B5's toggle: choosing a flag sets it on every target; No Flag clears) |
| Suspend | Suspend Card / Suspend Cards | some target card is not suspended | ⇧⌘J | `sched/manual-ops` M3 on every target card |
| Unsuspend | Unsuspend Card / Unsuspend <s> Cards (s = suspended targets) | some target card is suspended | | M4 on the suspended target cards only |
| Bury | Bury Card / Bury Cards | some target card is not buried | ⌘J | M2 on every target card |
| Unbury | Unbury Card / Unbury <b> Cards | some target card is buried | | M4 on the buried target cards only |
| Set Due Date… | Set Due Date… | always | ⇧⌘D | B10 (`setDueBrowser`) |
| Forget… | Forget… | always | ⌥⌘N | B10 Reset, with its two options (`restorePositionBrowser`, `resetCountsBrowser`) |
| Change Note Type… | Change Note Type… | target notes share one note type | ⇧⌘M | `notetypes/change-notetype`, sheet N22.8 |
| Find and Replace… | Find and Replace… | always, also with no selection | ⌥⌘F | V9 |
| Find Duplicates… | Find Duplicates… | always | | V10 |
| Delete | Delete Note / Delete <n> Notes, with "…" when it asks (V8.2) | always | ⌘⌫ (only while the table has focus) | B11 |
| Edit Note Type… | Edit Note Type… | one note type among the targets | | `notetypes/workspace` W14 |

- Suspend and Unsuspend are two commands (R1193 and B3's toggle are superseded for Browse; R1228's keys are superseded by round 7's: Bury ⌘J, Suspend ⇧⌘J). The same for Bury and Unbury. With a mix, both are shown.
- Every command is in the row's context menu, in the Mac menus (N21.8) and, with several rows, in the inspector (V7). Disabled with no selection, except Find and Replace and Find Duplicates.
- The first version's set is the table above: the actions round 8 question 7 drew and Antoine chose with "you can do all of them" (decision `browser`, "All bulk actions are in the first version"). The round 8 page listed these under "Later", and they are not in this table: Grade Now, Reposition, Mark Note (flags cover marking), Export Selected Notes, Create Copy, Edit Field across many notes, Clear Unused Tags, Create Filtered Deck (R1192, R1199, R1208 to R1211, R1213, R1214 wait). The plan lists this reading as an open point for Antoine.

### V8.2 When a command asks

- Nothing asks for Change Deck, Add Tags, Remove Tags, Flag, Suspend, Unsuspend, Bury, Unbury or Set Due Date: each is one undo step. The sheets of Change Deck, Add and Remove Tags, Set Due Date and Forget collect input; they are not questions.
- **Delete** asks when the target notes are more than one, or when the one target note has more than one card (in Cards mode, deleting a card's row deletes its note and its other cards). One note with one card is deleted at once (R1212's "no confirmation" stands only for that case). The question: title "Delete <n> Notes?" (n ≥ 2), or for one note "Delete “<sort field>”?" with its Sort Field cell text (cut to 40 characters with "…"; "Delete Note?" when that text is empty); message "Their <c> cards will also be deleted." or "Its <c> cards will also be deleted." (c the cards that go, c ≥ 2; for n ≥ 2 and c = n the message is left out); buttons as N20.7's (Mac: Cancel and Delete, Delete destructive, not the default; touch: Delete Notes / Delete Note destructive).
- **Forget…** is a sheet with its two options and the line "Review history is kept."; its button is Forget.
- **Find and Replace** asks nothing more: its preview is the question (V9).
- **Change Note Type** asks in its sheet (N22.8).

### V8.3 Undo

- Each command is one undo step (R1182; `browser/bulk-actions` B16's step ids). The Edit menu, the touch Undo button and VoiceOver name the step in Apple's style, from Meguri's display names (as `review/reviewer-strings` S5 does for review): Change Deck, Add Tags, Remove Tags, Flag, Suspend Card(s), Unsuspend Card(s), Bury Card(s), Unbury Card(s), Set Due Date, Forget, Change Note Type, Find and Replace, Delete Note(s), Edit Note (an inspector save). So "Undo Change Deck", "Undo Suspend Cards".
- No toast and no banner (R1184's message and its touch Undo button are superseded); VoiceOver announces the result with its count ("12 cards suspended", "3 notes deleted"), from `affected`.
- Undo of a delete restores the notes, their cards and their rows in the table (R1156 reversed); Undo of the other commands updates the rows in place.

## V9. Find and Replace (8A)

The sheet of `browser/find-replace` F2, changed:

- **Rows**: Find; Replace With; In (a menu: All Fields, then the field names of the target notes' note types, grouped under each note type's name when there are several; a name chosen under one note type still replaces in every target note whose note type has a field of that name, F6; then Tags); Notes (two choices: "<k> Selected", "All <m> Found"; Selected first and the default when there is a selection, disabled with none); options Match Case, Whole Words, Regular Expression.
- **Notes**: "Selected" is F8's selected notes. "All Found" is every note of the current results (the notes of all result rows), never notes outside them: it replaces F8's all-notes target, which this sheet no longer offers. m is that number of notes.
- **Match Case** is F1's Ignore case inverted (off by default: case is ignored). **Whole Words** (new): word characters are those of the `regex` crate's Unicode `\w` class. In literal mode `\b` is put before the escaped text when its first character is a word character and after it when its last is; in regular-expression mode the pattern becomes `\b(?:` + pattern + `)\b` whatever it holds (the user's pattern decides its own edges). The built pattern then goes through F4. The three options are remembered per device (F2). Find and Replace With histories stay F2's.
- **Preview** (replaces F10's count): as the user types, the core runs `find_replace_preview` (F10) on the target. It returns how many notes would change (F6 step 6: a match whose replacement gives back the stored value does not count) and, in result order, up to 100 of those notes, each with its Sort Field text, the name of the changed field (shown only with All Fields) and the new text with each removed and each inserted span marked. The sheet shows "<c> of <k> notes will change" above the list: removed text crossed out in red, inserted text in the accent colour; the list scrolls. While it runs it shows "Counting…"; an invalid pattern shows F10's inline error. With c = 0 it shows "No Matches" and the button is disabled.
- **The button** reads "Replace in <c> Notes" ("Replace in 1 Note"); Cancel beside it; the line "Undo restores every note." at the bottom of the sheet.
- **After**: the sheet closes, the rows update, Edit shows "Undo Find and Replace". No message (F9's message is superseded; VoiceOver announces "<c> notes changed").
- Opens on the selection (Selected) when there is one, otherwise on All Found; In starts on the remembered choice (F2).
- **Preview entries**: one entry per changed (note, field) pair, in result order, then field order; Tags count as one field. Each shows the old and new stored values with HTML tags removed (`browser/columns` C7.1's rule) and the difference between the two texts marked by a character diff (the shortest edit script): characters only in the old text crossed out in red, characters only in the new text in the accent colour. The count c is of notes, not entries.

## V10. Find Duplicates

- A small sheet: **Field** (a menu of every field name, grouped by note type; it starts on the Sort Field of the current row's note type; a name chosen under one note type matches that field name in every note type that has it, as Find and Replace does), **Notes** ("All Notes" or "All <m> Found", like V9; All Notes the default), Cancel and Find.
- Find runs the core's `find_duplicates(field name, target)`: for every target note whose note type has a field of that exact name, its **key** is the field value with HTML removed as `dupe:` removes it (`search/names-and-ids#N6`, media file names kept), character references decoded, White_Space runs collapsed to one space and trimmed, then simple case folding (`search/case-folding` C3). Notes with an empty key are left out. Notes with equal keys form a group; groups of two or more are the result.
- The result replaces the query with one Duplicates token (`search/filter-tokens` T3: "Duplicates in <field>: <g> groups") whose built item is the note ids of every grouped note, switches to Notes mode, and sorts by the field (V4) with the field's column shown while the token is present (display only: nothing is written to `meguriBrowserColumnSets`). Browse keeps the query it had before Find as the **previous query**; removing the Duplicates token restores it; any other change of the query drops it.
- With no group: the sheet stays and says "No duplicates found."
- Budget: 2 s over 150,000 notes, off the main thread, with progress and Cancel.

## V11. Status line

Under the table (Mac, iPad; N21.3). Left, by case (numbers with the locale's grouping, the first number bold):

| Case | Text |
|---|---|
| empty query | "<t> cards" (Notes: "<t> notes") |
| any other query | "<r> of <t> cards" (Notes: "notes"); singular when t is 1 ("1 of 1 card") |
| a search still streaming | "Searching… <r> of <t> cards" |
| with a selection of s ≥ 1 | the text above, then " · <s> selected" |

Right: the column set menu (V2.2): "Standard Columns" or "<note type name> Columns", with a chevron. On iPhone and the folded Duo, the list ends with "<r> of <t> cards" in a footer (N21.10). R1111's "3 of 1,204 cards selected" is superseded.

## V12. The inspector's Note tab

- **Preview** at the top: the current card as review renders it (`browser/preview` P8, `render/preview`), updating within 300 ms as fields change (R1220); audio does not play by itself (its play buttons do). A click flips question and answer; "Show Both Sides" in its menu shows both (`previewBothSides`, K6.3). With several cards, "Card <i> of <k> ›" steps through the note's cards (the inspector's card, not the selection). The preview's header folds it away (device state).
- **Deck** and **Type** rows (as Add Note's, N17.1). Deck shows the shown card's deck; choosing another deck runs Change Deck on that card (Cards mode) or on every card of the note (Notes mode). Type shows the note type; its menu lists the other note types (choosing one opens Change Note Type… on this note) and ends with Edit Note Type….
- **Fields**: the non-empty fields (`text/field-emptiness` E1) in field order, then one row "<e> Empty Fields" (disclosure) when e > 0. Opening it shows every field in field order; it stays open for that note type on this device until closed. A field that becomes empty or non-empty while the note is shown keeps its place until another note is shown. Audio is a player (play, length); a click on its edge selects it as one object; Delete removes the reference. Images are thumbnails. The HTML control shows the stored text (`editor/field-html`).
- **Tags**: a token field with completion from existing tags (`tags/complete`).
- **Formatting bar** under the fields: Bold, Italic, Underline, Cloze (only for a cloze note type, `editor/cloze-wrap`), Attach (an image or audio file, `editor` media insertion), HTML. The rest is in the Format menu (N21.8; `editor/formatting`): Highlight, Text Colour, Superscript, Subscript, Lists, Remove Formatting.
- **Saving**: a field's edit is saved when the field loses focus, when another row is selected, and when Browse is left (R1223; `editor/edit-flows`, the Browse editor). Each save is one undo step "Edit Note"; the row updates at once.

## V13. The inspector's Info tab

- `stats/card-info`'s snapshot of the shown card, as `docs/design/screen-inventory.yaml` `cardInfo.card` lays it out, with one change of order: the stats rows (C4), then **Review History** (C6), then **Forgetting Curve** (C7) below the history, drawn with Swift Charts in the system style (decision 2026-11-20). The range picker and tooltip stay C7.3 to C7.6's.
- ⌥⌘I (View › Card Info, R1226) shows the inspector with Info selected. It follows the current row live (R1247).

## V14. Touch

### V14.1 iPad and the Duo opened

- iPad landscape: the three panes; default columns state, Sort Field, Deck, Due (the standard set's display on iPad drops columns that do not fit, last first; the stored list is unchanged). iPad in portrait: the sidebar slides over from its toolbar button. With a keyboard and pointer the iPad behaves as the Mac: ⌘-click, ⇧-click, secondary click, the same keys.
- The Duo opened (regular width): the list (V14.3's two-line rows) in the left half, the inspector (V12, V13) in the right half; nothing on the fold. Filters (the sidebar's sections and Cards / Notes) are a sheet from the Filters button. Sort By is in the Filters sheet. With no row chosen, the right half shows the first result.

### V14.2 Select mode

- Select (toolbar) shows a selection circle on each row. A toolbar under the list then holds Change Deck…, Tags (Add Tags…, Remove Tags…), Suspend (Unsuspend when all targets are suspended), Flag and More, which lists the other commands of V8.1.
- Select All selects every result, not only rows on screen. On the Duo the right half shows V7's summary.
- The title reads "<s> Selected"; Done leaves Select mode.

### V14.3 iPhone and the Duo folded: the Browse tab

- A third tab, Browse (`magnifyingglass`), between Decks and Settings (on the Duo, the middle symbol of the side tab bar).
- The navigation bar: the title Browse, the Filters menu (`line.3.horizontal.decrease`: Filters…, which opens the Filters sheet; Select; Note Types, which pushes the Note Types list) and the Add menu. The search field with the same tokens and suggestions (`search/filter-tokens`).
- **Rows**: two lines: the Sort Field text; under it, in the secondary colour, the answer (the Answer cell, `browser/columns` C10, computed for visible rows only) and the card type, joined with " · " (Notes mode: the note type and "<k> cards"); trailing, the due date (V5.2) with V2.5's symbol before it. A suspended row is grey.
- **Filters sheet**: Clear and Done; Cards / Notes; Sort By (a menu of the sortable columns of the standard set and the direction); Saved Searches; Filter By, one row per kind of `search/filter-tokens` T3 with its current values ("Any" when none), each opening the list of its values with checks. What is chosen appears as tokens in the field, and tokens typed in the field show here (one query). R1234's chip row and R1236's Display menu are superseded.
- **Swipe left** on a row: Suspend or Unsuspend, Flag, Delete (Delete follows V8.2: it asks when the note has more than one card); a full swipe does the first. **Touch and hold**: the row's menu (V8.1). **Select**: in the Filters menu, or the system's multiple-selection gesture; the bar of V14.2.
- **A tap** opens the note in the Edit Note form (round 7's form, N20.8), with the empty fields folded and audio as a player (V12's field rules); its More menu: Preview, Card Info, then the card's commands (Suspend Card or Unsuspend Card, Flag, Change Deck…), then Delete Note in red. Previous and next move through the results (R1238).

## V15. Entry points

| From | Opens Browse with |
|---|---|
| ⇧⌘B, View › Browse, the Browse sidebar row, the Browse tab | the kept state (V1) |
| Review: More › Show in Browse (touch, after Edit Note), Card › Show in Browse ⇧⌘B (Mac, while a card is presented; View › Browse is then disabled so the key is never shared) | the kept query when it finds the card, the card selected and scrolled to; otherwise the query becomes one advanced token `cid:<id>` (R1113) |
| A deck's menu: Browse Cards (after Add Note, N20.2; `magnifyingglass`) | Deck: that deck, replacing the query |
| The deck page: a Suspended Cards row "Suspended Cards" with its count, shown while the deck (with subdecks) has suspended cards | Deck: that deck; Card State: Suspended |
| The heatmap: a click on a day (Mac) or its menu (touch) | the cards answered that day: Studied: Today for today; for an earlier day the advanced token `prop:rated=-<k>`, k the number of days before today (`search/history` H8), replacing the query |
| Note Types: Show Notes (an issue, `notetypes/workspace` W8) | Note Type: that type, plus the advanced token of the note ids |

Opening from review leaves the review (saved; it resumes from the deck page).

## V16. Strings

New text unless marked. Plurals are String Catalog plural entries.

| Text | Where |
|---|---|
| Browse | sidebar row, tab, View menu item, window section title |
| Cards, Notes | the mode control |
| %lld cards, %lld notes; %1$@ of %2$@ cards, %1$@ of %2$@ notes; Searching… ; · %lld selected | V11 |
| Standard Columns, %@ Columns, Use Standard Columns, Edit Note Type… | V2.2 |
| Fields, %lld More Fields, Standard, Size Columns to Fit | header menu |
| %lld Cards Selected, %lld Notes Selected, %lld notes, %lld decks, %lld suspended, No Selection | V7 |
| Change Deck…, Add Tags…, Remove Tags…, Flag, No Flag, Suspend Card(s), Unsuspend Card, Unsuspend %lld Cards, Bury Card(s), Unbury Card, Unbury %lld Cards, Set Due Date…, Forget…, Change Note Type…, Find and Replace…, Find Duplicates…, Delete Note, Delete %lld Notes…, Select Sibling Cards, Show in Browse, Browse Cards, Suspended Cards | V8, V15 (Suspend Card and Bury Card exist, `review/reviewer-strings` S18) |
| Delete %lld Notes?, Delete “%@”?, Delete Note?, Their %lld cards will also be deleted., Its %lld cards will also be deleted., Delete, Delete Note, Delete Notes, Cancel | V8.2 |
| Review history is kept., Forget, Restore original position where possible, Reset repetition and lapse counts | Forget sheet (the two option labels exist, `browser/browse-strings` W3) |
| Find, Replace With, In, All Fields, Tags, Notes, %lld Selected, All %lld Found, All Notes, Match Case, Whole Words, Regular Expression, %1$lld of %2$lld notes will change, Counting…, No Matches, Replace in %lld Notes, Undo restores every note. | V9 |
| Find Duplicates, Field, Find, No duplicates found., Duplicates in %@: %lld groups | V10 |
| Note, Info, Show Both Sides, Card %1$lld of %2$lld, %lld Empty Fields, Deck, Type, Tags, Review History, Forgetting Curve | V12, V13 |
| Select, Done, %lld Selected, Select All, Tags, More, Filters, Filters…, Note Types, Clear, Sort By, Filter By, Any | V14 |
| No Cards, No Notes, Clear Search, Try Again | the empty and error states (`docs/design/screen-inventory.yaml` `browse.browser`) |
| Undo %@ step names: Change Deck, Add Tags, Remove Tags, Flag, Suspend Card, Suspend Cards, Unsuspend Card, Unsuspend Cards, Bury Card, Bury Cards, Unbury Card, Unbury Cards, Set Due Date, Forget, Change Note Type, Find and Replace, Delete Note, Delete Notes, Edit Note | V8.3 |
| %lld cards suspended (and the other results), %lld notes changed | VoiceOver announcements |

## V17. Speed (the 150k fixture, `tools/mg fixtures 150k`; U90 and U242 measure)

| What | Budget | How |
|---|---|---|
| Keystroke echo in the field | one refresh interval | the query runs off the main thread; nothing waits on it (R1824) |
| First page of results, simple tokens (deck, tag, note type, state, flag, days) | 100 ms p95 (R1825) | `browser/result-sets` H3: one SQL statement returns only the ordered id list (8–16 bytes per row); no cells |
| First page, text words, fields, advanced regex | 300 ms p95, then streamed (R1826) | `search/query-model` Q10 two stages |
| Scrolling at any speed | no hitch above 5 ms/s (R1829) | the table asks `rows` (H7) only for visible indexes plus one page (200) each side, at most 1,000 per call, cached by id; cells never render templates; Answer cells (iPhone rows) come from `qa_cells` in the background with placeholders |
| Sort change, Cards / Notes switch | 200 ms (R1828), 100 ms (R1830) | resort on the id list (H8), toggle without a new run (H11) |
| Field columns | no slower than other cells | the row read takes the field values of the visible rows' notes in the same transaction; media lengths are read in the background and cached by file name |
| Suggestions | one frame (R1104) | the in-memory name index (D6.7); counts arrive later (T12.4) |
| Sidebar and suggestion counts | 200 ms for all visible | T12.4's grouped reads, cached by change seq |
| Select All on 150,000 rows | 50 ms (R1153) | a selection is an index range on the handle, never a list of rows in Swift |
| Bulk command on 10,000 notes | 2 s with progress (R1831) | one op over the resolved ids (H12), off the main thread |
| Find and Replace preview over all found notes | count within 1 s for 10,000 notes, streamed; Replace within 5 s for 150,000 (R1832) | the preview shares the session's snapshot and stops at the first 100 listed changes for its list while it keeps counting |
| Inspector on row change | 100 ms (R1830) | the note and card are read by id; the preview renders after the fields show |

## V18. Tests

- Core (`core/crates/meguri-engine/tests/` and `meguri-search/tests/`, each added as one `mod` line in its crate's `tests/main.rs`): V2.4's key reading and writing (missing, wrong shapes, unknown members kept, only the changed set written, a set whose note type was removed is ignored and kept, the Anki keys untouched); V3's cells (missing field dash, empty, media); V4.2's order on a mixed fixture (numbers, texts, empties, rows without the field, both directions, final key); V8.1's targeting (Unsuspend only on suspended targets, Unbury only on buried, Flag sets); V9's preview counts, list and Whole Words; V10's groups and key rule.
- Swift model tests (`MeguriKit/Tests/AppModelTests`): V2.2's set choice, V7's summary line, V8.1's titles and shown-when rules, V8.2's Delete question rule, V8.3's step names, V11's strings, V14.3's swipe actions.
- VM UI suites (one job at a time): the Mac three panes with a selection of one and of many, the Delete question, the Find and Replace preview, the iPad Select mode, the iPhone Browse tab; screenshots for N21's design review.
- Benchmarks (U242): V17's rows on the 150k fixture.

## V19. What this replaces

`browser/sidebar` S7, S8 and the R1168 to R1170 inputs (T9); `browser/bulk-actions` B3 and B4's toggles for Browse (V8.1), B5's toggle (Flag sets), B11's "No confirmation" (V8.2), B16's message (V8.3); `browser/find-replace` F2's Selected notes only checkbox, F8's all-notes target, F9's message and F10's count (V9); `browser/config-keys` K4 and K5 as Browse's display state (V2.4); `browser/columns` C5 tints and the C7.4 middle truncation (V2.5, V5.1); `browser/inspector` I8's three tabs (V12, V13); `browser/entry-points` E5's canonical commit (`search/filter-tokens` T8). Each section carries a "Changed 2026-11-20" note pointing here.
