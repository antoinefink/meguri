# Cloud review capture recovery

- Fixer session: `s-20261025T162405Z-9dc656e4`; fix base: `64de94b6`.
- U113 original start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
- Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Specs: `sync/cloudkit-records#K11.4`, `sync/cloudkit-records#K12.3`.

Root reports actual RED `core-test/20261101T094641Z-44058` for
`cloud_media_roundtrip::durable_review_with_lost_capture_recovers_card_and_new_revlog_after_crash`.
Review execution, failed capture, WAL coverage, held operations, SIGKILL and
same-file recovery assertions passed before reconciliation returned
`RuntimeTracking(InvalidData)` at the packet's line 305.
Independent review cleared P0–P2 coverage; the shared fixture retains the
original assertions.

The fix adds Revlog to the two ordinary insertion guards in `reconcile_binding`
and `insert_workflow_entity`. Allocation, projection, capture precedence,
holdback, publication and structural restrictions retain their existing logic.
The obsolete RED comment is removed. The Saved-count diagnostic is replaced
in full; equality and receipt-to-ACK comparisons are unchanged. This also
rewrites the log flagged by `private/20261101T094700Z-44613`.
The seeded child driver remains unchanged.

One bounded source consistency check completed in the filtered runner.
Post-fix execution is unrun: Saved results, matching ACKs, the second reopen
and peer convergence remain assertions awaiting root's focused test.
Root also owns independent review and provenance/privacy/similarity checks.
No build, test, Git, network, forbidden-source read or unit acceptance here.

Integration evidence:
- native_conformance_lifecycle_audit: no P0-P2 findings in the regression
  or the two admission guards.
- core-test/20261101T095037Z-48521: PASS, 1/1. The actual test completes
  pending Card and Revlog recovery, Service Saved receipt/ACK comparisons,
  the second physical reopen and the other device's fetch.
- private/20261101T095038Z-48865: PASS.
- Wider regression checks and the required batch gate remain pending.
  No unit or release acceptance is claimed.
- core-format/20261101T095320Z-55545: PASS.
- core-test/20261101T095340Z-56161: 84/84 PASS across media roundtrip,
  review roundtrip, incoming intents and recovery.
- similarity/20261101T095122Z-49892: PASS before whitespace-only formatting.
- private/20261101T095219Z-53886: evidence log PASS.
- core-lint/20261101T095437Z-56675: PASS.
- private/20261101T095441Z-56926: final formatted source and log PASS.
- The required integration gate remains open.
