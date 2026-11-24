import XCTest

/// Run only through the Mac VM UI lane. Uses the existing synthetic window
/// route to exercise MeguriApp's public Settings scene, with receiving off.
@MainActor final class SentenceMiningSettingsTests: XCTestCase {
    #if os(macOS)
    func testPublicSettingsDefaultDraftAndApply() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Requires development hooks")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
            route: "settings/sync/window/meguri-connected").arguments
        app.launch()
        defer { app.terminate() }
        let settings = app.windows.matching(NSPredicate(format: "identifier CONTAINS[c] %@", "Settings")).firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: build.bundleIdentifier)
        let row = settings.descendants(matching: .any).matching(identifier: "settings.root.integrations").firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.clickOrTap()
        let port = settings.textFields["settings.integrations.port"]
        let apply = settings.buttons["settings.integrations.applyPort"]
        XCTAssertTrue(port.waitForExistence(timeout: 5))
        XCTAssertEqual(port.value as? String, "44432")
        XCTAssertFalse(apply.isEnabled)
        XCTAssertTrue(settings.staticTexts["Off · Port 44432"].exists)

        func edit(_ value: String) {
            port.clickOrTap()
            port.typeKey("a", modifierFlags: .command)
            port.typeText(value)
        }
        edit("8765")
        XCTAssertTrue(apply.isEnabled)
        XCTAssertTrue(settings.staticTexts["Off · Port 44432"].exists, "Drafts do not apply")
        apply.clickOrTap()
        XCTAssertTrue(settings.staticTexts["Off · Port 8765"].waitForExistence(timeout: 5))
        XCTAssertFalse(apply.isEnabled)
        edit("1023")
        apply.clickOrTap()
        XCTAssertTrue(settings.staticTexts["settings.integrations.portError"].waitForExistence(timeout: 5))
        XCTAssertTrue(settings.staticTexts["Off · Port 8765"].exists)
        edit("44432")
        port.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(settings.staticTexts["Off · Port 44432"].waitForExistence(timeout: 5))
        XCTAssertFalse(apply.isEnabled)
    }
    #endif
}
