# Ids, USN and modification times

Spec: storage/ids-usn-mtime
Requirements: R772, R776, R906, R1456, R1469, R1470, R1471, R1472, R1474
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/ids/, conformance/scenarios/storage/noop/, conformance/scenarios/storage/stamps/

How a row of `collection.anki2` gets its id when it is created, what a write does to its `usn` and modification time, when a write is skipped because it changes nothing, and how `col.mod` follows changes. Graves are in `graves.md`, the `config` table in `config-table.md`. "Now" is the U21 clock seam's current instant: `now_ms` in epoch milliseconds, `now_s` = `now_ms` divided by 1000, rounded down.

## I1. Tables with allocated ids

| Table | Id column | Unit |
|---|---|---|
| `notes` | `id` | epoch ms |
| `cards` | `id` | epoch ms |
| `revlog` | `id` | epoch ms: the review instant |
| `decks` | `id` | epoch ms |
| `deck_config` | `id` | epoch ms |
| `notetypes` | `id` | epoch ms |

- The default deck and the default deck preset have the fixed id 1. They come with the new collection and are never allocated (U28).
- The other tables have no allocated id: `graves` holds the deleted object's id (`graves.md`), `config` is keyed by name, `tags` by tag text, `fields` and `templates` by note type id and ordinal, and `col` is the single row with `id` 1.

## I2. Allocation rule

A new row of one of the I1 tables takes a candidate value:

- `notes`, `cards`, `decks`, `deck_config`, `notetypes`: `now_ms`;
- `revlog`: the instant of the review in ms. For an answer that is the answer instant, which is `now_ms`; entries the scheduler writes without an answer (rescheduling, manual changes) also use `now_ms`.

If no row of the same table has the candidate as its id, the new row takes the candidate. Otherwise it takes the largest id in that table plus one. Only the row's own table counts: a note and its first card created at one instant both take `now_ms`.

Each new row is allocated on its own, in the order the operation creates them; a note's row comes before its cards, and its cards follow card generation's order (template order for a standard note type, U27). So rows of one table created at one frozen instant get `now_ms`, `now_ms + 1`, `now_ms + 2`, ... when no larger id exists. Anki's result depends only on whether the candidate is taken and on the table's largest id: it never fills a gap, and a free candidate is taken even when larger ids exist.

| Ids already in the table | `now_ms` | New id |
|---|---|---|
| none | T | T |
| T | T | T + 1 |
| T, T + 1 | T | T + 2 |
| T + 3 days | T + 1 day | T + 1 day (free) |
| T, T + 86 399 000 | T | T + 86 399 001 |
| T + 1 day, T + 3 days | T + 1 day | Anki: T + 3 days + 1; Meguri: I3 |

Meguri must allocate exactly these values outside I3's cap: interval fuzz is seeded from card ids, so a different card id changes scheduling (R772, R1469).

## I3. One-day cap (Meguri)

Meguri never allocates an id greater than `now_ms + 86 400 000` (R1470). When I2 gives a larger value, the new row takes instead the smallest integer greater than `now_ms` that no row of its table uses. A value of exactly `now_ms + 86 400 000` is allowed. In the unlikely case that every value from `now_ms + 1` to `now_ms + 86 400 000` is in use, the allocation fails and the operation writes nothing.

Anki has no cap, so its ids follow I2 even two days ahead. This difference is a deviation: `ids-future-cap-notes`, `ids-future-cap-boundary` and `ids-future-cap-revlog` in `conformance/deviations.yaml`. They replace the oracle's ids in the cap scenarios with the values this section gives; nothing else may differ.

| Ids already in the table | `now_ms` | I2 value | Meguri |
|---|---|---|---|
| T + 1 day, T + 3 days | T + 1 day | T + 3 days + 1 | T + 1 day + 1 |
| T, T + 1 day | T | T + 1 day + 1 | T + 1 |
| T, T + 86 399 000 | T | T + 86 399 001 | T + 86 399 001 (within the cap) |

## I4. Rows that arrive with an id

Rows that a sync, an import, an undo restore or a raw copy brings keep their given ids: no allocation, and no cap, applies to them. A review-log row whose given id already exists is not inserted, and the existing row stays as it is; that is Anki's rule for review-log rows from a sync, an import and an undo restore. The owning areas define the other tables' rules for given ids (U107 sync, U122 import). For undo (`engine/undo` A8): only a writer outside the undo history can take the id of a row an undo or redo puts back, and Meguri's check refuses that undo or redo before it writes (deviation `undo-revlog-taken-id-redo`).

## S1. Stamp columns

| Table | Modification time | USN |
|---|---|---|
| `notes` | `mod`, epoch seconds | `usn` |
| `cards` | `mod`, epoch seconds | `usn` |
| `revlog` | none | `usn` |
| `decks` | `mtime_secs` | `usn` |
| `deck_config` | `mtime_secs` | `usn` |
| `notetypes` | `mtime_secs` | `usn` |
| `templates` | none: `mtime_secs` is never stamped | none: `usn` is never stamped |
| `fields` | none: changes with its note type | none |
| `tags` | none | `usn` |
| `config` | `mtime_secs` | `usn` |
| `graves` | none | `usn` |
| `col` | `mod`, epoch ms (S5) | `usn`: set only by a sync (S6) |

A template row changes with its note type, whose `notetypes` row carries the stamps. Its own `mtime_secs` and `usn` are written as data: 0 for a template that is new or comes from a schema-11 note type (a sync, an import, a legacy update), and the stored values when a note type is saved again with its loaded templates (`specs/model/row-records.md` R7). No source in S3 sets them.

## S2. Local changes

A row that a local action creates or changes gets `usn` -1 and, where S1 lists a modification time, `now_s` (R1471). Local actions are the user's, the local API's, the optimizer's and day rollover's writes. A row with `usn` -1 is pending: the next sync sends it.

## S3. Stamps by write source

| Source | `usn` | Modification time | `col.mod` |
|---|---|---|---|
| Local change (S2) | -1 | `now_s` | set to `now_ms` (S5) |
| Incoming sync row or grave | the server's value | as received | not by the row; the sync's end sets it (R1531) |
| Marking sent rows as synced | the USN the server reported when the sync began | unchanged | unchanged |
| `.apkg` update of an existing note (R1602) | -1 | the incoming note's | set to `now_ms` |
| Undo or redo | the restored row's (`engine/undo` A4 to A6; a grave a redo adds back: -1, A5) | the restored row's | restored (S5) |
| Raw write: staging copies (U33, U122, U123), the unbury at day rollover (U40), scenario `db_exec` | as written | as written | unchanged |
| Day clock config writes: `rollover`, `localOffset`, `lastUnburied` (`sched/day-clock#D5`, `#D7`) | -1 | `now_s` | unchanged |
| Schema upgrade on open (`storage/schema-upgrade`, not an operation) | as that spec gives: -1 only for the decks and cards it changes (U9.1) | as that spec gives | unchanged |

Day rollover's unbury rewrites `cards.queue` without touching `mod` or `usn`, and records the day in the `lastUnburied` config key. That config write, like the day clock's writes of `rollover` and `localOffset`, stamps the row as a local change but leaves `col.mod` alone, as Anki does: Anki makes these writes outside any operation (`sched/day-clock#D5`).

## S4. Writes that change nothing

Meguri skips a write whose row equals the stored row in every column (R776, R906, R1472). The comparison takes the row as the caller hands it over, `usn` and modification time included, before any stamping; a caller that loads a row, edits it and writes it back passes the stored `usn` and modification time. A skipped write changes no column, records no change and does not move `col.mod`. A difference in `usn` alone is a real change and is stamped as S3 gives for its source. Inserting a row identical to an existing one and deleting a row that does not exist also change nothing.

Anki's operations decide row by row, and they do not always agree with this rule:

| Row | Anki skips the write when |
|---|---|
| note | guid, note type, fields, tags and `usn` equal the stored ones; `mod` is not compared |
| config value | the new value's bytes equal the stored bytes (`config-table.md` C4) |
| deck, deck preset | the whole record equals the stored one |
| card | never: every card update Anki makes is stamped, even when nothing else changes |
| note type | never: saving a note type always stamps it |

Meguri applies S4 everywhere. Where an Anki operation stamps an unchanged row and a scenario shows it, the area that owns the operation adds a deviation under R1472 (cards: U27, U44, U48; note types: U29).

## S5. `col.mod`

- An operation that makes at least one real change sets `col.mod` to `now_ms` once, when it commits, however many rows it changed (R1471). An operation that changes nothing leaves `col.mod` alone (R1472).
- A change to any config key counts, `curDeck` included, so it marks the collection changed for sync (R1474). The day clock's writes are the exception (S3).
- Undo sets `col.mod` back to the value it had before the undone operation; redo sets it to the value it had when that undo ran, which is the value the operation set unless something moved `col.mod` in between without emptying the undo history (a config write without an undo entry). Neither sets it to `now_ms` (`engine/undo` A7).
- Raw writes and the marking of sent rows leave it unchanged; a sync sets it at its end (R1531).
- Some Anki maintenance paths that keep no undo record set `col.mod` even when they change nothing. Meguri does not; the owning area records the deviation if a scenario shows it.

## S6. The `col` row

| Column | Meaning |
|---|---|
| `id` | 1 |
| `crt` | the collection's day 0, epoch seconds (U40) |
| `mod` | last change, epoch ms (S5) |
| `scm` | last schema change, epoch ms: a schema change forces the next sync to be one-way (U29, U109) |
| `ver` | 18 |
| `dty` | 0 |
| `usn` | 0 when never synced. A sync's end sets it to the USN the server reported when that sync began, plus one: the server's own USN once the sync is done (R1531). Local changes never write it (R1456). |
| `ls` | last sync, epoch ms; 0 when never synced |
| `conf`, `models`, `decks`, `dconf`, `tags` | empty strings at schema 18 |

## S7. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `storage/ids/same-instant` | I2 for notes and cards at one instant | recorded |
| `storage/ids/revlog-same-instant` | I2 for review-log rows | recorded |
| `storage/ids/taken-above-now` | I2 when a larger id exists, within the cap | recorded |
| `storage/ids/clock-back-cap` | I3 after the clock moves back two days | recorded; deviation `ids-future-cap-notes` |
| `storage/ids/cap-boundary` | I3 one millisecond past the cap | recorded; deviation `ids-future-cap-boundary` |
| `storage/ids/revlog-clock-back-cap` | I3 for review-log rows | recorded; deviation `ids-future-cap-revlog` |
| `storage/ids/all-tables` | I2 for decks, deck presets and note types | recorded |
| `storage/noop/identical-note-rewrite` | S4 for a note, then S2 and S5 | recorded |
| `storage/stamps/answer` | S2 and S5 for a card, a review-log row and `col.mod` | recorded |
| `storage/stamps/col-row` | S6 | recorded |

The ops `db_exec`, `edit_note`, `delete_notes` and `set_config` are specified in `specs/conformance/record-ops.md`. Behavior that only Meguri's own code shows (the write journal, write scopes, bulk paths) has no scenario; U23's Rust tests cover it.
