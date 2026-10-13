# Record ops: raw SQL, note edits, note deletion and config writes

Spec: conformance/record-ops
Requirements: R1883, R323, R776, R1466, R1471, R1472, R1473, R1474
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/

Four scenario ops (`dsl.md` D7.2) that the storage scenarios need: `db_exec` prepares and reads stored rows directly, `edit_note` and `delete_notes` run the note operations whose stamps and graves `specs/storage/` describes, and `set_config` writes one config key. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The notes area (U27) may add arguments to `edit_note` and `delete_notes` with their driver support; the meanings here stay.

## W1. `db_exec`

Runs one SQL statement directly on the collection database.

- **Argument** `{sql, params?}`:
  - `sql`: exactly one SQL statement, optionally ending with `;`, on the tables of the schema-18 collection file (`col`, `notes`, `cards`, `revlog`, `graves`, `decks`, `deck_config`, `notetypes`, `fields`, `templates`, `tags`, `config`). Placeholders are positional `?` only.
  - `params`: the values bound to the placeholders in order. A string binds as TEXT, an integer as INTEGER, any other number as REAL, `null` as NULL. Booleans, arrays and objects are invalid (`invalid_scenario`). Bytes are written with `CAST(? AS BLOB)` in the SQL.
- **Output** `{db_exec: {rows: [[<value>, ...], ...]}}`: the rows the statement returns, in the order SQLite returns them, each a list of its column values. A statement that returns no rows, which includes every statement that is not a query, gives `{rows: []}`. A value follows the SQLite storage class of the returned value, not the column's declared type:

  | Storage class | Value |
  |---|---|
  | INTEGER | integer |
  | REAL | number written with a fraction or an exponent: the shortest decimal that reads back as the same 64-bit float (REAL 1 is `1.0`) |
  | TEXT | string; byte sequences that are not UTF-8 become U+FFFD, one per invalid sequence |
  | BLOB | `{"sha256": "<lower-case hex SHA-256 of the bytes>"}` |
  | NULL | `null` |

  Scenarios read bytes they want to compare as text, for example `CAST(val AS TEXT)`, and give an `ORDER BY` whenever a query can return more than one row.
- **Effect.** The statement runs between operations, outside any of them, and commits at once. It stamps nothing: no `usn`, modification time or `col.mod` changes except what the statement itself writes, and it records no change. After a statement whose text does not begin with `select` (ignoring leading whitespace and letter case), the executor forgets every cached view of the collection (study queues, caches, derived data) and clears the undo history, as Anki does after a raw write.
- **Errors.** An SQL error (syntax, constraint, unknown table) is a run error, not a rejection: `anki_error` on the oracle, an executor error on Meguri. Scenarios use statements that succeed.
- **Uses.** Preparing states that another client or an older sync could leave (a server `usn`, config bytes in another layout, an existing grave), injecting corruption, and reading stored rows that no observation kind reports. Anki keeps note types, decks and deck presets cached across operations and may go on using its copy after a raw write, so a scenario that rewrites those rows observes them only through `db_exec` afterwards.

## W2. `edit_note`

Edits a note's fields and tags and saves it with the note-update operation Anki's editor uses (undoable).

- **Argument** `{note: #nN, fields?, tags?}`:
  - `fields`: a list replaces the note's fields from the first one in order, and fields after the list keep their stored text; a mapping replaces the named fields. Values are strings. A list longer than the note's field count, or an unknown field name, is `invalid_scenario`.
  - `tags`: replaces the note's tags with this list, in this order. When missing, the tags stay.
  - The note's other properties, its note type included, stay.
- **Output**: none (`{}`).
- **Effect.** The executor loads the stored note when the step runs, applies the changes and saves it. Saving a note whose fields, tags and note type equal the stored ones writes nothing (`specs/storage/ids-usn-mtime.md` S4, R776); a real change stamps the note and moves `col.mod` (S2, S5). Card generation and tag registration happen as on any note save (U27, U30).
- **Errors.** Editing a note that no longer exists is a run error.

## W3. `delete_notes`

Removes notes with Anki's note-removal operation (undoable).

- **Argument** `{notes: [#nN, ...]}`, at least one.
- **Output**: none (`{}`).
- **Effect.** Each listed note that exists is removed with all its cards, and graves are added (`specs/storage/graves.md` G3). A listed note that no longer exists is skipped without error. An operation that removed nothing changes nothing, `col.mod` included. A deleted note's and its cards' logical ids stay valid: `ids` still reports their real ids, and `card` reports `null` for the removed cards.

## W4. `set_config`

Writes one collection config key through the collection's config API, without an undo entry and keeping the undo history.

- **Argument** `{key, value}`: `key` a non-empty string, `value` any JSON value, `null` included. A number written with a fraction or an exponent (`1.0`, `1.0e16`) is a float and keeps its kind; one without is an integer and must lie in the signed or unsigned 64-bit range (`invalid_scenario` otherwise). Member order in the scenario does not matter: the value is written in compact form with members sorted (`specs/storage/config-table.md` C5).
- **Output**: none (`{}`).
- **Effect.** As `specs/storage/config-table.md` C4, as one operation that keeps no undo entry: a real change stamps the row and moves `col.mod`. The oracle writes the value whenever its compact bytes differ from the stored bytes; Meguri writes only when the value differs (deviation `config-same-value-other-bytes`).

## W5. Driver status

The oracle driver implements these four ops and `dsl.md` D7.2 marks them stable; scenarios that use them have goldens. The schema shapes (`scenario.schema.json`: `additionalProperties: false` with the arguments above) and the testkit model change together in U23's implementer step, as `conformance/AGENTS.md` asks; until then the schema keeps `edit_note`, `delete_notes` and `db_exec` on `draft_args`. Until Meguri's executor implements an op, scenarios that use it are `pending` (`dsl.md` D10): U23's adapters implement `db_exec` and `set_config`, the notes area's (U27) `edit_note` and `delete_notes`. `set_config` is new: until the testkit model reads it, T0 reports the scenarios that use it as `invalid`.
