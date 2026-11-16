# Verification status

**Integration status: failed.** No current passing full gate is established.

Last complete gate: `gate/20261116T005859Z-94440`, clean `35323460b`.
Keep integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Its result records 26 passing jobs, 11 failed jobs, one error and pending work.
T1 checks skipped after T0 did not run.

This register groups related failures. The original receipts retain every test
failure and attachment. Update a row only from actual verification evidence.
See [the repair rule](merge.md#failure-register).

| Check group | State and evidence | Next check |
|---|---|---|
| Rust head metadata, export ownership and CloudKit sort fixture | Focused-verified at `fefef71ad`: nine checks in `core-test/20261116T065928Z-24828` and 361 checks in `20261116T070312Z-26769` pass. [Repair record](review-log/U113-head-metadata-and-export-20261116.md). Full gate not rechecked. | Retain focused evidence; include the repair in the next required gate. |
| Native conformance | Failed `conformance/20261116T013712Z-42519` on the same CloudKit sort fixture. The Docker repair above passes; native execution remains unverified. | Run the affected native case. |
| Route inventories | Focused-verified on clean `e5b32751`: both Mac registry assertions and the bad-launch UI case on iPhone Debug/Profile and iPad Debug pass with zero skips. [Repair record](review-log/test-route-inventories-20261116.md). | Include in the required Xcode, snapshot and integration checks. Other failures in those suites remain open. |
| CloudKit saved-image observation | The stale digest expectation is repaired. Four Mac protocol checks pass on clean `6a1e6ec9`, each with zero skips. [Repair record](review-log/cloud-saved-image-expectation-20261116.md). | Verify the original mobile app-hosted lane and broader suites. This is not a live sync pass. |
| Native replay accessibility | Failed in Mac Swift package tests and both mobile Unit runs: `swift-test/20261116T022839Z-21716`, `unit-sim/20261116T040153Z-91363`, `20261116T040607Z-93798`. | Diagnose native observation and preserve every identity, label and activation assertion. |
| Replay diagnostic | The hostless Mac process had no AppKit application and its DOM fixture never publishes a native window. A VM-only fixture candidate refused the host `swift-test` path in `20261116T084546Z-15725`, before opening a window. Candidate remains outside master. | Bind native proof to a VM execution path before applying the fixture repair. Keep the original proof and the host safety guard. |
| Xcode matrix | Failed `xcode-check/20261116T023911Z-29156`: Mac Unit, app-hosted and TestHooks cases remain unresolved. | Inspect individual case failures and use focused selectors. |
| Snapshots | Error `snapshots/20261116T033204Z-55003`, including route, review timer, CloudKit smoke/soak and settings failures. | Separate build/runtime failures from image differences. Preserve approval requirements. |
| Mac accessibility audit | Failed `a11y/20261116T034951Z-81143`, sample-screen audit. | Inspect retained audit findings. |
| iPhone navigation | Failed `ui-test/20261116T035027Z-83215`, `NavigationLayoutTests/testTheTabLayoutHasDecksBrowseStatsAndSettings`. | Run the exact case after diagnosing its assertion. |
| Mac card audit | Failed `ui-test/20261116T035957Z-89573`, `CardWebViewBehaviorUITests/testCardSurfaceAccessibilityAudit`. | Inspect retained audit findings. |
| Mac UI suites | Failed `mac-ui/20261116T040956Z-97003`: test-hooks, agent-driver, accessibility and replay-export errors. | Classify the original suite failures before rerunning. |
| Host tooling | Failed `selftest-host/20261116T020948Z-61543`: five rig/recorder cases; a retained error reports exhausted Docker address pools. | Diagnose owned resource state. Preserve other projects' Docker data and the server freeze. |
| Frozen server scenario | `normal_sync::delayed_meta_structural_schema_change_requires_one_way_before_start` remains failed in `core-test/20261116T010259Z-21190`. No server code or test repair is authorized by this register. | Retain the failure and the shared validator limitation under [the freeze](review-first-delivery.md#anki-server-freeze--2026-10-26). |
| Remaining jobs | The notices check is pending. Three T1 jobs did not run after T0 failed. | Run the required checks. Keep both states incomplete. |

The same gate passed both app builds, native durability, Core Swift tests,
iPhone accessibility, iPad UI tests and performance smoke. Those results do not
clear the failed groups. Existing optional and human-only exceptions keep their
documented scope; this register grants no waiver or unit acceptance.
