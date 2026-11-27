# Settings navigation and background suspension

Both fixes started at `2b80682e8`.

## Mac Settings

In `ui-test/20261127T072318Z-17634`, the native Back button worked. Clicking Collection from Troubleshooting left the Collection import control absent. The first candidate keyed the stack to the section. It failed in `ui-test/20261127T073955Z-44040`. The accessibility tree showed Collection selected and Troubleshooting still in the detail column.

The revised stack uses a bound path for Sync pages and clears it when the section changes. The final Mac run `ui-test/20261127T074859Z-56244` passed both tests. They cover native Back, switching from Troubleshooting to Collection, Backups and Integrations, returning to the Sync root, and cancelling account sign-out. The Collection screenshot uses the synthetic connected fixture. The build reported the same `NSCameraUsageDescription` warning as the baseline.

## Background suspension

The original code gave Core 1,500 ms after the local flush and put suspend errors in the Decks error state. In `swift-test/20261127T073634Z-39494`, the regressions reproduced both reported messages and the fixed timeout after expiry. The first budgeted candidate exceeded SQLite's signed millisecond limit when given an artificial large estimate. That case reached the real writer in `swift-test/20261127T074223Z-48926`.

The final run `swift-test/20261127T075011Z-59044` passed six methods with 16 cases. It checks elapsed-time accounting, unknown and late estimates, SQLite's limit, lease expiry, foreground return, superseded work and successful writes after resume. The two named phase errors are injected at the final Core call. The tests also exercise real Core teardown and resume.

## Review and provenance

The filtered runs exported the Mac fix in `cleanroom/20261127T074556Z-54446` and the final timeout bound in `cleanroom/20261127T074650Z-55148`. Independent review confirmed that an unknown system estimate still needs a local suspend attempt and that elapsed time must use a monotonic clock. A second review checked the bound Settings path and the other Sync entry points.

The independent reviewer classified the complete session record as public operational metadata. The verified SHA-256 for `s-20261127T070330Z-cfed632d.json` is `1eb0a79c228b86f2c4f29c2bc9fa0aa9a1ed8120cbe4ebde8213019b4f5671bf`. The final source passed the staged privacy scan in `private/20261127T075022Z-61448`. The pull request records the iPhone simulator and hosted check results.
