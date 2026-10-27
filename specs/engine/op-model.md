# Op model and executor

Spec: engine/op-model
Requirements: R1490, R1494, R1495, R1658, R1790, R1860, R2002, R2006, R727 (mechanism); supports R2047, R2051 (O1.1)
Anki: none (Meguri design: no Anki behavior; written by the implementer, U24 execution note)
Scenarios: none; tests in `core/crates/meguri-engine/tests/`
Status: Approved 2026-10-27 (R2066) for O1.1, the `CloudIncoming` and `CloudRekey` rows of O1, the CloudKit wording of O2 and O5, and the `cloud_incoming_apply` row of O9, with `sync/policy`, `sync/cloudkit-records` and `sync/cloudkit-recovery`. Every other section is in force and unchanged.

How every write to `collection.anki2` runs: one serialized executor per open collection, ops with a source, one transaction per op, a sequence number, cancellation, the plan/commit pattern and its write-phase rule, and declared long writes. The change set an op produces is `engine/change-stream`; which ops the executor admits in which collection state is `engine/lifecycle`; jobs, cancel tokens and progress are `engine/jobs`. Undo (`engine/undo`, U25) consumes the op journal described here.

## O1. Ops and sources

An op is a closure that writes through one write scope (`storage/ids-usn-mtime`, U23) plus its spec:

| Field | Meaning |
|---|---|
| name | a static name; the catalog (O9) names every op that may declare a long write |
| source | one of the nine below |
| stamping mode | default from the source (table below); an op may set another (`KeepMtime` for `.apkg` updates, `CallerSupplied` for undo restores, `Raw` for the day clock's ops, U40, `sched/day-clock` D10) |
| preconditions | O7 |
| cancel token, progress sink | O5, `engine/jobs` |
| planned generation | the generation of the handle the op is submitted through (`engine/lifecycle` L3); none when the op read nothing before it was submitted and says so (`run_on_any_generation`) |
| privilege | set only by the lifecycle for the open steps and the exclusive job (`engine/lifecycle` L4) |
| when unavailable | refuse (default) or park (`engine/lifecycle` L4) |
| declared write | O8 |

| Source | Queue (O2) | Default stamping | Who |
|---|---|---|---|
| `UserUndoable` | interactive | `Local` | answers, edits, deck and note actions the user can undo |
| `UserNonUndoable` | interactive | `Local` | settings and other user writes that clear undo (U25) |
| `LocalApi` | interactive | `Local` | AnkiConnect and Migaku writes (R1658, R1789) |
| `SyncIncoming` | background | `CallerSupplied` | a normal sync's merge commit (U107). Manual Anki transfers only (frozen; KTD26) |
| `CloudIncoming` | background | `CallerSupplied` | a CloudKit apply of ready inbox entries and merge results (`sync/cloudkit-records#K13.2`, `#K12.3`), and the CloudKit barriers (O1.1) (U113, U114) |
| `CloudRekey` | background | `Local` | a CloudKit re-key of a colliding local object (`sync/cloudkit-records#K14.2`) (U113, U114) |
| `Optimizer` | background | `Local` | the FSRS optimizer commit (U47) |
| `Rollover` | background | `Local` | day-rollover writes (U40) |
| `Maintenance` | background | `Local` | Check Database, Empty Cards and other maintenance (U128-U131) |

What each source does to the undo history (recorded, cleared, truncated, the sync merge rule, the CloudKit targeted drop; skip-undo per op) is `engine/undo` M2.

### O1.1 CloudKit ops (KTD25)

Added for CloudKit sync. The rows above for the other sources do not change.

- **Remote-apply sources.** Two sources write data that came from another device: `SyncIncoming`, for manual Anki transfers only (frozen), and `CloudIncoming`, for CloudKit. They share no state. A CloudKit apply never uses `SyncIncoming`, and a manual transfer never uses `CloudIncoming`.
- **`CloudIncoming` stamping.** The op writes each changed or new row with the column values of the CloudKit payload, the modification time included, and `usn` -1. It sets `col.mod` to now when it changed a row, and leaves `col.usn` and `col.ls` as they are. It sets `col.scm` only when it applies a structural generation that changes local structure (`sync/cloudkit-records#K11.3`; the stamp rows of both CloudKit sources are `storage/ids-usn-mtime#S3`).
- **`CloudRekey` stamping.** `Local`: the rows it rewrites are local changes, sent onward like any other (`sync/cloudkit-records#K14.2`).
- **Intents.** Both ops record their intent in the tracking store before their `COMMIT` (`sync/cloudkit-records#K13.2`, `#K14.2`). A rollback (an error, a panic, or a cancel under O5's exception) leaves every key at its expected before digest, and the core then settles the intent at once (K13.2 "Failed applies"). After a cancel the entries stay ready and are applied again later. After any other rollback (an error, a constraint failure, a panic) the entries take the inbox reason `apply_failed` (failure `apply`, or `rekey` for a `CloudRekey`), and are applied again only at an event trigger, one entry per op (`sync/policy#P4` `ApplyReady{retryFailed}`), so a failing entry never loops on the writer. After COMMIT the core records the op's seq, taken from its result (O4), in the intent, also when the change set is empty (K13.2).
- **Targets and undo.** Both ops report their targets to the undo manager (`engine/undo#M9.8`). The result of a `CloudIncoming` or `CloudRekey` op also carries the held run that the drop removed, if any (`engine/undo#M9.8`).
- **Barriers.** The CloudKit barrier (`sync/policy#P7.1`) and the Sync Now capture (`sync/policy#P12.2`) are ops of source `CloudIncoming` that write no row: plain ops, with no catalog entry. Their source effect (`engine/undo` M2) is none, because they change nothing, and they publish no event (O4). The barrier's only undo change is what its call of `begin_cloud_hold` (`engine/undo#M9.6`) through its op context does: flags and redo drops. The Sync Now capture records only its seq and makes no such call. The barrier receives the cloud reader's lease from its caller and never waits for one on the writer (`engine/lifecycle#L11`). A barrier never waits for the WAL flush; the batch builder waits for it off the writer (`sync/policy#P7.1`).
- **Admission.** CloudKit ops run only in `Ready` (`engine/lifecycle#L4`); in any other state they are refused, never parked. The policy submits none while its gate is closed (`sync/policy#P6`).
- The CloudKit network work is not an op and not a job: the Swift actor owns it (`engine/jobs#J2.1`).

## O2. One writer, two queues

- One writer thread per open collection owns the writer connection (`storage/open-and-connections` C4). Nothing else writes the file.
- Submitted ops wait in one of two FIFO queues. The writer always takes the oldest interactive op first; it takes a background op or a maintenance task (O8) only when no interactive op is queued. It never preempts: an op on the writer runs to its end before the next one starts.
- Within a queue, ops commit in arrival order. Server writes and UI writes share the interactive queue, so they commit in the order they arrived (R1790): a `LocalApi` field update queued before a UI edit of the same note commits first.
- A submit returns at once with a pending result; the caller waits on it or not. Answers never wait for a sync: a normal sync plans off the writer and holds it only for its `SyncIncoming` commit (U107; R1495 with KTD6). CloudKit holds the writer only for its barriers, which write nothing, and for its `CloudIncoming` and `CloudRekey` commits (O1.1); it builds records off the writer (`sync/policy#P7`, R2006).
- Between ops the writer runs the WAL flush (`storage/open-and-connections` C9.2): 750 ms after the first commit that no full sync covers yet, whatever made that commit, it syncs the `-wal`. A due flush runs before the next queued op, so queues never push it past its 1 s bound, and an op that may hold the writer for long (a declared write or a maintenance task, O8) starts only after the pending flush. A failed sync covers nothing and is retried after 50 ms, doubling up to 1 s. With nothing to cover the writer sets no timer (R1854).

## O3. One transaction per op

- Every op is one `BEGIN IMMEDIATE` write scope with the journal on (R1490). It commits when the closure returns a value and rolls back when it returns an error, panics, or is cancelled (O5). After a rollback the collection is exactly as before the op (R1860, R2002). A panic is caught on the writer thread, which goes on with the next op; the op's result is `Panicked` with the panic message.
- A composite op runs several steps in its one transaction. A step runs inside a savepoint: when the step fails, its writes and its journal entries are rolled back to the savepoint, together with what the step noted for its op (media files, the restorer state of `engine/undo` M10, a sync merge report of M9.4), and the op decides whether to go on. The op still has one journal, one seq and one change set, so it is one undo entry for U25 (U140's create-then-delete move).
- The journal keeps before-images in memory up to 8 MiB per op (U23's default threshold); further before-images spill to an unnamed temporary file in the collection's directory, removed when the journal drops.

## O4. Sequence numbers and the result

An error after SQLite's successful `COMMIT` cannot roll the operation back.
The executor still applies its history and publishes its changes. It reports
`CommitCompleted` with the committed sequence, or `MediaCommitPending` when
staged attachment publication needs recovery. Callers refresh committed state
and recover attachments as needed; they never repeat the note operation.

- Every op that commits takes the next value of the engine's sequence counter at its commit, under the commit lock (O6). The counter is the change stream's (`engine/change-stream` S3): op seqs and event seqs are one strictly increasing sequence, from 1, for the life of the engine. An op that changed nothing still takes a seq, but publishes no event.
- The result: `seq`; the change set (`engine/change-stream` S1), shared, empty when nothing changed; `undo`, the name and source of the undo step the op recorded (`engine/undo` M1: a `UserUndoable` op that changed something, or a redo; the step's id is the op's seq), applied on the writer before the result is returned; `generation`; the op's own value; `past_cancel_point` (O5); timing (O10).
- An op that writes nothing gets an empty change set, `col.mod` stays unchanged (a scope without a change does not stamp it), and no event is published.

## O5. Cancellation

- An op's cancel token (`engine/jobs` J4) is checked when the writer takes the op: if cancel was requested while it waited, nothing runs and the result is `Cancelled`.
- The write phase starts when the writer takes the op. From then on the op completes: a cancel requested during the write phase is ignored and the result reports `past_cancel_point = true`. The progress sink switches to `Committing` at that moment, so the UI stops offering Cancel.
- Exception: an op whose catalog entry declares a cancellable write phase (O9; U107's `SyncIncoming` merge, a `CloudIncoming` apply submitted as `cloud_incoming_apply`) checks its token between batches; a cancel there rolls the whole op back and the result is `Cancelled`.
- Planning (O6) runs on the caller's thread and checks the token itself; a cancel during planning ends the op with `Cancelled` before anything is submitted.

## O6. Plan and commit

Heavy ops compute on a reader snapshot, off the writer, and then commit in one short write phase.

- A snapshot is a read transaction on one reader (`engine/lifecycle` L11) together with the seq of the last commit it sees. The commit lock makes that seq exact: the writer holds it from `COMMIT` until the op's seq is assigned, and a snapshot is begun (its first read made) while holding it.
- Planning is cancellable and reports progress in the `Planning` phase. It produces the planned writes and the preconditions (O7) that make them safe.
- The write phase is submitted as an ordinary op. Ops without a declared write (O8) aim at a write phase of at most about 200 ms, so answers stay durable within 1 s (R2006).
- On a `Stale` precondition the op is planned again on a new snapshot, at most 3 more times; after that the result is `Stale`.

## O7. The write-phase rule

A write phase never writes a value computed on the snapshot to a row that may have changed since. Each row it writes is either:

1. **re-derived** inside the write transaction: the op reads the live row and applies its transformation again; or
2. **covered by a precondition**, checked by the executor inside the write transaction before the op's closure runs. A failing precondition rolls back, writes nothing, and the result is `Stale`.

The precondition kinds:

| Kind | Holds when |
|---|---|
| `RowDigest{table, key, digest}` | the stored row's SHA-256 over the journal's image encoding (`journal::digest`, U23) equals `digest`; a digest of "absent" holds when no row has the key. The answer's `stateToken` is such a digest of the card row (R727; U43, U44) |
| `Untouched{table, key, since}` | no op with a seq above `since` changed the row (the touch index below) |
| `TableUntouched{table, since}` | no op with a seq above `since` changed any row of the table |

- Never mtime and `usn` alone: `Local` stamping has one-second resolution, so two edits in one second leave them equal.
- The precondition or re-derivation covers every row a planned decision was computed from, not only the rows written: a deletion decided by a note's content covers that note and its note type.
- An op whose target set is a predicate (all notes of a note type, all cards of a template) either selects the set inside the write transaction, or carries a set precondition: `TableUntouched` on the tables the predicate reads, and replans on mismatch. Rows added after the snapshot are then covered.
- The touch index: after each commit the writer records, for every key in the op's journal, that op's seq (keys compare as their table compares them: `tags` by `unicase`), and per table the last seq that changed it. It holds at most 1,000,000 keys; when a commit would pass that, the index is cleared and its floor becomes that commit's seq. `Untouched` and `TableUntouched` with `since` below the floor fail (the proof is gone), which makes the op replan.

## O8. Declared long writes and maintenance tasks

- An op whose write phase cannot stay near 200 ms declares its expected length. Only catalog ops may (O9); a declaration by any other op is refused with `InvalidArgument`.
- When the writer starts a declared write, it publishes `WriteDelay{op, declared}` on the change stream and keeps it readable from the collection handle until the write ends, then publishes `WriteDelayEnded{op}`, after the op's own change set: once U43 sees the delay end, the commit is visible. U43 reads it: answers accepted meanwhile stay in the session's overlay and pending-answer journal and commit as soon as the writer is free (KTD6). Every other write waits for the writer.
- A maintenance task is work that must run on the writer outside any transaction: `VACUUM`, `REINDEX`, `ANALYZE`, `wal_checkpoint`. It is queued as a background item with a declared duration, gets the writer connection itself, and publishes its delay like a declared write. The writer never starts one while an interactive op is queued (O2). During `ExclusiveBusy` it is accepted only from the exclusive job. Users: U128, U132, U109.
- `damaged_backup` is T7's Ready-state fallback after a snapshot fails with a corruption error (`package/backup-triggers#T7`). It uses the same declared-delay maintenance window so no collection commit or checkpoint runs during the main/WAL byte copy. The task runs no SQLite check, repair or collection write and neither records nor clears undo. T7 still defines the copy, publication, cleanup and other lifecycle states.

## O9. Op catalog

The list of named engine ops with their undo class (`engine/undo` M11, R2020), the one list of ops that may declare a long write, and the list of maintenance tasks. Units add their entry here and in `executor/catalog.rs` together. An op that is not listed is a plain op: it may not declare a long write. The writer syncs a pending WAL flush (`storage/open-and-connections` C9.2) before any op that may declare a long write, so an op that is short gets an entry that may not.

| Name | Unit | Undo class | Long write | Write cancellable between batches |
|---|---|---|---|---|
| `check_database` (vacuum and repairs) | U128 | destructive, not undoable | yes | no |
| `apkg_apply` (a large `.apkg` apply) | U120 | undoable | yes | no |
| `sync_incoming_merge` (a large `SyncIncoming` merge; manual Anki transfers, frozen) | U107 | not destructive | yes | yes |
| `cloud_incoming_apply` (a `CloudIncoming` apply of a complete structural generation with more than 500 entries, which is applied in one op, O8; any other incoming apply is a series of plain `CloudIncoming` ops of at most 500 entries each, `sync/cloudkit-records#K13.2`; O1.1) | U114 | not destructive | yes | yes |
| `notetype_structure_change` (on large note types) | U34 | undoable | yes | no |
| `memory_recompute` (`sched/memory-state` M4) | U46 | undoable | yes | yes |
| `enable_fsrs` (`sched/fsrs-policy` E1-E4) | U50 | not destructive, not undoable | yes | yes |
| `bulk_op` (bulk ops over large selections) | U86 | undoable | yes | no |
| `undo` (`engine/undo` M4) | U25 | undoable | no | |
| `redo` (`engine/undo` M5) | U25 | undoable | no | |
| `undo_long_write`, `redo_long_write` (undo and redo of a step whose op declared a long write) | U25 | undoable | yes | no |
| `vacuum` | U128, U132 | not destructive | maintenance task | |
| `reindex` | U128 | not destructive | maintenance task | |
| `analyze` | U132 | not destructive | maintenance task | |
| `wal_checkpoint` | U109, U132 | not destructive | maintenance task | |
| `damaged_backup` (T7 main/WAL byte copy) | U127 | not destructive | maintenance task | |

A destructive op that is not undoable is confirmed first and preceded by U127's pre-op backup. U37 exports the catalog's names and undo classes.

`memory_recompute` checks cancellation between batches inside its one transaction; cancellation rolls back the entire operation. Its caller supplies `UserUndoable` and the undo name for a user action, or `Optimizer` or `Maintenance` for those callers (`sched/memory-state` M4.3). It uses its own catalog entry, since `bulk_op` does not permit cancellation during writes.

`enable_fsrs` uses `UserNonUndoable` and owns one transaction containing the global flag and all recomputed card writes. Cancellation between batches rolls back both; only a successful commit clears undo. It does not inherit `memory_recompute`'s undo class or commit that work as a second operation (`sched/fsrs-policy` E4).

## O10. Timing and budgets

- Each op runs in a `tracing` span `op` with its name and source; at its end it records the rows the journal holds, the planning time (when it came through O6) and the write-phase time. The result carries the same numbers.
- Budgets, measured by the `executor` criterion suite (`core/crates/meguri-engine/benches/executor.rs`, U12 harness) on the M3 Max: an empty op's round trip (submit, writer, commit, result) under 50 µs at p50; an answer-sized write phase (one card update and one review-log insert, commit sync included) under 5 ms at p95. On Apple hosts the suite exits with failure when either is over. U241 keeps the baselines.
- Measured 2026-10-15 (`tools/mg bench-mac --bench executor`, M3 Max, bundled SQLite 3.53.2), two runs per writer setting at 10k and 150k cards:
  - With `synchronous=FULL` and `fullfsync` (`storage/open-and-connections` C9 before its switch): empty op p50 7.5–16 µs, p95 13–33 µs; answer write p50 4.0 ms, p95 4.97–5.09 ms, over the budget in three of four measurements. In Docker (`tools/mg bench`, Linux, the collection on tmpfs) the same answer takes p50 43–54 µs and p95 112–159 µs, so about 4 ms of each Mac answer was the commit's full sync. These numbers decided C9's switch.
  - With C9's setting (`synchronous=NORMAL`, full syncs on Apple; the first run also at 100k and 500k): empty op p50 7.5–16 µs; answer write p50 76–85 µs, p95 0.20–0.41 ms from 100k to 500k and 1.45–1.55 ms at 10k, p99 4.3–7.9 ms, maximum 38–164 ms. The commits that run the autocheckpoint or start the log again still pay full syncs (C9.1); they make the p99 and the maximum. Both runs had the host at a load average near 30, which also put one empty-op p95 (10k, second run) at 1.2 ms.
- Measured 2026-10-16 with the writer's flush (O2, `storage/open-and-connections` C9.2) and the same setting, two runs at 10k and 150k cards (the first is in C9.4): answer write p50 80–205 µs, p95 0.16–0.84 ms, p99 4.9–11.5 ms, maximum 29–103 ms. The second run's empty op: p50 7.8 and 12 µs, p95 14 and 42 µs. The host's load average was 28–52. Both budgets hold; the flush adds no time to a write phase, and an op that arrives while it runs waits for its one full sync, which shows only in the slowest samples.
