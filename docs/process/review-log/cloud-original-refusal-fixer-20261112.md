# Original normal Pause refusal — bounded fixer handoff

- Role: fixer, following `docs/process/roles/implementer.md` (which explicitly covers fixers; no separate fixer role document exists in this export).
- Starting commit: `4c8793b9fb9ad1050728d60be205d6f84ef4c5b2`, clean `master`, filtered “Permitted source baseline; no upstream history”. Requested upstream base is `d2e52db9`; this export cannot independently establish that ancestry.
- Authority: user-provided independent35379 diagnosis and approved P4/P15 (`specs/sync/policy.md`), J2.1.1 (`specs/engine/jobs.md`). No new contract exception.
- Own source and approved specs only. No reference, oracle, private input, network, agents or panels. No Docker/native compilation or tests in this runner.

## Repair

Driver's detailed initial execution/refusal paths return `CommandAttemptFailure` with the unchanged `DriverError` and optional sealed `OriginalIssuedGuardSuperseded`. Only UpdatePending at the core-action port and ResumeMedia at native disposition/origin preparation can qualify. The actual pre-effect validation compares committed policy authority and one Runtime State snapshot. Exact original opening, generation, binding and cross-guard consistency must match; tokens must be monotonic and at least one strictly older. Original and observed guards/opening are retained inside the immutable fact. Legacy validation and late native rechecks transport no fact. The additive `Runtime::policy_original_authority` samples opening/backend guard under the existing State lock; NativeAuthority remains final-check-only.

Each FFI job keeps its error and optional Core evidence together. Its terminal event reports that same error. Three new detail accessors read this stored result; existing getters keep their behavior. The fact keeps no strong reference to its original owner. Media work supplies no such evidence.

CoreBridge reads the detailed outcome through the original ordered/drained job, outside any fresh authority query. Only UpdatePending and ResumeMedia production admission use the internal detailed path. The executor unwraps the attempt transport to the same legacy error and carries its fact in NativeResult, then SyncCommandResult. Only reducer user-error projection and Service harness unexpected/first-failure accounting recognize it. Diagnostics retain the refusal; error stays non-nil, preserving success-only UI guards. Existing outputs, ACK counts, required refreshes, follow-ups, cancellation and SDK behavior are retained.

No command flag, task-local/global classification, error reset, success conversion, synthetic completion/ACK, retry, timeout or SDK rebinding was added. A refresh, later attempt or later validation cannot supply qualification to an earlier error. Wrong-port actions never qualify.

## Added verification source

- Host: `original_refusal_attempt_distinguishes_policy_and_backend_supersession` (both actions, policy and backend-only changes, original stale accessor and no pending/cycle effects); `original_refusal_attempt_rejects_foreign_owner_binding_and_generation_without_qualification` (real issued handle, foreign opening/binding, real replacement).
- FFI: `original_attempt_detail_retains_refusal_and_failed_terminal_across_reuse_pause_and_close` (both actions, pool-held real jobs, exact stale error, failed JobFinished, no effects, distinct attempt facts, original fact after close); `original_attempt_detail_current_wrong_action_then_pause_remains_unqualified`; `original_attempt_detail_queued_cancellation_and_generation_rejection_never_qualify`; `original_attempt_detail_foreign_facade_cannot_admit_or_qualify_command`; `original_attempt_current_invalid_media_and_retired_or_missing_sdk_never_qualify`.
- CoreBridge: `nativeOriginalRefusalKeepsLegacyErrorAndImmutableAttemptEvidence`; `nativeGenuineFailureThenPauseAndStaleReexecutionKeepsOriginalFailureAndOutputs`. Instrumentation registry includes all three additive detailed-read exports.
- Service/app: `serviceAppPauseQualifiesOnlyOriginalSupersededCommand`, UpdatePending and ResumeMedia rows, using the existing before/afterExecution observation seam. Holds actual issuance, public Pause, bounded actual SDK cancellation/StopEngine signals, original error/fact, no pending/network/media/data effects (including original-command Core call observations excluding pending take and media/fetch/send admissions), real result ACK and drain, unchanged failureRevision, public Resume, distinct valid successor and real successful exchange. Gates release on all throwing paths. No drain while deliberately held.

Existing checks for current command failures, shutdown and stale UpdatePending are unchanged. The original Pause/Resume cases remain. Tests for the shared cell compare the same returned values through the new storage type. No test limits, expected results or guard rules changed.

## Status and limits

Source consistency: `git diff --check` clean; exact source comparisons confirmed both named legacy tests and the original two normal-control rows are unchanged. Corrected the initial snapshot helper to preserve the NativeAuthority final-check-only convention. One source consistency pass only; no executable pass or acceptance claim. Integrator owns Rust/UniFFI/Swift compilation, focused tests, independent scoped review, provenance/privacy/similarity checks, commit attribution/session trailers and integration. Inspect potential concurrency/fixture assumptions in the new Service rows during focused execution. Qualification intentionally excludes later media stages, final compact enqueue checks, cleanup, refresh, post-effect failures and unrelated operations. This is no signed CloudKit scheduling, recovery, wider reset or final acceptance evidence.

Compiler integration follow-up: requested upstream base `d2e52db9`; the supplied seed was already applied on filtered runner baseline `f49ead8`. MG `core-test/20261112T083117Z-68913` stopped at compilation; no tests ran. The four ordinary result accessor sites in `cloud_asset_reads.rs` and `cloud_connection_jobs.rs` now read only `stored.result` before transpose; that field has crate visibility. Ordinary `Completion::finish` is available in production with crate visibility; `finish_attempt`, storage and terminal handling are unchanged. The two flagged source comments and review wording were rephrased. Tests, assertions, time limits and guard configuration are untouched. No builds or tests were run here; the integrator must compile, verify behavior and rerun provenance/privacy/similarity checks.

Remaining source concerns found in this pass, outside the named edits: `CloudMediaNextWorkResult.result` in `cloud_media.rs` and `CloudOriginalEditResult.result` in `test_hooks.rs` still call clone/transpose on the shared stored envelope. The direct cell comparison in `asset_lifecycle_jobs_original_selector_actual_cancelled_and_absent_returns_are_distinct` in `cloud_asset_lifecycle_job_tests.rs` still expects `Option<Result<_, CoreError>>`. These appear to require the same result projection before integration can compile; they were left unchanged. The named `cloud_first_asset_read.rs` is absent from this filtered export; production ordinary completion callers are present in the asset-read, connection and media sources.

## Shared storage integration completion

Starting filtered commit: `62e7b93` (permitted source baseline); requested upstream base remains `d2e52db9`. The supplied normal Pause refusal fix and first compiler integration pass were already applied. The preceding paragraph records the earlier pass; its three remaining source concerns are addressed by this follow-up.

`CloudMediaNextWorkResult.result` and the test-hook `CloudOriginalEditResult.result` now project only `stored.result` before transpose, preserving callback entry and legacy pending/value/error returns. Both direct cell comparisons in `asset_lifecycle_jobs_original_selector_actual_cancelled_and_absent_returns_are_distinct` now project the same field; their expected `Some(Err(CoreError::Cancelled))` and `None` values and terminal assertions are unchanged.

One own-code consistency pass covered qualified and imported `policy_jobs::Cell`, `Completion`, `RequiredCompletion`, and direct and multiline cell accesses, including test-only consumers. No additional old-envelope consumer was found. Independent cell types in collection entry/export and maintenance retain their own representation. The seed's ordinary completion and four getter corrections are preserved. No behavior, qualification, assertion expectation, timing, SDK, guard configuration or exemption changes were made. Earlier failure evidence and comment replacements are retained.

No builds or tests were run in this runner. No further compile issue was identified by source inspection; compilation remains unverified. The complete export includes the seed and this follow-up. The integrator owns executable checks and provenance/privacy/similarity checks.

## Integration checkpoint

- Actual upstream starting commit: `d2e52db9`. MG formatting passed. The unchanged staged privacy hook passed after prose corrections.
- `core-test/20261112T083117Z-68913` stopped during compilation: ordinary shared-cell accessors had not been converted. The filtered follow-ups corrected all located consumers.
- `core-test/20261112T084734Z-79786` compiled production code, then stopped at one new Rust test expression moving an error in a match guard. No executable test pass is claimed.
- The new expression requires an ownership-only correction with the same comparison. Independent source review is running separately. Swift checks, native app verification and required full integration remain pending.
- After reconnection, `ui-test/20261112T084205Z-73262` reported the phone available. `drive/20261112T084436Z-76719` then failed to enable automation before its script ran. It provides no app or convergence result.

## Foreign facade fixture correction

- Starting filtered commit: `51af0d7` (permitted source baseline); requested upstream master: `28330189`. The seed's exact Rust E0507 reference comparison and reviewed conditional ACK count are preserved.
- User-reported MG `core-test/20261112T085113Z-82325`: 84 tests, 81 passed and 3 failed. This pass addresses only the new foreign-facade test; the two older Host failures remain with the integrator.
- `original_attempt_detail_foreign_facade_cannot_admit_or_qualify_command` now creates a second real admission fixture after the first owner issues its command. It asserts distinct policy Arc identity, preserves the precise immediate WrongOwner assertion, checks unchanged pending counts, cycles and emitted update IDs on both hosts, and closes both hosts.
- Production behavior, qualification rules, original tests, expected errors, timeouts and privacy guards are unchanged. Own-source inspection and one bounded source consistency pass only; no builds, tests, network or agents. Integrator owns executable verification, independent review and provenance/privacy/similarity gates. No passing-run or acceptance claim.

## Focused verification and scoped review

- `cleanroom/20261112T084601Z-78062`: no production P0-P2 findings; one P2 in the new app test. UpdatePending keeps a required refresh, so its exact result-ACK count is two. ResumeMedia has one. The test now requires those exact counts.
- `cleanroom/20261112T085539Z-87891`: the test-only corrections have no P0-P2 findings. This review does not replace the final recovery review.
- `core-test/20261112T085113Z-82325`: 81 of 84 tests passed. The new foreign-owner test reused the same cached policy object and failed; its fixture now opens a second actual Host. Two older Host cases also failed and remain open: `first_manual_capture_cannot_send_after_changed_or_deleted_root` and `native_authority_retire_isolation_precedes_held_readback_and_survives_failure`. They have not been waived or proved unrelated by a baseline run.
- `core-test/20261112T085642Z-88427`: both corrected foreign-owner and shared-cell cancelled/absent-result cases passed. This is separate focused evidence, not a fresh pass of the 84-test run.
- `swift-test/20261112T085413Z-84470`: passed the two new original-result cases, the unchanged stale-command rejection case and seven instrumentation checks.
- `swift-test/20261112T085753Z-89549`: passed. `serviceAppPauseQualifiesOnlyOriginalSupersededCommand` and `serviceAppPauseAndResumeWithMediaRetiresOriginalSDKWithoutOperationFailure` each passed both parameter values. `currentCommandFailuresStayStickyIncludingDuringShutdown` passed. The tests retain required output delivery and ACK checks.
- Formatting passed. Signed Mac/phone verification, current similarity evidence, retained failed recovery cases and the full batch gate remain pending. No completed-unit or release claim.
