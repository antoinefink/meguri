# U114 quiet metadata admission — extra pass 1

Role: fixer, session `s-20261110T214025Z-20fa15fd`. Filtered source supplied
by root: master `f37683bd`. Original integration base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Authority: `sync/policy` P6, P14.3, P19; `sync/cloudkit-records` K12.1 rule 3;
`sync/cloudkit-recovery` V10.2; `engine/jobs` J2.1.1. This is extra pass 1
under `reviews-cloud-two-round-approval-20261114.md`, across the quiet and
cancellation issues together. Extra pass 2 requires root's later authorization.
Older U114 history/extensions and all six native-flush review rounds from
`a447a4d6` in `cloud-parked-flush-first-cause-fix-20261113.md` remain counted.
No acceptance or repair cap is reset.

## Source repair and causal evidence

`native-quiet-gate-diagnosis-20261114.md` records the same-lock native evidence:
`swift-test/20261114T025312Z-76011` selects only the strict collection-send
root prerequisite at the rejected quiet write. That trace was reversed.
The baseline `swift-test/20261114T024532Z-73394` remains failed evidence,
including the separate replacement `generationChanged` result.

Factor the original Runtime gate into a general gate and its additional send
check. Only `policy_record_quiet_with_work` uses the general gate. It retains
the original adapter guard and the existing usable/verified binding, crt,
publication hold, whole reconciliation, pause, incoming, tracking failure,
known/durable loss and epoch-verification checks. Driver ownership, lifecycle,
account/file authority, issued quiet proof and freshness checks are unchanged.
Every existing `gate_locked` caller, including batch construction and physical
send paths, still requires root confirmation or the existing publisher exemption.
The quiet stamp changes neither root authority nor exchange/delivery evidence.
Real tracking errors retain their required stop and typed failure.

The Host regression fixture extends the existing baseline's saved-SDK-state
choice independently of root confirmation. Accepted epoch authority still comes
from the existing Service-backed epoch fixture. Actual SDK will/did/finish
boundaries drain unchanged zones without a stored Active record. This reproduces
the gate facts; it does not explain why the native run's root was late.
The four tests cover durable reopen, refusal of a real dirty collection batch
and manual send after stamping, cancellation without consuming the candidate,
stale command rejection after Pause, and a real SQLite constraint failure with
required StopEngine. Explicit PolicyWork cancellation/checks distinguish a
latched write attempt from a pre-effect rejection. Global attention completeness
stays false.

## Cancellation diagnosis remains unresolved

Inspected `nativeOriginalFetchCancellationMatchesExpiryOrReplacementIntent`
and the actual coordinator, executor and adapter paths. The test retains two
different issued fetch commands and verifies each result/ACK exactly once,
with original task/backend/generation/revision and native receipt ownership.
The adapter retains original-instance witnesses and revokes dependency parks
before producing cancellation qualification. The executor retains later refresh
failure precedence; cleanup or refresh failure can legitimately prevent an owned
cancellation result. Source inspection alone does not identify which boundary
produced the recorded replacement `generationChanged`.

No cancellation source, test assertion, callback namespace, timing/heap bound,
SDK receipt rule or removal ACK ordering changes. No instrumentation is exported:
the known quiet repair should receive the next native run before deciding whether
a same-boundary scalar trace is needed. Replacement intent alone supplies no
owned-cancellation proof. The paired native failure remains unresolved.

## Handoff checks

Filtered-runner work is source-only: one consistency check; no builds, tests,
network, subagents or commits. Root owns native execution, independent review,
privacy/provenance/similarity verification and integration of this unit separately
from U113. No U113 row/incoming/layout code is changed.

Exact focused Rust selector (through `tools/mg core-test -- -E`):

```text
package(meguri-host) & binary(cloud_policy_driver) & (test(/^quiet_without_root_/) | test(/^actual_sdk_drains_qualify_quiet_only_after_both_zones_and_durable_evidence_write$/) | test(/^first_manual_capture_before_root_selects_pending_revisions_without_admitting_sends$/))
```

Exact native selectors (through `tools/mg swift-test --filter`, using the
appropriate package as root's existing job does):

```text
ReviewCollectionStartupTests/nativeOriginalFetchCancellationMatchesExpiryOrReplacementIntent
EventTests/nativeParkedFlushRequiresOwnFirstCauseAndSuccessfulOriginalCancellation
EventTests/nativeRequiredRefreshFailureReplacesExactCommandQualification
EventTests/nativeDeferredRequiredRefreshFailurePreservesOriginalFaultAndOutputPrefix
EventTests/nativeFlushCancellationWithoutOriginalSDKReturnsNoReceipt
EventTests/nativeReplacementCancellationReceiptCannotCompletePredecessor
```

The first native selector includes both original close/reopen and replacement
arguments. Keep both enabled. These are proposed selectors, not execution
receipts. No Unit, P24, full gate, milestone, signed SDK trial or release claim.

## Verification after integration

Focused Rust `core-test/20261114T062131Z-66978` passes all six selected
cases. Independent review `cleanroom/20261114T062107Z-66485` finds no
P0/P1 in this delta. It retains one P2: assert the exact SQLite constraint
error as well as the required stop and missing stamp. This is the first of
the two approved additional passes for U114; the older history is unchanged.

The unchanged native test runs both arguments in
`swift-test/20261114T062205Z-67593` and remains failed. The close/reopen
argument reaches its end without the earlier quiet error. The replacement
argument retains the original cancellation-ownership failures and
`cloudPolicy.generationChanged`. No test, assertion or limit changes.
This closes neither that defect nor U114 acceptance; the second approved pass
will use this narrower failure to diagnose the original command path.

Privacy `private/20261114T062135Z-67272` passes four files. Similarity
`20261114T062209Z-68731` passes three source files with no flags or
exceptions. These are focused checks, not the required full integration gate.
