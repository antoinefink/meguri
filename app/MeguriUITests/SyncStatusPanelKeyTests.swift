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

    /// Sync Off: Sync Settings is the default, so Return opens the Settings window.
    func testReturnRunsTheDefaultButton() throws {
        let (app, bundle) = try launch("settings/sync/status/off")
        defer { app.terminate() }
        let windows = app.windows.count
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(.return, modifierFlags: [])
        let panel = app.descendants(matching: .any)["sync.status.popover"]
        XCTAssertTrue(panel.waitForNonExistence(timeout: 5), "Return closes the panel through Sync Settings")
        let opened = expectation(for: NSPredicate(format: "count > %d", windows), evaluatedWith: app.windows)
        wait(for: [opened], timeout: 5)
    }

    /// Up to date: Esc closes the panel and opens nothing.
    func testEscapeOnlyClosesThePanel() throws {
        let (app, bundle) = try launch("settings/sync/status/meguri-up-to-date")
        defer { app.terminate() }
        let windows = app.windows.count
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(app.descendants(matching: .any)["sync.status.popover"].waitForNonExistence(timeout: 5))
        XCTAssertEqual(app.windows.count, windows, "Esc opens no window")
    }
    #endif
}
