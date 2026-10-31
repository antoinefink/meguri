# Cloud baseline record metadata fix

- Role: fixer. Supplied seed base `d9df5e6ce59a6c60de93eda65f1dc208d745acf7`;
  checkout starting commit `dd31718ef0877951f237689582bf5161be1af8f0`.
  Original batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9` unchanged.
  Complete prepared seed and separately prepared/reviewed Rust V5 storage preserved.
- Reported red evidence: `swift-test/20261031T162505Z-70725`, seven policy/snapshot
  failures after R3 strict fetched-record metadata checks. Reported exact cleanD9
  baseline `swift-test/20261031T163555Z-78197` passes
  `cloudKitSnapshotIncludesInboxContentPendingRootAndStaleDeliveries` and
  `nativePolicyExchangeCompletesActualRuntimeCapture`; neither run repeated here.
- Baseline export left root/incoming type and zone unset. Swift's raw-record
  conversion defaults the root to MGEntity; strict fetched validation requires
  MGActive in meguri-root, invalidating the SDK before intended policy/cancel
  steps and cascading into staleCommand. The baseline closure now exports root
  MGActive/meguri-root and incoming MGEntity/the actual collection zone.
- The pendingRoot reconstruction replaced system fields but defaulted type to
  MGEntity. It now preserves `f.root.recordType` and `f.root.zone`. Every existing
  snapshot/capture/readiness/cancel/stale-delivery assertion is unchanged.
- Scope: only baseline record metadata in `test_hooks.rs`, pendingRoot
  reconstruction in `CloudKitSyncTests.swift`, and this note. No production
  validation/default/tracking changes or unrelated test edits.
- One source-only consistency pass. No forbidden sources, private content/settings
  logs, network, builds, tests, formatters or commits. Root must run the existing
  seven failing native policy/snapshot cases with rebuilt FFI bindings, including
  the two named cases, and complete independent review and required gates.
  Native verification remains unrun here; no acceptance claim.
