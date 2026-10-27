# Check Database: notes and note types

Spec: maintenance/check-database-notes
Requirements: R1642 (invalid UTF-8 in notes, the tag list rebuild), R1643 (duplicate card ordinals, cards of missing templates, field counts with the `db-check` tag, missing note type recovery), R1644; R780 co-covered (`notes/note-records#F1`)
Anki: 26.09.3 for N3 to N8; N2 and N6.6's write rule are Meguri design (N9)
Scenarios: conformance/scenarios/maintenance/check-db-notes/

The notes stage of Check Database (U129): it runs inside the `check_database` repair op of `maintenance/check-database` C7, after the cards stage (C8, C9) and before the history stage (C10), with the same repair instant, stamping mode, report and rollback. Terms are those of `maintenance/check-database`. The counts are `conformance/collection-check#K5`'s kinds.

## N1. Order

1. N2 before the first note.
2. For each note type that has at least one note, in ascending order of the notes' `mid`: N4 (only when no note type has that id), N5, then N6 for each of its notes in ascending note id.
3. N7, then N8.

Progress: phase `notes`, completed = notes done, total = all notes (`maintenance/check-database` C15).

## N2. The tag registry during the stage

Meguri keeps the `tags` rows as they are while it visits the notes. Each note's tags are stored and registered as `tags/tags#T4` says (N6.6), against the registry as it is at that moment. After the last note, every row that is neither a tag some note stores nor an ancestor of one is removed, by `tags/tags#T11`'s rule (steps 1 and 2). Kept rows keep `collapsed`, `config`, `usn` and spelling. Nothing is counted.

So a parent row (`a` beside a note tagged `a::b`) and its expanded or collapsed state stay (R1642), and so does `config`.

Anki empties the registry before the first note, registers each note's tags again as the notes are visited (new rows: `usn` -1, collapsed, no `config`; spellings from the rows registered so far), and afterwards marks expanded again only the rows that were expanded before and that a note still uses. Its result therefore loses parent-only rows with their state, every `config`, and every row's `usn`, and may respell a tag after the first note that uses it. Deviation `check-db-tags-keep-rows` (N9). Decided 2026-10-27: R1642 asks to keep collapse states and parent tags, and `tags/tags#T14` keeps `config`; Anki's rebuild keeps neither for parent-only rows.

## N3. Note types visited

Only note types that have notes are visited, so N5 never touches a note type without notes. A note type that exists is read as stored (`notetypes/records`); one that cannot be decoded fails the op (`maintenance/check-database` C13).

## N4. A missing note type (R1643)

When notes name a `mid` that is no note type's id:

- The note type's field count n is the field count of that `mid`'s first note (lowest id): the number of U+001F in its `flds` bytes, plus 1. Meguri counts it from the bytes, so it works whatever the bytes hold. Anki decodes the note first and stops with an error, changing nothing, when that note's text is not valid UTF-8. Deviation `check-db-recover-invalid-utf8` (N9).
- k is the largest `ord` among the cards of that `mid`'s notes, or 0 when they have no card.
- A new note type is made from the stock Basic kind (`notetypes/stock-kinds` K3 to K5: fields `Front`, `Back`; template `Card 1`):
  - fields named `3`, `4`, ... are appended while it has fewer than n fields (n of 1 or 2 adds none);
  - k templates `Card 2` to `Card <k+1>` are appended (none when k is 0), each with `Card 1`'s front and back text, so every existing card has a template;
  - name `db-check-<ms>-<n>`, `<ms>` the repair instant in epoch milliseconds and `<n>` in decimal (`db-check-1894701600000-3`);
  - added as `notetypes/stock-kinds` K8 adds a stock kind: a new id (`storage/ids-usn-mtime#I2`), local stamps, the unique-name rule (`notetypes/records` N4 step 7: `db-check-1894701600000-3+` for a second missing type with the same n in the same run), `curModel` set to its id.
- The `mid` of every note of the old id becomes the new id (N6.6). Their cards keep their rows.
- Count: `notetypes_recovered` is 1 when at least one note type was recovered (K5); the report lists the names (`maintenance/check-database` C12).
- Template text and CSS are Meguri's stock Basic, not Anki's (`notetypes/stock-kinds` K9); scenarios compare the name, fields, templates and card requirements, never template text.

## N5. Image Occlusion field tags (R1644)

For a note type of N1 whose `original_stock_kind` is 6 (Image Occlusion) and that has at least five fields: for each of the fields at positions 0 to 4 whose `prevent_deletion` is false, `tag` becomes the position and `prevent_deletion` becomes true for positions 0 to 3 and false for position 4. Other fields and positions are left. When this changes a field's config, the note type is written (local stamps, not a schema change: `notetypes/schema-changes#G2`).

- A field at positions 0 to 3 whose `prevent_deletion` is already true keeps its `tag`, even a missing one: only unprotected fields are tagged.
- Position 4 (Comments) is never protected, so Anki finds it unprotected on every run and writes every such note type each time (`usn` -1, modification time now), also when nothing changes. Meguri writes it only when a field's config changes (`storage/ids-usn-mtime#S4`). Deviation `check-db-io-unchanged-skip` (N9).
- Note types of other kinds are never changed here, even with missing tags.

## N6. Each note

### N6.1 Invalid UTF-8 (R1642)

When the note's `flds`, `tags` or `sfld` is not valid UTF-8 (one of them is enough, `sfld` alone included), `flds` and `tags` are read as bytes and decoded with U+FFFD substituted for each maximal ill-formed subpart (the Unicode Standard's recommended practice), and `sfld` is set to the empty string; N6.6 then computes `sfld` and `csum` again. Count: `notes_invalid_utf8`, one per note. Not stamped. A note whose other text (`guid`) is not valid UTF-8 fails the op (`maintenance/check-database` C13), as in Anki (Anki applies this repair first, then fails on the same note and rolls everything back).

### N6.2 Duplicate card ordinals (R1643)

The note's cards are read once, in ascending card id. A card whose `ord` equals an earlier card's is deleted, with no grave. So the card with the lowest id keeps each ordinal. Count: `duplicate_card_ords`, one per deleted card. This also removes the extra ordinal-499 cards Anki makes for clozes numbered 500 and above (`notes/card-generation#G5`).

### N6.3 Cards without a template (R1643)

Only for a standard note type (a cloze note type keeps every card, R1643; a card whose cloze number has gone is Empty Cards' to remove). Each card of N6.2's list, which was read before N6.2 deleted anything, whose `ord` is not below the note type's template count is deleted, with no grave, and counted in `cards_without_template`. A card N6.2 already deleted is counted again here when its `ord` has no template: two cards of `ord` 5 on a two-template note type give `duplicate_card_ords` 1 and `cards_without_template` 2, and both cards are gone.

### N6.4 Field count (R1643, R780)

When the note's field count differs from the note type's (after N4, the recovered one's), the fields are repaired by `notes/note-records#F1`, the tag `db-check` is appended to the note's tags, and `field_counts` grows by one. Scenario `notes/fields/count-repair`.

### N6.5 Modification time in the future (R1644)

When the note's `mod` is above `now_s`, it becomes `now_s`. Its `usn` stays. Not counted.

### N6.6 Writing the note and its cards

- The row the note gets: `mid` the note type's id (N4's new id for a recovered one); `flds` normalized by `notes/note-records#N2` (control characters, NFC per `normalize_note_text`); `sfld` and `csum` from them (`text/checksum-and-sort-field` C2 to C4); `tags` as `tags/tags#T4` stores them against the registry (N2), registering new ones; `mod` as stored or as N6.5 set it; `usn`, `guid`, `flags` and `data` as stored.
- The note is written only when that row differs from the stored one in any column (`storage/ids-usn-mtime#S4`). The write does not stamp: `mod` and `usn` keep their values, whatever changed (scenario `notes/fields/count-repair`).
- Then the note's cards are generated as for an existing note: `notes/card-generation` G6 and G7 (only additions; a note with no card gets ordinal 0), placed and positioned by `notes/deck-placement-and-position` D3 and P2. New cards are rows of `notes/card-generation#G10`, with local stamps. Not counted. So a template that became non-empty, or a card a raw write deleted, gets its card here (R1644).

## N7. After the notes

- N2's removal of unused tag rows.
- When no note type is left, which happens only when no note is left either (N4 has recovered a note type for every remaining note), the stock Basic kind is added under its name as `notetypes/stock-kinds` K8 adds it: a new id, local stamps, `curModel` set to it. Not counted, not a schema change. The definition's checks pass on the stock kind, so whether they run cannot change the result (Anki skips them). Anki writes this row with `mtime_secs` 0 and `usn` 0; Meguri writes local stamps. Decided 2026-10-27, data safety: `usn` -1 makes the next manual Anki transfer send the note type the collection now relies on. Deviation `check-db-basic-stamped` (N9).

## N8. The schema stamp

`scm` is set (`maintenance/check-database` C11) when `duplicate_card_ords`, `cards_without_template`, `field_counts` or `notetypes_recovered` is above 0. N2, N5, N6.1, N6.5, N6.6 and N7 alone leave it.

## N9. Differences from Anki

Each is a deviation entry in `conformance/deviations.yaml`, added with the scenario golden that shows it (R1889):

| Id | Anki 26.09.3 | Meguri |
|---|---|---|
| `check-db-tags-keep-rows` | empties the tag registry and rebuilds it from the notes (N2) | keeps the rows, adds the notes' new tags, then removes only rows no note uses as a tag or an ancestor |
| `check-db-io-unchanged-skip` | writes every Image Occlusion note type with notes on every run (N5) | writes it only when a field's config changes |
| `check-db-recover-invalid-utf8` | stops with an error when the first note of a missing note type is not valid UTF-8 (N4) | counts the fields from the bytes and recovers |
| `check-db-basic-stamped` | adds the fallback Basic note type with `mtime_secs` 0 and `usn` 0 (N7) | local stamps (`usn` -1, `mtime_secs` `now_s`) |

N6.6 matches Anki: Anki writes every note, but a note whose row would not change is the same row either way.

## N10. Scenarios

### N10.1 Goldens on this branch

| Scenario | Shows |
|---|---|
| `notes/fields/count-repair` | N6.4 with `notes/note-records#F1`'s table: `db-check` tag, no stamp |
| `notetypes/records/stock-handoff`, `notetypes/records/stock-render-handoff`, `storage/new-collection/handoff` | live (T1): Meguri's collections give Anki's Check Database no problem |

### N10.2 To record (oracle maintainer)

In `conformance/scenarios/maintenance/check-db-notes/`, with existing scenario ops only (`db_exec`, `check_db`, `add_note`, `set_tag_collapsed`, `ensure_io_notetype`, `advance`, setup) and the observations `db_exec`, `notetype_structure`, `tag_tree` and `db_dump`. A scenario that recovers a note type (`recover-*`, `all-notes`) never observes `undo_status` (`maintenance/check-database` C14). Each damages rows by `db_exec`, sets `usn` 5 on them and advances 1 hour so stamps show, observes the rows and `SELECT mod, scm FROM col`, and ends with a second `check_db` that finds nothing.

| Scenario | Damage and observation |
|---|---|
| `recover-notetype` | a note type with three fields and three templates and two notes, whose `notetypes`, `fields` and `templates` rows are deleted by `db_exec`: `db-check-<ms>-3` with fields `Front`, `Back`, `3` and templates `Card 1` to `Card 3`, notes moved, cards kept, `notetypes_recovered` 1, `scm` set; no `undo_status` (`maintenance/check-database` C14) |
| `recover-two-same-count` | two missing `mid`s whose first notes have two fields: names `db-check-<ms>-2` and `db-check-<ms>-2+` |
| `recover-one-field` | a missing `mid` whose notes have one field: two fields, the notes padded and tagged `db-check`, `field_counts` counted |
| `duplicate-ords` | a card copied by `db_exec` with a new, higher id and the same `nid` and `ord`: the copy deleted, no grave, count 1, `scm` set |
| `missing-template` | an `ord` 5 card on a two-template note type: deleted, count 1; and two `ord` 5 cards: `duplicate_card_ords` 1, `cards_without_template` 2 |
| `cloze-kept` | a cloze note whose `c3` was removed from the text by `db_exec`, its card of `ord` 2 kept: no count |
| `invalid-utf8` | `flds` set to bytes `66 ff 6f 1f 62` and `tags` to ` a\xff ` by `CAST(X'..' AS TEXT)`: U+FFFD in place, `sfld` and `csum` recomputed, count 1, `mod` and `usn` kept; and a second note with valid `flds` and `tags` whose `sfld` alone is set to bytes `78 ff` the same way: `flds` and `tags` unchanged, `sfld` and `csum` recomputed, counted (`notes_invalid_utf8` 2 in all) |
| `future-mtime` | a note `mod` two days ahead: `now_s`, `usn` kept, not counted |
| `missing-card` | a note of a note type the scenario defines with two non-empty templates, whose `ord` 1 card was deleted by `db_exec`: card generated in the deck of its sibling, at a new position, not counted |
| `no-notetypes` | every note, card and note type deleted by `db_exec`: Basic added, `curModel` its id, and its `mtime_secs` and `usn` (deviation `check-db-basic-stamped`) |
| `io-field-tags` | `ensure_io_notetype`, one note, the Header field's `prevent_deletion` cleared and its `tag` removed (through the field config, by the scenario's note type update op if it can, else held): tag 2 and protection back; the note type's `mtime_secs` and `usn` (deviation `check-db-io-unchanged-skip` on the second run) |
| `tag-rebuild` | rows `a` (expanded), `a::b` (used, collapsed), `old` (unused, expanded), a note tagged `a::b` and `New` with no row (inserted by `db_exec`): `tag_tree` and the `tags` rows (deviation `check-db-tags-keep-rows`) |
| `all-notes` | every damage above with C18.2's card damage of `maintenance/check-database` in one collection: one report |

`io-field-tags` needs a way to change one field's `prevent_deletion` and `tag`; if no existing op can, the oracle maintainer holds it until one exists.

### N10.3 Meguri tests (`core/crates/meguri-engine/tests/check_db.rs`)

- N1's order: a recovered note type is visited before N5 and N6 of its notes; notes in id order; note types in `mid` order.
- N4 with an invalid UTF-8 first note: recovered, n from the bytes (deviation).
- N6.6 skips an unchanged note: no journal row for it.
- The structure report of a run that recovers a note type: reason 10 (`engine/change-stream#S2.5`, once approved).
