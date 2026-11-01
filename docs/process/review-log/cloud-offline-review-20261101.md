# U113 offline review account-check prerequisite — 2026-11-01

- Role: implementer; session `s-20261025T134357Z-235af9a2`.
- Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
- Original integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Supplied current base: `967512b787ae72f7d29e2c78a796e19a5b572394`.
- Scope: Q11 and the offline review case in K22.2. The earlier spec audit records the separate missing-mark crash case.

## Changes

Two distinct tests in `core/crates/meguri-sync/tests/cloud_media_roundtrip.rs` share one helper:

- `read_failed_offline_review_and_note_edit_survive_clean_reopen_then_upload`
- `temporarily_unavailable_offline_review_and_note_edit_survive_clean_reopen_then_upload`

Both publish `published_review_pair(false)` through the real Service and adopt through the real receiver. They check completed scan/adoption and drained SDK pending lists, retain the actual baseline ACKs, then close normally before opening the same receiver directory with unavailable identity and no fresh key bytes. Real Begin/Start/Show/Good operations change an actual new Card and add a Revlog with Good ease and 2,500 ms timing. End uses the review service and application barrier. The existing driver API is named `edit_note_fields(id, &[&str])`; its inspected handler calls `meguri_engine::notes::update` with `UserUndoable`, preserving tags. The tests use that real API, retaining the photo reference in the edited Note.

Assertions cover physical collection payloads, exact media bytes, retained prior rows/history, immediate persisted answer entity facts and the Note edit's exact capture proof, observer marks and pending revisions for Note/Card/Revlog, unchanged baseline ACKs, and WAL coverage within the current engine lifetime. While identity is unavailable, bounded ordinary drives must leave Service zones and its request cursor unchanged, with no engine instance, send calls, SDK pending entries or engine/batch trace. A second normal close and unavailable reopen must preserve the complete physical image and durable entity facts before any verification; persisted scan state must remain undrained. Matching verification, ordinary reconcile and Sync Now/drain must then produce exactly one accepted Service Save per edited row, checked by existing `assert_review_saved`, including encrypted envelope, system fields and digest receipts. Receiver and peer physical images and exact media must converge and survive online reopens; the receiver native file and binding stay stable. No sequence comparisons cross engine lifetimes.

Testkit changes are only `IdentityReads::TemporarilyUnavailable` and its exact mapping to production `IdentityStatus::TemporarilyUnavailable`, plus the fixer's read-only `ActiveReviewSession` IPC projection of the real host review service. No production guard or behavior changes, pending-model emulation, tracking/ACK seeding, fault injection, retries or skips were added. Existing lost-Undo tests are untouched.

## Verification and handoff

Source-only consistency: **PASS**, checking the two named tests, existing API names, exact identity mapping, structural delimiters and scope. The checker's initial API lookup incorrectly searched only `child_driver.rs` for `media_files`; including the existing `ChildDriver` extension in `publication_rig.rs` corrected the checker and passed, with no source change for that lookup. Native compilation and actual tests are **unrun** in this filtered implementer session, as requested; Root owns focused verification and independent review, plus required provenance/privacy/similarity checks and the batch gate. No Git, history, network, build, forbidden-source access or commit was performed here.

The assertions intentionally retain the contract's expected boundaries: unavailable identity must still allow observer capture of the real answer and Note edit; a clean offline reopen must retain those marks and physical rows; no remote work may precede matching verification; ordinary reconciliation and accepted Service outcomes must preserve/upload those exact edits. If Root encounters a failure, retain its exact assertion/error and phase and report it without weakening this prerequisite or repairing production in this patch. No production failure or passing test result is claimed by this unrun handoff.

P24 is unchanged and remains pending actual verification and spec-writer closure. This log grants no approval, acceptance, unit/batch/milestone completion or signed/device evidence.

## First execution

`core-test/20261101T115439Z-28834` compiled and ran both cases; both failed.
One queried retained Undo history after End as if it were an active session.
The other asked the single latest-callback cache to match a reader snapshot seq.
Independent reviewer `native_conformance_lifecycle_audit` identified these as
invalid observations. Replace them with an actual active-session read and
immediate persisted Observer marks for the answer's Card and new Revlog.
Keep the Note operation's precise capture receipt and all offline persistence,
no-send, accepted Service and peer checks. Runtime coverage is still unproved.

## Observation repair — fixer handoff

- Role: fixer; session `s-20261025T162405Z-9dc656e4`.
- Original U113 start remains `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`; original batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. No Git/history lookup was performed.
- Owned changes: `cloud_media_roundtrip.rs`, `meguri-testkit/src/cloud/child_driver.rs` and this log only. No P24/spec or production changes.

After the real End and its existing application barrier, the offline helper now asserts `review_active_session() == None`. The new IPC input calls only `host.review().active_session()` and projects its actual optional SessionId; it executes no review/runtime work and keeps no mirrored state. The pre-End assertions against the actual held answer and its Card/Revlog names remain. Retained Undo history is no longer treated as evidence that a session is active.

The answer no longer asks the single last-callback cache to match `answered.seq`. Before End, Note editing, reconciliation or any scan, the helper reads actual persisted entity facts and requires the exact physical Card and new Revlog payloads, Observer dirty marks, matching payload digests, positive bounded mark counters, increased local revisions and empty in-flight lists. The prior Card ACK payload, digests, fields, envelope, revision and server revision must remain intact. The new Revlog must have server revision and ACK revision zero, no ACK payload, and empty ACK fields/envelope. Missing facts or marks fail immediately; they are not repaired or bypassed.

The operation link was verified in own source: `meguri-engine/src/undo/mod.rs` constructs `StepInfo.id` from `commit.seq.into()`, and `undo/entry.rs` converts Seq to EntryId without changing its value. The existing Good handler waits for the real application barrier and reads the actual held step's EntryId; this new session has one answer. Both entity observations must equal that answer's `op_id` and the current generation. The answer operation must be newer than the pre-answer snapshot and no newer than the post-answer snapshot; the post-answer reader sequence is not assumed to equal the answer's commit sequence.

The Note edit receipt, WAL checks, retained pending writes, second offline restart, remote-write boundary and later Service/peer checks all remain enabled. A missing answer mark or a mismatched row still fails.

Runtime status after repair: **UNRUN**. Root owns compilation, both focused offline tests, independent review and required provenance/privacy/similarity and batch gates. This handoff grants no acceptance or P24 closure. No build, Git, history, network or forbidden-source access was performed in this fixer session.

One source-only consistency check: **PASS**. Confirmed the driver diff is exactly the read-only active-session input/method/handler; answer operation/sequence assertions use the verified own-code conversion; all code after the offline helper (including earlier Undo tests), the Note edit proof and the helper's remaining persistence/upload/reopen checks are byte-for-byte unchanged. This is not compilation or runtime evidence.

Second run: `core-test/20261101T120028Z-32577` failed both cases. ReadFailed
passed the new answer facts and later Note capture checks, then the explicit
offline drive requested reconciliation and received `RuntimeIdentityUnverified`.
TemporarilyUnavailable failed exact equality of the persisted and physical Card.
That row difference needs field-level output from this synthetic fixture.
Neither result proves the offline prerequisite complete.

## Second-run assertion correction and diagnostics — fixer handoff

- Role: fixer; session `s-20261025T162405Z-9dc656e4`. Starting commit and original batch base remain the supplied values above; no Git lookup performed.
- Owned files for this patch: `core/crates/meguri-sync/tests/cloud_media_roundtrip.rs` and this offline review log only. No production changes.

For `core-test/20261101T120028Z-32577`, ReadFailed passed the immediate persisted Card/Revlog observer proof and the Note capture/pending checks, then failed at the deliberately requested offline reconciliation. Both offline `drive_ready` calls now require an error whose complete display string is exactly `cloud device operation failed: RuntimeIdentityUnverified`. Inspected own-code mapping: `RuntimeError::IdentityUnverified` becomes `ChildFailure::RuntimeIdentityUnverified`, and `ChildFailure::into_error` formats that catalog tag with this exact prefix. Other errors and unexpected success fail. Both existing no-engine assertions and identical persisted entity comparisons remain; matching verification must still precede successful ordinary reconciliation.

TemporarilyUnavailable failed the exact persisted-versus-physical Card payload equality before End. That equality now reports both complete serialized synthetic fixture rows, canonical encoded bytes, post-answer reader sequence, answer operation id, entity observed sequence/generation, current generation, change counter and optional mark/source. Diagnostic serialization/encoding results are printed without unwrapping, so diagnostics cannot replace the failing equality with a serialization panic. The Note last-capture lookup and all its existing assertions now report the expected edit sequence/generation, answer context and synthetic Note fields; returned proofs also print their actual fields. No inferred cause or production repair is claimed.

Runtime/compilation status: **UNRUN**. Root owns running both focused cases and the next production decision. Step budgets, exact payload equality, unchanged Service assertions, final exact accepted-save and peer/reopen checks, and earlier crash tests are preserved. No retries, delays, skips, weakened comparisons, build, Git, network or forbidden-source access were introduced/performed. One source-only consistency check is recorded below; it is not runtime evidence.

One source-only consistency check: **PASS**. Exact restoration of the four authorized assertion/diagnostic blocks reproduces the input test file byte-for-byte, preserving all other checks and earlier crash tests. Both offline error assertions match the inspected driver string; synthetic row and Note capture diagnostics retain their original equality/proof requirements. Compilation and both focused cases remain **UNRUN**.

## Third execution and startup diagnosis

`core-test/20261101T120600Z-35794`: ReadFailed PASS through the full workflow;
TemporarilyUnavailable FAIL on the actual Card image comparison. Tracking still
has the original new Card, observed seq/generation zero, no mark and counter
zero; the physical Card has the answer and its new review id.

Independent reviewer traced this to `meguri-host/src/open.rs`: only ReadFailed
sets the uncertain-startup flag. With TemporarilyUnavailable, the engine treats
Unknown as a mismatch and turns a settled adoption decision Pending. Runtime
then has no usable binding and its observer returns before capture. The repair
must preserve only the settled, intent-free adoption for these two uncertain
account results, leave the current identity Unknown and retain all remote gates.
Signed-out, switched and differing accounts keep their mismatch handling.


## Uncertain-startup admission repair — fixer handoff

- Role: fixer; session `s-20261025T162405Z-9dc656e4`.
- Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`; original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9` (supplied, no Git/history lookup).
- Owned paths: `core/crates/meguri-host/src/open.rs` and this log only.

The single startup match arm now handles `IdentityStatus::ReadFailed | IdentityStatus::TemporarilyUnavailable`, setting the existing `startup_identity_read_failed` preservation flag while returning `AccountBinding::Unknown`. This implements the supplied independent review diagnosis for K10.2/K11.1/K11.4 and K22.2. It supplies no stored account and changes no engine/runtime gate: the existing settled CloudAdopt, no-outstanding-intent restriction remains; unfinished adoption still needs real proof. SignedOut, SwitchedAccounts and available different identities retain their handling. Runtime identity verification and send/apply/purge/engine gates are unchanged.

Own-code provenance: independently edited Meguri's existing host startup arm from the supplied reviewed correction and inspected own source only. No forbidden sources, history, network, Git or build access; no policy/spec edits, test changes, retries, limit changes or flag/module refactoring. Earlier review evidence and concurrent work are preserved.

One bounded source-only consistency check: **PASS**. Reversing this exact arm replacement reproduces the pre-edit source byte-for-byte; the log is append-only with its previous contents intact. This is not compilation or runtime evidence.

Compilation and both named `cloud_media_roundtrip` offline cases: **UNRUN** in this filtered fixer session. Runner/root owns compile, focused verification, independent review, provenance/privacy/similarity checks and required gates. The supplied prior receipt `core-test/20261101T120600Z-35794` remains ReadFailed PASS / TemporarilyUnavailable RED; no post-fix pass, acceptance, P24 closure or milestone completion is claimed.

## Post-repair integrator verification

`core-test/20261101T121029Z-39656`: 29/29 PASS. This includes all 27 recovery
checks and both full offline-review cases, with their unchanged pending-state,
second-reopen, identity gate, accepted Service save, exact peer and media checks.
The earlier failed receipts remain retained. No retry policy was introduced.

Independent reviewer `native_bounds_review` found no P0–P2 findings in the
production branch repair. The settled adoption/no-intent restriction, Unknown
account input and signed-out/different-account boundaries remain intact.
`similarity/20261101T121024Z-39364` passes for the production change. The scoped
privacy guard also passed before commit. The original batch base stays in force;
this evidence accepts no unit, full gate, signed SDK run, device check or trial.


## Adoption account matrix repair — fixer handoff

- Role: fixer; session `s-20261025T162405Z-9dc656e4`. Original U113 start and batch base remain as recorded above; no Git/history lookup.
- Owned changes: `core/crates/meguri-host/tests/cloud_adoption_guards.rs` and this log only. No production, policy/spec or concurrent Sent/Undo changes.

Supplied host-negative receipt `core-test/20261101T121151Z-40734` is 48/49 PASS: `initial_adoption_activation_crashes_relaunch_with_actual_proof_and_account_guards` retained the old ReadFailed-only finished-activation expectation. Per the supplied K10.2/V9.1/K22.2 and R6.6 correction, `preserves_finished` now requires `cloud.adopt.after_activation` and either `readfailed` or `unknown` (TemporarilyUnavailable). The same existing matrix adds explicit `SignedOut` and `SwitchedAccounts` labels mapped to their exact `IdentityStatus` variants; these still require Pending at both crash points.

Unfinished `replace.after_rename` adoption remains Pending for every nonmatching/unavailable account. All existing actual activation proof, native-file identity, non-same-account unverified/send-error, final ReadFailed reopen with unchanged decision, persisted state and zero-backup assertions remain intact. No other test, assertion, retry, skip or time limit changed. The supplied 29/29 offline/recovery PASS receipt above remains prior integrator evidence.

The filtered fixer checked the source delta and all 12 case expectations. It did not execute Rust tests. Root retained the failed receipt and owns runtime verification and acceptance.

Expanded matrix verification: `core-test/20261101T121554Z-42616` PASS49/49,
covering cloud_adoption_guards, cloud_publication_phase and backup provenance.
The existing crash matrix now checks 12 combinations including SignedOut and
SwitchedAccounts. Independent reviewer native_bounds_review found no P0–P2
findings. The failed 48/49 result remains retained. Rust formatting followed.
