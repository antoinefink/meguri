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
