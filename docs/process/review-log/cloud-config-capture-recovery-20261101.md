# Cloud Config capture recovery

- Implementer session: `s-20261025T134357Z-235af9a2`.
- Component base: `f21aa4c8cc529d20a2f5ad704fbe71c155f0cdb3`.
- U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
- Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Specs: `sync/cloudkit-records#K6`, `sync/cloudkit-records#K11.4`,
  `sync/cloudkit-records#K12.3`, `sync/cloudkit-records#K15.8`.

The test uses an actual manual-bury operation on an adopted Card. Its new
Config row, failed capture, WAL receipt and exact payloads are checked before
SIGKILL. The same files reopen before ordinary reconciliation. Later assertions
require real Service Saved results, exact ACKs and peer convergence.

Only the new fixture removes the generator's setting before publication.
Existing review fixtures retain it. No binding or acknowledgement is seeded.

- Fixer session: `s-20261025T162405Z-9dc656e4`.
- Fix base: `bea38a29b48025a728fb82af3b88dc4824a2609e` plus verified tests-first seed.
- Actual RED: `core-test/20261101T100320Z-61442`, ordinary reconciliation
  failed with `RuntimeTracking(InvalidData)` at test line 473.
- Before that failure, real manual bury, exact Config value/mtime, capture
  rollback, WAL, unchanged server, SIGKILL and same-file reopen assertions passed.

Both ordinary-new guards now also admit Config; structural kinds remain blocked.
The independently cleared production two-guard repair is preserved exactly.

- Actual result: `core-test/20261101T100600Z-64219`, 84/85 passed. The Config
  regression cleared actual ordinary reconciliation, then failed
  `scan.snapshot_seq >= committed.seq` at test line 473.
- Independent review diagnosed P2 TEST-CONTRACT, not a production sequence bug:
  `engine/change-stream:198` guarantees ordering only within an engine lifetime;
  `Engine::new` in `lifecycle/mod:194` creates a new stream, and
  `changes/stream:130` starts it at 1. Counters across SIGKILL/reopen are unrelated.

The corrected test reads a current snapshot before reconciliation. The scan
and its WAL receipt must match that snapshot's generation and file identity.
Its receipt covers its sequence. All earlier durability and later Service
checks remain enabled. The production repair is unchanged.

One bounded source consistency check passed for this delta. Source privacy and
similarity passed for the current repair before this delta; final proof is unrun.
The previous actual RED remains recorded above. No unit acceptance claimed.

Integration evidence:
- native_conformance_lifecycle_audit: no P0-P2 findings in the fixture,
  production admission change or corrected snapshot proof.
- core-test/20261101T100943Z-67703: focused regression PASS, 1/1. Actual
  pending Card/Config recovery, Service Saved/ACKs, physical reopen, media
  preservation and peer convergence all complete.
- core-format/20261101T100935Z-67048: PASS.
- private/20261101T100933Z-66786: source files pass; log paragraph failed
  and was rewritten. private/20261101T101000Z-68070 passes after the rewrite.
- similarity/20261101T100936Z-67288: PASS.
- Required batch and milestone gates remain open; no unit acceptance.

Final focused verification:
- core-test/20261101T101024Z-68355: 85/85 PASS.
- core-lint/20261101T101057Z-69012: PASS.
- private/20261101T101051Z-68737: PASS.
The original failed runs remain retained. No full gate or unit pass is claimed.
