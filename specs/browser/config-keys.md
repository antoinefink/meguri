# Browser and search configuration keys

Spec: browser/config-keys
Requirements: R1115, R1134, R1135, R1137, R1138, R1141, R1265, R1266, R1267, R1038, R1196, R1198; supports R1037, R1112, R1133, R1139, R1146, R1157, R1171, R1172, R1173, R1218, R2020
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/config/

Browse, its sidebar, the previewer, Settings › Browse and the search engine keep their synced settings in collection config keys (`storage/config-table`). Meguri reads and writes them with Anki's names and JSON shapes, so Anki clients and Meguri share them through sync. This file gives, for each key, its shape, its default, how a stored value is read, and what each user action writes. Stamping and the compact form are `storage/config-table` C4 and C5.

## K1. The keys

| Key | JSON shape | Read when absent or unreadable | Meaning |
|---|---|---|---|
| `activeCols` | array of column keys | `["noteFld","template","cardDue","deck"]` | Browse columns in cards mode, in display order (K4) |
| `activeNoteCols` | array of column keys | `["noteFld","note","template","noteTags"]` | Browse columns in notes mode (K4) |
| `sortType` | string, a column key | `noteCrt` as the sort column (K5) | cards-mode sort column |
| `sortBackwards` | boolean | false | cards-mode sort is descending |
| `noteSortType` | string, a column key | `noteCrt` as the sort column (K5) | notes-mode sort column |
| `browserNoteSortBackwards` | boolean | false | notes-mode sort is descending |
| `browserTableShowNotesMode` | boolean | false | Browse shows notes, not cards (R1115) |
| `savedFilters` | object: name → search text | `{}` | saved searches (K7) |
| `flagLabels` | object: `"1"`–`"7"` → name | `{}` | custom flag names (K8) |
| `collapseSavedSearches`, `collapseToday`, `collapseFlags`, `collapseCardState`, `collapseDecks`, `collapseNotetypes`, `collapseTags` | boolean | false | the sidebar section is collapsed (K6.2) |
| `previewBothSides` | boolean | false | the previewer shows each card on its answer side at once (K6.3) |
| `setDueBrowser` | string | `"0"` | last Set Due Date input from Browse (K6.6) |
| `defaultSearchText` | string | `""` | the search Browse opens with (K6.4) |
| `ignoreAccentsInSearch` | boolean | false | plain, `w:` and `sc:` searches ignore accents (K6.5) |
| `restorePositionBrowser` | boolean | **true** | Reset from Browse: "Restore original position where possible" (K6.6) |
| `resetCountsBrowser` | boolean | false | Reset from Browse: "Reset repetition and lapse counts" (K6.6) |
| `normalize_note_text` | boolean | **true** | search text and note text are NFC-normalized (K6.5); Meguri reads it and never writes it |

- Key names are exact, case included; `normalize_note_text` is the only one with underscores.
- "Unreadable" is `storage/config-table` C3: a missing key, a value not stored with the BLOB storage class, bytes that are not JSON. Each key's own reading rules (K2–K8) then say what a readable value of another shape means.
- A new collection holds `sortType` `"noteFld"` and `sortBackwards` false and none of the other keys (scenario `browser/config/new-collection`; the new-collection keys are U28's). Every other key in the table is created by the first write that changes it.
- Keys Meguri does not use are K9. Device-only Browse state is K11.

## K2. Reading

- A read never writes (R1138): no row is inserted, rewritten or restamped, `col.mod` does not move, and no change is reported. Defaults are not stored. Opening Browse, scrolling, searching and switching modes only read.
- When an object repeats a member name, the last occurrence counts (`storage/config-table` C3).

### K2.1 Boolean keys

`browserTableShowNotesMode`, `sortBackwards`, `browserNoteSortBackwards`, the seven `collapse*` keys, `previewBothSides`, `ignoreAccentsInSearch`, `restorePositionBrowser`, `resetCountsBrowser` and `normalize_note_text` read as the stored value when it is the JSON literal `true` or `false`, and as the key's default (K1) otherwise.

| Stored `val` | Reads as |
|---|---|
| `true`, ` true ` (spaces around) | true |
| `false` | false |
| `1`, `0`, `1.0`, `"true"`, `"false"`, `null`, `[true]`, `{}` | the default |
| `{` (not JSON), or `true` stored as TEXT | the default |

So `restorePositionBrowser` stored as `0` reads as true, and `normalize_note_text` stored as `"false"` reads as true.

### K2.2 String keys

`setDueBrowser` and `defaultSearchText` read as the stored value when it is a JSON string, the empty string included (`""` reads as `""`, not as `"0"`), and as the default otherwise (`5` and `null` read as `"0"` and `""`).

`sortType` and `noteSortType` are strings too; K5 gives their reading.

## K3. Column keys

The 19 columns, each named by a key that `activeCols`, `activeNoteCols`, `sortType` and `noteSortType` store. All 19 exist in both modes.

| Key | Column | Sortable in cards mode | Sortable in notes mode | First sort direction |
|---|---|---|---|---|
| `noteFld` | Sort Field | yes | yes | ascending |
| `question` | Question | no | no | none |
| `answer` | Answer | no | no | none |
| `template` | Card Type / Cards | yes | yes | descending |
| `deck` | Deck | yes | yes | ascending |
| `cardDue` | Due | yes | yes | descending |
| `cardEase` | Ease / Avg. Ease | yes | yes | descending |
| `cardIvl` | Interval / Avg. Interval | yes | yes | descending |
| `cardReps` | Reviews | yes | yes | descending |
| `cardLapses` | Lapses | yes | yes | descending |
| `noteCrt` | Created | yes | yes | descending |
| `cardMod` | Card Modified | yes | yes | descending |
| `noteMod` | Note Modified | yes | yes | descending |
| `note` | Note Type | yes | yes | ascending |
| `noteTags` | Tags | yes | yes | ascending |
| `originalPosition` | Position | yes | yes | descending |
| `stability` | Stability | yes | no | descending |
| `difficulty` | Difficulty | yes | no | descending |
| `retrievability` | Retrievability | yes | no | descending |

- Keys compare exactly, case included: `NoteFld` is not a column key.
- Every other string, the empty string included, is an **unknown column key**: add-on columns and columns of other clients or later releases.
- The first sort direction is the direction a column sorts in when the user first sorts by it (R1139, K5.2). What each column shows and how it sorts is U84's.

## K4. Active columns (`activeCols`, `activeNoteCols`)

Cards mode uses `activeCols`, notes mode `activeNoteCols`. The rules are the same for both.

### K4.1 Reading

1. The **base list** is the stored array's items in order, when the key holds a JSON array. When the key is absent, unreadable or not an array, the base list is the mode's default (K1).
2. An item is a **known column** when it is a string that is a column key (K3). Every other item is an **unknown entry**: an unknown column key, or an item that is not a string (`5`, `null`, an object).
3. The **displayed columns** are the known columns of the base list in base-list order, each key once, at its first occurrence. Unknown entries are never displayed (R1137).
4. When the base list holds no known column (`[]`, `["addonX"]`), the base list is its items followed by the mode's default keys, so the default columns are displayed and the unknown entries stay in front of them.

The base list is what the writes of K4.2 and K4.3 start from. Reading never writes it.

### K4.2 Showing and hiding a column

Each write starts from the base list with every repeated occurrence of a known column after its first removed. Unknown entries are never removed, merged or reordered, repeated ones included.

- **Show column X** (X a column key): when X is displayed, nothing is written. Otherwise X is appended at the end of the list, and the list is written.
- **Hide column X**: when X is not displayed, nothing is written. When X is the only displayed column, the hide is refused with kind `last_column` and nothing is written (R1133). Otherwise X is removed and the list is written.

### K4.3 Reordering

**Reorder** takes the new order of the displayed columns: every displayed column exactly once, in the new order. Any other input is refused with kind `invalid_order` and nothing is written.

Starting from the list of K4.2 (repeated known columns removed), every unknown entry stays at its index, and the positions that held known columns receive the new order, first position first. The result is written (R1135). When it equals the stored value, `storage/config-table` C4 writes nothing.

### K4.4 Examples

| Stored `activeCols` | Displayed | Action | Written |
|---|---|---|---|
| absent | noteFld, template, cardDue, deck | show `noteCrt` | `["noteFld","template","cardDue","deck","noteCrt"]` |
| absent | noteFld, template, cardDue, deck | hide `template` | `["noteFld","cardDue","deck"]` |
| `["noteFld","addonX","deck"]` | noteFld, deck | show `cardDue` | `["noteFld","addonX","deck","cardDue"]` |
| `["noteFld","addonX","deck"]` | noteFld, deck | hide `noteFld` | `["addonX","deck"]` |
| `["noteFld","addonX","deck","cardDue"]` | noteFld, deck, cardDue | reorder cardDue, noteFld, deck | `["cardDue","addonX","noteFld","deck"]` |
| `["deck","deck","addonX","deck"]` | deck | show `noteFld` | `["deck","addonX","noteFld"]` |
| `["deck"]` | deck | hide `deck` | refused `last_column`, nothing written |
| `["addonX"]` | noteFld, template, cardDue, deck | hide `cardDue` | `["addonX","noteFld","template","deck"]` |
| `[]` | noteFld, template, cardDue, deck | show `noteTags` | `["noteFld","template","cardDue","deck","noteTags"]` |
| `["noteFld",5,"addonX"]` | noteFld | show `deck` | `["noteFld",5,"addonX","deck"]` |
| `"noteFld"` (a string) | noteFld, template, cardDue, deck | show `deck` | nothing (deck is displayed) |
| `"noteFld"` (a string) | noteFld, template, cardDue, deck | show `note` | `["noteFld","template","cardDue","deck","note"]` |

### K4.5 Differences from Anki

- Anki's desktop browser shows the columns in an order it keeps per device, outside the collection; the array order is only the order in which columns were added. Meguri stores its order in the array, so it syncs (R1135). When Anki toggles a column, it appends or removes that key and keeps the rest of the array, Meguri's order included.
- Anki shows an unknown column key as an empty column labelled as an add-on's; Meguri hides it and keeps it (R1137).
- Anki shows an empty table for `[]` or for an array of unknown keys only; Meguri shows the default columns (K4.1 rule 4).

None of these appears in a scenario: the oracle runs Anki's collection library, not its desktop browser.

## K5. Sort keys

Cards mode uses `sortType` and `sortBackwards`, notes mode `noteSortType` and `browserNoteSortBackwards`.

### K5.1 Reading

- The **sort column** is the stored column key when it is a JSON string naming a column that is sortable in the mode (K3). Otherwise it is `noteCrt`: the key is absent or unreadable, or holds another type, an unknown column key, or a column that is not sortable in the mode (R1141).
- The **direction** is descending when the backwards key reads as true (K2.1), ascending otherwise.
- The fallback is never written back.

| Mode | Column key holds | Backwards key holds | Sort |
|---|---|---|---|
| cards | `"noteFld"` | `false` | noteFld ascending |
| cards | `"cardDue"` | `true` | cardDue descending |
| cards | absent | absent | noteCrt ascending |
| cards | `"question"` | `true` | noteCrt descending |
| cards | `"stability"` | `true` | stability descending |
| notes | `"stability"` | absent | noteCrt ascending |
| notes | `"myAddonColumn"` | `false` | noteCrt ascending |
| cards | `5` | `1` | noteCrt ascending |

### K5.2 Writing

- **Sort by column X** (a header click, R1139): X must be sortable in the mode, else the sort is refused with kind `unsortable_column` and nothing is written (R1140: the UI never offers it). When X is the current sort column (K5.1), the new direction is the reverse of the current one; otherwise it is X's first sort direction (K3). Then the column key is written with X and the backwards key with true for descending, false for ascending.
- **Set sort (X, direction)** (the iPhone Sort menu, R1146): X sortable in the mode, else refused `unsortable_column`. The column key is written with X and the backwards key with the direction.
- Each of the two keys is written by `storage/config-table` C4: a key that already holds the value is not written.

| Mode | Stored before | Action | Written |
|---|---|---|---|
| cards | `sortType` `"noteFld"`, `sortBackwards` `false` | sort by noteFld | `sortBackwards` `true` |
| cards | `sortType` `"noteFld"`, `sortBackwards` `false` | sort by cardDue | `sortType` `"cardDue"`, `sortBackwards` `true` |
| cards | `sortType` `"noteFld"`, `sortBackwards` `true` | sort by deck | `sortType` `"deck"`, `sortBackwards` `false` |
| cards | `sortType` `"question"`, `sortBackwards` `false` | sort by noteCrt (the current column) | `sortType` `"noteCrt"`, `sortBackwards` `true` |
| notes | neither key | sort by noteMod | `noteSortType` `"noteMod"`, `browserNoteSortBackwards` `true` |
| notes | neither key | set sort (noteCrt, ascending) | `noteSortType` `"noteCrt"`, `browserNoteSortBackwards` `false` (inserted) |
| cards | anything | sort by question | refused `unsortable_column` |

### K5.3 Differences from Anki

Anki's desktop browser falls back to `noteCrt` only when the stored value is not a column key or is missing. For a column key that is not sortable in the mode (`question` in cards mode, `stability` in notes mode) it runs the search with no order at all, and it reads a backwards value that is not a boolean by its truth value. Meguri sorts by `noteCrt` and reads only `true` as descending (R1141). Anki also compares a header click with the stored column, not with the fallback, so a click on Created over a stored `question` sorts in Created's first direction where Meguri reverses. No scenario shows these: they are Anki's desktop code, which the oracle does not run.

## K6. Single settings

### K6.1 Notes mode (`browserTableShowNotesMode`, R1115)

Toggling Browse's Cards/Notes switch writes the new mode: true for notes. Nothing else is written by the toggle; the column and sort keys of the other mode are only read.

### K6.2 Sidebar sections (R1157)

| Sidebar section | Key |
|---|---|
| Saved Searches | `collapseSavedSearches` |
| Today | `collapseToday` |
| Flags | `collapseFlags` |
| Card State | `collapseCardState` |
| Decks | `collapseDecks` |
| Note Types | `collapseNotetypes` |
| Tags | `collapseTags` |

Collapsing a section writes true to its key, expanding it writes false. Decks and tags inside the sections keep their own expansion in the deck and the tag rows, not here (U216, `tags/tags`).

### K6.3 Previewer (`previewBothSides`, R1218)

True: the previewer shows each card directly on its answer side, so question and answer appear together, and moving goes from card to card. Toggling writes the new value.

### K6.4 Default search (`defaultSearchText`, R1112, R1267)

- Settings › Browse writes the text exactly as typed, untrimmed.
- The text **is set** when it still has a character after removing these characters from both ends: U+0009–U+000D, U+001C–U+001F, U+0020, U+0085, U+00A0, U+1680, U+2000–U+200A, U+2028, U+2029, U+202F, U+205F, U+3000. Browse then opens with the stored text, untrimmed, and shows it in the field. When it is not set, Browse opens with `deck:current` and an empty field with a placeholder (R1112, U210).

| Stored | Is set | Browse opens with |
|---|---|---|
| absent | no | `deck:current`, empty field |
| `""`, `"   "`, `"　"` | no | `deck:current`, empty field |
| `"  deck:x "` | yes | `  deck:x ` |
| `"​"` | yes | `​` |

### K6.5 Search keys (`ignoreAccentsInSearch`, `normalize_note_text`)

- `ignoreAccentsInSearch` (R1038): Settings › Browse "Ignore accents in search" writes it. With it true, plain, `w:` and `sc:` searches ignore accents as `nc:` does (U81).
- `normalize_note_text` (R1037): read by search and the note save path (U81, U27); default true. No Meguri screen writes it.
- Search reads both through its context (U80): this file only gives their reading.

### K6.6 Remembered inputs (`setDueBrowser`, `restorePositionBrowser`, `resetCountsBrowser`)

- Set Due Date from Browse (R1196) prefills its field with `setDueBrowser` (K2.2). When the operation succeeds it writes the input text exactly as accepted (`"0"`, `"1-7"`, `"3!"`) to `setDueBrowser`, inside its own operation.
- Reset from Browse (R1198) presets its two options from `restorePositionBrowser` and `resetCountsBrowser` (K2.1). When the operation runs it writes the two chosen values to the two keys, inside its own operation.
- Each write is `storage/config-table` C4: a key holding the same value is not written, and a missing key is inserted even when the value equals the default (Anki does the same: a first Reset with the default options stores `restorePositionBrowser` true and `resetCountsBrowser` false).
- Set Due Date and Reset from review use their own keys (`setDueReviewer`, default `"1"`; `restorePositionReviewer`, `resetCountsReviewer`): those are the scheduler area's (U48), never these.

## K7. Saved searches (`savedFilters`)

### K7.1 Reading

- When the key holds a JSON object, each member whose value is a string is a saved search: the member name is its name, the value its search text. Members with other values are kept by every write and never shown.
- When the key is absent, unreadable or holds something other than an object, there are no saved searches, and the first saved-search write replaces the value with a new object.
- Names are compared exactly: `Leeches` and `leeches` are two saved searches. Names and search texts are stored as given, with no trimming or normalization.
- The order in which the sidebar lists them is U216's (R1158). Anki's desktop sidebar lists them by name in code point order.

### K7.2 Writes

Each write below changes the object's members as stated, keeps every other member and its value (`storage/config-table` C6) and writes the key in compact form (members sorted by name). The search text a caller passes is the committed canonical search (U88).

- **Save Search** (name, search) (R1171 Save Current Search; R1172 drag onto Saved Searches): an empty name is refused with kind `empty_name`. When a saved search of that name holds the same search text, nothing is written. Otherwise the member is set, replacing any member of that name. Asking for confirmation before replacing an existing saved search is the caller's (U217).
- **Update Saved Search** (name, search) (Update to Current Search): the member is set; nothing is written when it already holds that text. A name missing at that moment is added.
- **Rename Saved Search** (old name, new text): every `"` is removed from the new text. Nothing is written when the result is empty, equals the old name, or when no saved search has the old name. Otherwise the member of the new name gets the old member's search text (replacing any member of that name, after the caller's confirmation) and the old member is removed.
- **Delete Saved Searches** (names): every listed name that is a member is removed. Names that are not members are skipped; when none was removed, nothing is written.

| Stored before | Write | Stored after |
|---|---|---|
| absent | Save ("Leeches", `tag:leech`) | `{"Leeches":"tag:leech"}` |
| `{"Leeches":"tag:leech"}` | Save ("Leeches", `tag:leech`) | unchanged, nothing written |
| `{"Leeches":"tag:leech"}` | Save ("Leeches", `tag:leech is:suspended`) | `{"Leeches":"tag:leech is:suspended"}` |
| `{"b":"x","a":"y"}` | Save ("c", `is:new`) | `{"a":"y","b":"x","c":"is:new"}` |
| `{"Due":"is:due","n":3}` | Rename ("Due", `"Due" now`) | `{"Due now":"is:due","n":3}` |
| `{"A":"is:new","B":"is:due"}` | Rename ("A", `B`) | `{"B":"is:new"}` |
| `{"A":"is:new"}` | Rename ("A", `""`) | unchanged, nothing written |
| `{"A":"is:new","B":"is:due","C":"flag:1"}` | Delete (A, C, Z) | `{"B":"is:due"}` |
| `{"A":"is:new"}` | Delete (A) | `{}` |
| `[1,2]` | Save ("A", `is:new`) | `{"A":"is:new"}` |

## K8. Flag names (`flagLabels`)

The shape, the shown names and the Rename Flag and Restore Default Name writes are `tags/flags-and-marked` F4 (U30). Browse's sidebar, menus and every other place that shows a flag read it through that rule.

## K9. Keys Meguri does not use (R1266)

- `randomOrderReposition` and `shiftPositionOfExistingCards`: Anki's Reposition stores there the last state of its two options (random order, and shifting the positions of other new cards), and presets its dialog from them. Meguri never reads or writes them; its Reposition does not store its options in the collection (U48, U87). A scheduler scenario that observes these keys after a Reposition needs a deviation entry for this.
- Add-on keys and keys of other clients: never read or written (`storage/config-table` C7).
- No browser write touches any key other than the ones it names (R1138): showing a column writes only that mode's column key, a sort writes only that mode's two sort keys, and so on.

## K10. Writes, undo and changes

### K10.1 Every write

- Each write sets one key's whole value through `storage/config-table` C4: compact form, `usn` -1, `mtime_secs` now, `col.mod` moved, and nothing written when the stored value is already the same value.
- Besides the actions above, every key has a **whole-value write** that sets the key to a given value of its K1 shape (for a column list, any array of strings, unknown keys included; for `savedFilters` and `flagLabels`, any object of strings; for `sortType` and `noteSortType`, any string). The scenario op `browser_config_set` (`specs/conformance/browser-config-ops.md` B1) and tests use it. `normalize_note_text` has none.
- A write that changes a key reports it among the operation's changes as a config change naming the key (U24), so open screens refresh: the sidebar on `savedFilters`, `flagLabels` and the `collapse*` keys, Browse on the column, sort and mode keys. A write that changes nothing reports nothing.

### K10.2 Undo

| Keys and writes | Anki | Meguri |
|---|---|---|
| `browserTableShowNotesMode`, `activeCols`, `activeNoteCols`, the four sort keys, the `collapse*` keys, `previewBothSides` | no undo step; earlier undo steps stay undoable; redo steps are dropped | no undo entry; earlier undo entries stay undoable (U25's skip-undo class, the one the current-deck selection uses, R42) |
| `defaultSearchText`, `ignoreAccentsInSearch` | saved by the preferences dialog together with the other preferences, as one undoable step | written by Settings › Browse, with the undo class of Settings writes (U25) |
| `savedFilters` | no undo step, as the first row | one undoable entry per write, named "Save Search", "Update Saved Search", "Rename Saved Search" or "Delete Saved Search" (R2020); a write that changes nothing makes no entry |
| `flagLabels` | no undo step, as the first row | "Rename Flag" (`tags/flags-and-marked` F4) |
| `setDueBrowser`, `restorePositionBrowser`, `resetCountsBrowser` | inside the Set Due Date or Reset step, undone with it | the same (U48, U87) |

A write that clears the whole undo history would discard the user's undo steps at every sort, column or section change, which Anki never does; the first row's writes therefore never clear it.

## K11. Device-only Browse state

Column widths (per device and mode, R1136), row density (R1151), the inspector position, the search field's syntax mode, search history and recent searches stay on the device (KTD12, U210): never in the collection, never synced. Settings › Browse shows them next to the two synced settings of K6.4 and K6.5 (R1267).

## K12. Sync

The keys travel with the whole config table (`storage/config-table` C9). A sync that brings changed keys reports them like any change, and Browse, the sidebar and search read the new values; nothing is merged per key.

## K13. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `browser/config/new-collection` | K1: the browser keys a new collection holds | recorded |
| `browser/config/absent-defaults` | K1 defaults through Anki's reads, K2: reading writes nothing | awaits the `browser_config` observation |
| `browser/config/wrong-shapes` | K2.1, K2.2 and `storage/config-table` C3 per key; the stored bytes are kept | awaits the `browser_config` observation |
| `browser/config/write-shapes` | K1 shapes in compact form, K10.1 (same value writes nothing) | awaits `browser_config_set` and `browser_config` |
| `browser/config/other-keys-untouched` | K9, K10.1: a `flagLabels` write changes no other key | awaits `browser_config_set` |
| `browser/config/live-roundtrip` | values written by Anki read in Meguri, values written by Meguri read in Anki (T1) | awaits `browser_config_set` and `browser_config` |

The oracle runs Anki's collection library, not its desktop browser, so the scenarios observe stored values and Anki's own reads of them (`browser-config-ops.md` B2). Meguri's interpretation of those values (K4, K5.1, K6.4, K7.1) and its actions (K4.2, K4.3, K5.2, K7.2, K10.2) are tested in U83's Rust tests with the tables of this file: every K4.4, K5.1, K5.2, K6.4 and K7.2 row, the refusals, a read of every key on an empty config leaving the file's bytes unchanged, a notes-mode toggle making one write and no undo entry, and an undoable test-hooks operation followed by a Save Search, after which undo names "Save Search" and two undos restore the saved searches and then the earlier rows.
