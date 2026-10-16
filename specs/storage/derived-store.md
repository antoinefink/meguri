# The derived store

Spec: storage/derived-store
Requirements: R1487, R1488, R1489, R2016; supports R1453 (with U32), R1819
Anki: none (Meguri design: no Anki behavior; written by the implementer, U26 execution note)
Scenarios: none; tests in `core/crates/meguri-storage/tests/derived_store.rs` and `core/crates/meguri-engine/tests/derived_lag.rs`

Meguri's own indexes (count caches, folded text and full-text search, browser caches, daily aggregates, stats column caches, the media reference index) live in one SQLite file next to the collection, `meguri-derived.db`. Everything in it can be deleted at any time and rebuilt from the collection and the media folder. This spec covers the file, its stamp, how components register, how the store stays current (incrementally from committed ops, by catch-up, by rebuild), what readers see, and when the store is invalidated. The storage primitives are `meguri-storage/src/derived/`; the engine side (threads, observer, readiness, generations) is `meguri-engine/src/derived_host.rs`.

## D1. Placement

- The file is `meguri-derived.db` in the collection's directory, with SQLite's `-wal` and `-shm` files next to it while open.
- Nothing derived is ever written to `collection.anki2` (R2016): no table, index, view, trigger or temporary object, and no `ATTACH` on the collection writer. The collection's `sqlite_master` is the same before and after any derived activity.
- The file is never synced, backed up or exported: sync, backups (`VACUUM INTO` of the collection's `main` schema), `.colpkg` and `.apkg` exports read the collection and the media folder only, and the file is not in the media folder.
- Deleting the file while the collection is closed loses nothing: the next open rebuilds every component (R1488).

## D2. File format

- WAL journal mode, `synchronous=NORMAL` (losing the last transactions in a power cut makes the store lag, which D5 detects), `page_size` 4096 for new files, `cache_size` -8192 (8 MiB), `temp_store` `MEMORY`, `busy_timeout` 1000 ms. The `unicase` collation is registered (`storage/open-and-connections` C5), so derived tables may use it.
- `PRAGMA application_id` is `0x4D475244` ("MGRD"); `PRAGMA user_version` is the store format, 1. A file whose application id is another one and that holds tables, or whose format differs, is not this store's and is replaced (D5.1).
- Two tables of the store's own, created when the file is created:

  | Table | Columns |
  |---|---|
  | `meguri_stamp` | `id` (always 1), `crt`, `scm`, `col_mod`, `clean` |
  | `meguri_components` | `name` (key), `version`, `ready`, `build` |

- Every other table belongs to one component (D4). Its stored name is `<component>__<table>`; an index's is `<component>__<index>__<build>`. Tables named `meguri_build__<component>__<table>` exist only while a rebuild runs (D7).

## D3. The stamp

- `crt`, `scm` (seconds and milliseconds as the `col` row stores them) and `col_mod` are the collection mark: which collection the store was derived from and the `col.mod` of the last commit whose changes every ready component holds.
- `clean` is 1 only when the store was closed or suspended (D11) after applying every commit it had been told about; it is set to 0 when the store is bound to an open collection (D5) and when it resumes.
- Per component: `version` (the component's own schema version), `ready` (1 when its tables hold the content for the stamp's mark) and `build` (a counter that names its indexes).
- In memory only: the collection generation the store is bound to (`engine/lifecycle` L3) and the seq of the bind (`engine/op-model` O4). Generations and seqs restart with every process, so they are never stored.
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

- Components never name stored table names: the store resolves a table name for the target being written (the live tables, or the staging tables of a rebuild).
- Owners: U41 count caches, U80/U90 folded text and full-text search, U84 browser caches, U95 daily aggregates, U98 stats column caches, U130 media reference index.
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

The stamp then takes the collection's mark with `clean` 0, and every component that is not fresh gets `ready` 0 in the same transaction, so a crash during its catch-up or rebuild rebuilds it next time.

## D6. Incremental updates

- The engine's in-core observer (`engine/change-stream` S2) runs each component's capture on the collection writer thread and sends the commit (seq, generation, the `col` row after the op when the op changed it, the captured deltas) to the derived writer. Sending never blocks: the collection writer never waits for derived work.
- The derived writer is one thread with the only writable connection to the file. It applies commits in seq order, several queued commits per transaction. Commits of another generation, and commits at or before the bind's seq (already in the bind's mark), are dropped.
- Per component, by its state:
  - ready: the delta is applied in a savepoint. A failing apply rolls back that component's part, marks it `ready` 0 and queues its rebuild; the other components and the stamp go on.
  - refreshing (D7): the delta is kept with its seq.
  - waiting for a refresh: the delta is dropped, because the refresh's snapshot will hold it.
  - a "rebuild" capture queues a rebuild of that component.
- The stamp's mark becomes the last `col` row in the transaction.
- Normal syncs need nothing else: a `SyncIncoming` commit reaches components like any other (R1489).

## D7. Rebuild and catch-up

- At most one refresh runs at a time, on a builder thread. It waits while the collection is in an exclusive phase, takes the maintenance reader (KTD3) and one snapshot (`engine/op-model` O6) for the whole refresh, and runs the component's build or catch-up on it. Each batch it writes goes to the derived writer, which runs it in its own transaction and goes on with other components' commits between batches: a rebuild never holds back the count caches or anyone else's updates.
- A rebuild writes into staging tables. When it ends, one transaction creates the indexes on the staging tables, drops the live tables, renames the staging tables to the live names, applies the deltas kept since the refresh started whose seq is after the snapshot's (those at or before it are in the snapshot), and sets `ready` 1. Readers see the old content or the new content, never a partial one.
- A catch-up writes into the live tables; its last transaction applies the kept deltas after the snapshot and sets `ready` 1.
- Progress: completed and total rows as the component reports them, published at most every 100 ms (D8).
- A refresh stops at its next batch when the collection enters an exclusive phase (it is retried from the start once the phase ends), when the collection closes or is replaced, when the store is invalidated, suspended or closed. Its staging tables are dropped. More than 100,000 kept deltas also restart it.
- A refresh that fails (the component's build or catch-up returns an error, or the last transaction fails) is retried once from the start; a second failure in a row leaves the component `Stale` until the next bind or invalidation.

## D8. Readiness

- Each component is `Ready`, `Building{completed, total}` (a rebuild or catch-up runs) or `Stale` (its content is not current: waiting for a refresh, failed, or not yet bound), for one generation.
- Every change, and progress at most every 100 ms, is published on the change stream as an `Extension` event (`engine/change-stream` S3) carrying the component name, the generation and the readiness.
- Consumers read derived tables only while the component is `Ready` for their generation, and keep a correct path without it (deck counts and the review queue never wait for the search index).

## D9. Readers

- Each collection reader (`storage/open-and-connections` C6) attaches the file read-only (`mode=ro`), as schema `derived`, when it is leased for the generation the store is bound to and the store's file is open, and detaches it otherwise. A reader that attached an earlier file of the store attaches the current one. Consumers join collection tables with `derived.<component>__<table>`.
- A reader of a generation the store is not bound to has no `derived` schema, so it cannot read rows of an older generation.
- The collection writer never attaches the file.

## D10. Invalidation

- `CollectionReplaced` (`engine/lifecycle` L8; one-way sync, `.colpkg` import, restore): before the replacement returns, the derived writer stops any refresh, empties every component's tables, stamps the new collection's mark with `clean` 0 and binds the new generation. Every component then rebuilds (R1489).
- Check Database (U128) and an external change (U32, R1453) invalidate the store: every component is marked `Stale` and rebuilt through staging, so readers keep the old content until each swap.
- A crash between a collection commit and its derived update is found at the next bind by D5.2 and repaired by catch-up or rebuild.

## D11. Suspend and close

- The derived writer is a `SuspendParticipant` (U32): `flush` returns once every commit sent before it is applied or kept; `suspend` stops a running refresh (it restarts on resume), applies everything pending, writes `clean` 1 and checkpoints the file; `resume` writes `clean` 0.
- Close: the store is closed after the collection (U38: after `Engine::close`, so the ops the close drains reach the store). It stops any refresh, applies everything pending, writes `clean` 1, checkpoints and closes the file. A store closed while ops still commit leaves `clean` 1 with an older `col_mod`, which D5.2 turns into a rebuild.

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
| File truncated mid-page → recreated at bind and rebuilt | `tests/derived_lag.rs` |
| Rebuild over 150,000 cards while answers commit: answer write phase p95 under 5 ms; `Building` progress and `Ready` on the change stream | `tests/derived_lag.rs` |
| No derived activity changes the collection's `sqlite_master` | `tests/derived_lag.rs` |
