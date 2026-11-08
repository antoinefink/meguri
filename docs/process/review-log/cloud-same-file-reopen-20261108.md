# Managed same-file review journal reopen proof

Fixer session: `s-20261103T133750Z-f276f58e`. Component/spec base: `9d2d34cd`.
Filtered source start: `2aa50b99bbffa0fd3f8ed22ddeb9fd0923c86adc`.
Original unit starts remain unchanged: U113
`1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`; U114
`218ab2e2543a2c4822b79c1ac3520230ca3c1998` (continuation `babb20ab`);
original batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U151 and related U113/U114 acceptance remain pending.

Reviewed `sched/review-session` C12.1/C12.5/C12.7/C12.8 permits managed Darwin
version-2 same-file path-only ordinary recovery after the existing unused,
no-notice fresh-migration qualifier. Stored locator, incarnation, notice and
high-water remain; no path refresh, fresh notice or archive is authorized.
Raw strict, non-Darwin, legacy, native-change and corruption refusals remain.

Supplied actual fixed-phone diagnostic `44292` establishes `pathdiffers=true`,
`native_differs=false`, with privileged/live proof passed. Earlier `9dad9d02`
opened existing decks; the next normal update failed before Review. No rating,
reset or import occurred. These supplied facts are not execution evidence for
this test; no actual data, settings, identifiers or phone artifacts were used.

One test-only regression in `session_recovery` uses generated collections and
real Engine/ReviewService sessions. Both no-notice and production pristine-
migration notice fixtures accept one answer, settle/end/close, retain high-water
above one, then rename the whole containing directory twice under an outer
TempDir owner. Reopens use only the current path. Actual device/inode retention,
exact card/revlog rows, original header and archive bytes are checked. No second
database descriptor is opened while the writer is live. Open outcomes are saved
and Engine/ReviewService workers close/join before intentional Ready assertions.

Exact selector:
`c12_managed_same_file_path_reopen_preserves_darwin_history_and_refuses_non_darwin`
in the existing `meguri-engine` integration binary `session_recovery` (test-hooks
as in native-durability). Authored, unrun. Expected: semantic Darwin RED at
managed Ready; non-Darwin negative PASS. No production/API/tooling change.
Existing tests/assertions/timeouts and the entire protected `journal.rs` test
module are untouched; root runs the existing raw-strict negatives separately.

Only source consistency review was performed. No builds, tests, formatters,
scans, network or main-checkout writes. Root must compile/run RED and review
independently before authorizing production repair. Native/phone/integration
acceptance, provenance/privacy/similarity and all required gates remain pending.

## Test-first results

Independent review clears the generated real-session setup, retained history and archive checks, platform distinction and worker teardown. Coverage at this stage is terminal/compacted history and notice preservation; unresolved chains and crash coverage remain for implementation.

Formatting `core-format/20261108T220505Z-48611` and privacy `20261108T220608Z-49470` pass. Docker `core-test/20261108T220603Z-49164` compiles and passes the exact non-Darwin negative case; 21 other tests are excluded by selection. Native durability `20261108T220652Z-49920` compiles and runs 192 checks: 191 existing checks pass, and only the new same-file test fails at the intended Darwin Ready assertion with `pending_answer_identity_mismatch`. Workers close before that assertion. This is semantic RED, not acceptance. Production repair and all subsequent verification remain pending.
