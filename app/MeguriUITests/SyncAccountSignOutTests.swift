import XCTest

/// N15.2, MP6.4: Account's Sign Out uses Service → Off's question and handoff.
/// These routes own no provider or credentials; the owned Mac journey covers
/// confirmed sign-out and reconnect fields. Run only in the Mac test VM.
@MainActor
final class SyncAccountSignOutTests: XCTestCase {
    #if os(macOS)
    func testAccountSignOutCancellation() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Requires development hooks") }
        let app = XCUIApplication()
        defer { app.terminate() }

        func launch(_ state: String) throws -> XCUIElement {
            app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
                route: "settings/sync/window/\(state)").arguments
            app.launch()
            let settings = app.windows.containing(NSPredicate(format: "identifier == %@", "settings.root.content")).firstMatch
            XCTAssertTrue(settings.waitForExistence(timeout: 10))
            try app.bringToFront(bundleIdentifier: bundle)
            return settings
        }
        func unique(_ scope: XCUIElement, _ id: String) -> XCUIElement {
            let matches = scope.descendants(matching: .any).matching(identifier: id)
            let target = matches.firstMatch
            XCTAssertTrue(target.waitForExistence(timeout: 10), id)
            XCTAssertEqual(matches.count, 1, id)
            return target
        }
        func press(_ target: XCUIElement) throws {
            try app.bringToFront(bundleIdentifier: bundle)
            XCTAssertTrue(target.isEnabled)
            XCTAssertTrue(target.isHittable)
            target.click()
        }
        func capture(_ settings: XCUIElement, _ name: String) {
            let attachment = XCTAttachment(screenshot: settings.screenshot())
            attachment.name = name
            attachment.lifetime = .keepAlways
            add(attachment)
        }

        let settings = try launch("meguri-connected")
        XCTAssertFalse(settings.buttons["settings.sync.meguri.signOut"].exists, "Sign Out belongs inside Account")
        try press(unique(settings, "settings.sync.meguri.account"))
        _ = unique(settings, "settings.sync.meguri.account.address")
        _ = unique(settings, "settings.sync.meguri.account.username")
        for fact in ["https://sync.example.test/", "study-account"] {
            XCTAssertTrue(settings.descendants(matching: .any)
                .matching(NSPredicate(format: "label == %@ OR value == %@", fact, fact)).firstMatch.exists)
        }
        let signOut = unique(settings, "settings.sync.meguri.signOut")
        XCTAssertEqual(signOut.elementType, .button)
        XCTAssertEqual(signOut.label, "Sign Out…")
        capture(settings, "sync-account-signed-in")

        try press(signOut)
        let question = settings.sheets.firstMatch
        XCTAssertTrue(question.waitForExistence(timeout: 5))
        XCTAssertTrue(question.staticTexts["Turn Off Sync?"].exists)
        let confirm = question.buttons["Turn Off Sync"]
        XCTAssertTrue(confirm.waitForExistence(timeout: 5))
        capture(settings, "sync-account-sign-out")
        try app.bringToFront(bundleIdentifier: bundle)
        question.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(question.exists, "Return must not confirm sign-out")
        try press(question.buttons["Cancel"])
        XCTAssertTrue(question.waitForNonExistence(timeout: 5))
        XCTAssertTrue(signOut.exists && signOut.isEnabled)
        _ = unique(settings, "settings.sync.meguri.account.content")
        XCTAssertTrue(settings.descendants(matching: .any)
            .matching(NSPredicate(format: "label == %@ OR value == %@", "study-account", "study-account")).firstMatch.exists,
            "Cancel preserves the signed-in Account page")
        XCTAssertFalse(settings.descendants(matching: .any)["settings.sync.serviceChangeFailed"].exists)
    }
    #endif
}
