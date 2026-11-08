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

## Authorized implementation round (checks pending)

Author role/session: fixer `s-20261103T133750Z-f276f58e`; this filtered round
starts at `e27aee27d5d921d3dfa8b6ea0e51e3b923f393d0`. The component/spec base,
original unit starts and original batch base above remain unchanged. Root's
supplied RED receipt is `native-durability/20261108T220652Z-49920`: compilation
and 192 checks, 191 existing PASS, new Darwin Ready proof alone FAIL with
`pending_answer_identity_mismatch` (notice=false), workers closed first.
Docker `core-test/20261108T220603Z-49164` passed the exact non-Darwin negative;
independent test review cleared the setup. These are supplied root receipts,
not author executions or acceptance of this repair.

Author changes:

- `journal/managed.rs`: retain the exact pristine-v2 classifier first. Only
  privileged current Opening/generation/locked-descriptor proof plus exact
  valid Darwin UUID/inode equality permits a different stored absolute path.
  Decode the same retained bytes with the full ordinary codec and refuse a
  retired source. Verify required `pristine_v2_rebound` archives before normal
  initialization, including existing exact hash, pristine bytes, distinct
  incarnation and distinct binding checks; none of those checks is optional.
  The optional private archive pair accommodates ordinary sources without a
  required pristine archive; legacy-notice rules remain as before.
- `journal.rs`: factor post-decode retained-source initialization. Managed
  qualification passes that very descriptor/decoded state/frame number/complete
  prefix into initialization, never calls strict recovery to choose another
  source. Later reads verify the landed inode and exact retained prefix. Raw
  open's signature and strict path decoder stay unchanged; raw retired/legacy
  processing still precedes initialization. Retain full sync, append descriptor
  equality, normal torn-tail truncation and recovered high-water. Install the
  existing `OpeningEvidence` work before returning a journal to replay.
- Evidence keeps the current directory/source/archive bindings through replay,
  ordinary compaction and the final Migrate barrier, with the existing release
  before Ready. Append checks remain file/path-only; no synchronous writer
  identity callback occurs while the writer may await a replay receipt. Actual
  fresh replacement still retains the unlinked original descriptor and bytes.
- No path rewrite, new incarnation/notice/archive/migration reason/event, FFI,
  worker, pool, timer, retry, Ready/resume permission or UI change. A short
  journal `AGENTS.md` note records the current/historical binding and callback
  trap.

Source-to-live binding map:

| Operation | Authority/source | Live guard |
|---|---|---|
| qualify | retained first header; exact valid Darwin identity from current original locked descriptor; exact pristine classifier first | current privileged Opening, generation, writer `ReviewIdentity`, current canonical path and native tuple |
| decode | whole retained bytes, ordinary codec, stored locator only as expected codec field | no historical-path filesystem access; current directory and retained nofollow/single-link source binding |
| initialize/torn tail | same retained descriptor and decoded complete prefix; later reads only verify landed inode/prefix | current canonical path/native tuple plus full Opening/source/archive proof before truncation/publication |
| replay append | existing ordered worker, same append descriptor; evidence grows with actual appended bytes | current collection path/native tuple, directory, active/archive descriptors and exact bytes; no `ReviewIdentity` callback |
| compaction | ordinary recovered state/high-water; stored locator/incarnation/notice preserved | full current Opening proof before publication, retained files, landed append-descriptor equality |
| final barrier | existing Migrate work and original-descriptor identity re-read | full current proof; release evidence before Ready unchanged |

Added focused selectors, all in existing `meguri-engine` `session_recovery`
integration binary; no tooling change. Every case is authored and UNRUN:

- `c12_managed_darwin_same_file_crash_recovery_replays_once` (Darwin only):
  real captured Intent, Prepared, exact committed Prepared witness, and Prepared
  plus torn final frame; two directory renames/reopens, exact full collection
  post-images, retained locator/native/incarnation/notice, recovered high-water,
  no duplicate answer, no path migration report/archive and no tail ACK. Covers
  C12.8 named `c12_managed_darwin_same_file_crash_recovery_replays_once` and the
  unresolved/committed portion of
  `c12_managed_darwin_same_file_reopen_preserves_full_history_and_notice`.
- `c12_managed_same_file_native_change_copy_and_invalid_journals_refuse`:
  real Prepared history at a moved path; unequal UUID (device on non-Darwin),
  unequal inode, actual collection copy, unsupported version, v1, foreign
  platform, retired, complete corruption, invalid predecessor, invalid notice,
  hardlink and symlink. Ready/admission refuse and journal/database bytes stay.
  Covers named `c12_managed_darwin_native_change_copy_and_invalid_journals_refuse`.
  Linux supplies only native/wrong-platform negatives, never positive fake
  Darwin authority.
- `c12_managed_same_file_required_archive_failures_preserve_evidence`:
  real production pristine migration/notice followed by an answer and another
  directory rename. Missing archive, wrong hash, exact-hash archive with reused
  incarnation or identical stored binding, archive hardlink and symlink all
  refuse without changing journal/database/archive evidence. Covers the
  required-archive portion of that same C12.8 negative witness.
- `c12_managed_darwin_live_binding_mutation_refuses_before_append_and_compaction`
  (Darwin only): existing `storage.commit.before` and
  `storage.atomic.after_write` callback seams exchange the current collection
  or journal pathname/inode after capture. Replacements are precreated while
  closed and exchanged by rename, without another locked-main descriptor.
  The seam must be reached, Ready/admission refuse, and neither an append nor
  compaction lands over the exchanged evidence. Covers named
  `c12_managed_darwin_live_binding_mutation_refuses`.

The protected
`c12_managed_same_file_path_reopen_preserves_darwin_history_and_refuses_non_darwin`
remains byte-for-byte, covering terminal/compacted state with and without a notice,
history, high-water and two whole-directory renames. Together it supplies the
notice and repeated-path portions of the first two named C12.8 witnesses.
The entire old `journal.rs` test module, all old managed test modules and all
preexisting `session_recovery` source/assertions/time limits remain unchanged.
Existing crash/fault tests stay enabled. These additions are bounded captures
and fault cases, not a claim of new coverage of every process-crash boundary or
of valid multi-event predecessor chains under a notice.

Two older managed tests reject cases now permitted by C12.5.
`c12_managed_ineligible_mismatch_keeps_journal_and_refuses_ready` uses native equality
with a changed path for high-water/torn-tail cases.
`c12_managed_rebind_accepts_native_change_once_then_rejects_relocation` moves the
same file while retaining its valid notice/archive. Preserve their refusal
assertions using genuinely different native files; keep the new positive proof.
No extra OS-private-root condition was introduced. The old Darwin suite is not
claimed to pass before this correction.

Performed one focused own-source consistency review and direct protected-byte
comparison against the round-start file copies (no history reads). No author builds, tests, formatters, scans, network, phone artifacts or
main-checkout writes. Native/Docker compilation, formatting, focused positive
and negative execution, existing crash/fault and raw-strict checks, independent
implementation/test review, provenance/privacy/similarity and batch/milestone
checks are root-owned and pending. U113/U114/U151 acceptance remains pending.

Protected-byte comparison result: all original `session_recovery` bytes match
after removing the single new insertion; its protected proof matches exactly
(SHA-256 `cef3b311fb1ab236bb0892be82f29b56d0591a0a91dafd11390362318ffeaef1`).
The entire original test suffixes match for `journal.rs`
(`4d7f44f09603792b30a726264da23a8d86af9c7b6e3c6edb69773e26427395ff`)
and `journal/managed.rs`
(`f81d20c66220f3bff22be3169e42a4d3955396c494c80dd6f84fe8f3e14b1126`).

## Implementation review round 1

Export `cleanroom/20261108T221018Z-51760` applies to master without reverting the separately restored final-enqueue regression. Formatting passes; `core-test/20261108T222311Z-56011` compiles and passes all 92 selected Linux engine library/recovery checks. This does not establish the Darwin positives.

P2 from both reviews: existing matching retired recovery must still work after a pristine migration. Historical-path retired sources remain refused. Add that failed-replacement case. Use different native identities for the high-water/torn/Intent negatives and a real collection copy for the final notice case. Preserve all refusal assertions and the positive proof. Repair and native/phone checks remain pending.

Lint `core-lint/20261108T222714Z-57726` reports one new failure: `initialize_retained` has eight arguments against the limit of seven. Correct the private input grouping without suppressing lint. Privacy passes after rewriting only new review prose. This WIP is not accepted.
