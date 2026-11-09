# Original snapshot inspection: pending integration

U248 start: `e6b93870`. Batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261109T210639Z-47d06d5b`.
Fixer: `s-20261103T133750Z-f276f58e`.
Exports: 92118, 95751, 98117, 361.
Contract: conformance/cloudkit-agent J2.3 and J3.2.

The input is a verified snapshot with an optional WAL. A private read-only
copy supplies the baseline note and deck tree. Live data is inspected through
the current owner. The result contains digests and counts, with no mutation
or recovery authority.

Initial review found P1 note/deck selection and P2 WAL reset handling. R1 is
clear after repair. The real RESTART case keeps the physical tail, checks the
effective snapshot and rejects checksum damage. Sources remain unchanged.

Focused evidence: core-test 5031 passes 21 cases across both inspectors;
core-lint 6379 passes four checks; Swift Core 2743 passes seven tests; Swift
Kit 5322 passes 64 tests in four suites; Mac CloudAgent build 7483 passes.
Build and package results are not signed execution evidence.

The prepared 99-line AgentGeneratedBaseTests addition is held at
`build/source-preserve/held-original-diagnostics-20261109/changes.patch`.
It was included in passing run 5322. Privacy 2055 flags its diagnostic region.
Every assertion and the exact source are preserved. No exemption, skipped
case or weaker predicate was introduced. That addition must land before
acceptance. Existing master tests remain enabled.

U248, the selected program, signed exchange and full gate remain unaccepted.
