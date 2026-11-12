# iOS/iPadOS remote-notification capability — 2026-11-12

- Role: fixer. Starting upstream commit: `dd068dff`; filtered source baseline: `7cf53697d039cd716b321e5cd439968b7e146f91`.
- Authority: `specs/sync/policy.md` P11.3 (R1553) and the U154 plan requirement to enable remote-notification delivery for CKSyncEngine.
- Scope: add only `UIBackgroundModes = [remote-notification]` to `app/Meguri/Info-iOS.plist`, preserving existing keys and preprocessing. Add an iOS assertion to the existing preprocessed Info configuration check in `tools/tests/cloud_build_configuration.bats`.
- No other U154 implementation: no polling, app-refresh, task registrations, notification permissions, custom push handler or lifecycle changes.
- Source consistency: `git diff --check` passed; the source diff preserves existing plist keys and preprocessing directives. The existing assertion will cover both readiness preprocessing branches when the integrator runs it.
- No builds, tests, native checks or acceptance executed or claimed. The integrator owns the native build, exact built-bundle inspection, independent review and required integration gates.
- This adds the required platform setting. The receive delay still needs diagnosis.
- Only approved Meguri source/specs were consulted; no forbidden sources/history, network, agents or private values were used.

## Integration evidence

- Independent read-only review `cleanroom/20261112T095731Z-58631`: no P0-P2 findings.
- `selftest/20261112T095848Z-62082`: all three affected checks passed, including both preprocessing branches and unchanged full-plist comparisons. Earlier `selftest/20261112T095816Z-59070` selected zero tests and provides no coverage.
- `similarity/20261112T100001Z-64621` and `static-checks/20261112T100153Z-65934` passed.
- Signed normal Mac Release build `build-app/20261112T095822Z-61062` passed. This is a Mac product; it does not verify the iOS capability in an installed phone bundle. That build and device observation remain pending.
- Privacy `private/20261112T100207Z-66331` flagged one sentence in this log. The sentence was rewritten; the guard is unchanged. The commit hook must pass the complete final text.
- No completed U154, full gate, delivery fix or release acceptance is claimed.

## Installed phone check

`run/20261112T100422Z-69088` passed with normal launch verified. The actual built phone Info has `UIBackgroundModes = [remote-notification]`. No notification permission prompt or new handler was added. This verifies configuration and launch, not a delivery deadline.
