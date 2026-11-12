# Other-epoch fetched zone deletion — 2026-11-12

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Start: `93077a67d981bc8b67e1e5f68658bea45eb18877`.
Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Production patch: `bf0b3b6e`. No Unit trailer or acceptance claim.

The native callback validates every supplied owner before forwarding other-epoch collection-zone deletions. Reserved, bound, malformed and foreign identities still refuse the whole callback. Unrelated metadata retains its no-op behavior. The typed event crosses MaintenanceLane and FFI with its original SDK identity. Rust removes the retained rows, including due marks, in the guarded callback transaction before any later cursor. A failed write stops intake. An old callback cannot invalidate its successor.

## Review and repairs

Author run `cleanroom/20261112T005118Z-64160` exports code and coverage. Reviewer `005906Z-65512` identifies two P2 defects. A stale zone list can reinsert a removed tracking entry. The test also uses a current inventory where it needs incremental changes. Repair `010252Z-67762` updates the existing Arc fence under the commit lock and reads all pages with service-issued tokens. The added case checks refusal of the stale list and admission of a new list after the zone exists again. Reviewer `010639Z-68631` reports no unresolved P0–P2 finding.

The privacy hook rejects added prose and a custom test name. Filtered rewrite `010754Z-69248` changes only one comment, that name and prose. The test body and executable production statements remain identical. The hook then passes. No guard exception is added.

Initial Rust receipt `005934Z-65949` passes three of four new cases; the initial-inventory assertion fails. Swift `005937Z-66226` compiles the new path, then CloudKit rejects an empty owner in a test constructor before Meguri executes. The fixer substitutes a second valid foreign-owner input and keeps all refusal assertions.

Rust `011032Z-69967` passes 47 of 48 cases. The remaining new test compares full entity facts after admission reconciles them. Probe `011514Z-74596` preserves that failing assertion: all 27 images are exactly equal before admission; afterward only observer-open and observer-sequence stamps differ. Every other field remains equal. Fixer `011615Z-74916` moves the same full equality before admission and retains every later cursor, row, pending-mark, zone and replay check. Independent review `011732Z-75363` finds no P0–P2 issue or waiver: this checks persistence at its correct boundary under K11.4.

## Focused evidence

- `core-test/20261112T011741Z-75744`: 48 tests pass in isolation. This covers the native absence, cleanup-listing and adapter owners, including every deletion cause, rollback, cursor blocking, replay, reopen, stale SDKs and both inventory orders.
- `swift-test/20261112T011035Z-70268`: 32 tests pass, including native classification, actual Core/FFI delivery, cancellation/drain order and the interval registry.
- `swift-test/20261112T011322Z-72627`: both original-result-held review/resume variants pass with the real Service.
- Static `011037Z-70934` and similarity `011036Z-70724` pass for the production patch. The final test-only correction awaits refreshed receipts.

The failure receipts and filtered exports remain retained. No assertion, timeout or privacy limit is relaxed. No normalization is used in the persistence assertion.

## Open scope

This implements only K12.5's other-epoch fetched-deletion row. Full root, bound and media-zone loss translation remains open, including cause-specific isolation, fresh account checks, bound-loss reads, send/reset mappings, cache cleanup and signed SDK evidence. Creation discovery and other P24/Q11 obligations remain separate. No spec row is closed here. The required integration gate remains pending at the original base.

Normal device arrival and review evidence, original missing media and hierarchy failures remain recorded in `cloud-native-zone-metadata-20261112.md` and `cloud-account-projection-20261112.md`. No account values or actual connection settings enter this log.
