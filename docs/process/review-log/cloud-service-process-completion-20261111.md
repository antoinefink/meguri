# Service test-helper completion latch

2026-11-11. Role: fixer. Starting source commit: `39a18efb` (supplied by
the integrator); filtered checkout baseline: `0040551ead2f2c0b25557290deb7d347245616da`.
Spec: R109, R1953.

The supplied observation for `swift-test/20261111T171404Z-28634` records an
unfinished pagination case, stdout EOF, no remaining child and a busy queue in
Foundation `waitUntilExit`. Its exact MG owner was cancelled and the cancelled
receipt retained. This is neither a passing suite nor proof of a production sync
bug.

Replace only the helper's cross-thread `waitUntilExit` with a semaphore signalled
by the original Process termination handler, installed before `run`. The worker
and callback retain the semaphore; the callback captures neither the worker nor
the Process. A callback before `finish` leaves a signal for its later wait.
Launch errors still throw the fixed launch failure without entering `finish` or
waiting for a callback. The serialized, cached finish result consumes the signal
once, before reading the original termination status. EOF handling, bounded
stdout/stderr draining, diagnostics and close/cancel semantics are unchanged.

The existing pagination, malformed-protocol and EOF/cancellation tests remain
unchanged, including all status, join, diagnostic and idempotence assertions.
No sleeps, timing probes, fixtures, production edits or new limits were added.

Verification is one source consistency check only; no native products or tests
are built or run in this filtered checkout. Integration owns compilation,
independent review, the unchanged focused suite, provenance/privacy/similarity
checks and acceptance. No passing-suite or production-sync claim is made.
