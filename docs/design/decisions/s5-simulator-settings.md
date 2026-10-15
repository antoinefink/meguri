# Spike S5: setting the snapshot axes on simulators

Date: 2026-10-14. Unit: U10. Toolchain: Xcode 27.0 (27A266a), iOS 27.0 runtime 24A434, pool device Meguri-Pool-iphone-1 (iPhone 17 Pro). Question (swiftui-arch §9): which of the snapshot matrix's system-setting axes (R1971) can be set on a simulator, so that system chrome renders them?

## What was tried

- `xcrun devicectl device settings appearance --device <simulator UDID>` with each of `--mode`, `--text-size` (with `--larger-accessibility-sizes on`), `--increase-contrast`, `--reduce-transparency`, `--reduce-motion`, `--look-and-feel` and `--liquid-glass-opacity`, read back with `devicectl device info appearance`, and a pinned-status-bar home-screen screenshot (`simctl io screenshot`) after each change, compared with the default one.
- The same settings applied before an app-hosted snapshot run, rendering a probe (a `ControlBar` of glass buttons over striped content) in the app's key window.

## Result

| Setting | devicectl on a simulator | Rendered system chrome changes |
|---|---|---|
| Appearance (light, dark) | applies | yes (also set in-process by traits) |
| Text size, larger accessibility sizes | applies | yes (also set in-process by traits) |
| Increase Contrast | applies | yes (also set in-process by traits) |
| Reduce Transparency | applies | yes: the home-screen dock and the probe's glass turn opaque |
| Reduce Motion | applies; `UIAccessibility.isReduceMotionEnabled` follows | a still frame does not show it |
| Liquid Glass opacity (0.0 to 1.0; default 0.5) | applies | yes: the probe's glass differs at 0.0 and 1.0 |
| Look and feel (clear, tinted) | reports success, but the simulator lists one look only ("Liquid Glass") | no |

Simulator defaults: light, Large, Increase Contrast, Reduce Transparency and Reduce Motion off, opacity 0.5.

## Decision

- Appearance, Dynamic Type, Increase Contrast and the device class are set in the test process (traits, environment, window size) for every cell.
- Reduce Transparency, Reduce Motion and the glass opacity extremes are simulator settings: `tools/mg snapshots` lists each run's settings groups, sets each group with devicectl before its test run, checks it applied (`SystemSettingsTests` reads the accessibility flags; the probe of a group that changes Reduce Transparency or the opacity must render differently from the default group's, or the run fails), and restores the defaults afterwards.
- The tinted glass look is device-only on simulators, and so are the Mac's Reduce Transparency, Reduce Motion, glass opacity and look (the Mac's own settings are never changed, and system chrome ignores a SwiftUI environment override, so an in-process render would show the setting only in Meguri's own views). Their cells are `deferred`: listed in each run and checked at the monthly device session (`device-session.md`). Amended 2026-10-14 (U10 review): Mac Reduce Transparency and Reduce Motion cells had been rendered with the environment override.
- Mac snapshots render in an off-screen window, which cannot draw Liquid Glass (no backdrop: glass buttons lose their labels). They draw the control layer flat (`ChromeGlass.flat`), and the critic is told so; Mac glass is judged from `tools/mg screenshot --platform mac` and at the device session. SwiftUI on the Mac ignores Dynamic Type, so Mac cells use the default text size only.
- Foldable cells stay deferred until a foldable simulator exists (U252).
