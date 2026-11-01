# Cloud re-key foundation

Component start: `0d519bd4562953cb6a69380ab46f68fb72793cde`.
Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113 and U114 remain unaccepted.

Current code parks collisions and own landed tombstones with live Undo rows.
K14.2 requires identity and reference writes, a persisted intent, a covered WAL
sequence and one tracking transaction. K19 generations and media-name conflict
paths remain unaccepted.

Tests-first export: `cleanroom/20261101T002821Z-54292`.

- Note: different GUID and origin, unacked local identity, one tracking-only
  reserved successor and one occupied row. The moved identity must use the
  first available gap; its Cards must reference that new identity.
- Card: different note reference and origin, with history from actual reviews.
  Re-key must move the local Card and its Revlog references.
- Tombstone: a production-built deletion lands through Service and its reply
  is lost. Fixture rows are recreated with the child stopped; native deletion
  and Undo then restore them. The old tombstone stays final and the live rows
  must move to a fresh pending identity.
- Controls cover same-GUID origin adoption and refused acked-origin mismatch.

`core-test/20261101T003614Z-55706` compiles and runs six tests: three controls
or harness cases pass; the three behavior tests fail at their missing re-key
outcomes. Their prerequisite assertions pass. This is expected-red evidence,
not unit acceptance.

Independent tests-first review finds no P0–P2 issues. The test packet does not
cover all six kinds, failure/status handling, held reviews, undo effects or the
crash matrix. Those remain required implementation and verification work.

Packet cleanup `cleanroom/20261101T003927Z-57315` changes one test name,
one diagnostic and a comment. Test inputs and assertion conditions are fixed.
The final packet passes `private/20261101T004339Z-58786` and
`similarity/20261101T004340Z-59012`; Rust formatting is applied.
The production re-key path and its remaining matrix are the next work.
