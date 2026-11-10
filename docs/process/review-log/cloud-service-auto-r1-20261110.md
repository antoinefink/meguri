# Service SDK automatic-send R1 fix

- Role: recorded fixer; clean base `b36a5559` (provided by orchestrator).
- Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Scope: `CloudServiceSDK.swift`, `CloudServiceAutomaticSendTests.swift`, and minimal `CloudServiceAppHarness.swift` support. Read the implementer role file, which explicitly governs fixers, and applicable root/app/test guidance. Own source only; no forbidden/private/reference source, raw logs, git, network, builds, or test execution.

## R1 (P2)

The SDK previously cleared a Boolean wake in both send/run completion paths. An actual pending addition during a terminal empty callback could therefore disappear. The wake now carries its pending-update ordinal. Completion consumes only an observed ordinal; a later addition survives. Automatic runs retain their successful-batch progress loop; manual progress can request automatic continuation only while real pending names remain. Nil/failure does not unconditionally schedule another pass. Observation joins original runs across an actual successor without creating work.

Automatic scheduling also requires the current lifetime generation. Cancellation clears the wake and invalidates that authority. An old run's completion cannot restore it; only a successfully admitted fresh explicit network call qualifies a new generation. Detached admission, callback order, the single network slot, captured configuration, uncertain pending names, and revision-qualified removals remain in place.

The new content-free hooks hold the last empty pass's original `didSend` delivery after the original delegate handler-return notification and before SDK bookkeeping completes, and signal a real pending-addition application. At this interval the normal adapter can apply an update while network/event admission remains held. The regression establishes one explicit actual empty SDK pass, then uses only a normal durable local edit/TrackingChanged to request the later automatic batch. It requires a distinct actual Core batch, Service response and ACK, exactly one subsequent automatic run, and bounded termination. Its no-addition case requires no subsequent batch request or automatic run.

## Existing witness fixture corrections

The supplied run 20450 compiled the two existing functions/four parameter cases but reported 13 issues. No raw output was consulted. Source inspection identified two fixture intervals capable of contaminating those assertions:

- The harness used a historical fixed Core clock while the app's flush timer uses wall time. These focused fixtures now select the existing system-clock seam; other harness clients keep the historical default.
- The initial exchange's follow-up commands were not joined before changing automatic settings/retiring the initial SDK. The fixtures now join the actual original commands and ACKs first and require `operationFailed == false`.
- Direct SDK cancellation previously left the app admitting fresh commands. A minimal backend hook now places the SDK cancel/release comparison at the normal coordinator `cancelAndDrain` quiescence boundary, after admission closes and original commands receive owned shutdown cancellation. The original callback stays held across cancellation, and the original SDK-return/no-further-batch/stale-preparation/released-SDK assertions remain. The owner is then allowed to finish its real quiesce/close.
- Before final owner quiesce, cancel(false) explicitly proves that fresh preparation can enter a new SDK generation, then proves the old preparation is still refused without another batch request. This fresh trigger occurs only after the original no-further-batch assertions. Failure cleanup uses the same owner fence.

No harness failure recording, cancellation classification, failure state, original assertion or 10-second signal bound was removed or relaxed. No manual RequestSend/extra Sync Now was added to publication/native paths. Actual runtime diagnosis and any remaining contract conflict belong to orchestrator execution; these source changes do not claim the reported failures pass.

## Handoff

Focused names:

- `serviceSDKAutomaticSendUsesAdditionalOriginalBatchesForHandedBackRevision` — existing `[false, true]` automatic cases.
- `serviceSDKCancellationAndReleaseJoinOriginalAutomaticCallbackAndFenceAdmission` — existing `[false, true]` release cases.
- `serviceSDKTerminalEmptyPassRetainsOnlyNewRealPendingAddition` — new `[false, true]` addition cases.

One source consistency check before export; no compilation or tests in this fixer session. Orchestrator owns native compilation, all six focused cases, independent review, provenance/privacy/similarity verification, and integration. No unit/full-gate, signed-sync, release, or acceptance claim. Core publication token stability, peer page validation, service/native page sizing, incoming/FFI/Bridge/selected hooks, AppModel/Collection, and the parent publication-completion log are outside this patch.

## Root execution

Independent R2 source review found no P0–P2 finding.
`swift-test/20261110T014211Z-29982` failed: four parameter cases timed out
across the three functions. Manual send and the empty case without an addition
completed. Automatic send, the later addition, and both cancellation cases
remain open. No limits or assertions were changed.
`similarity/20261110T014259Z-31782` passed. Privacy flagged two redundant
comments; those comments were removed, with code and assertions retained.
No unit or integration gate acceptance is claimed.
