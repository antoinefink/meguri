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
