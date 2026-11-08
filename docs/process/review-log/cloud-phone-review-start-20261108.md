# Phone review-start observation

Fixer session: `s-20261103T133750Z-f276f58e` (clean room).
Supplied phone baseline: `9dad9d02`.
Filtered source starting commit: `bada3a2aa7316d8e698874926c28f12729b11d25`.

The supplied facts report successful journal repair and ready collection/deck
rows in normal launch `10768` and preserve drive `14206`. Preserved phone drive
`15742` selected the real deck and Study, then failed wait step `6` after 30
seconds: Preparing review remained with zero counts, Close enabled and Undo
disabled. No answer/rating was sent; no reset or import occurred. Private phone
artifacts were not read. The underlying suspended boundary remains unknown.

This patch observes the existing `review.session.status` accessibility value in
DEBUG only. A read-only coordinator diagnostic derives fixed categories from
observable phase and session presence: loading without a session is `coreStart`,
loading with a session is `cardLoad`; other phases map to `question`, `answer`,
`transition`, `waiting`, `finished`, `ended`, and `error`. The status element
exists only for its existing loading, waiting, finished, error and ended layouts.
No identifiers, counts, errors, paths, content, settings or raw messages enter
the diagnostic. No visual text, controls, styling or Release behavior changes.

`ReviewCoordinatorTests.startDiagnostic(cardLoad:)` holds the existing client
start and initial surface-load seams in two cases, checks their distinct fixed
categories, then releases each to settle normally without answering. Existing
tests, assertions and timing limits are preserved. Scope: `sched/review-session`
C1/C9; R1962 device/window evidence and R1987 iteration/gates remain pending.

All verification for this patch is pending with root: compilation, focused tests,
independent review, privacy/provenance/similarity and native accessibility/device
observation. Supplied earlier checks (104 Rust focused checks `8917`, lint `9372`,
similarity `9654`) are baseline facts only. This is diagnostics only, not a
production repair or a passing review/startup test. No builds, tests, scans,
network access or main-checkout writes were performed by this fixer.

## Integration

Independent source review: no P0–P2 findings. Privacy passes. `swift-test/20261108T213918Z-25729` passes the focused test with both held-start and held-load cases. Similarity `20261108T213936Z-26896` passes. These checks verify the diagnostic only; actual phone startup and the underlying repair remain pending.
