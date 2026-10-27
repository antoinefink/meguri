# Add-screen defaults and the next note

Spec: notes/add-defaults
Requirements: R798, R799, R800, R801, R803, R806; supports R48, R80, R796, R805, R816, R817, R1174, R1214, R1785
Anki: 26.09.3
Scenarios: conformance/scenarios/notes/add-defaults/

The core services the Add screen (U195) calls for its starting deck and note type and for a note type's default deck (U191), and the Add-screen rules around them: when the screen asks, what a blank draft is, what happens to a selection that stops existing, and what the next note holds after an add. The rules that pick the deck and the note type are `editor/config-keys` K4; this file does not restate them. The keys an add writes are `notes/note-records` A4. The live check of a draft is `notes/field-check`; tag suggestions are `tags/complete`.

## D1. The starting defaults

A read-only query on the query lane (KTD9). It writes nothing: no config key, no `col.mod`.

- **Input.**
  - `review_card`, optional: the id of the card that a review screen of this collection shows when the request is made (`editor/config-keys` K4.1). The core reads that card's `odid` and `did` to get K4.1's review deck. A card id that names no card gives no review deck.
  - `preset_deck`, optional: the deck an entry point chose (D3). It counts only when it names an existing normal deck. Otherwise it is ignored.
- **Output.** `deck_id` and `notetype_id`.
- **Rule.** Without a usable `preset_deck`, the result is K4.1's table. With one:

  | `addToCur` | Deck | Note type |
  |---|---|---|
  | `true` | `preset_deck` | K4.1's `true` row for `preset_deck` in place of the current deck for adding: `_deck_<preset_deck>_lastNotetype` when it names an existing note type, otherwise the current note type for adding |
  | `false` | `preset_deck`. The note type's last deck is not read | the current note type for adding |

  The deck column is R48: Add Note from a deck preselects that deck whatever the default-deck preference is (also R80 and R1174). Decided 2026-10-27: the note type column, which no requirement fixes, follows K4.1 for that deck. Anki has no deck-preset request and no oracle can show this table, so Meguri's tests cover it.
- **Errors.** With no note type, `InvalidArgument` (K4.1). When every deck candidate of K4.1 fails and the default deck (id 1) does not exist either, `NotFound` (`engine/op-model`). Anki fails the same way. Only a damaged collection lacks deck 1 (`decks/decks` D8 keeps it). When both apply, the mode picks the error, as in Anki: with `addToCur` `true` the deck is resolved first, so the result is `NotFound`; with `false` the note type is resolved first, so it is `InvalidArgument`. A usable `preset_deck` means the deck cannot fail, so only `InvalidArgument` remains.
- **A filtered result.** K4.1 takes the review deck when it exists, without checking its kind, so only a card with `odid` 0 in a filtered deck makes the result a filtered deck. Check Database repairs that card. The Add screen then applies D5 to the deck.

Examples (with `addToCur` `true`, `curDeck` naming the filtered deck `Cram`, and `_deck_<Spanish>_lastNotetype` naming `Vocab`):

| Request | Result |
|---|---|
| `review_card` a card with `did` Cram and `odid` Spanish | Spanish, Vocab |
| no `review_card` | Default, Default's last note type, else `curModel`'s, else the first by name |
| `review_card` an id that names no card | as with no `review_card` |
| `preset_deck` French, which has no last note type | French, `curModel`'s note type |
| `preset_deck` Cram (filtered) | ignored: as with no `preset_deck` |

## D2. The default deck of a note type

A read-only query on the query lane. It writes nothing.

- **Input.** A note type id.
- **Output.** A deck id, or none. It is the deck that `_nt_<id>_lastDeck` names, when that key reads as an integer and names an existing normal deck. Otherwise it is none: the key is missing or unreadable (`storage/config-table` C3), it names a missing deck, or it names a filtered deck. The note type is not looked up. A key left for a note type id that names no note type (after a sync or a raw edit) still gives its deck, as in Anki. It is no error.
- **Not read.** `addToCur`. The Add screen calls D2 only when `addToCur` is `false` and applies its result as K4.2 says. A none keeps the selected deck. Anki's request returns deck id 0 for none.
- **Users.** Only a note type change in the Add screen (K4.2, R800). Create Copy and prefilled drafts set their own deck (D3).

## D3. When the Add screen asks (R803)

| Entry | Note type | Deck |
|---|---|---|
| open, blank draft: Home toolbar "+", A key, review ⋯ Add, Browse's Add Notes (R796, R1214) | D1 | D1 |
| open from a deck: deck page "+", a deck's ⋯ Add Note in the deck list (R48), the empty-deck page's Add Note (R80), a deck's Add Note in Browse's sidebar (R1174) | D1 with `preset_deck` | D1 with `preset_deck` |
| Create Copy (R817, R1214) | the note's | the card's `odid` when not 0, otherwise its `did` (D5 applies when that deck is missing or filtered) |
| a prefilled draft (`guiAddCards`, U146, R1784) | the draft's | the draft's |

- The two open rows: the Add screen asks when it opens and when it is opened again while its draft is blank (D4). Each request carries the `review_card` of that moment. A draft that is not blank is kept as it is, with its note type and deck, and a `preset_deck` from the new entry is ignored (R803).
- The two replacing rows (Create Copy and a prefilled draft) never ask D1. When the draft is unchanged since the last add (D4), they replace it directly: note type, deck, fields and tags. Otherwise the user decides first. Create Copy asks R816's "Discard current input?": Discard replaces the draft, Keep Editing keeps it and drops the copy. A prefilled draft asks R1785's Replace draft / Keep draft prompt (U146). Decided 2026-10-27, for data safety; Anki replaces silently (D8).
- The Add screen does not ask again after an add (D6), after a note type change (K4.2) or when another operation changes decks or note types (D5).

## D4. Blank and unchanged drafts (R803, R816)

- A field value is **empty** when it is empty after every `<br>` is removed and Unicode White_Space is trimmed from both ends. `&nbsp;`, `<br/>`, `<div></div>` and an `<img>` are not empty.
- A draft is **blank** when every field is empty. Tags are not looked at. D3 uses this rule.
- A draft is **unchanged since the last add** when every field is empty, or is sticky and, after the same removal and trim, equals that field's value in the last added note. Tags are not looked at. Before the first add, after a note type change (K4.2 or D5) and after Create Copy or a prefilled draft replaces the draft (D3), there is no last added note, so the draft must be blank. The close prompt of R816 shows when the draft is not unchanged (U195).
- Anki trims with its own white-space set, which also holds U+001C to U+001F. Decided 2026-10-27: Meguri trims Unicode White_Space only. A draft holding only those characters is not blank in Meguri. A save removes them anyway (`text/checksum-and-sort-field` C1).

## D5. A selection that stops existing

Another operation (a deck or note type deletion, a deck turned filtered, an incoming sync) can remove what the Add screen has selected. The Add screen checks its selection after each such change and before an add:

- a selected note type that no longer exists is replaced by the first note type in name order (K4.1's fallback). The draft's content is mapped to it as for a note type change (R802), and the last added note is forgotten (D4). D2 is not consulted: the selected deck stays;
- a selected deck that no longer exists, or that is now filtered, is replaced by the default deck (id 1).

A change that leaves the selection valid changes nothing: the draft, its note type and deck, and the last added note all stay. Neither replacement writes anything. Decided 2026-10-27, so that the user's selection is not changed under them more than the change forces; Anki differs in two ways (D8).

## D6. After an add (R801, R806)

- **What the core writes.** An Add-screen add sends `update_add_defaults` true. The add then writes the three keys of `notes/note-records` A4 in the same operation and undo step. Undo removes them with the note (A5). This covers every add made in the Add screen: a blank open, a deck preset, Create Copy and a draft prefilled by `guiAddCards`. Adds by the local servers (U137, U139, U141) and importers send false and write none (R801). A refused add, a declined missing-cloze confirmation (R809 to R811) or a failed add writes nothing. No add writes `curDeck`.
- **The selection.** The note type and the deck stay selected. The Add screen does not ask D1 again.
- **The next draft.** For each field of the selected note type (its options as they are now), a field whose `sticky` option is on (`notetypes/records` N2, R805) keeps the value the draft held when the add ran. That value is the draft's own, before save normalization. Every other field is empty. All the draft's tags carry over unchanged, sticky or not. Focus returns to the first field (U195).
- **The next defaults.** A later D1 or D2 request reads the keys this add wrote. Example: with `addToCur` `false`, add a `Vocab` note to `Spanish::Mining`. A later D1 then gives `Vocab` (from `curModel`) and `Spanish::Mining` (from `_nt_<Vocab>_lastDeck`). With `addToCur` `true` and `curDeck` naming `Spanish::Mining`, it gives that deck and `Vocab` (from `_deck_<Spanish::Mining>_lastNotetype`).

## D7. Scenarios

Requested from the oracle maintainer. None is recorded yet: the oracle driver has no op or observation kind for D1 or D2. The K4 scenarios already requested in `editor/config-keys` K7 (`add-defaults-current-deck`, `add-defaults-by-notetype`, `first-notetype-fallback`, `notetype-change`) need the same two observation kinds, so one driver change serves both directories.

Requested observation kinds, modeled on `fields_check` (`conformance/note-ops` N4). Both read the collection and change nothing:

- `add_defaults`: request `{review_card?: #cN}`. Value `{deck: <deck name>, notetype: <note type name>}`, or the error kind for D1's errors. Oracle: the backend's defaults request, with the home deck of the card (`odid` when not 0, else `did`), or 0 without a card. A `#cN` that names a card the scenario has deleted is sent as home deck 0 (no review deck), as D1 treats it. `preset_deck` has no oracle counterpart and is not part of the request.
- `notetype_last_deck`: request `{notetype: <name>}` or `{notetype_id: <integer>}`; the raw id form names a note type that does not exist. Value `{deck: <deck name>}` or `{deck: null}`. Oracle: the backend's default-deck request for that note type id, with 0 read as null.

| Scenario (`notes/add-defaults/`) | Covers |
|---|---|
| `last-deck` | D2: `_nt_<id>_lastDeck` missing, naming a normal deck, a filtered deck and a deleted deck, stored as `"5"`, as `5.0` and as TEXT; a leftover key for a note type id with no note type (set by `db_exec`, requested by `notetype_id`), which still gives its deck; the same results with `addToCur` `true` and `false` |
| `review-card` | D1: a review card in a filtered deck (`odid` set), in a normal deck, and a deleted card; each with `curDeck` naming a filtered deck and a deleted deck |
| `no-notetype` | D1: every note type removed by `db_exec`, then `InvalidArgument` |
| `missing-default-deck` | D1: deck 1's row deleted by `db_exec`, `curDeck` naming a filtered deck, no review card, then `NotFound` |
| `after-add` | D6: `add_note` to `Spanish::Mining` with `Vocab`, then `add_defaults` with each value of `addToCur` and `notetype_last_deck` for `Vocab` |

Meguri's tests cover what no oracle shows: `preset_deck` (D1), D3 to D5 in the Add screen model, sticky carry-over (D6), and `update_add_defaults` false (`notes/note-records` A4).

## D8. Differences from Anki

The Add screen of Anki 26.09.3 differs from D3 and D5 in these ways. None is visible to a scenario, which calls D1 and D2 directly, so none has a `conformance/deviations.yaml` entry.

- **Another screen's deck or note type change.** After any operation from another screen that changes decks or note types, Anki rebuilds the draft in place: the same note type and deck, the fields mapped as for a note type change, and the last added note forgotten, so the close prompt then shows for a sticky field that still holds its value. It requests the starting defaults but uses nothing from them. Meguri keeps the last added note unless the note type is replaced (D5).
- **A replaced note type.** When Anki replaces a missing note type with the first by name, it treats that as a note type change: with `addToCur` `false` it also applies that note type's last deck (D2). Meguri keeps the selected deck (D5).
- **Create Copy and prefilled drafts.** Anki replaces a draft that is not blank without asking. Meguri asks first (D3).
- **Browse's sidebar Add.** Anki presets the deck and then selects the note type of Browse's current note, which with `addToCur` `false` can apply that note type's last deck over the preset. Meguri uses D1 with `preset_deck` (R48, R1174).
