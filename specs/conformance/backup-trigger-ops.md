# Backup trigger ops for scenarios

Spec: conformance/backup-trigger-ops
Requirements: R1630, R1632, R2008
Anki: 26.09.3
Scenarios: conformance/scenarios/package/backup-triggers/

The scenario vocabulary for when backups happen (U127): the op `backup`, on top of the backups folder, ops and kinds of `backup-ops.md` (F1 to F6). It belongs to the package area (`dsl.md` D7.2). The behavior observed is `specs/package/backup-triggers.md` T2 and T4.

## H1. `backup`

- **Argument** `{force: <boolean>}`; `force` is required.
- **Effect**, into the scenario's backups folder (`backup-ops.md` F1), complete when the step ends:
  - Oracle: Anki's collection backup with that force flag, waiting for it to finish (its thinning included).
  - Meguri, `force: false`: the rule of `package/backup-triggers#T2.3` evaluated at the scenario's current instant and, when it is due, the backup of `package/backups#B4`, retention included. The scheduler's deadline and 5-minute floor (T3) take no part, so the step never waits.
  - Meguri, `force: true`: Back Up Now (`package/backup-triggers#T5`).
- **Output** `{backup: {name: <file name> | null}}`: the name of the file the step added to the folder, or `null` when it made no backup.
  - Oracle: the one name present after the call and not before. When Anki reports a backup but no name was added (it replaced a file of the same name, H3), the run fails with `invalid_scenario`.
- No rejection kind. Any other failure is a run error (`anki_error` on the oracle, an executor error on Meguri).

## H2. Creation times in scenarios

The interval rule reads the creation times of the folder's entries (`package/backup-triggers#T2.2`).

- In an oracle worker, libfaketime freezes Anki's clock at the scenario's instant while the kernel stamps files with the real time, and libfaketime moves every file time it reports by the frozen clock's current lead over the real clock. So each entry reads as created at the scenario's current instant minus the real time since its creation (well under a second in a scenario), however far the scenario clock moved since.
- The Meguri executor supplies adjusted entry times through R1875.
- Hence in scenarios: with an interval of at least 1 minute, any entry of the folder blocks `backup {force: false}`; with an interval of 0, no entry does. No scenario can show an entry ageing past the interval; Meguri's scheduler tests do, with injected entry times (`package/backup-triggers#T10`).

## H3. Scenario rules

- **No collision.** The name a `backup` step would take must not exist in the folder: Anki would replace that file and Meguri would take the next second (`package/backups#B4`). `advance` at least 1 s between two steps that can both make a backup, and never seed that name.
- **Mixing with `force_backup`.** `backup` steps may come before and after a `force_backup`; `backup-ops.md` F7's one-per-opening rule counts `force_backup` steps only. Every backup counts as the last backup for T2.1, `force_backup` included (`package/backup-triggers#T2.3`).
- **Isolating the rules.** Scenarios about the change rule set `minimum_interval_mins` to 0 in `setup.config`, so no entry blocks; scenarios about the interval keep the default 30 and an entry in the folder.
- **Undo.** A scenario that undoes a change follows `undo-ops.md` V5 (a `db_exec` that is not a `select` before the first `undo`).
- **Frozen milliseconds.** Under a frozen clock an op stamps `col.mod` with the current instant. When the reference was stamped at that same instant (the setup and the last backup both ran at it, with no `advance` since), the op leaves `col.mod` at the reference and is no change (T2.1): `advance` before a change that must count.

## H4. Scenarios and what runs them

| Scenario | Shows | Deviation |
|---|---|---|
| `change-check` | T2.1 at interval 0: a `force_backup` sets the reference; no change, a change in the same millisecond, a change, `col.mod` moved back | |
| `change-check-undo` | T2.1: an undo that puts `col.mod` back is no change | |
| `foreign-entry` | T2.2: a foreign file blocks | |
| `forced-unchanged` | T4.2: a forced backup with no change | `backups-forced-always-written` |
| `reopen` | T2.3: the reference after a reopen | `backups-change-check-survives-reopen` |

- **Capabilities** (`dsl.md` D8): the op `backup`, besides the new collection, `setup.config`, `setup.notetypes`, `add_note`, `advance`, `db_exec`, `reopen`, `undo` (only `change-check-undo`), and `backup-ops.md`'s `add_backup_files`, `force_backup` and `backup_files`. Meguri declares `backup` in the package adapter (`core/crates/meguri-testkit/src/exec/package.rs`, U127), with the testkit's scenario model.
- The oracle driver implements H1. Every scenario has a golden, recorded with `tools/mg oracle record package/backup-triggers`.
