# Verified collection replace

Spec: engine/collection-replace
Requirements: R2004 (replace primitive), R2005, R1489, R1548 (publication decision; U113/U114 resolve it), R2051, R2056, R2066, R1638 (data side); supports R1543, R1626, R2003, R2010, R2019, and R1478, R1479, R1482, R2013, R2016 through the open gate
Anki: 26.09.3 for R7 (what a replacement from a collection package stamps); every other section is Meguri design
Scenarios: conformance/scenarios/package/collection-package/ (R7); tests in `core/crates/meguri-engine/tests/replace.rs`
Status: Under review (R2066) for the `CloudAdopt` row of R1, the `CloudAdopt` bullet of R2, the CloudKit wording of R2 and R5, R6.6 and R6.7, with `sync/policy`, `sync/cloudkit-records` and `sync/cloudkit-recovery`. Do not implement them until approved. Every other section is in force and unchanged.

The only way Meguri swaps the collection file for another file: a full download or upload (U109; manual Anki transfers, frozen), a restore (U127), a `.colpkg` import (U124), and the adoption of an iCloud collection (`CloudAdopt`, U113 and U114: Replace local collection from iCloud and the first connection to an iCloud collection, `sync/cloudkit-recovery#V13`, `#V14`). The primitive checks the incoming file with the open gate and the integrity check, applies the change its reason calls for, makes the file self-contained, and swaps it in during the exclusive phase as the swap step of `engine/lifecycle#L8`. It also keeps the device-only replacement and publication recovery state (R6). Reading and inspecting packages is `package/container`.

## R1. Reasons

| Reason | Caller | The staged file comes from | Change before the swap (R3 step 3) | Publication recovery (R6) |
|---|---|---|---|---|
| `Restore` | U127 | inspecting a backup (`package/container#P12`) | the primitive's package stamps: `scm` and `mod` set to the replace instant (R7) | set |
| `ColpkgImport` | U166 minimal caller; U124 full flow | inspecting a `.colpkg` | the package stamps, then any change U124 passes | set |
| `FullDownload` | U109 | the server's file, streamed into staging | U109's change: `ls` = `mod` (R1543) | manual Import: fresh publication identity and pending fence under R6, even if the old decision was clear |
| `FullUpload` | U109 | the file U109 transformed and uploaded | none (U109 changed it before the upload) | preserve the cloud decision; manual policy remains separate |
| `CloudAdopt` | U113, U114 (`sync/cloudkit-recovery#V14.2` step 8) | the staging fetcher's staged collection, built from an epoch (`sync/cloudkit-records#K18.4`) | none: no package stamps; the staged file already holds K18.4's `col` values | R6.6: the caller's fresh publication identity, bound to the new binding (active source) or local only (previous source); intent recorded; no fence |

`FullDownload` and `FullUpload` are manual Anki transfers (U109, frozen; KTD26). `CloudAdopt` is added for CloudKit; the other rows do not change.

The package stamps are Anki's behavior (R7), so the primitive applies them itself for both package reasons; U127 passes no change, and U124 passes only what its own spec adds. `ColpkgImport` exists in the type from this unit on; U166 supplies its minimal collection-only caller; U124 supplies the full flow.

## R2. Input

- **The staged file**: `collection.anki2` in a staging directory that U32's staging-directory primitive made in the collection directory (`package/container#P9`), handed over as an inspect handle or by the caller that wrote it there. It must be on the collection's file system (same device), else `CrossDevice`. No connection other than the primitive's own may be open on it.
- **The reason**, and the caller's change if any: a function the primitive runs inside the R3 step 3 transaction on the staged file.
- **For `CloudAdopt` only**, three values the caller passes and the primitive does not generate: the operation identity (the `adopt` recovery op id, `sync/cloudkit-recovery#V2.3`), the candidate publication identity (that op row's candidate identity column), and the candidate decision (bound to the new binding's account and environment, or local only, `sync/cloudkit-recovery#V14.1`). Without all three a `CloudAdopt` fails with `InvalidArgument` before anything is touched. For every other reason the primitive generates the identities as R6.1 says.
- **The exclusive phase.** The caller holds the long-operation slot and runs the phase's `Working` step with its privileged handle (`engine/lifecycle#L5`, `#L7`); otherwise `NotExclusive`, and nothing is touched. With no collection open (a first download, a first connection to an iCloud collection, a recovery from `Failed`), holding the slot is enough (L5).
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

Runs as the swap of `engine/lifecycle#L8` (step 2), after L8 step 1 has set the phase to `Replacing` and closed every connection to the live collection: every reader (U107's snapshot reader and its stage attachment, the CloudKit cloud reader (`engine/lifecycle#L11`) and the derived-store attachments included, after their read transactions end), then the writer with its checkpoint (`storage/open-and-connections#C10`).

1. The live `collection.anki2-wal` must be absent or empty (0 bytes). If not, nothing is changed and the replace fails with `LiveWalNotEmpty`: L8 reopens the old file and keeps the generation.
2. For `Restore` and `ColpkgImport`, durably record the replacement intent and publication fence (R6.2); R6.5 continues its already-recorded intent for the same operation. For `CloudAdopt`, durably record the replacement intent with the fence false (R6.6).
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

The following JSON lives only inside `deviceState`; these are Meguri-owned
maintenance records, not collection or sync fields:

| Key | Stored value |
|---|---|
| `lastBackup` | object with `snapshotMod`, `instant` (signed 64-bit epoch milliseconds) and `name` (the published backup filename string); keep the existing spelling and meaning |
| `lastCheckDatabase` | object with `instant` (signed 64-bit epoch milliseconds) and `result` as defined below |
| `lastOptimize` | signed 64-bit epoch milliseconds; keep the existing scalar representation |
| `lastQuickCheck` | object with `instant` (signed 64-bit epoch milliseconds) and `passed` (JSON boolean) |

`lastCheckDatabase.result` reuses the machine summary in
`conformance/collection-check#K5`: either every documented problem kind with an
unsigned 64-bit count and no `error` member, or `error: "corrupt"` with no problem
count members. It stores no localized messages or raw error text. Unknown
members remain allowed and preserved. This defines only these stored records and their accessors in U32's sidecar (`storage/durability-and-files`);
U128/U132 still own the operations and their scheduling and user presentation.

A genuinely absent sidecar, `deviceState` or optional record reads as no record.
A present null, wrong type, missing required member, invalid result variant or
number outside its declared integer range is an error, never absence or a
successful default. An unreadable document or invalid existing R6 state stays
an error. Reads do not repair it, and a setter must not replace corrupt data
with a new record. Maintenance accessors cannot initialize or reset R6 identity.

Use the existing sidecar lock and durable read/modify/write seam, with no nested
accessor calls. Merge only the supplied known fields. Preserve unknown members
at document, `deviceState`, record and result levels, plus every unrelated
record and the existing publication/intent state. A result-variant change may
remove only the old variant's known fields. Propagate read, validation and
publication failures rather than reporting a successful save.

Focused accessor tests must distinguish absent from malformed/null/incomplete
records, reject bad counts/times/booleans, roundtrip both result variants and
preserve unknown members and R6 state through updates. Cover independent
metadata writes sharing the lock and a failed durable publication. This
preparation adds no check operation, cadence, UI or server work.

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

### R6.5 Damaged-file Restore with retained WAL bytes

For T8 Restore from Failed or NoCollection with an existing main file, T7 may
leave a nonempty old WAL that cannot be checkpointed. A durable raw backup alone
does not waive `LiveWalNotEmpty`. Before changing that WAL, bind the verified
T7 main/WAL copies, their durable identities and the original WAL's native
identity, length, digest and disposition to this operation's existing R6 intent.
Use exact no-follow root/name bindings and independent regular files; refuse
aliases, hard links, changed or missing evidence. Recheck source/copy bindings
under the same advisory/exclusive ownership. A name or callback alone is no proof.

Save the WAL recovery state before its first move. Put this state in a required
field whose value existing main-only R6 readers reject. New optional WAL, version
or feature fields give no protection when those readers ignore them. Old readers
must stop before treating the intent as valid, opening the collection, rewriting
or clearing the intent. Keep this value until all WAL handling, recovery and
directory sync work is durably complete, including after main rename or
activation. Matching main-file bytes alone never ends this protection. Existing
intents keep their readable format.

Retire only the identified WAL into operation-owned recovery storage, without
overwrite or truncation, and synchronize the affected directories. The intent
must preserve recoverable bytes across the filesystem-action/metadata-update
crash window; retired bytes are not swept before R6 resolves them. Continue the
same intent and require R5's ordinary absent-or-empty WAL check before candidate
rename. Never leave old WAL beside a candidate when opening it.

First prove which main file is present using recorded bytes, native bindings and
valid phase evidence. Equal bytes or times do not prove the old file; keep an
unresolved equal result blocked. Then compare both WAL paths with the state saved
before the move. Recovery must safely handle a WAL still at its old path, a
completed move not yet saved in metadata, and an old-WAL move back that stopped
part way through. If both paths exist, neither exists, either binding changed or
their states conflict, preserve evidence and refuse any removal or replacement
that is not proven safe. Running recovery again must preserve a correct result.

Before candidate main rename, cancellation/failure restores the prior WAL view;
proven old-file recovery does so before any old open or checkpoint. A proven
candidate never receives old WAL, even with equal main bytes. Keep the T7 copies
and existing publication decisions. Complete WAL reconciliation before opening,
mutating checks, legacy-format conversion or intent cleanup. This is the same
R6 operation, not another journal or a waiver of the ordinary WAL guard.

Test process kills between each file action and its metadata update: save intent,
move old WAL, move it back, rename main and finish. Include equal main-file proof
that cannot select a branch, changed source/copy/root bindings, missing or corrupt
retained evidence, and refusal by old readers before activation and after rename
while WAL work is pending. Check exact old main/WAL recovery or keep old WAL away
from the candidate. Keep the existing nonempty-WAL and publication-fence tests.

### R6.6 `CloudAdopt` (U113, U114; `sync/cloudkit-recovery#V14`)

Added for CloudKit. R6.1 to R6.4 do not change for the other reasons.

- **Intent.** `CloudAdopt` uses R6.2's handshake with the three values R2 lists. The operation identity is the `adopt` recovery op id (`sync/cloudkit-recovery#V2.3`). The intent also records the candidate's decision: bound to the new binding's account and environment when the source is the active epoch, or local only when it is the previous epoch (`sync/cloudkit-recovery#V14.1`).
- **Candidate identity.** The publication identity in the candidate identity column of the `adopt` op row (`sync/cloudkit-recovery#V2.3`); for the active source V14.2 step 6 also writes it into the new binding. It is fresh: no earlier collection had it.
- **No fence.** The cloud side is the source, so the candidate is never fenced: the intent records `cloudPublicationPending` false. The adoption is itself the user's explicit cloud choice (R6.1), so it also resolves a fence that was pending before the op (`sync/cloudkit-recovery#V12.2`, `#V14.2` step 8). Of the replacement reasons, it is the only one that clears a fence. U113's other choices clear it without a replacement (`sync/cloudkit-recovery#V12.2`, `#V13`, `#V15.2` step 2).
- **After activation.** R6.3's first paragraph applies, with the candidate marked active with its recorded decision and the fence clear, not pending.
- **At startup**, for an unfinished `CloudAdopt` intent:

  | Proven state | Durable result |
  |---|---|
  | Intent phase; only the old file identity matches, also a recorded absence | restore the old identity and its exact prior decision, a pending fence included, then remove the intent |
  | Intent phase; only the candidate file identity matches | activate the candidate identity with its recorded decision and the fence clear (a bound decision only after the account check below), then complete the intent |
  | Durable activation exists | keep the candidate and its decision |
  | Both match, neither matches, or the metadata is unreadable | block publication and keep the journal, as R6.3. Only U113 resolves the block: `sync/cloudkit-recovery#V14.3`'s Cancel, then a V13 choice for the collection that opens |

- The account check of R6.3 applies to a bound decision: when the captured account binding no longer matches, the candidate is activated with `cloudPublicationPending` true and no recorded decision, so publication stays paused until U113's decision (`sync/cloudkit-recovery#V12.2`). It never becomes a decision for another account. A local-only decision has no account to check.
- R7's package stamps do not apply. The tracking side of the adoption (`sync/cloudkit-recovery#V14.2` step 9) runs after the reopen; this spec does not touch the tracking store.
- Tests, in `core/crates/meguri-engine/tests/replace.rs`: a `CloudAdopt` from a prior-clear and from a prior-pending decision ends with the fence clear and the recorded decision; a kill at each R8 fault point gives the table above; an account mismatch at startup activates the candidate with the fence pending and no `pendingDecisionReason`; `pendingDecisionReason` (R6.7) round-trips, reads absent as no reason, and is an error when null or an unknown string; `CloudAdopt` with no old file; `ClearedByOneWay` and `CollectionReplaced` as for any replacement.

### R6.7 The reason of a pending decision (`sync/cloudkit-recovery#V12.1`)

Added for CloudKit. When `Restore`, `ColpkgImport` or `FullDownload` (manual Import) sets the fence, `deviceState` also keeps the replacement's reason with the pending decision, until U113 resolves it, in the member `pendingDecisionReason`: the JSON string `restore`, `colpkgImport` or `manualImport`. U113 reads it for `MGEpoch.reason` (`sync/cloudkit-recovery#V12.2`). An absent member means no reason: a pending decision recorded before this member existed, or one that R6.6's account check left. A present null, another type or another string is an error, as R6.1 says. The member is cleared with the decision, and R6.3's restore of a prior decision restores it with that decision.

## R7. What a replacement from a collection package stamps (Anki 26.9.3)

- Anki's restore of a backup and its `.colpkg` import are one operation. On its copy of the incoming collection, after the copy has opened (upgraded when older) and before the swap, it sets `col.scm` and `col.mod` to the current time in epoch milliseconds. Nothing else in the file changes: no `usn`, no `ls`, no row of any other table.
- The stamps retain manual Anki-transfer compatibility. They do not identify a publication generation or resolve R6 recovery.
- Meguri's primitive makes the same change for `Restore` and `ColpkgImport` (R3 step 3), with one instant for both columns. Under a scenario's frozen clock both equal the scenario's current instant in milliseconds.
- Scenarios: `conformance/scenarios/package/collection-package/restore-backup.yaml`, `upgrade-on-import.yaml` and the import half of `export-import-roundtrip.yaml` observe the stamped `col` row in `db_dump` (`specs/conformance/package-ops.md` E2).

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
| `LiveWalNotEmpty` | R5 step 1 still refuses after a raw T7 copy unless R6.5 has safely retired the identified WAL |
| `Io` | any other file-system failure |

The lifecycle's own errors (`CollectionUnavailable`, `StaleGeneration`, `engine/lifecycle#L5`, `#L7`) pass through unchanged.

## R10. Tests

| What | Where |
|---|---|
| `Restore` of the backup fixture: the dump equals the oracle's restore of it | `conformance/scenarios/package/collection-package/restore-backup.yaml` (Meguri's `import` adapter runs inspect, then this primitive with `Restore`) |
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

Recover the old or candidate view of the media folder and its journal (`media/media-files`, `media/media-journal`) under `package/container#P16.3` before any collection writer, mutating day check, reader, Ready state or replacement event. An unresolved media state blocks Ready even where R6 alone permits validated local review. P9's unconditional sweep must not remove these owned recovery files. A post-rename failure reports the candidate active or its activation uncertain; it never implies rollback. Keep the backup and recovery facts across delivery failure and relaunch.
