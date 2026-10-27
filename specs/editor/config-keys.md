# Editing and week-start config keys

Spec: editor/config-keys
Requirements: R115, R138, R139, R140, R149, R798, R800, R803, R804, R852, R866, R867, R868, R1381, R840, R848
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/config-keys/

Four collection config keys that the Settings pages (U160) show and that the editor, the Add screen and the stats read: `pasteImagesAsPng`, `pasteStripsFormatting`, `addToCur` and `firstDayOfWeek`. Storage rules are `storage/config-table`; this file adds each key's type, default and meaning. K8 and K9 add the editor's keys `shrinkEditorImages`, `closeHTMLTags` and `customColorPickerPalette` (U192).

## K1. Rules shared by the four keys

- Each key holds one JSON scalar, not an object. A read is a typed read (`storage/config-table` C3): a missing key, a value not stored as a BLOB, bytes that are not valid JSON and a value of another type all read as the key's default. A read never writes.
- Types are strict. A boolean key reads only `true` and `false`; `1`, `0`, `"true"` and `null` read as the default. `firstDayOfWeek` reads only the integers of K5; `1.0`, `"1"` and `true` read as the default.
- A write replaces the whole value (`storage/config-table` C4), in compact form (C5): `true`, `false`, or an integer such as `1`. Member edits (C6) never apply to these keys. A write that gives the value already stored writes nothing.
- Meguri writes a key only when the user picks a value different from the one the read gives. Opening or closing Settings, the Add screen or the Stats screen writes nothing. A missing key stays missing until the user changes the setting; a stored value of another type keeps its bytes until then. Meguri never removes these keys: going back to the default writes the default value.
- Every write of these keys, from Settings or from the Stats menu, is written at once as one operation with no undo entry (R115), a skip-undo write of source `UserNonUndoable` (`engine/undo` M2, as `conformance/record-ops` W4): it keeps the earlier undo steps and empties the redo history (`engine/undo` A2, A11.1).
- The keys are in the collection's `config` table, so they follow the collection to every device: CloudKit sends them as config entities, where the later version wins (`sync/cloudkit-records` K15.8); a manual Anki transfer replaces the whole table (C9). They are not Meguri keys (C8) and keep Anki's names.

| Key | JSON type | Default | Read by |
|---|---|---|---|
| `pasteImagesAsPng` | boolean | `false` | editor paste, drop and photo import (K2) |
| `pasteStripsFormatting` | boolean | `false` | editor paste (K3) |
| `addToCur` | boolean | `true` | Add screen defaults (K4) |
| `firstDayOfWeek` | integer | `0` (Sunday) | Stats calendar, heatmap, widget snapshot (K5) |

## K2. `pasteImagesAsPng` (R138, R852, R868)

- `false`: bitmap image data that Meguri encodes itself is stored as JPEG with the extension `.jpg`. `true`: as PNG with the extension `.png`. The file name and encoder settings belong to the paste rules (R868).
- It applies to every pasted or dropped image that the paste order (R867) takes as image data, and to camera and photo-library images (R852, a Meguri addition: Anki has no photo import). An image that the paste order takes as a local file URL keeps its own bytes and extension.
- The key is read at each paste, so a change applies to the next paste without reopening the editor.

## K3. `pasteStripsFormatting` (R139, R866)

- It chooses the mode of a plain paste (⌘V): `false` keeps formatting, `true` strips it. A paste with Shift held inverts the choice: ⇧⌘V and ⌥⇧⌘V (the Edit menu's Paste and Match Style) use the other mode. The meaning of each mode is the paste rules' (R866 to R869).
- The key is read at each paste.
- Anki's label is "Paste without shift key strips formatting"; Meguri's Settings label is R139's.

## K4. `addToCur` (R140, R798, R800, R804)

`true` is Anki's "When adding, default to current deck"; `false` is "Change deck depending on note type". A new collection stores `true` (`storage/new-collection` B4), and a missing or unreadable key reads `true`.

### K4.1 The starting deck and note type

When the Add screen opens with a blank note (R803), it asks the core for its starting deck and note type. The input is the review deck: the deck of the card that a review screen of this collection is showing when the defaults are requested (on open, and on a reopen with a blank note, R803), however the Add screen was opened. It is the card's `odid` when that is not 0 and its `did` otherwise. With no card showing, there is no review deck.

Two helper rules:

- **Current deck for adding.** The deck that `curDeck` names (read as `decks/decks` D11 reads it, 1 when missing or unreadable), when that deck exists and is a normal deck. Otherwise the review deck, when one was given and it exists. Otherwise the default deck (id 1). Unlike D11, a `curDeck` that names a missing or filtered deck falls to the review deck before the default deck.
- **Current note type for adding.** The note type that `curModel` names, when that key reads as an integer (`storage/config-table` C3: a string such as `"1600000000000"` or a float such as `1.6e12` does not) and that note type exists. Otherwise the first note type in name order (the `unicase` collation of `storage/schema-18`). Decided 2026-10-27: name order, so the fallback does not depend on SQLite's query plan; scenario `editor/config-keys/first-notetype-fallback` confirms Anki's pick. With no note type at all the request fails with `InvalidArgument` (`engine/op-model`), and the Add screen has nothing to add with (U195).

Then:

| `addToCur` | Deck | Note type |
|---|---|---|
| `true` | the current deck for adding | the note type that `_deck_<that deck's id>_lastNotetype` names, when that key reads as an integer and the note type exists; otherwise the current note type for adding |
| `false` | the deck that `_nt_<that note type's id>_lastDeck` names, when that key reads as an integer and the deck exists and is normal; otherwise the current deck for adding | the current note type for adding |

So with `true` the deck decides the note type, and with `false` the note type decides the deck. Example: `addToCur` `true`, `curDeck` names a filtered deck, review card with `odid` of deck "Spanish": deck Spanish, note type Spanish's last note type, else `curModel`'s, else the first by name.

### K4.2 Changing the note type in the Add screen (R800)

- `true`: the selected deck stays.
- `false`: when the new note type's `_nt_<id>_lastDeck` names an existing normal deck, that deck is selected. Otherwise the selected deck stays (it is not reset to the current deck).

### K4.3 What `addToCur` does not change

It changes only these defaults. An add from the Add screen writes the keys of `notes/note-records` A4 (`_nt_<id>_lastDeck`, `_deck_<id>_lastNotetype`, `curModel`), whatever `addToCur` holds, and the deck the user picks is where the note goes (`notes/deck-placement-and-position` D2).

## K5. `firstDayOfWeek` (R149, R1381)

| Stored integer | Week starts on |
|---|---|
| `0` | Sunday |
| `1` | Monday |
| `5` | Friday |
| `6` | Saturday |

- Any other value, including the integers 2, 3, 4 and 7, reads as Sunday.
- Readers: the Stats calendar's week rows (R1381), the heatmap grid and the widget snapshot (R149, U100, U165, U227, U228). They take the week start as input and read it again after a change or a sync.
- Writers: the Stats calendar's "Week starts on" menu (R1381) and the Settings › Heatmap "First day of week" row (R149). Both offer exactly the four days above. Decided 2026-10-27: no other weekday is offered, since Anki reads any other value as Sunday.
- The `heatmap` key's own settings are a separate object (C6, R1337, R1338); its week start is this key, not a member of `heatmap`.

## K6. Differences from Anki

- Anki's Preferences screen compares all its values with those it had on open when it closes. With no change it writes nothing (and makes no undo step). When the user changed any preference on the screen, even one that is not an editing key (such as a backup setting), it writes all its editing keys (`addToCur`, `pasteImagesAsPng`, `pasteStripsFormatting`, `defaultSearchText`, `ignoreAccentsInSearch` and `renderLatex`), so a missing key is created with its default value. Meguri writes only the key the user changed (K1).
- Anki's Stats screen writes `firstDayOfWeek` together with its other two graph keys (`cardCountsSeparateInactive` and `futureDueShowBacklog`) whenever a graph setting is set, even to its current value (a click on the weekday already selected counts), so any such click creates the missing ones with their default values; a key that already holds the value sent is left as it is. Meguri writes only a changed key.
- In both cases the values that Anki and Meguri read are the same; only which keys exist and their `usn` and `mtime_secs` differ.
- Anki's Preferences screen saves as an undoable step named "Preferences"; its Stats screen's write is no undo step. Meguri records no undo step for any of these writes (K1, R115).

## K7. Scenarios

Requested from the oracle maintainer, in `conformance/scenarios/editor/config-keys/`; none is recorded yet. The ops they need do not exist yet. The model is `conformance/browser-config-ops` B1 and B2: an `editor_config_set` op (a whole-value set of one of the four keys, no undo entry) and an `editor_config` observation kind (the four typed reads with defaults). The K4 scenarios need no op of their own: they use the two observation kinds that `notes/add-defaults` D7 requests, `add_defaults` (K4.1) and `notetype_last_deck` (the per-note-type default deck that K4.2 applies), so one driver change serves both directories. K4.2's Add-screen behaviour (which deck stays selected) has no oracle counterpart; Meguri's Add screen model tests cover it (`notes/add-defaults` D7).

| Scenario | Covers |
|---|---|
| `bool-defaults` | K1, K2 to K4: each boolean key missing, stored `true`, `false`, `1`, `0`, `"true"`, `null` and as TEXT; the value read |
| `first-day-of-week` | K5: missing, each of 0, 1, 5, 6, then 2, 7, -1, `1.0`, `"1"`; the value read |
| `write-scalar` | K1: writing each key over a missing key, over the same value and over a value of another type; stored bytes, `usn`, `mtime_secs`, `col.mod` |
| `add-defaults-current-deck` | K4.1 with `true`: normal current deck with and without `_deck_<id>_lastNotetype`; that key naming a deleted note type; filtered current deck with a review deck; filtered current deck without one; `curDeck` naming a deleted deck |
| `add-defaults-by-notetype` | K4.1 with `false`: last deck normal, filtered, deleted and missing; `curModel` naming a deleted note type |
| `first-notetype-fallback` | K4.1: `curModel` missing, and stored as the string `"<id>"` of an existing note type, with note types whose name order differs from id order |
| `notetype-change` | K4.2: `notetype_last_deck` observations for note types whose `_nt_<id>_lastDeck` is usable, names a filtered deck, names a deleted deck and is missing, with `addToCur` `true` and `false` (the value does not depend on it) |

## K8. Editor option keys: `shrinkEditorImages` and `closeHTMLTags` (R848, U192)

| Key | JSON type | Default | Read by |
|---|---|---|---|
| `shrinkEditorImages` | boolean | `true` | image display in the editor (`editor/paste` P18, R878) |
| `closeHTMLTags` | boolean | `true` | typing in a field's HTML mode (`editor/formatting` F12) |

- K1's rules hold for these two keys as for its four: strict typed reads with the default for a missing or unreadable value, whole-value compact writes, a write only when the user picks a value different from the one read, never on open or close, no removal, one skip-undo write of source `UserNonUndoable` with no undo entry, and sync as collection config.
- The only writer is the editor's options menu (`editor/formatting` F12): choosing an item writes the opposite of the value read.
- The editor reads both when it loads a note (`editor/field-html` B3 `options`) and again after each write and after a change from a sync or another device (`engine/change-stream`), and passes the new values to every open editor (`editor/formatting` F13 `setOptions`).
- A new collection does not store them, so both read `true` until the user changes one.

## K9. `customColorPickerPalette` (R840, U192)

The custom colour swatches of the editor's colour pickers, shared with the image occlusion editor's fill colours (U206), which read and write the key by the same rules.

- **Shape.** A JSON array of colour strings, one per slot. The palette has 16 slots, as Anki's colour dialog has 16 custom colours; slot i is entry i.
- **Reading.** A typed read (`storage/config-table` C3). Entry i, for i from 0 to 15, holds a colour when it is a string `#` followed by 6 or 3 hexadecimal digits in either case; it reads as that colour, written as lower-case `#rrggbb` (`#F0a` reads `#ff00aa`). Any other entry, an entry `#ffffff`, and a missing entry read as an empty slot. Entries from index 16 on are not read. A missing key, a value that is not an array and an empty array read as 16 empty slots.
- **Shown.** The swatches are the colours of the non-empty slots, in slot order. Decided 2026-10-27: `#ffffff` counts as empty, because Anki writes every slot of its colour dialog and fills the slots the user never set with that dialog's default, white. White stays available in every picker.
- **Writing.** Only when the user adds or removes a swatch (`editor/formatting` F12), never when a colour is picked or applied, and never on open or close. Decided 2026-10-27: every add or remove reads the key again (C3) when it writes and applies the change to that value inside the same operation, never to the value the editor read when it opened, so a swatch that the image occlusion editor or another device added in between is kept; the swatches shown are then those of the value written. Adding puts the colour in the lowest free slot; when there is none, slots 1 to 15 move down to 0 to 14 (each with its entry) and the colour takes slot 15. A colour already shown is not added again. Removing makes every slot holding that colour empty; when none holds it, nothing is written. The value written is always 16 strings, as a whole-value compact write (C4, C5): each slot's colour as lower-case `#rrggbb`, `#ffffff` for an empty slot, and, in a kept slot, its stored string unchanged. Entries from index 16 on are dropped.
- **Kept slots.** Decided 2026-10-27 (data safety first): a slot whose entry is a string Meguri cannot read (for example `red`, `#12345` or `#aarrggbb`, which Anki may read and show, K10) is kept: it shows no swatch, an add does not use it, and a write keeps its string as stored. A free slot is one that is missing, holds a value that is not a string, or reads `#ffffff`. Anki's writes instead replace every slot with its colour dialog's colour for that slot, so an entry it could read (`red`) comes back as `#ff0000`, and one it could not read takes the dialog's colour (white unless set earlier in the session).
- The write is one skip-undo write of source `UserNonUndoable` (K1) and syncs as collection config (K1).

| Stored | Swatches shown |
|---|---|
| missing, `[]`, `{}`, `"#ff0000"` | none |
| `["#ff0000","#ffffff","#00FF00"]` | `#ff0000`, `#00ff00` |
| `["red","#12345","#abc",7]` | `#aabbcc` |
| 16 times `"#ffffff"` | none |

Example: with the stored value `["#ff0000","#ffffff","#00FF00"]`, adding `#0000ff` writes `["#ff0000","#0000ff","#00ff00","#ffffff",…]` with thirteen `"#ffffff"` at the end. With `["red","#12345","#abc",7]`, adding `#0000ff` writes `["red","#12345","#aabbcc","#0000ff",…]` with twelve `"#ffffff"` at the end (slots 0 and 1 kept, slot 3 free).

## K10. Differences from Anki for K8 and K9

- Anki's editor reads `shrinkEditorImages` and `closeHTMLTags` as a script value's truthiness after a fallback: `null` and a missing key take the default `true`; `0`, `""` and `false` read as off; `1`, `"false"`, arrays and objects read as on. Meguri reads only `true` and `false` and takes the default `true` for everything else (K1). With values Anki writes, which are always booleans, both read the same.
- Anki records toggling either option as an undoable config change; Meguri records no undo step (K8, R115).
- Anki writes all 16 palette slots each time its colour picker opens or a colour is chosen, from its colour dialog's state, and accepts any colour name its toolkit reads (such as `red` or `#aarrggbb`) when loading the palette. Meguri writes only when the user changes the palette and reads only the forms of K9; other string entries show no swatch and keep their bytes through Meguri's writes (K9 kept slots).
- Anki's image occlusion editor reads the palette in its own way: only when every entry is a string (otherwise it shows no swatches at all), it hides only entries that start with `#ffffff`, and it shows every other entry as stored, unparsed (`red`, `#ABC`, `#12345`). So for `["red","#12345","#abc",7]` it shows none, and for `["#ABC"]` it shows `#ABC`. Meguri's image occlusion editor (U206) applies K9's rules, as its editor does: `#aabbcc` in both cases. Writes do not differ by editor: Anki's image occlusion editor writes the same 16-slot dump as its editor's colour picker, and Meguri's writes as K9 says.
- The last text and highlight colours are not collection keys in either app: Anki keeps them in its profile, Meguri in device-local preferences (`editor/formatting` F7).

## K11. Scenarios for K8 and K9

Requested from the oracle maintainer, in `conformance/scenarios/editor/config-keys/`, with the ops K7 asks for (an `editor_config_set` that also covers these keys, and an `editor_config` observation that adds them); none is recorded yet.

| Scenario | Covers |
|---|---|
| `option-bool-defaults` | K8: each key missing, stored `true`, `false`, `1`, `0`, `"false"`, `null` and as TEXT; the value read |
| `option-write` | K8: writing each key over a missing key, over the same value and over a value of another type; stored bytes, `usn`, `mtime_secs`, `col.mod` |
| `palette-read` | K9: the table's stored values; the slots read |
| `palette-write` | K9: a Meguri write after an add and after a removal, also over `["red","#12345","#abc",7]` (kept slots); the stored bytes and that the oracle's config read returns the 16 strings |

In the quarantined editor image (`editor/field-html` E2), the oracle maintainer also records Anki's palette value after a colour is picked in a fresh profile (the default slots of K9) and after loading `["red","#12345","#abc"]`.
