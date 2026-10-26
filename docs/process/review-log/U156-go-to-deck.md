# U156 — Go to Deck

Supplemental start: `1df3bcea29004e49565d0bd002c076aff180ddeb`.
Original U156 start: `3e8808ed0784467bbab89d86f37298e860a0b437`.
Intake: `build/unit-starts/U156-go-to-deck-20261026.json`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Search predicate

The scoped prepared-work audit found no Go to Deck implementation. The filtered
implementer session `s-20261025T134357Z-235af9a2` produced
`cleanroom/20261026T050752Z-87256` from `9394489c`. It adds the small immutable
R26 predicate and a Home test target. Terms match independently inside full-name
components; all terms are required. Inputs and snapshot order remain intact.
There is no mutation, cache, network or UI implementation in this slice.

Independent R1 (`/tmp/meguri-u156-filter-review-r1.json`) found no P0–P2 issue.
The privacy scan flagged one synthetic test example anonymously. The filtered
fixer session `s-20261025T162405Z-9dc656e4`, export
`cleanroom/20261026T051225Z-91931`, changes that single example while keeping its
accented case-insensitive assertion. The same independent reviewer inspected the
one-line repair; no behavior finding remains. The privacy guard is unchanged.

`swift-test/20261026T051130Z-89815` passed eight tests before the fixture rewrite.
The first non-escalated invocation stopped at the OrbStack socket permission;
No tests executed in that invocation.
Privacy failure receipt: `private/20261026T051202Z-91253`. `private/20261026T051433Z-95620` passed after the fixture rewrite.
`similarity/20261026T051204Z-91501` passed before the fixture rewrite.

D17 query integration, native sheet, keyboard ownership, actual-ID navigation,
create-from-query, rendered platform checks and the full unit/batch gate remain
open. This commit carries no Unit trailer or acceptance claim.

Final predicate verification: `swift-test/20261026T051509Z-96404` passed eight
tests, no skips or warnings. `similarity/20261026T051435Z-95903` passed for the
integrated query and final predicate. No production predicate change followed.
