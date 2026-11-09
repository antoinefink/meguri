# Original-SDK Ended completion contract

Status: R2066 technical review complete. This future mode is ready to implement; no Unit, integration or release acceptance is recorded.
Starting commit: `03abad1a0950d38fac1f32bc607904449050018a`.
Spec writer: `s-20261108T190858Z-d79453ef`.
Scope: `engine/jobs#J2.1.1`, `engine/suspend#S1`, `engine/suspend#S4`.

The ordinary registry drops paused or queued-cancelled closures before their checks run. Its ordinary typed completion destructor can then supply Cancelled. The reviewed future mode runs an already-registered original-SDK Ended body despite cancellation, on the existing pool. It preserves actual authority checks and exact body outcomes; it does not admit new work through pause.

## Written technical review and closures

[Technical proposal](../../../build/cloud-ended-completion-proposal-20261109.md).


| Finding | Closure in the adopted contract |
|---|---|
| Production suspend producer drain was incorrectly left unknown | Source confirms `ReviewCollection.swift:1127–1130` awaits `backgroundAfterLocalFlush` before suspend; `SyncCoordinator.swift:856–861` drains operations before freeze; `NativeCollectionCloudBackend.swift:149–152` calls adapter freeze, which waits for handlers/networks/applying work (`CloudKitSyncAdapter.swift:621–632`). Preserve this order and prove the new queued selector adds no normal-path refusal. |
| Paused refusal was presented as equivalent to the synchronous selector | The queued completion selector newly refuses paused admission even with a valid original SDK. The existing synchronous Rust wrapper is unchanged. Normal producers finish before pause; direct Rust or deliberately late starts must test the new refusal explicitly. |

The review also required two implementation constraints, now explicit:

- Ignoring cancellation is separate from the first-Core-effect latch. Recheck original Host/generation/command/SDK after lock delay, including the `cancel_native_activity` clear-active boundary. Registration is not an effect and cannot bypass those checks.
- Apply the result-state and listener-failure rules in `engine/jobs#J2.1.1`.

The completion category joins the existing early Host-cleanup and Runtime-suspend drains. Deadlines, ordinary flush order, paused admission and one-terminal-event rules remain. This adds no network authority, SDK call, thread, reservation, rollback or per-attempt accounting semantics. It is not a dormant Started receipt design.

## Required implementation proof

Proposed witness names identify required coverage, not tests already implemented or passed:

- `required_sdk_completion_runs_after_queued_cancel_once`: hold all existing pool workers, admit/cancel completion, then prove one body, exact result and one terminal event; no SDK call.
- `required_sdk_completion_rechecks_original_after_driver_wait`: change Host/generation/SDK during lock contention; reject stale mutation, including clear-active cancellation. No fake effect at entry.
- `required_sdk_completion_retains_post_effect_result`: retain real outputs, typed projection failure and required follow-ups after cancellation.
- `required_sdk_completion_actual_cancelled_survives_listener_failure`: distinguish Some(Err(Cancelled)) from None, preserve exact body result and retain ordinary placeholder behavior; include setup failure, paused no-body refusal and panic.
- `required_sdk_completion_early_drain_precedes_runtime_teardown`: queue an accepted completion or pause its execution; require its actual result or authority refusal before Runtime teardown or connection release. On deadline expiry, report that teardown is incomplete.
- `required_sdk_completion_production_suspend_drains_before_pause`: preserve the confirmed normal suspend/close producer chain; separately prove late direct Rust admission refuses even with a valid SDK.
- Preserve existing send/fetch ownership, Started–SDK–Ended order, inline callbacks, error-before-Ended, app-cancel non-failing-cycle behavior, same-kind aggregation and ordinary job defaults.

Source anchors: `jobs/registry.rs::start`, `wait_connection_replies_until`, `wait_host_recovery_until`; `cloud_policy_jobs.rs::Completion::drop`; `PolicyWork`; `CloudPolicyService.runOrdered`; `cancel_native_activity`; lifecycle close/quiesce; original adapter network counters.

This contract concerns future completion work only. The disjoint resource-port cleanroom copies were already sealed at the starting commit; this record does not review, modify or accept them. No source change, test, build or verification receipt is claimed here. Parent review, privacy checks and commit remain separate.
