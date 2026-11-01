# Shared app cloud composition

U113/U114 continuation. Start: `58639e9d0880a57f1335598bba545b2e048a8b74`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Neither unit is accepted by this component.

## Change

The native entry point and initial-adoption tests now use one internal backend
factory. It wires the workflow, policy, connection, media, asset reads, account
reader and executors together. Native construction still supplies one private
CloudKit database and shared staging. Tests replace transport dependencies.

The test backend now delegates lifecycle and policy work to that production
backend. It records existing diagnostics without repeating implementation.
All existing test assertions, cancellation checks and time limits remain.
The fixture's media transport explicitly fails unsupported requests.

This closes a composition gap. It does not prove outgoing review/media exchange
or live CloudKit delivery: the existing fixture still refuses sends, publication
and asset reads. A mutable Service transport and a second receiving Host remain
required. The existing tests must not be reported as that missing evidence.

## Review and verification

- Filtered implementation: `cleanroom/20261101T031524Z-24222`, session
  `s-20261025T134357Z-235af9a2`.
- Independent correctness/plan review: no P0–P2 findings. No spec changes were
  needed. No fixes or UI renders were required for this composition refactor.
- `swift-test/20261101T031801Z-25191`: 58 tests in two suites pass, including real
  adoption/review, cancellation, account and lifecycle checks. MG also built the
  package without warnings and generated the real publication fixture.
- `private/20261101T031829Z-26592`: pass on both files.
- `similarity/20261101T031828Z-26342`: pass on both files.

The original integration/milestone gates and signed account/schema requirements
remain open. No Unit trailer is added.
