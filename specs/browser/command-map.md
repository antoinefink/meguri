# Browse commands and shortcuts

Spec: browser/command-map
Requirements: R1231; supports R200, R217, R218, R225, R238, R260, R261, R262, R263, R265, R266, R267, R268, R269, R270, R271, R272, R273, R275, R282, R1186, R1225, R1226, R1227, R1228, R1229, R1230
Anki: 26.09.3 for M2's Anki columns and M3 (the desktop browser's menus, previewer and sidebar)
Scenarios: none (shortcut catalog; M6 lists the tests)

Anki's browser commands with their menus and shortcuts, set against the shortcuts the Product Contract gives Meguri's Browse (U218's command catalog), so that R1231's "Anki's mapping (Ctrl→⌘, Alt→⌥)" can be checked row by row and every place where Meguri differs is named. The Product Contract's requirements are the authority for Meguri's keys; this file adds no binding of its own. Meguri's menu placement is U218's (plan); M2 lists Anki's. Command names are described, not quoted: menu titles are Meguri's wording (`browser/browse-strings` W6).

## M1. Notation

The Anki column is Anki's key sequence as its desktop app declares it. On macOS Anki's toolkit reads Ctrl as ⌘, Alt as ⌥ and Meta as ⌃; that reading is "Anki on Mac". The Meguri column is the requirement's key; "none" means no requirement gives Browse one.

## M2. Commands

| Anki menu | Command | Anki | Anki on Mac | Meguri | Requirement |
|---|---|---|---|---|---|
| Edit | Undo, Redo | Ctrl+Z, Ctrl+Shift+Z | ⌘Z, ⇧⌘Z | ⌘Z, ⇧⌘Z | R1225 |
| Edit | Select All | Ctrl+Alt+A | ⌥⌘A | ⌥⌘A, and ⌘A with the table focused | R1225, R263 |
| Edit | Select Notes | none | | none | R1153 |
| Edit | Invert Selection | Ctrl+Alt+S | ⌥⌘S | ⌥⌘S | R1225, R263 |
| none | Copy rows | none (no such command) | | ⌘C with the table focused | R1225, R1215 |
| Edit | Create Filtered Deck | Ctrl+G, Ctrl+Alt+G | ⌘G, ⌥⌘G | reserved, unbound | R1230 |
| Edit (Mac only) | Close window | Ctrl+W | ⌘W | none (no Browse window, R1232) | |
| View | Toggle Cards/Notes | Ctrl+Alt+T | ⌥⌘T | ⌥⌘T | R1226 |
| View | Full Screen | none | | the app's ⌃⌘F | R217 |
| View | Toggle Sidebar | none | | ⌃⌘S | R1226, R217 |
| View | Zoom In, Zoom Out (the editor) | none | | none; the app's card zoom ⌘+ and ⌘− scale the preview's card | R218, R143 |
| View | Reset Zoom (the editor) | Ctrl+0 | ⌘0 | none; the app's ⌘0 resets card zoom, which the preview shows | R218, R143 |
| View | Layout (auto, vertical, horizontal) | none | | none; the inspector position is a device setting (R1222) | |
| Cards | Card Info | Ctrl+Shift+I | ⇧⌘I | ⌥⌘I | R1226, R272 |
| none (a window key and the editor's Preview button) | Preview | Ctrl+Shift+P | ⇧⌘P | ⇧⌘P | R1226, R273 |
| Notes | Add Notes | Ctrl+E | ⌘E | the app's Add command (no Browse binding in R1227) | R1214 |
| Notes | Create Copy | Ctrl+Alt+E | ⌥⌘E | ⌥⌘E | R1227, R273 |
| Notes | Export Notes | Ctrl+Shift+E | ⇧⌘E | ⇧⌘E | R1227, R273 |
| Notes | Add Tags, Remove Tags | Ctrl+Shift+A, Ctrl+Alt+Shift+A | ⇧⌘A, ⌥⇧⌘A | ⇧⌘A, ⌥⇧⌘A | R1227, R268 |
| Notes | Clear Unused Tags | none | | none | R1191 |
| Notes | Toggle Mark | Ctrl+K | ⌘K | ⌘K | R1227, R269 |
| Notes | Change Note Type | Ctrl+Shift+M | ⇧⌘M | ⇧⌘M | R1227, R265 |
| Notes | Find Duplicates | none | | none | |
| Notes | Find and Replace | Ctrl+Alt+F | ⌥⌘F | ⌥⌘F | R1227, R266 |
| Notes | Manage Note Types | none | | the app's Note Types ⇧⌘N | R282 |
| Notes | Delete | Ctrl+Del | ⌘ with the forward-delete key | ⌘⌫, only with the table focused | R1227, R271 |
| Cards | Change Deck | Ctrl+D | ⌘D | ⌘D | R1228, R265 |
| Cards | Set Due Date | Ctrl+Shift+D | ⇧⌘D | ⇧⌘D | R1228, R267 |
| Cards | Grade Now | Ctrl+Shift+G | ⇧⌘G | ⇧⌘G | R1228 |
| Cards | Reset (Forget) | Ctrl+Alt+N | ⌥⌘N | ⌥⌘N | R1228, R267, R238 |
| Cards | Reposition | Ctrl+Shift+S | ⇧⌘S | reserved, unbound | R1230 |
| Cards | Toggle Suspend, Toggle Bury | Ctrl+J, Ctrl+Shift+J | ⌘J, ⇧⌘J | ⌘J, ⇧⌘J | R1228, R269 |
| Cards | Flags 1 to 7 | Ctrl+1 to Ctrl+7 | ⌘1 to ⌘7 | ⌘1 to ⌘7, before any app-level ⌘-number while Browse has focus | R1228, R270 |
| Go | Find | Ctrl+F | ⌘F | ⌘F (focus and select the search text) | R1229, R260 |
| Go | Sidebar Filter | Ctrl+Shift+F | ⇧⌘F | ⇧⌘F | R1229, R260 |
| Go | Sidebar (focus the sidebar) | Ctrl+Shift+R | ⇧⌘R | unbound | R1229 |
| Go | Note (focus the editor) | Ctrl+Shift+N | ⇧⌘N | ⌥⌘L | R261 |
| Go | Card List (focus the table) | Ctrl+Shift+L | ⇧⌘L | ⇧⌘L | R1229, R261 |
| Go | First, Last | Home, End | Home, End | Home, End (M4) | R1229, R263 |
| Go | First, Last extending the selection | Shift+Home, Shift+End (window keys, no menu item) | | Shift+Home, Shift+End (M4) | R1229 |
| Go | Previous, Next | Ctrl+P, Ctrl+N | ⌘P, ⌘N | ⌃⌘↑, ⌃⌘↓ anywhere in Browse | R1229, R262 |
| Help | Help page | F1 | | none | |

- With the table focused, ↑ and ↓ move its current row (R262). That is the table's own navigation, not the Previous and Next command: it does not keep the editor's focused field (`browser/inspector` I3), as in Anki.
- The commands' behaviour is `browser/inspector` I4 for the Go menu, `browser/bulk-actions` for the Notes and Cards menus, `browser/find-replace` for Find and Replace, `browser/preview` for Preview.
- R1229 lists the sidebar's and the note editor's focus commands as unbound, and R261 binds ⌥⌘L to the editor pane; the Browse screen contract (`docs/design/screen-inventory.yaml` `browse.browser`, Keyboard) follows R261. U218's catalog does too.

### M2.1 Previewer (R275)

Keys of the preview while it has focus (`browser/preview` P3):

| Key | Anki | Meguri |
|---|---|---|
| ← / → | previous / next (`browser/preview` P3) | same |
| Return | next (the default button) | same |
| R | replay | same |
| B | Both sides | same |
| Esc, Ctrl+Shift+P (⇧⌘P) | close | same |

### M2.2 Sidebar

Anki binds its two sidebar tools to Alt+1 (Search) and Alt+2 (Select); Meguri's keys for them, and for the sidebar's Rename command, are an open point (`browser/sidebar` S15). In the list, Return runs the item and Delete deletes (`browser/sidebar` S11; Meguri's key is ⌫).

## M3. Enablement in Anki

For comparison with `browser/bulk-actions` B1 (Meguri's rule, which U218's catalog implements):

- Disabled with an empty table: Select All, Invert Selection, First, Last.
- Disabled with no selection: Select Notes, Export Notes, Add Tags, Remove Tags, Toggle Mark, Change Note Type, Delete, Change Deck, Set Due Date, Reset, Reposition, Toggle Suspend, Toggle Bury, and the Flag submenu.
- Disabled with no current row: Create Copy and Card Info. Disabled with no row above, or below, the current row: Previous, and Next.
- Never disabled: Grade Now, which with no selection shows a "no selection" hint and does nothing. Preview has no menu item; its key opens the previewer only when the editor shows a note.
- Delete Notes acts only when the table has the keyboard focus, in the menu as well as by key; Meguri follows that (R1227).
- Toggle Suspend, Toggle Bury, Toggle Mark and the flag items show the current item's state as a checkmark (B2).

Meguri: Create Copy is disabled with no current row and Grade Now with no selection (B1); Previous and Next as Anki (`browser/inspector` I4); Preview is disabled with no shown card (`browser/preview` P2). Card Info stays enabled with no current row: its tab shows "(No card to display.)" (`cardInfo.card` screen contract).

## M4. Meguri context rules

- While Browse has focus, ⌥⌘N is Reset for the selection and File › New Deck keeps its menu item without the shortcut (R238, AE10).
- ⌘1 to ⌘7 are Browse's flags while Browse has focus (R1228); outside Browse they keep their app-level meaning.
- ⇧⌘S, ⌘G and ⌥⌘G are reserved for Reposition and Create Filtered Deck and stay unbound (R1230).
- **Text fields keep their keys.** While a text field has focus (the search field, the sidebar's filter box, an editor field, an inline rename), the keys a text field uses keep their text meaning there: ↑, ↓, ←, →, Home, End and their Shift forms, ⌘A, ⌘C, Return and ⌫. Home, End, Shift+Home and Shift+End act on the table in every other focus state of Browse; ⌘A, ⌘C, ↑, ↓ and ⌘⌫ only with the table focused (R262, R263, R1225, R1227); ←, →, Return, R and B only with the preview focused (M2.1). ⌃⌘↑, ⌃⌘↓ and the other modified keys of M2 act in every focus state of Browse (R262). This rule is Meguri's (platform text editing); M4 does not claim Anki's behaviour for it.
- Every shortcut appears in the Mac menu bar and in the iPadOS menu and keyboard overlay (R1231, R278); no two enabled Browse commands share a key in one focus state (R200).

## M5. Differences from Anki's mapping

| Command | Anki on Mac | Meguri | Why |
|---|---|---|---|
| Card Info | ⇧⌘I | ⌥⌘I | ⇧⌘I is Import in Meguri's single window (R272) |
| Note (focus the editor) | ⇧⌘N | ⌥⌘L | R261; ⇧⌘N is the app's Note Types (R282) |
| Previous, Next | ⌘P, ⌘N | ⌃⌘↑, ⌃⌘↓ | ⌘N is Add Note (R262) |
| Sidebar focus | ⇧⌘R | unbound | R1229 |
| Toggle Sidebar | none | ⌃⌘S | R1226, R217 |
| Copy rows | none | ⌘C | R1215 |
| Delete | ⌘ with forward delete | ⌘⌫ | the Mac keyboard's delete key (R271) |
| Reposition, Create Filtered Deck | ⇧⌘S, ⌘G, ⌥⌘G | unbound | deferred features (R1230) |
| Reset Zoom | ⌘0 (the editor's zoom) | ⌘0 is the app's card zoom | R218 |

Every other row of M2 with a key on both sides is Anki's key read as M1 says.

## M6. Tests

`BrowseCommandCatalogTests` (U218) asserts M2's Meguri column and M4, with each command's focus states; `BrowseShortcutsUITests` drives them on Mac and with an iPad keyboard.
