# app/

## Project

- Build settings live only in `Config/*.xcconfig`, never in `project.pbxproj`: target settings in `<Target>.xcconfig`, configuration differences in `Debug`, `Profile`, `Release`, `AppStore` (Profile and AppStore include Release and override a few lines).
- Folders are synchronized groups: add, move or delete files under `Meguri/`, `MeguriWidget/`, `MeguriUITests/`, `MeguriSnapshotTests/` without touching the pbxproj. A plist or entitlements file there is copied as a resource unless the folder's exception set in the pbxproj lists it. The pbxproj changes only for a new target, package product or capability.
- A fresh checkout needs `tools/mg core-xcframework` before any `xcodebuild`: package resolution fails without the binary target. After that the Meguri schemes' pre-action rebuilds the core for the configuration and platform (log: `build/xcode/core-pre-action.log`; it unsets `SDKROOT`, which is `auto` here and breaks xcrun tools). A build without the pre-action (another scheme, `-target`) is caught by the Meguri target's "Check core configuration" phase, which fails when the core stamp's configuration, features (`MEGURI_CORE_FEATURES`) or slice do not match.
- Local packages are both `XCLocalSwiftPackageReference`s and folder references in the Packages group; without the folder references test plans cannot see package test targets ("There are no test bundles available to test").
- Pass `-derivedDataPath` under `build/`; `xcodebuild -target` without it writes `app/build/`.
- `tools/mg xcode-check` checks this project end to end; extend it when you add a configuration-dependent setting, entitlement or capability.

## Configurations

| | Hooks | AnkiWeb | Ids |
|---|---|---|---|
| Debug | yes | yes | `.dev` |
| Profile | yes | yes | `.dev` |
| Release | no | yes | daily driver |
| AppStore | no | no | daily driver |

- Profile is Release plus `MEGURI_TEST_HOOKS` (so performance tests can load fixtures) and therefore the `.dev` ids. R1866's "Release configuration" for performance tests means Profile. Never archive or distribute it.
- Never launch a Release or AppStore build on the Mac: it would replace the daily-driver app. Run their UI tests on simulators only (`TestHooksTests` skips them on the Mac).
- Building Release or AppStore for the Mac is enough to harm it: Xcode registers the product (daily-driver ids, widget included) with LaunchServices and PlugInKit. Unregister and delete it once done (`lsregister -u`, `pluginkit -r`, as `tools/mg xcode-check` does).
- Packages never test app compilation conditions: SwiftPM sees only debug/release. `MEGURI_TEST_HOOKS` code lives only in `Meguri/TestHooks/`, `ANKIWEB_ENABLED` code only in `Meguri/AnkiWeb/` (KTD21). `App/MeguriApp.swift` is the one other file that names the hooks.
- Packages expose seams instead (`AppModel`: `LaunchConfiguration`, `AppClock`, `CollectionLocation`, `InitialPresentation`). Views read time through `@Environment(\.appClock)`, never `Date()`.
- On the Mac a sheet takes `\.locale` from its own window, not from its presenter: presented content gets the launch environment again (`RootView`).
- On the Mac, never present an alert while the app is hidden (launched with `open -j`, as agents must, or a login item set to Hide): the window closes and the app quits, with no crash report. Present launch-time alerts after `await NSApp.untilUnhidden()`, as `RootView` does.

## Test hooks

- Launch options are `-Meguri<Name> <value>` (table in `TestHooks/LaunchOptions.swift`); UI tests build them with `MeguriUITests/Support/MeguriLaunch.swift`. A bad option replaces the UI with `testhooks.error.message`.
- Each UI area registers its routes (`area/screen[/state]`) in `RouteRegistry.standard`, from a `TestHooks/<Area>Routes.swift`, with literal paths: `tools/mg xcode-check` reads them from the source, launches the Mac app hidden on each and fails on a computed path.
- Fixtures must sit inside the checkout: launched outside XCTest, the sandboxed Mac hook build can read only there (`-Dev` entitlements). The Mac UI-test runner is sandboxed too and cannot write them: create fixtures before the run (committed under `Fixtures/`, or by `tools/mg` jobs), never from a test. Keep them out of the synchronized folders, whose files are copied into bundles (a folder in an exception set is not excluded).

## Signing and entitlements

- `MEGURI_SIGNING` in `Config/Signing.xcconfig`: `adhoc` until the `xcode-account` user step, then `team`.
- Mac entitlements are `Meguri-macOS[-Dev][-AdHoc].entitlements`: `-Dev` (hook builds) adds read-only access to the checkout, `-AdHoc` drops the App Group. Keep the shared keys identical in all four. R2041/KTD22: no capability beyond sandbox, network client and server, user-selected files and app-scope bookmarks (plus the App Group when signed).

## Tests

- Accessibility identifiers: `area.screen.element[.id]`, element names lowerCamel (R1964).
- System alerts drop the identifiers of their buttons, on iOS and on the Mac. Tests reach them through the alert (`app.alerts` on iOS, `app.sheets` on the Mac) and the button's label; identifier audits must accept them.
- `MeguriSnapshotTests` is the app-hosted bundle: snapshots and in-process tests of app code (`@testable import Meguri`, Debug only).
- Xcode gives every Mac app it builds for testing, UI tests included, read access to `/` (plus testmanagerd lookups): no Mac test can catch a sandbox read denial. The `-Dev` fixture entitlement matters for launches outside XCTest; check entitlements on products of `xcodebuild build`, never of a test build.
- Mac UI tests (`TestHooksTests`, `testMacWindowSize` included) run only through `tools/mg mac-ui`, never directly (root `AGENTS.md`). `tools/mg xcode-check` reports them as queued unless `MEGURI_ALLOW_MAC_UI=1`, so Mac-only coverage (window size, Mac routes) comes from the hourly mac-ui sweep.
- Test plans never retry failures (R1953). The `Performance` plan's options are fixed by R1866, and the `Meguri-Profile` scheme runs it without a debugger.
- The `Unit` plan runs on the Mac: `CoreBridgeTests` expects the Mac slice.
