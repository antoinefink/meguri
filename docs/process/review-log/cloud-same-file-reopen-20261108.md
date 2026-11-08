# Managed same-file review journal reopen proof

Fixer session `s-20261103T133750Z-f276f58e`; spec authority `9d2d34cd`,
`sched/review-session` C12.1/C12.5/C12.7/C12.8. Original filtered proof start
`2aa50b99bbffa0fd3f8ed22ddeb9fd0923c86adc`; implementation round start
`e27aee27d5d921d3dfa8b6ea0e51e3b923f393d0`; correction round starts from
filtered master WIP `67474fe9`.
Original unit starts remain U113 `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`,
U114 `218ab2e2543a2c4822b79c1ac3520230ca3c1998` (continuation `babb20ab`);
original batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9` is unchanged.
U113/U114/U151 acceptance remains pending.

## Supplied root evidence and R1 findings

The protected same-file proof obtained semantic Darwin RED in
`native-durability/20261108T220652Z-49920`: 192 compiled checks, 191 existing
PASS, only the new Darwin Ready assertion FAIL with
`pending_answer_identity_mismatch`; workers closed before assertion.
`core-test/20261108T220603Z-49164` passed the exact non-Darwin negative.
Independent test review cleared the generated real-session setup.

The pre-R2 candidate `cleanroom/20261108T221018Z-51760` passed 92 selected
Linux engine library/recovery checks (`core-test/20261108T222311Z-56011`).
R1 found P2: a retired journal whose path/native identity still matches must
recover when `Header.notice.reason` is `PristineV2Rebound` and its archive
validates. A moved retired journal must fail. Two older negative setups also
needed authorized changes to fit the same-file contract.
Lint receipt `core-lint/20261108T222714Z-57726` failed on eight
`initialize_retained` arguments (limit seven). Root recorded a staged privacy
pass before this round. The privacy guard stays unchanged, without exemptions.

## Focused correction

- `Retired` recovery requires equal path/native bindings and a valid required
  archive. Invalid archives preserve the source. Reset changes incarnation,
  sets counter 1, clears `Header.notice` and skips retired events. The held
  descriptor remains guarded through the final barrier. Moved paths fail;
  raw handling stays unchanged.
- `journal.rs` groups the current collection, native tuple and canonical path in
  private `CurrentBinding`; `initialize_retained` has six arguments. Its retained
  source/decoded-state tuple stays explicit. No lint suppression or public flag.
- Only two older managed negative fixture setups change, as authorized:
  `c12_managed_ineligible_mismatch_keeps_journal_and_refuses_ready` gives only
  high-water, torn-tail and structurally valid Intent cases a genuinely unequal
  stored inode; all other cases and every existing assertion remain.
  `c12_managed_rebind_accepts_native_change_once_then_rejects_replacement`
  (formerly `...rejects_relocation`) copies the closed main file to a new inode
  instead of renaming it. All prior refusal/journal/archive byte assertions
  remain; an added assertion establishes the actual inode inequality.
- `c12_managed_retired_same_path_recovers_moved_path_refuses`:
  production migration; `Header.notice.reason = pristine_v2_rebound`; exact
  archive; one accepted answer. The callback saves actual `Retired` bytes and
  returns an error; cleanup reaches Ready. After close, those bytes are restored.
  This is a closed-state fixture, not a kill test. Saved path/native/incarnation/
  notice equal the migration header. Missing archive refuses open without writes.
  Restored archive allows same-path reset: new incarnation survives reopen,
  `Header.notice` is absent, cards/revlog are unchanged. Moving preserves native
  identity but refuses open; journal/database/archive bytes remain exact.

Ordering remains: qualify the retained source against privileged current
Opening/generation/locked-descriptor identity; decode its bytes using its stored
locator; verify required archive; guard mutation/publication against the current
binding; initialize that retained source or the exact staged reset descriptor;
retain evidence through replay, compaction and final Migrate; release before
Ready. Historical paths never supply live filesystem checks. Append checks
remain file-only and do not call writer `ReviewIdentity`. No second main database
descriptor, catch-all fallback, fresh journal selection after qualification,
path refresh, tooling, UI, timeout or reservation change.

## Source preservation and remaining verification

Direct comparison against this round's allowed-source copies confirms:

- Protected same-file proof is byte-identical, SHA-256
  `cef3b311fb1ab236bb0892be82f29b56d0591a0a91dafd11390362318ffeaef1`.
- Whole original raw `journal.rs` test module is byte-identical, SHA-256
  `4d7f44f09603792b30a726264da23a8d86af9c7b6e3c6edb69773e26427395ff`.
- All preexisting `session_recovery` source matches after removing the single
  new test insertion. Managed tests have only the two precise fixture deltas
  above; their old module is not claimed byte-identical.

One input/output consistency comparison; R2 reviews: two, no P0–P2 findings.
Darwin RED unresolved; post-R2 build/native evidence pending.
Pending: formatting, lint, compilation, Retired/negative cases, native same-file
and crash/raw checks, independent review, provenance/privacy/similarity, phone
and batch/milestone gates. No acceptance.
Author scope: source edits only; no builds/tests/scans/network/private reads/main
writes. Privacy guard, protected proof, raw tests and older tests are preserved.
Final-enqueue RED test/reservation and prepared native MG lane remain intact.

## Corrected-source verification

R2 export `58924`, then comment/name-only export `64179`, resolve the reviewed issue and fixture conflicts. Final review has no P0/P1 findings; both R2 reviews cleared P0–P2. The raw journal test module and protected same-file proof remain byte-identical. Only the authorized negative fixture setups change their native identity or use a real copy; no refusal assertions or limits are removed.

Formatting and staged privacy pass. Linux `core-test/20261108T224404Z-65789`: 108 passed, 19 excluded by the filter. Lint `20261108T224540Z-66571` passes. Similarity `20261108T224537Z-66278` passes all three files without exceptions. Native durability `20261108T224658Z-67153`: 225 passed, zero failed/ignored, including 27 recovery cases and 28 journal unit cases. The original native semantic RED now passes.

The corrected phone build, real review flow, media and two-device convergence remain pending. The independent final-enqueue regression remains RED at `53422` with its R2066 decision pending. No full batch gate or U113/U114/U151 acceptance is claimed.
