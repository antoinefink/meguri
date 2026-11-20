import Foundation
import XCTest
#if os(macOS)
import AppKit

/// Deck Options' keys on the Mac (N20.9, `sched/deck-options` O15.3): ⌥⌘O
/// opens the page on the shown deck, ⌘↩ saves as ⌘S does, and Save returns
/// to the deck page with the value kept.
@MainActor
final class DeckOptionsUITests: XCTestCase {
    func testOptionCommandOOpensThePageAndCommandReturnSaves() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundle = try build.bundleIdentifier
        guard bundle.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false).arguments
        app.launch()
        defer { app.terminate() }
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 20))

        let row = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "home.sidebar.deck.")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        row.clickOrTap()
        let page = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10))

        // Deck › Deck Options… (⌥⌘O).
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .option])
        let save = element(app, "deckOptions.options.save")
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        let field = element(app, "deckOptions.options.option.newPerDay")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        let original = (field.value as? String) ?? ""
        let changed = original == "37" ? "38" : "37"

        try app.bringToFront(bundleIdentifier: bundle)
        field.clickOrTap()
        app.typeKey("a", modifierFlags: .command)
        app.typeText(changed)
        XCTAssertTrue(waitForValue(field, changed))
        // Typed text counts as a change before it is committed (O15.3).
        XCTAssertTrue(waitForEnabled(save))

        // ⌘↩ saves: the page returns to the deck page.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: .command)
        XCTAssertTrue(save.waitForNonExistence(timeout: 20), "⌘↩ saves and closes the page")
        XCTAssertTrue(page.waitForExistence(timeout: 10))

        // The saved value is what the page shows when it opens again.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .option])
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        XCTAssertTrue(waitForValue(field, changed))
        XCTAssertFalse(save.isEnabled, "A page that opens has nothing to save")
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func waitForValue(_ target: XCUIElement, _ value: String) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND value == %@", value),
            object: target)], timeout: 10) == .completed
    }
    private func waitForEnabled(_ target: XCUIElement) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND enabled == true"),
            object: target)], timeout: 10) == .completed
    }
}
#endif
