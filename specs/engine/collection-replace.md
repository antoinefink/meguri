# Verified collection replace

Spec: engine/collection-replace
Requirements: R2004 (replace primitive), R2005, R1489, R1548 (the pause signal; U113 and U107 enforce it), R1638 (data side); supports R1543, R1626, R2003, R2010, R2019, and R1478, R1479, R1482, R2013, R2016 through the open gate
Anki: 26.09.3 for R7 (what a replacement from a collection package stamps); every other section is Meguri design
Scenarios: conformance/scenarios/package/collection-package/ (R7); tests in `core/crates/meguri-engine/tests/replace.rs`

The only way Meguri swaps the collection file for another file: a full download or upload (U109), a restore (U127) and a `.colpkg` import (U124). The primitive checks the incoming file with the open gate and the integrity check, applies the change its reason calls for, makes the file self-contained, and swaps it in during the exclusive phase as the swap step of `engine/lifecycle#L8`. It also keeps the device-only state that replacements and the sync pause need (R6). Reading and inspecting packages is `package/container`.

## R1. Reasons

| Reason | Caller | The staged file comes from | Change before the swap (R3 step 3) | Sync pause (R6) |
|---|---|---|---|---|
| `Restore` | U127 | inspecting a backup (`package/container#P12`) | the primitive's package stamps: `scm` and `mod` set to the replace instant (R7) | set |
| `ColpkgImport` | U124 | inspecting a `.colpkg` | the package stamps, then any change U124 passes | set |
| `FullDownload` | U109 | the server's file, streamed into staging | U109's change: `ls` = `mod` (R1543) | untouched |
| `FullUpload` | U109 | the file U109 transformed and uploaded | none (U109 changed it before the upload) | untouched |

The package stamps are Anki's behavior (R7), so the primitive applies them itself for both package reasons; U127 passes no change, and U124 passes only what its own spec adds. `ColpkgImport` exists in the type from this unit on; its caller lands with U124.

## R2. Input

- **The staged file**: `collection.anki2` in a subdirectory of `<collection directory>/meguri-staging/` (`package/container#P9`), handed over as an inspect handle or by the caller that wrote it there. It must be on the collection's file system (same device), else `CrossDevice`. No connection other than the primitive's own may be open on it.
- **The reason**, and the caller's change if any: a function the primitive runs inside the R3 step 3 transaction on the staged file.
- **The exclusive phase.** The caller holds the long-operation slot and runs the phase's `Working` step with its privileged handle (`engine/lifecycle#L5`, `#L7`); otherwise `NotExclusive`, and nothing is touched. With no collection open (a first download, a recovery from `Failed`), holding the slot is enough (L5).
- The process keeps its advisory lock on the collection (R2014) through the whole replacement.

## R3. Checking the staged file (R2005)

For every reason, in this order, before the live collection is touched:

1. The open gate on the staged file, every phase (`storage/open-and-connections#C2`): the refusals of R1479, R1482 and R2013 (`StagingRefused{gate}`), the upgrade of schemas 11 and 14 to 17 applied to this file only (R1478; its pre-upgrade copy lands in the staging subdirectory and goes with it), and the expected-object check (R2016).
2. `PRAGMA integrity_check` on the gate's writer, `unicase` registered: anything but the single row `ok` is `StagingCorrupt`.
3. The change of R1, as one `IMMEDIATE` transaction on that writer: the package stamps first for `Restore` and `ColpkgImport`, then the caller's change. A failure is `CallerChangeFailed` and rolls back.

A file that inspect has already checked is checked again: what is swapped in must have passed the checks in the state it is swapped in.

## R4. Finalising the staged file

1. On the staged file's writer: `PRAGMA wal_checkpoint(TRUNCATE)`, which must report "not busy", then `PRAGMA journal_mode=DELETE`, which must return `delete`.
2. Close every connection to the staged file.
3. No `collection.anki2-wal`, `-shm` or `-journal` may remain in the staging subdirectory; otherwise `StagingNotFinalised`.
4. `PRAGMA integrity_check` again, on a read-only connection opened with `mode=ro&immutable=1` (so no file appears next to it) and `unicase` registered: `ok`, else `StagingCorrupt`.
5. A full sync of the staged file (`F_FULLFSYNC` on Apple platforms, `storage/open-and-connections#C9.2`).

The result is one self-contained file: everything committed is in it, and it needs no journal file. A staging copy whose changes were still in its `-wal` after step 3 of R3 has them folded in by step 1.

## R5. The swap

Runs as the swap of `engine/lifecycle#L8` (step 2), after L8 step 1 has set the phase to `Replacing` and closed every connection to the live collection: every reader (U107's snapshot reader and its stage attachment and the derived-store attachments included, after their read transactions end), then the writer with its checkpoint (`storage/open-and-connections#C10`).

1. The live `collection.anki2-wal` must be absent or empty (0 bytes). If not, nothing is changed and the replace fails with `LiveWalNotEmpty`: L8 reopens the old file and keeps the generation.
2. For `Restore` and `ColpkgImport`, record the sync pause (R6.2).
3. Remove the live `collection.anki2-shm` and `collection.anki2-wal`. They hold nothing (step 1); a stale `-wal` left next to the new file would be applied to it by SQLite.
4. Rename the staged file over `collection.anki2`: one `rename`, on one file system.
5. A full sync of the collection directory.
6. Remove the staging subdirectory (best effort; the leftover sweep removes what a kill leaves, `package/container#P9`).

Then L8 steps 3 to 5 run as written there: the open through the gate, the undo history cleared, the generation plus one, `CollectionReplaced{generation}` published once, the day clock restarted, readers reopened lazily, and every `GenerationListener` told, so the derived store binds to the new file and rebuilds (R1489, `storage/derived-store`). With no collection open before, R5 has no live file to check, and step 4 creates `collection.anki2`.

## R6. Device-only state (R1548, R2010, R2019)

### R6.1 Where it lives

One member `deviceState` of U32's sidecar `meguri-sidecar.json` in the collection directory (KTD12). It is never synced and never written into the collection (R1468), and there is no other copy. It is written through U32's atomic sidecar write (temporary file, full sync, rename). A missing or unreadable member reads as the defaults below; the pause flag then reads as off, which is safe because a replaced collection carries a new `scm` (R7), so its next sync is a one-way sync anyway.

| Field | Default | Written by | Read by |
|---|---|---|---|
| `oneWayChoicePending`: the automatic-sync pause | false | R6.2 (set), U109 (cleared after a user-started one-way sync succeeds) | U113's gate, U107 |
| `pendingScm`: the `scm` of the collection the pause belongs to | none | R6.2 | R6.3 |
| `pauseBeforeReplace`: the pause flag and `pendingScm` as they were before a replace still in progress | none | R6.2 | R6.3 |
| last backup: the `col.mod` it holds, its time, its file name | none | U126, U127 | U127 ("changed since the last backup"), U132 |
| last Check Database: time and result | none | U128 | U132 |
| last optimize: time | none | U132, U109 after an upload | U132 |
| last quick check: time and whether it passed | none | U132 | U132 |
| server counts of the last failed sanity check | none | U107 | U109 (and cleared by it after a one-way sync succeeds) |

Member names inside `deviceState` are the implementer's. The accessor (`maintenance/device_state.rs`) gives typed reads and writes of each field.

### R6.2 Around a replacement

- `Restore` and `ColpkgImport`, at R5 step 2, in one sidecar write: `pauseBeforeReplace` = the current flag and `pendingScm`; then `oneWayChoicePending` = true and `pendingScm` = the staged file's `scm` (after R3 and R4). The write is durable before the rename, so a kill after the rename cannot lose the pause.
- When the replace fails or is cancelled before the rename: the flag and `pendingScm` go back to `pauseBeforeReplace`, which is then removed. A pause that an earlier restore set therefore survives a failed second restore.
- After a successful swap and reopen: `pauseBeforeReplace` is removed.
- `FullDownload` and `FullUpload` leave all three fields alone; U109 clears the flag and the stored sanity counts once the one-way sync the user started has succeeded.

### R6.3 At open

When the collection opens and `pauseBeforeReplace` is present (a kill interrupted a replacement):

- the live `scm` equals `pendingScm`: the swap happened; keep the flag on and remove `pauseBeforeReplace`;
- otherwise the swap never happened: restore the flag and `pendingScm` from `pauseBeforeReplace` and remove it.

When the flag is on with no `pauseBeforeReplace` and the live `scm` differs from `pendingScm`, a one-way sync replaced the collection after the pause was set: clear the flag. If a replaced collection's new `scm` happened to equal the old one (a clock set back), the pause stays on although the swap failed; the user's next one-way sync lifts it.

## R7. What a replacement from a collection package stamps (Anki 26.9.3)

- Anki's restore of a backup and its `.colpkg` import are one operation. On its copy of the incoming collection, after the copy has opened (upgraded when older) and before the swap, it sets `col.scm` and `col.mod` to the current time in epoch milliseconds. Nothing else in the file changes: no `usn`, no `ls`, no row of any other table.
- The new `scm`, with the new `mod`, makes the next sync with the server a one-way sync that the user must choose (`sync/meta-status#M3`, R1548; Anki also turns automatic sync off until it restarts).
- Meguri's primitive makes the same change for `Restore` and `ColpkgImport` (R3 step 3), with one instant for both columns. Under a scenario's frozen clock both equal the scenario's current instant in milliseconds.
- Scenarios: `package/collection-package/restore-backup`, `upgrade-on-import` and the import half of `export-import-roundtrip` observe the stamped `col` row in `db_dump` (`specs/conformance/package-ops.md` E2).

## R8. Failures and kills

- **Before the rename** (R3, R4 and R5 steps 1 to 3): the old collection file, its generation and every handle stay as they were; the staging subdirectory is removed (or swept at the next launch after a kill); the pause is as R6.2 and R6.3 say. R3 and R4 run before L8's `Replacing` step, so a refused file never closes the live collection.
- **After the rename**: the new file stands. After a kill, the next launch opens it, and it passes the integrity check (R2003, the restore analogue of AE86).
- A failed open after the swap: L8 sets `Failed{reason}`.
- Fault points (U21's `faultpoint!`, cataloged by U250): `replace.after_check` (after R3), `replace.after_finalise` (after R4), `replace.after_pause_recorded` (R5 step 2), `replace.after_live_wal_removed` (R5 step 3), `replace.after_rename` (R5 step 4) and `replace.after_dir_sync` (R5 step 5).

## R9. Errors

| Error | When |
|---|---|
| `NotExclusive` | R2: the caller is not in the exclusive phase (with a collection open) or does not hold the slot |
| `CrossDevice` | R2: the staged file is not on the collection's file system |
| `StagingRefused{gate}` | R3 step 1: the open gate refused or failed (`TooOld`, `TooNew`, `V1Scheduler`, `NotACollection`, `UpgradeFailed`, `Corrupt`) |
| `StagingCorrupt` | R3 step 2 or R4 step 4 |
| `CallerChangeFailed{source}` | R3 step 3 |
| `StagingNotFinalised` | R4 step 3 |
| `LiveWalNotEmpty` | R5 step 1 |
| `Io` | any other file-system failure |

The lifecycle's own errors (`CollectionUnavailable`, `StaleGeneration`, `engine/lifecycle#L5`, `#L7`) pass through unchanged.

## R10. Tests

| What | Where |
|---|---|
| `Restore` of the backup fixture: the dump equals the oracle's restore of it | `package/collection-package/restore-backup` (Meguri's `import` adapter runs inspect, then this primitive with `Restore`) |
| After a `Restore`: `CollectionReplaced(generation + 1)` once, older readers closed and their handles failing with `StaleGeneration`, the derived store rebuilding, `oneWayChoicePending` on with `pendingScm` equal to the new `scm` | `core/crates/meguri-engine/tests/replace.rs` |
| Kill at `replace.after_pause_recorded` (and every fault point before it): the relaunch opens the old file unchanged (SHA-256), the pause is back to its value before the replace; a cancel before the rename does the same at once | same file |
| A pause from an earlier restore survives a second restore killed before its rename | same file |
| Kill at `replace.after_rename` and `replace.after_dir_sync`: the relaunch opens the new file, `integrity_check` `ok`, the pause on | same file |
| `FullDownload` with a truncated staged file: `StagingCorrupt` (or the gate's `Corrupt`), the old collection unchanged; U109 reuses this fixture | same file |
| `FullDownload` with a schema-11 staged file: upgraded on the staged copy; a kill right after the rename reopens a schema-18 collection | same file |
| A non-empty live `-wal` injected before the swap: `LiveWalNotEmpty`; the old collection opens and passes `integrity_check` | same file |
| A staged copy whose change is still in its `-wal`: R4 folds it in, and the swapped-in file holds the change | same file |
| Stored sanity counts survive a relaunch; U109's success path clears them (U109's test) | same file |
| The caller is not in the exclusive phase: `NotExclusive`, nothing touched | same file |
