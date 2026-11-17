# Flags and marked notes

Spec: tags/flags-and-marked
Requirements: R897, R898, R899, R900, R901, R902; supports R1173, R1192, R1195, R2020
Anki: 26.09.3
Scenarios: conformance/scenarios/tags/flags/, conformance/scenarios/tags/marked/

A card can carry one of seven colored flags, the user can rename the flags, and a note is marked by a tag. Tag terms are those of `tags/tags`; config writes follow `storage/config-table`.

## F1. Stored flag

- The flag is the low three bits of `cards.flags` (`model/row-records`): 0 no flag, 1 to 7 a flag. The other bits belong to other clients: every write keeps them.
- `flag:N` searches (U82) and every display read only the low three bits.

| Flag | Default name |
|---|---|
| 1 | Red |
| 2 | Orange |
| 3 | Green |
| 4 | Blue |
| 5 | Pink |
| 6 | Turquoise |
| 7 | Purple |

The default names are Anki's English names; other languages use Meguri's own translations. Colors are DesignSystem tokens, one per flag (R900, U9).

## F2. Setting a flag

Input: cards and a flag value. Listed cards that no longer exist are skipped.

1. Refuse a value above 7. Refusal kind `invalid_flag`; nothing changes.
2. For each listed card whose low three bits differ from the value: replace the low three bits with the value, keep the other bits, and write the card with `usn` -1. Its `mod` stays as it was: a flag change marks the card for sync but is not a modification time change (so Browse's card-modified column and study queues do not move). Writing the card row rebuilds its `data` text (`model/card-data-json#C4`).
3. A card that already has the value is not written. The count is the number of cards written. When any card was written, `col.mod` becomes `now_ms`.

`flags` 131 (0b1000_0011) set to 5 becomes 133 (0b1000_0101); set to 5 again, nothing is written; set to 0, 128.

The step is undoable, named "Set Flag" (R2020). Undo restores `flags`, `usn` and `col.mod`.

## F3. Toggling a flag (R898, R1195)

The review ⋯ menu, its keys (⌘1–⌘7 on Mac and iPad keyboards) and Browse's Flag menu toggle:

- There is a current card: the one on screen in review, the focused row's card in Browse. When its flag equals the chosen flag, F2 runs with 0 on every target card; otherwise with the chosen flag.
- The targets are the current card in review, and all selected cards in Browse (all cards of the selected notes in notes mode, R1181).
- Browse's "No flag" item runs F2 with 0.

So a card with flag 5 toggled to 5 ends with no flag; 0b1000_0011 toggled to 5 gives 0b1000_0101, and toggled to 5 again 0b1000_0000.

## F4. Flag names (R899, R1173)

The names live in the synced collection config key `flagLabels` (`storage/config-table`): a JSON object whose members `"1"` to `"7"` hold the custom names, for example `{"1":"Hard","4":"Look up"}`.

**Reading.** The shown name of flag N is the member `"N"` when it is a non-empty string, else the default name (F1). A missing key, or a value that is not an object, reads as `{}` (config-table C3). Members other than `"1"`–`"7"`, and non-string members, are kept by every write and never shown.

An empty member is a deliberate difference. Anki shows any existing member as stored, the empty string included, so `{"1":""}` gives flag 1 an empty name there. Meguri shows the default name instead, so that every menu item, sidebar row and accessibility label keeps a name. Anki's own rename never stores `""` (an empty rename restores the default, below), so only another client's value gives this case. Example: `{"1":"","2":"Hard"}` shows `Red` (Anki: an empty name) and `Hard`; renaming flag 1 to `Red` then writes nothing (step 3), where Anki writes `{"1":"Red","2":"Hard"}`.

**Renaming flag N to a text** (Browse's sidebar, inline):

1. Remove every `"` from the text (Anki's sidebar does this for every rename).
2. Empty result: restore the default name (below).
3. A result exactly equal to flag N's shown name: nothing is written.
4. Otherwise set member `"N"` to the result, keeping every other member and its value (config-table C6), and write the key (config-table C4: compact form, members sorted). A stored value that is not an object is replaced by `{"N": <result>}`.

**Restoring the default name** (the sidebar's Restore Default Name, or an empty rename): when member `"N"` exists, remove it, keeping the other members, and write the key; otherwise nothing is written. The key stays, as `{}` when no member is left.

Each rename and restore that writes is one undoable step named "Rename Flag" (R2020; U217). Anki writes the key without an undo step and keeps its undo history; the stored value is the same. The key travels with the whole config table on sync (config-table C9).

Every place that shows a flag uses the shown name (R900): the review menu, Browse's sidebar, flag column and menus, Card Info, search tokens and autocomplete.

## M1. The marked tag (R901)

A note is marked when it stores a tag equal to `marked` (`tags/tags#T1`, full folding): `marked`, `Marked` and `MARKED` all count; a descendant such as `marked::x` alone does not.

## M2. Marking and unmarking

- **Mark**: add the tag `marked` to the notes (`tags/tags#T10` Add). It takes `tags/tags#T3`'s spelling, so with a row `Marked`, or only a row `Marked::x`, the notes store `Marked` (Anki stores `marked` in the second case, T3's deviation). Undo step "Update Tag".
- **Unmark**: remove the tag `marked` from the notes (`tags/tags#T10` Remove). Every matching tag goes, so `Marked` and `marked::x` are removed together; `markedly` stays. Undo step "Remove Tag".
- **Toggle**: in review (the `*` key and the ⋯ menu), unmark the current note when it is marked (M1), else mark it. In Browse (⌘K, R1192), the focused row's note decides for all selected notes: unmark them all when it is marked, else mark them all.

## M3. Showing and finding marked notes (R902)

Review shows a star while the current note is marked (M1; U175). Browse and saved searches find marked notes with `tag:marked` (search area, U80–U82).

## F5. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `tags/flags/set` | F2 (bits kept, `mod` kept, `usn`, count, refusal) | awaits the `set_flag` driver op |
| `tags/marked/mark` | M2 Mark and Unmark through the tag operations | awaits the `add_note_tags` and `remove_note_tags` driver ops |

F3's toggles, F4's names and M2's toggles are decided by the UI layer in Anki, which the oracle does not run (it runs the collection library only). Meguri's tests cover them (U30 `tests/flags.rs`, `tests/tags.rs`; U217 for the sidebar): toggling a flag on and off, the Browse rule with a current card, `flagLabels` set to "Hard" for flag 1 then restored (the member removed, other members untouched), an equal name writing nothing, a `"` removed, a non-object value replaced, and unmarking a note tagged `Marked`. The empty-member case of F4 (`{"1":"","2":"Hard"}`, then a rename of flag 1 to `Red`) is still to be added to `tests/flags.rs`.
