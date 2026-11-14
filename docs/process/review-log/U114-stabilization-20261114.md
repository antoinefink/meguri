# U114 stabilization — 2026-11-14

Approval: reviews-cloud-stabilization-approval-20261114.md. Starting commit:
6fbc9a24bb69305d882e1f5b26e276bbd9e97676. Integration base remains
1416a520542a8c5daa38a23bfe398da704c3d6c9. Earlier rounds remain counted.

## First additional round

Author cleanroom/20261114T080837Z-59757 changes two fixture files only.
InitialCloudHarness now owns one scratch container with Collection and Backups
as siblings. Its cleanup removes that container. The entry-drain fixture uses
the same layout. Production CollectionLocation defaults stay unchanged. The
shared old backup folder is neither read nor removed.

The added isolation test creates two harnesses and checks their collection and
backup paths differ. It creates one backup folder and verifies the other stays
absent. Every original assertion, argument, event, callback, clock and limit
remains unchanged.

Independent review cleanroom/20261114T081641Z-66451 finds no P0, P1 or P2
in this delta. It approves the fixture correction only.

Native run swift-test/20261114T082133Z-70421 passes nine functions and 18
argument cases on the applied patch. All six previously failed adoption
functions now pass unchanged. The isolation test, four entry-drain cases and
both original cancellation cases pass too. This establishes the shared backup
path as the cause of those six functions' earlier failures.

Earlier failed receipts stay failed: swift-test/20261114T074553Z-36446 and
swift-test/20261114T075600Z-46766. This focused result is not a full-suite pass.
Five native functions still need work. They cover asset retry, original account
proof, close/reopen, mixed/flush cancellation and the Resume start. The retained
asset exception reports a wait for retry completion.

Privacy private/20261114T082425Z-73610: pass, two files. Similarity
20261114T082426Z-73609: pass, two files, zero flags, zero exceptions.
The current U114 extension has one of two reviews complete. The U113 P1 is
still open. No Unit, P24, gate or release pass is claimed.

## Second additional round

Start: 693e09e8. Author 093411Z-21082. Final export 094707Z-25695.
This round retains the failed observations and candidates. Temporary traces
were removed before final review.

Original cancellation now reaches the proof bridge with its retained cause.
Account output marks only its own validated cancellation. Earlier errors stay
errors. The media fixture uses the SDK scheduled path; its old manual call
had no adapter completion. Resume follows the existing R11 decision.

Native 094719Z-26147 passes six functions and 17 cases. Independent review
094929Z-29425 finds no P0, P1 or P2. It changes no source. The review covers
the candidate, not unit acceptance.

Bridge 094940Z-30196 fails compilation and runs no tests. Export 095048Z-33135
adds explicit types in one new test. Runtime code, assertions and call order
stay unchanged. Bridge 095753Z-47707 compiles: 11 functions pass, two fail.
Five modes disagree with the asserted stored outcome. One old fixture has
no UpdatePending before its intended fault. These failures remain open.

Native 095102Z-33657: 156 functions, 155 pass, one fail. The earlier eleven
pass. The result file retains the failed case and its evidence.

Privacy 095911Z-49471 passes all 13 changed source/test files. Similarity
095926Z-49762 passes the same files: zero flags and zero exceptions. Static
checks 100105Z-52458 pass.

The two approved rounds are used. Further source repair is held for Antoine's
scope decision. The request names these three checks only. U113 remains open.
No Unit trailer, P24 closure, full gate, signed trial or release pass is claimed.

Typing-delta review 100313Z-55515 finds no P0, P1 or P2. It confirms unchanged
semantics, assertions, calls and gate order. This completes source review
coverage for the same second round. Both failed test runs remain failed.

## Third stabilization round

Approval: `cloud-recovery-extension-20261114.md`. Start: `270d9af3`.
Final author export: `cleanroom/20261114T120906Z-43375`.

The account fixture now expects failed after an accepted account transition;
ordinary cancellation still expects cancelled. Original Swift error, producer,
cleanup, ownership and ACK checks stay intact.

For malformed, one real policy exchange establishes the epoch before the
UpdatePending is obtained. Setup calls are counted separately. The attempt
must add no calls, drains or durable changes; its error and proof assertions
stay exact. The old baseline helper requires automatic sync off. This case
keeps automatic sync on and checks its epoch and current SDK directly.

| Check | Run | Result |
| --- | --- | --- |
| Native | swift-test/20261114T121436Z-47783 | 3 functions, 14 cases pass |
| Privacy | private/20261114T121907Z-52081 | 2 files pass |
| Similarity | 20261114T121910Z-52346 | 2 files, 0 flags, 0 exceptions |
| Static | static-checks/20261114T121923Z-52703 | pass |

Read-only review `cleanroom/20261114T121637Z-49944` finds the two fixture
corrections sound, with no P0 or additional P1/P2 in their delta. Its export
equals the author seed byte for byte. One unit P1 remains: the original
adopted-phone reopen failure from run095102 is unexplained and unrepaired.

Diagnostic runs 113533 and 113824 pass the focused and broader contexts;
113824 passes 156/156. They establish no causal repair. All temporary
diagnostics are removed. Failed runs 113134, 113345, 115227 and 120225 remain.
The review identifies preparation-to-registration media ordering as an
unproved interval, not an established cause. Round four remains available
for that finding. U113, P24, signed delivery and the batch gate remain open.
No Unit trailer or acceptance claim.

## Fourth stabilization round

Base: `1ae90c34`. This uses the second round approved by
`cloud-recovery-extension-20261114.md`; earlier counts remain unchanged.

| Receipt | Result |
| --- | --- |
| swift-test/20261114T124234Z-68059 | Original two cases fail, 15 issues |
| cleanroom/20261114T125448Z-77155 | First media repair |
| swift-test/20261114T131457Z-98916 | Two cases pass |
| swift-test/20261114T131737Z-1035 | Existing 14 native cases pass |
| cleanroom/20261114T131249Z-96665 | P1 registration handoff; P2 cleanup scope |
| core-test/20261114T131314Z-97575 | Six cases stop at command setup |
| core-test/20261114T132334Z-5317 | Explicit test-hooks has the same six failures |
| cleanroom/20261114T131903Z-2924 | Fixes handoff, cleanup and epoch setup |
| core-test/20261114T133015Z-8176 | All six focused cases pass |

P4 keeps the token during scheduling changes. Media may do no work while its
selected native engine is being built. One registration task now provides the
actual SDK result to both waiters. Failed construction ends this scope before
cleanup. An earlier prepared command keeps proof of its original SDK's first
configuration stop. Real errors and ordinary stops retain their own outcomes.

The Rust fixture lacked epoch intake. It now uses the normal record path and
checks known equal crt, the same SDK and unchanged root proof. The original
negative assertions remain. B/C fixtures are unchanged.

The final source check and broader tests are pending. Integration changed only
comments, one test identifier and diagnostic text. It kept every assertion and
all runtime code. Failed privacy receipts remain available; the guard is intact.
The historical 095102 owner is unknown. U114 has no acceptance claim.


### Final source verification and focused checks

Source review `cleanroom/20261114T132926Z-7227`: no P0/P1/P2 in the export.
Integration afterward changed comments, one test name and diagnostic text only.

| Receipt | Result |
| --- | --- |
| core-test/20261114T133239Z-10934 | FFI suite: 340 pass, 44 fail |
| core-test/20261114T133629Z-13624 | Clean a887852f comparison: 43 fail, 1 pass |
| swift-test/20261114T133842Z-15679 | Three native cases pass; cleanup has two failed expectations |
| swift-test/20261114T134441Z-21181 | Existing 14 native cases pass |
| static-checks/20261114T134551Z-22939 | Pass |
| similarity/20261114T134552Z-23199 | Pass, 12 files, zero flags |
| private/20261114T133846Z-16829 | Final source and log pass |

The export test differs between the broad run and the clean comparison. Its
Busy and zero-preflight checks passed; its tree comparison failed. Its cause
remains open. The other 43 failures reproduce on clean master.

The catch performs a cloud refresh. That result belongs in outputs; P10.1 also
allows a Changes emission. Our new test expects neither. The source/contract
reading is `build/cloud-resume-diagnosis/U114-cleanup-expectations-reading-20261114.md`.
Round-four verification will correct those new expectations to check the exact
refresh and emission. Errors, ACKs, prior IDs and data checks stay required.
This fixture correction is covered by the extension; runtime code stays fixed.

This is WIP. Required test failures, the historical failure and batch gate remain
recorded. There is no Unit or release acceptance.


### Focused verification outcome

Fixture author `cleanroom/20261114T134959Z-26679` changes only the new cleanup
case. Independent check `135246Z-28800` finds no P0/P1/P2 in that delta.
The exact refresh, ID, observation, error and ACK checks pass with data unchanged.

| Receipt | Result |
| --- | --- |
| swift-test/20261114T135323Z-29229 | All four focused native cases pass |
| swift-test/20261114T135455Z-32437 | 159 tests pass, two suites, 340.165 seconds |
| static-checks/20261114T140323Z-39008 | Pass |
| similarity/20261114T140321Z-38745 | Pass, one changed file, zero flags |
| private/20261114T135327Z-30350 | Fixture passes |

The wider native run includes the adopted-phone reopen regression. Its old
failed receipt remains unexplained; this pass does not reconstruct its cause.
The FFI failures recorded above, P24 work and required batch gate remain open.
No Unit trailer or release acceptance is added.
