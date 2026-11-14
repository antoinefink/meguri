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
