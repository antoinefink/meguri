# FSRS optimizer (U47)

Spec-writer notes for `specs/sched/optimizer.md` (O1 to O6 and O17) and the
inactive scenarios under `conformance/scenarios/sched/optimizer/`. Read at tag
26.09.3 (`refs/anki-26.09.3`) and the `fsrs` crate at its `v6.6.2` tag
(`refs/fsrs-rs`, read with `git show v6.6.2:<path>`: the checkout is past the
tag); no code was copied. The oracle was not run for this pass: the ops of
O17.1 are not in the driver yet, and each scenario's expected values in its
comments are read from these files, for the recording stage to confirm.

| Spec section | Files read | Read for |
|---|---|---|
| O1, O4, O5, O6 | `rslib/src/scheduler/fsrs/params.rs` (the one-preset computation, the legacy and time-series evaluations, the item builder's sort), `proto/anki/scheduler.proto` (`ComputeFsrsParamsRequest`, `ComputeFsrsParamsResponse`, `EvaluateParamsLegacyRequest`, `EvaluateParamsResponse`) | the early return with no item, the strict loss comparison (a tie keeps the list in use), the short-term check, the health-check gate (more than 300 items) and its formula, which list the call returns |
| O1, O2 | `rslib/src/deckconfig/update.rs` (the all-preset mode: presets the screen did not send taken from storage, the trimmed `param_search` test, the search writer's default search, every step counted, health check off, a failed preset printed and skipped, `lastFsrsOptimize` written; the save clearing older lists when `fsrs_params_6` is empty), `proto/anki/deck_config.proto` (the update modes) | O1's second column and O2's name table |
| O1, O4, O17.1 | `ts/routes/deck-options/FsrsOptions.svelte` (the one-preset button: the untrimmed `paramSearch` test, the running total below 1440 minutes, the health-check toggle passed through, the web view's date parser for the cutoff) | the one-preset column; why `compute_fsrs_params` takes the step count as an argument (the count is the screen's, not the library's) |
| O6 | `rslib/src/config/bool.rs` | `fsrsHealthCheck` reads true when missing |
| O3, O17.2 | fsrs `src/training.rs` at `v6.6.2` (`compute_parameters`: fewer than 8 prepared items return the default parameters; fewer than 64, or only initialization items, return the fitted initial stabilities with the other defaults), `src/model.rs` (the model clips parameters at construction) | the crate's fallbacks; why 5 items give equal losses with an empty list in use (`not-better`, `relearning-check`) |
| O17.1 | `rslib/src/scheduler/fsrs/params.rs` (the progress thread updates the review count at least once before the call joins it; the count is reported nowhere else) | how the oracle can read `reviews` |
| O3 (the crate's preparation), O17.2, O17.3 | fsrs `src/dataset.rs` at `v6.6.2` (the outlier analysis over each card's first item, grouped by first rating and first interval: groups dropped from the smallest while under the larger of 20 and the rating's first-item count divided by 20, then kept only at 6 or more and within 100 days, 365 for Easy; every training item of a dropped pair removed), `src/training.rs` (the preparation runs before the 8 and 64 checks) | why one to 19 cards give the crate's defaults (`not-better`, `relearning-check`, `items`, `weight-search`, `preset-names`, `health-check`), and why `adopt` and `live-adoption` use 24 cards whose first reviews all come 1 day after the learning row |
| O17.1 | `rslib/src/storage/deckconfig/` (the name lookup: the `name` column is `COLLATE unicase` and the first match is taken), `rslib/src/storage/upgrades/` (the column's collation) | why `preset` is matched byte-equal, not through Anki's lookup (`preset-names`, `Twin` and `twin`) |
| O17.3 | `rslib/src/search/sqlwriter.rs` (the preset search matches `did` or `odid`) | `items` card c9 in a filtered deck |

Findings the spec relies on and the recording stage must confirm:

- The short-term check on the default parameters, from stability 1.0 after
  Again at 2 days, ends below 1.0 after 2 Good steps on the same day and
  above 1.0 after 40 (a hand computation puts the crossing near 13): the two
  cases of `relearning-check`.
- The item builder reads every row of every card the search finds, whatever
  the card's `type` or `queue` (O2's clarification, `items` card c8).
- The legacy evaluation reads the searched cards' rows only, never the rows
  of deleted cards that a whole-collection search trains on: `eval` of the
  `one_preset` white-space step of `weight-search` is not on the training
  items, which the deviation of that step covers.
- With the 24 cards of `adopt` and `live-adoption` (one kept group of 24
  first items, 240 items, below the 300 of the health check), the list
  trained at counts 1 and 2 has a lower log loss than both the empty list
  in use and the `fsrs6-c` list, so `kept` is false in every `adopt` call and
  both sides adopt Learner's list in `live-adoption`. Not computed for this
  pass; if a recording shows `kept` true, the history is changed, not the
  spec.
- Anki's all-preset save gives `Small` of `live-adoption` (one card) the
  crate's defaults, equal losses at its count of 1, and writes the empty
  list in use back; Meguri skips it below 64 items. Both leave an empty list,
  so `fsrs_model_check` and `blob_decode` agree.
- `health-check`'s call 1: the margins of the adjusted log loss and RMSE
  from 1.11 and 1.53 (O6) are not known; the recording stage reports them
  (from a debugging run of the crate, not from the golden) and changes the
  history when either lies within 0.01 of its threshold.
