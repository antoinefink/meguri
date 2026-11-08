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
