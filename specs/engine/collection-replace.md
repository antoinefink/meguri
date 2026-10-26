# Verified collection replace

Spec: engine/collection-replace
Requirements: R2004 (replace primitive), R2005, R1489, R1548 (publication decision; U113/U114 resolve it), R2051, R2056, R2066, R1638 (data side); supports R1543, R1626, R2003, R2010, R2019, and R1478, R1479, R1482, R2013, R2016 through the open gate
Anki: 26.09.3 for R7 (what a replacement from a collection package stamps); every other section is Meguri design
Scenarios: conformance/scenarios/package/collection-package/ (R7); tests in `core/crates/meguri-engine/tests/replace.rs`

The only way Meguri swaps the collection file for another file: a full download or upload (U109), a restore (U127) and a `.colpkg` import (U124). The primitive checks the incoming file with the open gate and the integrity check, applies the change its reason calls for, makes the file self-contained, and swaps it in during the exclusive phase as the swap step of `engine/lifecycle#L8`. It also keeps the device-only replacement and publication recovery state (R6). Reading and inspecting packages is `package/container`.

## R1. Reasons

| Reason | Caller | The staged file comes from | Change before the swap (R3 step 3) | Publication recovery (R6) |
|---|---|---|---|---|
| `Restore` | U127 | inspecting a backup (`package/container#P12`) | the primitive's package stamps: `scm` and `mod` set to the replace instant (R7) | set |
| `ColpkgImport` | U166 minimal caller; U124 full flow | inspecting a `.colpkg` | the package stamps, then any change U124 passes | set |
| `FullDownload` | U109 | the server's file, streamed into staging | U109's change: `ls` = `mod` (R1543) | manual Import: fresh publication identity and pending fence under R6, even if the old decision was clear |
| `FullUpload` | U109 | the file U109 transformed and uploaded | none (U109 changed it before the upload) | preserve the cloud decision; manual policy remains separate |

The package stamps are Anki's behavior (R7), so the primitive applies them itself for both package reasons; U127 passes no change, and U124 passes only what its own spec adds. `ColpkgImport` exists in the type from this unit on; U166 supplies its minimal collection-only caller; U124 supplies the full flow.

## R2. Input

- **The staged file**: `collection.anki2` in a staging directory that U32's staging-directory primitive made in the collection directory (`package/container#P9`), handed over as an inspect handle or by the caller that wrote it there. It must be on the collection's file system (same device), else `CrossDevice`. No connection other than the primitive's own may be open on it.
- **The reason**, and the caller's change if any: a function the primitive runs inside the R3 step 3 transaction on the staged file.
- **The exclusive phase.** The caller holds the long-operation slot and runs the phase's `Working` step with its privileged handle (`engine/lifecycle#L5`, `#L7`); otherwise `NotExclusive`, and nothing is touched. With no collection open (a first download, a recovery from `Failed`), holding the slot is enough (L5).
- The process keeps its advisory lock on the collection (R2014) through the whole replacement.

## R3. Checking the staged file (R2005)

For every reason, in this order, before the live collection is touched:

1. The open gate on the staged file, every phase (`storage/open-and-connections#C2`): the refusals of R1479, R1482 and R2013 (`StagingRefused{gate}`), the upgrade of schemas 11 and 14 to 17 applied to this file only (R1478; its pre-upgrade copy lands in the staging directory and goes with it), and the expected-object check (R2016).
2. `PRAGMA integrity_check` on the gate's writer, `unicase` registered: anything but the single row `ok` is `StagingCorrupt`.
3. The change of R1, as one `IMMEDIATE` transaction on that writer: the package stamps first for `Restore` and `ColpkgImport`, then the caller's change. A failure is `CallerChangeFailed` and rolls back.

A file that inspect has already checked is checked again: what is swapped in must have passed the checks in the state it is swapped in.

## R4. Finalising the staged file

1. On the staged file's writer: `PRAGMA wal_checkpoint(TRUNCATE)`, which must report "not busy", then `PRAGMA journal_mode=DELETE`, which must return `delete`.
2. Close every connection to the staged file.
3. No `collection.anki2-wal`, `-shm` or `-journal` may remain in the staging directory; otherwise `StagingNotFinalised`.
4. `PRAGMA integrity_check` again, on a read-only connection opened with `mode=ro&immutable=1` (so no file appears next to it) and `unicase` registered: `ok`, else `StagingCorrupt`.
5. A full sync of the staged file (`F_FULLFSYNC` on Apple platforms, `storage/open-and-connections#C9.2`).

The result is one self-contained file: everything committed is in it, and it needs no journal file. A staging copy whose changes were still in its `-wal` after step 3 of R3 has them folded in by step 1.

## R5. The swap

Runs as the swap of `engine/lifecycle#L8` (step 2), after L8 step 1 has set the phase to `Replacing` and closed every connection to the live collection: every reader (U107's snapshot reader and its stage attachment and the derived-store attachments included, after their read transactions end), then the writer with its checkpoint (`storage/open-and-connections#C10`).

1. The live `collection.anki2-wal` must be absent or empty (0 bytes). If not, nothing is changed and the replace fails with `LiveWalNotEmpty`: L8 reopens the old file and keeps the generation.
2. For `Restore` and `ColpkgImport`, durably record the replacement intent and publication fence (R6.2).
3. Remove the live `collection.anki2-shm` and `collection.anki2-wal`. They hold nothing (step 1); a stale `-wal` left next to the new file would be applied to it by SQLite.
4. Rename the staged file over `collection.anki2`: one `rename`, on one file system.
5. A full sync of the collection directory.
6. Remove the staging directory (best effort; the leftover sweep removes what a kill leaves, `package/container#P9`).

Complete R6.3 durable activation before opening any writer. Local collection-file media input also completes R11 before reopen. Then L8 steps 3 to 5 run as written there: the open through the gate, the undo history cleared, the generation plus one, `CollectionReplaced{generation}` published once, the day clock restarted, readers reopened lazily, and every `GenerationListener` told, so the derived store binds to the new file and rebuilds (R1489, `storage/derived-store`). With no collection open before, R5 has no live file to check, and step 4 creates `collection.anki2`.

That reopen is not an external change (R1453). U32's open compares the file's `mod` and `scm` with the last values it saw (`storage/durability-and-files#F3`), and a replaced file always differs. At L8's reopen the replace itself is the change, and L8 step 4 already handles it. So that reopen reports no external change: no `ChangedExternally` undo clear and no whole-collection change set besides `CollectionReplaced`. The sidecar's last-seen `mod` and `scm` become the new file's, as at any open. At a relaunch after a kill, the ordinary open check applies.

## R6. Device-only state and replacement recovery (R1548, R2010, R2051, R2056)

### R6.1 Durable identities and decisions

Keep `deviceState` in U32's `meguri-sidecar.json`. It is app-private, never synced and never stored in collection rows.
All sidecar writers share one lock and preserve other members. Publish changes through U32's durable atomic write.

A publication generation is a persistent opaque identity, independent of the process's lifecycle generation counter.
Give the existing local collection an identity before its first replacement. Restore, ColpkgImport and manual Import through FullDownload each get a fresh persistent publication identity.
The candidate captures the account/configuration/environment binding explicitly, including unbound local-only use.
Manual Import always sets cloudPublicationPending=true; an old clear decision never authorizes the imported data.
Bind each publication decision to its collection identity and account identity. Include the app configuration and cloud environment when connected.
An explicit unbound account means local-only.
Local-only use needs no Apple account or network. Connecting later must not reinterpret an unbound decision as permission to upload.

`cloudPublicationPending` means publication requires an explicit decision. It blocks sending; it does not prevent local review.
Keep legacy `oneWayChoicePending` and its manual-transfer state separate. Neither `scm` nor a successful Anki transfer resolves the cloud decision.
Only U113/U114 may resolve it after the explicit cloud/local choice and their required recovery checks.
Keep old-generation queued work isolated; no callback, retry or restart may apply it as work for the replacement generation.

Retain these other prepared device fields and their consumers:

| Value | Writer and consumer |
|---|---|
| Last backup: snapshot `col.mod`, instant and file name | U126/U127; U127 change check and U132 |
| Last Check Database: instant and result | U128; U132 |
| Last optimize instant | U132/U109; U132 |
| Last quick check: instant and pass/fail | U132 |
| Last failed manual-transfer sanity counts | U107; U109 clears them after successful manual one-way transfer |

Their accessor owns no CloudKit transport, record format, account discovery or network work.

### R6.2 Before activation

Restore, ColpkgImport and FullDownload used for manual Import use this handshake after validation/finalisation
and after all live writes drain. FullDownload retains its ls=mod caller transform, not R7 package stamps.
An in-progress replacement blocks publication for both the old and candidate generations.

The durable intent records:

- an operation identity and replacement reason;
- the old and candidate publication identities, with the captured account binding;
- the old generation's complete prior publication decision, including an already-pending decision;
- identities of the closed old and final candidate files: byte length plus a streaming SHA-256 digest;
- explicit absence when no old collection file exists, and the intent's current phase.

Compute these file identities after final checkpoints and connection closure. No writer may mutate either file before the swap.
Persist the intent and candidate pending fence before renaming. A failed durable write prevents the rename.
The candidate identity includes its final R7 stamps. Equal timestamps never imply equal generation identities.
Use the ordinary R5 atomic swap and directory sync.

### R6.3 After activation and on reopen

After the rename and directory sync, durably mark the candidate generation active and pending before opening any new writer.
Then reopen through the existing lifecycle. Record the final decision before removing the intent.
A post-rename failure still leaves the candidate active and fenced; an error alone does not prove the old collection survived.

At startup reconcile an unfinished intent before opening the collection or running any mutating day check:

| Proven state | Durable result |
|---|---|
| Intent phase; only old file identity matches, including recorded absence | Restore the old identity and its exact prior decision, then remove the intent |
| Intent phase; only candidate file identity matches | Activate the candidate identity with publication pending, then complete the intent |
| Durable activation exists | Retain the target generation and its fence. Later writes may change the database hash. |
| Both match, neither matches, or metadata is unreadable | Block publication. Keep the journal for recovery. |

A known pre-rename cancel or failure restores the prior decision only after proving the old file remains live.
Prior-clear returns to clear; prior-pending remains pending. The captured account binding must still match.
An account mismatch or uncertain binding keeps publication paused. It never transfers authorization to another account.
Ambiguity may allow validated local review, but never automatic publication or silent replacement of recovery evidence.
Legacy manual-transfer bookkeeping follows its own contract. A manual transfer cannot remove this fence.
Manual Import is pending after proven candidate activation, regardless of the old decision. Proven old-file
recovery restores its exact prior decision; uncertain file/account identity stays paused. Old-generation cloud
callbacks and queued work cannot publish the imported generation. Only U113/U114's separate cloud/local decision
may resolve it. FullUpload/manual success never clears an existing cloud decision.

### R6.4 Scope and verification

RF2 implements this local durable boundary only. Future CloudKit integration must use it, not replace it with a disposable cache.
Test old clear/pending states, no old file, account mismatch, identical timestamps, identical file bytes, unreadable metadata,
and kills before intent, after intent, after rename, after directory sync and after durable activation.
Prove old queued work stays isolated and a later manual transfer cannot resolve a pending cloud decision.
Include manual Import from prior-clear and prior-pending decisions, explicitly unbound/changed accounts and no
old file, with the same identical-time/bytes and journal fault cases. A successful collection/media transfer
cannot clear the imported generation's pending fence.

## R7. What a replacement from a collection package stamps (Anki 26.9.3)

- Anki's restore of a backup and its `.colpkg` import are one operation. On its copy of the incoming collection, after the copy has opened (upgraded when older) and before the swap, it sets `col.scm` and `col.mod` to the current time in epoch milliseconds. Nothing else in the file changes: no `usn`, no `ls`, no row of any other table.
- The stamps retain manual Anki-transfer compatibility. They do not identify a publication generation or resolve R6 recovery.
- Meguri's primitive makes the same change for `Restore` and `ColpkgImport` (R3 step 3), with one instant for both columns. Under a scenario's frozen clock both equal the scenario's current instant in milliseconds.
- Scenarios: `package/collection-package/restore-backup`, `upgrade-on-import` and the import half of `export-import-roundtrip` observe the stamped `col` row in `db_dump` (`specs/conformance/package-ops.md` E2).

## R8. Failures and kills

- **Before the rename** (R3, R4 and R5 steps 1 to 3): the old collection file, its generation and every handle stay as they were; the staging directory is removed (or swept at the next launch after a kill); publication recovery follows R6.2 and R6.3. R3 and R4 run before L8's `Replacing` step, so a refused file never closes the live collection.
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
| After a `Restore`: `CollectionReplaced(generation + 1)` once, older readers closed and their handles failing with `StaleGeneration`, the derived store rebuilding, the candidate publication identity active and `cloudPublicationPending` true | `core/crates/meguri-engine/tests/replace.rs` |
| After a `Restore` and after a `FullDownload`: no external change reported at the reopen (no `ChangedExternally`, no whole-collection change set besides `CollectionReplaced`), the sidecar's last-seen `mod` and `scm` are the new file's, and U32's sidecar members and `deviceState` are both still present | same file |
| Kill at `replace.after_pause_recorded` (and every fault point before it): the relaunch opens the old file unchanged (SHA-256), the prior publication decision is restored only after the old file identity is proven; a cancel before the rename does the same at once | same file |
| An already-pending publication decision survives a later replacement killed before its rename | same file |
| Kill at `replace.after_rename` and `replace.after_dir_sync`: the relaunch opens the new file, `integrity_check` `ok`, publication pending | same file |
| `FullDownload` with a truncated staged file: `StagingCorrupt` (or the gate's `Corrupt`), the old collection unchanged; U109 reuses this fixture | same file |
| `FullDownload` with a schema-11 staged file: upgraded on the staged copy; a kill right after the rename reopens a schema-18 collection | same file |
| A non-empty live `-wal` injected before the swap: `LiveWalNotEmpty`; the old collection opens and passes `integrity_check` | same file |
| A staged copy whose change is still in its `-wal`: R4 folds it in, and the swapped-in file holds the change | same file |
| Stored sanity counts survive a relaunch; U109's success path clears them (U109's test) | same file |
| The caller is not in the exclusive phase: `NotExclusive`, nothing touched | same file |

## R11. Local collection-file media recovery (Meguri design)

For `package/container` P16, extend this replacement's R6 intent with the owned media actions and recovery files. Publish the validated media and own journal after R5's durable intent and before its database rename. Keep the existing advisory lock, initial/Ready admission, forced backup, executor and generation ownership. This adds no second collection writer or independent replacement lifecycle.

Record old and candidate native file identities from locked files alongside R6's finalized byte identities. When their bytes are identical but media differs, use the native identities or durable activation witness to prove which file became active. Ordinary identical-database imports must remain supported. If the evidence cannot distinguish them, retain it and fail closed rather than choosing a media view from timestamps or hashes alone.

Recover the old or candidate media/journal view under P16 before any collection writer, mutating day check, reader, Ready state or replacement event. An unresolved media state blocks Ready even where R6 alone permits validated local review. P9's unconditional sweep must not remove these owned recovery files. A post-rename failure reports the candidate active or its activation uncertain; it never implies rollback. Keep the backup and recovery facts across delivery failure and relaunch.
