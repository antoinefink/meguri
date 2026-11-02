# CloudKit generation codec

Status: U113 component WIP; no unit, P24 item or gate acceptance.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Component start: `37510a81e6c5bc87168393ae29d79c4734285d49`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Reviewer: `native_bounds_review`.

Contract: `sync/cloudkit-records#K5.3`, `#K7`, `#K19.2` field tables and `#K20`.
This component encodes Generation, MemberList, Member, LayoutMap and TypeChange.
It adds no Joining, claims, map composition, structural writes or incoming
application. The P24 tests for those semantics remain required and open.

The existing entity/control codecs supply the wire primitives and future-format
convention. Literal vectors cover the three states, reason codes, nested fields,
packed signed maps, explicit optional values, malformed input and size bounds.
The member asset ceiling comes from K7 A2's CKAsset limit; it creates no new
generation attention or claim policy.

## Review and checks

Initial filtered export: `cleanroom/20261102T004915Z-27723`.
Round 1 found one P2: materializing a large MemberList and re-encoding it for
comparison could exceed the device memory budget. The required repair keeps
the asset ceiling and uses a bounded decode, with an actual large-input
allocation regression. The metadata-only limit tests do not prove that path.

Initial checks, before the allocation repair:

- `core-test/20261102T005621Z-28716`: 39 passed, zero skipped; new generation
  vectors and existing entity/control codec tests.
- `core-format/20261102T005700Z-29268`: pass.
- `core-lint/20261102T005725Z-29552`: pass; 17-crate layering check.
- `private/20261102T005622Z-28943`: pass, four files.
- `similarity/20261102T005735Z-29807`: pass.

Repair: `cleanroom/20261102T005916Z-30328`. Asset decoding returns a borrowed
MemberListView and borrowed members. Counting writers validate canonical
lengths without a second full asset buffer. No member collection or individual
name strings are allocated by decoding or iteration.

The regression constructs an actual 104,857,600-byte canonical asset containing
3,382,503 members. A thread-local allocation counter measures decode and full
iteration after fixture construction. Both stay within 64 KiB of requested
allocations. A positive control verifies the counter. This synchronous-path
measurement covers temporary allocations; it does not measure process RSS.
Round 2 independent review found no P0–P2 defects and accepted that witness.

`core-test/20261102T010426Z-31297`: 40 passed, zero skipped, including the
accepted full-size asset and existing entity/control codecs. Source privacy
`private/20261102T010427Z-31522` flagged two newly written test lines; a filtered
text-only cleanup deleted five comment lines in
`cleanroom/20261102T010543Z-31950`. Its independent review found no behavior or
assertion changes. Scanner rules and exceptions were not changed.

Final focused checks:

- `core-test/20261102T010715Z-33209`: 40 passed, zero skipped.
- `core-format/20261102T010701Z-32648`: pass.
- `private/20261102T010702Z-32867`: pass, five files including this log.
- `similarity/20261102T010723Z-33467`: pass.
- `core-lint/20261102T010743Z-33809`: pass; 17-crate layering check.

These checks do not accept U113 or establish cross-device structural sync. The
original integration gate remains required. No VM or mobile check ran during
the resize.
