# Note records and note operations

Spec: notes/note-records
Requirements: R769, R770, R771, R772, R773, R774, R775, R776, R777, R778, R779, R780, R781, R801 (core side), R1457, R1458, R1594; supports R823
Anki: 26.09.3
Scenarios: conformance/scenarios/notes/records/, conformance/scenarios/notes/save/, conformance/scenarios/notes/fields/

How Meguri stores a note and what adding, updating, repairing and deleting a note write. The pieces this spec composes are specified elsewhere: the row layout (`model/row-records#R3`), the GUID text (`text/guid`), the save normalization, sort field and checksum (`text/checksum-and-sort-field` C1 to C4), field emptiness (`text/field-emptiness`), ids, stamps and skipped writes (`storage/ids-usn-mtime`), graves (`storage/graves`), config values (`storage/config-table`), a note's tags (`tags/tags#T4`) and undo (`engine/undo`). Which cards a save creates is `notes/card-generation`; where they go and their positions is `notes/deck-placement-and-position`.

Stamping terms (`usn` -1, `now_s`, `now_ms`, `col.mod`) are those of `storage/ids-usn-mtime`.

## N1. The note row

| Column | Content |
|---|---|
| `id` | allocated when the note is added (`storage/ids-usn-mtime` I2, I3); never changes |
| `guid` | A1; never changes (R771) |
| `mid` | the note type's id; changes only through Change Note Type (U35) |
| `mod`, `usn` | stamps (A2, U3) |
| `tags` | `tags/tags#T4` |
| `flds` | the field values in the note type's field order, joined by U+001F (R769). A saved note has exactly as many values as its note type has fields. |
| `sfld` | the sort-field text of the saved fields, bound as TEXT (`text/checksum-and-sort-field` C2, C3) |
| `csum` | the checksum of the saved first field (C4) |
| `flags`, `data` | 0 and the empty string for a new note; an update never writes them, so a stored value stays (R778) |

Reading splits `flds` at every U+001F, so a stored value with another count than the note type's is read with that count; only F1 changes it.

## N2. Field normalization on save

Every save that writes a note row normalizes every field value of the note, the ones the caller did not change included, by `text/checksum-and-sort-field` C1: control characters other than TAB and LF are removed, then NFC is applied when `normalize_note_text` is on (R775). The key is read on each save: a missing key, or a value that is not a JSON boolean, reads as `true`; `false` turns NFC off. So a field stored in decomposed form while the key was off becomes NFC at the note's next real save once the key is on again (scenario `notes/save/normalize-off`).

`sfld` and `csum` are computed from the normalized values (C2 to C4), so with the key off they are computed from the decomposed text: `cafe` U+0301 stores `csum` 922075179, `café` (U+00E9) 4096017706.

## A1. Adding a note: input

- The note type, by id.
- The field values, exactly one per field of the note type, in field order.
- The tags, a list of strings.
- The deck the caller chose (the Add screen's deck, an importer's or a local server's deck), by id. It need not exist and may be a filtered deck: `notes/deck-placement-and-position` D2 decides where the cards go.
- Optionally a GUID. When given it is stored byte for byte (U120 keeps package GUIDs, U139 pre-generates one); otherwise a new one is drawn (`text/guid` G3).
- `update_add_defaults`: true only for adds from the Add screen (U195). See A4.

Refusals, which write nothing: an unknown note type and a field count different from the note type's are invalid arguments (`InvalidArgument`, `engine/op-model`). No scenario reaches them, since Anki's callers cannot build such a request. Adding never checks for an empty first field, cloze problems or duplicates: those are `notes/field-check`, and only the Add screen acts on them (R808 to R812).

## A2. Adding a note: the row

The new row holds:

- `flds`: the values normalized by N2;
- `tags`: the list stored as `tags/tags#T4` says, registering new tags;
- `sfld`, `csum`: from the normalized values;
- `mod` `now_s`, `usn` -1, `flags` 0, `data` the empty string;
- `guid` from A1;
- `id`: allocated by `storage/ids-usn-mtime` I2 with I3's cap, before any of the note's cards.

Then the note's cards are created (`notes/card-generation` G1, G6) and placed (`notes/deck-placement-and-position`). The operation sets `col.mod` to `now_ms` once. It returns the new note's id and the number of cards it created (at least 1).

Examples (scenario `notes/records/add`):

- Fields `<b>食べる</b>`, `たべる`, `to eat` CR LF `line two` U+0001 U+0007 TAB `tab` with tags `verb`, `JLPT::N5`, `Verb` store `flds` `<b>食べる</b>` U+001F `たべる` U+001F `to eat` LF `line two` TAB `tab`, `tags` ` JLPT::N5 verb `, `sfld` `食べる` and `csum` 567984260.
- Fields `cafe` U+0301, (empty), U+212B store `café` (U+00E9), (empty), `Å` (U+00C5).
- A note type whose sort field is its second field stores `sfld` 7, an integer, for a second field ` 007 `, and `csum` from the first field `<i>seven</i>`; a second field `12abc` stays text.

## A3. Adding a note: the position counter

Adding moves the collection's new-card position counter at most once per note (`notes/deck-placement-and-position` P1, P2): four one-card notes added to a new collection take positions 1 to 4 and leave `nextPos` at 5.

## A4. Add-screen defaults (R801)

When `update_add_defaults` is true, the add also writes three collection config keys, in the same operation and undo step:

| Key | Value (a JSON integer) |
|---|---|
| `_nt_<note type id>_lastDeck` | the deck id of A1, as given: also when that deck is missing or filtered and the cards went elsewhere |
| `_deck_<deck id of A1>_lastNotetype` | the note type id |
| `curModel` | the note type id |

Ids are written in decimal in the key names. Each write follows `storage/config-table` C4: a key that already holds the value is not written again.

Anki writes the three keys on every add, whichever program adds. Meguri's local servers (U137, U139, U141) add with `update_add_defaults` false, so the user's Add-screen defaults do not follow an external tool (R801). The scenario op `add_note` is an Add-screen add (`specs/conformance/note-ops.md` N1).

## A5. Undo and bulk adds

- An add is one undo step, "Add Note" (`engine/undo`). Undo removes the note, its cards, the tags it registered and its config writes; it adds no grave.
- A bulk add takes a list of A1 inputs and runs them in list order as one operation and one undo step, "Add Note". Each note is handled exactly as a single add: its own id, its own position draw (P2), its own A4 writes when the flag is set. It returns the new note ids in list order. Importers (U120) and local servers (U139) use it.

## U1. Updating a note: input

- The note id.
- The new field values, exactly one per field of the note's note type.
- The new tags, a list.

The GUID, the note type and the id never change through an update (R771). Refusals, which write nothing: a field count different from the note type's is an invalid argument; a note that no longer exists is an error the caller can tell apart from that one, since the edit flows react to a note deleted meanwhile (U196).

## U2. Saves that change nothing

The update builds the row it would store: fields normalized (N2) and tags as `tags/tags#T4` would store them against the registry as it is. When that row's `flds` and `tags` equal the stored ones, the update writes nothing at all: no stamp, no tag registration, no card generation, no config write, and `col.mod` stays (R776, R1472). The comparison covers `flds` and `tags` only (the GUID, note type and id cannot change); `sfld` and `csum` are not compared, so a stored `sfld` or `csum` that another program computed differently stays until a real change rewrites it.

So saving a note opened in the editor without typing leaves it untouched (AE40, scenario `notes/save/untouched-fields`).

Anki compares the values as given, before normalizing them. When they differ from the stored ones only until the save normalizes them (decomposed text for a stored NFC field, a control character the save removes, the stored tags in another order, another case of a registered tag), Anki stamps the note with `usn` -1 and `mod` and `col.mod` now, although the stored row stays the same; Meguri writes nothing. Deviation `note-save-same-after-normalizing` (R776, R1472), scenario `notes/save/same-after-normalizing`.

## U3. Saves that change something

Otherwise the row is written with:

- `flds`, `tags`, `sfld`, `csum` as in U2's built row (so every field is normalized, not only the changed ones);
- `mod` `now_s`, `usn` -1 (R777);
- `guid`, `mid`, `flags`, `data` as stored.

New tags are registered (`tags/tags#T4`). Then the cards the note now requires are added (`notes/card-generation` G7), placed by `notes/deck-placement-and-position` D3; existing cards are not touched. `col.mod` becomes `now_ms`.

A field the caller did not change is stored again exactly as it was, apart from N2 (R823): markup an editor would not write itself survives (scenario `notes/save/untouched-fields`).

An update is one undo step, "Update Note", merged with the step before it as `engine/undo` A3 says.

## F1. Field count repair (R780)

A shared function, used by Check Database (U128) and the importers (U120), that makes a note's field list match its note type's field count n (n is at least 1):

- fewer than n values: empty values are appended up to n;
- more than n values: values n+1 onwards are appended to value n, in order, each preceded by `; ` (semicolon, space), also when a value is empty.

| Stored values | n | Repaired values |
|---|---|---|
| `three`, `b`, `c` | 2 | `three`, `b; c` |
| `one` | 2 | `one`, (empty) |
| `four`, `b`, (empty), `d` | 2 | `four`, `b; ; d` |
| `single`, `x`, `y` | 1 | `single; x; y` |

The caller decides what else happens. In Anki 26.09.3, Check Database also adds the tag `db-check` to each repaired note, saves it without stamping `mod` or `usn`, and counts it in `field_counts` (`specs/conformance/collection-check.md` K5); that belongs to U128 (scenario `notes/fields/count-repair`).

## D1. Deleting notes (R781)

Input: a list of note ids. Notes that do not exist are skipped; when none exists, nothing changes and `col.mod` stays.

For each note that exists, in list order: each of its cards is removed with a card grave, then the note row with a note grave (`storage/graves` G3, which gives the order). Before the rows go, the notes are appended to the deleted-notes log (`notes/deleted-notes-log`). The operation sets `col.mod` to `now_ms` once and is one undo step, "Delete Note". Undo puts the rows back as they were and removes the graves the deletion added (`engine/undo`, `storage/graves` G6); the log keeps its lines.

Callers: the review and Browse "Delete Note" actions (in Browse's card mode, the notes of the selected cards) and the local servers' delete calls. Deleting a deck or a note type, Empty Cards and incoming sync deletions remove notes through their own operations (U28, U34, U35, U107), which do not write the log.

## D2. Removing cards

A shared function used by D1 and by other units (Empty Cards U35, template removal U34): it removes a card row and adds its card grave (`storage/graves` G2, G3). Its variant "remove cards and the notes they leave empty" also removes each note left with no card, with its note grave, and writes no log line.

## S1. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `notes/records/add` | N1, N2, A2, A3, A4 | recorded |
| `notes/save/untouched-fields` | U2, U3, AE40 | recorded |
| `notes/save/normalize-off` | N2 with the key off, then on again | recorded |
| `notes/save/same-after-normalizing` | U2's deviation | recorded; deviation `note-save-same-after-normalizing` |
| `notes/fields/count-repair` | F1 through Check Database | recorded |
| `storage/noop/identical-note-rewrite` | U2, U3 stamps | recorded (U23) |
| `storage/graves/delete-note` | D1 graves | recorded (U23) |
| `notes/records/undo-add` | A5: undo and redo of an add, config writes included | recorded |
| `undo/delete-notes`, `undo/edit-note`, `undo/edit-note-new-card` | U3 and D1 undo | recorded (U25) |

Meguri's tests cover what no scenario shows: the refusals of A1 and U1, A4 with `update_add_defaults` false (no key written, config `mtime_secs` unchanged), bulk adds, a caller-supplied GUID, D2, and the log (`notes/deleted-notes-log` L6).
