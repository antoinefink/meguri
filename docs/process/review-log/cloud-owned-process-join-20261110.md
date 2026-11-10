# Owned setup process cleanup

Base: `5267339c`. Infrastructure repair; no unit completion or signed acceptance.
Antoine requested that this repair be completed directly. The prepared
process wrapper was retained and reviewed locally before integration.

## Failure and change

MG `20261110T194812Z-84309` reproduced a transport EOF followed by EPERM in
cleanup. The original child had exited but had not been reaped. That second
error hid the first one. Earlier probes had observed the child before closing
it and did not cover this case.

The managed original keeps its PID reserved through nonreaping exit observation
and a complete check that its group contains only that terminal original.
It then calls waitpid once and checks the actual result. Poll, wait and
communicate cannot report completion before those checks. Cleanup never signals
a reaped PID. Foreign members, incomplete observations, lost child ownership
and mismatched results fail closed. A cleanup failure remains a technical
failure; the original request error is retained as its cause.

The existing command, transport and shutdown budgets are unchanged. Enumeration
is limited to the original reserved group. This does not claim to find escaped
processes in other groups. No account values or SDK records are inspected by
the regression test.

## Review and checks

Local review corrected two issues in the prepared draft: final reaping must
use an explicit checked waitpid, and uncertain cleanup must not preserve a
setup requirement that could hide the cleanup failure. A failed group check
also prevents any later signal or completion claim.

MG `20261110T202705Z-92208`: seven focused tooling wrappers passed, including
the generated ownership models, transport lifecycle and owner setup tests.
MG `20261110T202843Z-93213` ran 32 related tooling wrappers: 31 passed, and
one retained an old launcher mock. That mock was moved to the managed factory.
MG `20261110T203052Z-95121` then passed all five affected wrappers, including
the added cancellation tests and selected-coordinator coverage.

Privacy scan `20261110T203048Z-94857` passed. The guard remained unchanged.
Similarity scan `20261110T202856Z-93600` passed before field renaming; the
final source scan `20261110T203120Z-95750` passed all eight files.

MG `20261110T202748Z-92554`: the same native VM EOF reproducer now passes.
There is no poll, wait or status observation between the original EOF and
actual owner close. Close succeeds and the original exit23 is retained.
No account lookup, SDK work, app launch or native build occurs in this probe.
The temporary MG probe route was removed afterward.

The final integrated patch has no new independent review.
Antoine requested direct work. Full integration and real signed setup
remain pending. Passing cleanup is not proof of a working CloudKit round trip.
