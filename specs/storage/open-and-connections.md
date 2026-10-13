# Opening a collection and its connections

Spec: storage/open-and-connections
Requirements: R1449, R1451, R1452, R1454, R1479, R1482, R2013, R2016; supports R1823
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

One function, callable on any file path (the live collection, and U124's staging copies before a swap). It runs these phases in order and stops at the first refusal. Before phase 4 nothing has written to the file or created a file next to it (C3).

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

Only when `ver` is 11, 14, 15, 16 or 17: `storage/schema-upgrade#U2` (R1480). Nothing is written to the original before the copy is complete.

### C2.4 Phase 4: read-write open

1. Open the writer connection (C4): read-write, never create (`SQLITE_OPEN_READWRITE` without `SQLITE_OPEN_CREATE`), same URI encoding, no `immutable`.
2. Run the connection initialization (C5), then the writer settings of C4 in their order (the journal mode switch is among them).
3. When `ver` is below 18: run the upgrade (`storage/schema-upgrade`) as one transaction on the writer. Inside it, read `col.ver` again; if it differs from the probe's value, roll back and refuse as C1 says for the new value.
4. Report the open (and the upgrade's progress) to the caller. U24 turns it into the lifecycle states `Opening` and `Upgrading`.

### C2.5 Phase 5: expected objects

Run `storage/schema-18#S6.1` on the writer. `missing_tables` refuses with `NotACollection` (the writer closes; an upgrade committed in phase 4 stays committed, the pre-upgrade copy stays too). Otherwise the result goes with the opened collection to the caller.

### C2.6 What an open never does

- No table scans: the whole gate, for a file at 18, reads the `col` row, at most one `config` row and `sqlite_master`. A warm open of `realistic-150k` takes at most 150 ms (R1823, measured by U12 and U241).
- No integrity check on a normal open (Anki does not either). Callers that need one run `PRAGMA integrity_check` after the gate (U124 on staging copies, U128).
- No write to the file other than the journal-mode switch and an upgrade. In particular, `col.mod` and `col.scm` do not change on open.

## C3. Files Meguri refuses stay untouched

- A refused file (C1, C2.2) keeps its exact bytes (same SHA-256) and the directory gains no file: no `-wal`, no `-shm`, no `-journal`. The one exception: when a `-wal` file was already there, a `-shm` may appear next to it (C2.1). Pre-upgrade copies are made only after the refusals (C2.3).
- Anki does not keep this guarantee: it switches a rollback-journal file to WAL mode before it checks the version, so a refused file comes back with a changed header. Goldens therefore record only the refusal kind; `tests/open_refuse.rs` asserts the bytes and the directory listing, for WAL-mode and rollback-journal fixtures.
- Hot rollback journal: when a non-empty `-journal` file lies next to a rollback-journal file (an old client crashed in the middle of a write), the `mode=ro` probe fails with SQLite's read-only rollback error (`SQLITE_READONLY_ROLLBACK`, extended code 776). The gate then opens the file read-write without changing its journal mode, reads `col.ver` so that SQLite rolls the journal back (which restores the last committed state, as any SQLite client does on first read), closes, and runs phases 1 and 2 again. This is the only case in which the gate writes to a file it may then refuse. If the second probe meets the error again, the gate fails with `Io`.

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
| `synchronous` | C9 | |
| `fullfsync`, `checkpoint_fullfsync` | `ON` on Apple platforms when `synchronous=FULL` (C9) | a plain `fsync` on Apple does not flush the drive's cache |
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

- Default, in force until the measurement below says otherwise: `synchronous=FULL`, and on Apple platforms `fullfsync=ON` and `checkpoint_fullfsync=ON`. Each commit is then durable on return. Anki leaves SQLite's default (`FULL` in WAL mode) and never sets `fullfsync`.
- Switch: if U12's answer-commit benchmark shows the answer write phase, commit sync included, above U24's p95 budget of 5 ms on the target devices, the writer uses `synchronous=NORMAL` instead, and U32's flush, at most 1 s after any commit and before suspension, issues a full sync of the `-wal` file itself. That flush must not depend on checkpoint progress, because a checkpoint blocked by a reader syncs nothing.
- Measured so far (preliminary, not the deciding run): on the development Mac (Apple silicon, internal SSD, macOS system SQLite 3.54.0), committing one card update plus one review-log insert in WAL mode took p50 0.22 ms and p95 0.40 ms with `FULL` and full fsync, p95 0.12 ms with `FULL` alone, p95 0.03 ms with `NORMAL`. The deciding numbers come from U12's benchmark of the bundled SQLite on iPhone and iPad; they are recorded in this section by a spec-writer change, and the choice follows them.

## C10. Close

The storage side of closing (U32 orchestrates it):

1. Finish every reader's statements and close every reader.
2. Run the checkpoint primitive on the writer. With every reader closed it cannot be busy.
3. Close the writer. SQLite removes the `-wal` and `-shm` files when the last connection closes after a complete checkpoint.

After a clean close, `collection.anki2` alone holds the whole collection, no `-wal` or `-shm` file is left, and Anki opens it without any conversion (R1449, scenario `storage/open/oracle-made-reopen`).

## C11. Scenarios and tests

| What | Where |
|---|---|
| Anki-made collection opened and closed by Meguri, then `integrity_check` `ok` in Anki (T1 live) | `storage/open/oracle-made-reopen` |
| Versions 10, 12, 13 and 19 refused, WAL-mode and rollback-journal files | `storage/open/refuse-versions` (kinds only, C3) |
| v1-scheduler collections at 18 and 11 refused | `storage/open/v1-scheduler` (deviation `storage-v1-scheduler-refused`) |
| Collation order and a unique-index collision | `storage/open/collation` |
| Refused file unchanged: SHA-256 and directory listing | `tests/open_refuse.rs` (Meguri only) |
| Reader keeps its snapshot during a writer commit; no `SQLITE_BUSY` on readers | `tests/connections.rs` |
| `VACUUM INTO` succeeds on the sync and maintenance readers; a write fails on every reader role | `tests/connections.rs` |
| Collation registration failure fails the open | `tests/connections.rs` |
| No `-wal`/`-shm` after close; the main file alone holds the data | `tests/connections.rs` |
| Warm open of `realistic-150k` within 150 ms, no full scan in the query plans | U12 benchmark (R1823) |
