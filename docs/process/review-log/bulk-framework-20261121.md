# U86 bulk framework design doc: review log, 2026-11-21

Branch `unit/u86-bulk-find-replace` (PR #63), implementer session `s-20261121T105418Z-96f4ce9b`. The file under review is `specs/browser/bulk-framework.md`, a new Meguri-only design document (K1 to K6, and K7 added after this review). The U86 implementer wrote it to record how Browse's bulk actions run in the core. The coordinator asked for an independent review before merge.

## Independent review

The reviewer was a Claude subagent (Opus) that did not write the document. It was given the file, the code under `core/crates/meguri-engine/src/bulk/` and the executor, and the specs `browser/bulk-actions`, `browser/find-replace`, `browser/result-sets` H12, `engine/op-model` O5 to O9, `engine/jobs` J1 to J5, `engine/undo` A2 and M2, and `specs/README-format.md`. It read no Anki source. It answered three questions:

1. Is the document enough for a U87 implementer to add a new bulk action? Not yet: the executor facts held, but the steps through the code's API were absent.
2. Does it contradict another spec? Yes in one place, K5 against B7.2 (finding 1).
3. Does it follow the format rules? Yes, and it needs no Status line. `README-format.md` allows a Status line "only in a contract that a requirement says must be reviewed before implementation"; no requirement asks that of this document. Its `Anki: none (Meguri design, U86)` line follows existing Meguri design docs.

## Findings and what was done

- **Must-fix.**
  - K5 said `affected` is the rows written, and that affected 0 records no step and keeps `col.mod`. This contradicts B7.2 steps 5 and 6: Change Deck counts only moved cards, and a deck created inline makes a step at count 0. Fixed: `affected` is B16's count for the action and can be 0 while rows are written. "Nothing changed" is now about a commit that changes no row.
- **Should-fix, all fixed.**
  - K5: `considered` for a selection is the length of the resolved list. Delete Notes' second count goes in its own result. Clear Unused Tags takes no K1 target (K1).
  - K3: O7's "every row a planned decision was computed from". Guidance on re-deriving rows that reviews write (cards, the review log) rather than using `TableUntouched` on those tables, which would end in `Stale`.
  - K2: the start call refuses only as `engine/jobs` J1 and J3 say. Every action refusal, K1's included, is the job's `Failed` outcome. The result is kept with the started job (`BulkJob`), and `JobFinished` carries no counts.
  - K3: the small op takes the engine name its spec gives (O1). The 2,000-row threshold counts target rows only. The declared length is about 60 µs per row.
  - K4: how an action's refusals map to `EngineError`. K1: engine errors met while resolving a selection are returned as they are.
  - K3 and K5: `past_cancel_point` (O5). The code dropped it; `BulkOutcome` now carries it.
- **Nits, fixed.**
  - K5: the step name comes from `undo::names`, kind `Other`, and is fixed before the commit reads anything.
  - The intro says `browser/batch-edit` is not written yet.
  - K1: a resolved empty list is refused, as an empty list is.
  - K7 (new): the steps of building an action, with Find and Replace as the worked example.
- **Not in this file, for the spec side.**
  - B16 says the counts come with "U86's `JobFinished`".
  - The intro of `browser/bulk-actions` still lists "the result message" as framework scope, although its round 8 change removed messages.

## Status line

No Status line is added. The coordinator decides.
