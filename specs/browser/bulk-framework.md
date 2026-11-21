# Bulk action framework

Spec: browser/bulk-framework
Requirements: R1181, R1182, R1183, R1184, R1185, R1245, R1831, R1832, R1860
Anki: none (Meguri design, U86); the actions it runs keep their own specs and Anki behaviour
Scenarios: none of its own; the actions' scenarios (`conformance/scenarios/browser/bulk/`, `browser/find-replace/`); tests in `core/crates/meguri-engine/tests/find_replace.rs`

How Browse's actions over many notes or cards run in the core (U86): what they act on, the job that runs them, their two phases, cancellation, progress, the undo step, the counts they report and `selection_summary`. Which rows an action writes is its own spec: `browser/bulk-actions` (U87's card and note actions), `browser/find-replace` (Find and Replace), `browser/batch-edit` (Edit Field). The executor rules are `engine/op-model` O5 to O9, jobs `engine/jobs`, undo `engine/undo`. Code: `core/crates/meguri-engine/src/bulk/`.

## K1. Targets

- A note action takes one of: a list of note ids; all notes; a selection of a search handle (`browser/result-sets` H12: the handle, its revision and index ranges). A card action takes a list of card ids or a selection.
- A selection stays index ranges until the action's job runs; the job resolves it with H12's `resolve_selection` (target notes or target cards) in the core, so large selections never cross the FFI. A selection whose handle was released, whose session was closed or whose revision changed is refused with `InvalidArgument` and nothing is read further.
- A list (given or resolved) that is empty or names an id twice is refused with `InvalidArgument` before anything else is read (`browser/bulk-actions` B7.2, `browser/find-replace` F11).

## K2. The job

- A short start call returns a job handle at once (id, cancel token, progress sink); the work runs on the engine's job pool as a job of kind `bulk op` (`engine/jobs` J1, J2), which takes no long-operation slot.
- The job ends with exactly one `JobFinished{id, outcome}`: `Succeeded`, `Cancelled`, or `Failed` with the refusal. Its result (K5) is held by the job handle and is there before `JobFinished` is published, so a listener that sees the event can read it.
- A job cancelled before it started never runs and its result is `Cancelled`.

## K3. Phases

1. **Compute.** On a snapshot of the maintenance reader, off the writer: the action reads its rows a page at a time, checks its refusals, and plans what it will write. It writes nothing. It checks the cancel token between pages and reports progress in the `Planning` phase with the rows read and the total (`engine/jobs` J5).
2. **Commit.** One write transaction through the executor (`engine/op-model` O6): from the moment the writer takes it, progress is `Committing`, the UI stops offering Cancel and a cancel is ignored (O5). No progress counts are reported inside the transaction (the listener is never called from the writer). Every row it writes is re-derived from the live row or covered by a precondition (O7); a failed precondition makes the action plan again on a new snapshot, at most three times, then it is refused with `Stale`.
   - An action that plans to write more than 2,000 rows runs its commit as the catalog's `bulk_op` (O9) and declares its write (O8), so answers wait in the review overlay meanwhile; a smaller one is a plain op named after the action.

## K4. Cancellation and refusals

- A cancel before the commit starts, or any refusal, writes nothing, records no undo step and leaves the undo and redo histories as they were (`engine/undo` M2); the collection is unchanged (R1860).
- An error inside the commit rolls the whole transaction back: no row of the action is written.

## K5. Result and undo

- The result is `{affected, considered}` (`browser/bulk-actions` B16): the rows written (notes for a note action, cards for a card action) and the number of ids the action was given, deleted ones included; for the all-notes target, the notes the compute phase read. With it come the commit's seq, its change set (the table refreshes the rows it lists, R1185) and the undo step, if any.
- The commit is one op, so the action is one undo step on the shared stack, named after the action (R1182; the names are `browser/bulk-actions` B16's).
- **No match.** An action that writes nothing (affected 0) records no undo step and leaves `col.mod` as it was; like every user op it still empties the redo history (`engine/undo` A2, `browser/find-replace` F9).

## K6. `selection_summary`

`selection_summary(target)` reads, in one read transaction on the query reader and without writing:

- `field_names`: `browser/find-replace` F3's field names for the target's notes; for the all-notes target, those of every note type of the collection;
- `notetypes`: the note types of the target's notes that exist, ascending id; every note type for the all-notes target;
- `notes`: how many of the target's notes exist (all notes: the collection's note count).

A note that no longer exists, or whose note type does not exist, adds no field name and no note type.
