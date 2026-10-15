# Row records of a schema-18 collection

Spec: model/row-records
Requirements: R1456, R1459, R1462, R1465, R1484, R1485, R322
Anki: 26.09.3
Scenarios: conformance/scenarios/model/ (card rows through `card-data/`; blob rows through `proto/` and `schema11/`)

The typed value of each row of every table in `collection.anki2` at schema 18, the units of its columns, and how Anki treats stored values that do not fit. The table definitions (DDL, indexes, collation) are the storage area's (U22); ids, `usn` and modification-time stamping are the storage area's too (U23). Protobuf blobs are `model/protobuf-records`, card `data` is `model/card-data-json`.

## R1. Reading stored values

Unless a table says otherwise, a column is read as follows, as Anki does:

- An integer column must hold an SQLite INTEGER that fits the declared type (N1 of `model/json-numbers` gives the ranges); a value of another storage class, or out of range, makes the row unreadable (an error naming the table, column and row id).
- A text column must hold TEXT.
- Exceptions are listed with each table: some columns read a misfit value as a default instead.
- Meguri writes each value with the storage class above: integers as INTEGER, text as TEXT, blobs as BLOB (`model/protobuf-records` P1).

## R2. `col`

One row, `id` = 1.

| Column | Type | Meaning |
|---|---|---|
| `id` | i64 | always 1 |
| `crt` | i64 | collection creation, epoch **seconds**; the start of day 0 for day numbers |
| `mod` | i64 | last change to the collection, epoch **milliseconds** |
| `scm` | i64 | last schema change, epoch milliseconds; a value greater than `ls` means the next sync must be one-way |
| `ver` | i64 | schema version, 18 |
| `dty` | i64 | unused, 0 |
| `usn` | i32 | on a client, the server's update sequence number as of the last sync |
| `ls` | i64 | last sync, epoch milliseconds |
| `conf`, `models`, `decks`, `dconf`, `tags` | text | the empty string at schema 18 (R1456); JSON at schema 11 (`model/schema11-json` S12) |

## R3. `notes`

| Column | Type | Meaning |
|---|---|---|
| `id` | i64 | note id |
| `guid` | text | stored byte for byte (`specs/text/guid`) |
| `mid` | i64 | note type id |
| `mod` | i64 | modification time, epoch seconds |
| `usn` | i32 | update sequence number |
| `tags` | text | space-separated, with a leading and a trailing space, or the empty string |
| `flds` | text | field values joined by U+001F |
| `sfld` | text or integer | sort field (`specs/text/checksum-and-sort-field`); read as text |
| `csum` | i64 | checksum, an unsigned 32-bit value; a value outside 0 to 4294967295 reads as 0 |
| `flags` | i64 | unused: a new note gets 0, and updates leave the stored value |
| `data` | text | unused: a new note gets the empty string, and updates leave the stored value |

## R4. `cards`

| Column | Type | Meaning |
|---|---|---|
| `id` | i64 | card id |
| `nid` | i64 | note id |
| `did` | i64 | deck id (for a card in a filtered deck: that filtered deck) |
| `ord` | u16 | template ordinal (cloze number - 1 for cloze note types) |
| `mod` | i64 | modification time, epoch seconds |
| `usn` | i32 | update sequence number |
| `type` | card type | 0 new, 1 learning, 2 review, 3 relearning |
| `queue` | card queue | 0 new, 1 learning, 2 review, 3 interday learning, 4 preview, -1 suspended, -2 buried by the scheduler, -3 buried by the user |
| `due` | i32 | meaning depends on `queue`, below |
| `ivl` | u32 | interval in days (review and relearning cards) |
| `factor` | u16 | ease in permille (2500 = 250%); 0 for new cards |
| `reps` | u32 | number of reviews |
| `lapses` | u32 | number of lapses |
| `left` | u32 | learning steps left; its encoding belongs to the scheduling specs |
| `odue` | i32 | for a card in a filtered deck, its `due` before it moved there; else 0 |
| `odid` | i64 | for a card in a filtered deck, its home deck; else 0 |
| `flags` | u8 | the low 3 bits are the user flag (0 none, 1 to 7); other bits kept |
| `data` | text | `model/card-data-json` |

`due` by queue (R1459):

| Queue | `due` |
|---|---|
| 0 new | position in the new queue |
| 1 learning, 4 preview | epoch seconds |
| 2 review, 3 interday learning | day number, days since the day of `col.crt` |
| -1, -2, -3 | the `due` of the queue the card would be in when restored (by its `type`) |

Read tolerances (oracle): `type` and `queue` take the low 8 bits of the stored integer first (two's complement for `queue`), so a stored 258 reads as type 2; a value that is then not in the list above makes the row unreadable (type 7, queue 9). `due` and `odue` read as 0 when the stored value is not an INTEGER within the `i32` range (a `due` of 2^40 reads as 0). `ivl` of -1 makes the row unreadable. `data` that is not TEXT reads as empty.

Meguri keeps `odue`, `odid` and filtered-deck membership intact although it never creates filtered decks (R1483), and keeps `factor` and the other SM-2 columns although it schedules with FSRS only (R1484).

## R5. `revlog`

| Column | Type | Meaning |
|---|---|---|
| `id` | i64 | review time, epoch milliseconds |
| `cid` | i64 | card id |
| `usn` | i32 | update sequence number |
| `ease` | u8 | button: 1 Again, 2 Hard, 3 Good, 4 Easy; 0 for manual changes |
| `ivl` | i32 | interval after the review: positive in days, negative in seconds |
| `lastIvl` | i32 | interval before the review, same units |
| `factor` | u32 | ease after the review in permille; with FSRS, the difficulty mapped to 100–1100 |
| `time` | u32 | answer time in milliseconds |
| `type` | review kind | 0 learning, 1 review, 2 relearning, 3 filtered (early review), 4 manual, 5 rescheduled |

- Meguri writes kinds 0, 1, 2, 4 and 5, and keeps rows of kind 3 it receives (R1462).
- Read tolerances: `ivl` and `lastIvl` are read after conversion to an integer (a REAL value is truncated by SQLite); `time` reads as 0 when it is not an INTEGER within the `u32` range; `type` takes the low 8 bits and reads as 0 when the result is not a known kind. Meguri never rewrites a revlog row it did not create, so these read values never replace stored ones.

## R6. `graves`

| Column | Type | Meaning |
|---|---|---|
| `oid` | i64 | id of the deleted object |
| `type` | i64 | 0 card, 1 note, 2 deck |
| `usn` | i32 | update sequence number |

Rules for adding graves are the storage area's (U23).

## R7. Note types, decks and presets

| Table | Columns besides the blobs | Blob columns |
|---|---|---|
| `notetypes` | `id` i64, `name` text, `mtime_secs` i64 (seconds), `usn` i32 | `config` |
| `fields` | `ntid` i64, `ord` u32 (position), `name` text | `config` |
| `templates` | `ntid` i64, `ord` u32 (position), `name` text, `mtime_secs` i64, `usn` i32 (Anki keeps both 0) | `config` |
| `decks` | `id` i64, `name` text (native form, `model/schema11-json` S11), `mtime_secs` i64, `usn` i32 | `common`, `kind` |
| `deck_config` | `id` i64, `name` text, `mtime_secs` i64, `usn` i32 | `config` |

- Deck names in the table are in native form: components joined by U+001F (R1465). Code that shows or accepts names uses the human form with `::`.
- A note type is its `notetypes` row with its `fields` and `templates` rows ordered by `ord`; `ord` runs from 0 without gaps.

## R8. `tags`

| Column | Type | Meaning |
|---|---|---|
| `tag` | text | tag name; hierarchy levels joined by `::` |
| `usn` | i32 | update sequence number |
| `collapsed` | bool (INTEGER 0 or 1) | collapsed in the browser sidebar |
| `config` | blob or NULL | unused by Anki (it writes NULL); kept unchanged |

## R9. `config`

| Column | Type | Meaning |
|---|---|---|
| `KEY` | text | config key |
| `usn` | i32 | update sequence number |
| `mtime_secs` | i64 | modification time, epoch seconds |
| `val` | blob | the value as JSON bytes |

How values are read, written and preserved is `specs/storage/config-table` (U23). A `val` that is not a BLOB or not the JSON a reader expects is an exception to R1: a typed read gives the reader's default and never fails (`specs/storage/config-table` C3).

## R10. Errors

A row that cannot be read under R1 to R9 (or whose blob fails `model/protobuf-records` P9) yields a typed error naming the table, the column and the row's key, and no record. Meguri neither repairs nor guesses on read; repairs belong to Check Database (U128, U129).
