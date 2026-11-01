# Cloud identity reference writers

Component start: `dac106a2a7bf3b16ba7d2411dd1186a05a127d0a`.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113/U114 remain unaccepted.

Source packet: `cleanroom/20261101T021740Z-93477`.
It adds storage reference/stamp/journal/rollback/reopen cases for all six
integer-keyed kinds and pure same-object cases for K14.1. A forced raw Card
write replaces the typed round trip after a latest-Revlog identity move,
preserving unrelated scheduling cells and custom data.

Independent R1 finds no P0–P2 issues in the packet. Its review preceded the
actual execution result; it does not establish runtime or unit acceptance.

`core-test/20261101T022434Z-95205`: 33 selected tests, 32 pass and 1 fails;
96 tests are outside the filter. All seven pure same-object tests and all
fifteen existing runtime re-key cases pass, including the memory cap.
The storage Notetype case fails: every old field/template key remains and
no new field/template key exists. The other expected cells match.

The parent is moved before the old parent's parts pager runs. That pager's
approved contract returns an empty page for a missing parent. Repair must
preserve the pager contract and migrate every part in the same writer scope.
Add multi-page/gapped-ordinal and occupied destination-part coverage; preserve
all row, stamp, journal and rollback assertions. Failure diagnostics should
report bounded locations rather than dump complete row maps.

Storage and pure predicate tests do not prove cross-device origin adoption,
hold-back, structural generations, mixed recovery or media conflicts. Those
requirements and the original integration gates remain open.

Repair export: `cleanroom/20261101T022823Z-96048`.
A private writer-bound parts pager preserves the public missing-parent rule.
The added case covers 253 fields and 253 templates, ordinal gaps, opaque
configuration bytes, late cancellation, 1,025 journal entries and reopen.
`core-test/20261101T023311Z-97453`: all 35 selected tests pass; 96 are outside
the filter. The original Notetype failure is repaired.

R2 requires a further P1 fix: checking only ordinals present in the source
allows a destination orphan at another ordinal to become part of the moved
Notetype. Refuse any destination parts through a metadata-only check without
changing I5 allocation. Both overlapping and nonoverlapping orphan fixtures
must retain exact data and complete rollback/reopen checks.
Use one-part private pages to match the identity writer's residency policy;
this is not a claim about whole-runtime Notetype heap or native RSS.

Second repair: `cleanroom/20261101T023640Z-98503`.
Metadata-only destination checks now reject any Fields or Templates row before
the first graph write. Private reads keep one part body; public paging and I5
allocation are unchanged. Overlapping and nonoverlapping orphan cases cover
both tables at ordinals 1, 503 and 999, including immediate whole-row equality,
opaque bytes, rollback and reopen. Late cancellation still follows more than
250 moved parts, and all 1,025 complete journal targets remain checked.

`core-test/20261101T023857Z-99656`: all 35 selected tests pass; 96 are outside
the filter. R3 independent review has no unresolved required findings for
this component. No fields, templates, native data or foreign parts are
discarded to obtain these results; no limits or assertions were weakened.

The remaining runtime conflict paths, generations, media conflicts, native
cross-device checks and original batch/milestone gates remain required.

`private/20261101T024038Z-1088` and
`similarity/20261101T024039Z-1420` pass.
`core-lint/20261101T024034Z-373` reports one Clippy error: the test's
`Cell::text(&format!(...))` has an unnecessary borrow. Fix that expression;
retain the successful focused-test evidence and run lint again.

`cleanroom/20261101T024253Z-2178` removes only that unnecessary borrow.
Independent review confirms identical fixture values and assertions; the
35-case execution evidence remains applicable. No production behavior changes.
`core-lint/20261101T024410Z-3017` passes formatting, Clippy, layering and
the forbidden-host scan. No lint suppression was added.

Final source scans pass: `private/20261101T024605Z-3977` and
`similarity/20261101T024556Z-3622` (3 files, 0 flags, 0 allowlist matches).
No unit or integration-gate acceptance follows from these focused receipts.
