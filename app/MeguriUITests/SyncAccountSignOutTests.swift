import XCTest

/// N15.2, MP6.4: Account's Sign Out uses Service → Off's question and handoff.
/// These routes own no provider or credentials: confirmation must report failure,
/// never pretend that a real sign-out succeeded. Run only in the Mac test VM.
@MainActor
final class SyncAccountSignOutTests: XCTestCase {
    #if os(macOS)
    func testAccountSignOutCancellationConfirmationAndReconnectFields() throws {
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
        func press(_ target: XCUIElement) {
            XCTAssertTrue(target.isEnabled)
            XCTAssertTrue(target.isHittable)
            target.click()
        }
        func capture(_ name: String) {
            let attachment = XCTAttachment(screenshot: app.screenshot())
            attachment.name = name
            attachment.lifetime = .keepAlways
            add(attachment)
        }

        let settings = try launch("meguri-connected")
        XCTAssertFalse(settings.buttons["settings.sync.meguri.signOut"].exists, "Sign Out belongs inside Account")
        press(unique(settings, "settings.sync.meguri.account"))
        _ = unique(settings, "settings.sync.meguri.account.address")
        _ = unique(settings, "settings.sync.meguri.account.username")
        XCTAssertTrue(settings.staticTexts["https://sync.example.test/"].exists)
        XCTAssertTrue(settings.staticTexts["study-account"].exists)
        let signOut = unique(settings, "settings.sync.meguri.signOut")
        XCTAssertEqual(signOut.elementType, .button)
        XCTAssertEqual(signOut.label, "Sign Out…")
        capture("sync-account-signed-in")

        press(signOut)
        let question = settings.sheets.firstMatch
        XCTAssertTrue(question.waitForExistence(timeout: 5))
        XCTAssertTrue(question.staticTexts["Turn Off Sync?"].exists)
        let confirm = question.buttons["Turn Off Sync"]
        XCTAssertTrue(confirm.waitForExistence(timeout: 5))
        capture("sync-account-sign-out")
        question.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(question.exists, "Return must not confirm sign-out")
        press(question.buttons["Cancel"])
        XCTAssertTrue(question.waitForNonExistence(timeout: 5))
        XCTAssertTrue(signOut.exists && signOut.isEnabled)
        XCTAssertTrue(settings.staticTexts["study-account"].exists, "Cancel preserves the signed-in Account page")
        XCTAssertFalse(settings.descendants(matching: .any)["settings.sync.serviceChangeFailed"].exists)

        press(signOut)
        XCTAssertTrue(question.waitForExistence(timeout: 5))
        press(question.buttons["Turn Off Sync"])
        XCTAssertTrue(question.waitForNonExistence(timeout: 5))
        _ = unique(settings, "settings.sync.serviceChangeFailed")
        XCTAssertTrue(signOut.exists && signOut.isEnabled, "The inert fixture cannot commit a handoff; failure stays retryable in Account")

        // The inert fixture cannot reach success/Off. Check the existing unsigned
        // setup state separately, without inventing a successful credential removal.
        app.terminate()
        let reconnect = try launch("meguri-never-signed-in")
        for (name, value) in [("address", "https://reconnect.example.test/"), ("username", "another-account"), ("password", "synthetic-password")] {
            let field = unique(reconnect, "settings.sync.meguri.\(name)")
            XCTAssertEqual(field.elementType, name == "password" ? .secureTextField : .textField)
            press(field)
            field.typeKey("a", modifierFlags: .command)
            field.typeText(value)
            if name != "password" { XCTAssertEqual(field.value as? String, value) }
        }
        XCTAssertTrue(unique(reconnect, "settings.sync.meguri.signIn").isEnabled)
        capture("sync-account-reconnect")
    }
    #endif
}
