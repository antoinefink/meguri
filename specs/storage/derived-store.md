# The derived store

Spec: storage/derived-store
Requirements: R1487, R1488, R1489, R2016; supports R1453 (with U32), R1819
Anki: none (Meguri design: no Anki behavior; written by the implementer, U26 execution note)
Scenarios: none; tests in `core/crates/meguri-storage/tests/derived_store.rs` and `core/crates/meguri-engine/tests/derived_lag.rs`

Meguri's own indexes (count caches, folded text and full-text search, browser caches, daily aggregates, stats column caches, the media reference index) live in one SQLite file next to the collection, `meguri-derived.db`, and in the files a component owns beside it (D1; `stats_columns`' column files, `stats/graphs-common#G9.3`). Everything in the store can be deleted at any time and rebuilt from the collection and the media folder. This spec covers the file, its stamp, how components register, how the store stays current (incrementally from committed ops, by catch-up, by rebuild), what readers see, and when the store is invalidated. The storage primitives are `meguri-storage/src/derived/`; the engine side (threads, observer, readiness, generations) is `meguri-engine/src/derived_host.rs`.

## D1. Placement

- The file is `meguri-derived.db` in the collection's directory, with SQLite's `-wal` and `-shm` files next to it while open.
- A component may also own files beside it, in a directory next to `meguri-derived.db` that only that component uses (`stats_columns`' column files, `stats/graphs-common#G9.3`). They are part of the store under every rule of this section, and they are content of the component only through a record table of its own: a file is live only while a row of that table names it, and a file no row names is deleted (at bind, or after a swap once no reader maps it). Deleting them, or `meguri-derived.db`, while the collection is closed loses nothing; files left by a deleted or recreated store are named by no row, so the next bind deletes them (D5.1).
- Nothing derived is ever written to `collection.anki2` (R2016): no table, index, view, trigger or temporary object, and no `ATTACH` on the collection writer. The collection's `sqlite_master` is the same before and after any derived activity.
- The file is never synced, backed up or exported: sync, backups (`VACUUM INTO` of the collection's `main` schema), `.colpkg` and `.apkg` exports read the collection and the media folder only, and the file is not in the media folder.
- Deleting the file while the collection is closed loses nothing: the next open rebuilds every component (R1488).

## D2. File format

- WAL journal mode, `synchronous=NORMAL` (losing the last transactions in a power cut makes the store lag, which D5 detects), `page_size` 4096 for new files, `cache_size` -8192 (8 MiB), `temp_store` `MEMORY`, `busy_timeout` 1000 ms. The `unicase` collation is registered (`storage/open-and-connections` C5), so derived tables may use it.
- `PRAGMA application_id` is `0x4D475244` ("MGRD"); `PRAGMA user_version` is the store format, 1. A file whose application id is another one and that holds tables, or whose format differs, is not this store's and is replaced (D5.1).
- Two tables of the store's own, created when the file is created:

  | Table | Columns |
  |---|---|
  | `meguri_stamp` | `id` (always 1), `crt`, `scm`, `col_mod`, `clean`, `seq` |
  | `meguri_components` | `name` (key), `version`, `ready`, `build` |

- Every other table belongs to one component (D4). Its stored name is `<component>__<table>`; an index's is `<component>__<index>__<build>`. Tables named `meguri_build__<component>__<table>` exist only while a rebuild runs (D7). A table name starts with a letter, so each stored name has one owner: `foo___rows` is table `rows` of `foo_`, never of `foo`. Emptying, swapping or dropping a component's tables takes exactly the names it owns, never a prefix match.

## D3. The stamp

- `crt`, `scm` (seconds and milliseconds as the `col` row stores them) and `col_mod` are the collection mark: which collection the store was derived from and the `col.mod` of the last commit whose changes every ready component holds.
- `clean` is 1 only when the store was closed or suspended (D11) after applying every commit it had been told about; it is set to 0 when the store is bound to an open collection (D5) and when it resumes.
- `seq` is the seq (`engine/op-model` O4) of the last commit the derived writer handled: every ready component holds every commit up to it. The bind writes the bind's seq (D5); each transaction of commits (D6) and the last transaction of each refresh (D7) write the writer's latest. Seqs restart with every process, so `seq` means something only to the process that bound the store; every bind writes it again, and D5.2 never reads it.
- Per component: `version` (the component's own schema version), `ready` (1 when its tables hold the content for the stamp's mark) and `build` (a counter that names its indexes).
- In memory only: the collection generation the store is bound to (`engine/lifecycle` L3), which restarts with every process.
- Stamp and component rows change in the same transaction as the derived rows they describe, so the store is never ahead of what it records. A transaction of the derived file is atomic only for that file: every derived update must be idempotent, because the collection commit it follows may be applied again by catch-up (D7) after a crash.

## D4. Components

A component registers, before the collection opens:

| Item | Rule |
|---|---|
| name | `[a-z][a-z0-9_]*`, at most 40 characters, no `__`, not starting with `meguri` |
| version | a positive integer; a change rebuilds the component |
| tables | each a name (same rule as the component name, at most 40) and either a column list with options (`(id INTEGER PRIMARY KEY, ...) WITHOUT ROWID`) or a virtual table module (`fts5(...)`) |
| indexes | each a name, its table, its column list, and whether it is unique |
| capture | on the collection writer thread, after each commit with a non-empty change set (`engine/change-stream` S2): from the observer payload, the owned data the component needs, or "nothing", or "rebuild" |
| apply | on the derived writer: writes one captured delta into the component's tables, idempotently |
| build | from a collection snapshot: fills the component's tables completely, in batches, cancellable, with progress |
| catch-up | optional: from a collection snapshot and the mark of the last applied commit, brings the tables up to date; or answers "rebuild". Without one, catch-up is a rebuild |
| deferred until | optional: the name of a host signal (`[a-z][A-Za-z0-9]*`, for example `homeIdle`). From each bind (D5) until the store gets that signal for the bound collection, or a consumer calls `request_refresh` for the component (below), a refresh queued for the component is held: it does not start (D7) and the component stays `Stale` (D8). Whichever comes first releases the hold for the rest of that bind; a refresh queued after it starts as D7 allows. A replacement or an invalidation (D10) does not hold it again; the next bind does. Without the item, no refresh is held |
| persist | optional: on the derived writer, writes what the component keeps outside its tables (an in-memory overlay, `stats/graphs-common#G9.3`) into its own files and record rows, in transactions of its own. Run at suspend and close (D11) and by `release` (below) |
| release | optional: on the derived writer, drops what the component holds in memory (mapped files, an overlay). Run only by `release` (below), after the component's persist |

- Components never name stored table names: the store resolves a table name for the target being written (the live tables, or the staging tables of a rebuild).
- Owners: U41 count caches, U80/U90 folded text and full-text search, U84 browser caches, U95 daily aggregates, U98 stats column caches, U130 media reference index.
- Host entry points on the engine's derived host (`derived_host.rs`), callable from any thread but the main thread; none blocks the collection writer:
  - `signal(name)`: records the host signal for the bound collection and releases the hold of every component whose `deferred until` names it. A signal no component names, one sent again, or one sent while no collection is bound changes nothing.
  - `request_refresh(component)`: releases that component's hold and returns at once; a component without the item or already released is unchanged. The caller then waits on D8's events or reads without the component.
  - `release(component)`: queued on the derived writer after every commit already sent to it; runs the component's persist, then its release, and returns once both have run. When persist fails (an error, a panic, or a failed transaction), the writer marks the component `ready` 0 and queues its rebuild, as D6 does for a failed apply, and still runs release; when `ready` 0 cannot be written, the store unbinds (D6). A component without the items is unchanged.
- Added 2026-11-17 for U98 (`stats/graphs-common#G9.2`, G9.3, G9.5): `deferred until`, persist, release and the three entry points. U98 adds them to the store and the derived host, with D13's tests for them; components without the items behave as before.
- A component whose content depends on rows a `Raw` or `CallerSupplied` write can change without moving `col.mod` or the row's modification time (U40's unbury at rollover changes `cards.queue`) does not catch up from modification times: its catch-up answers "rebuild".

## D5. Binding to an open collection

The store binds to each opened collection as the collection's first open step (`engine/lifecycle` L2), before any op of that open commits. The open step reads the collection mark and the last seq in one reader snapshot, then hands both to the derived writer and returns: opening, checking and assessing the file never delay `Ready`. A failure anywhere in D5 is logged and leaves the components `Stale`; it never fails the collection's open.

### D5.1 The file

| Found | Then |
|---|---|
| no file | create it (removing a stray `-wal` or `-shm`) |
| not a database, unreadable, `PRAGMA quick_check(1)` not `ok`, another application id with tables, another format | delete it with its `-wal` and `-shm` and create it again |
| otherwise | open it |

Then every registered component gets its row and its live tables: a missing row, a changed version or a missing table empties it (tables dropped and created empty, `ready` 0). Tables and rows of components that are no longer registered are dropped, and so are staging tables left by an interrupted rebuild.

Then the derived writer sweeps the component-owned files (D1). Each registered component's directory loses every file that no row of its record table names (after the step above, so a component whose tables were emptied loses all its files). The directory of a component that is no longer registered is removed, and so is every component directory left when the file was created or recreated; a component creates its directory again when it next writes a file. A failure to delete is logged and retried at the next bind: a file no row names is never read (D1).

### D5.2 Verdicts

Each component gets one verdict, from the first matching row:

| Condition | Verdict |
|---|---|
| the file was created or recreated, or holds no stamp | rebuild |
| the component's row is new, its version changed, or `ready` is 0 | rebuild |
| the stamp's `crt` or `scm` differs from the collection's | rebuild (another collection) |
| the stamp's `col_mod` is after the collection's `col.mod` | rebuild (the collection went back: a restored file) |
| `clean` is 1 and `col_mod` differs | rebuild (the collection changed while Meguri did not have it open, R1453) |
| `clean` is 0 | catch-up from the stamp's `col_mod` (a crash between a collection commit and its derived update; also when `col_mod` is equal, since a `Raw` write does not move it) |
| otherwise | fresh: `Ready` |

The stamp then takes the collection's mark with `clean` 0 and the bind's seq, and every component that is not fresh gets `ready` 0 in the same transaction, so a crash during its catch-up or rebuild rebuilds it next time.

## D6. Incremental updates

- The engine's in-core observer (`engine/change-stream` S2) runs each component's capture on the collection writer thread and sends the commit (seq, generation, the `col` row after the op when the op changed it, the captured deltas) to the derived writer. Sending never blocks: the collection writer never waits for derived work.
- The derived writer is one thread with the only writable connection to the file. It applies commits in seq order, several queued commits per transaction. Commits of another generation, and commits at or before the bind's seq (already in the bind's mark), are dropped.
- Per component, by its state:
  - ready: the delta is applied in a savepoint. A failing or panicking apply rolls back that component's part, marks it `ready` 0 and queues its rebuild; the other components and the stamp go on.
  - refreshing (D7): the delta is kept with its seq, before the transaction begins, so a transaction that then fails loses nothing the refresh needs.
  - waiting for a refresh: the delta is dropped, because the refresh's snapshot will hold it.
  - a "rebuild" capture queues a rebuild of a ready component, and makes a waiting or running catch-up a rebuild: a catch-up may not see what such a commit changed (a sync brings rows whose modification times are before its `since`).
- A transaction that fails as a whole (the D12 fault point, an I/O error) marks every ready component `ready` 0 and queues its rebuild; the stamp keeps its mark.
- When `ready` 0 cannot be written (here or at an invalidation, D10), the file may still say `ready` 1 for a component that misses commits, and any later stamp would pass its rows as current (D8) or, marked clean, as fresh at the next open. The store then unbinds, as a crash would leave it: nothing more is written to the file (its stamp neither advances nor is marked clean; a file that says `clean` 1 gets `clean` 0, D11), any refresh stops, readers no longer attach it (D9) and every component is `Stale` (its `cause_seq` the failed transaction's last commit, none for an invalidation or a resume). The next open assesses the file (D5.2); a replacement first opens it again, then empties it (D10).
- The stamp's mark becomes the last `col` row in the transaction, and its `seq` the last commit of the batch, the dropped ones included.
- Normal syncs need nothing else: a `SyncIncoming` commit reaches components like any other (R1489).

## D7. Rebuild and catch-up

- At most one refresh runs at a time, on a builder thread. A queued refresh starts once no other refresh runs and, for a component with `deferred until` (D4), once its hold is released; a held refresh never delays another component's. It waits while the collection is in an exclusive phase, takes the maintenance reader (KTD3) and one snapshot (`engine/op-model` O6) for the whole refresh, and runs the component's build or catch-up on it. Each batch it writes goes to the derived writer, which runs it in its own transaction and goes on with other components' commits between batches: a rebuild never holds back the count caches or anyone else's updates. A batch that fails or panics is rolled back and fails the refresh, even when the build goes on.
- A rebuild writes into staging tables. When it ends, one transaction creates the indexes on the staging tables, drops the live tables, renames the staging tables to the live names, applies the deltas kept since the refresh started whose seq is after the snapshot's (those at or before it are in the snapshot), sets `ready` 1 and writes the stamp's `seq`. Readers see the old content or the new content, never a partial one. A component's files (D1) that the old record rows named and the new ones do not are deleted by the derived writer once no reader maps them; those still unnamed at the next bind go in D5.1's sweep.
- A catch-up writes into the live tables; its last transaction applies the kept deltas after the snapshot, sets `ready` 1 and writes the stamp's `seq`.
- Progress: completed and total rows as the component reports them, published at most every 100 ms (D8).
- A refresh stops at its next batch when the collection enters an exclusive phase (it is retried from the start once the phase ends), when the collection closes or is replaced, when the store is invalidated, suspended or closed. Its staging tables are dropped. More than 100,000 kept deltas also restart it, and a "rebuild" capture restarts it as a rebuild (D6).
- A refresh that fails (the component's build or catch-up returns an error or panics, one of its batches fails, or the last transaction fails, a kept delta's apply included) is retried once from the start; a second failure in a row leaves the component `Stale` until the next bind or invalidation.

## D8. Readiness

- Each component is `Ready`, `Building{completed, total}` (a rebuild or catch-up runs) or `Stale` (its content is not current: waiting for a refresh, held until its `deferred until` signal or a `request_refresh` (D4), failed, or not yet bound), for one generation.
- Every change, and progress at most every 100 ms, is published on the change stream as an `Extension` event (`engine/change-stream` S3) carrying the component name, the generation and the readiness. The envelope's `cause_seq` is the seq of the commit that made the component stale (its "rebuild" capture, its failed apply, or the last commit of a failed transaction), on that `Stale` and on every event of the refresh it queued, retries included. Events of a bind, a replacement or an invalidation carry none.
- `Ready` says a component follows the commits, not that it holds the latest: a commit reaches the derived writer after it is published (D6). Before reading a component's tables, a consumer asks `is_current(component, snapshot)` with its collection snapshot. It is true when the component is `ready` in the store as the snapshot reads it and that view holds exactly the commits the snapshot sees: the stamp's `seq` there equals the snapshot's seq, or it is before it, every op up to the snapshot's has run the observers (`engine/change-stream` S2) and the store holds every commit it was sent (a commit it was not sent, an empty change set included, changes nothing in it). A `seq` after the snapshot's is not current: SQLite fixes the snapshot's view of the store at its first read of `derived`, not when the snapshot began, so a commit the store applied in between is in that view and not in the collection's. The check reads the store through the snapshot, so the answer holds for every derived read of that snapshot, before or after the check; consumers ask before their first.
- After each transaction of commits, while a component is `Ready`, the derived writer publishes an `Extension` event `DerivedApplied` carrying the generation and the stamp's new `seq`, which is also its `cause_seq`. A consumer that found a component not current asks again with a later snapshot at the next `DerivedApplied` or `Ready`.
- Consumers read derived tables only while the component is current for their snapshot, and keep a correct path without it (deck counts and the review queue never wait for the search index).

## D9. Readers

- Each collection reader (`storage/open-and-connections` C6) attaches the file read-only (`mode=ro`), as schema `derived`, when it is leased for the generation the store is bound to and the store's file is open, and detaches it otherwise. A reader that attached an earlier file of the store attaches the current one. Consumers join collection tables with `derived.<component>__<table>`.
- A reader of a generation the store is not bound to has no `derived` schema, so it cannot read rows of an older generation.
- The collection writer never attaches the file.

## D10. Invalidation

- `CollectionReplaced` (`engine/lifecycle` L8; one-way sync, `.colpkg` import, restore, and a swap that failed after it changed the file): before the replacement returns, the derived writer stops any refresh, empties every component's tables, stamps the new collection's mark with `clean` 0 and binds the new generation. Every component then rebuilds (R1489).
- A swap that failed before changing the file moves no generation: the store stays bound and keeps its content, and the readers reopened after the swap attach it again (D9).
- Check Database (U128) and an external change (U32, R1453) invalidate the store: every component is marked `Stale` and rebuilt through staging, so readers keep the old content until each swap.
- A replacement while the store is not open (it could not be opened, or it unbound, D6) opens it for the new collection and empties it all the same: whatever the file's verdicts, every component rebuilds.
- A crash between a collection commit and its derived update is found at the next bind by D5.2 and repaired by catch-up or rebuild.

## D11. Suspend and close

- The derived writer is a `SuspendParticipant` (U32): `flush` returns once every commit sent before it is applied or kept; `suspend` stops a running refresh (it restarts on resume) and returns only once that refresh's builder has ended, so no derived thread holds a collection snapshot; meanwhile the writer goes on applying commits and refuses the stopped refresh's batches. It then runs each registered component's persist (D4), in registration order, after every commit already sent; a persist that fails gets `ready` 0 and a queued rebuild as `release` does (D4), committed before the next step, and its rebuild starts after `resume` (D7). It then writes `clean` 1 and checkpoints the file; `resume` writes `clean` 0. When a failed persist's `ready` 0 cannot be written, the store unbinds (D6) and writes no `clean` 1.
- When `resume` cannot write `clean` 0, the file would say clean while ops commit again without reaching it, and D5.2 would find its rows fresh at the next open (a `Raw` write, such as the rollover's unbury, does not move `col_mod`). `resume` then returns the error and the store unbinds (D6).
- A file the store lets go of while it says `clean` 1 (a failed resume, or an unbind or an unusable replacement while suspended) is kept until `clean` 0 is written into it, its mark and `seq` unchanged: the store tries at each later commit, `suspend`, `resume` and close, and a `resume` succeeds only once it is written. Nothing else is written into it; a bind or a replacement that opens the file again writes its own stamp.
- Close: the store is closed after the collection (U38: after `Engine::close`, so the ops the close drains reach the store). It stops any refresh, applies everything pending, runs each component's persist as `suspend` does, writes `clean` 1, checkpoints and closes the file. A store closed while ops still commit leaves `clean` 1 with an older `col_mod`, which D5.2 turns into a rebuild.

## D12. Threads and budgets

- `meguri-derived-writer` and `meguri-derived-builder`, both through the engine's thread registry at utility QoS (`engine/jobs` J6). The writer waits on its queue without a timeout when idle (R1854).
- The capture on the collection writer thread is the only derived work there; an answer's write phase stays under 5 ms at p95 while a component of 150,000 cards rebuilds (`engine/op-model` O10).
- Fault point `engine.derived.before_apply`: on the derived writer, before a batch of commits is applied (the crash between a collection commit and its derived update).

## D13. Tests

| What | Where |
|---|---|
| A missing, truncated, foreign or other-format file is recreated; a good one is opened | `tests/derived_store.rs` |
| Verdicts of D5.2 | `tests/derived_store.rs` |
| Staging swap: a reader holding a read transaction sees the old content, the next one the new; indexes and FTS5 tables survive the swap | `tests/derived_store.rs` |
| Readers join `derived.*` read-only; the collection's `sqlite_master` is unchanged; `VACUUM INTO` of the collection holds no derived table | `tests/derived_store.rs` |
| File deleted while closed → rebuilt with the same content | `tests/derived_lag.rs` |
| Process killed between a collection commit and its derived update → catch-up; content equals a full rebuild | `tests/derived_lag.rs` |
| `CollectionReplaced` → rebuilt for the new generation; no reader of the new generation sees old rows | `tests/derived_lag.rs` |
| A swap that failed after changing the file → rebuilt for the new generation; one that failed before leaves the store bound, its content kept and readers attached again | `tests/derived_lag.rs` |
| File truncated mid-page → recreated at bind and rebuilt | `tests/derived_lag.rs` |
| Rebuild over 150,000 cards while answers commit: answer write phase p95 under 5 ms; `Building` progress and `Ready` on the change stream | `tests/derived_lag.rs` |
| No derived activity changes the collection's `sqlite_master` | `tests/derived_lag.rs` |
| Components `foo` and `foo_`: emptying, rebuilding or dropping one leaves the other's live and staging tables | `tests/derived_store.rs` |
| A commit batch whose transaction fails while a rebuild runs still reaches the rebuilt tables | `tests/derived_lag.rs` |
| `suspend` during a build returns once the builder let go of its snapshot | `tests/derived_lag.rs` |
| A component with `deferred until`: its rebuild verdict at bind leaves it `Stale` with no refresh started until `signal` of that name; `request_refresh` starts it at once instead; a refresh queued after the release starts at once; another component's refresh is not held; a signal of another name changes nothing; the next bind holds it again | `tests/derived_lag.rs` |
| Persist at `suspend` and close runs after every commit sent before them and before `clean` 1: a commit applied just before is in the persisted state; a failing persist leaves `ready` 0, `clean` 1, and the next bind rebuilds the component | `tests/derived_lag.rs` |
| `release(component)` with pending in-memory state persists it before releasing, and a later read reloads the same content; a failed persist marks the component `ready` 0 and queues its rebuild; a component without the items is unchanged | `tests/derived_lag.rs` |
| A panicking apply, build batch or kept delta is rolled back, its refresh runs again and the writer goes on; a build that goes on after a failed batch is not made ready | `tests/derived_lag.rs` |
| Readiness changed by a commit carries that commit's seq as `cause_seq` | `tests/derived_lag.rs` |
| A commit whose apply is held: the component stays `Ready` but is not current for a snapshot that sees the commit; once applied, `DerivedApplied` names its seq, a new snapshot is current and one that read the store before is not; an op the store is not sent leaves it current | `tests/derived_lag.rs` |
| A commit applied after a snapshot began: a snapshot that read the store before is still current and its derived rows match its collection; one whose first read of `derived` comes after is not current | `tests/derived_lag.rs` |
| A snapshot reads the stamp's `seq` of a ready component from its own view of the store; writing the mark keeps the `seq` | `tests/derived_store.rs` |
| A "rebuild" capture while a catch-up waits or runs makes it a rebuild that holds the commit | `tests/derived_lag.rs` |
| A failed commit or an invalidation whose `ready` 0 cannot be written (another connection holds the file's write lock): the store unbinds; once the file can be written, a later commit leaves the component not current, readers without `derived`, the stamp not clean after close, and the next open repairs it | `tests/derived_lag.rs` |
| A replacement of an unbound store binds it again and rebuilds every component | `tests/derived_lag.rs` |
| A resume whose `clean` 0 cannot be written (another connection holds the file's write lock) fails and unbinds the store; after a `Raw` commit, the file says `clean` 0 once it can be written, at the next commit, `suspend`, `resume` or close, and the next open repairs the rows | `tests/derived_lag.rs` |
