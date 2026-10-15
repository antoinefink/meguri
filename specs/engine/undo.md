# Undo and redo

Spec: engine/undo
Requirements: R719, R720, R721, R725 (as KTD7 reconciles it), R1795, R1796, R2020; supports R365, R366, R370, R371, R723, R726, R1811
Anki: none for sections M1 to M12 (Meguri design from KTD5, KTD6, KTD7 and spec-flow C3, Q1, Q4, Q11; written by the implementer, U25). The observable results of undoing each op are Anki's (26.09.3) and come from the spec writer: pending, see the last section.
Scenarios: `conformance/scenarios/undo/` (pending, spec writer); mechanism tests in `core/crates/meguri-engine/tests/undo_basic.rs`, `undo_truncation.rs`, `undo_holdback_props.rs`, `op_catalog.rs`

The undo manager of one open collection (C3): the steps it records from committed ops, what every other op does to them, undo and redo as ops, the reasons a step is unavailable, the held-back run a normal sync keeps local, and the rule that no uploaded step is ever undone. Ops, sources and journals are `engine/op-model`; the change set is `engine/change-stream`.

## M1. Steps

- A step is recorded from every committed op whose source is `UserUndoable` and whose journal (U23) holds at least one change, unless the op is an undo or a redo (M4, M5). An op that changed nothing records nothing and leaves redo alone.
- A step holds: its id, which is the seq of the op that made it (`engine/op-model` O4); its name; its kind; its session; the op's journal (every changed row's before-image, and its after-image, a digest for notes); the notes it touched (the notes it changed and the notes of the cards it changed, from its change set); `col.mod` before and after the op; the write phase the op declared, if any (O8); its upload state (M9); and an optional restorer state the op attached (M10).
- The name is the step name the op gives (Anki's op names: "Answer Card", "Bury", "Suspend", R719, R370); without one it is the op's name. The kind is `Answer` for an answer and `Other` for every other step; only the held-back run (M8) reads it. The session is the review session the op names, if any.
- The touched set of a step is the keys of its journal, the notes it touched, and the graves of the cards, notes and decks it removed (M4.3).
- At most 30 steps are kept (R719). A 31st drops the oldest. Steps live in memory only: before-images past the journal's spill threshold live in its unnamed temporary file, which the system deletes when the journal drops, so nothing survives a quit and nothing needs a sweep at launch. Closing the collection drops every step.

## M2. What each op does to the history

Each op source has one undo effect, applied on the writer right after the op commits and before its result is returned, so a caller that waited for an op sees the history it left.

| Source | Effect when the op changed something |
|---|---|
| `UserUndoable` | recorded as a new step (M1); redo is cleared |
| `UserNonUndoable` | clear all, reason `ClearedByNonUndoableOp` (R366, R725); an op marked skip-undo instead changes nothing in the history (below) |
| `LocalApi` | never recorded; truncates at the deepest intersecting step (M7, R1795, R1796) |
| `SyncIncoming` | the merge rule of M9.4 (KTD7) |
| `Optimizer`, `Maintenance` | clear all, reason `ClearedByNonUndoableOp` |
| `Rollover` | clear all, reason `ClearedByRollover` (R462) |

- Clear all drops every step and every redo step, and sets the reason peek reports once the history is empty (M3).
- Skip-undo: an op of source `UserNonUndoable` may be marked skip-undo. It is neither recorded nor clears anything: selecting a deck (`curDeck`, `activeDecks`; U28's op and U43's session start) keeps the answer before it undoable (R42). Any other source marked skip-undo is refused with `InvalidArgument` at submit.
- An op that changed nothing has no effect, whatever its source.

## M3. Peek and scopes

- `peek(scope)` and `peek_redo(scope)` report either an available step (id, name, kind, session, declared write) or a reason. They read no rows.
- Scope `Global` looks at the top step. Scope `Session(id)` looks at the top step only when it belongs to that session; when the top step belongs to another session or to none, the answer is `None`.
- Reasons:

  | Reason | When |
  |---|---|
  | `None` | nothing to undo (session start, everything undone, or a session scope whose top step is not its own) |
  | `ClearedByRollover` | the history was cleared by day rollover |
  | `ClearedBySync` | a normal sync's incoming changes dropped the steps below |
  | `ClearedByOneWay` | the collection was replaced: one-way sync, restore or `.colpkg` import |
  | `ClearedByNonUndoableOp` | a non-undoable user op, the optimizer, maintenance or Check Database cleared it |
  | `ChangedExternally` | a local-server write or an outside change truncated the history, or an undo found its rows changed (M4.1) |
  | `AlreadyUploaded` | the top step was uploaded by a sync (M9) |
  | `UploadPending` | the top step is in the snapshot of a running sync (M9) |

- On an empty history peek reports the reason the oldest step was last dropped with: clear all, a truncation, a sync drop, or a failed undo. Dropping the oldest step to keep 30 resets it to `None`. A redo peek reports only `None` when nothing can be redone.

## M4. Undo

Undo is an op (catalog `undo`, or `undo_long_write` for a step whose op declared a long write; `engine/op-model` O9) of source `UserUndoable`, written in the `CallerSupplied` stamping mode, so every restored row keeps its own `usn` and modification time (`storage/ids-usn-mtime` S3). Inside its transaction, on the writer:

1. **Target.** The top step for the scope (M3). None: refused with `UndoUnavailable` and the reason peek reports (`None`, or why the history was emptied). Upload state `Uploaded` or `Pending`: refused with `AlreadyUploaded` or `UploadPending` (M9); the core refuses whatever the caller. A refusal writes nothing.
2. **Check.** Every row of the step's journal must still equal the step's after-image: the same digest (`storage` journal digest), or absent where the step removed it. For the `col` row only the columns the step changed other than `mod` are compared. If any row differs, the step and every older step are dropped, the reason becomes `ChangedExternally`, and the undo is refused with that reason, writing nothing.
3. **Restore.** Every journal row gets its before-image back: rows the step inserted are deleted, rows it deleted are inserted again (with the graves it added removed, since those are journal rows too), changed rows are written back whole. Rows of `decks`, `notetypes`, `fields` and `templates` the step changed are deleted first and inserted after, so names moving between rows never meet in their unique indexes. Graves: when the step removed a card, note or deck without adding its grave (the grave existed already, `storage/graves` G2), the undo also removes that grave if it is there, as Anki does (`storage/graves` G6); its redo puts it back.
4. **`col`.** The columns other than `mod` that the step changed get their before values; `mod` gets the value it had before the step (`storage/ids-usn-mtime` S5), never now.
5. **Commit.** The undo op's own journal becomes a redo step with the undone step's name, kind, session, declared write and restorer state; its id is the undo op's seq. Its `col.mod` values are the step's, swapped, so redo sets `mod` back to the value the step set.

The result's value names the undone step; its change set has source `UserUndoable`; `undo` (the recorded-step metadata of O4) is empty.

A step whose op declared a long write (a job-run op, KTD9: bulk ops, large structure changes, an `.apkg` apply) reports that write in peek; its undo and redo declare the same write (O8), so callers start them on the maintenance lane and never block the interactive lane.

## M5. Redo

Redo is an op (catalog `redo`, or `redo_long_write`) like undo, on the top redo step: refused with `None` when there is none; the same check (M4.2: here a failure drops that redo step and every redo step below it) and restore (M4.3, M4.4). Its journal becomes a new step at the top of the history, with the redo step's name, kind, session, declared write and restorer state, and `col.mod` values swapped back; the other redo steps stay. Any newly recorded step clears redo (R720).

## M6. Clearing without an op

- `clear(cause)` drops every step and redo step at once: `Rollover` (U40, when the day changes even if it writes nothing; `ClearedByRollover`), `CheckDatabase` (U128; `ClearedByNonUndoableOp`), `ExternalChange` (U32, when another process changed the file; `ChangedExternally`).
- A collection replacement (`engine/lifecycle` L8) clears everything with `ClearedByOneWay`, on the writer, right after the swap reopened the file and before any op of the new generation runs.
- Close drops everything.

## M7. Local-server writes (R1795, R1796)

- A `LocalApi` op is never recorded. When it changed something, each step is checked for intersection with it, newest first: a step intersects when the op changed a row whose key is in the step's journal, or changed a note the step touched (M1: so an answer on a card of a note the API edits intersects, AE80).
- The history is cut below the newest intersecting step: that step and every older one are dropped, the newer ones stay. When several steps intersect, the cut is at the newest of them, so no kept step would restore a row the write changed, and no kept answer is on a card of a note it edited (AE80). With one intersecting step this is R1796's "deepest such entry". The reason becomes `ChangedExternally`, which ⌘Z reports once the kept steps are undone.
- Redo steps are truncated the same way: the intersecting redo step nearest the top and every one below it go.

## M8. The held-back run (KTD6, spec-flow Q11)

- `held_back_run(session)` is the contiguous run of steps at the top of the history that belong to that session and are neither uploaded nor upload-pending, down to and including the newest answer among them. It is empty when there is no session, when the top step is not that session's, is uploaded or pending, or when no such answer is reached before the run ends.
- Its value: the steps (ids, names, kinds) and the rows they touched, each with its before-image from the oldest step of the run that touched it (absent when the run created it). The `col` row is not in it.
- The session for hold-back ends when U43 says so (the summary or congrats screen is dismissed, KTD6); U43 then stops passing it.

## M9. Sync

1. **Begin.** At its snapshot barrier (U107: an op on the writer between ops, writing nothing), a normal sync calls `begin_sync_hold(session)` through its op context. It returns the snapshot seq (the last committed op's) and the held-back run (M8), and flags every other step up to the snapshot that is not yet uploaded `UploadPending`. Only the writer can call it, so no undo runs between the snapshot and the flags. A sync that begins while an earlier one still has pending steps marks those uploaded first, since its `finish` may have been sent.
2. **`finish` sent.** Once the `finish` request has been sent, whatever its outcome, the pending steps become `Uploaded` and stay so (R1520). Redo steps made up to the snapshot are dropped; steps and redo steps made after it are left alone.
3. **Failure before `finish`.** Pending steps become undoable again.
4. **Merge.** A `SyncIncoming` op may report its merge: the snapshot seq and the staged incoming targets (row keys and graves, a grave given as the key of the card, note or deck it deletes), whether the merge applied them or not. Without a report the targets are the keys its journal changed (graves mapped the same way, `col` left out) and the snapshot is the running sync's, or none. When there are targets:
   - every step up to the snapshot is dropped, except the held-back run captured at begin;
   - the held-back run is dropped whole when any target is in the touched set of one of its steps;
   - a step made after the snapshot is dropped alone when a target is in its own touched set;
   - redo steps made up to the snapshot are dropped, and later ones that a target touches;
   - when the oldest step was dropped, the reason becomes `ClearedBySync`.
   Matching is by key (a target's note does not reach the note's cards).
5. An uploaded step is never undone (M4.1) and the held-back run is never flagged, so a revlog row the server holds is never deleted by an undo (spec-flow C3).

## M10. Restorers (R721)

State outside the collection that an undo or redo must put back (U44's load-balancer cache and learning cutoff, U43's session queue) is restored by restorers registered on the collection. An op attaches an opaque restorer state to the step it records. After an undo or redo commits, on the writer and before the change set is published, each restorer gets the direction, the step (id, name, kind, session), the change set and that state. Like observers they keep short and never submit ops; one that panics is logged and the others still run. Showing the card next (R722) is U43's.

## M11. Undo classes (R2020)

- Every op of the op catalog (`engine/op-model` O9) declares one class: `Undoable` (recorded as a step), `DestructiveNonUndoable` (not undoable: confirmed first and preceded by U127's pre-op backup), or `NonDestructive` (not undoable and destroys nothing). Units add their ops to the catalog with their class.
- Every op source declares its effect (M2): recorded, clear all, truncate, or the sync merge; skip-undo is per op. `tests/op_catalog.rs` enumerates both.

## M12. Budget

Undo of an answer (one card row, one review-log row and `col`) returns within R1811's 50 ms with a wide margin: the `undo` criterion suite (`core/crates/meguri-engine/benches/undo.rs`) times the undo op's round trip at 10k and 150k cards and fails on Apple hosts when its p95 reaches 5 ms.

- Measured 2026-10-16, undo and redo of an answer alternating: `tools/mg bench-mac --bench undo` (M3 Max) p50 77 µs (10k) and 92 µs (150k), p95 0.20 ms and 0.25 ms; `tools/mg bench --bench undo` (Docker, tmpfs) p50 29 µs and 32 µs, p95 63 µs and 91 µs.
- A first Mac run, with undo and redo as ops that may declare a long write, measured p50 4.0 ms and p95 4.96 ms: the writer syncs a pending WAL flush before such an op (C9.2's full sync, about 4 ms). Plain `undo` and `redo` are therefore short catalog ops, and only a step of a long write uses `undo_long_write` and `redo_long_write`.

## Observable results of undoing each op

Pending: the spec writer records, per op, what undoing and redoing it leaves in the collection compared with the oracle's `undo` (names, rows, graves, `col.mod`), with scenarios under `conformance/scenarios/undo/`.
