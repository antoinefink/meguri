# Epoch page diagnostics

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: 470a2205.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Physical observation 92804 sees a failed initial download after 158 replies
and 5,033 received records. Its exact typed result identifies fetch,
pageRecordValidation and primary validation, with no cleanup failure. It does
not establish the total server page count or the rejected record's type.

Both native and fake-service pages include the epoch control record. Core
already handles it before entity decoding. The old predicate checks payload
absence and exact equality of both encrypted data and opaque system fields.
Ordinary media work does not rewrite the epoch control record. Separate
server reads producing different archives remains an unproved explanation.

Export 99139 adds three finite stages: pageEpochShape, pageEpochData and
pageEpochSystemFields. Ordered component checks preserve the exact original
predicate, including missing expected data, error type and cleanup precedence.
No field comparison is dropped. Core, FFI and Swift map the same finite tags.
Independent review finds no P0–P2 issue.

Core 8248 passes four tests covering all three refusal stages, both cleanup
outcomes, immutable original results, followup refusal and unchanged-record
acceptance. Integrated Core 10195 and wider Core 11450 also pass. Source
privacy and Core-lint 13692 pass. Native 14656 passes all 46 selected tests,
including the new finite-stage mapping test. A new signed observation remains
pending. No live failure repair or unit acceptance is claimed.

## Signed observation

Signed run 18686 installs clean source f90fb368. Drive 20558 completes its
nine observation steps within the unchanged download limit, but the app still
reports failure. The received count is again 158 pages and 5,033 records.
The retained stage is now pageEpochSystemFields. Earlier ordered predicates
therefore passed: matching epoch record kind/name, no payload, retained
expected record, and equal encrypted data. Only the opaque archives differ.
The observation does not establish equal native server version tags.

Independent contract review confirms that a strict native version projection
supports K10.1, K18.4 and V14.2. Native receives the epoch from the same work
request. It may preserve that original archive only after
checking identity, type, current owner, asset absence, exact encrypted data
and equal nonempty server change tags. Missing or unequal tags must refuse.
Core's byte equality and cleanup remain required. No rule is amended and no
version-equality claim is made before the native check runs.
