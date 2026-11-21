# Spec record for U265, U87 and U92: review log, 2026-11-21

Branch `unit/spec-record-3` (PR #85), spec-writer session `s-20261121T180131Z-d1c2cc95`. The lines marked "changed 2026-11-21" with "implementation review" in `browser/find-replace` F9 to F13, `browser/workspace` V8.3, V9 and V17, `browser/bulk-framework` K1 to K3, `browser/bulk-actions` B1, B7.1, B7.2, B9, B11, B16 to B18, `browser/result-sets` H1's session line, `conformance/bulk-ops` X1 and X5, `conformance/stats-ops` H9, `conformance/harvest-followups` H7 and `stats/card-info` C7.4, C7.5, C8, C10, C11, C13.2 and C14.1 record what U265 (PR #78, merged), U87 (PR #77, open) and U92 (PR #82, open) built. The coordinator approved the sentences and rulings under Antoine's permission of 2026-11-20 to update specs (`docs/process/standing-decisions.md`).

## Decisions

1. The approved sentences are written in the sections they name. F11's confirmed count replaces the earlier "lands with U265" bullet. The refusal-order addition says "below", not "above", because the confirmed-count bullet comes after the refusals.
2. V8.3 and B16 follow what PR #77 built: one step, "Unbury/Unsuspend", for both commands (its tests assert that name). The engine keeps V4's name, which `undo_status` compares with the oracle. The app takes the Undo title from the command it ran, keyed by the step id in the action's result. A step the app holds no command for shows the step name.
3. U87's and U92's text is written now. Their scenarios are not called active: the spec side activates them after each PR is on `master`, in a second PR.
4. Ten of the eleven deferred catalog rows listed by PR #73 are linked and `done`. Each was checked against its Anki test at the pin (a separate subagent read the tests; this log quotes none). `h28ba189d8c7e` stays `deferred`. `notes/transform/replace-saves` covers the Find and Replace path. The test also asks for a transform with card generation off and with registered tag spelling, which only `h83c15fb8bf40`'s planned `refresh_notes` op reaches. `tools/mg harvest-report`: pass, 1,179 classified.
5. `docs/design/screens.md` and `screen-inventory.yaml` change three words ("the first 100 notes that change"); no design decision changes.

## Independent review

Reviewer: an independent Claude subagent (Opus). It received the diff and the sentence files, not the writer's reasoning. It checked them against the rest of `specs/browser/`, `specs/stats/` and `specs/conformance/`, the design screen texts, and the code of PRs #77, #78 and #82.

No approved text is missing or changed in meaning; the code of PRs #77, #78 and #82 agrees with what the diff says of it.

- P1:
  - Fixed. K2's review admission did not say that the session refuses new answers (`Unavailable`) from admission until the commit settles, which is what PR #77 does. K2 says so now; K3's overlay sentence points to it, and B7.2 step 7 says its re-reads guard callers that are not admitted. **For the coordinator:** answers are refused for the whole job (up to R1831's 2 s for 10,000 notes) while a review session is open.
- P2:
  - Fixed. K3 listed notes among the tables answering never writes; a leech answer writes its note's tags. Qualified.
  - Fixed. "The first 100 changes" now reads "the first 100 notes that change" in F10's note, V17, `screens.md` and `screen-inventory.yaml`.
  - Fixed. The unit brief `B1-browser.md` and the main plan still described F10's range; both now point to F10.
  - Fixed. B17, X5 and C13.2 now say that the spec side activates the scenarios after PRs #77 and #82 merge.
  - Fixed. B17's not-built list now includes the chooser tests.
  - Fixed. Plan section 9's U86 sentence no longer says the confirmed count is in progress.
  - Not changed. C10 cites `whole_collection` from `engine/suspend` and `sched/queue-building` B10, where it is defined and published. Adding it to `engine/change-stream` S1's table is outside these rulings.
  - Fixed. This log was missing.
  - Fixed. V8.3 now says which title a step shows when the app holds no command for it.
- P3:
  - Fixed. K2 says only U87's FFI admits so far; Find and Replace's start call must too. K1 points to B1 for the all-notes target. V17 says the preview counts as the run counts on current rows, not that the counts are equal. `browser/result-sets` no longer names the preview as a search-session caller. F10 cites `core/pins.toml`. B7.2's known limit says the register row comes with PR #77. C10 says "exists but cannot be decoded". C8 adds the non-finite REAL and non-UTF-8 `rollover` rules the code follows. C13.2 and H9 say "the 18 with goldens (all but `damaged`)". B9 uses K4's wording. V8.3's singular follows V8.1's count. B18 gains the two refusals, and F13 gains Whole Words.
  - Not changed. `sched/presets` V6 gains no note; C11 states the Card Info side.
  - Not changed. The stats-ops intro says Meguri's executor has none of H9 to H14. It is corrected with the activation PR.
  - Not changed. The `change_deck_choices` harvest rows keep U87 as owner. Changing their owner is for the rows' owner, not this record.

## Change Deck activation (after PR #77 merged)

1. The five Change Deck scenarios land from `held/harvest-batch-2` and are active (`notes/area.yaml`; `search/state/*` was already active by glob). The test-data similarity lane flagged four on generic text: the review-log column list of an `INSERT` and card requests of eight or nine references. These were rewritten as an `INSERT` with no column list and two card requests (which `conformance/observations` merges). The goldens were re-recorded with `tools/mg oracle record`; only their scenario hashes changed. On the 10 files: similarity 0 flagged, `goldens-check` pass, and `tools/mg conformance --lane docker notes/ search/` 0 T0 failures, with the five passing.
2. The twelve Change Deck rows of H7 are linked and `done`, after a check of each against its test at the pin (subagent; no Anki text here). h66f7a2aeee4f's test edits a preset's steps rather than moving a card, so it links `sched/options/remap-steps` as well, and its reason now says so.
3. V8.3 and B16: the coordinator's note on PR #77 is written. The screen derives the title from the action it ran. A title read from the stored undo history alone needs a later core unit, which adds the command to the step through `StepInfo`, `UndoMeta` and the FFI.
