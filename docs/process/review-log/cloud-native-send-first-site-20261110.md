# Native send first-site diagnostic — 2026-11-10

Role: recorded fixer. Clean start supplied by root: `5149ad9e`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Request and evidence boundary

Diagnostic only. Root supplied that native32328's first actual `requestSend`
failed with `connection.authorityChanged` while Automatic was enabled; later
`missingReply` describes an exhausted signal stream. Those facts do not locate
the original throwing call. No runtime evidence or raw logs were read here.
No workflow-snapshot comparison is weakened: root reports that the final native
authorization compares compact SDK/generation/binding/token facts.

## Change

`DebugCloudDiagnostics` adds one command-scoped DEBUG capture. It retains only
the first fixed site enum and the existing payload-free `failureTag` category.
It retains no error object, arbitrary error description, associated string,
identifier, path, record, account value, settings value or timestamp. There is
no new log sink or history. Task-local propagation isolates concurrent commands;
the send enqueue closures explicitly capture the same observation because SDK
admission uses a detached task.

The adapter observes existing send admission, pending-update application,
`beginSend`, SDK preparation, invocation reporting, final SDK/execution enqueue
authorization, enqueue, drain, completion, deferred updates, validation and finish.
Native exchange observations cover the surrounding existing Core calls. Policy
execution supplies request-preparation and exchange completion/cleanup sites.
The first captured inner `sendError` survives later reporting, finish or exchange
cleanup errors. Error objects are rethrown without wrapping; existing cancellation
mapping and operation/output/ACK ordering stay in place. A `drain` tag names
the operation join boundary. It gives no finer SDK location.

`CloudServiceAppHarness` appends `firstSite=<fixed label>
firstCategory=<fixed taxonomy>` to its existing failure diagnostic. The same
capture is available during ACK input diagnostics and after command return.
Existing first-failure retention, `Issue.record`, assertions, stream semantics
and ten-second bounds are preserved. Native four-test source is unchanged.

## Consistency and handoff

One source consistency review compared the four edited Swift files with their
runner-start copies, then corrected site labels around deferred-update validation
and the ACK-input mapping. No compilation, build, test, Unit, full gate, native
run or signed-product verification was performed. No pass or acceptance claim.
Root owns execution and independent review, including the requested single
first-positive function-only invocation covering both variants once, and the
required provenance/privacy/similarity checks.

Overlap: `CloudKitSyncAdapter.swift` is shared with the saved-removal implementer.
This patch touches `send`, `executePolicyOperation` and `runCapturedExchange`;
it only annotates existing pending-update call sites. It does not edit saved
removal callbacks, pending-update plumbing implementations, FFI, Core method APIs
or native test bodies. Root should reconcile any overlapping hunks in `send`.
No behavior fix, reprojection, owner, timer, retry, network request, authority or
settings change is introduced. Only Meguri's own permitted source/process docs
were read. No forbidden/private/Anki source, Git, network or raw-log access.

## Root verification

Independent R1 review found no P0–P2 defects in this diagnostic patch.
`swift-test/20261110T105104Z-75894` compiled, then failed both variants with
three issues. The manual case still lacks the known SDK-removal observation.
The automatic case failed earlier, at `resumeMedia` with `bridge.unavailable`;
it did not reach the send failure under investigation. No send-site cause is
proved by that run. Existing assertions and time limits remain intact.

Four source files passed privacy scan `private/20261110T105145Z-78370`.
Its note-only finding was resolved by rewriting the paragraph; the note scan
passed. Similarity `similarity/20261110T105151Z-79348` passed. No Unit or
integration gate acceptance is claimed.
