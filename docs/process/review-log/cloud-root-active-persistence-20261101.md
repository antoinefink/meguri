Root pointer recovery, 2026-11-01.

U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Component start: aeb9e555.
Fixer session: s-20261025T162405Z-9dc656e4.
Authority: sync/cloudkit-recovery V26.1; sync/policy P24;
conformance/cloudkit-acceptance Q11.

A valid Active read was traced but not saved on missing-zone isolation.
The first regression saves a newer same-epoch version through Service, then
requires its exact envelope and system fields immediately and after reopen.
Read-only test IPC exposes the stored adapter fact through the existing snapshot.

core-test/20261101T182721Z-79457 failed at the intended missing-write assertion.
R1 writes Active and isolates in one transaction. Its first run,
core-test/20261101T183147Z-87319, passes both exact-data checks, then fails a
fixture assumption that an isolated, unstarted owner's identifier must change.
The test now delivers the late read and checks that it cannot grant authority.
Production identifier allocation and guards stay unchanged.

Independent R1 review found P2: normal confirmation and ordinary epoch changes
also need to save Active. Root-drain fallback can otherwise use the older fact.
Two actual-Service regressions cover record-ID reads and completed root fetches.
core-test/20261101T184156Z-94524 ended with a helper compiler error.
The Sync test crate has no CoreHost dependency. Move the helper to testkit,
which already owns CoreHost. No dependency was added.
core-test/20261101T184853Z-96276 then compiled: missing-zone passed, both new
cases failed at their exact stored-data assertions.

R2 saves Active with RootAck in one transaction for normal confirmation, and
Active with isolation in one transaction for every isolation result.
Active is written before the binding update so the existing trigger clears
absence state. Runtime cache changes follow a successful commit and readback.
Isolation grants no RootConfirmed and opens no builder. Account, file,
instance, scan and publication guards stay enabled.

Independent R2 review and the helper-move recheck found no P0-P2 findings.
core-test/20261101T184946Z-96902 PASS3/3, 26 unselected: all new paths plus
the existing 12 proof permutations, reopen, late-read, pending and ACK checks.
core-format/20261101T184929Z-96627 PASS.
private/20261101T185103Z-97334 PASS on six source files.
similarity/20261101T185105Z-97575 PASS on the same source.

core-test/20261101T185129Z-97904 PASS29/29: the full recovery target.
Common CloudKit tests will run once with the pending media repair. The P24 root item is not closed
by this code commit; its full requirement still needs spec review.
No Unit trailer, batch, milestone, signed-device or release acceptance.
The VM is being resized by Antoine; no VM job was started.
