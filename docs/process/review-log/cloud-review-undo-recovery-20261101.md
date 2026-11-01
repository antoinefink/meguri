# Cloud review Undo capture recovery — test packet

- Role/session: implementer `s-20261025T134357Z-235af9a2`.
- Supplied base: `dc68c0a5faa185b76893763bd1f0205ad76b5bb9`.
- Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
- Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Contracts: `sync/cloudkit-records#K11.4`, `#K17.1`, `#K12.3`.

Supplied independent audit diagnosis (P1): a held actual answer creates a
never-sent Revlog. Actual Undo removes the physical row. If its observer capture
fails, the live tracking image remains. After a crash, the absent tracked Revlog
hits `WholeBindingReconciliation` in `workflow_runtime.rs`'s tracked-absence
guard (supplied lines 537–542), blocking sync. This is not evidence of uploading
an undone answer. The ordinary observer already removes an unsent Revlog in
`workflow_store.rs` (supplied lines 838–842); ordinary Undo is not assumed broken.

Added independently named tests in `cloud_media_roundtrip.rs`:

- `captured_review_undo_removes_never_sent_revlog_and_survives_crash`: normal
  capture control, using the same real answer/Undo/crash/reopen path.
- `lost_review_undo_capture_recovers_absent_never_sent_revlog_after_crash`:
  capture failure armed immediately before real Undo; expected RED at ordinary
  reconcile of the absent tracked Revlog, after proving physical Undo, failed
  capture rollback, stopped tracking, WAL coverage and same-file reopen.

The helper starts from `published_review_pair(false)` and verifies Service
Save results from publication and adoption. It checks actual Card
and Revlog payloads, successful answer capture, Core/cloud holds, zero server
revision, no flights and unchanged Service requests. Existing facts expose no
independent `sent` field; the never-handed precondition is established from those
actual facts and the absence of any Service request for the new Revlog. It does
not seed a `sent` value, ACK or tracking image. The existing public child
`undo()` already wraps only `self.call(Input::Undo)`; no driver change was needed.

Neither path drives, ends the review or reconciles between Undo and SIGKILL.
Reopen uses the same files without new key bytes, verifies file/binding identity
and physical payloads/media, then verifies account identity and invokes ordinary
reconcile. Sequence comparisons use only the same engine lifetime, following
the existing Config recovery test's reopened-observation pattern.

Post-reconcile assertions describe required future passing behavior: remove the
never-sent undone Revlog from tracking; classify the actual restored Card by its
row digest against the known ACK while checking its payload and ACK payload
digest; verify accepted Service Saves for the other pending
rows; retain physical durability and peer convergence, unrelated history/media,
empty SDK pending lists, and no Service attempt at all for the undone Revlog.

This patch adds tests only. The follow-up repair must check `!sent`, `server_erev == 0` and no in-flight evidence before dropping an
absent Revlog. K12.3 handed-flight evidence must survive. Already-sent deletion
remains separate work; this packet authorizes no broad absent-Revlog exception.

Verification: one bounded source consistency check only in the filtered runner.
No compilation, tests, builds, Git, network or acceptance performed here. RED is
expected and **unrun**, including the independent control. Root owns the actual
RED-first run, compilation, independent review, provenance/privacy/similarity
checks and final evidence. Existing tests, fixtures, step bounds and timeouts
are unchanged.

## Integrator reproduction

`core-test/20261101T114812Z-24328` ran both tests in Docker after formatting:
normal capture control PASS; lost capture test FAIL at the intended ordinary
reconcile boundary with `RuntimeIncompleteWholeBindingReconciliation`. All
physical Undo, capture-loss rollback, WAL and same-file crash/reopen assertions
preceded that failure successfully. No production repair applied yet.

Independent reviewer `native_conformance_lifecycle_audit` found no P0–P2
findings in the tests-first packet. It requires the repair to retain a durable
a stored handoff flag, safe decoding of old rows, and a test that retains the
flag through upload, ACK, flight removal and reopen.

P24 still needs the offline identity test that Q11 lists.
The spec-writer audit in `cloud-p24-scan-closure-20261101.md` found no other P24
prerequisite for the narrow absent-row/never-sent repair. No acceptance claimed.
