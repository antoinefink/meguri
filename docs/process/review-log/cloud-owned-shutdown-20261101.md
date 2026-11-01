Owned shutdown fix, 2026-11-01. Role: fixer; clean-room source only (R1955).
Seed start: 6ee50dc4. Integration head supplied: b0605802.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch start: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Specs: sync/policy P4, P11.4, P12.6–7, P13.1; engine/lifecycle L3, L10.

R1 repair tracks cancellation origin per original task and owner revision.
Only shutdown-owned CancellationError is quieted; other failures remain sticky.
Drain still waits for acknowledgement. Retired owners reject new work.
Service diagnostics use fixed categories and retain original error values.
R2 P1 resolution keeps required close effects from either output variant.
Backend retention now spans result handling and task drain.
R2 P2 resolution preserves an earlier background-expiry cancellation origin.
Only current tasks that have not been cancelled receive shutdown ownership.
Independent R2 coordinator source review reported no P0–P2 findings.

Eight new test names and their assertions are retained, using deterministic gates.
Coverage includes close ordering, required effects, failures, retry and owner fences.
Integrator reported compilation failure in cloud-owned-shutdown-focused-tests-20261101.log.
Fixture passed String to MaintenanceLane.call; later typed-throws errors cascaded.
Fix uses @testable Instrumentation and BridgeCall("testCloudPrepareAdapterBaseline").
Source signatures permit the existing untyped outer do/catch to rethrow errors.
Three flagged comment regions were removed; test logic was preserved.
One source consistency check only; compile fix remains UNRUN.

Native checks are still pending.
After stop, the old Swift object has no authority to start new work.
Required finalization must finish before that authority ends.
Retain swift-test/20261101T140513Z-968 and its five native failures.
Integrator owns focused/native runs, review, provenance, privacy, similarity and gates.
No acceptance claimed. No builds, tests, history, network or forbidden reads here.
R2 compile fix: mark the baseline closure @Sendable for MaintenanceLane transfer; UNRUN.

Root: swift-test/20261101T144351Z-17936 compiled; 15/21 passed. Six cases
stopped at fixture setup: backups were inside the collection directory.
Use sibling directories in the test. The safety check stays enabled.
Native workflow and the six blocked cases remain unaccepted.

Layout correction start: 18ac384a. The test creates collection and backups
as separate children of its temporary parent. Assertions and cleanup stay
unchanged. Independent review found no P0–P2; focused execution is pending.

Native: swift-test/20261101T144749Z-20763 PASS2/2. Reviews, media, automatic
asset recovery and final reopen pass against the synthetic Service. The large
exchanges took 227/178 ms, previously 9391/9310 ms; the limit stayed ten seconds.
Live CloudKit and close with an original operation held still need proof.

Focused: swift-test/20261101T144959Z-22621 compiled, 15/21 passed. Directory
setup now succeeds. Six cases fail because the helper has no issued resumeMedia
command. Replace its manual adapter setup with the proven ServiceAppHarness
startup. Preserve opaque handles and all scheduler assertions.

Helper replacement: integration start 71901282; filtered source start bd5c1d02.
Retain swift-test/20261101T144959Z-22621: 6/21 fail at resumeMedia capture.
Reuse ServiceAppHarness adopt/exchange; observe first real resumeMedia/updatePending.
Read-only callback records/yields; existing ten-second signal waits for issuance.
Copied handles enter only fake execution; actual host stays owned by the harness.
Require actual owner close and clean Service exit; failure cancels/joins before close.
Six native fixture cases are Mac-guarded; both pure-fake cases stay platform-generic.
All eight test bodies are unchanged. Helper replacement UNRUN; no acceptance.
Integrator owns all 21 focused cases, native two-case follow-up, review and guards.

Real-command fixture run: swift-test/20261101T150338Z-27960, 19/23 passed.
Four tests fail in actual native close: updatePending reaches a backend whose
SDK has already stopped, and the original guard rejects it as stale. Both
full review/media workflows pass. Required finalization must finish before
SDK authority ends; the next repair is a two-phase native close.

Two-phase native close, integration start cf4dbe76.
The original U113 and batch starts above remain unchanged.
Filtered exports: cleanroom/20261101T152322Z-61413 (initial quiesce),
20261101T153454Z-69044 (deferred stop), 20261101T155104Z-77178
(compiler capture), and 20261101T160018Z-83654 (fixture corrections).

The coordinator quiesces native transports, joins original results and required
control effects, then retires/releases the original SDK. Deferred StopEngine
keeps the accepted Core command and SDK until close completes.
A stop error keeps both objects for retry. New work is fenced; original callback
bookkeeping stays valid. Production guards and deadlines remain enabled.

R1 P1: StopEngine ended SDK authority before required commands completed.
R2 resolved it; R3 found no P0-P1 or other P2 issues.
Tests run issued commands through the production Service.

Retain swift-test/20261101T153544Z-70301, the failed Sendable capture build.
The correction captures the immutable Sendable engine, not its mutable Instance.
Retain swift-test/20261101T155312Z-78666: 58/62 passed. Four new fixtures had
incorrect token assumptions. Settings deliberately revoked the old token;
tracking-failure/probe recovery now supplies the intended same-authority path.
A changed adapter guard precedes the stale-command test.
Retain swift-test/20261101T155628Z-81179: 23/24 passed. The settled exchange did
not guarantee the new fixture's UpdatePending. The repair commits a real Note
edit, flushes and awaits tracking, then gates its actual command before execution.
The result and command gates release after quiescence, including failure cleanup.

swift-test/20261101T160732Z-87035 PASS24/24: all previous focused app checks,
review/media persistence, automatic asset retry/reopen, cancellation provenance,
error state, close retry, old-owner rejection and native command completion.
private/20261101T160719Z-86742 PASS on the final staged source.

Adapter: swift-test/20261101T161014Z-90282 PASS7/7, all new quiesce and
stop regressions, including the four corrected cases. The unchanged 58 checks
from the earlier run are retained separately; no fresh PASS62/62 is claimed.
Similarity: similarity/20261101T161114Z-93561 PASS on all changed code.

No Unit trailer or U113/U114,
batch, milestone or release acceptance. Live signed CloudKit, mobile and final
human evidence remain required. The original failed integration gate stays open.
