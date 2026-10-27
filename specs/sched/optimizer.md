# FSRS optimizer

Spec: sched/optimizer
Requirements: R691, R692, R693, R694, R695, R696, R697, R698, R699, R700, R701, R1850, R1852, R1904, R1905; supports R704, R1468, R1486, R2052; AE28
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/optimizer/ (not written yet: O17)
Status: Under review (R2066) for O12 and O13. Do not implement them until approved. Every other section is in force.

How Meguri re-optimizes each preset's FSRS parameters (U47): which presets and cards it trains on, when it adopts a result, what the adoption writes, when a run starts, and how a run stays safe beside review sessions and CloudKit. The training items are `sched/fsrs-items#I7`; the memory-state refresh is `sched/memory-state#M4`; the preset view is `sched/presets`. "Optimize" here is FSRS parameter fitting only, not the SQLite optimize of U132 (`maintenance/optimize-integrity`).

Terms from other specs:

- home deck, home preset, parameter list in use, decay: `sched/presets#V1`, `#V6`; desired retention in force: `#V7`;
- unsupported preset: `sched/fsrs-policy#U1`, `#U2` (U45's `UnsupportedParams`, `sched/fsrs-transitions#T13`);
- today's day number: the day query of `sched/day-clock#D5` (`days_elapsed`); `next_day_at`: `sched/fsrs-items#I1.2`;
- ops, sources, snapshots and preconditions: `engine/op-model` O1, O6, O7; jobs, cancel tokens and progress: `engine/jobs` J1, J4, J5;
- the CloudKit policy, its inputs, actions and gate: `sync/policy` P3, P4, P6, P17.

## O1. Anki's two optimizer paths (reference)

Anki 26.09.3 optimizes only when the user asks, from the deck options screen. It has two paths, and they differ:

| | Optimize (one preset, the screen's button) | Save and optimize all presets |
|---|---|---|
| Presets | the preset being edited | every row of `deck_config`, the screen's current preset last |
| Card search | `param_search` when it is not the empty string, else the text `preset:"<name>" -is:suspended` with `\` and `"` in the name escaped with `\`. White space alone counts as a search: it parses as the whole collection (`search/grammar#G1`), and training then reads every review-log row, rows of cards that no longer exist included | `param_search` when it is not empty after trimming white space, else the same two terms written by Anki's search writer, which escapes `"` but not `\` |
| Relearning step count passed to training | the leading relearning steps whose running total stays below 1440 minutes (O4) | the number of relearning steps |
| Health check | when `fsrsHealthCheck` is on and the preset has more than 300 items | never |
| Minimum item count | none (the crate's own fallbacks, O4) | none |
| A preset whose computation fails | error dialog | the preset keeps its list; the run goes on |
| Cutoff date | the screen turns the stored text into milliseconds with the web view's date parser, not with `sched/fsrs-items#I1.3`: the two parsers accept different texts (that parser takes forms such as `2024/01/05`, which I1.3 refuses) and may read one text as different instants; a text it cannot read is an error | read with `sched/fsrs-items#I1.3`; a refused text fails the whole save, nothing written |
| Result | the screen shows the new list; the user must save | every preset's `fsrs_params_6` is set to the result, which is the list in use when nothing better was found, so a preset with only an FSRS-5 list gets that list copied into `fsrs_params_6`; then the deck options save runs as usual |
| `lastFsrsOptimize` | not written | set to today's day number |
| Memory states | the save recomputes decks whose list in use changed, and reschedules them when the screen's "Reschedule cards on change" is on | the same |
| Undo | the save is one undoable step, "Deck Options" | the same |
| Cancel | aborts training; nothing is written | aborts the whole save; nothing is written |

Both paths use the same training items (`sched/fsrs-items#I7`), the same training settings apart from the relearning step count, and the same adoption rule (O4, O5). The screen warns when `lastFsrsOptimize` is positive and more than 30 days before today.

Meguri has no optimizer UI (R691) and runs one automatic all-preset path. It follows the one-preset path where the plan names that path's rules (R694, R700) and decides the rest below; O16 lists the differences.

## O2. Presets and their card sets (R694; settles `sched/fsrs-items#I7`)

- A run visits every row of `deck_config` in ascending `id` order. `n` in the progress (O15) is that count.
- **Default card set.** When the preset's `param_search` (schema-11 `weightSearch`) is empty after removing leading and trailing white space (Unicode White_Space), the set is the cards whose `did` or `odid` is a normal deck whose preset id is this preset's `id`, and whose `queue` is not -1 (suspended). Buried cards, new cards and cards in filtered decks (through `odid`) are in the set; new cards give no item.
- **Custom card set.** Otherwise U80 runs the stored `param_search` text as stored (untrimmed), and the set is the cards it finds. A text U80 refuses skips the preset (O3).
- The rows read are those of `sched/fsrs-items#I1.1` for the set's cards. Review-log rows of cards that no longer exist are never read.
- Decided 2026-10-27: Meguri selects the default set by preset id, not through a `preset:` search text. Anki's two paths both find a preset by name with the escapes still in the value (`search/names-and-ids#N3.6`), so Anki's set differs in these cases:

  | Preset name | Anki, one-preset path | Anki, all-preset path | Meguri |
  |---|---|---|---|
  | holds `"` | no card | no card | the preset's cards |
  | holds `\`, not as its last character | no card | the preset's cards | the preset's cards |
  | ends in `\` | no card | search refused: the preset keeps its list | the preset's cards |
  | holds white space other than U+0020 (a tab, a line break, U+00A0, U+3000: the set of `search/grammar#G1`) | no card | no card | the preset's cards |
  | equal under unicase to an earlier preset's name | the earlier preset's cards | the earlier preset's cards | the preset's cards |

  The white-space row holds because the search parser turns every such character into U+0020 before it parses, inside quotes too (`search/grammar#G1`), so the value no longer equals the stored name. When the name with U+0020 in those places is another preset's name, Anki finds that preset's cards instead (the unicase row). Training a preset on its own cards is the purpose of the default search, and a name never changes which cards a preset schedules. For every other name the sets are equal. The all-preset column is read from Anki's search writer and parser; scenario `sched/optimizer/preset-names` (O17) confirms it on the oracle. The count of `sched/memory-state#M5` already counts such a preset's cards (`sched/memory-state#M10`).
- Decided 2026-10-27: white space alone in `param_search` means the default set, as in Anki's all-preset path. Anki's one-preset path trains on the whole collection's review log instead, rows of cards that no longer exist included (O1); Meguri never reads those rows (above, O16). The M5 count reads it as a search (`sched/memory-state#M5`); the two features stay as Anki has them.
- `sched/fsrs-items#I7`'s card-set bullet states this section's default set (O18 item 2, made).

## O3. Presets a run skips

A preset is checked in this order. The first rule that applies records its outcome (O14), and the preset is not written; its cards keep their rows. The run goes on with the next preset.

| Order | Condition | Outcome |
|---|---|---|
| 1 | the preset is unsupported (R704): nothing of it is read, built, trained or written | `unsupported` |
| 2 | its stored `ignore_revlogs_before_date` is refused (`sched/fsrs-items#I1.3`) | `invalidCutoff` |
| 3 | U80 refuses its `param_search` | `searchFailed` |
| 4 | fewer than 64 training items (R693): the item count of `sched/fsrs-items#I7` | `tooFewItems` |
| 5 | the crate fails in training (O4) or in the adoption rule (O5), or the adopted list fails validation (O5); a failed health check is no failure (O6) | `failed` |

- Decided 2026-10-27: a refused cutoff skips that preset only. Anki's all-preset save fails whole; skipping keeps the other presets' results and writes nothing for the broken one. The typed error of `sched/fsrs-items#I1.3` names the preset in the outcome.
- The 64-item minimum is Meguri's (R693). Anki has none: with fewer than 8 usable items the crate returns its default parameters, with fewer than 64 it fits only the first four, and the adoption rule then compares them.

## O4. Training (R694)

For a preset that passes O3, the items, card ids and counts are `sched/fsrs-items#I7`'s, with `next_day_at` of the run's start (O8). The crate call is `compute_parameters` with this `ComputeParametersInput`:

| Member | Value |
|---|---|
| `train_set`, `card_ids` | the preset's items and card ids, in I7's order |
| `enable_short_term` | true |
| `num_relearning_steps` | the relearning step count below |
| `training_config` | `TrainingConfig` with `num_epochs` 8 and every other member at the crate's default (the crate's default epoch count is 5) |
| `progress` | a `CombinedProgressState`; the run's cancel token sets its `want_abort` (O15) |

**Relearning step count.** Walk the preset's `relearn_steps` (minutes, `sched/presets#V5`) in order, adding each step to a running total computed in f64 from the stored f32 values. The walk stops at the first step whose running total, that step included, is 1440 or more; that step and those after it do not count, and every step before it counts. A NaN total is never 1440 or more, so a NaN step counts and the walk goes on (the total stays NaN, so every later step counts too).

| `relearn_steps` | Count |
|---|---|
| `[]` | 0 |
| `[10]` | 1 |
| `[10, 60]` | 2 |
| `[10, 1440]` | 1 |
| `[1440]`, `[2880, 10]` | 0 |
| `[1000, 439.5, 0.5]` | 2 (1000, 1439.5; the third reaches 1440) |
| `[10, NaN, 2000]` | 3 (the totals from the second step on are NaN) |

The same count goes to the adoption rule (O5).

- Training runs on the optimizer's own rayon pool, built through the thread registry at utility QoS (`engine/jobs#J2`, `#J6`); nothing uses rayon's global pool (KTD8).
- A preset's items are dropped before the next preset's are built (R1852, `sched/fsrs-items#I7`). What a run keeps between presets is only each preset's result (a list of 21 values or none), its outcome, counts and health result.

## O5. The adoption rule (R695, R1905)

Let `current` be the preset's parameter list in use (`sched/presets#V6`; empty means the crate's defaults, as `FSRS::new` builds them) and `new` the list `compute_parameters` returned. On the preset's items:

1. `loss(current)` and `loss(new)` are the `log_loss` members (f32) of `FSRS::evaluate` on all of the preset's items, with each list's model.
2. When `loss(new)` < `loss(current)`, adopt `new`.
3. Otherwise, when the relearning step count (O4) is 0 or 1, keep `current`.
4. Otherwise run the short-term check on `current`'s model: from the memory state (stability 1.0, difficulty 1.0), take the Again state of `next_states` with desired retention 0.9 and 2 elapsed days; from it, take the Good state of `next_states` with retention 0.9 and 0 elapsed days, as many times as the relearning step count. When the stability reached is below 1.0, keep `current`; otherwise adopt `new`.

- Step 4 is Anki's: `current` lets stability grow through a lapse's same-day relearning steps, a fault that log loss does not measure, so a list trained with that step count replaces it. Decided 2026-10-27: Meguri keeps step 4 (R695 names it, and R1904 compares results with the oracle's). For R1905, a list "fits better" when step 2 or step 4 adopts it; the R1905 test shows that a `new` with `loss(new)` ≥ `loss(current)` is never adopted when step 3 applies, or when the check of step 4 finds a stability below 1.0. Plan correction: O18.
- **Validation (R696).** An adopted list must have 21 values, all finite, and `FSRS::new` must accept it (the crate that Anki 26.09.3 uses is the same 6.6.2). A list that fails is not adopted; the outcome is `failed`.
- A crate error in step 1 or step 4 fails the preset (`failed`). The case where `current` cannot be built does not occur: such a preset is unsupported (O3 rule 1). Anki adopts `new` without comparing in that case.
- Outcomes: `adopted` or `notBetter` (steps 3 and 4 keeping `current`).

| Relearning step count | `loss(new)` vs `loss(current)` | Short-term check on `current` | Result |
|---|---|---|---|
| any | lower | not run | adopted |
| 0 or 1 | equal or higher | not run | `notBetter` |
| 2 or more | equal or higher | stability below 1.0 | `notBetter` |
| 2 or more | equal or higher | stability 1.0 or above | adopted |

## O6. The health check (R700)

- It runs for a preset that trained (O4, whatever O5 decides) when `fsrsHealthCheck` reads on (`sched/config-keys#G2`: bool reader, default true) and the preset has more than 300 items. Otherwise the preset has no health result.
- The crate call is `evaluate_with_time_series_splits` with the same `ComputeParametersInput` as O4. Its progress callback returns false once the cancel token is set.
- With `c` the item count, `r` the share of items whose last review has a rating above 1, `L` and `E` the `log_loss` and `rmse_bins` it returns, all in f32:
  - adjusted log loss = `L` / (0.623 × (4 × `r` × (1 − `r`))^0.738);
  - adjusted RMSE = `E` / (0.0135 / (`r`^0.504 − 1.14) + 0.176 / ((`c` / 1000)^0.825 + 2.22) + 0.101);
  - the result is `good` when the adjusted log loss is at most 1.11 or the adjusted RMSE is at most 1.53, and `bad` otherwise.
- A crate error gives no result; it does not fail the preset. An `Interrupted` error is the run's cancellation (O15).
- The result is recorded for the diagnostics view (O14). Meguri shows no dialog (R700) and the result never changes the adoption.

## O7. The job (R698)

One optimizer job at a time per engine (KTD9). States:

`Idle → Training{preset k of n} → AwaitingGate{jobId, adopted} → Committing → Done{adopted, skipped}`, and the ends `Cancelled` and `Abandoned{reason}`, with reasons `inputChanged`, `sessionActive`, `superseded` and `gateFailed`.

| Call | Effect |
|---|---|
| `optimizer_start(trigger, guard)`, trigger `automatic` or `optimizeNow`; `guard` is the P4 guard of its `StartOptimizer` (O12) | refuses, writing nothing, with the first that applies: `staleGuard` (the guard is not current, `sync/policy#P4`), `notReady` (lifecycle not `Ready`), `fsrsOff` (`fsrsPolicyStatus()` is not `ok`, `sched/fsrs-policy#S1`), `sessionActive` (a review session is admitted, `sched/review-session#C2`), `running` (a job is in `Training` or `Committing`), `notDue` (trigger `automatic` and O11's cadence is not due). A job in `AwaitingGate` is ended `Abandoned{superseded}` first. Otherwise starts a J1 job and returns its handle |
| `optimizer_commit(jobId, guard)`; `guard` is the P4 guard of its `OptimizerGate` (O12) | for a job that is not in `AwaitingGate` (unknown, `Committing` or ended): refuses with `notAwaitingGate`, writes nothing and publishes no end. Otherwise it moves the job to `Committing` at once, before it queues the op, then runs O8's checks and O9's op; the job ends `Done` or `Abandoned{reason}`. A guard that is not current, at the call or at O8's checks, ends the job `Abandoned{inputChanged}` with nothing written, and the call returns `StaleGuard` |
| `optimizer_abandon(jobId)` | ends a job in `AwaitingGate` as `Abandoned{gateFailed}`; writes nothing. For a job not in `AwaitingGate` (a `Committing` job included): refuses with `notAwaitingGate` and changes nothing |
| `cancel_job(jobId)` (`engine/jobs#J1`) | O15 |
| `optimizer_diagnostics()` | O14 |

- The training job finishes (`JobFinished{Succeeded}`) when it reaches `AwaitingGate`; its results stay in the engine, keyed by job id, until `optimizer_commit` or `optimizer_abandon` ends them. Results are kept in memory only: a close, a replacement or a relaunch drops them, and the next run starts again.
- Every run that is not cancelled reaches `AwaitingGate`, also when no preset was adopted: its commit still writes `lastFsrsOptimize` (R692).
- Each end publishes `OptimizerEnded{jobId, outcome}` on the change stream, and the core reports it to the policy (O12). An end that follows the commit op is published and reported only after that op's observers have run (`engine/change-stream#S2`), so the policy gets the commit's `TrackingChanged` before it (O12 rule 6).
- The optimizer takes no long-operation slot (`engine/lifecycle` L9 table).

## O8. The revision gate (R699, KTD8)

At start the job records, before reading anything: the collection generation; the binding (`sync/policy#P1`: its id, its epoch, and its state, `active` or `isolated` with the V2.2 reason) or none; today's day number and `next_day_at`; and the seq `S` of its first snapshot (`engine/op-model#O6`). Each preset's items are read from a snapshot of their own, taken when that preset's turn comes, so no read transaction stays open during training. All snapshots are at `S` or later.

`optimizer_commit` runs its checks inside the commit op's write transaction, before anything is written. The job ends `Abandoned{inputChanged}` and nothing is written when any of these holds:

- the generation differs (a replacement, a close and reopen);
- the binding differs from the recorded one (another binding, a first connection, a disconnect), or its state or isolation reason does (an isolation such as K18.4's `epoch_switched`, which keeps the id and epoch, or a V9.2 Resume);
- the guard of the `OptimizerGate` that called the commit is not current (`sync/policy#P4`; O7): this ends the job as above, and the call returns `StaleGuard`;
- today's day number differs;
- an op with a seq above `S` changed a row of `revlog`, `cards`, `decks` or `deck_config`, or the config row of `fsrs`, `lastFsrsOptimize`, `fsrsHealthCheck`, `rollover`, `creationOffset` or `localOffset`: the preconditions `TableUntouched{table, S}` and `Untouched{config, key, S}` (`engine/op-model#O7`). Any source counts: local edits, answers, `CloudIncoming` applies (the gate's fetch included) and maintenance.

It ends `Abandoned{sessionActive}` when a review session is admitted, or was admitted at any time since the job started, even if that session has ended (U43's session-start hook marks the job, O10).

- A failing precondition here ends the job; it never re-plans (`engine/op-model#O6`'s retry does not apply).
- Since every snapshot is at `S` or later and nothing in the input changed after `S`, all presets were trained on one state of the collection.
- The core never calls the network for the optimizer. A CloudKit fetch before the commit is the policy's (O12); a fetch is not a lock (O13).

## O9. The commit (R692, R696, R697)

One op, source `Optimizer` (`engine/op-model` O1), catalog entry `optimizer_commit`: not destructive, not undoable, long write, write cancellable between batches (`engine/op-model` O9; O18 lists the entry). In its one transaction:

1. For each adopted preset: `fsrs_params_6` is set to the adopted list through the optimizer's setter (`decks/deck-presets#P10`). Nothing else in the record changes: `fsrs_params_5`, `fsrs_params_4`, later lists, `other` and unknown fields keep their bytes (R1486, `conformance/normalized-dump#N11.5`). The record is stamped (`usn` -1, `mtime_secs` now); a list equal to the stored one writes nothing.
2. For each adopted preset: the memory states of its cards are refreshed with `sched/memory-state#M4` (caller `#M7.3`): the cards whose home preset is this preset, processed as M4.1 says (suspended and buried included, new cards not), with the new list. Only `s`, `d`, `dr` and `decay` change, and only on cards whose values change (M4.2). No due date, interval, queue, type or review-log row changes (R697).
3. `lastFsrsOptimize` = today's day number (config write, `storage/config-table#C4`), also when no preset was adopted.

- Undo: source `Optimizer` clears the undo history (`engine/undo#M2`, reason `ClearedByNonUndoableOp`). No session is open at the commit (O8), so no answer loses its undo.
- Cancel between batches (suspend, O15; a session start, O10) rolls the whole op back: nothing is written and the job ends `Cancelled` or `Abandoned{sessionActive}`.
- Progress: phase `Committing`, cards visited out of the cards step 2 selects.
- Result `Done{adopted, skipped}`: the adopted preset ids, and every other preset id with its O3 or O5 outcome.
- Unsupported presets and their cards keep their bytes (`sched/fsrs-policy#U4`, K3 step 4).

Example (AE28, core part): `lastFsrsOptimize` 31 days before today; preset A trains but `notBetter`; preset B is adopted. Training ends in `AwaitingGate` with nothing written. `optimizer_commit` with nothing changed since `S`: A's record is not written; B's `fsrs_params_6` holds the new list; B's cards with changed values have new `s`, `d` and `decay` (the new list's decay, `sched/presets#V6`); no card's `due`, `ivl`, `queue` or `type` changes; `lastFsrsOptimize` = today.

## O10. Review sessions (R699)

- `startSession`'s optimizer hook (`sched/review-session#C2` step 2) requests cancellation of a job in `Training` without waiting: the job ends `Cancelled` and writes nothing. It marks a job in `AwaitingGate`, or in `Committing` whose op has not started (O7 moves the job before it queues the op), whose commit then ends `Abandoned{sessionActive}` (O8).
- **Submission hook.** The commit op holds the writer, and C2's admission (step 1) runs on the writer, so a hook after admission would make the start wait for the whole commit. So `startSession` has a second optimizer hook, at submission, before the start is queued for the writer and before admission: when an optimizer commit op runs, it requests that op's cancellation without waiting, and does nothing else (it marks nothing and cancels no training; C2 step 2 does that after admission). The commit rolls back at its next batch boundary and ends `Abandoned{sessionActive}`; then the writer takes the start and C2 runs as usual. So a session never waits for more than one commit batch.
- Decided 2026-10-27: the submission hook fires for every submitted start, also one that admission then refuses (`FsrsPolicyPending`, `PresetUnsupported`). Admission can be decided only on the writer, after the commit, so predicting the refusal would mean waiting for the commit. A refused start therefore still costs that run: nothing is written, and the next arming input (O12 rule 2) runs it again. Data stays safe either way, since a cancelled commit writes nothing. C2 needs this hook as a new step before step 1; O18 lists the correction for its owner.
- `optimizer_start` refuses while a session is admitted (O7). The policy starts no run during a session (O12).
- Answers never wait for training: training reads snapshots and writes nothing.

## O11. Cadence (R692)

- The core computes **due**: `lastFsrsOptimize` (i32 reader, `sched/config-keys#G2`) is 0, or today's day number minus it is 30 or more (computed in i64). A value above today is not due. A value that does not read as i32 reads as 0 (never optimized), so it is due.
- The core reports `OptimizerCadence{due, fsrsOk}` to the policy (O12) when the collection becomes `Ready`, after every commit that changes `lastFsrsOptimize` or `fsrs` (local or `CloudIncoming`), and at every day rollover (`sched/day-clock#D9`). `fsrsOk` is `fsrsPolicyStatus()` = `ok`.
- Trigger `automatic` needs due; `optimizeNow` (R701's Optimize now) bypasses only due (O7).
- After one device's commit syncs, `lastFsrsOptimize` holds that device's day number on every device, at most a day or so from each device's own, so the others are not due (R692).

## O12. CloudKit and the policy (R699; the optimizer branch of `sync/policy#P18`)

The optimizer branch of the CloudKit policy. It lives with the policy in `core/crates/meguri-sync/src/cloud/policy/` (O18) and keeps the policy pure (`sync/policy#P2`). U47's app-side `OptimizerCoordinator` executes its actions; nothing else starts the optimizer (KTD5).

**Modes**, read from the policy state (`sync/policy#P5`) only; the first row that matches gives the mode:

| Mode | When |
|---|---|
| local-only | row 1 of `sync/policy#P14.2` holds: iCloud not enabled on this device, the sidecar decision local only, the binding isolated `stopped`, or every binding of another container or environment |
| connected, auto | a binding, every condition of the gate holds (`sync/policy#P6`), Sync automatically on |
| connected, manual | a binding, every condition of the gate holds, Sync automatically off |
| blocked | otherwise |

Decided 2026-10-27: attention items (`sync/policy#P14.4`) and the status words do not set the mode. An item that leaves the gate open (`conflicts`, `quotaFull`, `oversize`, `mediaMissing`, `exchangeIncomplete`, `cloudError`, …) leaves the mode connected; the status `paused{automaticOff}` is connected, manual; `paused{userPaused}`, a fence, a decision required, an isolated binding other than `stopped` or `environment_changed`, a `crt` mismatch, a scan and every other closed gate are blocked. So the conflict copies of O13's accepted race never stop the optimizer.

**Inputs** added to `sync/policy#P3`:

| Input | Source | Carries |
|---|---|---|
| `OptimizerCadence{due, fsrsOk}` | the core (O11) | |
| `OptimizerJob{jobId, state}` | the core: `Training` at start, `AwaitingGate`, `Ended{outcome}` | the job id and state; the core does not know the policy's mode, which the policy keeps (State, below) |
| `OptimizeNowRequested` | Swift: the diagnostics view's Optimize now (R701) | |
| `OptimizeNowExpired` | `OptimizerCoordinator`: the deadline of a pending Optimize now (rule 4) passed | |
| `OptimizerStartRefused{trigger, reason}` | `OptimizerCoordinator`, when `optimizer_start` refuses (O7) | the trigger and O7's refusal |
| `ProcessingTaskLaunched{optimizer}` | Swift: the iOS and iPadOS processing task's handler (U154 slot) | |

**Actions** added to `sync/policy#P4`. Each carries P4's guard. `EndProcessingTask` is `SyncCoordinator`'s, as are the `RequestFetch` and `RequestSend` the rules emit; `OptimizerCoordinator` executes the others:

| Action | Effect |
|---|---|
| `StartOptimizer{trigger}` | `optimizer_start(trigger, guard)`; a refusal comes back as `OptimizerStartRefused` |
| `OptimizerGate{jobId}` | `optimizer_commit(jobId, guard)`. The action returns once the call has moved the job to `Committing` or refused; the end arrives as `OptimizerJob{Ended}` |
| `AbandonOptimizer{jobId}` | `optimizer_abandon(jobId)` |
| `CancelOptimizer{jobId}` | `cancel_job(jobId)`: a job in `Training` or `AwaitingGate` ends `Cancelled`, a `Committing` job's op is cancelled and the job ends `Cancelled`, and an ended job is left as it is (O15). Nothing is written |
| `OptimizeNowRefused{reason}` | the view shows the reason |
| `OptimizerDueChanged{due}` | U154: with `due` set when the app moves to the background, submit one optimizer processing request (network required, external power preferred); with it clear, none |
| `EndProcessingTask{optimizer}` | U154: marks the optimizer processing task completed, when one runs |

- `AbandonOptimizer` and `CancelOptimizer` act whatever the guard: ending a job writes nothing. `optimizer_start` and `optimizer_commit` refuse a guard that is not current (O7, O8), so a binding change, an isolation, a resume, an account change or a replacement between the emission and the call starts or commits nothing.
- **Order.** `OptimizerCoordinator` runs the optimizer actions of an action list after the list's core actions (`sync/policy#P4`), in list order, and does not wait for `SyncCoordinator`'s actions of that list, which skip them. So the `CloudIncoming` ops that an `ApplyReady` of the same list submits are queued before the commit op, and the background queue commits them first (`engine/op-model#O2`); O8's checks then see them. `EndProcessingTask` runs in `SyncCoordinator`'s order, so in rule 6's list it runs after the `RequestSend` has returned (P4: the rest of the list runs in the send's task).
- **The optimizer's fetch call.** A `RequestFetch{root, collection}` that a rule below emits carries a call id (`call`) from a counter of the optimizer state. When the action list already holds a `RequestFetch{root, collection}` (the P10.1 opening rows, P10.1's `DeviceSettings{automatic: on}` row, P11.1's first row), the rule gives that one the call id instead of adding a second; two rules in one list share it. U114 copies the call id onto each `FetchDrained` it emits for that `fetchChanges` call or for P4's one repeat of it; every other drain (another action's call, an exchange's fetch, a fetch the engine scheduled itself) carries none. A drain that carries the id read the database changes after the call started (`sync/policy#P3` `FetchDrained`), and P4 runs one call of an instance at a time, so it cannot be the drain of a fetch that started before the rule asked.
- **Deadlines.** `OptimizerCoordinator` holds two deadlines, each of one operation (`sync/policy#P2`): Optimize now's (rule 4) and the gate's (rule 5). It starts the gate's when the job it started reaches `AwaitingGate` (the job's progress, `engine/jobs#J5`, O15), which is the input at which the policy emits the gate's fetch, and disarms it when it executes `OptimizerGate` or `AbandonOptimizer` for that job or the job ends. When it passes still armed, it calls `optimizer_abandon`; a job that `optimizer_commit` has moved to `Committing` is refused there and goes on (O7).

**State** added to `sync/policy#P5`, in memory: the platform (macOS, or iOS and iPadOS; fixed when the policy is created); cadence (`due`, `fsrsOk`); the job id and the policy's job state: `starting` (from `StartOptimizer` until `OptimizerJob{Training}` or `OptimizerStartRefused`), `Training`, `AwaitingGate`, `Committing` (set when the policy emits `OptimizerGate`), `ending` (set when it emits `AbandonOptimizer` or `CancelOptimizer`), cleared at `OptimizerJob{Ended}`; the mode the job started in (recorded when the policy emits `StartOptimizer`, cleared with the job or at `OptimizerStartRefused`); `armed`, `drainedSinceArmed` and the arming call id; a pending Optimize now and its call id; the gate's call id and its job id (cleared when the policy emits `OptimizerGate`, `AbandonOptimizer` or `CancelOptimizer` for that job, or at its end); the call-id counter; and whether a processing task runs. After a relaunch all are empty. An empty cadence reads as `due` false and `fsrsOk` false. "A job" in the rules below means any job state above.

**Rules.**

1. **Eligible**: lifecycle `Ready`, no active session, no job (State), `fsrsOk`, and the platform allows: on macOS while the app is open; on iOS and iPadOS in the foreground or while the optimizer processing task runs (R698). **Applies settled**: the tracking summary of the last `TrackingChanged` (`sync/policy#P3`) shows no `ready` inbox entry and no `applyInProgress`. Waiting inbox entries do not count (O13). A drain's fetched records are applied by `ApplyReady`, a core action that submits `CloudIncoming` ops only after `step` returns (`sync/policy#P4`), so a run that started at the drain itself would take its first snapshot (O8) before those applies commit, train on a state not yet reconciled, and then always abandon with `inputChanged`. Every connected start and the gate therefore wait until the applies have settled. Decided 2026-10-27: an `ApplyReady{retryFailed}` in the same list (an event trigger with an `apply_failed` entry waiting, P10.1) does not hold a start or the gate back. Its ops are queued before the optimizer's call (Order, above), so a retried apply that commits does so before the commit's checks, which then abandon `inputChanged`, and one that fails writes nothing. Waiting for it would stall: after its fifth failure an entry is retried only once per collection open (`sync/cloudkit-records#K13.2`), and an `ApplyReady{retryFailed}` that retries nothing makes no `TrackingChanged`. The cost is at most one unused run per retried entry that applies.
2. **Arming.** These inputs set `armed` when the cadence is due: `Lifecycle{Ready}` with a new generation, `SessionEnded`, `Foreground`, `ProcessingTaskLaunched{optimizer}`, and an `OptimizerCadence` whose `due` turns from false to true (from empty too, State). So at launch the run is armed whichever of `Lifecycle{Ready}` and the first `OptimizerCadence` comes first. In mode connected, auto, arming also emits `RequestFetch{root, collection}` with a new call id, the arming call (or gives the call id to the one the list already holds: the P10.1 and P11.1 rows, above). Setting or clearing `armed` clears `drainedSinceArmed`, and so does the mode entering connected, auto; when it enters it while armed, the `RequestFetch{root, collection}` of that input (P10.1's opening rows and `DeviceSettings{automatic: on}` row hold one) becomes the arming call. Mode connected, manual arms nothing: with Sync automatically off, the optimizer runs only through Optimize now. Starting a run clears `armed`; `SessionStarted` and a gate that closes clear it too. A `ProcessingTaskLaunched{optimizer}` that arms nothing (the cadence is not due, or the mode is connected, manual or blocked) emits `EndProcessingTask{optimizer}` (rule 8).
3. **Automatic start.** When armed and eligible and due, emit `StartOptimizer{automatic}`: in mode local-only at once; in mode connected, auto once `drainedSinceArmed` is set and the applies have settled (rule 1). `drainedSinceArmed` is set only by an input that comes while `armed` is set and the mode is connected, auto: a `FetchDrained{collection}` of the arming call (either `fetched`), or the `ExchangeEnded{succeeded}` of an exchange the policy started after arming. The policy checks at that input and again at every later input while armed, `TrackingChanged` included, so the start comes at the `TrackingChanged` whose summary shows the drain's applies settled. Blocked: none. So a run on a connected collection starts from a state that a fetch made after arming reconciled with CloudKit, with the fetched records applied (R699), and retries after an abandon wait for the next arming input, never for a timer. When rules 3 and 4 would both start a run at one input, only `StartOptimizer{optimizeNow}` is emitted; it clears `armed` and the pending request.
4. **Optimize now.** `OptimizeNowRequested` emits `OptimizeNowRefused{reason}` with the first that applies: `notReady`, `fsrsOff`, `sessionActive`, `running` (a job, or an Optimize now already pending), `cloudBlocked` (mode blocked). Otherwise, in mode local-only, `StartOptimizer{optimizeNow}`; in either connected mode, `RequestFetch{root, collection}` with a new call id, and the request is pending. While it is pending, `StartOptimizer{optimizeNow}` is emitted, and the request cleared, once a `FetchDrained{collection}` of that call has come, the applies have settled (rule 1) and the conditions still hold; the policy checks at every input, `TrackingChanged` included, as in rule 3. The pending request is cleared with `OptimizeNowRefused{reason}` when a refusal condition above starts to hold (a session start, a blocked mode, …), with `OptimizeNowRefused{fetchFailed}` at an `EngineActivity{collection, fetch, ended}` whose outcome is `failed` before the drain, and with `OptimizeNowRefused{fetchFailed}` at `OptimizeNowExpired`: `OptimizerCoordinator` holds a 5-minute deadline from the request, a deadline of one operation (`sync/policy#P2`), and reports that input when it passes with the request still pending. So a fetch that never drains never leaves a request that a much later, unrelated drain would start. Decided 2026-10-27: Optimize now is an explicit request like Sync Now, so its two fetches run also with Sync automatically off; its adopted result then waits for the next send (rule 6).
   - **A refused start.** The core can still refuse a `StartOptimizer` (O7: its view of the session, the cadence, `fsrsOk` or a job can be ahead of the policy's). `OptimizerStartRefused{trigger, reason}` clears the `starting` job and its start mode. For trigger `optimizeNow` it emits `OptimizeNowRefused{reason}`, so the view shows the core's reason; for `automatic` it emits nothing, and the next arming input runs again. While a processing task runs it also emits `EndProcessingTask{optimizer}` (rule 8).
5. **The gate.** At `OptimizerJob{AwaitingGate}`, with the mode the job started in from the policy state, and again at every later input while the policy holds the job in `AwaitingGate`:
   - a session has started (`SessionStarted`), or the mode is not the job's start mode (it changed, or became blocked): `AbandonOptimizer{jobId}`;
   - job started in mode local-only and the mode is still local-only: `OptimizerGate{jobId}` at once;
   - job started connected and the mode is still that connected mode: at `AwaitingGate`, `RequestFetch{root, collection}` with a new call id, the gate's call; then `OptimizerGate{jobId}` at the first input at which a `FetchDrained{collection}` of the gate's call has come, the applies have settled (rule 1) and the bound epoch is confirmed for the current engine instance (`sync/policy#P5`, P10.1 "Confirmed root"). The fetched records are thereby applied before the commit's checks, which then see them (O8). The root zone is fetched in the same call: a drained root fetch that finds `MGActive` naming another epoch isolates the binding (`sync/cloudkit-records#K18.4`), and when the binding records no `MGActive`, the confirmation waits for the `root_read` that P10.1 asks for. An isolation makes the mode blocked (an abandon at that input), and the guard of an `OptimizerGate` already emitted not current (P4: every `Binding` input but a step change increases the token), so `optimizer_commit` refuses it (O7, O8).
   - When the policy emits `OptimizerGate{jobId}`, it holds the job as `Committing` and clears the gate's call id, so no later input, the commit's own `TrackingChanged{cause: local}` included, emits a second `OptimizerGate` for it; when it emits `AbandonOptimizer`, it holds the job as `ending` and emits nothing more for it until its end. A refused `optimizer_commit` or `optimizer_abandon` (`notAwaitingGate`, O7) publishes no end; the job's one end comes from the call that ended it.
   - `OptimizerCoordinator`'s 5-minute gate deadline (Deadlines, above) ends a gate whose fetch never drains: `optimizer_abandon` ends the job `Abandoned{gateFailed}`. The next arming input runs the job again.
6. **After the commit.** The commit is a local change: its dirty marks (`sync/cloudkit-records#K11.1`) are written by an observer of the commit op, so its `TrackingChanged{cause: local}` reaches the policy before `OptimizerJob{Ended{Done}}` (O7), and P10.1's local row has handled it. At `Ended{Done}`, in mode connected, auto: `UpdatePending{full}`, then `RequestSend{collection}` when sendable (`sendable` plus `parked` above 0, as after a full update, P10.1), in that one action list. So the result leaves without waiting for the flush timer, also from a background processing task, and the send keeps `sync/policy#P4`'s rule that every `RequestSend` follows an `UpdatePending{full}` in its own list. In mode connected, manual, nothing: it waits for the next Sync Now. A failed send keeps the result pending (R699); nothing retries it outside the normal policy.
7. **Due flag.** `OptimizerDueChanged{due}` is emitted when this value changes: cadence due, `fsrsOk`, no job, and mode connected, auto or local-only.
8. **Expiry and background.** `BackgroundExpiring` while the optimizer processing task runs emits `CancelOptimizer{jobId}` when the policy holds a job other than `starting` or `ending`, then `EndProcessingTask{optimizer}`. On iOS and iPadOS a move to the background without the task cancels training through suspend (O15). **The processing task's end.** The policy emits `EndProcessingTask{optimizer}` and clears its processing-task flag at the first of: `OptimizerJob{Ended}` of the run (the list's last action, so in rule 6's list it follows the `RequestSend`, and `SyncCoordinator` runs it after that send has returned), `OptimizerStartRefused` (rule 4), a `ProcessingTaskLaunched{optimizer}` that arms nothing (rule 2), and `BackgroundExpiring`. So the task never outlives its run, and the result of a commit in mode connected, auto leaves before the task completes.
9. No optimizer input or action starts an Anki transfer or reads Anki sync state (`sync/policy#P16`, R1549).

On macOS, `OptimizerCoordinator` starts the job in the same turn as `StartOptimizer` and holds a `ProcessInfo` background activity for the job's life; it does not use `NSBackgroundActivityScheduler`, whose deferral would separate the start from the fetch that preceded it.

Policy tests (U113's table and property tests, optimizer cases): session end on connected, auto with due cadence → P9's actions and `RequestFetch{root, collection}` with a call id, then `StartOptimizer` at that call's drain when it fetched nothing; at launch, `Lifecycle{Ready}` before the first `OptimizerCadence{due: true}` and after it → armed in both orders; a drain with no call id (another call's, or an engine-scheduled fetch's) that arrives after the arming or gate request → no `StartOptimizer` and no `OptimizerGate`; armed with a drain seen, then a closing gate, a `Foreground` while blocked and an opening gate → no start in the opening list, and a start only after the opening fetch's drain; `OptimizerStartRefused` for `optimizeNow` → `OptimizeNowRefused` with the core's reason, and for `automatic` → nothing, a later arming input starting again; rules 3 and 4 at one input → one `StartOptimizer{optimizeNow}`; the gate passed → one `OptimizerGate`, and the commit's `TrackingChanged{cause: local}` emits no second one; during the gate, a `Binding` input that isolates the binding (`epoch_switched`), a mode change or a `SessionStarted` → `AbandonOptimizer`; a gate drain while the binding records no `MGActive` → no `OptimizerGate` until `RootConfirmed`; `BackgroundExpiring` with a job → `CancelOptimizer` then `EndProcessingTask`; `Ended{Done}` in connected, auto with a processing task → `UpdatePending{full}`, `RequestSend{collection}`, `EndProcessingTask` in that order; an `apply_failed` entry and an `ApplyReady{retryFailed}` in the gate drain's list → `OptimizerGate` in that list after it; modes with `needsAttention{conflicts}` and the gate open → connected, and with `paused{automaticOff}` → connected, manual; a drain with `fetched` true and a ready inbox entry → no start at the drain, and `StartOptimizer` only at the `TrackingChanged` whose summary shows no `ready` entry and no `applyInProgress`; a session that starts before the drain → no start; `AwaitingGate` → `RequestFetch{root, collection}`, then `OptimizerGate` only after the inbox is applied; an incoming change during the gate fetch → the commit abandons `inputChanged` (core test); Optimize now in each mode, and every refusal, a second request while one is pending, a pending request cleared by a failed fetch and by `OptimizeNowExpired` with no start at a later drain; `Ended{Done}` in connected, auto → `UpdatePending{full}` then `RequestSend{collection}` in one list; Sync automatically off → no automatic start and no send after the commit; after a `Done` that adopted nothing, the next drain emits no start (the cadence is not due); the property test with the optimizer inputs added still finds no two concurrent exchanges, no optimizer start during a session, no `StartOptimizer` or `OptimizerGate` in mode connected while a `ready` entry or an apply in progress remains, and every `RequestSend` after an `UpdatePending{full}` in its list (P4, P20, P21).

## O13. Two devices (R2052, KTD8)

- **One after the other.** When A's commit is on the server before B's gate call starts, B's gate fetch brings it (preset rows, `lastFsrsOptimize`, cards): the gate counts only the drain of its own call (O12 rule 5), which read the database changes after it started. Those `CloudIncoming` writes touch O8's tables after B's `S`, so B's commit abandons with `inputChanged`, and B's cadence is then not due. B adopts nothing trained on a state that excludes an applied change of A. Records of A that wait in B's inbox (reasons `reference`, `generation`, `structure`, `format`, `asset`, `apply_failed`; O12 rule 1) do not hold B's gate back; they are the accepted race below.
- **At the same time.** A fetch is not a lock: both may pass their gates and send. The CloudKit merge then decides (`sync/cloudkit-records#K15`): a preset's `config` changed on both sides takes the later version and keeps a conflict copy of the other (K15.7); `lastFsrsOptimize` keeps the later version's day (K15.8), which may differ by a day or so from the other device's (each computes its own day number, `sched/day-clock#D5`), and either value leaves both devices not due (O11); cards follow the card rule, so a card's memory state may come from the other device's parameters until its next answer or the next refresh. Every list kept is one a device adopted from its own reconciled state, so no data is lost and scheduling stays valid. This is KTD8's accepted race.
- **A stopped device.** Decided 2026-10-27: a binding isolated `stopped` is mode local-only (O12), so a device that stopped syncing optimizes on its local history, which may lack other devices' reviews since the stop. After a V9.2 Resume the merge treats its preset writes like the race above: K15.7 keeps the later version of each preset's `config` and a conflict copy of the other, which Review Conflicts lists (`sync/cloudkit-recovery#V17`), and the later `lastFsrsOptimize` keeps every device not due until 30 days after it. This is accepted as part of KTD8's race: no data is lost, the other list stays in its conflict copy for the user, and Optimize now runs at any time. Treating `stopped` as blocked would keep a device that stopped syncing for good from ever optimizing automatically (R692), against R699's local-only rule.
- Decided 2026-10-27 (data safety): a preset `config` conflict between two optimizer writes that differ only in `fsrs_params_6` keeps `sync/cloudkit-records#K15.7`'s conflict copy; nothing is dropped as a duplicate, and K15.7 is unchanged. So the accepted race may list one preset conflict per such preset in Review Conflicts (`sync/cloudkit-recovery#V17`).
- U248's soak runs this race with two devices due on the same day (CloudKit fake and the signed lane) and checks that the collections converge.

## O14. Diagnostics (R701, R1468)

Device-local, never in the collection (R1468): the member `optimizerLog` of `deviceState` in U32's sidecar `meguri-sidecar.json` (`engine/collection-replace#R6.1`, `storage/durability-and-files`), beside `lastOptimize`. Decided 2026-10-27: not a U26 derived-store component. A component has only capture, apply and build (`storage/derived-store#D4`), all fed from collection commits or snapshots, while a `Cancelled` or `Abandoned` run has no commit; and suspend invalidates the whole derived store on an external change (`engine/suspend`), which would lose this history, which nothing can rebuild. The sidecar already holds device-only maintenance records with a durable atomic write.

`optimizerLog` is a JSON object:

| Member | Value |
|---|---|
| `lastRun` | object: `instant` (signed 64-bit epoch milliseconds), `trigger` (`automatic` or `optimizeNow`), `end` (`done`, `cancelled`, or `abandoned` with a `reason` member: `inputChanged`, `sessionActive`, `superseded` or `gateFailed`), `adopted` (array of preset ids, empty unless `done`) |
| `presets` | object keyed by the preset id as a decimal string; each value: `day` (today's day number of that run), `instant`, `outcome` (O3, O5), `items` (item count), `reviews` (review count used, `sched/fsrs-items#I7`), `health` (`good`, `bad`, or absent for none) |

- **Write points.** One sidecar write per run end, after the end: `Cancelled` and `Abandoned` replace `lastRun` only; `Done` replaces `lastRun` and, after the commit has committed, sets the `presets` entry of every preset the run visited and drops the entries of preset ids that no longer exist. Nothing else writes it. A failed sidecar write is logged and changes nothing in the collection; the run's result stands.
- R6.1's rules apply: the write goes through the sidecar lock and its durable read, modify and write, keeps unknown members, and a malformed `optimizerLog` is an error, never absence, and is not replaced. Decided 2026-10-27: it never stops a run (the log gates nothing); `optimizer_diagnostics()` then reports the log as unreadable, and Check Database's repair of malformed device records is its repair path (O18).
- Losing the sidecar or the member loses only this history.
- `optimizer_diagnostics()` returns: `lastFsrsOptimize` as a local date (the synced value, which another device may have written), the last run on this device, whether a job runs and its progress, and for each current preset its name and `presets` entry (none when it has none). It returns no parameter values (R691).
- `SchedulerDiagnosticsView` (app-side leg) shows that data read-only, with Optimize now and the refusal reason of O12 rule 4.

## O15. Progress, cancellation, suspend, budgets

- Progress (`engine/jobs#J5`): phases `Training` (preset k of n, the crate's iteration and total, the preset's review count), `Evaluating` (preset k of n), `HealthCheck` (preset k of n), `AwaitingGate`, then O9's `Committing`.
- Cancel: the token sets the crate's `want_abort`; the crate's `Interrupted` error ends the job `Cancelled` with nothing written (R698). The token is also checked between presets and between the steps of O4 to O6. `cancel_job` on a job in `AwaitingGate` ends it `Cancelled`; on a job in `Committing` it cancels the commit op, queued or running, which writes nothing (O9), and the job ends `Cancelled`; on an ended job it does nothing.
- Suspend (`engine/suspend` S1): the job is a `SuspendParticipant`. Suspend cancels a job in `Training`, ends a job in `AwaitingGate` as `Cancelled`, and cancels a running commit, which rolls back. The next arming input runs again; R1850's "resumed" means this new run.
- Budgets: training, evaluation and the health check over 1,000,000 review-log rows take at most 30 s on the M3 Max at utility QoS (R1850); peak footprint at most 300 MB at `realistic-500k` (R1852), measured by `meguri-cli bench optimizer` with `fsrsHealthCheck` on and a preset above 300 items, recording the item builder's bytes per preset. A breach of R1852 that cannot be fixed without changing the item set is a stop condition reported as an open plan question: items are never trimmed (R1902).

## O16. Differences from Anki

Each is Meguri's design; the deviation ids are proposed for `conformance/deviations.yaml` (O18).

| Difference | Anki | Meguri | Proposed deviation |
|---|---|---|---|
| when it runs | on request only | automatically every 30 days, plus Optimize now (R691, R692) | none (no oracle observation) |
| default card set | by name, with the cases of O2 | by preset id | `optimizer-preset-by-id` |
| `param_search` of white space alone | one-preset path: the whole collection's review log, rows of cards that no longer exist included; all-preset path: the default search | the default set, by preset id (O2) | `optimizer-blank-search-default` |
| cutoff date | one-preset path: the web view's date parser; all-preset path: `sched/fsrs-items#I1.3` | `sched/fsrs-items#I1.3` only | none (the all-preset path's reading; a refused text is the "refused cutoff" row below) |
| minimum items | none | 64 (R693) | `optimizer-min-items` |
| presets not adopted | all-preset path writes the list in use into `fsrs_params_6` | not written (R695) | `optimizer-unadopted-unwritten` |
| relearning step count | all-preset path: all steps | steps below 1440 minutes, as the one-preset path (R694) | `optimizer-relearning-steps-in-day` |
| health check | one-preset path only, dialog | every run, recorded (R700) | none |
| refused cutoff | all-preset save fails | that preset skipped | `optimizer-invalid-cutoff-skips` |
| reschedule on change | follows the screen's option | never (R697) | none (Meguri has no such option) |
| undo | one "Deck Options" step | not undoable, clears undo | `optimizer-not-undoable` |
| unchanged cards | the save stamps every recomputed card | only changed cards (R687) | `memory-recompute-unchanged-mod`, `memory-recompute-unchanged-usn` (existing) |
| other writes | the save also writes the screen's global options | only `fsrs_params_6`, cards and `lastFsrsOptimize` | none |

## O17. Scenarios and tests

No optimizer op or observation exists in the oracle driver or the runner at this pin, so no scenario is written yet. U47 adds these oracle ops and the scenarios below in one spec-writer pass; each golden is recorded at the current `conformance/oracle.json` pin.

- Oracle ops needed: `compute_fsrs_params{preset, current?, relearning_steps, health_check}` (Anki's one-preset backend call, with the search of O2 built by the driver, returning the list, `fsrs_items` and the health result), `evaluate_fsrs_params{preset, params}` (log loss and RMSE(bins) on the preset's items), and `optimize_all_presets` (the all-preset save) for O1's columns. Observation: `fsrs_training_items{preset}` (item count, review count, the items' tag ids).
- Comparison (R1904): item counts, review counts and tag ids exactly; the adoption decision exactly, except when the two losses are within 1e-4 of each other; the adopted list's log loss and RMSE(bins), evaluated on the same items, within 0.002 absolute of the oracle's. Raw parameter values are never compared.

| Scenario | Covers |
|---|---|
| `sched/optimizer/items` | `sched/fsrs-items#I7` on fixed review logs: counts, tag ids, cutoffs, Reset markers, cram rows |
| `sched/optimizer/adopt` | O4, O5 step 2 |
| `sched/optimizer/not-better` | O5 step 3 |
| `sched/optimizer/relearning-check` | O4's step count, O5 step 4 in both directions |
| `sched/optimizer/weight-search` | O2's custom set, white space alone (deviation `optimizer-blank-search-default` for the one-preset op), a refused search |
| `sched/optimizer/preset-names` | O2's name table, with `optimize_all_presets`: a name with `"`, with `\` inside and at the end, with a tab and with U+00A0, and one equal under unicase to an earlier preset's |
| `sched/optimizer/health-check` | O6 at 301 and 300 items, with the check on and off |

Meguri-only tests (`core/crates/meguri-engine/tests/optimizer.rs`), one per plan test scenario of U47: AE28's core part (O9 example); 63 items skipped; `fsrs_params_5` and `fsrsWeights` byte-identical after adoption, and an FSRS-5-only preset gets `fsrs_params_6` filled; a stored `weightSearch` overriding the default set; cancel during training and during the commit (collection digest unchanged); a session start during training, during `AwaitingGate` (even if it ended before the commit) and during the commit, and a refused start during the commit (the commit still ends `Abandoned{sessionActive}`, O10); the `optimizerLog` writes of O14 for each end, and a malformed `optimizerLog` that the run leaves as it is; each O8 change (generation, binding, the binding's state with the same id and epoch, day, each table and config key) → `inputChanged`; a stale guard at `optimizer_commit` → `StaleGuard`, `Abandoned{inputChanged}`, nothing written; `optimizer_commit` or `optimizer_abandon` on a `Committing` or ended job → `notAwaitingGate`, nothing written, no second end; `optimizer_abandon` after `optimizer_commit` queued its op behind another background op → refused, and the commit ends `Done`; `optimizer_abandon` → `gateFailed`, then a new start runs; a start while `AwaitingGate` → `superseded`; an unsupported preset beside an adopted one (`sched/fsrs-policy#T3` row U47, K3 step 4); a run that adopts nothing still writes `lastFsrsOptimize`; the commit emits `OptimizerEnded{Done}` and runs no sync; health check at 350 and 250 items; the R1905 test (O5). A T1 live check opens the collection in the oracle after adoption and builds a model from every preset's list (R696).

## O18. File homes and plan corrections

Decided 2026-10-27:

- The optimizer's policy branch (O12) is `core/crates/meguri-sync/src/cloud/policy/optimizer.rs`, with its inputs and events in the same `cloud/policy/` module and its tests in `core/crates/meguri-sync/tests/cloud_policy.rs`. `meguri-sync/src/policy/` holds the frozen manual-transfer policy (`sync/policy#P2`, `#P16`), and the two share no state. `meguri-host/src/wiring/sync.rs` and `meguri-ffi/src/sync.rs` carry the four O12 inputs as the plan says, and `OptimizerStartRefused`.
- The optimizer section the plan asks U47's spec writer to add to `specs/sync/policy.md` is O12 and O13 of this file. `sync/policy#P18` keeps its place and points here.
- The `engine/op-model` O9 catalog row this file needs (O9): `optimizer_commit`, source `Optimizer`, not destructive, not undoable, long write, write cancellable between batches.
- The deviation ids proposed for `conformance/deviations.yaml` (O16): `optimizer-preset-by-id`, `optimizer-blank-search-default`, `optimizer-min-items`, `optimizer-unadopted-unwritten`, `optimizer-relearning-steps-in-day`, `optimizer-invalid-cutoff-skips`, `optimizer-not-undoable`. U47's spec-writer pass adds them with the scenarios of O17, whose goldens they explain.

Corrections other specs need (each owner makes them; until then this file decides). Made 2026-10-27: items 1 to 5, in `sched/review-session#C2` step 0, `sched/fsrs-items#I7`, `engine/collection-replace#R6.1`, `maintenance/check-database` C14, and `sync/policy#P3`, `#P4`, `#P5`, `#P10.1` and `#P18`; the `engine/op-model#O9` row above is made too. The items stay listed as the record of what changed:

1. `sched/review-session#C2` (required): a new step before step 1, "On submission, before the start is queued for the writer: request cancellation of a running optimizer commit op without waiting (`sched/optimizer#O10`). It fires for every submitted start, also one that admission then refuses." Step 2's hook keeps its meaning (training cancel, `AwaitingGate` mark).
2. `sched/fsrs-items#I7` card-set bullet: "the cards of `sched/optimizer#O2`'s default set (by preset id, not suspended), or the cards U80 finds for the preset's `param_search` when it is not empty after trimming white space". O2 supersedes the bullet until then.
3. `engine/collection-replace#R6.1` deviceState table: a row for `optimizerLog`, the object of O14, written only by U47.
4. Check Database (`maintenance/check-database`): add `optimizerLog` to the malformed device records it repairs, by removing a malformed member (O14).
5. `sync/policy#P10.1`, the `FetchDrained{collection}` row (for the CloudKit review): "with auto off only Sync Now and a `publish` op's step `zones` fetch" → also Optimize now's fetches (O12 rule 4); and P3 and P4 list O12's inputs and actions by reference to `sched/optimizer#O12`.
6. Not yet made (`sync/policy`'s owner; `sync/policy#P18` records them until then, decided 2026-10-27): P3 `FetchDrained` and P4 `RequestFetch` gain the optional call id of O12 ("The optimizer's fetch call"); P2's list of deadlines names `OptimizerCoordinator`'s two (O12, Deadlines); P4's ordering bullets say how `OptimizerCoordinator` runs its actions (O12, Order); P5 names the platform constant; P3 and P4 list `OptimizerStartRefused`, `CancelOptimizer` and `EndProcessingTask`.

Corrections for the plan's owner (the plan is not a spec):

1. U47 **Files**: `core/crates/meguri-sync/src/policy/optimizer.rs` and `src/policy/events.rs` → `src/cloud/policy/optimizer.rs` and the `cloud/policy/` module; `core/crates/meguri-sync/tests/policy.rs` (optimizer cases) → `tests/cloud_policy.rs`; `specs/sync/policy.md` (optimizer section) → `specs/sched/optimizer.md` O12 and O13, cited by `sync/policy#P18`.
2. U47 **Approach** and test scenarios: `syncRequested(optimizerAdopted)` is O12 rule 6 (`OptimizerJob{Ended{Done}}` and `RequestSend{collection}`); "session ended → sync action, then an optimizer start after success" is O12 rules 2 and 3 (`RequestFetch{collection}`, then `StartOptimizer` at the drain).
3. R695 and R1905: R695's "Anki's extra relearning-steps check" can adopt a list whose log loss is not lower (O5 step 4). Reword R1905 to "adopts new parameters only under the adoption rule of `sched/optimizer#O5`".
4. Open Questions, "Optimization without a sync account": obsolete. R699 now lets a local-only collection optimize under the local guards (O12 mode local-only); the diagnostics view does not say that it needs sync.
5. The plan's 64-item, 30-day and health-check rules stand; O1 records that Anki's all-preset path has no minimum and no health check.
6. R694: `search preset:"<name>" -is:suspended unless the preset's stored weightSearch overrides it` → "the preset's own cards (by preset id, not suspended) unless its stored `weightSearch` overrides it (`sched/optimizer#O2`)".
7. The device-only state list (KTD12 and the U-unit text that names "optimizer run diagnostics (U47 derived-store table)") → "optimizer run diagnostics (U47, `deviceState.optimizerLog` in U32's sidecar, `sched/optimizer#O14`)".
