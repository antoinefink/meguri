# Op model and executor

Spec: engine/op-model
Requirements: R1490, R1494, R1495, R1658, R1790, R1860, R2002, R2006, R727 (mechanism)
Anki: none (Meguri design: no Anki behavior; written by the implementer, U24 execution note)
Scenarios: none; tests in `core/crates/meguri-engine/tests/`

How every write to `collection.anki2` runs: one serialized executor per open collection, ops with a source, one transaction per op, a sequence number, cancellation, the plan/commit pattern and its write-phase rule, and declared long writes. The change set an op produces is `engine/change-stream`; which ops the executor admits in which collection state is `engine/lifecycle`; jobs, cancel tokens and progress are `engine/jobs`. Undo (U25) consumes the op journal described here.

## O1. Ops and sources

An op is a closure that writes through one write scope (`storage/ids-usn-mtime`, U23) plus its spec:

| Field | Meaning |
|---|---|
| name | a static name; the catalog (O9) names every op that may declare a long write |
| source | one of the seven below |
| stamping mode | default from the source (table below); an op may set another (`KeepMtime` for `.apkg` updates, `CallerSupplied` for undo restores, `Raw` for U40's rollover queue write) |
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
| `SyncIncoming` | background | `CallerSupplied` | a normal sync's merge commit (U107) |
| `Optimizer` | background | `Local` | the FSRS optimizer commit (U47) |
| `Rollover` | background | `Local` | day-rollover writes (U40) |
| `Maintenance` | background | `Local` | Check Database, Empty Cards and other maintenance (U128-U131) |

What each source means for undo (recorded, skip, clear) is U25's.

## O2. One writer, two queues

- One writer thread per open collection owns the writer connection (`storage/open-and-connections` C4). Nothing else writes the file.
- Submitted ops wait in one of two FIFO queues. The writer always takes the oldest interactive op first; it takes a background op or a maintenance task (O8) only when no interactive op is queued. It never preempts: an op on the writer runs to its end before the next one starts.
- Within a queue, ops commit in arrival order. Server writes and UI writes share the interactive queue, so they commit in the order they arrived (R1790): a `LocalApi` field update queued before a UI edit of the same note commits first.
- A submit returns at once with a pending result; the caller waits on it or not. Answers never wait for a sync: a normal sync plans off the writer and holds it only for its `SyncIncoming` commit (U107; R1495 with KTD6).

## O3. One transaction per op

- Every op is one `BEGIN IMMEDIATE` write scope with the journal on (R1490). It commits when the closure returns a value and rolls back when it returns an error, panics, or is cancelled (O5). After a rollback the collection is exactly as before the op (R1860, R2002). A panic is caught on the writer thread, which goes on with the next op; the op's result is `Panicked` with the panic message.
- A composite op runs several steps in its one transaction. A step runs inside a savepoint: when the step fails, its writes and its journal entries are rolled back to the savepoint and the op decides whether to go on. The op still has one journal, one seq and one change set, so it is one undo entry for U25 (U140's create-then-delete move).
- The journal keeps before-images in memory up to 8 MiB per op (U23's default threshold); further before-images spill to an unnamed temporary file in the collection's directory, removed when the journal drops.

## O4. Sequence numbers and the result

- Every op that commits takes the next value of the engine's sequence counter at its commit, under the commit lock (O6). The counter is the change stream's (`engine/change-stream` S3): op seqs and event seqs are one strictly increasing sequence, from 1, for the life of the engine. An op that changed nothing still takes a seq, but publishes no event.
- The result: `seq`; the change set (`engine/change-stream` S1), shared, empty when nothing changed; `undo`, the op's name and source when the op is `UserUndoable` and changed something (U25 adds the entry); `generation`; the op's own value; `past_cancel_point` (O5); timing (O10).
- An op that writes nothing gets an empty change set, `col.mod` stays unchanged (a scope without a change does not stamp it), and no event is published.

## O5. Cancellation

- An op's cancel token (`engine/jobs` J4) is checked when the writer takes the op: if cancel was requested while it waited, nothing runs and the result is `Cancelled`.
- The write phase starts when the writer takes the op. From then on the op completes: a cancel requested during the write phase is ignored and the result reports `past_cancel_point = true`. The progress sink switches to `Committing` at that moment, so the UI stops offering Cancel.
- Exception: an op whose catalog entry declares a cancellable write phase (O9; U107's `SyncIncoming` merge) checks its token between batches; a cancel there rolls the whole op back and the result is `Cancelled`.
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

## O9. Op catalog

The one list of ops that may declare a long write, and of maintenance tasks. Units add their entry here and in `executor/catalog.rs` together.

| Name | Unit | Long write | Write cancellable between batches |
|---|---|---|---|
| `check_database` (vacuum and repairs) | U128 | yes | no |
| `apkg_apply` (a large `.apkg` apply) | U120 | yes | no |
| `sync_incoming_merge` (a large `SyncIncoming` merge) | U107 | yes | yes |
| `notetype_structure_change` (on large note types) | U34 | yes | no |
| `bulk_op` (bulk ops over large selections) | U86 | yes | no |
| `vacuum` | U128, U132 | maintenance task | |
| `reindex` | U128 | maintenance task | |
| `analyze` | U132 | maintenance task | |
| `wal_checkpoint` | U109, U132 | maintenance task | |

U37 exports the catalog's names and undoability; U25 adds each op's undo class (R2020).

## O10. Timing and budgets

- Each op runs in a `tracing` span `op` with its name and source; at its end it records the rows the journal holds, the planning time (when it came through O6) and the write-phase time. The result carries the same numbers.
- Budgets, measured by the `executor` criterion suite (`core/crates/meguri-engine/benches/executor.rs`, U12 harness) on the M3 Max: an empty op's round trip (submit, writer, commit, result) under 50 µs at p50; an answer-sized write phase (one card update and one review-log insert, commit sync included) under 5 ms at p95. On Apple hosts the suite exits with failure when either is over. U241 keeps the baselines.
- Measured 2026-10-15 (`tools/mg bench-mac --bench executor`, M3 Max, bundled SQLite, the writer settings of `storage/open-and-connections` C9 then in force: `synchronous=FULL` with `fullfsync`): empty op p50 7.6 µs, p95 13–16 µs at 10k and 150k cards; answer write p50 4.0 ms, p95 5.07–5.09 ms, over the 5 ms budget. In Docker (`tools/mg bench`, Linux, the collection on tmpfs) the same answer takes p50 43–54 µs and p95 112–159 µs, so about 4 ms of each Mac answer is the commit's full sync. C9's switch rule (`synchronous=NORMAL` with U32's flush) decides the setting from the device numbers.
