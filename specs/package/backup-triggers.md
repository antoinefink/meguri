# Backup triggers, safety backups and restore

Spec: package/backup-triggers
Requirements: R1630, R1631, R1632, R1637 (core), R1638, R2008, R1475 (backup side), R2010, R2020 (backup side), R1662 (backup side), R1539 (provider to U109), R2056 (local side of the iCloud replacements); budgets R1848, R1854
Anki: 26.09.3 for T2.1, T2.2 and T4.1 (when Anki makes a backup and which operations force one); every other section is Meguri design from the Product Contract and the plan (U127)
Scenarios: conformance/scenarios/package/backup-triggers/
Status: Approved 2026-10-27 (R2066) for the two iCloud rows of T6.1 and the T6.1 bullet that explains them, with `sync/policy`, `sync/cloudkit-records` and `sync/cloudkit-recovery`. Every other section is in force and unchanged.

When Meguri backs up: periodic backups (T2, T3), "Back Up Now" (T5), safety backups before destructive operations (T6), the byte copy of a damaged collection (T7), and restore (T8). Names, the writer, retention, the limits and the listing are `package/backups`; the package format is `package/container`; the swap is `engine/collection-replace`. The scenario vocabulary is `specs/conformance/backup-trigger-ops.md` on top of `specs/conformance/backup-ops.md`.

## T1. Kinds of backup

| Kind | Started by | T2.2 interval | T2.1 change | Outcome |
|---|---|---|---|---|
| Periodic | the scheduler (T3): a deadline, `Opened`, `Background`, `Quit` | applies | applies | a backup, or nothing |
| Back Up Now | the user (R1632) | ignored | ignored | always a backup, or an error |
| Safety | an operation of T6.1 | ignored | ignored | always a backup, or the operation is refused |
| Damaged-collection copy | a safety backup that cannot snapshot the collection (T7) | ignored | ignored | a byte copy, or the operation is refused |

Every kind but the damaged-collection copy is a package written by `package/backups#B4` (its name, its retention). Server copies (U109, `package/backups#B2.2`) are not backups of the live collection and are none of these kinds.

## T2. When a periodic backup is made

A backup that is not forced is made only when the collection changed since the last backup (T2.1) and no entry of the backups folder is younger than the minimum interval (T2.2). Anki applies T2.1 to its forced backups too; Meguri does not (T4.2).

### T2.1 Changed since the last backup (Anki 26.9.3)

- The reference is the value of `col.mod` (epoch milliseconds) that the last backup was taken from.
- Any difference between current `col.mod` and the saved reference means changed. A missing reference also means changed.
- Only `col.mod` decides. Commits that leave `col.mod` at the reference leave the collection unchanged for this rule: an op stamped in the same millisecond as the reference (possible under a frozen clock), or an undo that puts `col.mod` back to the reference (`engine/undo` A7). A raw write that moves `col.mod` to an earlier value is a change.
- Anki keeps the reference in memory for one opening of the collection, takes it at the start of each backup (a backup whose write then fails still counts), and takes it for forced backups too. So every opening starts with no reference, and Anki skips a forced backup when nothing changed since the previous backup of the same opening ("Create Backup" then reports that the collection is unchanged).

### T2.2 The minimum interval (Anki 26.9.3)

- The interval is `minimum_interval_mins` of the synced `backups` config key, as `package/backups#B6.1` reads it (default 30).
- A periodic backup is not made when the backups folder holds an entry whose creation time `c` satisfies `c <= now` and `(now - c)`, in whole seconds rounded down, `< interval × 60`.
- Every directory entry counts, whatever its name or type: backups and server copies, any file the user put there (`notes.txt`), a file whose name dates it months ago (its creation time counts, never the date in its name), unpublished work (`.meguri-tmp-…`), a directory, a symbolic link (the link itself, not its target).
- Use OS creation timestamps. Ignore timestamp read errors.
- An interval of 0 never blocks. A folder that does not exist has no entries (Meguri's choice: Anki fails a backup that is not forced when it cannot list the folder, and its desktop always creates the folder first).
- Anki computes `interval × 60` in 32-bit unsigned arithmetic that wraps: from 71,582,789 minutes on, its limit is the product modulo 2^32 seconds, and 1,073,741,824 minutes gives 0 (nothing blocks). Meguri computes the product exactly. The settings offer at most 9999 minutes (`package/backups#B6.2`); no scenario uses such values.

### T2.3 Meguri's rule

A periodic backup is due when T2.1, with Meguri's reference, finds a change and T2.2 finds no blocking entry.

- **The reference** is the last-backup record of the device sidecar (`engine/collection-replace#R6.1`): the `col.mod` held by the snapshot of the last published backup of the live collection, of any kind (periodic, Back Up Now, safety, and the writer of `package/backups#B4` whoever calls it), with that backup's instant and file name. It survives a close, a reopen and a relaunch. Deviation `backups-change-check-survives-reopen` (scenario `package/backup-triggers/reopen`): after a reopen with no change, Anki makes a periodic backup and Meguri does not, so a relaunch on iOS never adds a backup of an unchanged collection.
- The record is written once the backup is published (`package/backups#B4` step 4), with the `col.mod` read inside the snapshot's own read transaction, never the live value when the backup ends: a commit made during the backup stays a change. A backup that fails or is cancelled writes nothing (Anki counts it, T2.1).
- When two backups of the live collection run at once, a backup does not replace a record whose snapshot was taken after its own.
- A damaged-collection copy (T7), a server copy (U109) and a backup of a standalone file never write the record.
- A record that is missing or cannot be read means no reference: changed.

## T3. The periodic scheduler (Meguri design; R1630 with R1854)

There is no polling. The scheduler in the engine computes a single deadline and publishes it; U154's `BackupTrigger` keeps one timer for it and calls `run_due_maintenance(Deadline)` when it fires (KTD5). With nothing to back up, nothing is armed and nothing wakes.

### T3.1 Inputs

- **Pending change**: the collection is open and Ready and its `col.mod` differs from the reference (T2.3).
- Commits and `CollectionReplaced` from the change stream, lifecycle changes, the end of every backup of the live collection (any kind), and the triggers of `run_due_maintenance`: `Opened`, `Deadline`, `Background`, `Quit` (`BackgroundProcessing` does nothing for backups; it is U132's).
- A **periodic attempt** is a periodic backup the scheduler started because the rule was due, whatever its outcome (published or failed). One cancelled by `suspend` is not an attempt.

### T3.2 The deadline

When armed, the deadline is the latest of these instants, or now when none exists or the latest is already past:

- `c_max + interval`, where `c_max` is the newest creation time among the entries T2.2 counts;
- the start of this engine's last periodic attempt + 5 minutes.

Notes on the deadline:

- The 5 minutes keep R1630's cadence ("checked every 5 minutes"): periodic backups are at least 5 minutes apart even with an interval under 5 minutes, and a failing backup is retried at most every 5 minutes. They apply to the scheduler only, never to `Background`, `Quit` or a forced backup.
- Computing the deadline reads the folder once (names and creation times, no contents). Nothing scans the folder again until the next re-evaluation of T3.3.
- Examples (interval 30): the last backup at t − 40 min and a change committed at t: deadline t, the backup runs at t. The last backup at t − 10 min and 100 changes between t and t + 19 min: armed once at the first change for t + 20 min, exactly one backup, at t + 20 min.

### T3.3 Arming, moving and clearing

- **Armed** when the state is Ready, a change is pending and no deadline is armed: at the first commit after which `col.mod` differs from the reference, at `Opened` (a change left from an earlier run), after `CollectionReplaced` (a replaced collection's `col.mod` differs from the reference, `engine/collection-replace#R7`), and at the end of an exclusive phase.
- Later commits never move an armed deadline.
- **Re-evaluated** (the deadline computed again, armed, moved or cleared) when any backup of the live collection ends, when the deadline fires, and when the exclusive phase ends.
- **Cleared** when no change is pending any more (a backup of any kind recorded the current `col.mod`, or an undo put `col.mod` back to the reference), and when the collection closes, fails or enters the exclusive phase.
- Each change of the armed deadline (armed, moved, cleared) publishes one `MaintenanceDeadline{at}` event, with `at` absent when cleared. A re-evaluation that leaves it where it was publishes nothing. `run_due_maintenance` returns the deadline as it stands when the call ends.

### T3.4 Triggers

| Trigger | What the scheduler does |
|---|---|
| `Opened` | after the lifecycle is Ready: arm when a change is pending; when the deadline is now, continue as `Deadline` |
| `Deadline` | when Ready, a change is pending, the 5-minute floor is met and T2.3 is due: run a periodic backup (`package/backups#B4`) to its end, then re-evaluate. Otherwise re-evaluate: a newer entry or the floor moves the deadline, no change clears it. A timer that fires before the armed instant only re-evaluates |
| `Background` | T2.3 alone, without the floor: when due, the backup runs and ends before the call returns (U151 sends it within its background-task window, KTD15); then re-evaluate |
| `Quit` | as `Background`, within R1848 (at most 2 s of work at 150k cards) before the quit is acknowledged. U151 sends it after its quit-time sync, the order Anki uses (closing sync, then its close backup) |
| `BackgroundProcessing` | nothing for backups |

- `run_due_maintenance` returns once the backup it started has ended (published, failed or cancelled).
- **States.** Periodic backups run only in Ready: never in ExclusiveBusy (the restore, import or one-way sync takes its own safety backup), Failed (Anki makes no close backup of a collection that fails its quick check), NoCollection, Opening or Upgrading. A periodic backup that is running when an exclusive phase starts ends before the swap: it holds the maintenance reader, which `engine/lifecycle#L8` waits for.
- **Failure.** Logged with the error's name, never shown as a modal (R1848). It counts as an attempt; the change stays pending and the deadline is re-armed by T3.2. U132's status keeps showing the last backup that succeeded.
- **Suspend.** The scheduler is a `SuspendParticipant` (U32): `flush` does nothing; `suspend` cancels a running periodic backup (`package/backups#B4`: nothing published, the change stays pending) and keeps the armed deadline; `resume` does nothing (U154's timer fires late when the deadline passed meanwhile).
- **Mac (R1662).** Periodic backups happen only while Meguri runs: nothing is scheduled outside the process, and `Quit` is the last trigger.
- Anki, for comparison: its desktop checks every 5 minutes while it runs and once when the profile closes; its next check can come up to 5 minutes after the instant T3.2 computes. Meguri backs up at the deadline itself. No scenario shows this (the oracle runs no timer).

## T4. Forced backups

### T4.1 What Anki forces (Anki 26.9.3)

Anki's desktop makes a forced backup (interval ignored, T2.1 still applied, the operation waiting for it) before a one-way download (full download), before a `.colpkg` import (after the user confirms the replacement) and before Check Database, and for "Create Backup" (menu). It makes none before a one-way upload, before restoring a backup, or before deleting decks, note types, empty cards or unused media. Restoring a backup from the main window first closes the collection as any close does (the closing sync when automatic sync is on, then the close backup, which is not forced, so T2.1 and T2.2 decide), then imports the chosen backup with no forced backup. Until the restored profile is closed again, Anki syncs nothing automatically and makes no close backup; its 5-minute periodic checks go on.

### T4.2 Meguri

- Back Up Now, safety backups and damaged-collection copies ignore T2.1 and T2.2: they always write. Deviation `backups-forced-always-written` (scenario `package/backup-triggers/forced-unchanged`): Anki makes no forced backup when nothing changed since the last backup of the same opening; Meguri writes one, because R1632 reports the file it made and a safety backup must exist whatever an earlier backup holds.
- Meguri forces backups before more operations than Anki (R1631, R2008, a requirement deviation): T6.1.

## T5. Back Up Now (R1632)

- A forced backup of the live collection, written by `package/backups#B4` as a background job: it never takes the writer, and a review session may be open.
- Result: the new file's name, its size in bytes, the instant it is named after, and the names retention deleted (B4 step 7). It writes the record (T2.3); the scheduler re-evaluates (T3.3).
- Allowed in Ready only. In ExclusiveBusy it is refused with `CollectionUnavailable` (the exclusive job takes its own backup), in `Failed{Corrupt}` with `CollectionCorrupt`, in any other state with `CollectionUnavailable`.
- Errors: those of B4 (`InsufficientSpace`, `DiskFull`, `Io`, `Cancelled`, `DestinationExists`).

## T6. Safety backups (R1631, R2008, R2020, R1539)

### T6.1 Which operations

| Operation | Unit | Taken | Anki |
|---|---|---|---|
| one-way download from an Anki server, the recovery download from `Failed{Corrupt}` included (a manual Anki transfer, frozen with the Anki server work; KTD26) | U109 | always | yes (download) |
| one-way upload to an Anki server (a manual Anki transfer, frozen; KTD26) | U109 | always | no |
| restore | T8 | always | no |
| `.colpkg` import | U124 | always | yes |
| Check Database | U128 | always | yes |
| Empty Cards deletion | U131 | when its list of cards is not empty | no |
| Delete Unused media | U130 | when it deletes at least one file | no |
| deleting decks | U28 | when the deletion removes at least one card (U28 decides which cards a deletion removes; a filtered deck's deletion removes none) | no |
| deleting a note type | U34 | always | no |
| a structural edit (`notetypes/schema-changes#G2`) other than deleting a note type (row above): reasons `fields_changed`, `sort_field_changed`, `templates_changed`, `notetype_restored`, `notes_notetype_changed`, `preset_removed` or `notetypes_merged` (R1475, R2008) | U34, U35, U28, U49, U121 (classification U29) | whenever the op's plan classifies it so (`notetypes/schema-changes#G3`), whatever `scm` and `ls` hold; once per logical operation, before its first collection write; for an `.apkg` import that merges note types (U121), before the import's first collection write | no |
| any op whose `engine/op-model#O9` undo class is "destructive, not undoable" | its unit | always (R2020) | — |
| Replace local collection from iCloud (`CloudAdopt`: the adoption of `sync/cloudkit-recovery#V14`, also at a first connection, V13.3, and from the damaged-collection screen, V19) | U113, U114 | always when a collection file exists: B4's backup, or T7's byte copy when the collection is damaged; none without a collection file (T6.2) | — |
| Replace iCloud collection (`sync/cloudkit-recovery#V15`) | U113, U114 | never: the local collection does not change. The replaced iCloud collection stays recoverable as the previous epoch for 30 days (`sync/cloudkit-records#K18.5`, `sync/cloudkit-recovery#V14.1`) | — |

- A conditional row is decided when the op is submitted, from the state its plan reads.
- A structural edit has no confirmation step (`notetypes/schema-changes#G4`, R1476). When its unit shows a destructive-mapping preview, the user accepts that preview before the op is submitted: fields, templates or cards that the edit removes (U34, U35), Restore to Default's question (R914), the note type delete question (R918), the decks a preset removal reassigns (R733), or the merge preview of an `.apkg` import (U121). The backup follows the submit and precedes the op's first collection write (T6.2). Changed 2026-10-27: this row replaces one that took the backup only for the first schema change since the last sync, after the Anki confirmation that R1476 no longer has.
- Coalescing (decided 2026-10-27): a structural edit whose `engine/op-model#O9` undo class is undoable takes no new safety backup when, in this opening, a safety backup ended less than 10 minutes before the op is submitted and every commit since that backup can still be undone. In every other case it takes one. Ops of the "destructive, not undoable" class always take their own (row above). This keeps one recoverable copy for a burst of note type edits without a full backup, and its wait, per edit.
- An incoming CloudKit change that sets `scm` (`notetypes/schema-changes#G2` `cloudkit_structure_applied`) takes no safety backup on the receiving device; the device that made the edit took one (decided 2026-10-27).
- A collection backup holds no media: the backup before Delete Unused media protects the collection only; the files themselves go to the trash (R2018).
- The two iCloud rows (added 2026-10-27, R2056) keep a recoverable copy of each side being replaced. `CloudAdopt` takes its backup in the `Working` step of its exclusive phase, after one `package/container#P10` preflight for it (`sync/cloudkit-recovery#V14.2` step 7), the way T6.2 runs the backup of restore and `.colpkg` import in their phases. It is protected from retention until the adoption ends, and it holds no media, which the preview says (V14.2 step 1). A failure refuses the adoption with `SafetyBackupFailed` before the swap, and the local collection is unchanged (V14.3).

### T6.2 How it runs

- The op's descriptor carries its safety class. Before the op's write phase is queued, the executor (U24) asks the engine's `BackupService` port for a forced backup and waits for it. `meguri-package` implements the port and `meguri-host` registers it when it opens a collection (U38), so the app, the local servers and headless runs all take the same backups.
- An op of an exclusive phase (one-way sync, a manual Anki transfer, frozen, KTD26; restore; `.colpkg` import; `CloudAdopt`, `sync/cloudkit-recovery#V14.2` step 7) takes it during the phase's `Working` step, after the drain (`engine/lifecycle#L7` step 3). `CloudAdopt` was added for CloudKit (R2066 review with the T6.1 iCloud rows); the others do not change. A plan-and-commit op (`engine/op-model#O6`) takes it after its planning and before its write phase; a replan after `Stale` takes no second one.
- Other ops keep committing while it runs (answers never wait for it); it holds every commit made before its snapshot, and the guarded op commits only after it is published. In the op log, the backup's instant precedes the op's commit.
- The backup's name is protected from retention (`package/backups#B5.1`) from its publication until the op ends (commit, failure or cancel).
- It writes the record (T2.3, except a T7 copy); the scheduler re-evaluates (T3.3).
- **Failure**: the op is refused with `SafetyBackupFailed{cause}` (cause: the B4 or T7 error), none of its writes runs and the collection is unchanged. It never proceeds without the backup.
- **Cancel**: cancelling the op while its backup runs cancels the backup (B4) and the op ends `Cancelled`. A backup already published stays.
- **Nothing to back up**: with no collection file at the collection path (a first download into an empty directory, or a `CloudAdopt` at a first connection with no local collection, `sync/cloudkit-recovery#V13.3`), the op proceeds without a safety backup.

## T7. Safety backup of a damaged collection

### T7.1 When

A safety backup becomes a byte copy when the lifecycle is `Failed` (`Corrupt`, or an open that failed) or `NoCollection` with a collection file present, or when the snapshot of `package/backups#B4` fails with SQLite's corruption errors (`SQLITE_CORRUPT`, `SQLITE_NOTADB`). It runs no `quick_check` of its own: a collection whose quick check failed is already `Failed{Corrupt}` (U132).

### T7.2 The copy

- **Files**: `collection.anki2`, and `collection.anki2-wal` when it exists and is not empty. Never `-shm`.
- **When it is taken**: while nothing writes the file and no checkpoint runs: in `Failed`, where the engine refuses writes; in `NoCollection`; in the `Working` step of an exclusive phase, after the drain; and in Ready (a snapshot that failed) as a writer maintenance task with a declared delay (`engine/op-model#O8`), so no op commits during the copy.
- **Names**: `corrupt-YYYY-MM-DD-HH.MM.SS.anki2` and, for the WAL, the same name followed by `-wal`. The fields follow `package/backups#B2.1` and the local time of B3. When either name exists, the next second's name is taken, at most 60 seconds past the instant; then `DestinationExists`.
- **Publication**: each file through U32's atomic file (purpose `backup`) with exclusive create, the `-wal` copy first and the main file last, so a main copy never stands without its WAL. Before each rename, the copy's size and SHA-1 must equal the source's (both computed while copying, the copy read back for its own).
- **Preflight**: the two source sizes + 4 MiB (`package/container#P10`); too little space is `InsufficientSpace`.
- The copies are foreign entries for `package/backups`: retention never deletes them and the listing leaves them out, so Restore and U132's "most recent backup that passes an integrity check" never pick one. They are recovery artifacts the user reaches in Files or Finder. They count for T2.2 like any entry and write no record.
- Any failure (`InsufficientSpace`, `DiskFull`, `Io`, `DestinationExists`) removes the unpublished files, and a WAL copy already published when its main copy fails, and refuses the op with `SafetyBackupFailed`.

## T8. Restore (R1637, R1638, R2010)

1. **Choice**: a file of the backups folder whose name `package/backups#B2.3` recognises as a backup or a server copy (`server_copy`). Any other name, an unverified server copy included (it is not a package), is refused with `InvalidArgument` before anything is read.
2. **Slot**: take the long-operation slot (`engine/lifecycle#L5`); held: `Busy{running_op}`.
3. **Preflight**: one check (`package/container#P10`) for the inspect and the safety backup together: the inspect's need plus B4's need for the live collection (or T7's for a damaged one).
4. **Inspect** the file (`package/container#P12`). A refusal ends the restore with its error (`Corrupt`, `TooNew`, `SchemaUnsupported`, ...); the live collection, its `-wal`, the backups and the media keep their bytes (R1638). From here on the restore uses the staged copy, so retention deleting the chosen file later changes nothing.
5. **Session**: while a review session is open (KTD17), refused with `SessionActive`.
6. **Exclusive phase**: enter `ExclusiveBusy{op: Restore}` (`engine/lifecycle#L7`): U43's overlay is flushed and queued writes drain. The session check runs again as the phase is entered: a session that opened since step 5 refuses the restore with `SessionActive` and drops the phase.
7. **Safety backup** of the current collection (T6.2, or T7 when damaged), protected until the restore ends. With no collection file, none.
8. **Replace**: the verified replace with reason `Restore` (`engine/collection-replace#R2` to `#R5`): the open gate and integrity check again, `scm` and `mod` stamped (R7), the replacement intent and publication fence recorded (R6.2), the swap, `CollectionReplaced`.
9. **Result**: the restored name, safety backup name (or none), new generation and `cloudPublicationPending` true. R6 preserves the publication fence until U113/U114 resolve an explicit cloud/local choice. Manual Anki transfers remain separate.

- **Cancel or failure before the rename**: the lifecycle returns to the state it had (Ready, or `Failed`) with the same generation; publication recovery follows `engine/collection-replace#R6`; the safety backup stays.
- **States**: allowed from Ready, from `Failed{Corrupt}` (`engine/lifecycle#L4`) and from NoCollection. No existing collection file means no safety backup; an existing file requires T7. A retained nonempty old WAL uses `engine/collection-replace#R6.5`, without weakening `LiveWalNotEmpty`.
- After it, the restored collection's `col.mod` (the restore instant) differs from the reference, so the scheduler arms a deadline (T3.3) from the safety backup's creation time.
- Anki, for comparison: its restore is its `.colpkg` import (the same stamps, `engine/collection-replace#R7`), with no forced backup first and automatic sync off until the restored profile closes (T4.1).

## T9. Errors

| Error | Where |
|---|---|
| `SafetyBackupFailed{cause}` | T6.2, T7: the backup failed; the op did nothing |
| `SessionActive` | T8 steps 5 and 6 |
| `Busy{running_op}` | T8 step 2 |
| `InvalidArgument` | T8 step 1 |
| `CollectionUnavailable`, `CollectionCorrupt` | T5 |

The errors of `package/backups#B4`, `package/container#P13`, `engine/collection-replace#R9` and `engine/lifecycle` pass through unchanged. U133a maps them to its FFI error set.

## T10. Scenarios and tests

| What | Where |
|---|---|
| T2.1: no change since a backup of any kind; a change in the same millisecond; a change; `col.mod` moved back (interval 0) | `package/backup-triggers/change-check` |
| T2.1: an undo puts `col.mod` back, no backup | `package/backup-triggers/change-check-undo` |
| T2.2: a backup name dated months ago but created now blocks; a forced backup ignores it; a 1-minute-old backup blocks | `package/backup-triggers/recent-entry` |
| T2.2: a foreign file blocks | `package/backup-triggers/foreign-entry` |
| T4.2: a forced backup with no change (deviation `backups-forced-always-written`) | `package/backup-triggers/forced-unchanged` |
| T2.3: the reference survives a reopen (deviation `backups-change-check-survives-reopen`) | `package/backup-triggers/reopen` |
| T2 timing bounds and exact arithmetic; T3 deadline events, triggers, retry floor and exclusive-phase suppression | `core/crates/meguri-engine/tests/maintenance_scheduler.rs` (injected clock and entry times) |
| `change_check_survives_reopen`: reopen and relaunch keep the reference | same file |
| Safety classes: an op of each class takes its backup before its commit, conditional classes decided at submit (a deck with 3 cards: a backup; an empty deck: none), structural-edit coalescing (a second undoable structural edit 9 minutes after the first: no backup; at 10 minutes, or after a commit that left the undo history: a backup), `SafetyBackupFailed` with injected disk full (the op does nothing, the collection hash unchanged), cancel during the backup | same file (fake ops); each op's unit adds its own ordering test (U28, U29, U34, U35, U49, U109, U121, U124, U128, U130, U131) |
| `back_up_now_writes_when_unchanged`; Back Up Now's result (name, size); refused in ExclusiveBusy and `Failed{Corrupt}` | `core/crates/meguri-package/tests/backup_service.rs` |
| T7: a page-corrupted collection: the safety backup is a byte copy whose SHA-1 equals the source's, `corrupt-…` named, kept by retention, absent from the listing; disk full: `SafetyBackupFailed`, no main copy | `core/crates/meguri-package/tests/restore.rs` |
| T8: a backup with a corrupted page refused, the collection hash unchanged (R1638); refused while a session is open; once in ExclusiveBusy an answer is refused and every answer committed before is in the pre-restore backup; cancel before the rename keeps lifecycle and generation; restore of a server copy | same file |
| T8 end to end: restore removes five later reviews, emits CollectionReplaced and retains the pre-restore backup. R6 publication fencing survives restart and a manual Anki transfer. | same file, in Docker and in the iOS simulator through U133a |
| T6.1 iCloud rows: a `CloudAdopt` of a staged collection takes a B4 backup in its `Working` step, before the swap, and the backup holds every answer committed before the phase; a damaged collection gets a T7 byte copy; no collection file, no backup; disk full gives `SafetyBackupFailed` and the local collection hash is unchanged. Replace iCloud collection writes no backup | U113 and U114's recovery tests, with fake staging (`sync/cloudkit-recovery#V23`) |

Goldens: every `package/backup-triggers` scenario has one, recorded by the oracle with the op `backup` (`specs/conformance/backup-trigger-ops.md` H1), and their outcomes follow T2 and T4. Two goldens hold Anki's side of a deviation, and the deviation gives Meguri's: `forced-unchanged` (T4.2) and `reopen` (T2.3). The scenarios fail goldens-check and the T0 run until U127's adapter declares `backup` and the testkit's scenario model reads it (`backup-trigger-ops` H4), besides U126's `add_backup_files`, `force_backup` and `backup_files` (`backup-ops` F8).
