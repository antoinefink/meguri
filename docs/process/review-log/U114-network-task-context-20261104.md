# U114 native network task context

Status: WIP; no unit acceptance.
Unit start: 218ab2e2543a2c4822b79c1ac3520230ca3c1998.
Component start: 2a8678b5.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Signed build34298 retains local reviews and recognizes the iCloud account.
It crashes at the native SDK fetch call. Two earlier reports have the same
CloudKit stack offsets. The reports contain no assertion message.

Source review finds that network tasks can inherit task-local context from an
SDK delegate callback. The event fence does not clear that context when the
callback returns. Export64142 adds a deterministic test at the real admission
seam, with controls for media context, selected action and priority.

Swift66861: 8 pass, 1 fail. Only the context-isolation assertion fails, as
expected before repair. Similarity67900 passes. Private68149 flags one new
comment; it must be rephrased. Independent review also requires a finite bound
and joined cleanup for the new test helper.

The context test fails. A live signed run must still show whether the repair
also prevents the SDK crash.

Export69683 changes the retained network task to a detached task with caller
priority. It restores only the explicit media context and DEBUG action. The
original SDK, authority, event fence and cancellation/drain handle stay intact.
The new helper has one joined watchdog and tests for both missing arrivals.
Independent review is clear. Swift71438: 22 pass. Private72529 and similarity72537
pass. Profile73206: 9 pass, with DEBUG disabled. Docs private74669 passes.
The live signed check remains required. A further test-only change will move
SDK-entry observations to parent assertions for consistent test attribution.

Export79345 captures entry facts before any join, then asserts them in the
parent test. Independent review is clear. Swift81600: 10 pass. Production code
is unchanged. Private83051 and similarity83027 pass. Signed build77287 passes
its flag, signature, profile and hash
checks. It stays running through a manual sync attempt beyond the earlier
crash point. That run still stops: its first new failure is RecordQuietPoint
with UnqualifiedCompletion. Full sync and the batch gate remain incomplete.
