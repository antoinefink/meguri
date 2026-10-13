# Storage ops and observations for scenarios

Spec: conformance/storage-ops
Requirements: R1449, R1478, R1479, R1482, R2016
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/

The scenario vocabulary the storage area needs (U22): the op `open`, which replaces the scenario's collection with a copy of a fixture file and opens it, the final form of the draft op `db_exec` (`dsl.md` D7.2), and the observation `schema_objects`. Scenarios also use `db_dump` (`observations.md`, the `db_dump` section of the sync harness change, U13). The behavior these observe is `specs/storage/`.

## P1. `open`

### P1.1 Argument and effect

`open: {fixture: <path>}`. `fixture` is a path relative to `conformance/fixtures/`, ending in `.anki2`, naming a file that exists (else `invalid_scenario` when the scenario loads).

1. If a collection is open, close it as at a handoff (no downgrade; Meguri closes per `storage/open-and-connections` C10).
2. Delete the session's `collection.anki2` and any `collection.anki2-wal`, `-shm` and `-journal` next to it. Other files in the session directory stay.
3. Copy the fixture byte for byte to `collection.anki2`.
4. Open it the way the executor opens any collection:
   - Meguri: the open gate of `storage/open-and-connections` C2, upgrade included.
   - Oracle: Anki's collection open (the backend's open-collection call through the `anki` package). The open must not write anything besides what Anki's storage open and upgrade write: the Python layer's scheduler loading adds `creationOffset` when the v3 scheduler is on and the key is missing, and the fixtures are made so that this never happens (`storage/schema-upgrade#U16`); the driver must not add other writes.
5. The clock is the scenario's clock: upgrade stamps (renamed decks) carry its current instant.

### P1.2 Output and rejections

- Success: `{}`.
- Rejection (`dsl.md` D7.3): `{"open": {"error": <kind>}}` with kind from this closed list:

  | Kind | When | Who records it |
  |---|---|---|
  | `file_too_old` | `col.ver` below 11 | oracle and Meguri |
  | `file_too_new` | `col.ver` 12, 13 or above 18 | oracle and Meguri |
  | `v1_scheduler` | the collection uses the v1 scheduler | Meguri only: Anki opens such a file (deviation `storage-v1-scheduler-refused`, R1482) |

- After a rejection no collection is open. The next step must be another `open`; any other step is a run error (`invalid_scenario`). Anki's refusal does change the refused file (`storage/open-and-connections` C3); no observation looks at it.

### P1.3 Scenarios that use it

- A scenario with an `open` step has no `setup`, and its first step is `open`. Executors create no fresh collection for it: the session starts empty and the first `open` brings the collection.
- Logical ids (`#nN`, `#cN`) are assigned only to notes and cards the scenario creates; a fixture's notes and cards have none. Scenarios read them by real id through `db_exec` and `db_dump`.
- A handoff after an `open` hands over the opened collection as usual (`dsl.md` D9.2), with any other file in the session directory (Meguri's pre-upgrade copy).

## P2. `db_exec`

The draft row of `dsl.md` D7.2, fixed here. When the oracle driver gains the op, the D7.2 row becomes stable with this argument, output and rejection list, and the schemas follow.

### P2.1 Argument

`db_exec: {sql: <text>, params?: [<value>, ...]}`.

- `sql` is exactly one SQL statement (a trailing `;` is allowed). Two statements, or SQL that does not prepare, is `invalid_scenario`.
- `params` binds positional parameters (`?`) in order: an integer as INTEGER, a number written with a fraction or exponent as REAL, a string as TEXT, null as NULL. Booleans, arrays and objects are `invalid_scenario`. A blob is written in the SQL as a literal (`x'0a1b'`).
- It runs on the collection's read-write connection (Anki's own connection on the oracle, so the `unicase` collation and Anki's SQL functions are there; Meguri's writer with its registered functions, `storage/open-and-connections` C5), as one transaction of its own, committed when the step ends. Writes bypass every usn, mtime and `col.mod` rule: they are raw. On the oracle that means running the statement outside the database proxy's transaction wrapper: when a wrapped transaction that wrote (any statement not starting with `SELECT`, pragmas included) commits, Anki stamps `col.mod` with the current time, which Meguri's raw write never does.

### P2.2 Output

`{"db_exec": {"rows": [[<value>, ...], ...]}}`: the rows the statement returns, in the order SQLite returns them, each a list of its column values. A statement that returns none (an `UPDATE`, an `INSERT` without `RETURNING`) gives `{"rows": []}`. Values by the SQLite storage class of each value, as `db_dump` writes them:

| Storage class | Value |
|---|---|
| INTEGER | integer |
| REAL | number with a fraction or an exponent, the shortest decimal that reads back as the same 64-bit float (REAL 1 is `1.0`) |
| TEXT | string |
| BLOB | `{"sha256": "<lower-case hex SHA-256 of the bytes>"}` |
| NULL | `null` |

Pragmas are statements too: `PRAGMA integrity_check` gives `{"rows": [["ok"]]}` on a sound file, `PRAGMA journal_mode` `{"rows": [["wal"]]}`. Use `ORDER BY` whenever more than one row comes back: without it the order is SQLite's and may differ between the oracle and Meguri.

### P2.3 Rejections

`{"db_exec": {"error": <kind>}}`, the statement having changed nothing:

| Kind | SQLite result |
|---|---|
| `unique_violation` | a `UNIQUE` or `PRIMARY KEY` constraint failed (extended codes 2067 and 1555) |
| `constraint_violation` | any other constraint failed (`NOT NULL`, `CHECK`, foreign key) |

Every other failure is a run error.

## P3. `schema_objects`

### P3.1 Request and value

Request: `schema_objects` (no arguments). Value: a list with one entry per row of the main database's `sqlite_master`, sorted by `type`, then `name` (code point order):

`{"type": <"table" | "index" | "view" | "trigger">, "name": <name>, "tbl_name": <table>, "sql": <normalized text or null>}`

`sql` is null for objects SQLite stores without text (automatic indexes). Temporary objects (SQLite's temp schema) are not listed.

### P3.2 Normalized SQL text

The stored `sql` text, normalized so that layout and comments never matter:

1. remove every `--` comment, from the `--` to the end of its line (the line break stays);
2. replace every run of spaces, tabs, carriage returns and line feeds with one space;
3. remove spaces at the start and at the end.

Nothing else changes: letter case, punctuation and the spaces next to it stay. The rule treats quotes like any other character, so it would also cut a `--` inside a quoted name or string; no object of a collection has one.

Example: a `notes` table stored as several lines with a comment line about `sfld` normalizes to the single line of `storage/schema-18#S2`.

### P3.3 Use

The open and upgrade scenarios observe `schema_objects` after their last change: it shows the objects an upgrade creates with their exact definitions, and it is the per-scenario check of the schema guard (R2016, `storage/schema-18#S6.2`).
