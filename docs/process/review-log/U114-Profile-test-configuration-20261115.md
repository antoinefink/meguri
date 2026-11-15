# U114 Profile test-configuration repair

Starting commit: `7b561614`. The full batch gate uses original base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Three tests used helpers available only in Debug, so Profile could not compile. Debug keeps its existing checks. Profile runs the supported cases. The SDK fake can now wait at a test gate before throwing the same error; with no gate it behaves as before. Production code, optimization, the benchmark and its five-millisecond limit are unchanged.

Filtered author: `cleanroom/20261115T054817Z-61783`; independent source review: `cleanroom/20261115T055535Z-67865`, no required P0–P2 findings. Root inspected the complete two-file patch before integration.

Verification:

- Profile: `swift-test/20261115T062708Z-96875`, PASS; 3 passed, 0 failed, 0 skipped. Includes the unchanged benchmark.
- Profile runtime capture `swift-test/20261115T063130Z-3784`: one method passed, zero failures/skips. This was a separate selection because the first filter did not include its full method name.
- Debug: `swift-test/20261115T063151Z-5085`, PASS; 3 passed, 0 failed, 0 skipped. All original parameter values ran.
- Privacy: `private/20261115T062719Z-99113`, pass for both source files.
- Similarity: `similarity/20261115T062736Z-99518`, two files scanned, zero flagged or allowlisted. Root read only its filtered implementer report.

The earlier full-gate Profile compilation failure `swift-test/20261115T053953Z-58052` remains retained. The enclosing gate `gate/20261115T042009Z-87341` was cancelled after the Profile and Kit failures were established, to repair them with focused checks; it is incomplete and does not pass. The partial Xcode run is not a passing whole-suite receipt. No unit, milestone, P24 or release acceptance is claimed. A complete gate remains required after pending repairs.
