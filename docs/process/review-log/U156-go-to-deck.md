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

## Native picker WIP

Native UI starts at `2deea6a46f36eed6e75731e2979d7499ff31da5e` and consumes the
committed D17 query. Filtered implementation `cleanroom/20261026T052147Z-6332`
adds the retained model, native searchable sheet, menu/toolbar actions and
actual-ID owner activation. It does not implement create-from-query.

Independent R1 found two P2 issues: sheet traits were used as layout authority,
and successful real owner continuations lacked proof. Filtered fixer
`cleanroom/20261026T054332Z-22433` carries RootView's live layout through the
existing presentation environment and adds three real-core cases to the existing
serialized owner suite. R2 is clear in source. The tests use the existing fake
rendering surface, while collection/session/selection calls are real.

`swift-test/20261026T055903Z-33665` failed Swift 6 closure conversion. Filtered
fix `cleanroom/20261026T060356Z-38612` adds exactly two MainActor/Sendable closure
annotations; independent R3 is clear. `swift-test/20261026T060620Z-40910` then
failed only two test-macro unwrap expressions. Filtered fix
`cleanroom/20261026T060949Z-44156` gives each unwrapped session ID an explicit
UInt64 local before assigning the optional capture. R4 is clear; assertions and
resume call counts remain intact.

`swift-test/20261026T061210Z-46064` compiles and executes 65 tests: 63 pass,
2 fail, no skips. The hidden-Default real-owner case passes. Both compact cases
stop at their eligibility assertion because they choose empty id 1 from the
fixture. Correcting that fixture selection and executing the actual session
handoff remain required. These failures are retained; no successful compact
execution is claimed.

Pre-application privacy checks cover all twelve prepared files and pass. Final
privacy/similarity, native app compilation, rendered Mac checks, keyboard/focus,
compact native layout proof and the original-base full integration gate remain
required. This is a WIP checkpoint with known test failures, explicitly permitted
by Antoine; no Unit trailer or completed-unit acceptance is claimed.
