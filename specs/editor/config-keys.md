# Editing and week-start config keys

Spec: editor/config-keys
Requirements: R115, R138, R139, R140, R149, R798, R800, R803, R804, R852, R866, R867, R868, R1381
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/config-keys/

Four collection config keys that the Settings pages (U160) show and that the editor, the Add screen and the stats read: `pasteImagesAsPng`, `pasteStripsFormatting`, `addToCur` and `firstDayOfWeek`. Storage rules are `storage/config-table`; this file adds each key's type, default and meaning.

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
- **Current note type for adding.** The note type that `curModel` names, when it exists. Otherwise the first note type in name order (the `unicase` collation of `storage/schema-18`). Decided 2026-10-27: name order, so the fallback does not depend on SQLite's query plan; scenario `editor/config-keys/first-notetype-fallback` confirms Anki's pick. With no note type at all the request fails with `InvalidArgument` (`engine/op-model`), and the Add screen has nothing to add with (U195).

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

- Anki's Preferences screen writes all its editing keys (`addToCur`, `pasteImagesAsPng`, `pasteStripsFormatting`, `defaultSearchText`, `ignoreAccentsInSearch` and `renderLatex`) each time it closes, so a missing key is created with its default value. Meguri writes only the key the user changed (K1).
- Anki's Stats screen writes `firstDayOfWeek` together with its other two graph keys whenever any graph setting changes, so changing one creates the others. Meguri writes only the changed key.
- In both cases the values that Anki and Meguri read are the same; only which keys exist and their `usn` and `mtime_secs` differ.
- Anki's Preferences screen saves as an undoable step named "Preferences"; its Stats screen's write is no undo step. Meguri records no undo step for any of these writes (K1, R115).

## K7. Scenarios

Requested from the oracle maintainer, in `conformance/scenarios/editor/config-keys/`; none is recorded yet. The ops they need do not exist yet. The model is `conformance/browser-config-ops` B1 and B2: an `editor_config_set` op (a whole-value set of one of the four keys, no undo entry) and an `editor_config` observation kind (the four typed reads with defaults), plus separate ops for the Add-screen defaults (K4.1, K4.2) and for the per-note-type default deck.

| Scenario | Covers |
|---|---|
| `bool-defaults` | K1, K2 to K4: each boolean key missing, stored `true`, `false`, `1`, `0`, `"true"`, `null` and as TEXT; the value read |
| `first-day-of-week` | K5: missing, each of 0, 1, 5, 6, then 2, 7, -1, `1.0`, `"1"`; the value read |
| `write-scalar` | K1: writing each key over a missing key, over the same value and over a value of another type; stored bytes, `usn`, `mtime_secs`, `col.mod` |
| `add-defaults-current-deck` | K4.1 with `true`: normal current deck with and without `_deck_<id>_lastNotetype`; that key naming a deleted note type; filtered current deck with a review deck; filtered current deck without one; `curDeck` naming a deleted deck |
| `add-defaults-by-notetype` | K4.1 with `false`: last deck normal, filtered, deleted and missing; `curModel` naming a deleted note type |
| `first-notetype-fallback` | K4.1: `curModel` missing, with note types whose name order differs from id order |
| `notetype-change` | K4.2: the deck after a note type change, both values of `addToCur`, with and without a usable last deck |
