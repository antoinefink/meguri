import XCTest

/// native-interface-contract N23.1 and N23.3, sync/status-strings T20.3: on the
/// Mac, Return runs the panel's default button and Esc only closes the panel.
/// The routes draw the production panel over an inert coordinator, so no
/// action reaches a server; what a key did shows in the windows.
@MainActor
final class SyncStatusPanelKeyTests: XCTestCase {
    #if os(macOS)
    private func launch(_ route: String) throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("Requires development hooks") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: route).arguments
        app.launch()
        let bundle = try build.bundleIdentifier
        XCTAssertTrue(app.descendants(matching: .any)["sync.status.popover"].waitForExistence(timeout: 10),
                      "The route opens the panel")
        return (app, bundle)
    }

    /// The Settings scene's window, by SwiftUI's identifier for it.
    private func settingsWindow(_ app: XCUIApplication) -> XCUIElement {
        app.windows.matching(NSPredicate(format: "identifier CONTAINS[c] %@", "Settings")).firstMatch
    }

    /// Sync Off: Sync Settings is the default, so Return opens the Settings window.
    func testReturnRunsTheDefaultButton() throws {
        let (app, bundle) = try launch("settings/sync/status/off")
        defer { app.terminate() }
        XCTAssertFalse(settingsWindow(app).exists)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(settingsWindow(app).waitForExistence(timeout: 5), "Return runs Sync Settings")
        XCTAssertTrue(app.descendants(matching: .any)["sync.status.popover"].waitForNonExistence(timeout: 5))
    }

    /// Up to date: Esc closes the panel and opens nothing.
    func testEscapeOnlyClosesThePanel() throws {
        let (app, bundle) = try launch("settings/sync/status/meguri-up-to-date")
        defer { app.terminate() }
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(app.descendants(matching: .any)["sync.status.popover"].waitForNonExistence(timeout: 5))
        XCTAssertFalse(settingsWindow(app).exists, "Esc opens no window")
    }
    #endif
}
