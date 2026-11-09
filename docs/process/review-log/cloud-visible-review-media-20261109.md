# Visible review media priority

Original prepared start: `783efd28`. Resumed start: `e1f027dc`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Filtered fixer session: `s-20261103T133750Z-f276f58e`.
Contract: `sync/policy#P13.2`, R1584. Anki: none.

Core media references now reach card payloads and the review owner.
The current card and rendered head publish their deduplicated names through generation-scoped callbacks.
Leaving review clears them. Desired names survive backend attachment, media toggles and successful resume.
Settings precede delivery; ordinary unchanged updates do not duplicate commands.
This changes no controls, layout, HTML, CSS or authored card styling.

Independent reviews found delivery marked while media was disabled, then unchanged references lost across Runtime resume.
The final repair covers both completed suspension and already-active Core resume after failed background preparation.
Round 4 reports no P0–P2 findings. New tests observe actual Rust priority commands through the synthetic service harness.
They preserve the same references across both resume paths and check normal deduplication afterward.
Other coverage includes late renders, cache hits, attachment failure and generation isolation.

Privacy `20261109T164755Z-22729` passes all ten files.
Similarity `20261109T164758Z-22983` passes all ten source files without an allowlist.
Native verification remains failed, as recorded below.
The original batch gate and real signed round trip remain pending.
U113, U114 and U151 remain unaccepted.

Native `20261109T163201Z-7412` built the Apple core slices and app modules but failed test compilation.
Two existing fixture adapters still expected the old one-callback network initializer.
Filtered export `20261109T163911Z-17514` updates both to await preparation before final validation.
ServiceSDK also rejoins event bookkeeping after preparation, preserving the original lifetime, generation and drain.
Independent review finds no P0–P2 issue in this test-only correction.
The failed run remains retained; it proves no Swift test passed.

## Retained native failure

Native `20261109T164120Z-18334` compiles and runs 41 checks but records five issues.
The new resume fixture assumed one backend, although adoption creates a replacement.
It also resumed Core before native commands and their acknowledgements had joined.
Filtered export `20261109T164522Z-21810` corrects those setup errors.
The test now checks the retained backend identity, baseline count and generation separately.
It uses the existing ten-second signal budget to join real commands before and after rebind.
Owned observation tasks are joined on success and after failure cleanup.
Independent review reports no P0–P2 finding in that fixture correction.

Native `20261109T164758Z-23220` compiles and runs the same 41 checks; three issues remain.
Completed suspension passes. Already-active Core resume still produces two real command errors and sets operationFailed.
The fixture retains every error assertion and deadline. No failing result counts as passing.

Independent diagnosis finds that Runtime rebind discards verification and media state during already-active resume.
Core then publishes Available before the existing account read restores proof.
A post-resume replay gate alone cannot close this interval.
Local reviews must resume without waiting for network operations.
A spec-writer is reviewing exact live-binding preservation and the required fault tests.
This WIP commit preserves the feature and regression; it accepts no unit or gate.
