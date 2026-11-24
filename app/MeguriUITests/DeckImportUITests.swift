import XCTest

/// U162's deck package import sheet over the scripted service
/// (`collection/deck-import/live*`): the main path, a merge confirmation and a
/// refusal, with the keyboard rules of `package/transfer-sheets` X2.3 (Return
/// never confirms a loss, Esc cancels). Run in the VM through
/// `tools/mg ui-test --platform mac|ipad --suite DeckImportUITests`.
@MainActor final class DeckImportUITests: XCTestCase {
    private let id = "settings.deckImport"

    private func launch(_ route: String) throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks else { throw XCTSkip("Requires the test hooks") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: route).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return (app, bundle)
    }

    func testTheMainPathImportsShowsTheLogAndCloses() throws {
        let (app, bundle) = try launch("collection/deck-import/live")
        XCTAssertTrue(element(app, "\(id).import").waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertTrue(text(app, "Import “Spanish Verbs.apkg”").waitForExistence(timeout: 5), app.debugDescription)
        XCTAssertTrue(element(app, "\(id).withScheduling").exists)
        XCTAssertTrue(element(app, "\(id).updateNotes").exists)
        capture(app, "deck-import-options")
        try click(element(app, "\(id).import"), app, bundle)
        let done = element(app, "\(id).done")
        XCTAssertTrue(done.waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertTrue(element(app, "\(id).undo").exists)
        XCTAssertTrue(element(app, "\(id).line.new").exists && element(app, "\(id).line.duplicate").exists, app.debugDescription)
        capture(app, "deck-import-result")
        #if os(macOS)
        // Return is Done here.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(.return, modifierFlags: [])
        #else
        try click(done, app, bundle)
        #endif
        XCTAssertTrue(waitGone(done), app.debugDescription)
        XCTAssertTrue(waitForLabel(element(app, "testhooks.deckImport.outcome"), "applies 1; tokens none"))
    }

    func testAMergeConfirmationIsNeverTheDefaultAndSendsItsToken() throws {
        let (app, bundle) = try launch("collection/deck-import/live-confirm")
        let importButton = element(app, "\(id).import")
        XCTAssertTrue(importButton.waitForExistence(timeout: 20), app.debugDescription)
        try click(importButton, app, bundle)
        let confirm = element(app, "\(id).confirm")
        XCTAssertTrue(confirm.waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertTrue(text(app, "Import “Spanish Verbs.apkg”?").waitForExistence(timeout: 5), app.debugDescription)
        XCTAssertTrue(element(app, "\(id).loss.emptied0").exists)
        XCTAssertTrue(element(app, "\(id).backup").exists)
        capture(app, "deck-import-confirmation")
        #if os(macOS)
        // X2.3: Return does nothing destructive.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(confirm.waitForExistence(timeout: 2))
        XCTAssertFalse(element(app, "\(id).done").exists)
        // Esc cancels back to the options; nothing was sent.
        app.typeKey(.escape, modifierFlags: [])
        #else
        try click(element(app, "\(id).cancel"), app, bundle)
        #endif
        XCTAssertTrue(waitGone(confirm), app.debugDescription)
        XCTAssertTrue(importButton.waitForExistence(timeout: 5))
        try click(importButton, app, bundle)
        XCTAssertTrue(confirm.waitForExistence(timeout: 20), app.debugDescription)
        try click(confirm, app, bundle)
        let done = element(app, "\(id).done")
        XCTAssertTrue(done.waitForExistence(timeout: 20), app.debugDescription)
        try click(done, app, bundle)
        XCTAssertTrue(waitGone(done))
        // Each Import asked first; the cancelled preview sent nothing; the confirm sent the shown token.
        XCTAssertTrue(waitForLabel(element(app, "testhooks.deckImport.outcome"), "applies 3; tokens none,none,2"),
                      label(element(app, "testhooks.deckImport.outcome")))
    }

    func testARefusalSaysWhyAndRetries() throws {
        let (app, bundle) = try launch("collection/deck-import/live-refused")
        let importButton = element(app, "\(id).import")
        XCTAssertTrue(importButton.waitForExistence(timeout: 20), app.debugDescription)
        try click(importButton, app, bundle)
        let refusal = element(app, "\(id).refusal")
        XCTAssertTrue(refusal.waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertTrue(label(refusal).contains("kept changing"), label(refusal))
        XCTAssertTrue(label(refusal).contains("unchanged"), label(refusal))
        capture(app, "deck-import-refusal")
        try click(element(app, "\(id).retry"), app, bundle)
        let done = element(app, "\(id).done")
        XCTAssertTrue(done.waitForExistence(timeout: 20), app.debugDescription)
        try click(done, app, bundle)
        XCTAssertTrue(waitForLabel(element(app, "testhooks.deckImport.outcome"), "applies 2"))
    }

    // MARK: Helpers

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }
    private func label(_ element: XCUIElement) -> String {
        [element.label, element.value as? String ?? ""].filter { !$0.isEmpty }.joined(separator: " ")
    }
    /// A title: the Mac's title bar gives it as the value, iOS as the label.
    private func text(_ app: XCUIApplication, _ text: String) -> XCUIElement {
        #if os(macOS)
        // The Mac's title leads the form (`SheetTitle`), with its own identifier.
        let title = element(app, "\(id).title")
        _ = title.waitForExistence(timeout: 5)
        XCTAssertTrue(label(title).contains(text), "title reads \(label(title))")
        return title
        #else
        return app.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", text, text)).firstMatch
        #endif
    }
    private func click(_ target: XCUIElement, _ app: XCUIApplication, _ bundle: String) throws {
        XCTAssertTrue(target.waitForExistence(timeout: 10), app.debugDescription)
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundle)
        #endif
        target.clickOrTap()
    }
    private func waitGone(_ element: XCUIElement, timeout: TimeInterval = 10) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: element)],
                         timeout: timeout) == .completed
    }
    private func waitForLabel(_ element: XCUIElement, _ text: String, timeout: TimeInterval = 10) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            element.exists && self.label(element).contains(text)
        }, object: nil)
        return XCTWaiter().wait(for: [expectation], timeout: timeout) == .completed
    }
    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
