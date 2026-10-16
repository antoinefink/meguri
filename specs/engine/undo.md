# Undo and redo

Spec: engine/undo
Requirements: R719, R720, R721, R725 (as KTD7 reconciles it), R1795, R1796, R2020; supports R365, R366, R370, R371, R723, R726, R1811
Anki: none for sections M1 to M12 (Meguri design from KTD5, KTD6, KTD7 and spec-flow C3, Q1, Q4, Q11; written by the implementer, U25). Sections A1 to A11 are Anki's observable results at 26.09.3, written by the spec writer; A11 lists what they change in M1 to M12.
Scenarios: `conformance/scenarios/undo/` (A10); mechanism tests in `core/crates/meguri-engine/tests/undo_basic.rs`, `undo_truncation.rs`, `undo_holdback_props.rs`, `op_catalog.rs`

The undo manager of one open collection (C3): the steps it records from committed ops, what every other op does to them, undo and redo as ops, the reasons a step is unavailable, the held-back run a normal sync keeps local, and the rule that no uploaded step is ever undone. Ops, sources and journals are `engine/op-model`; the change set is `engine/change-stream`.

## M1. Steps

- A step is recorded from every committed op whose source is `UserUndoable` and whose journal (U23) holds at least one change, unless the op is an undo or a redo (M4, M5), or the top step takes the op in (merging, below). An op that changed nothing records nothing, and still empties redo (M2, A2).
- A step holds: its id, which is the seq of the op that made it (`engine/op-model` O4); its name; its kind; its session; the op's journal (every changed row's before-image, and its after-image, a digest for notes); the notes it touched (the notes it changed and the notes of the cards it changed, from its change set); the `col.mod` its undo restores, which is the value `col.mod` held just before the op (A7); its time, the epoch second at which the op began (the writer reads the clock before the op's closure runs); the write phase the op declared, if any (O8); its upload state (M9); and an optional restorer state the op attached (M10).
- The name is the step name the op gives, one of `undo-ops.md` V4's names exactly (`src/undo/names.rs`: "Answer Card", "Update Note", "Delete Note", R719, R370, A1); without one it is the op's name. The kind is `Answer` for an answer and `Other` for every other step; only the held-back run (M8) reads it. The session is the review session the op names, if any.
- **Merging (A3).** The top step takes in a step about to be recorded when both are "Update Note" steps whose journals changed one and the same note row (there before and after) and nothing else but `col`, both have the same session, the top step is still local (never uploaded or pending, M9), and the new step's time minus the top step's is below 60 seconds (a negative difference counts as 0). The top step keeps its id, kind, session, touched notes, before-images, `col.mod` and time, and takes the new op's after-images (the journals composed, meguri-storage `Journal::then`), so its undo puts the note back as before its first edit and the check of M4.2 accepts it. The step a redo records merges the same way (M5, A3: a redo of an "Update Note" step is a note update). The step an undo leaves on redo never merges. The two conditions Anki has no counterpart for, local and same session, keep a step a sync uploaded or holds pending unchanged and keep a session-scoped undo (M3) from reverting an edit made outside its session.
- The touched set of a step is the keys of its journal, the notes it touched, and the graves of the cards, notes and decks it removed (M4.3).
- At most 30 steps are kept (R719). A 31st drops the oldest. Steps live in memory only: before-images past the journal's spill threshold live in its unnamed temporary file, which the system deletes when the journal drops, so nothing survives a quit and nothing needs a sweep at launch. Closing the collection drops every step.

## M2. What each op does to the history

Each op source has one undo effect, applied on the writer right after the op commits and before its result is returned, so a caller that waited for an op sees the history it left.

| Source | Effect when the op changed something |
|---|---|
| `UserUndoable` | recorded as a new step, or merged into the top step (M1); redo is cleared, also when the op changed nothing (A2) |
| `UserNonUndoable` | clear all, reason `ClearedByNonUndoableOp` (R366, R725); an op marked skip-undo instead keeps the steps and clears redo (below) |
| `LocalApi` | never recorded; truncates at the deepest intersecting step (M7, R1795, R1796) |
| `SyncIncoming` | the merge rule of M9.4 (KTD7) |
| `Optimizer`, `Maintenance` | clear all, reason `ClearedByNonUndoableOp` |
| `Rollover` | clear all, reason `ClearedByRollover` (R462) |

- Clear all drops every step and every redo step, and sets the reason peek reports once the history is empty (M3).
- Skip-undo: an op of source `UserNonUndoable` may be marked skip-undo. It is never recorded and keeps every step, and it clears redo, changed or not, as every op but undo and redo does (A2, A11.1): selecting a deck (`curDeck`, `activeDecks`; U28's op and U43's session start) keeps the answer before it undoable (R42), and so does a config write without an undo entry (`record-ops.md` W4). Any other source marked skip-undo is refused with `InvalidArgument` at submit.
- An op that changed nothing has no other effect: a `UserUndoable` or skip-undo op clears redo and nothing else, and an op of any other source leaves both histories (A11.6). A refused undo or redo, and an op that fails and rolls back, leave both.

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

1. **Target.** The top step for the scope (M3) when the writer takes the op, not when it was submitted: an undo submitted while a bulk op holds the writer undoes the bulk op's step once that commits. The writer resolves the step before the op's write phase starts, and the op then runs as `undo_long_write` with the step's declared write when the step has one, so the WAL flush before a long write and its `WriteDelay` (O2, O8) follow the step it restores. None: refused with `UndoUnavailable` and the reason peek reports (`None`, or why the history was emptied). Upload state `Uploaded` or `Pending`: refused with `AlreadyUploaded` or `UploadPending` (M9); the core refuses whatever the caller. Only the writer adds steps, so between the resolve and the restore another thread can only drop the step (M6, M9.2) or flag it: the op then refuses with the reason peek reports, or `None` when another step took its place. A refusal writes nothing.
2. **Check.** Every row of the step's journal must still equal the step's after-image: the same digest (`storage` journal digest), or absent where the step removed it. For the `col` row only the columns the step changed other than `mod` are compared. If any row differs, the step and every older step are dropped, the reason becomes `ChangedExternally`, and the undo is refused with that reason, writing nothing.
3. **Restore.** Every journal row gets its before-image back: rows the step inserted are deleted, rows it deleted are inserted again (with the graves it added removed, since those are journal rows too), changed rows are written back whole. Rows of `decks`, `notetypes`, `fields` and `templates` the step changed are deleted first and inserted after, so names moving between rows never meet in their unique indexes. Graves: when the step removed a card, note or deck without adding its grave (the grave existed already, `storage/graves` G2), the undo also removes that grave if it is there, as Anki does (`storage/graves` G6); its redo puts it back pending (M5).
4. **`col`.** The columns other than `mod` that the step changed get their before values; `mod` gets the step's `col.mod` (M1: the value it held just before the step's op, or, for a redo step, when the undo ran; A7, `storage/ids-usn-mtime` S5), never now.
5. **Commit.** The undo op's own journal becomes a redo step with the undone step's name, kind, session, declared write and restorer state; its id is the undo op's seq and its time the second the undo began. Its `col.mod` is the value `col.mod` held when the undo ran, read before the restore, so the redo puts back that value (A7: the one the step set, unless something that keeps the history moved it since, such as a skip-undo write).

The result's value names the undone step; its change set has source `UserUndoable`; `undo` (the recorded-step metadata of O4) is empty.

A step whose op declared a long write (a job-run op, KTD9: bulk ops, large structure changes, an `.apkg` apply) reports that write in peek; its undo and redo declare the same write (O8, resolved on the writer as M4.1 says), so callers start them on the maintenance lane and never block the interactive lane.

## M5. Redo

Redo is an op (catalog `redo`, or `redo_long_write`) like undo, on the top redo step: refused with `None` when there is none; the same check (M4.2: here a failure drops that redo step and every redo step below it) and restore (M4.3, M4.4), except that every grave it writes back, the graves its undo removed, gets `usn` -1, also one that held a server `usn` before the deletion (A5). Its journal becomes a new step at the top of the history, with the redo step's name, kind, session, declared write and restorer state; its `col.mod` is the value `col.mod` held when the redo ran, and its time the second the redo began. It merges into the top step as a new step would (M1). The other redo steps stay. Every `UserUndoable` op but undo and redo, and every skip-undo op, clears redo (M2, A2, R720).

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
- After A11's changes (the `col.mod` read before the restore, the merge check): `tools/mg bench --bench undo` (Docker) p50 33 µs (10k) and 32 µs (150k), p95 86 µs and 105 µs.
- A first Mac run, with undo and redo as ops that may declare a long write, measured p50 4.0 ms and p95 4.96 ms: the writer syncs a pending WAL flush before such an op (C9.2's full sync, about 4 ms). Plain `undo` and `redo` are therefore short catalog ops, and only a step of a long write uses `undo_long_write` and `redo_long_write`.

## Observable results of undoing each op

A1 to A10 say what Anki's undo and redo leave in the collection and in the two histories, for the ops that scenarios run today (`specs/conformance/record-ops.md` W2 to W4, `undo-ops.md` V3, `dsl.md`'s `answer`). The scenarios under `conformance/scenarios/undo/` record it from the oracle through the ops and the observation of `specs/conformance/undo-ops.md`. Meguri matches all of it except A8, a deviation. A11 lists what this changes in M1 to M12.

### A1. Step names (R370, R719)

Every step carries the name of the operation that made it; Anki shows it as "Undo Update Note" and "Redo Update Note". The names for today's scenario ops:

| Scenario op | Anki operation | Step |
|---|---|---|
| `edit_note` | note update | Update Note (merged per A3) |
| `delete_notes` | note removal | Delete Note |
| `add_note`, `setup.notes` | note addition | Add Note |
| `answer` | answer | Answer Card |
| `suspend` | suspend | Suspend |
| `set_config` with `undoable: true`, `remove_config` | config write, config removal | Update Config |
| `set_config` (W4) | config write without an undo entry | none (A2) |
| `db_exec` not beginning with `select`, `check_db`, `reopen` | raw write, Check Database, close | none: both histories emptied (A2) |

`undo-ops.md` V4 lists every name Anki 26.09.3 gives, with the ids `undo_status` reports. Where R719's examples differ, V4 has Anki's wording: "Reset Card" (not "Reset") and "Unbury/Unsuspend" (not "Unbury"). Renaming and moving a tag are both "Rename Tag", renaming and moving a deck both "Rename Deck", and building and rebuilding a filtered deck both "Build Deck".

### A2. What each op does to the two histories

- **Recording.** An undoable operation that changed at least one row records one step (A3's merge aside). At most 30 steps are kept: the 31st drops the oldest, so after 31 edits 30 undos succeed and the first edit stays.
- **Every operation empties the redo history**, whether or not it changed anything and whether or not it records a step: an `edit_note` that saves the note unchanged (`storage/ids-usn-mtime` S4), a config write without an undo entry (W4), a config write of the value the key already holds, the removal of a missing key. Only undo and redo leave it as it is.
- **A config write without an undo entry (W4) keeps the undo steps**: it writes the key and moves `col.mod` as usual, and the steps below it can still be undone (A7 shows its effect on `col.mod`).
- **Both histories are emptied** by a `db_exec` statement that does not begin with `select` (also one that changes nothing), by Check Database and by closing the collection. A `select` leaves both.
- **`delete_notes` whose notes no longer exist** runs no operation (`record-ops.md` W3): both histories stay as they are.
- Undo moves its step to the redo history and redo moves it back; neither touches any other step. With nothing to apply they are refused (`undo-ops.md` V1).

Scenarios: `undo/redo-cleared`, `undo/thirty-steps`.

### A3. Merged note edits

A note update that changes exactly one note row and no other row besides `col` (no tag registered, no card generated) records no step of its own when both of these hold:

- the top step of the undo history is an "Update Note" step that changed exactly the same note row and no other row besides `col`;
- that step was recorded less than 60 seconds ago: the clock's current second minus the second at which the step's first operation began is below 60 (both whole epoch seconds, rounded down; a clock moved back counts as 0).

The top step then covers both edits. Its time stays that of its first operation, so edits at 0 s, 30 s and 59 s form one step and an edit at 60 s starts a new one, even 1 s after the previous edit. A step put back by a redo counts as recorded at the redo, so an edit within 60 s of a redo can merge into the redone step.

A redo of an "Update Note" step is a note update for this rule. When the redone step changed exactly one note row and no other row besides `col`, the step the redo records merges into the top step under the same two conditions as an edit: the redo applies its rows and leaves the redo history as any redo does, but adds no step of its own. Two such redos of one note in the same second are 0 s apart, so they merge. An undo never merges (this could only show with the clock moved back): the step it moves to the redo history stays whole.

In `undo/edit-note-merge`, the redo of the "gatito" step at 12:01:05 records a step, the redo of the "michi" step of the same note in the same second merges into it, and an edit of that note 30 s later merges too. One undo then reverts all three: the note and `col.mod` are back as they were before the first redo, nothing is left to undo, and the redo history holds only that "Update Note" step (the edit emptied it of the other note's step, A2).

- Undoing the merged step puts the note back as it was before its first operation (the first edit, or the redo that began the step), and `col.mod` to its value before that operation (A7).
- Redoing it puts the note back as it was when the undo ran: the row the last merged edit or redo left, `mod` included.
- An edit of another note, and an edit that registers a new tag or generates a card, records a step of its own, and no later edit or redo is merged into such a step. An edit that changes the note's tags only to tags already registered is a plain note change and merges.
- The rule looks at the rows changed, so it holds for any note update (an editor save as much as `edit_note`).

Scenarios: `undo/edit-note-merge`, `undo/edit-note-new-card`.

### A4. `edit_note` (note update)

The operation changes the note row (`mod` now in seconds, `usn` -1), adds a `tags` row (`usn` -1) for each tag not yet registered, adds a card row for each card the new content generates (U27), and sets `col.mod` to now.

- **Undo** writes the note row back exactly as it was before the edit, every column included: its `mod`, and its `usn` even when that was a server `usn`. It deletes the tag rows and the card rows the edit added; the deleted cards get no grave. Any other row the edit changed gets its earlier content back. `col.mod`: A7.
- **Redo** writes back the rows as they were when the undo ran: the edited note with the edit's `mod` and `usn` -1, the tag rows and the generated cards with their ids and stamps. `col.mod`: A7.

Scenarios: `undo/edit-note`, `undo/edit-note-new-card`.

### A5. `delete_notes` (note removal)

The operation deletes each note and its cards and adds their graves with `usn` -1, keeping a grave that already exists as it is (`storage/graves` G2, G3), and sets `col.mod` to now.

- **Undo** inserts every deleted card row and note row again exactly as stored before, each with its own `mod` and `usn`. It deletes every grave of the deleted cards and notes, including a grave that existed before the deletion (G6: an object re-created under an id that was deleted before, whose old grave stayed). After the undo no grave remains for them. `col.mod`: A7.
- **Redo** deletes the rows again and adds back every grave the undo removed, each with `usn` -1, a grave that had a server `usn` before the deletion included: after a deletion, its undo and its redo, all the deletion's graves are pending. `col.mod`: A7.
- Anki writes a restored note's `flags` as 0 and `data` as empty, the values every note Anki writes holds, and a restored card's `data` in its own encoding (`specs/model/card-data-json.md`). Meguri restores the stored rows whole, so the two differ only for rows whose `flags` or `data` another writer set. No scenario shows it; a scenario that does needs a deviation (for card data, the `card-data-unknown-keys` family).

Scenarios: `undo/delete-notes`, `undo/delete-note-existing-grave`.

### A6. Config writes

For the writes that record a step (`undo-ops.md` V3):

- **Undo** of a write that added the key deletes its row; of a write that changed it, writes the earlier row back exactly (`val` bytes, `usn`, `mtime_secs`), a server `usn` included; of a removal, inserts the removed row again exactly. `col.mod`: A7.
- **Redo** writes back the row as it was when the undo ran (the written value with the write's `mtime_secs` and `usn` -1), or deletes it again for a removal. `col.mod`: A7.
- A write of the value the key holds, or the removal of a missing key, writes nothing, records nothing and leaves `col.mod`; it empties the redo history (A2).
- A config write without an undo entry (W4) records nothing, and an undo or redo of another step never touches its row.

Scenarios: `undo/config-writes` (harvest row `hde5c73b93e34`), `undo/col-mod`.

### A7. `col.mod` (`storage/ids-usn-mtime` S5)

- **Undo** sets `col.mod` to the value it had just before the undone operation ran.
- **Redo** sets `col.mod` to the value it had when the undo of that step ran. That is the value the operation set, unless something moved `col.mod` after the operation without emptying the undo history, such as a config write without an undo entry (W4): the redo then restores that later value.
- The undo of a redone step sets `col.mod` to the value it had when the redo ran, which is the value from before the operation unless something moved it in between.
- Neither undo nor redo sets `col.mod` to now, and neither changes `col.usn`.
- `col.scm`, for a step that changed it (a schema change, U29), follows the same rule. No scenario shows it yet.

Scenario: `undo/col-mod`; the other scenarios show the plain case.

### A8. A review-log row restored under a taken id (`storage/ids-usn-mtime` I4)

A redo of an answer inserts the answer's review-log row again under its id. When a row with that id exists by then, Anki inserts nothing and keeps the existing row, and applies the rest of the redo (the card is answered again, `col.mod` per A7). The step the redo records still counts the review-log row as its own: an undo of it deletes whatever row then holds that id (seen in a spec-writer probe of the oracle; the scenario stops before it).

In Anki only a writer outside the undo history can take the id between the undo and the redo, since every operation empties the redo history; the scenario uses an SQL trigger as that writer.

Meguri differs: its check (M4.2) finds a row where the step's after-image has none, so it refuses the redo with `ChangedExternally` (`changed_externally` in the scenario, `undo-ops.md` V1) and drops it. The undo of the answer agrees with Anki: the card row is back with its `mod` and `usn`, the answer's review-log row is gone (the trigger's row takes its place on both), and `col.mod` is back. Deviations `undo-revlog-taken-id-redo`, `undo-revlog-taken-id-status` and `undo-revlog-taken-id-card` in `conformance/deviations.yaml` (R1796).

Scenario: `undo/revlog-taken-id` (harvest row `hf7664c5a108b`).

### A9. Rows of other tables

Every row a step changed, in any table, comes back as A4 to A6 describe for notes, cards, tags and config: an inserted row is deleted, a deleted row is inserted again, a changed row is written back whole with its own stamps; redo does the reverse with the rows as the undo found them. Graves are the one exception (A5).

### A10. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `undo/edit-note` | A1, A4 and A7 for one edit, the note's server `usn` restored; refusals at both ends | recorded |
| `undo/edit-note-merge` | A3: edits at 0, 30 and 59 s merge, one at 60 s and an edit of another note do not; undo and redo of the merged step; two redos of one note's steps in the same second merge, an edit 30 s later merges into them, and one undo reverts all three, leaving nothing to undo | recorded |
| `undo/edit-note-new-card` | A3, A4: an edit that registers a tag and generates a card, and a later edit that is not merged into it; the undo deletes the card without a grave and the tag row | recorded |
| `undo/delete-notes` | A5: cards and notes back with their stamps, graves gone; redone and undone again | recorded |
| `undo/delete-note-existing-grave` | A5 with G6: graves that existed before the deletion are removed by the undo and come back pending with the redo | recorded |
| `undo/config-writes` | A6: an added, a changed and a removed key, undone and redone; a same-value write and a missing-key removal; harvest `hde5c73b93e34` | recorded |
| `undo/col-mod` | A7 with a config write without an undo entry between an edit and its undo | recorded |
| `undo/redo-cleared` | A2: which ops empty the redo history, which keep the undo steps, and the refusals | recorded |
| `undo/thirty-steps` | A2: the 30-step limit | recorded |
| `undo/revlog-taken-id` | A8; harvest `hf7664c5a108b`; deviations `undo-revlog-taken-id-*` | recorded |

`conformance/scenarios/undo/area.yaml` lists none as active yet: the implementer marks them active with the undo adapter (`undo-ops.md` V6).

### A11. What this changes in M1 to M12

Anki's results above that the mechanism did not produce at first. M1, M2, M4 and M5 now follow items 1 to 5 (U25's implementer, with the code); item 6 stays as written.

1. **Redo emptied by every op (A2).** M1's "an op that changed nothing records nothing and leaves redo alone" and M2's skip-undo op that "is neither recorded nor clears anything" become: every op of source `UserUndoable` empties the redo history when it runs, whether or not it changed something; a skip-undo op (W4's config write, and R42's deck selection, where Anki records a "Select Deck" step and so empties redo too) keeps the undo steps and empties redo. Undo and redo keep it.
2. **Merging (A3).** A step needs the second at which its first op began (the clock seam's time, rounded down to whole seconds; a redone step takes the time of the redo). When an "Update Note" op merges into the top step, the step keeps its before-images and takes the op's after-images, so that M4.2's check accepts the next undo; its id, kind, session and touched set stay. The step a redo of an "Update Note" step records merges the same way; the redo step an undo leaves never merges.
3. **Graves on redo (A5).** A redo of a deletion writes every grave its undo removed with `usn` -1, also one that held another `usn` before the deletion. M4.3's removal of the earlier grave is right; its redo puts the grave back pending, not with the earlier `usn`.
4. **`col.mod` on redo (A7).** The redo step takes as its `col.mod` value the one `col.mod` held when the undo ran, and the step a redo records takes the one it held when the redo ran, instead of the step's own values swapped (M4.5, M5).
5. **Names (A1).** The steps of these ops carry the names of `undo-ops.md` V4 exactly: "Update Note", "Delete Note", "Add Note", "Answer Card", "Suspend", "Update Config".
6. **Unchanged, for the record.** Anki's operations without an undo record (Check Database, raw statements) empty both histories even when they change nothing; M2 gives an op that changed nothing no effect. The scenario ops of that kind are `check_db` and `db_exec`, which Meguri clears explicitly (M6, `record-ops.md` W1), so no scenario shows a difference; the units that add other such ops decide.
