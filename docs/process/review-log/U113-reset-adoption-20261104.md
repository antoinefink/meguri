# U113 adoption after encrypted-data reset

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: dcc783ef.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Export32178 removes the reset-only adoption refusal and adds a separate test
target. Independent review finds no P0–P2 issues. The tests deliver real Service
reset errors, recover the cloud copy, and check adoption, media, ACKs, reopen,
review, cancellation, failed validation and staging restart.

Private42763 flags one comment. Export43157 removes only that comment;
private43933 passes. Format45138 formats the test target. Only the tests are
retained here. The production guard remains until the tests run against it.
Disk headroom must be restored before compilation and behavioral checks.
No semantic RED, passing runtime checks or full reset acceptance is claimed.

Core58796 compiles this target, but all four cases fail before reset injection
in the normal sender-adoption setup with RuntimeStaleIncarnation. This is not
semantic RED for the reset guard. Core65740 passes ordinary first adoption.
Source inspection identifies ordinary media replay during activation after the
old binding is marked unusable. A focused witness and repair are required before
continuing the reset cases. The reset-only guard remains unchanged.
