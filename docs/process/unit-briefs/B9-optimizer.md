# B9-optimizer: unit brief (implementer view)

Written from origin/master `5cae7aef` plus the uncommitted working tree on 2026-11-10. The two commits after the first draft's base `21fc5c1f` change no optimizer fact: `cf03979b` touches browser, stats and storage specs and their provenance, and `5cae7aef` adds AnkiConnect recordings and one `conformance/AGENTS.md` line about them. Sources: the plan (`docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` ### U47), `specs/`, `conformance/`, `docs/process/`, `docs/design/` and Meguri code. Nothing under `oracle/`, `refs/` or `local-data/` was read.

## 1. Batch

- Units: U47 only ("Automatic FSRS optimizer job and its app-side coordination"). Milestone M1 (`docs/process/milestones.yaml`, M1 `units`; the M1 gate names "the optimizer (R1904, R1905; U47 ships here)").
- Why it is a batch of its own: one unit with three legs. The legs are (a) the core in `meguri-sched`, `meguri-engine`, `meguri-storage`, `meguri-ffi` and `meguri-cli`; (b) the policy branch in `meguri-sync/src/cloud/policy/`, attached to U113; and (c) the app-side leg (`OptimizerCoordinator`, `SchedulerDiagnosticsView`), attached from U51 and U154. Each leg has different blockers (§2).
- Priority (`docs/process/review-first-delivery.md`, "Next feature priority"): reviews and iCloud first (U113, U114 and their dependencies), then editing, search and statistics. The same section says: "Before U47 builds on an O19 item, add its named test and settle the recorded correction through the spec-writer workflow; the approval alone does not close O19."
- Gates (`docs/process/tiers.md`): the core leg is Rust internals plus FFI, so it is not a pure-Rust batch. It needs one full gate at the batch boundary (`tools/mg gate --base <batch-start-commit>`), and M1 needs its milestone gate.
- Suggested order:
  1. Spec-writer pass, outside the implementer role: settle the five open O19 items (§5) and the owner corrections O18 items 6 to 9. Also correct the uncommitted O17 sentence that says the typed scenario model does not know the ops (§3.4).
  2. One commit for the conformance change: the held spec, schema, oracle driver ops, eight scenarios and goldens (§3.4), together with the testkit typed model (§4): the `Step` variants, `Step::NAMES`, `Step::name()`, and `ObservationKind::FsrsModelCheck` with its `name()`. Do not land the held part without the testkit part. `Step` is a closed serde enum, so the scenarios would not parse: they would be `invalid`, which always fails T0 and `tools/mg goldens-check` (`dsl#D10`), and master would turn red. `conformance/AGENTS.md`: "Change spec, schema, the testkit model (`core/crates/meguri-testkit/src/scenario/`) and, for a new op or kind, the oracle driver together." The implementer writes the testkit part from `specs/conformance` and the schema only, never from `oracle/`. The orchestrator joins it to the spec-side part. No adapter is in this commit, so each scenario is `pending` on Meguri and stays inactive.
  3. Core training and adoption in `meguri-sched/src/optimizer/` (O2 to O6, O15 pool and cancel), with the `compute_fsrs_params` adapter. Make those scenarios pass and mark them active in the commit that lands the adapter (`conformance/AGENTS.md`).
  4. Engine job, revision gate, commit op, session hooks, suspend participant, cadence and diagnostics (O7 to O11, O14, O15), with the `optimizer_run` and `fsrs_model_check` adapters for `live-adoption`. Then the FFI and `meguri-cli bench optimizer`.
  5. Policy branch (O12, O13) once U113 has merged.
  6. App-side leg once U51, U114, U151, U154, U160 and U163 have merged.

## 2. Dependency status

"Merged" means that origin/master has a commit with a `Unit:` trailer for that unit (`git log --format='%h|%s|%(trailers:key=Unit,valueonly)'`).

| Dep | Leg | Status | Evidence |
|---|---|---|---|
| U46 | core | merged | 77b3b724 "Complete U46 memory history resolution and cancellable bulk recompute" |
| U42 | core | merged | 47fec101 |
| U43 | core | merged | 24a55378 (accepted with the interim R554 exception) |
| U49 | core | merged | 5d651707 (VM performance check queued) |
| U80 | core | merged | 8a13d266 |
| U24 | core | merged | 5827e66b |
| U26 | core | merged | fb85a2c8 |
| U12 | core | merged | 6a0ddfd8 |
| U45 | core (U1 check) | merged | 2c656bc7 |
| U32 | core (sidecar, suspend) | merged | 17aed1de |
| U51 | app | **not merged** | no origin/master commit has a parsed `Unit: U51` trailer, and no implementation exists. The spec-writer commit 10913245 names `Unit: U51` in its message body (line 33), not in its trailer block, so the trailer query does not return it |
| U113 | policy | **not merged** | no trailer; `docs/process/review-log/U113-cloudkit.md`: "Status: incomplete; no Unit trailer or acceptance" |
| U114 | app, policy | **not merged** | no trailer; `U114-cloudkit-adapter.md`: "incomplete; no Unit trailer or P24 acceptance". Its blocked-red escalations are all `resolved` in `docs/process/provenance/escalations.jsonl`, so none is an open blocker: `E-20261101T232833Z-dbc5` (2026-11-02T11:38:56Z, rounds 7 and 8 approved), `E-20261102T122155Z-e1eb` (2026-11-02T12:36:05Z, one final test-only round approved) and `E-20261102T125141Z-4069` (2026-11-03T18:06:06Z, R10 and R11 approved; "No unit acceptance granted"). U113's approval `E-20261102T070407Z-6a04` is resolved too (2026-11-02T07:04:44Z) |
| U151 | app | **not merged** | no trailer; `U151-media-cancellation-20261104.md`: "WIP"; no `CollectionStore` in `app/` |
| U154 | app | **not merged** | no trailer; no `BackgroundTaskIdentifiers.swift` |
| U160 | app | **not merged** | no trailer |
| U163 | app | **not merged** | no trailer; no Maintenance page row |
| U13 | app (`OptimizerGateServerTests`) | merged, **frozen** | 79e3fb8b; the Anki server freeze in `review-first-delivery.md` |
| U7, U9, U10 | app | merged | e56cf87b, a645e610, 2ff5b898 |

- Every core-leg dependency has merged. The policy branch is blocked on U113. The app-side leg is blocked on U51, U113, U114, U151, U154, U160 and U163.
- Units that wire in after U47 (not dependencies): U36's optimizer preservation leg (plan ### U36, "An optimizer write (U47 leg)"), U248's two-device optimizer race, U250's "optimizer write" fault class, and U249 ("Wires in when landed": "U47 (optimizer-adopted parameters)").
- Units that list U47 as a dependency: U242 (profiles and the `optimize` bench group), U101, U245 and U246. U242 has not merged. `tools/perf/budgets.yaml` R1850 `core` is `pending: U242`, and R1850 `device` is `pending: U256`.

## 3. Per unit: U47

### 3.1 Spec ids for `Spec:` trailers

- Core training (in force): `sched/optimizer#O2`, `#O3`, `#O4`, `#O5`, `#O6`, `#O15`; `sched/fsrs-items#I7` (with `#I1.1`, `#I1.2`, `#I1.3`); `sched/presets#V5`, `#V6`; `sched/config-keys#G2`.
- Job and commit: `sched/optimizer#O7`, `#O8`, `#O9`, `#O10`, `#O11`. Related: `sched/memory-state#M4`, `#M7.3`; `decks/deck-presets#P10`; `sched/review-session#C2` (steps 0 and 2); `engine/op-model#O9` (the `optimizer_commit` row at line 138); `engine/jobs#J1`, `#J4`, `#J5`, `#J6`; `engine/suspend#S1`; `sched/day-clock#D5`, `#D9`; `sched/fsrs-policy#S1`, `#U1`, `#U4`, `#K3`, `#T3` (row U47). O7 to O10 are touched by open O19 items 3 and 5 (§5).
- Diagnostics: `sched/optimizer#O14`; `engine/collection-replace#R6.1` (the `optimizerLog` row); `maintenance/check-database` (the C14 repair, line 327).
- Policy branch: `sched/optimizer#O12`, `#O13`; `sync/policy#P18` (with `#P3`, `#P4`, `#P5`, `#P6`, `#P10.1`, `#P11.2`, `#P11.3`).
- Requirements: R691–R701, R1850, R1852, R1904, R1905, AE28; supports R704, R1468, R1486, R2052. The app-side leg also needs R1970 (previews). That leg is "Default execution with no spec-writer pass" (plan Approach), so its trailers are requirement ids.
- `conformance/optimizer-ops` (P1 to P5) is not on master: `specs/conformance/optimizer-ops.md` is untracked. Cite it only after it lands.

### 3.2 Meguri design docs the unit writes

- None. No Files entry of U47 is marked "(Meguri)". `specs/sched/optimizer.md` appears in Files, but only spec writers edit specs (`specs/AGENTS.md`).

### 3.3 Committed scenarios and goldens

- None. `git ls-files conformance/scenarios/sched/optimizer conformance/goldens/sched/optimizer` is empty. `conformance/scenarios/sched/area.yaml` names no optimizer scenario as active or reference.

### 3.4 Prepared, held scenarios

All eight scenarios and their goldens are untracked: prepared, held until the optimizer conformance change lands as one commit (§1 step 2). That commit holds the untracked `specs/conformance/optimizer-ops.md`, the uncommitted optimizer rows in `conformance/schema/{scenario,observation}.schema.json` and `protocol.json`, the `eval` tolerance row in `specs/conformance/observations.md` O7, the `sched/optimizer` O17 rewrite (also uncommitted), the oracle driver ops, and the testkit typed model (§4). The working tree has no testkit change (`git status core/` is empty), so that part is still to write. The committed O17 says the scenario files "land in the same change as the schema, the ops and their goldens, not before". Without the testkit part the scenarios would be `invalid`, not `pending`.

The uncommitted O17 rewrite says: "Meguri's executor and its typed scenario model do not know these ops yet, so each scenario is `pending` on Meguri". That is true only if the testkit typed model lands in the same commit. If the typed model does not know an op, the scenario does not parse and is `invalid`. The spec writer must correct this sentence before the commit: the typed model knows the ops, and only the adapters are missing.

All goldens record oracle lock `232216513595…`, which equals `conformance/oracle.json`. Freshness checked 2026-11-10: the RFC 8785 (JCS) SHA-256 of each parsed scenario (`dsl.md` line 40) equals its golden's `scenario_sha256` (adopt `9e639236…`, health-check `93b475ec…`, items `8bd24f82…`, live-adoption `d2f0d8d6…`, not-better `5a96b2ee…`, preset-names `1e3b71b0…`, relearning-check `c945aaf9…`, weight-search `a5cac3c4…`). So `adopt.yaml`, `health-check.yaml`, `live-adoption.yaml` and `preset-names.yaml`, which were edited at or after their goldens' times, are still fresh. Still run `tools/mg goldens-check` in the commit that lands them.

| Scenario (`conformance/scenarios/sched/optimizer/`) | Ops and observations | Covers | Active once landed |
|---|---|---|---|
| `items` | `compute_fsrs_params` ×12, `legacy_update` ×3, `db_exec` ×11, `suspend`, `bury` | I7 counts: Reset, cram, no learning row, suspended, buried, new, `odid`, cutoffs, `invalid_cutoff` | yes, after the adapter |
| `adopt` | `compute_fsrs_params` ×3, `db_exec` ×2, `legacy_update` | O5 step 2, 240 items | yes |
| `not-better` | `compute_fsrs_params` ×2, `db_exec` ×2 | O5 step 3 | yes |
| `relearning-check` | `compute_fsrs_params` ×2 (counts 2 and 40), `db_exec` ×2 | O5 step 4 | yes |
| `weight-search` | `compute_fsrs_params` ×5, `db_exec` ×6, `legacy_update` ×3, `suspend` | O2 custom set, white space alone, `invalid_search` | yes; needs deviation `optimizer-blank-search-default` for the `one_preset` step |
| `health-check` | `compute_fsrs_params` ×3, `db_exec` ×3, `set_config` ×2 | O6 at 301 and 300 items, key on, off and missing | yes |
| `preset-names` | `compute_fsrs_params` ×14, `db_exec` ×12, `optimize_all_presets`, `legacy_update`, `advance`; observations `legacy_json`, `timing` | O2 name table on Anki's paths | no: to be declared `reference` in `sched/area.yaml` (O17.3, `dsl#D9.4`); the declaration is not made |
| `live-adoption` (live, `handoff` ×2) | `optimizer_run`, `db_exec` ×8, `advance`; observations `fsrs_model_check`, `blob_decode`, `timing` | T1 check: R692, R696, R697 | T1, after the core job and commit |

Counts are top-level steps in the working-tree files. Every op and observation used, except the four new optimizer forms (§4), is in the testkit already.

- Deviations: the seven ids proposed in O16 and O18 (`optimizer-preset-by-id`, `optimizer-blank-search-default`, `optimizer-min-items`, `optimizer-unadopted-unwritten`, `optimizer-relearning-steps-in-day`, `optimizer-invalid-cutoff-skips`, `optimizer-not-undoable`) are in neither the committed nor the working `conformance/deviations.yaml`. Add each one with the goldens it explains, once it has its Meguri test. `memory-recompute-unchanged-mod` and `-usn` already exist (origin/master `conformance/deviations.yaml` lines 735 and 742; lines 752 and 759 in the uncommitted working tree).
- Harvest rows deferred to U47 (`conformance/catalog/harvest.csv`, `status` deferred): `h14c25ad44110` (cutoff before the learning start), `hdf5b05a2842f` (learning rows across the ignore date) and `hccc87afa9021` (items sorted by review time with card ids aligned; O17.1 says the oracle cannot observe this, so it is a Meguri-only test). Classification and links are spec-side.

### 3.5 Plan test scenarios → scenario or Meguri-only test

| Plan test scenario | Where |
|---|---|
| AE28 core part (A not better, B adopted, nothing written before the commit) | Meguri-only, `meguri-engine/tests/optimizer.rs` (O9 example); the Anki side is `live-adoption` |
| 63 items skipped | Meguri-only (O3 rule 4; O17.3). The history must survive the crate's preparation (O17.2). `live-adoption` "Small" (5 items) also shows a skip |
| `fsrsParams5` and `fsrsWeights` byte-identical | Meguri-only (O17.3 list, plus an FSRS-5-only preset getting `fsrs_params_6`); U36 preservation leg (plan ### U36) |
| Stored `weightSearch` overrides | `weight-search` (`compute_fsrs_params`), plus an engine test |
| Cancel during training | Meguri-only (digest unchanged) |
| Session starts during training | Meguri-only |
| Revision, account or generation change → `inputChanged`; session after training → `sessionActive`; local-only guards | Meguri-only: each O8 change, the custom-set table bullet, `sessionActive` before `inputChanged` |
| `optimizer_abandon` → `gateFailed`, then a new start runs | Meguri-only |
| 34-parameter preset beside a normal preset | Meguri-only (`sched/fsrs-policy#T3` row U47, K3 step 4) |
| Nothing adopted → `lastFsrsOptimize` still written | Meguri-only |
| Commit ends `Done` with no sync | Meguri-only (`OptimizerEnded{Done}`, no sync); policy rule 6 in `cloud_policy.rs` |
| Health check at 350 and 250 items | Meguri-only (O17.3); `health-check` covers 301 and 300 and the key |
| T1 live: the oracle builds every preset's model | `live-adoption` |
| Two Meguri clients converge | U248 soak (CloudKit fake plus signed lane); O19 item 4's `optimizer.rs` tests |
| Bench: 1M revlog ≤ 30 s, `realistic-500k` ≤ 300 MB | `meguri-cli bench optimizer` (new: `core/tools/meguri-cli/src/bench/` does not exist). The plan's U242 (### U242, Files) owns creating that directory with its per-group suites, `optimize` among them, and U242 depends on U47. So U47's `bench/optimizer.rs` lands before the directory's owner does: keep it to the one suite, in a layout U242 can extend. No `realistic-500k` or `revlog-1m` fixture name exists in the testkit or cli; `meguri-cli generate` takes `500k` and `1m`. `tools/perf/budgets.yaml` does use `fixture: revlog-1m` for both R1850 cases (`core`, suite `optimize`, `fsrs.optimize`, 30000 ms; `device`, `MaintenancePerfTests`) |
| Policy branch (four cases) | `core/crates/meguri-sync/tests/cloud_policy.rs` (file exists, from U113), following the "Policy tests" paragraph of O12 and the O19 tests |
| App-side leg (eight cases) | `OptimizerCoordinatorTests.swift`, UI test via U163's row, U154 processing-task tests |
| Snapshots: empty, typical, overflow (30 presets), error | `app/MeguriSnapshotTests/SchedulerDiagnosticsSnapshotTests.swift` |
| (not in plan) R1905: a rejected list is never adopted | Meguri-only (O5, O17.3) |
| (not in plan) relearning count table, 8 epochs, short-term, `Some(count)` passed | Meguri-only, asserted on the `ComputeParametersInput` (O17.3) |
| (not in plan) selection of the default set by preset id for O2's names | Meguri-only (O17.3) |

## 4. Testkit and adapter work

`core/crates/meguri-testkit` has none of these names (grep in `src/`: 0 hits each):

| Form | Kind | Defined in | Notes |
|---|---|---|---|
| `compute_fsrs_params {preset, relearning_steps, form?}` → `{items, reviews, kept, health, eval:{log_loss}}`; rejections `invalid_cutoff`, `invalid_search` | op | `conformance/optimizer-ops#P1` (held), `sched/optimizer#O17.1` | `preset` is the exact stored name; an unknown name is `unknown_ref` and duplicates are `invalid_scenario`; Meguri ignores `form` and applies no 64-item minimum (P1.5) |
| `optimizer_run {}` | op (live chain) | `optimizer-ops#P3`, O17.1 | local-only policy, `OptimizeNowRequested`, then the coordinator's core calls; any end other than `Done` is a run error |
| `optimize_all_presets {deck}` | op, oracle only | `optimizer-ops#P2` | Meguri never runs it, because `preset-names` is reference scope |
| `fsrs_model_check {presets}` → `{name: ok\|invalid_params}` | observation | `optimizer-ops#P4`, `observations#O4` (uncommitted row) | Meguri uses U45's check, without U2's later-list rule |
| `eval.log_loss` absolute tolerance 0.002 | comparison | `observations#O7` (uncommitted row), `optimizer-ops#P5` | the existing tolerances are relative |

- `Step` (`src/scenario/model.rs:966`) is a closed serde enum with no catch-all. A scenario that uses an unknown op does not deserialize, so it is `invalid`, not `pending` (`dsl#D10`; `tests/scenario_format.rs` expects "unknown variant"). `invalid` always fails T0 and goldens-check, so this work lands in the same commit as the held conformance change (§1 step 2). Add typed variants and `ObservationKind::FsrsModelCheck` so the scenarios load and report `pending` until the adapters exist (D10). A new variant goes in four places: the `Step` enum, `Step::NAMES` (`model.rs:1160`, a fixed-size `[&str; 87]`, so the length changes), `Step::name()` (`model.rs:1254`), and for the observation `ObservationKind` (`model.rs:2077`) with its `name()` (`model.rs:2251`). `validate.rs` does not list ops one by one: it special-cases only some ops (`LegacyUpdate`, `SaveDeckOptions`, `SetSchedulingPrefs`, the media ops, `Observe`, `Handoff`). Add a check there only if the ops file states a load-time shape rule (P1's duplicate preset name is a run error, `invalid_scenario`, not a load check). The Meguri executor registers each adapter by name and asserts that the name is in `Step::NAMES` (`exec/meguri.rs:120`); a scenario that uses an unregistered op is `pending` (`exec/meguri.rs` module comment, line 4).
- The other forms in use already exist: setup `config`, `presets` (`{name}`), `decks` (`{name, preset}`), `notetypes`, `notes`; ops `legacy_update` (preset patch: `weightSearch`, `ignoreRevlogsBeforeDate`, `fsrsParams6`, `fsrsParams5`), `db_exec`, `set_config`, `suspend`, `bury`, `advance`, `handoff`; observations `legacy_json`, `blob_decode`, `timing`.
- `conformance/AGENTS.md`: change the spec, the schema, the testkit model and the oracle driver together, and mark a scenario active in the change that lands its adapter.

## 5. Open questions and decisions

Blocking (each is listed under `sched/optimizer#O19` "Open"; `review-first-delivery.md` requires its named test and a spec-writer decision before U47 builds on it):

1. **O12 rules 1, 2, 3 and 8, State and Inputs.** The processing-task launch decision, `FetchCallEnded`, the arming-call-ended flag and the started-during-task bit. No review has read the fourth-review correction. Blocks the policy branch. Test: `cloud_policy.rs` (all six input orders, and so on) and a U151 or U114 adapter test.
2. **O12 rules 1 and 8, `EndProcessingTask`, the processing task's own sequence** (with `sync/policy#P11.2`). Blocks the iOS app leg and U154. Test: U47 with U154 and U151, using a fake core, a fake engine and a scripted application state.
3. **O12 State and rules 1 and 4, with O7: a job that never sends an end** (the start id, `Training` reported inside `optimizer_start` before the other inputs). Blocks how the core job reports `OptimizerJob` and the policy's job state. Test: `optimizer.rs` (1,000 repetitions with a recording sink) and `cloud_policy.rs`.
4. **O13 "One after the other"** (U114 fetch-call counting through `CKOperationGroup`, will/did pairing, a missing did event). This overlaps `sync/policy#P24`'s open V10.2 / P3 "The start of a fetch call" item. Blocks the connected-mode gate and the two-device tests. Test: U114 adapter tests (a) to (f), and the `optimizer.rs` B-abandons cases.
5. **`CancelOptimizer`, O9 cancel points, O10 submission hook, O15 suspend, C2 step 0** (a `Committing` job that is cancelled). Blocks the core commit op. It needs owner corrections that are not made:
   - O18 item 7: `sched/review-session#C2` step 2 marks a `Committing` job whose op has not started. Line 179 does not say this yet.
   - O18 item 8: `engine/op-model#O9` cancel points and the `sessionActive` reason on the `engine/jobs#J4` token.
   - O18 item 9: `engine/suspend#S1` calls `cancel_job` for the optimizer. `engine/suspend.md` does not mention the optimizer.

   Test: `optimizer.rs`, with a hook that pauses the op at each point.

Not blocking:

- O18 item 6 (`sync/policy`'s owner: the call id on P3 and P4, P2 deadlines, the P4 ordering, the P5 platform, and so on) is not made. `sync/policy#P18` records it until then, and the spec decides meanwhile. In practice it is settled together with item 1.
- **Stop condition R1852.** If the `realistic-500k` peak exceeds 300 MB without changing the item set, stop and report to Antoine. Never trim items (plan Execution note; O15; plan Open Questions).
- **Frozen server lane.** The plan lists `OptimizerGateServerTests.swift (against the U13 server)` and "AE28 end to end passes against the U13 server". Anki server work is frozen (`review-first-delivery.md`), and the plan's U47 sync contract says Anki endpoints are manual-transfer only (KTD26). The disposition of that test is open; the core is not affected.
- **Screen contract.** `docs/design/screens.md` and `screen-inventory.yaml` give `settings.diagnostics` to U51 and U153 ("Diagnostic information and Copy Debug Info", states typical and error). Neither file has an entry for the scheduler diagnostics view that U47 owns. But `screen-inventory.yaml` lines 1001 to 1003 (the deck options entry) already say parameter optimization "reports in home.fsrsSheet and settings.diagnostics". So the inventory routes optimizer reporting to the U51/U153 `settings.diagnostics` screen. That conflicts with U47 owning a separate `settings.diagnostics.*` view reached from U163's Maintenance row. The screen contract must settle which screen shows it. The plan asks for the identifiers `settings.diagnostics.*` and previews for empty, typical, overflow, error and loading (R1970). The plan Open Questions default the location to a "Scheduler diagnostics" row on U163's Maintenance page. UI work starts at `docs/design/workflow.md` with a screen contract. This blocks the app-side UI only.
- Settled 2026-10-27, so not open: local-only optimization is allowed (plan Open Questions "obsolete"); the default set is chosen by preset id (O2); white space alone means the default set; a refused cutoff skips only that preset; O5 step 4 is kept; diagnostics live in the sidecar, not the derived store (O14). The plan's R694, R1905 and KTD12 corrections are made.

## 6. Known traps

- The working tree differs from master. The spec header and O17 of `specs/sched/optimizer.md` are uncommitted edits (master says the ops are not in the driver). `eval` carries only `log_loss`, because `rmse_bins` does not repeat (`optimizer-ops#P1.4`).
- The plan and the spec disagree. The spec wins (`specs/sched/optimizer.md` O18 records the plan corrections):
  - the plan's `Writing` state is `Committing` (O7);
  - the job also ends in `Failed{error}` and `Abandoned{collectionClosed}`;
  - the commit gate also checks the day, the binding state and the P4 guard (O8);
  - the plan path `meguri-storage/src/derived/optimizer_log.rs` writes the sidecar `deviceState.optimizerLog`, not a derived-store table (O14).
- An existing helper does not fit. `meguri-engine/src/sched/memory.rs:38` `preset_selection` (U46) tests `param_search.is_empty()` untrimmed. That is right for the M5 count, but O2 says white space alone means the default set for the optimizer, and the two features stay different on purpose (O2's last decided bullet). Also confirm that its `HomeDecks` path includes cards through `odid` and leaves out `queue = -1` (O2).
- The existing hook is not enough. `meguri-engine/src/session/service.rs:481` `SessionStartHook` cancels only the optimizer jobs in `JobRegistry.running()`. A job leaves J1's running list at `AwaitingGate` (O7, O15), so marking an `AwaitingGate` or `Committing` job needs the optimizer's own state. C2 step 0, the submission hook, is a second hook (O10).
- Two meanings of "Optimize". The engine's `tests/integrity.rs` and `maintenance` optimizer names refer to SQLite optimize (U132, `maintenance/optimize-integrity`). The FSRS optimizer is the `OpSource::Optimizer` commit.
- Crate settings (`fsrs = "=6.6.2"`, `core/Cargo.toml`):
  - set `num_epochs` to 8 (the crate default is 5);
  - pass `num_relearning_steps` as `Some(count)`, including `Some(0)` (`None` means 1);
  - count relearning steps in f64 from f32 values, stop at the first running total ≥ 1440, and let NaN steps count (O4 table);
  - adopt a list only when it has 21 finite values and `FSRS::new` accepts it (O5).
- Thread pool. `docs/plans/research/rust-stack.md` §2 suggests `build_global()` and says the health check's splits spawn on the global rayon pool. O4 and KTD8 say that nothing uses the global pool and that training runs on the optimizer's own utility-QoS pool, built through the thread registry (`engine/jobs#J2`, `#J6`).
- Memory. Drop a preset's items before building the next preset's (R1852, O4). Between presets, keep only the result, the outcome, the counts and the health result.
- Order of skip rules (O3): unsupported, then `invalidCutoff`, `searchFailed`, `tooFewItems` (< 64), `failed`. The minimum is Meguri's; `compute_fsrs_params` does not apply it (P1.5).
- Health check (O6): it runs only for `adopted` or `notBetter` with more than 300 items. A crate error gives no result and does not fail the preset. It never changes the adoption.
- Commit (O9):
  - a list equal to the stored one writes nothing;
  - refresh memory states only on cards whose values change, suspended and buried included, new cards not, with no due, ivl, queue, type or revlog change;
  - write `lastFsrsOptimize` even when nothing is adopted;
  - the commit is not undoable and clears undo (`ClearedByNonUndoableOp`).
- Revision gate (O8):
  - `sessionActive` is checked before `inputChanged`;
  - notes, notetypes, fields, templates and the search config keys are watched only when a custom set was visited;
  - a failing precondition never re-plans.
- Calls (O7): `optimizer_commit` returns only `Accepted`, `notAwaitingGate` or `StaleGuard`. Every started job publishes exactly one end. Report `Training` inside `optimizer_start`, before the work can run.
- Cadence (O11): `lastFsrsOptimize` is 0 on the day the collection is created and reads as never optimized (due). Scenarios advance whole days before they read it (`optimizer-ops#P5`).
- Diagnostics (O14): a malformed `optimizerLog` is an error. The run never replaces it, and it never stops a run. Write the sidecar once per end. `optimizer_diagnostics()` returns no parameter values (R691). The refusal texts are fixed by O14's table.
- Network (O8 and O12 rule 9): the core makes no network calls for the optimizer. No optimizer input or action starts an Anki transfer.
- macOS: hold a `ProcessInfo` activity for the job's life. Never use `NSBackgroundActivityScheduler` (O12).
- Goldens: `kept`, `items`, `reviews` and `health` compare exactly, and raw parameter values never reach a golden (O17.2). Goldens come only from `tools/mg oracle record`, and Meguri output never becomes a golden (`conformance/AGENTS.md`).
- Undo resets: `legacy_*` and `blob_decode` empty Anki's undo history; keep them out of a step's undo window (`conformance/AGENTS.md`).
- Simulator and Mac UI suites run only through `tools/mg` jobs in the VM. The plan's "`xcrun simctl` launch" means `tools/mg` (AGENTS.md).
