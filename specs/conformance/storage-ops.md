# Storage ops and observations for scenarios

Spec: conformance/storage-ops
Requirements: R1449, R1478, R1479, R1481, R1482, R2016
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/

The scenario vocabulary the storage area needs (U22, U33): the op `open`, which replaces the scenario's collection with a copy of a fixture file, or of a session file, and opens it, how executors reach fixture files, the observation `schema_objects`, the op `schema11_copy`, which writes a schema-11 copy of the collection to a session file (P4), and the observations `file_col`, `file_objects` and `file_query`, which read such a file without opening it as a collection (P5 to P8). Scenarios also use the op `db_exec`, whose definition is `record-ops.md` W1 (U23's spec side; P2 restates what the storage scenarios rely on), and the observation `db_dump` (`observations.md` O8, U13's spec side). The behavior these observe is `specs/storage/`.

## P1. `open`

### P1.1 Argument and effect

`open: {fixture: <path>}` or `open: {file: <name>}`, exactly one of the two keys.

- `fixture` is a path relative to `conformance/fixtures/`, ending in `.anki2`, naming a file that exists (else `invalid_scenario` when the scenario loads).
- `file` names a session file (P8.1) that an earlier `schema11_copy` step of the scenario wrote, before a handoff included. A name with no such file in the session directory is a run error (`unknown_ref`). The file itself stays in the session directory, unchanged, so later steps can still read it (P5 to P7).

1. If a collection is open, close it as at a handoff (no downgrade; Meguri closes per `storage/open-and-connections` C10).
2. Delete the session's `collection.anki2` and any `collection.anki2-wal`, `-shm` and `-journal` next to it. Other files in the session directory stay.
3. Copy the fixture, or the session file, byte for byte to `collection.anki2`.
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

- A scenario with an `open` step has no `setup`, and its first step is an `open` that names a fixture. Executors create no fresh collection for it: the session starts empty and the first `open` brings the collection. Later `open` steps may name a fixture or a session file.
- Logical ids (`#nN`, `#cN`) are assigned only to notes and cards the scenario creates; a fixture's notes and cards have none. Scenarios read them by real id through `db_exec` and `db_dump`.
- A handoff after an `open` hands over the opened collection as usual (`dsl.md` D9.2), with any other file in the session directory (Meguri's pre-upgrade copy, the files `schema11_copy` wrote).

### P1.4 Where executors find fixture files

- Fixtures are read-only inputs: an executor reads a fixture's bytes to copy them, and never opens the file under `conformance/fixtures/` with SQLite, writes it or locks it.
- Meguri's executor reads them from the checkout's `conformance/fixtures/`.
- An oracle worker reads them from the directory named by the environment variable `ORACLE_FIXTURES`: the checkout's `conformance/fixtures/`, mounted read-only into the worker. Every tool that starts workers for scenario runs mounts it and sets the variable: `tools/mg oracle record` and `compare`, and the workers of `tools/mg conformance --live`.
- A worker without `ORACLE_FIXTURES`, or one in which `<ORACLE_FIXTURES>/<fixture>` is not a regular file, fails the `open` step with the protocol error kind `fixture_error` (`conformance/schema/protocol.json`), a run error. The session directory is left as it was before the step.
- The `fixture` path is resolved below that directory only: the scenario schema allows no `..`, no leading `/` and no empty segment.

## P2. `db_exec`

The definition is `record-ops.md` W1 (U23's spec side). This section restates the parts the storage scenarios rely on, and must never say anything W1 does not; when the two differ, W1 holds and this section is corrected.

### P2.1 Argument

`db_exec: {sql: <text>, params?: [<value>, ...]}`.

- `sql` is exactly one SQL statement (a trailing `;` is allowed).
- `params` binds positional parameters (`?`) in order: a string as TEXT, an integer as INTEGER, any other number as REAL, null as NULL. Booleans, arrays and objects are `invalid_scenario`.
- It runs between operations, outside any of them, on the collection's own read-write connection (on the oracle, Anki's connection with its `unicase` collation and SQL functions; on Meguri, the writer of `storage/open-and-connections` C4 with C5's registrations), and commits at once. It stamps nothing: no usn, modification time or `col.mod` changes except what the statement itself writes. On the oracle this rules out Anki's database proxy transaction, which stamps `col.mod` when a transaction in which a statement other than a `SELECT` ran commits.
- After a statement whose text does not begin with `select` (ignoring leading white space and letter case), the executor forgets every cached view of the collection and clears the undo history (W1).

### P2.2 Output

`{"db_exec": {"rows": [[<value>, ...], ...]}}`: the rows the statement returns, in the order SQLite returns them, each a list of its column values; `{"rows": []}` when it returns none. Values by the SQLite storage class of each value, as `db_dump` writes them:

| Storage class | Value |
|---|---|
| INTEGER | integer |
| REAL | number with a fraction or an exponent, the shortest decimal that reads back as the same 64-bit float (REAL 1 is `1.0`) |
| TEXT | string |
| BLOB | `{"sha256": "<lower-case hex SHA-256 of the bytes>"}` |
| NULL | `null` |

Scenarios read pragma values through SQLite's table-valued pragma functions, which are queries: `SELECT * FROM pragma_integrity_check` gives `{"rows": [["ok"]]}` on a sound file, `SELECT * FROM pragma_journal_mode` `{"rows": [["wal"]]}` on a WAL-mode file. Use `ORDER BY` whenever more than one row comes back: without it the order is SQLite's and may differ between the oracle and Meguri. Read text whose bytes matter with `hex(...)`.

### P2.3 Removed

`db_exec` records no rejection: any SQL error, a failed constraint included, is a run error (`anki_error` on the oracle, an executor error on Meguri), as W1 says. Scenarios that show a constraint at work use `INSERT OR IGNORE` and read back what was stored (`storage/open/collation`); Meguri's own tests assert SQLite's error codes.

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

## P4. `schema11_copy`

### P4.1 Argument and effect

`schema11_copy: {name: <file name>}`. `name` is a session file name (P8.1).

- Writes a schema-11 copy of the open collection (`storage/schema11-downgrade`) to the file `name` in the session directory, replacing a file of that name. The collection itself does not change, and stays open.
- Meguri: the downgrade of `storage/schema11-downgrade` D1, with the open collection as the source and that path as the destination.
- Oracle: Anki's own downgrade to schema 11, applied to a copy of the collection file, never to the collection: close the collection without the downgrade, copy its file, downgrade the copy with Anki through a path that reports a failed downgrade as an error (Anki's plain close with the downgrade logs a failure and returns normally, leaving the file at schema 18), move the result to `name`, open the collection again. Nothing of this may write the collection: its `db_dump` and `schema_objects` after the step equal those before it.
- The clock does not matter: the downgrade stamps nothing.

### P4.2 Output and rejections

- Success: `{}`.
- Rejection (`dsl.md` D7.3): `{"schema11_copy": {"error": "invalid_json"}}` when a config value is not JSON the downgrade can read (`storage/schema11-downgrade` D3.1: not one JSON value, or a number beyond the 64-bit float range, or nested too deep). Anki reports it as its JSON error kind. Nothing is written then: no file of that name appears, and one that was there keeps its bytes.
- Every other failure is a run error, on both sides: a config value stored as TEXT, a key that is not text, a blob that does not decode (Anki reports these as database or protobuf errors, which name no data fault a scenario could rely on). Scenarios avoid them; Meguri's tests cover them (`storage/schema11-downgrade` D11).

### P4.3 Use

A live scenario's file crosses a handoff with the session directory (`dsl.md` D9.2), so the oracle can read and open the copy Meguri wrote.

## P5. `file_col`

Request: `{file_col: {name: <file name>, columns: [<column>, ...]}}`. `name` as in P8.1; `columns` a non-empty list, without repeats, of `crt`, `mod`, `scm`, `ver`, `dty`, `usn`, `ls`, `conf`, `models`, `decks`, `dconf` and `tags`.

Value: `{<column>: <value>, ...}`, one member per requested column, from the single row of the file's `col` table (P8.2):

- `crt`, `mod`, `scm`, `ver`, `dty`, `usn`, `ls`: the stored integer.
- `conf`, `models`, `decks`, `dconf`, `tags`: the stored text parsed as JSON, numbers as `model-ops.md` L1 says: integers stay integers (up to 18446744073709551615, the largest a compact config value holds, `storage/config-table` C3), numbers written with a fraction or exponent stay non-integers. So `models` is `{"<note type id>": <note type object>, ...}` as `specs/model/schema11-json.md` S12 lays it out.

Compared as any value (`observations.md` O7), so the order in which a writer puts the members of a column or of its objects never counts. It is `model-ops.md` L7's `package_col` for a plain collection file.

Errors: a `col` table without exactly one row, a column of another storage class, a text that is not JSON, or JSON that repeats a member name are run errors.

## P6. `file_objects`

Request: `{file_objects: {name: <file name>}}`, `name` as in P8.1. Value: exactly what `schema_objects` (P3) reports, for the file's `sqlite_master` instead of the open collection's: one entry per row, sorted by `type` then `name`, `sql` normalized per P3.2.

## P7. `file_query`

Request: `{file_query: {name: <file name>, sql: <text>}}`, `name` as in P8.1, `sql` exactly one SQL statement that only reads (a `SELECT`, or a query of a table-valued pragma function such as `pragma_journal_mode` or `pragma_integrity_check`).

Value: `{rows: [[<value>, ...], ...]}`, the rows the statement returns, values by storage class as `db_exec` writes them (P2.2). Use `ORDER BY` whenever more than one row comes back. A statement that fails, or that would write, is a run error.

`SELECT * FROM pragma_journal_mode` reports the file's own journal mode: `delete` for a rollback-journal file, `wal` for a WAL-mode file (header bytes 18 and 19 both 2).

## P8. Session files

### P8.1 Names

A session file name is a file name of lower-case ASCII letters, digits, `.`, `_` and `-` that starts with a letter or digit, ends in `.anki2` and is not `collection.anki2` (else `invalid_scenario` when the scenario loads). Upper-case letters are refused because a case-insensitive volume (APFS, where Meguri and the native oracle lane run) gives names that differ only in case one file: `Collection.anki2` would be the collection itself, and two session files could replace each other on one side only. `open: {file}`, `file_col`, `file_objects` and `file_query` name a file that an earlier `schema11_copy` step of the scenario wrote, before a handoff included; a name with no such file in the session directory is a run error (`unknown_ref`).

### P8.2 Reading a session file

`file_col`, `file_objects` and `file_query` read the file as the SQLite database it is, never through the executor's collection open, which would upgrade it to schema 18 and empty its JSON columns.

- The read never changes the file: copy it byte for byte to scratch space and open the copy as a plain SQLite database, then discard the copy. A plain open reads the journal mode from the file header, so a copy left in WAL mode by mistake reports `wal` (P7).
- The connection registers the collation `unicase` (`storage/schema-18#S4`) and no function, so a query may name a column that uses it.
- Nothing in the collection, or in the file, changes. Each request may appear once per observe step; a scenario that reads the file several ways uses several steps.
