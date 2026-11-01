# Retained export admission

Status: admission repair focused-verified; shared integration gate pending.
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
Behavior test results are recorded below.

Privacy: `private/20261101T204331Z-92084` passed. Earlier receipt
`private/20261101T204257Z-91565` flagged three prose lines in this log;
those passages were rephrased without changing the guard or the specification.

## Implementation and checks

Filtered tests: `cleanroom/20261101T204506Z-94396`. Both provider-owned restart
cases failed at Busy on unchanged production in
`core-test/20261101T204855Z-95343` (two selected, two failed).

Filtered repair: `cleanroom/20261101T205103Z-97546`, fixer session
`s-20261025T162405Z-9dc656e4`. The host validates collection admission,
cancellation and output layout, then refuses an occupied non-safe slot before
new export work. The later ownership check, actual preflight, output protection
and cleanup remain intact. The reviewed test seed was preserved unchanged.

Independent review: `native_bounds_review`, one repair round, no P0–P2 findings.
The generation/admission check is read-only; admitted export still flushes
reviews before acquiring the snapshot reader.

`core-test/20261101T205345Z-99767` passed all 26 selected FFI export and host
export-safety tests; 204 tests were unselected. This includes both new cases,
the original provider/restart case, safe-slot and no-slot failures, source and
backup preservation, review capture and retained-output validation.

`core-format/20261101T205458Z-821`, `core-lint/20261101T205459Z-1102` and
`similarity/20261101T205457Z-426` passed. Source privacy:
`private/20261101T205457Z-203`, status pass, two files checked.
The historical gate's exact error is still unknown. Its failure and the
interrupted gate remain recorded; focused passes do not replace the full gate.
