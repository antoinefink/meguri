# Original edit Service fixture — WIP

Start: `a4ea342e`. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261103T133750Z-f276f58e`.
Contract: conformance/cloudkit-agent J3.2; CloudKit K10/K12.

The fixture uses system clocks and joins existing work before installing holds.
An actual conditional re-save of the unchanged baseline creates a real Service
conflict. No unsupported quota command or fabricated response is used. Typed
failure observations retain the original request and response. The failure case
checks the original revision's non-ACK, retained durable flight history, returned
handler and joined active work. A failed save need not erase durable history.

Independent source review found no required finding.
`swift-test/20261110T061641Z-58395` failed. The fixed edit rejected collection
metadata; it compares an early clock sample with a later commit stamp. Both
negative cases also found automatic scheduling still enabled after publication.
These failures remain open. Cleanup reported additional media bridge failures.
No signed exchange, unit or gate acceptance is claimed.
