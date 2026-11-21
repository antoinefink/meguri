# Bulk action framework

Spec: browser/bulk-framework
Requirements: R1181, R1182, R1183, R1184, R1185, R1245, R1831, R1832, R1860
Anki: none (Meguri design, U86); the actions it runs keep their own specs and Anki behaviour
Scenarios: none of its own; the actions' scenarios (`conformance/scenarios/browser/bulk/`, `browser/find-replace/`); tests in `core/crates/meguri-engine/tests/find_replace.rs`

How Browse's actions over many notes or cards run in the core (U86): what they act on, the job that runs them, their two phases, cancellation, progress, the undo step, the counts they report and `selection_summary`. Which rows an action writes is its own spec: `browser/bulk-actions` (U87's card and note actions), `browser/find-replace` (Find and Replace), and Edit Field's Meguri design doc `browser/batch-edit` (not written yet). The executor rules are `engine/op-model` O5 to O9, jobs `engine/jobs`, undo `engine/undo`. Code: `core/crates/meguri-engine/src/bulk/`; `bulk/find_replace.rs` is the worked example of K7.

## K1. Targets

- A note action takes one of: a list of note ids; all notes (Find and Replace only, `browser/bulk-actions` B1; changed 2026-11-21, implementation review); a selection of a search handle (`browser/result-sets` H12: the handle, its revision and index ranges). A card action takes a list of card ids or a selection. An action on the whole tag registry (Clear Unused Tags) takes no K1 target.
- A selection stays index ranges until the action's job runs; the job resolves it with H12's `resolve_selection` (target notes or target cards) in the core, so large selections never cross the FFI. A selection whose handle was released, whose session was closed, whose revision changed, or whose handle belongs to another collection or generation than the one the action runs on is refused with `InvalidArgument` and nothing is read further. An engine error met while resolving (the collection closed, a storage error) is returned as it is.
- A list (given or resolved) that is empty or names an id twice is refused with `InvalidArgument` before anything else is read (`browser/bulk-actions` B7.2, `browser/find-replace` F11). This holds for a resolved list too: a card action on a notes-mode selection whose notes have no cards is refused, as an empty list is.

## K2. The job

- A short start call returns a job handle at once (id, cancel token, progress sink); the work runs on the engine's job pool as a job of kind `bulk op` (`engine/jobs` J1, J2), which takes no long-operation slot. The start call refuses only as J1 and J3 say (a suspended collection); every refusal of the action, K1's included, is the job's `Failed` outcome.
- The job ends with exactly one `JobFinished{id, outcome}`: `Succeeded`, `Cancelled`, or `Failed` with the refusal. The event carries no counts: the action's result (K5) is kept with the started job (`BulkJob::outcome`, `BulkJob::wait`) and is there before `JobFinished` is published, so a listener that sees the event can read it.
- A job cancelled before it started never runs and its result is `Cancelled`.
- Changed 2026-11-21 (implementation review): **Review admission.** Every bulk action runs admitted by the review service (U87's `ReviewService::admit_card_action`; U87's FFI start calls run its actions through it, and Find and Replace's start call, which comes with U265's FFI, must too). Answers the review session has accepted and not yet written are written first, so a held answer on a card the action rewrites or removes is settled before the action reads it, and the session settles after the action's commit (`sched/review-session` C13). From admission until then, the session takes no new answer: one is refused as unavailable (`Unavailable`), as during the review service's other admitted operations; the review screen holds it and tries again. Admission ends when the action ends, whether it commits, is refused, is cancelled or fails; only a commit (also one whose result is an error after it committed) makes the session settle first. The answers written at admission are their own committed steps: K4's "nothing written" for a refused or cancelled action covers the action's own rows and step, not those answers.

## K3. Phases

1. **Compute.** On a snapshot of the maintenance reader, off the writer: the action reads its rows a page at a time, checks its refusals, and plans what it will write. It writes nothing. It checks the cancel token between pages and reports progress in the `Planning` phase with the rows read and the total (`engine/jobs` J5).
2. **Commit.** One write transaction through the executor (`engine/op-model` O6): from the moment the writer takes it, progress is `Committing`, the UI stops offering Cancel and a cancel is ignored (O5); the result then reports `past_cancel_point`. The phase change to `Committing` is delivered from the writer before the transaction begins, so a listener must return at once and never call the engine; no progress counts are reported inside the transaction.
   - Every row the commit writes is re-derived from its live row or covered by a precondition (O7), and so is every row a planned decision was computed from (O7: a decision read from decks, presets or the review log is covered too). A failed precondition makes the action plan again on a new snapshot, at most three times, then it is refused with `Stale`.
   - Which of the two: tables that answering cards writes during a plan (cards, the review log) are re-derived in the commit from the stored rows, since a `TableUntouched` on them fails with each answer and ends in `Stale`. `TableUntouched` fits the tables answering does not write (notes, note types, fields, templates, deck presets; a leech answer does write its note's tags, `sched/answering` A2, which at worst makes the action plan again; changed 2026-11-21, implementation review). Changed 2026-11-21 (implementation review): decks are not among them, since an answer rewrites the deck rows of the studied-today counters (`sched/answering` A6); an action that decides from a deck reads it again in the commit instead, as Change Deck reads its target deck again before its first write (`browser/bulk-actions` B7.2 step 7). Find and Replace re-derives every note and its cards under `TableUntouched` on notes, note types, fields and templates.
   - An action whose plan writes more than 2,000 target rows (the notes or cards it rewrites; side rows such as tag rows, memory states or added cards do not count) runs its commit as the catalog's `bulk_op` (O9) and declares its write (O8), about 60 µs per row, so answers wait in the review overlay meanwhile (an admitted action already refuses new answers for its whole run, K2; changed 2026-11-21, implementation review). A smaller one is a plain op with the engine name its own spec gives (O1): `find_replace`, `change_deck`.

## K4. Cancellation and refusals

- A cancel before the commit starts, or any refusal, writes nothing, records no undo step and leaves the undo and redo histories as they were (`engine/undo` M2); the collection is unchanged (R1860).
- An error inside the commit rolls the whole transaction back: no row of the action is written.
- An action's refusals are `EngineError`s: its own refusal enum inside a variant of its own (`EngineError::FindReplace`), or an existing variant where one fits (`NotFound`, `DeckRefused`, `SchedOp`).

## K5. Result and undo

- The result is `{affected, considered}` (`browser/bulk-actions` B16).
  - `affected` is the count B16's table gives for the action (for Find and Replace the notes written; for Change Deck the cards moved, B7.2 step 5). It can be 0 while the commit writes rows (B7.2 steps 0 and 4).
  - `considered` is the length of the list K1 gives, after a selection is resolved, deleted ids included: for a card action on a notes-mode selection, every card of each selected note. For the all-notes target it is the notes the compute phase read. An action with a further count (Delete Notes' cards removed, B11) returns it in its own result beside these two.
  - With them come the commit's seq, its change set (the table refreshes the rows it lists, R1185), the undo step if any, and `past_cancel_point`.
- The commit is one op, so the action is one undo step on the shared stack (R1182), named with V4's name from `undo::names` (`FIND_AND_REPLACE`, `UPDATE_TAG`, `CHANGE_DECK`, `SUSPEND`) and kind `Other`. The name is fixed when the commit's spec is built, before the commit reads anything: an action whose step name depends on what the commit reads must decide it in its plan.
- **Nothing changed.** A commit that changes no row records no undo step and leaves `col.mod` as it was; like every user op it still empties the redo history (`engine/undo` A2, `browser/find-replace` F9).

## K6. `selection_summary`

`selection_summary(target)` reads on the query reader, without writing (a selection is resolved first, in its own read transaction, then the summary is read in one):

- `field_names`: `browser/find-replace` F3's field names for the target's notes; for the all-notes target, those of every note type of the collection;
- `notetypes`: the note types of the target's notes that exist, ascending id; every note type for the all-notes target;
- `notes`: how many of the target's notes exist (all notes: the collection's note count).

A note that no longer exists, or whose note type does not exist, adds no field name and no note type.

## K7. Building an action

1. A blocking `run(collection, target, input, cancel, progress)`: resolve the target (`NoteTarget::resolve` or `CardTarget::resolve`, for this collection), `bulk::check_ids`, the action's own checks that need no rows, then `plan_and_commit_with(collection, ReaderRole::Maintenance, op_spec(NAME, STEP, 0, &cancel, &progress), plan, |_, planned| op_spec(NAME, STEP, rows(planned), &cancel, &progress), commit)`.
2. `plan` reads the snapshot a page at a time with `PlanContext::check_cancel` between pages and returns the plan's value (ids and what the commit needs, never row images) with its preconditions (K3). `commit` re-reads, re-derives and writes, and returns `BulkCounts`.
3. A start call `bulk::start(engine, move |job| run(&collection, &target, &input, job.cancel.clone(), job.progress.clone()).map(BulkOutcome::of))`. Changed 2026-11-21 (implementation review): where a review service can be active (every FFI start call), the closure runs `run` admitted (K2, `ReviewService::admit_card_action`), as U87's start calls do; `bulk/find_replace.rs`'s `start` predates K2 and its FFI start call must add this.
