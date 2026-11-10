# Native first-stop diagnostic — source-only WIP

Recorded fixer in the filtered runner. Starting baseline: supplied current clean
master, exact commit retained by root. Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Run `swift-test/20261110T120258Z-86282` completed the manual path. Its automatic
path reported `sdkLocked check=stopping`. The stop trigger is unknown.
prepareExecution does not retire an SDK. No behavior fix is made here.

Only CloudKitSyncAdapter gains diagnostics. DEBUG per-owned-instance storage
retains the first closed phase and fixed failure tag, discarding error payloads.
The existing private DebugCloudDiagnostics.failureTag classifier is reused via
its internal Value(.commandResult, error:).failure projection; no diagnostics
API or other source file changes. Existing handleEvent errors/receipt diagnostics,
handlerReturned, saved-pending-removal failures, post-return pending application,
and nextBatch preparation/build failures record at their original failure paths.
Payload-free deliveryFailed uses the literal tag deliveryFailed. The post-return
public drain still selects current exactly as before; its diagnostic attributes
the error to that selected instance, never a different SDK's global lastFailure.

At the first accepted beginStop for an owned instance, stdout prints one line:

```text
cloudNative.firstStop reason=<closed reason> policyStop=<bool> final=<bool> firstFailurePhase=<closed phase or none> firstFailureTag=<fixed tag or none>
```

Reasons identify registration failure, direct/issued automatic reconfiguration,
held/final/issued policy stop, invalid pending-action authority, failed/invalid
returned handler, post-return updates or nextBatch failure. No instance ID,
record, account, settings, path, payload, reason text or free-form error is logged.
Existing stop-task reuse precedes the diagnostic; repeated calls cannot replace
the first line, even after a failed stop attempt. Storage/classification/print are
DEBUG-only; the private reason parameter only carries fixed callsite identity.
All original error objects, failure mutations/order, callbacks, authority reads,
actualFailed behavior and drain/cancel order remain. No extra Core call, wait,
retry, send, manual exchange, failure suppression or quietedIssue.record change.
Existing Core invocation/SDK and Driver diagnostics remain unchanged.

One source-only consistency check, no builds/tests/Git/network/raw logs or
forbidden/private sources. Root owns compilation, both variants of
selectedOriginalRevisionACKAndSettledPeerApplyUseRealService(earlierAutomaticACK:)
and the 17-case selector in cloud-reconcile-observer-provenance-20261110.md,
independent review and provenance/privacy/similarity checks. The first-stop line
must come from root's actual focused stdout; no runtime evidence is claimed here.
No Unit, gate, signed-product or acceptance claim.

Consistency-check outcome: the one source-only checker confirmed all original
Host assertions unchanged, then stopped on its own overbroad Phase A comparison:
it included the intentionally removed helper definitions before the fixture.
This is a checker-boundary error, not evidence of a Phase A change. It was not
rerun; its later Release-equivalence and diagnostic-field checks did not execute.
Root must independently review/compile/verify the exported source.

## Root result

Independent R1 source review found no P0–P2 issue. Privacy and similarity passed.
`swift-test/20261110T121921Z-56677` compiled and ran both variants. Manual
exchange completed. Automatic exchange failed with media Tracking /
RecoveryRequired at runtimePreparation. The first stop was
policyConfigureAutomatic, with no earlier recorded failure on that instance.
This identifies the next error boundary, not its precise cause. The SDK stopping
guard remains intact. Signed multi-device sync remains unproved.
