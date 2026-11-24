import XCTest

/// Run only through the Mac VM UI lane. Uses the existing synthetic window
/// route to exercise MeguriApp's public Settings scene, with receiving off.
@MainActor final class SentenceMiningSettingsTests: XCTestCase {
    #if os(macOS)
    func testPublicSettingsIndependentCheckboxes() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Requires development hooks")
        }
        let app = XCUIApplication()
        // Normal collection entry, isolated empty fixture, no device defaults.
        // Both listeners use OS-assigned test ports, never somebody else's app.
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"),
            locale: "en_US", animations: false, ankiConnectPort: 0, migakuPort: 0).arguments
        app.launch()
        defer { app.terminate() }
        XCTAssertTrue(app.staticTexts["Welcome to Meguri"].waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: build.bundleIdentifier)
        app.windows.firstMatch.typeKey(",", modifierFlags: .command)
        let settings = app.windows.containing(NSPredicate(format: "identifier == %@", "settings.root.content")).firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 10))
        let row = settings.descendants(matching: .any).matching(identifier: "settings.root.integrations").firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.clickOrTap()
        let migaku = settings.checkBoxes["settings.integrations.migaku.enabled"]
        let ankiConnect = settings.checkBoxes["settings.integrations.ankiConnect.enabled"]
        XCTAssertTrue(migaku.waitForExistence(timeout: 5) && ankiConnect.exists)
        XCTAssertTrue(migaku.label.contains("44432") && ankiConnect.label.contains("8765"))
        func check(_ migakuOn: Bool, _ ankiConnectOn: Bool) {
            XCTAssertEqual((migaku.value as? NSNumber)?.boolValue, migakuOn)
            XCTAssertEqual((ankiConnect.value as? NSNumber)?.boolValue, ankiConnectOn)
            for (name, enabled) in [("migaku", migakuOn), ("ankiConnect", ankiConnectOn)] {
                let status = settings.descendants(matching: .any).matching(identifier: "settings.integrations.\(name).status").firstMatch
                XCTAssertTrue(status.waitForExistence(timeout: 5))
                let expected = enabled ? "Listening on 127.0.0.1:" : "Off · Port"
                let predicate = NSPredicate { _, _ in
                    [status.label, status.value as? String ?? ""].contains { $0.contains(expected) }
                        || status.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", expected)).count > 0
                }
                XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: status)], timeout: 5), .completed)
            }
        }
        check(false, false)
        migaku.clickOrTap()
        check(true, false)
        ankiConnect.clickOrTap()
        check(true, true)
        migaku.clickOrTap()
        check(false, true)
        ankiConnect.clickOrTap()
        check(false, false)
        XCTAssertFalse(settings.textFields["settings.integrations.port"].exists)
    }
    #endif
}
