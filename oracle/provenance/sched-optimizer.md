# FSRS optimizer (U47)

Spec-writer notes for `specs/sched/optimizer.md` (O1 to O6 and O17) and the
inactive scenarios under `conformance/scenarios/sched/optimizer/`. Read at tag
26.09.3 (`refs/anki-26.09.3`) and the `fsrs` crate at its `v6.6.2` tag
(`refs/fsrs-rs`, read with `git show v6.6.2:<path>`: the checkout is past the
tag); no code was copied. The first pass did not run the oracle; the
recording pass of 2026-11-10 added the ops of O17.1 to the driver
(`driver/ops/optimizer.py`, `specs/conformance/optimizer-ops.md`) and
recorded the eight goldens, which confirm the findings listed at the end.

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

Findings the spec relies on, as the goldens of 2026-11-10 show them:

- The short-term check on the default parameters keeps the empty list in
  use at a count of 2 and returns the defaults at 40 (`relearning-check`:
  `kept` true, then false).
- The item builder reads every row of every card the search finds, whatever
  the card's `type` or `queue` (`items`: c8 gives 6 items, c9 in a filtered
  deck 6 items and 9 reviews).
- With the 24 cards of `adopt`, every call returns a trained list (`kept`
  false at counts 1 and 2 and with the `fsrs6-c` list in use), with the same
  log loss each time; 240 items and 264 reviews. Training repeats exactly:
  two compare runs matched the recording.
- `live-adoption`: after Anki's all-preset save in Meguri's place, every
  preset's model builds and its blobs decode, no card's scheduling columns
  and no review-log row changed, and `lastFsrsOptimize` is 10, the day
  number after the scenario's `advance: 10d`.
- `health-check`'s call 1 is `good`. A one-off run of Anki's time-series
  evaluation (the `EvaluateParams` call, same search, cutoff and count; not
  kept in the driver) gave log loss 0.4470 and RMSE(bins) 0.1023 with 274 of
  the 301 items' last ratings above 1: adjusted log loss about 1.639 (0.53
  above 1.11) and adjusted RMSE about 1.061 (0.47 below 1.53).
- `preset-names` gives O2's name table on both paths exactly as the scenario
  comment lists it (`Trail\` refused on the all-preset path; `Tab<TAB>Split`
  gets the 6 items of Tab's `Split` card; `twin` gets Twin's card), and the
  all-preset save empties `Trail\`'s FSRS-5 list.
- `weight-search`: 12, 18, 12 and 30 items, then the refused search.

## Driver (`specs/conformance/optimizer-ops.md`)

| Section | Files read | Read for |
|---|---|---|
| P1.2 | `rslib/src/search/writer.rs` (the term writer: double quotes escaped, quoted when the text holds a space, U+3000 or a parenthesis), `rslib/src/search/builder.rs` (joining with grouping wraps the two terms in parentheses), `rslib/src/deckconfig/update.rs` (the all-preset default search), `ts/routes/deck-options/FsrsOptions.svelte` (the one-preset default search and its untrimmed test) | the two default searches the driver writes |
| P1.3 | `rslib/src/deckconfig/service.rs` (the ignored-before count reads the date before it searches), `rslib/src/scheduler/fsrs/params.rs` (the date reading) | why a count on a search that finds nothing checks the cutoff text alone |
| P1.4 | `rslib/src/scheduler/fsrs/params.rs` (the progress thread's review count; the legacy evaluation's card search), `rslib/src/progress.rs` (a new progress handler resets the stored report, so the count is read before the evaluation), `rslib/src/scheduler/service/mod.rs` (the request members), fsrs `src/inference.rs` at `v6.6.2` (RMSE bins summed from a hash map, whose order differs between calls) | `reviews`, and why `eval` has no `rmse_bins` |
| P2 | `rslib/src/deckconfig/update.rs` (compute-all mode, `println!` of each preset's result, `lastFsrsOptimize`), `proto/anki/deck_config.proto` | the save and why the driver points fd 1 at `/dev/null` for it |
| P4 | fsrs `src/inference.rs` at `v6.6.2` (evaluation of no item is not-enough-data), `rslib/src/scheduler/fsrs/params.rs` (the legacy evaluation builds the model before it evaluates), `rslib/src/scheduler/fsrs/error.rs`, `rslib/src/error/mod.rs`, `rslib/src/backend/error.rs` (both errors are the invalid-input kind, told apart by their translated messages) | the model check |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| sched/fsrs-items I3 training form and I7 (cutoff cases, counts, order); planned scenarios sched/optimizer/cutoff-learning-start, cutoff-splits-learning, training-item-counts (not yet on disk; held until the typed model parses compute_fsrs_params) | `rslib/src/scheduler/fsrs/params.rs` (tests on the cutoff and training-item alignment; the training-item builder and the parameter computation request flow) | Which behaviors the harvested tests require: a learning start before the cutoff drops the card, a cutoff inside the learning run drops it, an instant exactly at the cutoff keeps it; items are sorted by review id with card ids aligned; the review count. Also why order and alignment reach only the fsrs crate's input and no RPC exposes them. I also looked at the fsrs 6.6.2 crate (not Anki) to confirm that card ids drive recency weighting. Golden values come from the oracle. |
