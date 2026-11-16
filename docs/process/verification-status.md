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
| Native conformance | The affected native regression binary passes all eight tests with zero skips in `conformance/20261116T094851Z-94699` on clean `efac37b97`, including the original failed case. [Evidence](review-log/U113-head-metadata-and-export-20261116.md#native-regression-follow-up). | Run the complete corpus and required gate. This focused receipt is diagnostic-only, not full conformance acceptance. |
| Route inventories | Focused-verified on clean `e5b32751`: both Mac registry assertions and the bad-launch UI case on iPhone Debug/Profile and iPad Debug pass with zero skips. [Repair record](review-log/test-route-inventories-20261116.md). | Include in the required Xcode, snapshot and integration checks. Other failures in those suites remain open. |
| CloudKit saved-image observation | The stale digest expectation is repaired. Four Mac protocol checks pass on clean `6a1e6ec9`, each with zero skips. [Repair record](review-log/cloud-saved-image-expectation-20261116.md). | Verify the original mobile app-hosted lane and broader suites. This is not a live sync pass. |
| Native replay accessibility | Still failed. Complete Mac native proof moved to the existing app-hosted Snapshot VM lane in `de9317f8`; its latest diagnostic run on clean `ccd8ecf`, `mac-hosted-test/20261116T125318Z-53598`, fails at owned path `root/0/0` (depth 2) with AXChildren error -25204 and no value. Original mobile Unit failures remain open. [Repair and evidence](review-log/replay-native-test-placement-20261116.md). | Localize the failing own-window read before another repair. Retain exact native identity, label, element revalidation and 20-press coverage. Mobile native proof is still unverified after passive diagnostics. |
| Replay fixture and lifecycle | The relocation is on master, with a VM-only real window and shared lifecycle guard. Host DOM/localization passes one test with three platform cases. Five selected app-hosted lifecycle/startup checks pass with zero skips. Earlier compiler failures, native crash and cleanup exception remain recorded. These passes do not clear the failed native proof or whole suites. | Keep the mandatory Snapshot proof and host safety boundary. Include changes in affected suites and the required integration gate; preserve all earlier failures. |
| Xcode matrix | The full Mac Unit component now passes on clean `85b53e8f` in `xcode-check/20261116T125837Z-67484`: 1,553 passed, zero failed, four skipped. The broader matrix remains failed; app-hosted and TestHooks failures from `20261116T023911Z-29156` remain open. | Retain the Mac Unit result. Resolve the other component failures before the required matrix and integration checks. |
| Review lifecycle ownership | Focused-verified: the complete Mac Unit workload on clean `85b53e8f`, `xcode-check/20261116T125837Z-67484`, passes all six originally failed methods. Suite cases are serialized; all original parameters, internal races, assertions and deadlines remain. No production fix or sole timing cause is established. [Evidence](review-log/review-lifecycle-suite-isolation-20261116.md). | Include the current source in the required integration gate. Preserve the original failures and diagnostic snapshots; this pass does not clear native replay or the full gate. |
| Credential integration | Focused-verified in the existing team-signed VM path on clean `85b53e8f`, `mac-hosted-test/20261116T133036Z-251`: the unchanged credential integration method passes one test with zero skips. The original ordinary-snapshot app was ad-hoc signed and returned a missing-entitlement error. | Verify ordinary snapshot signing authority. No store implementation, permissions or profile scope changed; the selected pass is not full snapshot acceptance. |
| Snapshots | Error `snapshots/20261116T033204Z-55003`, including route, review timer, CloudKit smoke/soak and settings failures. | Separate build/runtime failures from image differences. Preserve approval requirements. |
| Mac accessibility audit | Failed `a11y/20261116T034951Z-81143`. Its crash stack reaches CGS_REQUIRE_INIT inside the native window screenshot, before image sampling or an audit report. No contrast cause is established. | Test the independently reviewed Mac runner-initialization candidate against the original case. Keep native capture and all audit assertions. |
| iPhone navigation | Current selected check `ui-test/20261116T094127Z-64628` passes with zero skips on clean `f5fe5466`; a fresh native capture shows all four identifiers. No UI change. The earlier failure's cause remains unproven. [Observation](review-log/compact-tab-observation-20261116.md). | Retain the original failure and verify the case in the required gate. This selected pass grants no full-gate acceptance. |
| Mac card audit | Failed `ui-test/20261116T035957Z-89573`, `CardWebViewBehaviorUITests/testCardSurfaceAccessibilityAudit`, with the same native screenshot-runner crash as the sample audit. | Verify the original case after the bounded runner repair; do not infer a product audit defect from this crash. |
| Mac UI suites | Failed `mac-ui/20261116T040956Z-97003`: test-hooks, agent-driver, accessibility and replay-export errors. | Classify the original suite failures before rerunning. |
| Host tooling | Failed `selftest-host/20261116T020948Z-61543`: five rig/recorder cases; a retained error reports exhausted Docker address pools. | Diagnose owned resource state. Preserve other projects' Docker data and the server freeze. |
| Frozen server scenario | `normal_sync::delayed_meta_structural_schema_change_requires_one_way_before_start` remains failed in `core-test/20261116T010259Z-21190`. No server code or test repair is authorized by this register. | Retain the failure and the shared validator limitation under [the freeze](review-first-delivery.md#anki-server-freeze--2026-10-26). |
| Remaining jobs | The notices check is pending. Three T1 jobs did not run after T0 failed. | Run the required checks. Keep both states incomplete. |

The same gate passed both app builds, native durability, Core Swift tests,
iPhone accessibility, iPad UI tests and performance smoke. Those results do not
clear the failed groups. Existing optional and human-only exceptions keep their
documented scope; this register grants no waiver or unit acceptance.
