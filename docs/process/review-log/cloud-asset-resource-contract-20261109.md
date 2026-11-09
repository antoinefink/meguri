# Cloud asset resource contract

Starting commit: `7452b2144330e29a4f6f2f8891ccd6fd4bf7559e`.
Integration start: `911163310de76102cd31395f5fe3d4d9cdab885b`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Spec-writer session: `s-20261109T084309Z-3e8bd3ea`.
Ledger-only commit: `4046ecd6` (session run `92958`, pass).
Scope: `engine/jobs#J2.1.1`, `engine/suspend#S1`, `#S4`,
`engine/lifecycle#L3`, `#L8`, `#L11` remain reader prerequisites. Anki: none.

The [technical proposal](../../../build/cloud-asset-terminal-contract-proposal-20261109.md)
received independent review before these spec edits. Initial review found three
P1 findings and one P2. Revision made the registry cutoff check running jobs and
busy threads together, latched original owner/generation facts despite registry
unpause, preserved reader replacement protection without a callback-held mutex,
and settled Enqueueing rejection through false admission. A further P2 required
uncaptured Cloud readers only: refused transfer returns the original lease intact,
including an empty Sync-role lease left by an owned SyncSnapshot. Independent final review
reported no remaining P0–P2 findings. This records technical review, not Antoine's
approval, an implementation proof or unit/release acceptance.

The first sandboxed session attempt (`90755`) failed with `session-lock` before
any ledger append. Root recorded the session through host tooling. Only the
ledger commit above is counted. No build/test or private/reference source was
used for this spec amendment. The reader-only amendment and short reader log
landed in `ea99cf87`. This resource amendment is now on main for root's final
privacy/review and commit; implementation and unit acceptance remain pending.

Independent `resource_contract_review` reported no P0–P2 behavioral, race or
authority findings in the prepared resource amendment. Draft privacy run `58257`
passed all three files. The scoped Status additions record this new internal
review on 2026-11-09, without extending earlier Antoine/R2066 approval claims.
The close-state mirror permits Full only for Active; Prepared must select
DisposalOnly, matching the existing Host admission boundary.

The generic reader primitive is already verified; see
[reader lifetime pin evidence](cloud-reader-lifetime-pin-20261109.md). Its use
across the actual production callback and accounting path still needs the
submission-conversion proof. This log grants no new implementation acceptance.

## Mandatory implementation witnesses

The reader primitive evidence above is a prerequisite. The rows below still
require resource-path proof, including pin lifetime across the actual callback
and accounting return. Verify resource liveness/cutoff before submission
conversion. Keep independent reviews, focused checks,
provenance/privacy/similarity checks and coherent batch gates.

| Area | Required proof |
|---|---|
| Reader transfer | Transfer the same real uncaptured Cloud reader. Preserve ReaderLease's thread-bound ownership; the pin cannot newly permit cross-thread SQLite use. A second same-role reader cannot open or return. Native callback holds no actual core mutex; the reader-slot mutex is available. Real replacement/close cannot close the writer, swap the file or advance generation until pin return after accounting. Cover true/false enqueue, errors, cancellation, unwind and reader-close deadline. |
| Refused transfer | On a Sync-role lease, capture_sync produces an owned SyncSnapshot and leaves the lease empty. |
| | Keep that snapshot alive. Transfer must refuse the empty Sync lease intact, with replacement still blocked. |
| | The returned lease stays with its caller until Runtime settles the refused enqueue; only then may the caller choose to drop it. |
| | Separately refuse an uncaptured non-Cloud reader intact. |
| | For an empty Cloud lease, call capture_cloud and drop its borrowed snapshot. Require intact transfer refusal and continued replacement protection. |
| | Cloud capture_sync fails with TransactionOpen; it cannot create the owned-snapshot fixture. |
| Selection | Real projection/tracking contention does not block short lane admission or unrelated review start/end admission. Cancellation before selection has no effect; after selection it preserves the issued typed handle. Exhausted None differs from no body return. |
| Required resource body | Occupy the existing pool, queue and cancel complete/cancel/finish, then release workers. Each body runs once and publishes one event after its exact stored return. No job waits on a job behind it. Preserve actual typed Cancelled, panic/no-body distinction, post-effect errors and required follow-ups. |
| Retained ownership | Keep the current regression tests. |
| | Queue work across account, SDK, policy, generation and Host-close changes; create an actual successor. |
| | Its request must survive the old terminal, cancel, duplicate and finish calls. |
| | Keep accepted accounting fenced. |
| Late calls | Test retry and first-read handles in Prepared and after pause, close/unpause, resume, replacement to Ready and reopen. |
| | A DisposalOnly call never upgrades and accesses no provider, tracking, reader, writer or SDK. |
| | Complete returns CoreError::Unavailable with exact reason `The result cannot be applied.`. |
| | Preserve error mappings for Full bodies and synchronous calls. |
| Cutoff and deadline | Register calls on each side of the cutoff. Hold a pool worker after its job leaves the running map but before its busy guard drops; teardown must wait. Under the registry mutex, inspect both jobs and busy threads, releasing locks before retries or waits. Drain earlier work before releasing resources. Explicit later cleanup cannot access released stores. An expired deadline must report incomplete teardown. |
| Final submission | After real reader/file/selection preparation, hold real Driver and Runtime projections. Final compact original command/SDK validation and synchronous enqueue stay short; worker preparation/accounting may wait. No application await, fresh owner, provider or tracking acquisition intervenes. Preserve physical file/publication guards and exact error precedence. |
| Enqueueing refusal | After Enqueueing, cancellation/compact rejection/transfer refusal invokes no native call, consumes no attempt or grant and leaves no owner. Finish Runtime's false-enqueue path before returning the exact rejection. Selected-only cancel cannot settle Enqueueing. |
| Accepted submission | Inject accounting failure after true enqueue. Original receipt remains accepted; join its terminal exactly once, retire only original Core work, and retain outstanding accounting. False enqueue consumes no attempt or grant. Preserve retry limits and uncharged first-conflict reads. |
| Swift drain and setup | Preserve CloudAssetReadExecutorTests.swift's original line-166 test. Add barriers inside queued Core completion and finish so concurrent close cannot return early. Fail listener/tracking setup after admission, cancel and drain, and retain actual result ahead of setup errors. Keep staged-file leases through completion and failures. |
| Activity | Original terminal precedes Core completion, finish and one terminal activity round. Raw errors precede Ended; empty pass, app cancel, activity authority refusal and producer-before-pause ordering keep existing behavior. Late Ended still refuses paused admission. |

Add each `sync/policy#P24` rule's named `conformance/cloudkit-acceptance#Q11`
test before building on that rule. This amendment closes no P24 item and changes
no retry, grant, intake, writer/WAL, ACK or final-evidence requirement. It adds no
thread, global reservation, long-operation slot, per-attempt Core token, rollback
or accounting guarantee. The synchronous compatibility APIs remain; unbounded
Swift maintenance-lane fallback is not final J2.1.1 compliance.
