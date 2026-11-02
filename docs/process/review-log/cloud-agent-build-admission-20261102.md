# Signed-agent build admission

Status: component verified; signed execution remains open.
Integration start: `1b8bc3d3d75847e09706b857d063ae61636d5fec`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-agent#J2.1`, Q7 and Q13.1.

Prepared host tooling was resumed through filtered exports, ending with
`cleanroom/20261102T051350Z-14197`. The producer freezes complete build inputs
and exact receipt references after signing. Independent checks inspect product
identity, configuration, signatures and grants. Binary reads have a separate
aggregate budget from JSON reads. Ad-hoc and initially dirty builds compile
without signed input proof. The new signed job occupies its required T3 slot.

Independent reviewer: `native_bounds_review`.
Reviews corrected device scope and binary read limits, then repaired dirty-build
handling and two Xcode path interpretations. Round 3 found a P1: normal Xcode test libraries were
rejected, and positive fixtures had removed their loader values.
The unchanged Mac and simulator captures now drive the tests. Their embedded
library is bound to the frozen inventory and deep signature checks. Only the
exact captured SDK and system library references are admitted; additions and
escapes remain refused. Round 4 has no required findings.

The test-only filtered export `cleanroom/20261102T050620Z-11867` reproduced
the P1 in `selftest/20261102T051231Z-13520`: 12 of 13 wrappers passed and the
36 nested cases stopped at the expected loader refusal. After the narrow
production repair, `selftest/20261102T051615Z-14990` passed all 13 wrappers,
including those nested cases, with no skips.
`private/20261102T051611Z-14761` passed for all eight source files.
`similarity/20261102T051632Z-15544` scanned both Python files and passed.
`static-checks/20261102T051630Z-15273` passed all three checks.

The captured layouts came from actual CloudAgent builds for Mac and iOS
simulator on SDK 27.2. Generated product trees and command doubles are unit
evidence only. No fixture changes the original checker policy.
VM routing, delivery, installation and runtime proof remain unimplemented.
The signed job fails explicitly while those parts are missing and emits no
passing signed scenario cells. No VM action, Unit trailer, full gate or signed
acceptance is claimed. The original batch boundary remains open.
