#if os(macOS)
import AppKit

/// The app delegate SwiftUI forwards to on the Mac (KTD10). It decides the
/// app's lifetime around its one window (R11, R1667): closing the window
/// quits Meguri, unless a Migaku or AnkiConnect integration is enabled,
/// whose servers live in this process; then the app keeps running and the
/// Dock icon (or Window > Meguri, which the `Window` scene provides) opens
/// the window again. ⌘Q always quits.
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Whether an integration is enabled. The integrations controller (U144)
    /// sets it; the test hooks set it from `-MeguriIntegrations`.
    static var closeCollection: (() async -> Bool)?

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let close = Self.closeCollection else { return .terminateNow }
        Task { sender.reply(toApplicationShouldTerminate: await close()) }
        return .terminateLater
    }

    static var keepsRunningWithoutWindow = false
    /// Opens the main window; the root view hands over SwiftUI's
    /// `openWindow` action, which outlives the window.
    static var openMainWindow: (() -> Void)?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        !Self.keepsRunningWithoutWindow
    }

    /// A click on the Dock icon (or opening the app again) with the window
    /// closed opens it.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        guard !hasVisibleWindows, let openMainWindow = Self.openMainWindow else { return true }
        openMainWindow()
        return false
    }
}
#endif
