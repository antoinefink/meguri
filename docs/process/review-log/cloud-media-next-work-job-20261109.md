# Cloud media preparation job

Status: WIP, not accepted. U113/U114/U151 remain open.
Start: `317ae508`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261103T133750Z-f276f58e`.

Phone drive `97629` verifies continuity and records
`maintenanceQueued:CloudMedia.next_work` before and after its failed 30-second
review wait. No answer is sent. The call can perform collection-sized media
preparation synchronously. The existing job contract J2.1.1 covers moving this
work; final enqueue is a separate boundary and remains unchanged.

Filtered candidate `cleanroom/20261108T233305Z-2130` adds an original-owner
typed job on the existing pool, short start/result calls, explicit cancellation
and drain, and service ordering. Its author reports an unresolved SDK check
after journal contention and later feed-consumer installation.

Independent review finds P2 items: a cancelled service-order waiter can start
native work; SDK validation must remain current at the first effect without
reversing Runtime/media lock order; the held-scan review proof accepts a
refusal. Preserve exact cancellation, result, ordering and lifecycle behavior.
No repair is claimed complete.

Core `4422` fails compilation in the new terminal helper: stream timeout
returns Option, not Result. Its focused tests have not run. Format completes;
privacy passes. The SDK boundary remains under review. No phone build uses
this candidate, and no gate or unit acceptance is claimed.

Fix round, session `s-20261103T133750Z-f276f58e`, preserved base `9fc81a3f`:
thread the optional original SDK into the moved selector and validate it under
the first existing selector state guard after local feed capture, before early
returns, cached work or mutations, independently of PolicyWork's effect latch.
Restore consumer installation immediately after media open; synchronous FFI
next_work now shares the retained Host preparation helper. Local capture stays
SDK-independent under policy P6 / K11.1; no new publisher gate or lock order.
Add held-scan same-token SDK replacement coverage requiring the exact stale
error and no issued work, and enable FSRS through production setup for the
existing real-review proof. Preserve its Started assertion and all old tests.
Source consistency review only; compilation, tests, lint, provenance/privacy/
similarity and device verification remain the integrator's responsibility.

## Verification

Review applies P6 and K11.1 to local capture. The original SDK check remains
under the selector state guard. No new lock is added.

Core `7138` runs six checks: five pass; the held-review fixture is refused.
Export `8361` enables FSRS through production setup and preserves Started.
Core `11092` passes all 77 media-port checks, including the seven new job
checks and the real review start/end before scan release. Swift `8851` passes
seven registry tests and both bridge tests, with both listener-setup cases.
Lint `11816` and similarity `12085` pass. Independent Swift and Rust review
have no remaining P0–P2 findings in this repair.

Privacy `12414` flags only a new test comment. Export `13259` removes its two
lines; comparison against the verified patch confirms no code or assertion
change. Final privacy passes. The guard is unchanged.

Actual signed phone verification is pending. The separate final-enqueue RED
`53422`, required batch/milestone gates and U113/U114/U151 acceptance remain
open. These focused passes do not complete the sync workflow.

## Signed phone observations

On `3007ad0e`, normal preparation `14563` passes. Drive `17094` completes
all ten functional steps: a real deck starts a review and reveals its answer.
Drive `18766` submits one Good and returns to the overview. Normal restart
`21615` passes; drive `23380` confirms the same scheduling counts after
reopen. Counts change from 20/0/19 to 19/1/19, with one answer retained.
These are real collection actions, with no fixture or reset.

The drive receipts remain failed: generated replay controls lack identifiers;
one capture also reports overlapping ruby bounds. Neither failure is waived.
A broken image remains visible. Audio playback has not been proved.

Drive `30182` requests Sync Now once. Read-only follow-up `32673` shows a
completed sync and a generic sync error. Missing media changes from 2,398 to
2,397 and pending media from 2,923 to 2,922. This is progress, not complete
media delivery. The earlier unavailable-action text belongs to a disabled
details button; it does not prove that Sync Now was refused.

Filtered export `34824` adds only a DEBUG accessibility value on the existing
status row, using the existing content-free diagnostic recorder. Labels,
actions, policy and Release behavior stay unchanged. This permits reading
its last diagnostic without entering a replacement flow. It is not a live
error history or proof that its last value describes the current attempt.

Diagnostic review of export `34824` has no P0–P2 findings. Swift `35887`
passes all 19 diagnostic tests; privacy `37253` passes. The actual status
capture remains pending until the updated signed build is installed.
