# Native app package progress

Source: clean `9903258b`. Original pending batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Run: `swift-test/20261109T184913Z-7686`.
Command: `NSUnbufferedIO=YES tools/mg swift-test --package MeguriKit --config Debug`.

The full run completes: 754 tests, 753 passed, one failed, no skipped tests.
The failure is
`native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy()`.
It reports zero native links and fails its link-count, native-ID and unique-match
assertions. Its DOM checks pass. The source review cannot yet distinguish tree
publication timing, opaque descendants dropped by the test walker, or a product
accessibility defect. A bounded diagnostic is being prepared; the assertions
and limits remain enabled.

All 409 AppModel tests pass, including current review, media, resume and Service
fixtures. The native playback test where an older run stopped completes in
0.276 seconds. The 1,000-card page-reuse test completes in 33.441 seconds.
The separate 1,000-swap stress case retains one existing known WebKit issue.
This receipt neither explains the historical stall nor removes its evidence.

Independent diagnoses and next-check plans are retained under `build/`:
`cloud-replay-a11y-diagnosis-20261109.md`,
`cloud-batch-gate-readiness-20261109.md`,
`cloud-mac-unit-focus-plan-20261109.md` and
`cloud-snapshot-focus-diagnosis-20261109.md`.
Whole Mac Unit, app-hosted and snapshot checks, the original batch gate,
signed exchange and final acceptance remain pending. No unit is accepted here.
