import Foundation
import XCTest
#if os(macOS)
import AppKit

/// Production sheet, menus and U28 operations over the generated local-review fixture.
@MainActor
final class DeckManagementUITests: XCTestCase {
    func testCreateRenameSubdeckShortcutsAndSheetStateAcrossResize() throws {
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
        let enable = element(app, "home.policy.enable")
        if enable.waitForExistence(timeout: 5) { try click(enable, app: app, bundle: bundle) }
        XCTAssertTrue(element(app, "home.collection.createDeck").waitForExistence(timeout: 20))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("n", modifierFlags: [.command, .option])
        let field = element(app, "home.deckName.name")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, "home.deckName.confirm").isEnabled)
        try click(field, app: app, bundle: bundle)
        let initialName = (17...32).map { "QZ\($0)" }.joined(separator: "-")
        let renamedName = "QZ33"
        let childPath = renamedName + "::QZ34"
        app.typeText(initialName)
        // Existing window hook changes layout without reconstructing the owner.
        let resized = expectation(description: "The main window resized with its name sheet open")
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: Notification.Name(bundle + ".testhooks.windowResized"),
            object: nil, queue: .main) { _ in resized.fulfill() }
        defer { center.removeObserver(observer) }
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name(bundle + ".testhooks.resizeWindow"), object: "minimum", userInfo: nil,
            deliverImmediately: true)
        wait(for: [resized], timeout: 10)
        XCTAssertTrue(waitForValue(field, initialName))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, "home.deckName.done").waitForExistence(timeout: 10))
        XCTAssertTrue(savedName(app, initialName))
        try click(element(app, "home.deckName.done"), app: app, bundle: bundle)
        let page = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        let selectedPage = page.identifier

        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("r", modifierFlags: [.command, .shift])
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, initialName))
        try click(field, app: app, bundle: bundle)
        app.typeKey("a", modifierFlags: .command)
        app.typeText(renamedName)
        try click(element(app, "home.deckName.confirm"), app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deckName.done").waitForExistence(timeout: 10))
        XCTAssertTrue(savedName(app, renamedName))
        try click(element(app, "home.deckName.done"), app: app, bundle: bundle)
        XCTAssertTrue(element(app, selectedPage).waitForExistence(timeout: 10), "Rename preserves selected deck identity")

        let sidebarRow = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label == %@", "home.sidebar.deck.", renamedName)).firstMatch
        XCTAssertTrue(sidebarRow.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        sidebarRow.rightClick()
        let subdeck = app.menuItems["New Subdeck…"].firstMatch
        XCTAssertTrue(subdeck.waitForExistence(timeout: 10))
        try click(subdeck, app: app, bundle: bundle)
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, renamedName + "::"))
        // Do not click or reposition the caret: R59's native insertion point is tested.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("QZ34")
        XCTAssertTrue(waitForValue(field, childPath))
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, "home.deckName.done").waitForExistence(timeout: 10))
        XCTAssertTrue(savedName(app, childPath))
        try click(element(app, "home.deckName.done"), app: app, bundle: bundle)
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        let childPage = page.identifier
        XCTAssertNotEqual(childPage, selectedPage)

        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("r", modifierFlags: [.command, .shift])
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, childPath))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(element(app, childPage).waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, "home.deckName.sheet").exists)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func click(_ target: XCUIElement, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        target.clickOrTap()
    }
    private func waitForValue(_ target: XCUIElement, _ value: String) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND value == %@", value),
            object: target)], timeout: 10) == .completed
    }
    private func savedName(_ app: XCUIApplication, _ name: String) -> Bool {
        let target = element(app, "home.deckName.savedName")
        return XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(
            format: "exists == true AND (label CONTAINS %@ OR value == %@)", name, name), object: target)], timeout: 10) == .completed
    }
}
#endif
