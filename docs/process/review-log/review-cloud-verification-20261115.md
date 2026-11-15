# Review and cloud verification checkpoint

Source: `b1229b2f3e4507277039483969328cd47408fcd8`, clean and pushed. The pending batch starts at `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

`gate/20261115T145015Z-4203` was cancelled through MG after conformance completed. It is not a passing gate. Nine completed component receipts remain valid observations of this source:

- Static checks, static tree checks, pin canary, canary scenarios, Rust lint and dependency policy passed.
- `core-test/20261115T145429Z-24259`: 5,174 executed tests passed; ten skipped tests are not passes.
- `native-durability/20261115T152601Z-34107`: 316 passed, none skipped. This tests native storage, not Apple's network exchange.
- `conformance/20261115T152952Z-37233`: the selected T0 lane passed. Its summary reports 676 passes, 421 skips, 289 pending and two reference cases. These categories overlap; unsupported and inactive cases are not passes.

The gate stopped before the Apple lane. Its child Xcode check needs team signing for Mac Snapshot. Other Mac UI jobs retain their default signatures. No checks or assertions are disabled.

Focused results:

- `mac-hosted-test/20261115T155401Z-53645`: the real device-only Keychain integration test passed with team signing. One executed test, no skips or failures; synthetic credentials only.
- `ui-test/20261115T160142Z-60615`: the team-signed build passed, but the test app did not start. No review action ran. The reason remains unknown.
- The app's review controls and persisted sessions passed the standard Mac UI test. Its full receipt remains in the local build records.

The exported VM frame includes crash alerts over Meguri. It cannot establish visual quality. The alerts have not been attributed to a test or product failure.

`build-app/20261115T162051Z-68749` built the normal Mac preview with zero errors and warnings. Its signature, profile and executable hash were verified. The previous preview quit normally before building; exactly one new matching preview opened. Native inspection could not connect. The corrected process-limited lookup found its window, but capture returned an error. No visual pass is claimed.

These results establish local review behavior and protected credential storage. Earlier normal-app media transfer evidence remains recorded separately. A real Mac–iPhone review exchange in both directions, media rendering and playback, and retained state after restarting both devices remain unproved. The full batch gate and final acceptance remain pending. No Unit trailer is added.

## Gate signing repair

Start: `b1229b2f3e4507277039483969328cd47408fcd8`. Repair checkpoint: `ce5a7f8b`. The override lives inside the child command substitution. Parent and sibling signing modes stay intact; the child remains the final command, preserving its status and result handling.

`selftest/20261115T164331Z-82240` failed all three new signing assertions before the repair. An earlier test-only run also exposed Bats' errexit context; the failure-path test was corrected before the repair. `selftest/20261115T164412Z-82583` then passed all three. `selftest/20261115T164826Z-88359` passed all 117 gate regression tests, none skipped.

Independent read-only review `cleanroom/20261115T164806Z-87002`: CLEAR P0–P2 for this change. It traced direct, merge, scheduled and milestone callers. No source was changed by the reviewer. Privacy passes with the guard unchanged.

`similarity/20261115T165234Z-92674`: no flags; all three shell, Bats and instruction files are outside scanner coverage. The filtered implementer report was read. This is not similarity coverage of product source.

The separate catch-up path still inherits caller signing. A replay of the Xcode check therefore needs the explicit team mode until that path is repaired. Missing credential grants remain failures. This existing limitation is not a skip or an acceptance exception. The full batch gate remains pending.
