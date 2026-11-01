# Cloud fixture typed assets — 2026-11-01

- Role: implementer; filtered source copy; current base `6ee50dc4`.
- Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
- Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Intent: remove redundant large integer-array JSON cycles in synthetic Swift fixtures.
- ServiceJSON retains Data buffers; explicit Codable preserves integer-array JSON.
- Binary preparation detaches only allowlisted Asset.bytes; logical replies retain buffers.
- Shared header/body/count/aggregate bounds and reference validation remain enabled.
- Typed and Data accessors share one prepared owner, worker queue and reply drain.
- Legacy Data replies retain exact raw JSON; malformed legacy input still reaches CLI.
- Prepared transports and original receipts retain semantic typed values.
- Original request/response JSON getters encode on demand and propagate errors.
- Production code, Rust protocol, workflow assertions, retries and deadlines unchanged.
- Parallel fixer's SyncCoordinator files and CloudServiceAppHarness.swift untouched.
- Existing callers need no edits: the one JSON getter caller already uses try.
- Extended existing tests through both Data and typed paths:
  `binaryAssetsPreparedCancellationDrainsBodiesAndConcurrentFramesStayAligned`;
  `binaryAssetsPreparationRejectsBoundsBeforeAcceptanceOrIO`.
- Added `typedAndDataAPIsPreserveOneMiBAssetsAndConditionalSavedReceipts`.
- Added `binaryAssetsRejectMalformedReferencesAndKeepBuffersAsOrdinaryJSONOutsideAssets`.
- Source consistency: one final owned-source diff/whitespace review; no builds/tests.
- Validation: source UNRUN; root owns compile, focused checks, independent review,
  provenance/privacy/similarity guards and unchanged-workflow phase timing.
- No performance claim; native measurements pending integration.

Root result: `swift-test/20261101T142916Z-8558` PASS20/20 for Process,
Inventory and ReviewMediaBinding. Independent review found no P0–P2.
Privacy and similarity checks pass. Native workflow timing and its shutdown
repair remain separate; no speedup, P24 or Unit acceptance is claimed.
