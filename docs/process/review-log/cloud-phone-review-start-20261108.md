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

## Bounded identity observation (verification pending)

Same fixer session/provenance; this pass starts at
`d360e8e9709f3b5a951a01068121888c749e773a`.
Supplied actual `e788c867` normal-phone run `28864` succeeded, but preserved drive
`30623` failed step 2 on collection reopen, before review start; the review-start
diagnostic was not reached. The existing retry diagnostic was
`phase:coreResult;bridge:invalidArgument;lifecycle:failed.openStep;step:review-session;reason:pending_answer_identity_mismatch`.
Baseline `9dad9d02` opened and showed actual decks in `14206`; no answer was sent
during the review spinner in `15742`. No answer or reset was performed in this
pass. Successful opening cannot be assumed to persist. Path versus native
mismatch remains unknown: installation bundle URLs do not identify the journal
or current collection and cannot prove path-only relocation.

This static DEBUG observation adds only fixed `_path`, `_native`, `_path_native`
classifications at the existing managed-open migration-header mismatch refusal,
and exact whole-message Swift allowlist coverage using synthetic constants.
Release retains the generic refusal; other failure sites remain generic. No
identity values are exposed, no repair or relocation is implemented, and native
equality and all proof/archive/refusal ordering remain required under
`sched/review-session` C12.5/C12.8. The entire preexisting `journal.rs` test module,
including the protected RED proof, is unchanged. Bounded comparison observation,
builds, tests, independent review and provenance/privacy/similarity verification
remain pending with root. No phone artifacts, research, network, builds, tests,
scans or main-checkout writes were used for this pass.

Independent review clears the fixed comparison categories and unchanged refusal. `core-test/20261108T215127Z-35874` passes 68 engine tests. Privacy `20261108T215123Z-35619` passes. Similarity `20261108T215306Z-37866` passes three files. Swift run `20261108T215302Z-36812` matched no tests and fails; corrected selection `20261108T215449Z-39740` passes both diagnostic and presentation tests. Actual phone comparisons remain unknown until the next signed run.

## Actual review start, 2026-11-09

Signed phone run `69464` on `ecf13bbb` passes normal preparation. Preserve drive `71666` opens the collection, selects a deck and activates Study. Step 6 fails after 30 seconds; the status remains `coreStart` before and after the wait. No card loads or answer is sent. This is not a successful review or sync proof.

Filtered export `cleanroom/20261108T230226Z-74304` adds only DEBUG per-start fixed stage observations through the existing status value. It changes no Release behavior, scheduling, cancellation, time limit, visible layout or settings. Independent review clears production behavior and finds a P2 test-order race. Focused Swift `76481` compiles and fails with lifecycle busy: the new test starts two real review jobs before awaiting the first. Privacy `77965` and similarity `77508` pass. The test correction is pending; no unit is accepted.

Export `78424` moves the existing first-job completion wait before the second start. All assertions remain. Independent review has no P0–P2 findings. Swift `81433` passes the real bridge test; Swift `78777` passes both model tests, including both loading cases. Privacy `81152` passes. The diagnostic is ready for the phone; the underlying wait and signed review/sync proof remain open.

Signed preparation `82623` on `e537e037` passes. Preserve drive `84319` verifies continuity and fails review wait step 6 after 30 seconds. The status is `maintenanceQueued` before and after the wait: the start closure has not entered MaintenanceLane. No answer is sent. This does not identify the holding call.

Filtered owner exports `86059` and `87100` cover the existing maintenance call wrapper and the direct cloud queue wrapper, inside their executing bodies. Only compiled BridgeCall names enter the DEBUG status. Independent review clears production behavior and finds a test-only idle-check race. Swift `87948` fails compilation on the new test's actor isolation; this is retained. Similarity `88819`, privacy `89344`, and model Swift `90121` pass. The test correction and actual phone owner observation remain pending. No production cause or repair is claimed.
