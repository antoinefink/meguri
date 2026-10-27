# Maintenance ops for scenarios

Spec: conformance/maintenance-ops
Requirements: R1655 (scenario side); supports R1883
Anki: 26.09.3
Scenarios: conformance/scenarios/maintenance/optimize/

The scenario op that runs the automatic optimize of `specs/maintenance/optimize-integrity.md` M1. Check Database's op `check_db` is `collection-check.md` K10.

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
- Meguri's adapter comes with U132's implementation, which then lists `optimize/*` in `conformance/scenarios/maintenance/area.yaml`.
