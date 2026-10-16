# Opening a collection and its connections

Spec: storage/open-and-connections
Requirements: R1449, R1451, R1452, R1454, R1479, R1482, R2013, R2016; supports R1823, R1491 and R2006 (C9, with U32)
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/open/

How Meguri decides whether it may open a file, opens it without touching files it must refuse, and which SQLite connections it then keeps with which settings. The file layout is `storage/schema-18`; the upgrade of schemas 11 to 17 is `storage/schema-upgrade`. Close orchestration, suspension, the sidecar and the advisory lock that keeps a second Meguri process out (R2014) are U32's; this spec gives the storage primitives they use.

## C1. Versions

`col.ver` decides what happens. Anki 26.09.3 accepts the same versions and refuses the same ones with the same two kinds.

| `col.ver` | Meguri | Error class (C8) | Scenario rejection kind, both executors (`specs/conformance/storage-ops.md` P1.2) |
|---|---|---|---|
| below 11 | refuse | `TooOld` | `file_too_old` |
| 11, 14, 15, 16, 17 | copy, upgrade to 18, open (`storage/schema-upgrade`) | | |
| 12, 13 | refuse | `TooNew` | `file_too_new` |
| 18 | open | | |
| above 18 | refuse | `TooNew` | `file_too_new` |

- 12 and 13 were intermediate versions of unreleased clients; a file at either was left by a newer or unclean client and must be closed cleanly by that client first. They share the "made by a newer or unclean client" message class with versions above 18 (R1479). The error carries the version found, so the UI can say which.
- A file that is not SQLite, has no table `col`, has no row in it, or whose `ver` is not an integer is refused with `NotACollection`. Anki would instead try to create a new collection inside such a file; Meguri never writes into a file it did not recognize.
- Meguri never downgrades a file on open (R2013). The only downgrade is U33's schema-11 writer, which runs on a staging copy.

## C2. The open gate

One function, callable on any file path (the live collection, and U124's staging copies before a swap). It runs these phases in order and stops at the first refusal. Before phase 4 nothing has written to the file or created a file next to it, with one exception: C3's in-place rollback of a hot journal, which happens only after a rolled-back copy of the file has passed phases 1 and 2.

### C2.1 Phase 1: probe

A read-only connection, opened with a `file:` URI:

| Next to the file | URI query |
|---|---|
| no `-wal` file and no non-empty `-journal` file | `mode=ro&immutable=1` |
| a `-wal` file (any size) | `mode=ro` |
| a non-empty `-journal` file | `mode=ro` (C3, hot rollback journal) |

- Why: a plain read-only connection to a WAL-mode file creates `-wal` and `-shm` files when the directory is writable, and cannot remove them when it closes. `immutable=1` creates nothing, but it ignores both a `-wal` file, whose frames may hold the newest `col.ver`, and a hot rollback journal, behind which the main file may hold half-written pages. In those two cases the probe reads without `immutable`; with a `-wal`, SQLite may then create a `-shm` (the WAL index, no data) next to it.
- The path goes into the URI percent-encoded: every byte outside `A-Z a-z 0-9 - . _ ~ /` is written `%XX` (upper-case hex), so paths with spaces (`Application Support`), `?`, `#` or `%` open the right file.
- The probe registers `unicase` (C5) and nothing else.
- It reads, with no table scan (R1823): whether `sqlite_master` has a table `col`; `ver` from the first row of `col`; and, for C2.2, one JSON value. SQLite's "not a database" error (`SQLITE_NOTADB`) refuses with `NotACollection`, its corruption error (`SQLITE_CORRUPT`) fails with `Corrupt`, C3's rollback case is handled there, and any other error fails with `Io`.
- It then closes, before phase 3 or 4 opens anything else.

### C2.2 Phase 2: refusals

1. `ver` per C1: `TooOld`, `TooNew` or `NotACollection`.
2. The v1-scheduler marker (R1482). The scheduler version is the JSON value of the collection config key `schedVer`:
   - at `ver` 11, the member `schedVer` of the JSON object in `col.conf`;
   - at `ver` 14 to 18, the `val` of the `config` row whose `KEY` is `schedVer` (a primary-key lookup).

   The collection uses the v2 or v3 scheduler only when that value parses as the JSON integer `2`. A missing key, `1`, any other number (`3`, `2.0`), a string or unparseable JSON mean the v1 scheduler: refuse with `V1Scheduler`. Anki reads the key the same way and treats the same values as v1, but opens such a file and upgrades its scheduler in place; Meguri refuses instead and tells the user to open the collection once in current Anki (deviation `storage-v1-scheduler-refused`, R1482). When `col.conf` at `ver` 11 is not a JSON object, refuse with `UpgradeFailed` (the upgrade could not read it either).

### C2.3 Phase 3: pre-upgrade copy

Only when `ver` is 11, 14, 15, 16 or 17: `storage/schema-upgrade#U2` (R1480). Nothing is written to the original before the copy is complete, except C3's rollback of a hot journal, which restores the last committed state that the copy then holds.

### C2.4 Phase 4: read-write open

1. Open the writer connection (C4): read-write, never create (`SQLITE_OPEN_READWRITE` without `SQLITE_OPEN_CREATE`), same URI encoding, no `immutable`.
2. Run the connection initialization (C5), then the writer settings of C4 in their order (the journal mode switch is among them).
3. When `ver` is below 18: run the upgrade (`storage/schema-upgrade`) as one transaction on the writer. Inside it, read `col.ver` again; if it differs from the probe's value, roll back and refuse as C1 says for the new value.
4. Report the open (and the upgrade's progress) to the caller. U24 turns it into the lifecycle states `Opening` and `Upgrading`.

### C2.5 Phase 5: expected objects

Run `storage/schema-18#S6.1` on the writer. `missing_tables` refuses with `NotACollection` (the writer closes; an upgrade committed in phase 4 stays committed, the pre-upgrade copy stays too). Otherwise the result goes with the opened collection to the caller.

### C2.6 What an open never does

- No table scans: the whole gate, for a file at 18, reads the `col` row, at most one `config` row and `sqlite_master`. A warm open of `realistic-150k` takes at most 150 ms (R1823, measured by U12 and U241).
- Host measurement so far (not the device numbers, which U12 owns and records here): `tests/open_budget.rs` grows `schema-18.anki2` to 100,000 notes, 150,000 cards and 450,000 review entries (a 60 MB file), closes it, opens it once to warm the cache, then times 40 opens through the whole gate (probe, refusals, writer with its settings, expected-object check) and fails when the p95 is above 150 ms. In the Linux development container on arm64 (Apple silicon host) with the bundled SQLite, four runs gave p50 0.20–0.23 ms, p95 0.23–0.41 ms and a maximum of 0.60 ms.
- No integrity check on a normal open (Anki does not either). Callers that need one run `PRAGMA integrity_check` after the gate (U124 on staging copies, U128).
- No write to the file other than the journal-mode switch, an upgrade and C3's rollback of a hot journal. In particular, `col.mod` and `col.scm` do not change on open.

## C3. Files Meguri refuses stay untouched

- A refused file (C1, C2.2) keeps its exact bytes (same SHA-256) and the directory gains no file: no `-wal`, no `-shm`, no `-journal`. A `-journal` file that was already there keeps its bytes too. The one exception: when a `-wal` file was already there, a `-shm` may appear next to it (C2.1). Pre-upgrade copies are made only after the refusals (C2.3).
- Anki does not keep this guarantee: it switches a rollback-journal file to WAL mode before it checks the version, so a refused file comes back with a changed header. Goldens therefore record only the refusal kind; `tests/open_refuse.rs` asserts the bytes and the directory listing, for WAL-mode and rollback-journal fixtures and for a file behind a hot journal.
- Hot rollback journal: a non-empty `-journal` file next to a rollback-journal file means a client crashed in the middle of a write. The main file may hold half-written pages, and only a rollback shows the last committed `col.ver`. The `mode=ro` probe fails with SQLite's read-only rollback error (`SQLITE_READONLY_ROLLBACK`, extended code 776). The gate then decides on a copy, so that a file it refuses keeps its bytes and its journal (R1479):
  1. Copy the file and its `-journal` into a new temporary directory outside the collection's directory, as one name and that name plus `-journal`, so that SQLite pairs them. If the copy cannot be made, the open fails with `Io`.
  2. Roll the copy back there: open it read-write (registering `unicase`, C5), read `col.ver`, close. SQLite rolls the journal back on that first read and removes it.
  3. Run phases 1 and 2 on the copy. A refusal ends the open: the original's bytes, its journal and its directory stay unchanged. The temporary directory is removed whatever the outcome.
  4. Only when the copy passes: open the original read-write without changing its journal mode, read `col.ver` so that SQLite rolls it back (restoring the last committed state, as any SQLite client does on first read, and removing the journal), close, and run phases 1 and 2 again on the original. Phases 3 to 5 follow as usual.

  If a probe after a rollback, of the copy or of the original, meets the read-only rollback error again, the gate fails with `Io`.

## C4. The writer connection

Exactly one per open collection, owned by the engine's writer thread (U24); every write to `collection.anki2` goes through it (only meguri-storage issues SQL against the file, KTD2). Settings, in this order, right after C5:

| Setting | Value | Why |
|---|---|---|
| `busy_timeout` | 1000 ms | another process never writes (R1450, advisory lock R2014); waits only for Meguri's own readers when checkpointing |
| `locking_mode` | `NORMAL` (SQLite's default; never set `EXCLUSIVE`) | readers on other connections (R1452). Anki holds an exclusive lock that keeps every other connection out; Meguri deliberately does not (deviation from Anki, no golden can see it) |
| `page_size` | 4096 | before `journal_mode`: it only takes effect for a file that has no page yet (U28's new collection), and switching to WAL writes the first page; a no-op otherwise, as in Anki |
| `cache_size` | -40960 (40 MiB) | Anki's value |
| `temp_store` | `MEMORY` | |
| `journal_mode` | `WAL` | persistent in the file; Anki writes and uploads WAL-mode files. A no-op when the file is already in WAL mode |
| `synchronous` | `NORMAL` (C9) | a commit returns without a sync; U32's flush makes it durable within 1 s |
| `fullfsync`, `checkpoint_fullfsync` | `ON` on Apple platforms, whatever `synchronous` is (C9) | every sync SQLite still issues (checkpoints, the WAL header on restart) then flushes the drive's cache; a plain `fsync` on Apple does not |
| prepared-statement cache | 128 statements | |
| `wal_autocheckpoint` | SQLite's default (1000 pages) | passive checkpoints after commits |

`foreign_keys` stays at the bundled build's default; the schema declares no foreign keys, so it has no effect (Anki's build has the same default). Writes run in `BEGIN IMMEDIATE` transactions (U23's write scopes); the upgrade is one of them.

## C5. Connection initialization

Every connection Meguri opens to a collection file (probe, writer, every reader) first registers the collation `unicase` with the comparison of `text/unicode-primitives#U2` (`unicase` crate `=2.6.0`, KTD2). If registration fails, the connection is closed and the open fails with `ConnectionInit`: Meguri never touches a collection without the collation, because writing through an index ordered by another comparison corrupts it (R1451).

Then, on the writer and every reader (not the probe), the registered **initializers** run, in registration order. Other crates use them to add the SQL functions their queries need (search U80/U81, queue building U41) without meguri-storage depending on them. The set of initializers is fixed when the collection opens; a failing initializer fails that connection's open with `ConnectionInit`. Functions are registered deterministic and innocuous, and appear only in statements, never in schema objects (`storage/schema-18#S4`).

## C6. Readers

Read-only connections, each assigned to one role and never pooled or shared across roles (KTD3). All are opened with `mode=ro` (`SQLITE_OPEN_READONLY`), only after the writer has set WAL mode, and they run C5.

| Role | Users | `query_only` |
|---|---|---|
| interactive | review session, rendering, Card Info, editor open | on |
| query | row pages, deck counts, heatmap | on |
| search worker (one per worker; the search area sets the count) | searches; interruptible | on |
| sync | normal and one-way sync snapshots, `VACUUM INTO` for a full upload | off |
| maintenance | maintenance, backups (`VACUUM INTO`), stats cache builds, derived rebuilds | off |
| server (two, macOS only, while the local servers run) | local servers' read handlers (R1790) | on |

- Every reader: `busy_timeout` 250 ms, `cache_size` -40960, `temp_store` `MEMORY`, prepared-statement cache 128.
- `query_only` makes SQLite refuse every write, temporary tables included. The sync and maintenance readers do without it because `VACUUM INTO` fails under it; on them `mode=ro` still refuses every write to the collection file (SQLite's read-only error). SQLite would allow TEMP tables there; Meguri creates none on any reader.
- A reader is opened when its role is first used and kept until the collection closes; server readers open and close with the servers.
- Long-lived snapshots (a sync, a backup, a derived rebuild) never use the interactive, query or server readers.

## C7. Snapshots and checkpoints

- A reader's read transaction sees the database as of its first read, whatever the writer commits meanwhile, and a writer commit never makes a reader fail with `SQLITE_BUSY` (WAL with normal locking). Tested by `tests/connections.rs`.
- Every open read transaction stops checkpoints from going past it, so the `-wal` grows. Read transactions stay short: page by id ranges, finish every statement within the call that started it, and never keep a statement open across FFI calls.
- **Checkpoint primitive** (R1454). On the writer, outside any transaction (inside one it fails with `TransactionOpen` and does nothing): run `PRAGMA wal_checkpoint(TRUNCATE)` once, and return SQLite's three numbers: busy flag, frames in the log, frames checkpointed. It succeeded when the busy flag is 0; then the main file holds every committed transaction and the `-wal` file is empty. When busy (a reader still holds an older snapshot), it returns the outcome without retrying; the caller (U32) decides. Anki's equivalent fails in the same situation.
- Sending the collection anywhere (R1454): backups, exports and full uploads read a `VACUUM INTO` snapshot from the maintenance or sync reader (KTD3), which is complete without a checkpoint. A caller that copies or sends the collection file itself runs the checkpoint primitive first and copies only after it succeeded.

## C8. Errors

Typed errors of the open gate. Each refusal tells the user what to do (UI copy: U153).

| Error | When | Data |
|---|---|---|
| `TooOld` | `ver` below 11 | version found |
| `TooNew` | `ver` 12, 13 or above 18 | version found |
| `V1Scheduler` | C2.2 | |
| `NotACollection` | C1, or required tables missing (C2.5) | missing table names |
| `UpgradeFailed` | the upgrade could not convert the file (`storage/schema-upgrade#U10`) | version found, step that failed |
| `Corrupt` | SQLite reports the file corrupt while the gate reads it | |
| `ConnectionInit` | collation or initializer registration failed (C5) | |
| `NotFound` | no file at the path | |
| `Io` | any other I/O or SQLite failure | |

A new collection is not opened through this gate; U28's bootstrap creates it.

## C9. Durability of the writer

Decided on 2026-10-15 by the switch rule (C9.4): the writer runs `synchronous=NORMAL`, and on Apple platforms it keeps `fullfsync=ON` and `checkpoint_fullfsync=ON`. A commit returns without a sync, and U32's flush (C9.2) makes it durable within 1 s. The setting is the same on every platform.

### C9.1 What the writer does

- In WAL mode, `NORMAL` writes a commit's frames to the `-wal` file and returns without a sync. SQLite still syncs in three places: the `-wal` before a checkpoint copies its frames, the main file after the checkpoint, and the `-wal` header when a commit starts the log again after a complete checkpoint. With the two Apple pragmas, each of these syncs is a full one (`F_FULLFSYNC`, which also flushes the drive's cache).
- A commit that runs the passive autocheckpoint (C4) or starts the log again therefore still pays full syncs inside its write phase. Such commits are rare (the autocheckpoint runs once per 1000 pages of log), so they show in p99 and the maximum rather than at p95.
- None of these three pragmas is stored in the file. Anki sets none of them and keeps SQLite's defaults; it opens a file Meguri wrote with no difference, and no golden can observe the setting.

### C9.2 The flush (U32)

Without the flush, a returned commit becomes durable only at the next checkpoint or at close. No release ships before U32 delivers it.

- After every commit on the writer that appended frames to the `-wal`, a full sync of the `-wal` file completes at most 1 s after that commit returned. `flush_for_background()` and `suspend()` return only when every commit that returned before them is covered.
- The flush syncs the `-wal` file itself. It never relies on a checkpoint: a checkpoint held back by a reader's snapshot (C7; for example U107's sync snapshot) syncs nothing.
- A full sync is `F_FULLFSYNC` (an `fcntl` command) on Apple platforms, and the platform's file sync elsewhere.
- One sync covers every commit that returned before the sync started. A commit that returns during a sync is covered by the next one. Commits close together share one sync.
- The first flush after the `-wal` file was created also syncs the directory that holds it, so that the file's directory entry survives a power loss.
- The flush runs outside every op's write phase, so op timings (`engine/op-model` O10) do not include it. Its cost is one full sync, about 4 ms at p50 on the M3 Max (C9.4), at most once per second while commits arrive.
- A failed sync is never silent. The commits it covered have already returned, so U32 reports the failure as an I/O error; its design doc decides what the user sees.
- One route that meets all of this with the bundled SQLite: on the writer's thread, between ops, the file control `SQLITE_FCNTL_JOURNAL_POINTER` returns the connection's `-wal` file. A sync of that file with the full-sync flag issues `F_FULLFSYNC` on Apple, and SQLite adds the one-time directory sync when this connection created the file.
- U32's counting-VFS test proves the flush: a reader holds a snapshot, and each of 20 answer commits is followed by a full sync of the `-wal` within 1 s. U32 records the measured cost in C9.4.

### C9.3 What a failure can lose

- A killed process (SIGKILL, a jetsam kill, a crash of Meguri) loses no returned commit, because its frames are already in the operating system's cache.
- An OS crash or a power loss can lose the commits whose flush had not completed: at most those of the last second. Recovery keeps the commits up to the first one that did not fully reach the disk (the `-wal` frames carry checksums), so the reopened collection is an earlier consistent state and never corrupt (R1491).
- R1491 and R2006 allow this window (durable within 1 s). R2002's bound of one in-flight operation still holds for a killed process. For an OS crash or a power loss, the bound is this window, which the switch rule accepts.

### C9.4 Why: the rule and the measurements

The rule (U22): `synchronous=FULL` with full syncs on Apple, unless U12's answer-commit benchmark shows the answer write phase, commit sync included, above U24's p95 budget of 5 ms on the target devices. Then `NORMAL`, with U32's flush.

The answer write is one op that reads a card, updates it and inserts one review-log row, timed from submit to result, commit included (`engine/op-model` O10; suite `core/crates/meguri-engine/benches/executor.rs`), on generated collections of 10k and 150k cards.

| Where | SQLite | Writer setting | Answer write p50 | p95 |
|---|---|---|---|---|
| M3 Max, macOS 27.0.1, internal SSD; `tools/mg bench-mac --bench executor`, two runs (2026-10-15) | bundled 3.53.2 | `FULL`, `fullfsync`, `checkpoint_fullfsync` | 4.02–4.04 ms | 4.97–5.09 ms (5 ms or more in three of four) |
| Docker, Linux arm64, collection on tmpfs; `tools/mg bench --bench executor`, one run | bundled 3.53.2 | `FULL` (a sync costs nothing on tmpfs) | 0.043–0.054 ms | 0.11–0.16 ms |
| Development Mac, earlier preliminary run | macOS system 3.54.0 | `FULL` with full fsync | 0.22 ms | 0.40 ms |
| same | same | `FULL` alone | | 0.12 ms |
| same | same | `NORMAL` | | 0.03 ms |
| M3 Max, after the switch: awaiting the rerun of `tools/mg bench-mac --bench executor` | bundled 3.53.2 | `NORMAL`, `fullfsync`, `checkpoint_fullfsync` | | |
| iPhone and iPad: not measured (no device runs until signing allows device builds) | bundled 3.53.2 | | | |

On the M3 Max the answer's p99 was 8.2–9.4 ms and its maximum 16–56 ms.

- About 4 ms of each Mac answer is the commit's full sync: the same op costs 0.11–0.16 ms at p95 when the sync costs nothing.
- The earlier preliminary p95 of 0.40 ms "with `FULL` and full fsync" does not describe Meguri. It was taken with the macOS system SQLite, not the bundled build that Meguri ships (KTD3), and its full-sync commits cost a small fraction of the bundled build's (p50 0.22 ms against 4.02 ms). It is superseded.
- The rule fires without the iPhone and iPad numbers. The Mac is one of Meguri's target platforms, and on the M3 Max that U24's verification names, the answer write is at the 5 ms budget and over it in three of four measurements. The cost is the drive-cache flush that a full sync must make, so code changes cannot remove it.
- Device numbers, when U12 has them, are recorded in this table. They cannot reverse the choice, because the setting is the same on every platform and the Mac is already over the budget.

## C10. Close

The storage side of closing (U32 orchestrates it):

1. Finish every reader's statements and close every reader.
2. Run the checkpoint primitive on the writer. With every reader closed it cannot be busy. Its syncs (full ones on Apple, C9.1) also make the last commits durable, whatever U32's flush has done.
3. Close the writer. SQLite removes the `-wal` and `-shm` files when the last connection closes after a complete checkpoint.

After a clean close, `collection.anki2` alone holds the whole collection, no `-wal` or `-shm` file is left, and Anki opens it without any conversion (R1449, scenario `storage/open/oracle-made-reopen`).

## C11. Scenarios and tests

| What | Where |
|---|---|
| Anki-made collection opened and closed by Meguri, then `integrity_check` `ok` in Anki (T1 live) | `storage/open/oracle-made-reopen` |
| Versions 10, 12, 13 and 19 refused, WAL-mode and rollback-journal files | `storage/open/refuse-versions` (kinds only, C3) |
| v1-scheduler collections at 18 and 11 refused | `storage/open/v1-scheduler` (deviation `storage-v1-scheduler-refused`) |
| Collation order and a unique-index collision (`INSERT OR IGNORE`, rows kept) | `storage/open/collation` |
| A plain insert of a name equal under folding to a stored one fails with extended code 2067 (1555 for a tag) and stores nothing, for decks, note types, fields and templates (per note type) and tags; presets have no unique index on `name` | `tests/collation_order.rs` |
| Refused file unchanged: SHA-256 and directory listing | `tests/open_refuse.rs` (Meguri only) |
| Hot journal, file refused: the refusal is the one for the rolled-back version (a file at 10 whose crashed transaction had written 18 is `TooOld` 10); the file's SHA-256, the journal's SHA-256 and the directory listing are unchanged | `tests/open_refuse.rs` (Meguri only) |
| Hot journal, file accepted: the file is rolled back in place and opens at its last committed version (a file at 18 whose crashed transaction had written 10 opens at 18 with no upgrade); the crashed transaction's objects are gone, `integrity_check` is `ok`, and the journal is gone | `tests/open_refuse.rs` (Meguri only) |
| Writer settings per C4: `synchronous` reads 1 (`NORMAL`); `fullfsync` and `checkpoint_fullfsync` read 1 on Apple platforms and 0 elsewhere | `tests/connections.rs` |
| With a reader holding a snapshot, each of 20 answer commits is followed by a full sync of the `-wal` within 1 s (C9.2) | U32's counting-VFS test |
| Answer write p95 under 5 ms on the M3 Max with C9's setting | `tools/mg bench-mac --bench executor` (`engine/op-model` O10) |
| Reader keeps its snapshot during a writer commit; no `SQLITE_BUSY` on readers | `tests/connections.rs` |
| `VACUUM INTO` succeeds on the sync and maintenance readers; a write fails on every reader role | `tests/connections.rs` |
| Collation registration failure fails the open | `tests/connections.rs` |
| No `-wal`/`-shm` after close; the main file alone holds the data | `tests/connections.rs` |
| Warm open of `realistic-150k` within 150 ms, no full scan in the query plans | U12 benchmark (R1823) |
| Warm open of a 150,000-card schema-18 file within 150 ms at p95, on the host (C2.6) | `tests/open_budget.rs` |
