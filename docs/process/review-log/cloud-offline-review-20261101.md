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

Assertions cover physical collection payloads, exact media bytes, retained prior rows/history, capture proofs for the answer and Note edit, observer marks and pending revisions for Note/Card/Revlog, unchanged baseline ACKs, and WAL coverage within the current engine lifetime. While identity is unavailable, bounded ordinary drives must leave Service zones and its request cursor unchanged, with no engine instance, send calls, SDK pending entries or engine/batch trace. A second normal close and unavailable reopen must preserve the complete physical image and durable entity facts before any verification; persisted scan state must remain undrained. Matching verification, ordinary reconcile and Sync Now/drain must then produce exactly one accepted Service Save per edited row, checked by existing `assert_review_saved`, including encrypted envelope, system fields and digest receipts. Receiver and peer physical images and exact media must converge and survive online reopens; the receiver native file and binding stay stable. No sequence comparisons cross engine lifetimes.

Testkit changes are only `IdentityReads::TemporarilyUnavailable` and its exact mapping to production `IdentityStatus::TemporarilyUnavailable`. No production guard or behavior changes, pending-model emulation, tracking/ACK seeding, fault injection, retries or skips were added. Existing lost-Undo tests are untouched.

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
