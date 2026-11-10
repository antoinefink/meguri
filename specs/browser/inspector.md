# Browse inspector and row navigation

Spec: browser/inspector
Requirements: R1221, R1223, R1224, R1229 (Go commands), R261; supports R262, R263, R822, R1185, R1222, R1247, R1830
Anki: 26.09.3 for I2 to I7 (the desktop browser's editor area and Go menu); none for the Meguri rules I8 lists
Scenarios: none (UI behaviour; I10 lists the tests)

Which note Browse's editor pane shows (U221), what follows the current row and what follows the selection, how moving between rows keeps the focused field, and the Go commands that move the current row or the keyboard focus. How the pane saves, what blocks a row change and what it does when another writer changes its note are `editor/edit-flows` W2 and W5 (U196); this file adds only what Browse's commands do with them. The editor itself is `editor/field-html` (U190); Card Info is `stats/card-info` (U222); the preview is `browser/preview`; selection and the current row are U212's (`browser/result-sets` H12). The oracle does not run Anki's browser window, so Meguri's tests check this file (I10).

## I1. Terms

- **Current row**: the focused row of the table, kept whether or not it is selected (`browser/bulk-actions` B2).
- **Single selected row**: the selected row when exactly one row is selected; otherwise there is none. It can differ from the current row: with row 3 selected, ⌘-click row 5 (current row 5, rows 3 and 5 selected), then ⌘-click row 5 again (current row 5, only row 3 selected).
- **Shown note**: the note of the single selected row: in cards mode its card's note, in notes mode the row's note.

## I2. What follows what (R1221, R1247)

| Pane | Follows | With none |
|---|---|---|
| Editor | the shown note (single selected row) | no editor; Meguri shows the multi-selection summary or the empty state (I8) |
| Preview | the single selected row's card (`browser/preview` P1) | the preview closes (P2) |
| Card Info | the current row's card; in notes mode the note's card with the lowest `ord`, then the lowest id | Card Info's empty view "(No card to display.)" (`cardInfo.card` screen contract) |

Each pane updates when its input changes: a click, a key, a search that changes the rows, a toggle of Cards/Notes (H11's mapping), and an action that removes rows.

## I3. Keeping the focused field (R1224)

- When the current row moves by Browse's **Previous** or **Next** command (I4), and the editor had a field focused, the next shown note opens with the field at the same position (index) focused, caret restored as `editor/field-html` F4 says. A note with fewer fields gets no focused field.
- The preview's move onto another row (`browser/preview` P4) is the same command, so it keeps the focused field too.
- Every other way of changing the row opens the note with no field focused, and the keyboard focus stays where it was: a click, the table's own ↑ and ↓ while the table has focus (R262; not the Previous and Next command), a search, First and Last. Anki behaves the same.

## I4. Go commands (R1229, R261)

| Command (R1229) | Does | Enabled |
|---|---|---|
| Find | focuses the search field and selects all its text | always |
| Sidebar Filter | shows the sidebar if hidden and focuses its filter box (`browser/sidebar` S6) | always |
| Card List | focuses the table | always |
| Note (⌥⌘L, R261) | flushes the editor (`editor/edit-flows` W2), then focuses the editor's first field | when the editor shows a note |
| Previous, Next | flushes the editor (W2), then moves the current row one row up or down; the selection becomes that row alone; the table scrolls to it; I3 applies | when a current row exists and a row exists above it, or below it (as Anki) |
| First, Last | moves the current row to the first or last row; the selection becomes that row alone | when the table has rows |
| First, Last with Shift | as First and Last, but the selection becomes every row from the previous current row to the new one, both included; rows selected before outside that range are no longer selected | when the table has rows |

- A failed flush stops the command: the current row, the selection and the editor stay as they were (`editor/edit-flows` W2, "A failed flush"). That covers Previous and Next from the preview too (`browser/preview` P4).
- Select All and Invert Selection need rows too (Anki disables them on an empty table).
- Shortcuts and the focus states they act in are `browser/command-map` M2 and M4's.

## I5. Saving and moving (R1223)

- Saving, the flush before every row change, search, mode switch, sort change, menu command, hiding of the pane and closing of Browse, and the failed flush are `editor/edit-flows` W2 and W3 with `editor/field-html` S1 and S2. The edited row's cells refresh from the change set (R1185, U211).
- Anki saves the editor before a row change, a committed search, a Cards/Notes toggle, Previous, Next and Note, Change Deck, Add Tags, Remove Tags, Clear Unused Tags, Toggle Suspend, Toggle Bury, the flags, Reposition, Set Due Date, Reset, Grade Now, Select Notes, Change Note Type, Find and Replace and Find Duplicates, and before the window closes. A failed save there has no recovery path (`editor/edit-flows` W8).

## I6. Changes from elsewhere (R822, R1185)

- A change set from another writer that lists the shown note, its note type or the note's removal is handled by `editor/edit-flows` W5: W5.1 says which changes reload the note, which refresh only the field options and which change nothing; W5.2 flushes and reloads; W5.3's banner applies to a local server's change while an edit is unsaved; W5.4 closes the editor when the note is removed (I2's "with none").
- A change set that changes the note's cards (flags, suspension, deck) but not the note leaves the editor as it is (W5.1); the row and Card Info refresh (U211, `stats/card-info` C10) and the preview renders again (`browser/preview` P5).
- Delete Notes first moves the current row and the selection to a row whose note is not deleted (`browser/bulk-actions` B11); the editor then shows that row's note, or none, and the preview follows (`browser/preview` P9).

## I7. Opening the preview

The preview opens only when there is a shown card (`browser/preview` P1, P2). Anki opens it only when its editor shows a note, which is the same condition except for a note without cards in notes mode.

## I8. Meguri rules

- With several rows selected the editor pane shows the summary ("12 cards selected", or notes in notes mode) with the bulk actions (R1221); Anki hides its editor area then. The summary's wording is Meguri's (`browser/browse-strings` W6).
- The inspector sits on the right or below the table by the device setting (`browser/config-keys` K11), and can be collapsed (R1222). Collapsing it does not change I2: a collapsed editor still flushes on row changes. Whether it is collapsed and which tab it shows are kept on the device, never in the collection.
- R1830: from a row change to editable fields at most 100 ms on a 150k collection.

## I9. Differences from Anki

| Difference | Anki | Meguri |
|---|---|---|
| Several rows selected | the editor area is hidden | the summary with the bulk actions (I8, R1221) |
| Failed save before a row change | no recovery path; the row changes | the row change does not happen (I4, `editor/edit-flows` W2) |
| Note changed by another writer | reloaded without saving first | `editor/edit-flows` W5 and W8 |
| Shortcuts of Previous, Next, Note, Card Info | Ctrl+P, Ctrl+N, Ctrl+Shift+N, Ctrl+Shift+I | the Product Contract's (`browser/command-map` M2, M5) |

## I10. Tests

`EditorPaneTests` (U221): I2's table with a fake selection and current-row publisher (one row, two rows, a selected row other than the current one, notes mode); I3 (Next keeps field 3, a click and the table's ↓ do not, a note with two fields gets none); I4's enabled states at the first and last row and with no current row; a failed flush that keeps the row on Next; I6 with fake change sets, each routed to the `editor/edit-flows` W5 path. `BrowseShortcutsUITests` (U218) drive I4's commands. `InspectorUITests` (U221) covers the panes together.
