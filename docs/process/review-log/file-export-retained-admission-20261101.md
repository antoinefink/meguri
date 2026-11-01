# Retained export admission

Status: specification clarification; implementation and verification pending.
No new unit or gate acceptance is recorded.

Starting source: `c6c912a1892683488e69984916c27106a6ae9296`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Spec-writer session: `s-20261101T204048Z-b8f3c3a4`.

The interrupted gate `gate/20261101T201216Z-18345` exposed the existing
provider-read/restart test's Busy assertion failure. Its actual error variant
was not printed. A diagnostic-only change retained the assertion and printed
only fixed variant names. Focused run `core-test/20261101T203722Z-88044` passed
one selected test without a production change. This does not repair or explain
the original failure. Both receipts remain retained; the gate is incomplete.

Admission: Busy for an occupied output slot with unproven cleanup ownership.
Lifecycle, cancellation and path validation precede admission. The early check
performs no mutation. Safe-slot deletion requires capacity preflight and output
protection. An admitted export flushes reviews before its snapshot.

Independent review: `native_bounds_review`, one round, no P0–P2 findings.
The next test slice must prove Busy precedence against capacity and invalid
source failures, preserve retained bytes and ownership, and retain safe-slot
and empty-slot controls. It must fail against unchanged production before the
implementation changes. Preserve the original provider-lifetime test.

Checks: independent review, privacy scan and `git diff --check`.
New behavior tests: pending.

Privacy: `private/20261101T204331Z-92084` passed. Earlier receipt
`private/20261101T204257Z-91565` flagged three prose lines in this log;
those passages were rephrased without changing the guard or the specification.
