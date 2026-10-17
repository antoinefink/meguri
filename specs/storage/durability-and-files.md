# Durability and file safety

Spec: storage/durability-and-files
Requirements: R1450, R1453, R1454, R1491–R1493, R2002, R2004, R2006, R2007, R2011, R2012, R2014, R2015
Anki: none (Meguri design, U32)
Scenarios: Rust integration tests in meguri-storage and meguri-engine

## F1. Paths and ownership

Swift supplies the private collection directory and a separate backups directory. `CollectionPaths` names collection.anki2, collection.media, collection.media.db, meguri-derived.db and meguri-sidecar.json there. It rejects overlapping backups and collection directories. File protection is inherited from Swift's directory (U151). The widget never opens these files.

The engine holds an advisory `meguri.lock` in the collection directory for the complete open lifetime, including suspension and replacement. It is never unlinked: unlinking a locked inode would let another process lock a different inode. A failed nonblocking acquisition reports “open in another program”. The SQLite writer also probes `BEGIN IMMEDIATE` and rolls back before changing settings, so an exclusive owner is refused. This cannot exclude an uncooperative client that starts later; data-version detection covers its commits.

## F2. Commit durability

Use open-and-connections C9 unchanged: NORMAL commits and the writer's full WAL sync at most one second later, even when a snapshot prevents checkpoints. Background flush explicitly syncs the WAL after participants flush and the accepted queue drains. Close and suspend close readers, perform TRUNCATE, and require its busy flag to be zero before marking clean. No clean flag is written on a failed checkpoint. SQLite transactions remain the unit of atomicity.

An automatic WAL-sync failure publishes `DurabilityStatus::Failed { through, error }` on the ordered extension-event stream at the first failure of an episode. Retrying never advances coverage. The first successful retry publishes `Recovered { through }`; repeated failures do not flood the stream. The event consumer can present the I/O error while keeping already-committed changes intact (presentation belongs to U153). Explicit background flush and suspend return their I/O failures directly. No error is called corruption merely because a disk sync failed.

## F3. Sidecar

`meguri-sidecar.json` is device-only JSON with clean_shutdown, last_mod, last_scm and upgrade_in_progress. Unknown JSON members survive writes. Missing or malformed state counts as unclean. Managed open records the previous cleanliness and compares mod/scm before setting clean_shutdown=false durably. An upgrade sets upgrade_in_progress before its transaction, and clears it only after successful upgrade. Close and suspend save the final mod/scm and clean_shutdown=true after a successful checkpoint; resume records false before admitting writes. A failed open or operation never claims a clean shutdown.

All four known members must be present with their declared types for a record to count as valid. A valid last_mod/last_scm pair of zero is compared normally; absence is tracked separately from timestamp values.

External changes while connections are open use `PRAGMA data_version` on the writer (which excludes its own commits), checked inside BEGIN IMMEDIATE before each write, and on resume. Suspension closes SQLite connections, so data_version tokens cannot span it: resume additionally compares the saved file identity, size, mtime/ctime and WAL state, and the sidecar mod/scm. This catches edits even when their client leaves mod/scm unchanged. The reopen establishes a fresh data-version baseline.

## F4. Atomic files

`AtomicFile` creates a unique `.meguri-tmp-<purpose>-*` file in the destination directory with exclusive creation. Purposes are replace, stage, export, backup, media and sidecar. Write, full sync, caller verification, rename, then full directory sync in that order. The verifier must close all handles before rename. A verification or pre-rename I/O failure leaves the destination unchanged and removes the temp on normal unwinding. ENOSPC/EDQUOT become InsufficientSpace. Failure after rename reports an error but the new verified file is visible; callers must not assume the old inode survived. `publish_new` uses exclusive publication for backup names and refuses a destination collision without touching the existing file. A cancelled caller drops its AtomicFile without publishing. A panic has the same cleanup. A process kill leaves only a recognized temp.

The engine refuses to invoke a collection-swap callback if the preceding collection close fails or its final checkpoint is busy. It attempts to reopen the original and returns that failure without advancing the generation.

Launch cleanup runs under the collection lock in the supplied collection/backups directories and in collection.media. Media cleanup removes only top-level reserved regular files; it never follows a symlink used as the media directory or descends into media subdirectories. It removes regular files (never follows symlinks) whose names use that exact temp namespace, plus their SQLite -wal/-shm/-journal siblings, and the named legacy sync-stage, partial-export and partial-backup patterns. The reserved `.meguri-stage-*` directories created by `staging_directory` are recursively removed without following symlinks; other directories are preserved. Unrecognized files, complete backups and pre-upgrade copies survive. These names are reserved for unpublished work; higher units use AtomicFile for new staging/backup/export files.

## F5. Space and integrity

`SpaceProvider` supplies available bytes for a directory; engine's `DiskSpaceProvider` is this port re-exported at the engine layer for Swift composition. The default uses statvfs available blocks, with checked multiplication. A check compares the caller's complete required budget against available bytes, returning InsufficientSpace without starting work. Each consumer owns its size estimate. Real I/O failures still map disk exhaustion to the same error.

`Reader::quick_check` uses `PRAGMA main.quick_check` on that reader, never the writer; attached rebuildable stores are checked separately and cannot make the collection report corrupt. `integrity_check(path)` uses `PRAGMA main.integrity_check` on any existing file opened read-only, registers unicase, and returns every diagnostic (including SQLite corruption errors). Only a single “ok” is healthy. Scheduling checks and the Failed(Corrupt) policy belong to U132; these primitives do not block Ready or stop the writer themselves.

## F6. Fault boundaries

Named points: storage.commit.before, storage.commit.after; storage.upgrade.after_copy, storage.upgrade.in_transaction; storage.atomic.after_write, storage.atomic.after_sync, storage.atomic.after_verify, storage.atomic.before_rename, storage.atomic.after_rename; storage.stage.before_rename; storage.backup.before_rename; storage.sidecar.before_save; existing derived-store points. All use meguri-model's test-hooks faultpoint macro. Post-commit faults can report failure with a committed transaction: they model a lost acknowledgment, never a rollback of committed data.
