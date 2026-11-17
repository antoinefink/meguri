# FSRS optimizer ops for scenarios

Spec: conformance/optimizer-ops
Requirements: R692, R694, R695, R696, R697, R700, R1904
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/optimizer/ (planned, not yet on disk: `sched/optimizer#O17`)

Three scenario ops (`dsl.md` D7.2) and one observation kind (`observations.md` O4) for the FSRS optimizer (`sched/optimizer`): `compute_fsrs_params` runs one preset's computation and writes nothing, `optimize_all_presets` is Anki's all-preset save (oracle only), `optimizer_run` is Meguri's optimizer run in a live chain, and `fsrs_model_check` asks whether a model builds from a preset's parameters. The oracle implements all four; Meguri's typed scenario model and executor do not yet, so the rows are draft. A scenario with an op the typed model does not parse is `invalid` (`dsl.md` D10), so the scenarios of `sched/optimizer#O17.3` are held and not yet on disk until the typed model parses these ops; from then on they are `pending` on Meguri until its executor runs them. `conformance/schema/scenario.schema.json` holds their shapes and `protocol.json` lists them for oracle workers.

## P1. `compute_fsrs_params`

### P1.1 Shape

| Argument | Output | Rejections |
|---|---|---|
| `{preset, relearning_steps, form?}` | `{items, reviews, kept, health, eval}` | `invalid_cutoff`, `invalid_search` |

- `preset`: a preset's exact stored name, compared character for character (never under unicase: with presets `Twin` and `twin`, `twin` is the second). `Default` is the default preset. A name no preset has is `unknown_ref`; two presets with the same exact name are `invalid_scenario`.
- `relearning_steps`: an integer from 0 to 4,294,967,295, the relearning step count that training and the adoption rule use (`sched/optimizer#O4`, `#O5`). It is always given: the rule that counts a preset's steps below one day (O4) is Anki's deck options screen, outside its collection library, and Meguri tests it apart (`sched/optimizer#O17.3`).
- `form`: `one_preset` (when missing) or `all_presets`, which of Anki's two default searches the oracle uses (P1.2). Meguri ignores it.
- Any other key or type is `invalid_scenario`. The op takes no logical ids.

### P1.2 The search (oracle)

The search depends on the preset's stored `param_search` (schema-11 `weightSearch`) and on `form`:

| `form` | `param_search` used as stored when | Otherwise, the default search |
|---|---|---|
| `one_preset` | it is not the empty string (white space alone is used) | `preset:"<name>" -is:suspended`, with `\` written `\\` and `"` written `\"` inside the quotes |
| `all_presets` | it is not empty after removing leading and trailing white space (Unicode White_Space) | `(preset:<name> -is:suspended)`, where the term `preset:<name>` has each `"` written `\"` and is put in double quotes only when it holds U+0020, U+3000, `(` or `)` |

These are the two searches of `sched/optimizer#O1`, written as Anki writes them; `sched/optimizer#O2` gives which cards they find for unusual names. A search Anki refuses (a card search raises its search error) is the rejection `invalid_search`.

### P1.3 The cutoff (oracle)

The preset's stored `ignore_revlogs_before_date` is read as `sched/fsrs-items#I1.3` says: the empty text is 0, an accepted text is 00:00:00 UTC of its date in epoch milliseconds, and a text that Anki's ignored-before count refuses (the reading of `sched/memory-state#M5`) is the rejection `invalid_cutoff`. The cutoff is checked before the search, as `sched/optimizer#O3` orders its rules 2 and 3, so a preset with both a refused cutoff and a refused search records `invalid_cutoff`.

### P1.4 What the oracle runs and reports

One `ComputeFsrsParams` request with the search of P1.2, the parameter list in use as `current_params` (`sched/presets#V6`: `fsrs_params_6`, else `fsrs_params_5`, else `fsrs_params_4`, else empty), the cutoff of P1.3 as `ignore_revlogs_before_ms`, `relearning_steps` as `num_of_relearning_steps`, and `health_check` set as Anki's deck options screen reads `fsrsHealthCheck` (`sched/config-keys#G2`: its bool reader, true when missing). Then:

| Member | Value |
|---|---|
| `items` | the response's `fsrs_items`: the item count of `sched/fsrs-items#I7` for the cards the search finds |
| `reviews` | the review count of `sched/fsrs-items#I7`: the `reviews` member of the last parameter-computation progress report the call leaves; 0 when `items` is 0 (the call then reports no progress). Anki reports the count nowhere else |
| `kept` | true when the returned list equals the list in use value for value (an empty list equals only an empty list), false otherwise |
| `health` | `good` or `bad` from the response's `health_check_passed`, or `null` when the response has none (the check runs only above 300 items with `health_check` on, `sched/optimizer#O6`) |
| `eval` | `{log_loss}`: the f32 `log_loss` of `EvaluateParamsLegacy` for the returned list with the same search and cutoff, or `null` when `items` is 0 |

- `eval` carries no `rmse_bins`: the crate sums its RMSE bins in hash-map order, so the last bits of that value differ between two calls on the same items, and a golden must repeat exactly. `log_loss` is summed in item order and repeats.
- The legacy evaluation reads the rows of the cards the search finds. With a search that finds the whole collection (white space alone, `one_preset`), training also reads the rows of cards that no longer exist and the evaluation does not, so `eval` is then not on the training items; the planned `sched/optimizer/weight-search` covers that step with deviation `optimizer-blank-search-default` (`sched/optimizer#O16`).
- The op writes nothing. Its first day query on a collection that has none writes `rollover`, as any day query does.

### P1.5 Meguri

O2's card set for the preset (by preset id; `form` ignored), O4's training with the given count, O5's adoption rule with the same count, and O6's health check, without O3 rule 4's minimum, the job, the gate or a commit (`sched/optimizer#O7` to `#O9`). With no item it returns the list in use without training. `items`, `reviews` and `kept` as in P1.4; `eval` the `log_loss` of the crate's evaluation of the returned list on the same items. A refused cutoff and a refused search are the two rejections (O3 rules 2 and 3); a crate error in training, evaluation or validation (O3 rule 5) is a run error.

## P2. `optimize_all_presets` (oracle only)

| Argument | Output | Rejections |
|---|---|---|
| `{deck}`: a deck's stored human name | none (`{}`) | none |

Anki's "Save and optimize all presets" on the deck options screen of `deck` (`sched/optimizer#O1`, second column): one deck options save in the compute-all mode (`UPDATE_DECK_CONFIGS_MODE_COMPUTE_ALL_PARAMS`) with the deck's preset as stored (Anki's preset read, not the screen's loading, which fills an empty FSRS-6 list from older ones), FSRS on, rescheduling off, the deck's limits as the screen leaves them (`deck-options-ops.md` E1) and the other collection-wide values as read. Anki computes every preset, sets each one's `fsrs_params_6` to its result, writes `lastFsrsOptimize` and then saves as usual, all as one "Deck Options" undo step. A save Anki refuses (a refused cutoff in any preset) is a run error. Meguri has no such save (R691), so only reference scenarios (`dsl.md` D9.4) and the oracle's side of P3 use it. Observe lists after it only for presets whose lists it does not train (`sched/optimizer#O17.1`).

## P3. `optimizer_run` (live chains)

| Argument | Output | Rejections |
|---|---|---|
| `{}` | none (`{}`) | none |

- **Meguri:** one run of U47 in local-only mode, committed to its end, as `sched/optimizer#O17.1` describes: the adapter first passes the policy `Lifecycle{Ready}` and the `OptimizerCadence` the core reports at `Ready`, then `OptimizeNowRequested` only when those inputs started no run (a due cadence starts `StartOptimizer{automatic}` at once, `sched/optimizer#O12` rules 2 and 3); any end other than `Done`, and any refusal, is a run error.
- **Oracle:** P2 on the default deck (id 1). The golden of a live chain is the oracle running every segment (`dsl.md` D9.2), so it holds Anki's save in Meguri's place, and a chain observes only what both runs give (no parameter value, memory state or card stamp).

## P4. `fsrs_model_check` (observation)

| Arguments | Value |
|---|---|
| `{presets: [<name>, ...]}`: exact stored names, each once, at least one | `{<name>: "ok" \| "invalid_params"}` |

Whether an FSRS model builds from the preset's parameter list in use now (`sched/presets#V6`; R696). It never changes the collection.

- **Oracle:** Anki's legacy evaluation of that list on a search that finds no card. Anki builds the model before it looks at the items, so its invalid-parameters error is `invalid_params`, and its not-enough-history error (the model built, no item) is `ok`. Any other outcome is a run error.
- **Meguri:** U45's check (`sched/fsrs-policy#U1`'s construction rule): `ok` when the crate's model constructor accepts the list, `invalid_params` otherwise. U2's later-list rule is not part of this check.

## P5. Comparison and scenario rules

- `items`, `reviews`, `kept` and `health` compare exactly; `eval`'s `log_loss` within 0.002 absolute (`observations.md` O7). Raw parameter values never reach a golden.
- `kept` decides on a strict loss comparison, and the op reports only the returned list's loss, so a scenario keeps every call away from a tie: either fewer than 20 first items per first rating, so that the crate returns its defaults and an empty list in use has the same loss, or a history whose every item trains (`sched/optimizer#O3`, `#O17.2`).
- `health` compares exactly, so a scenario's adjusted values (`sched/optimizer#O6`) keep at least 0.01 from 1.11 and 1.53; the planned `sched/optimizer/health-check` records its margins in its comments.
- `lastFsrsOptimize` holds the day number, which is 0 on the day the collection was made: a scenario that reads it advances the clock by whole days first, so the value it observes differs from 0.
