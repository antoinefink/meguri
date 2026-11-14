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

## Second and last approved pass — causal local-entry ordering repair

Session remains `s-20261110T214025Z-20fa15fd`. Filtered source: `889384d3`,
with only the supplied R1 P2 exact Constraint assertion seed. This repair and
its preceding diagnostic author are stages of the same second/last approved
pass, not further review rounds. Integration base `1416a520` and prior native
slice `a447a4d6`, every earlier failure and the complete history/cap remain.
Root owns the final independent U114 review; further rounds require an owner
decision. Global attention completeness remains **false**.

Run `swift-test/20261114T063329Z-75532` keeps both native test arguments.
Its trace comes from `cleanroom/20261114T062547Z-71440`, with privacy pass
`private/20261114T063105Z-74490`. The full trace patch is removed afterwards.
The expiry branch returns its original receipt after a successful refresh.
In the replacement branch, each command keeps its receipt through cleanup
and ACK. The following refresh returns `generationChanged`. That later error
is real and must remain visible. The source repair changes the activation
order, leaving the executor's error rules intact.

Source confirms that local confirmation previously entered Core activation
before native cancellation/drain. Only the later CollectionReplaced event
started the replacement drain, after Core had invalidated the old generation.
Authority for the repair: RF2.3–RF2.4, `engine/lifecycle` L7–L8,
`engine/suspend` S1/S4, `engine/jobs` J2.1.1 and unchanged `sync/policy` P4/P15.

Confirmation now synchronously holds the existing sync owner admission and
records replacement cancellation on original command tasks before revoking
their proof waits. Before calling the real activation service, the entry task
joins the existing owner quiesce/result/required-follow-up/ACK/backend-close
sequence while Core still owns the old generation. Account reads retain that
owner-close path's exact original generated-refusal/shutdown checks. Command
cancellation remains replacement intent and needs its actual native receipt.
No result/error/freshness guard is relaxed. Shutdown/quit/background and initial
adoption use their original drain behavior; no generic close architecture is
introduced.

The entry hold keeps the already configured platform, with no second bootstrap
or new native owner during activation. Only terminal retained-host setup
releases it and attaches a fresh backend. Pre-commit cancellation or stale
confirmation reattaches to Core's still-ready original generation. Failed drain
prevents activation, retains the original backend and the admission fence, and
reports failure; it does not automatically retry. Recovery-required outcomes
retain the fence; explicit Continue joins its drain before retained-host setup.
Ordinary close of an already drained recovery hold clears that entry state for
the original reopen path. No preview, replacement intent or successor supplies
Core or SDK authority.

Added actual local-entry regression
`ReviewCollectionStartupTests/entryDrainsOldSyncBeforeActivationAndRestoresRetainedHost`
uses real inspection/activation and Core lifecycle queries, gates the existing
transport-close boundary and activation-service entry, and covers successful
replacement, actual stale refusal, actual cancellation and failed drain. It
checks old-generation liveness until close, blocked new work across activation,
fresh attachment/settings replay, unchanged-generation recovery and retained
failure. The original native cancellation test and harness are wholly unchanged.
The seed strengthens only the quiet database-failure assertion to exact
`RuntimeError::Tracking(meguri_sync::cloud::Error::Constraint)`; all other
assertions remain. Quiet `889384d3`, strict sends, callback namespaces, SDK
receipt checks, original removal/ACK order and later refresh-error precedence
are unchanged. No diagnostic helper or tracing API is added.

Root integration: apply this exported delta including its supplied seed on
`889384d3`, separately from U113. Only collection-entry ownership/drain code,
its new regression, that one Rust assertion and this log change. Leave journal,
row layout and Core projection untouched. Use the exact native and Rust
selectors above, keeping both original native arguments; also select:

```text
ReviewCollectionStartupTests/entryDrainsOldSyncBeforeActivationAndRestoresRetainedHost
ReviewCollectionStartupTests/recoveryEvidenceStaysOnRealOwnerAndCannotRestartImport
ReviewCollectionStartupTests/cancelledMalformedAndStaleEntriesKeepRealOldCollection
ReviewCollectionStartupTests/missingCloudConfigurationKeepsRealLocalChangesAndReviewsUsable
ReviewCollectionStartupTests/genuineShutdownResultFinalizesOnOriginalAuthorityBeforeNativeClose
```

The filtered author checks source consistency and exports the patch. Root runs
the named tests and records their actual results below. Independent review,
privacy, similarity and the required integration gate remain separate checks.
No unit or release acceptance follows from this source export alone.

## Final focused results for the approved extension

Independent review `cleanroom/20261114T064637Z-82967` finds no P0/P1
and no new P2 in the second-pass source. The two additional reviews are now
used; prior rounds remain recorded. The original native test and its harness
are unchanged. Temporary trace code is absent.

Build `swift-test/20261114T064751Z-83930` runs no tests: a new nonthrowing
test callback contains a throwing assertion. Export
`cleanroom/20261114T065025Z-86825` wraps that complete assertion in a
`do/catch` which records the error. Root verifies that the equality, remaining
assertions and all production files are unchanged. No failure is hidden.

`swift-test/20261114T065441Z-88706` passes all six selected functions, with
11 argument cases. Both original expiry/reopen and replacement cases pass.
The four import cases cover activation, stale refusal, cancellation and a
failed drain. Recovery, missing cloud configuration, preservation of the old
collection and the original shutdown result also pass.

`core-test/20261114T065646Z-90658` passes six checks, including the exact
SQLite Constraint error. Privacy `private/20261114T065447Z-89830` passes
all six paths. Similarity `20261114T064921Z-86420` covers five source paths;
`20261114T065651Z-90963` refreshes the sole test-file change. Both pass with
no flags or exceptions. Earlier compile/test failures remain in their runs.

These results resolve the recorded quiet and paired native cancellation defects
within this extension. They do not prove a new real-device round trip, complete
U114, close P24 or pass the required integration gate. The separate U113 R8
persistence finding remains open. No Unit trailer or release acceptance.
