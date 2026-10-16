# Spike S4: keeping the Mac app alive without its window

Date: 2026-10-16. Unit: U150. Toolchain: Xcode 27.0 (27A266a), macOS 27.0.1 (26A434) in the test VM `meguri-mac`. Question (swiftui-arch §0.4, §9): does a `Window` scene with an `NSApplicationDelegateAdaptor` whose `applicationShouldTerminateAfterLastWindowClosed` returns false keep the app running after its only window closes, as R11 needs while an integration is enabled? SwiftUI's documentation says an app whose primary scene is a single `Window` quits when that window closes.

## What was tried

- The app as built: `Window("Meguri", id: "main")` and `AppDelegate` (`app/Meguri/Mac/AppDelegate.swift`). Its `applicationShouldTerminateAfterLastWindowClosed` returns `!keepsRunningWithoutWindow`; its `applicationShouldHandleReopen` opens the window with SwiftUI's `openWindow(id: "main")` when no window is visible. The test hooks set the flag from `-MeguriIntegrations on|off` and close the window as its close button does (`performClose`) on the `closeWindow` command.
- `NavigationLayoutTests` in the VM (`tools/mg ui-test --platform mac --suite NavigationLayoutTests`, run 20261016T105834Z-94766), no synthesized input:
  - `testClosingTheMacWindowQuitsWithoutIntegrations`: flag off, close the window.
  - `testClosingTheMacWindowKeepsTheAppWithIntegrationsAndReopeningShowsIt`: flag on, close the window, then open the running app again with `NSWorkspace.openApplication` (what a Dock click sends).
- In the app's own process (`ShellSnapshotTests`): the menu bar has no New Window item, no ⌘N and no Settings item.
- The Window menu after the window closed, read by the same UI test (run 20261016T131637Z-52970).

## Result

| Case | Result |
|---|---|
| Flag off, window closed | the app quits (not running within 2 s) |
| Flag on, window closed | no window, the process keeps running |
| Flag on, app opened again | the window comes back, on the same deck page: the process and its navigation state survived |
| Window menu, window closed | no item for the window: Close, Minimize, Zoom, the tiling items, Bring All to Front; no "Meguri" |

The delegate's answer overrides the `Window` scene's quit-on-close. The `Window` scene does not add a Window menu item that reopens it (swiftui-arch §2 assumed it would).

## Decision

Keep `Window` plus the delegate adaptor (KTD10). The fallback (`WindowGroup` with `.commandsRemoved()` and no ⌘N) is not needed. The app adds Window > Meguri itself (`Mac/MainWindowCommands.swift`, `openWindow(id: "main")`); the Dock icon reopens the window through `applicationShouldHandleReopen`. The integrations controller (U144) sets `AppDelegate.keepsRunningWithoutWindow` while an integration is enabled. Clicking Window > Meguri was not driven: synthesized input does not reach the app on macOS 27.0.1 (user step `mac-input`); the tests check the item is there, with the window open (in process) and after it closed (run 20261016T141307Z-24112).
