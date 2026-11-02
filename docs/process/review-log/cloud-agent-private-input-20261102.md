# Protected expected-account input

Status: helper verified; runtime integration and U248 remain open.
Component start: `53b2f01d17fe41fd9881de81aebc0980c9efd75d`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-agent#J2.2`.

The helper validates the exact header and private input fields, protected
directory and file authority, retained bindings and opaque identity bytes.
It exports no private input or identity digest. It does not choose the current
account as expected. The caller must admit the platform Application Support
root and retain this check at the actual operation boundary.

Independent reviewer: `native_bounds_review`, no required findings.
Initial runtime checks found the shared generated fixture path issue recorded
in the protocol-support log. After that repair, one test still used obsolete
literal replacements from an older shared fixture. It now changes exact JSON
fields and verifies the changed input before testing each refused header.
All four refusal assertions and negative authority cases remain.

Final `swift-test/20261102T032234Z-79890` passed all 38 focused tests, no skips,
including this helper's 13 tests. Privacy `private/20261102T032247Z-81241`
and final-file similarity `similarity/20261102T032333Z-81566` passed.
No actual account, SDK request, VM operation or user setup was used.

Runtime lookup, database-start ordering, private delivery and signed execution
remain required. This helper supplies none of that evidence. No Unit trailer,
full gate or milestone acceptance is claimed.
