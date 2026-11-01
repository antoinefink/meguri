# Delayed zone inventory regression

U113/U114 tests-first checkpoint; neither unit is accepted.
Starting commit:36ba535cefb570a7350d7c18527c5d2b55d32de3.
Original integration base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Contracts: conformance/cloudkit-acceptance Q11, sync/policy P10.3,
sync/cloudkit-recovery V26.1. The related P24 entry remains open.

The test publishes two epochs through the actual Service and publication rig.
It captures an inventory response, performs an authorized old-zone deletion,
requires the actual successful reply and immediate tracking-row removal, then
handles that exact earlier inventory. It inspects live and reopened tracking,
while independently checking that the server kept the current zone and Active.
No tracking model, seeded acknowledgement or cleanup fix is included.

Source cleanroom/20261101T045826Z-75248 used implementer session
s-20261025T134357Z-235af9a2. The first run, core-test/20261101T050631Z-78790,
failed at fixture open before the intended assertion. Fixer
cleanroom/20261101T050817Z-79375 supplied the actual saved publication decision
and checked native file identity. core-test/20261101T051138Z-80330 then exposed
its missing direct Host test dependency. cleanroom/20261101T051230Z-80724
added only the existing workspace dev dependency and lock entry. Both fixes used
session s-20261025T162405Z-9dc656e4. Production dependencies are unchanged.

Actual behavior RED: core-test/20261101T051421Z-81417 compiles and reaches the
required assertion. The earlier inventory reinserts the successfully deleted
zone row. This is the regression to repair; the two earlier failures are not
claimed as behavior evidence. The test remains enabled.

Independent reviewer native_conformance_lifecycle_audit found no P0-P2 findings.
The reviewer confirmed original Service outcomes, matching published ownership,
retention eligibility and durable-state assertions. A future retained listing
API may change the delivery call while preserving these assertions.

core-format/20261101T051611Z-82072 applied the standard formatter.
private/20261101T051614Z-82348 passes all three source/metadata files.
similarity/20261101T051617Z-82633 passes the test source.
The fix, native listing transport, complete RunCleanup and all original
integration/milestone evidence remain required. No Unit trailer is added.
