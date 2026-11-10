# Maintenance ops for scenarios

Spec: conformance/maintenance-ops
Requirements: R1655 (scenario side), R1640 (M2, scenario side); supports R1883
Anki: 26.09.3
Scenarios: conformance/scenarios/maintenance/optimize/; M2: conformance/scenarios/maintenance/check-db/corrupt-page.yaml (`maintenance/check-database` C18.4)

The scenario op that runs the automatic optimize of `specs/maintenance/optimize-integrity.md` M1, and the op that damages a collection file so that Check Database's integrity stage finds it corrupt (M2). Check Database's op `check_db` is `collection-check.md` K10.

## M1. `optimize`

### M1.1 Argument and output

- `optimize: {}`. Any key is `invalid_scenario`.
- Output: none (`{}`). The op records no rejection: a failure is a run error (`anki_error` on the oracle, an executor error on Meguri).

### M1.2 Effect

Runs the automatic optimize on the scenario's open collection, at once: `VACUUM`, then `ANALYZE` (`optimize-integrity` M1). The 14-day cadence and Meguri's idle and trigger rules (`optimize-integrity` M4, M5) do not apply, and nothing the op does is observed outside the collection file.

- Oracle: the Python layer's collection optimize call, which the desktop runs for its 14-day optimize. It sends the two statements, one at a time, through the collection's raw query API, outside any transaction. Anki then remembers that a statement other than a `select` ran, and would stamp `col.mod` at the next explicit commit request; like `db_exec` (`record-ops.md` W1), the driver never sends one, so the op stamps nothing. Anki also empties its undo history and study queues, as after any `db_exec` that is not a `select`.
- Meguri: the engine's optimize primitive (U132's `maintenance/optimize.rs`), the one the idle trigger runs, called directly. It runs between operations, never inside one. Its trailing checkpoint (`optimize-integrity` M8.3) is part of it.

### M1.3 What scenarios observe after it

- Stored rows and `col` stamps, with `db_exec` or `db_dump`: unchanged.
- `SELECT * FROM pragma_freelist_count` (`[[0]]`), `SELECT * FROM pragma_integrity_check` (`[["ok"]]`), `schema_objects`.
- Rows of `sqlite_stat1` for tables whose contents the scenario controls, with `db_exec`. W1's "what SQLite itself provides" includes the statistics tables. These rows are the one exception to `storage/schema-18` S5's rule that statistics rows are never compared. Never `sqlite_stat4` and never `pragma_page_count`: their values depend on the SQLite release (`optimize-integrity` M2).
- Never `undo`, `redo` or `undo_status` after an `optimize` step: Anki has emptied its histories (M1.2) and Meguri keeps them (`optimize-integrity` M2).

### M1.4 Status

Stable: the arguments, output and effect above are fixed, and the three scenarios have goldens recorded with the oracle driver's `optimize` op.

- The op runs only where the oracle driver carries it (an `optimize` op in the driver's maintenance ops, with `schema/protocol.json` describing it) and the testkit model reads it (`core/crates/meguri-testkit/src/scenario/model.rs`). Both land in the same commit as these scenarios, the `scenario.schema.json` key and the `dsl.md` D7.2 row: without them every scenario here is `invalid` (`dsl.md` D10) and T0 and `goldens-check` fail. The oracle maintainer then re-records the goldens with `tools/mg oracle record maintenance/optimize` and checks them with `tools/mg oracle compare maintenance/optimize`.
- Meguri's adapter came with U132's implementation, which lists `optimize/*` as active in `conformance/scenarios/maintenance/area.yaml` (the comment above that list still says that no scenario is active; the file belongs to the change that activates scenarios).

## M2. `corrupt_page`

The way a scenario gives Check Database a file that fails its integrity check (`maintenance/check-database` C4, C13; `collection-check.md` K10 `corrupt`). `db_exec` cannot do it: SQL writes go through SQLite, which keeps the file sound.

### M2.1 Argument and output

- `corrupt_page: {table: <name>}`. `table` is one of the twelve tables of `storage/schema-18` S2; any other value, or any other key, is `invalid_scenario`.
- Output: none (`{}`). The op records no rejection: a failure (the file cannot be written or opened again) is a run error.

### M2.2 Effect

Both executors do the same thing to the file:

1. Close the scenario's collection as `reopen` does (`sync-ops.md` S6). When a `collection.anki2-wal` file remains, open the file with a plain SQLite connection, run `PRAGMA wal_checkpoint(TRUNCATE)`, and close that connection, so no newer copy of a page in the log hides the edit.
2. Read the page size from the main file's header (bytes 16 and 17, big-endian; the value 1 means 65,536) and the table's root page number from `sqlite_schema` (`rootpage`), with that connection before closing it, or with a read-only one.
3. Overwrite that whole page of the main file with bytes `0xFF`. Page 1 is never chosen: no table of S2 has its root there.
4. Open the collection again as `reopen` does. Logical ids stay valid.

- Oracle: the driver does steps 1 to 4 around Anki's collection close and open, with Python's file and SQLite modules. When Anki cannot open the damaged file again (a table its open reads), the step is a run error (`anki_error`).
- Meguri: the executor does steps 1 to 3 on the closed session file and step 4 through the engine's open. The engine's open does not check integrity after a clean close (`maintenance/optimize-integrity` M5.1), so the open succeeds and the collection is `Ready`.

### M2.3 Writing scenarios with it

- Damage `revlog`: Anki's open does not read it, and an open that needs a review-log row is a Meguri executor error to fix, not a reason to pick another table. A scenario never reads the damaged table afterwards (a read of the page is an SQLite error, a run error on both sides), and after the op runs only `check_db`, `advance`, `set_clock` and `observe` kinds that read no collection.
- `check_db` then records `{"check_db": {"error": "corrupt"}}` on both sides. On Meguri, Check Database's corrupt verdict also moves the collection to `Failed{Corrupt}` (C4), where write ops are refused: so no op that writes follows it. A second `check_db` records `corrupt` again (Check Database runs from `Failed{Corrupt}`, `maintenance/optimize-integrity` M7.2).
- Not in actor scenarios, not before a `handoff` (Meguri hands over no file in `Failed{Corrupt}`), and never between an `empty_cards` or `change_notetype_info` step and the step that uses it (it reopens the collection). The oracle refuses the first two before any step runs, as `invalid_scenario` with the index of the `corrupt_page` step in an actor scenario, or of the first `handoff` after it.

### M2.4 Status

Draft (`dsl.md` D7.2): the arguments and effect above are fixed, and the oracle implements them (the driver's maintenance ops, `schema/protocol.json`, the `scenario.schema.json` shape). `maintenance/check-db/corrupt-page` (`maintenance/check-database` C18.4) has its golden, recorded with `tools/mg oracle record maintenance/check-db/corrupt-page`. Pending: the testkit model (`core/crates/meguri-testkit/src/scenario/`) and Meguri's executor. Until the model reads the op, the scenario is `invalid` in T0 (`dsl.md` D10), so it lands with that model; it stays inactive until U128's adapter runs it.
