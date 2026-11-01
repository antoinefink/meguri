# Cloud media snapshot root spelling — 2026-11-01

- Fixer session: `s-20261025T162405Z-9dc656e4`; supplied starting base `64de94b6`.
  Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`;
  original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Supplied actual full original App roundtrip diagnostic:
  `swift-test/20261101T094621Z-42449` reached the exact media comparison.
  Raw URLs and paths differed; standardized paths matched. Both name counts were
  1; name sets, name/case-key pairs, byte keys and exact byte maps matched.
  Both physical files existed with expected contents: no media loss observed.
- Independent review found P2: the common `MediaNameSnapshot` initializer exposed
  unstable equivalent root spellings. Source inspection confirmed the Rust DTO
  mapping delegates to that initializer, which stored the incoming URL unchanged.
- Normalize once there with `standardizedFileURL`, following the convention
  reported for `ReviewCollection.acceptsMedia`. Preserve generation and exact
  name/case-key bytes. No explicit symlink resolution, Rust/Host path changes,
  identity/security changes, App assertion/helper changes or budget changes.
- Added filesystem-free DTO coverage for equivalent directory spellings through
  explicit construction and Rust DTO mapping, requiring exact normalized URL
  equality and unchanged generation/name/case-key bytes. Existing expectations
  remain intact.
- Focused/native tests, independent review and provenance/privacy/similarity
  verification pending with the integrator; no build or test run in this filtered
  fixer session. One bounded source consistency check performed before handoff.
  No acceptance claim.

Integration evidence:
- native_bounds_review: no P0-P2 findings in the normalization repair.
- swift-test/20261101T095122Z-49784: 2/2 snapshot tests PASS.
- swift-test/20261101T095216Z-52797: original full App test PASS. Actual
  review and registered media survive reopen and fresh adoption through the
  production coordinator using the deterministic Service.
- private/20261101T095120Z-49509: PASS.
- similarity/20261101T095122Z-49892: PASS.
- Signed live-service and multi-device evidence remain required. This test
  is not Apple's network. The required batch gate and U113/U114 remain open.
