# Media conflict naming and acknowledged reuse

Status: focused repair verified; U113 and batch acceptance remain pending.
Component start: `a05b642648bf10190931b1ad6ea47391c4d57e16`.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `sync/cloudkit-records#K15.9`, `sync/policy#P24`.

The real Service test exposed an extra local revision when a conflict reused
an existing acknowledged destination. Diagnostic run
`core-test/20261102T014915Z-48019` identified the failing assertion.
The repair preserves the ACK payload when its case key, SHA-256, SHA-1 and
byte count match. Physical spelling and observed file facts
remain separate. Normal feed, scan and reopen preserve that state; a later
content edit still needs a real Save and acknowledgement.

Review rounds:

1. The initial repair passed the original regression. Review found that a
   SHA-256-only comparison could mark different canonical images clean.
2. Exact spelling was inconsistent with K15.9; see `cfd1f33d`.
   The controls now hold a real downloaded reply before changing the file.
3. The repair uses the stored ACK. It does not add a Save.
   The ordinary feed records the actual physical facts.
4. Independent reviewer `native_bounds_review` found no required fixes.
   A revision gap alone does not prove pending ordinary work: existing image
   convergence permits it, and retained-flight ACK handling preserves edits.
   A rename observed before staging remains additional characterization work;
   the test here covers a real rename after staging, feed, scan and reopen.

Focused run `core-test/20261102T025600Z-62750` passed 37 tests with no skips.
Formatting `core-format/20261102T025650Z-63331`, lint
`core-lint/20261102T025727Z-63961` and similarity
`similarity/20261102T025657Z-63605` passed. A final prose and generated-byte
cleanup changed no assertions or control flow; privacy
`private/20261102T030330Z-68528` passed. Final focused run
`core-test/20261102T030338Z-68754` passed all 37 tests, with no skips.

The P24 item remains open for its remaining acceptance and fault evidence.
This component claims no Unit trailer, full gate, signed platform run or
milestone acceptance. The next required full gate retains the batch base above.
