# Graves

Spec: storage/graves
Requirements: R1473, R1541
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/graves/

A grave records that a card, note or deck was deleted, so the next sync can delete it on the other side. Stamping terms are those of `ids-usn-mtime.md`.

## G1. Table

| Column | Content |
|---|---|
| `oid` | the deleted object's id |
| `type` | 0 card, 1 note, 2 deck |
| `usn` | -1 while pending; the server's USN once synced |

- The primary key is (`oid`, `type`); the table has no row id, and the index `idx_graves_pending` on `usn` serves the pending query. The DDL is `storage/schema-18` S2 and S3.
- Only cards, notes and decks get graves. Deleting a note type, a deck preset, a review-log row, a tag or a config key leaves none. Note type and preset deletions are schema changes that force the next manual Anki transfer to be one-way (KTD26, `notetypes/schema-changes#G1`). A sync only ever adds tags, so a tag removed on one device stays on the others. Config travels whole (`config-table.md` C9).

## G2. Insert or ignore

Adding a grave whose (`oid`, `type`) already exists leaves the existing row exactly as it is, `usn` included (R1473). It is no change in the sense of `ids-usn-mtime.md` S4: it does not move `col.mod` by itself.

## G3. Local deletions

Every local deletion adds its graves with `usn` -1, in the operation that removes the rows:

| Deletion | Graves |
|---|---|
| a note | one type 0 grave per card of the note, then a type 1 grave for the note |
| cards | one type 0 grave per card; a note left with no cards is removed too, with its type 1 grave |
| a normal deck other than the default deck | type 0 and type 1 graves for its cards and the notes they leave empty, then a type 2 grave; each child deck the same way |
| the default deck (id 1) | none: its cards and notes go as above, and the deck stays, renamed back to its default name |
| a filtered deck | a type 2 grave only: its cards go back to their home decks |
| a note type | type 0 and type 1 graves for all its cards and notes; none for the note type |

- Deleting an object that no longer exists does nothing: no grave, no change, `col.mod` stays. So repeating a deletion adds no grave.
- The owning areas specify the operations themselves (U27 notes and cards, U28 decks, U29 note types, U35 Change Note Type and Empty Cards).

## G4. Incoming deletions

When a sync applies the other side's deletions, it removes each listed note, card and deck, whether or not it exists locally, and adds a grave for it with the server's USN, per G2 (U107). Anki applies the note list first, then cards, then decks; a deleted note's cards arrive as card graves of their own.

## G5. Pending graves

- Graves with `usn` -1 are pending: a normal sync sends them and then sets their `usn` to the server's USN (U107).
- A one-way upload deletes every grave first (R1541, U109).

## G6. Undo

Undoing a deletion restores the rows and deletes the graves of the deleted cards, notes and decks; redo adds them again with `usn` -1 (`engine/undo` A5). The undo also deletes a grave that existed before the deletion, whose insert G2 ignored: an object re-created under a deleted id (an import that keeps ids) loses its old grave, and after the redo that grave is pending whatever `usn` it had. Meguri does the same, although its write journal records no change for the ignored insert (`engine/undo` M4.3, A11).

## G7. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `storage/graves/delete-note` | G2, G3 for a note with three cards, a pre-existing grave and a repeated deletion | recorded |
| `undo/delete-notes`, `undo/delete-note-existing-grave` | G6 | recorded |

The ops are specified in `specs/conformance/record-ops.md`.
