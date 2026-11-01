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
