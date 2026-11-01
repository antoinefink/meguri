# Native old-zone deletion checkpoint

U113/U114 remain unaccepted. This is a component of their pending integration batch.
Starting commit: 36ba535cefb570a7350d7c18527c5d2b55d32de3.
Verification baseline: baa5461fbcfbd68684e701c55fe592959d7e4f39.
Original batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Contracts: sync/policy P4 and P10.3; sync/cloudkit-records K18.5 and K12.5;
sync/cloudkit-recovery V26.1.

## Behavior

A retained command reads the due rows of its original binding. It reads Active,
checks the selected zones, then checks that complete set again at enqueue. One
native operation submits that batch. Each result updates durable tracking from
its actual per-zone outcome. A missing callback grants no deletion success.
A reset keeps the existing isolation path. Cancellation and backend close await
the original reply. The test Service supports the same deletion boundary.

Cleanup failures remain separate from ordinary exchange failure. Copy Debug Info
includes bounded error categories, SDK codes, reset flags and retry durations.
It excludes zone names, account values, connection details and record contents.
The due count comes from retained tracking rows, so a failed deletion can be
issued at the next drain within the daily cleanup interval.

## Source and independent review

The initial filtered source run was cleanroom/20261101T044635Z-72113,
session s-20261025T134357Z-235af9a2. Fixes used session
s-20261025T162405Z-9dc656e4. Source copies used no reference or private trees.

Round 1 found two P2 issues: cleanup errors affected exchange accounting, and
cleanup diagnostics were absent from the export. cleanroom/20261101T050556Z-78364
fixed both. Round 2 was clear. The next build required test syntax repairs.
Source: cleanroom/20261101T053254Z-90818.

Round 4 examined actual runtime failures. The projection omitted the due count,
and terminal callback fixtures read SDK-wrapped getters. The empty-queue fixture
also discarded its first real command. cleanroom/20261101T054441Z-96519 corrected
these paths, added a failed-then-successful later-drain regression and retained
the exact error assertions. The callback observer is internal and disabled in
production construction. Round 5, reviewer native_bounds_review, found no
P0-P2 issue in that repair. RunCleanup and stale inventory remain open.

## Verification

core-test/20261101T050121Z-76599 exposed Rust type inference errors.
core-test/20261101T052720Z-85233 selected zero tests and is not passing evidence.
core-test/20261101T052812Z-86299 passed all nine original deletion tests.

swift-test/20261101T052858Z-86689 and 20261101T053032Z-88575 exposed test
compilation failures. After their repair, swift-test/20261101T053541Z-91640
passed seven instrumentation tests but failed deletion tests. Those failures
led to the round-4 repairs above. They are retained as failed evidence.

swift-test/20261101T054058Z-93814 passed all 18 app diagnostics, coordinator
and cleanup-failure boundary tests. The latest repair leaves those files intact.
core-format/20261101T054747Z-98000 completed after the latest source repair.
Final focused results and source scans follow below.

## Remaining acceptance

RunCleanup, conservative media collection, conflict-copy retention and stale
inventory handling remain required. The enabled full app round trip has not
passed. Earlier failed integration gates, signed CloudKit checks, platform
checks and final human evidence remain required. No Unit trailer is added.

core-test/20261101T054803Z-98418 passed all 10 deletion tests.
similarity/20261101T054907Z-109 passed the component source.
swift-test/20261101T054839Z-98821 compiled and passed instrumentation and
native callback suites. Seven fixture assertions failed: normal pending-update
commands were present, and executor close returned its typed closed error.
cleanroom/20261101T055055Z-2630 corrected those exact expectations and added
direct caller-task cancellation for both held operations. Round 6 review by
native_bounds_review found no P0-P2 finding.

Additional results:

| Run | Result |
| --- | --- |
| swift-test/20261101T055303Z-3474 | Pass, 19 tests |
| swift-test/20261101T055408Z-5067 | Pass, 78 tests |
| private/20261101T055316Z-4635 | Pass, 39 files before these notes |
| similarity/20261101T055416Z-6040 | Pass, final test delta |

App verification swift-test/20261101T055555Z-6610 passed all 18 tests against
the final core. core-lint/20261101T055607Z-7688 failed: two redundant Rust
result wrappers, plus the earlier Q11 test's upward dev dependency. These
technical failures remain open at this WIP checkpoint. The Q11 test will move
to the Host test crate; its behavior assertion stays enabled.
