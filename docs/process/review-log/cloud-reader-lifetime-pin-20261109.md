# Cloud reader lifetime pin

Starting commit: `ea99cf870dfc31c39b6da5b32cef695d6c31ac43`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Filtered export: `96973`; boundary probe passes. Session:
`s-20261103T133750Z-f276f58e`. Anki: none.

The engine-only primitive has no production caller. It transfers the same
Cloud reader, keeps thread ownership and returns it before lease/close wake.
The exact contract is lifecycle L3/L8/L11 at `ea99cf87`.

Rust formatting passes. Privacy `4829` passes all seven files.
Core `4555` fails compilation before tests: an integration test calls a private
error constructor. Independent review also finds a test moving a borrowed
Exclusive and its owning slot into a thread. Both require a test-only repair;
no check is counted as passing from this run. Source review is still running.
No unit acceptance or batch gate pass is claimed.

Independent production review clears P0-P2. Filtered test correction `5666`
uses the public error variant and acquires slot/Exclusive inside the worker.
The original privileged writer probe, real replacement and owned-snapshot
checks remain. A bounded handoff and failure cleanup release protection before
joining. Independent re-review clears P0-P2; all limits and assertions remain.
Compilation and focused execution still await this corrected source.
