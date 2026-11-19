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

    func testGoToDeckNativeSearchNavigationAndKeyboardOwnership() throws {
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

        let paths = ["QZ90::QZ91", "QZ90::QZ92"]
        let pagePrefix = "home.deck.page."
        let page = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", pagePrefix)).firstMatch
        var pageIDs: [String] = []
        for path in paths {
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeKey("n", modifierFlags: [.command, .option])
            let name = element(app, "home.deckName.name")
            XCTAssertTrue(name.waitForExistence(timeout: 10))
            try click(name, app: app, bundle: bundle)
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeText(path)
            XCTAssertTrue(waitForValue(name, path))
            try click(element(app, "home.deckName.confirm"), app: app, bundle: bundle)
            XCTAssertTrue(element(app, "home.deckName.done").waitForExistence(timeout: 10))
            XCTAssertTrue(savedName(app, path))
            try click(element(app, "home.deckName.done"), app: app, bundle: bundle)
            XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
            if let previous = pageIDs.last {
                XCTAssertTrue(element(app, previous).waitForNonExistence(timeout: 10))
            }
            XCTAssertTrue(page.waitForExistence(timeout: 10))
            let identifier = page.identifier
            XCTAssertTrue(identifier.hasPrefix(pagePrefix))
            XCTAssertFalse(identifier.dropFirst(pagePrefix.count).isEmpty)
            pageIDs.append(identifier)
        }
        XCTAssertNotEqual(pageIDs[0], pageIDs[1])
        // Derive picker row identities only from the pages the real create flow opened.
        let rowIDs = pageIDs.map { "home.goToDeck.deck." + $0.dropFirst(pagePrefix.count) }
        let picker = app.sheets.firstMatch
        let search = picker.textFields.matching(identifier: "home.goToDeck.search").firstMatch
        let list = picker.descendants(matching: .any).matching(identifier: "home.goToDeck.list").firstMatch
        let go = picker.buttons.matching(identifier: "home.goToDeck.go").firstMatch
        let firstRow = picker.descendants(matching: .any).matching(identifier: rowIDs[0]).firstMatch
        let secondRow = picker.descendants(matching: .any).matching(identifier: rowIDs[1]).firstMatch

        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .shift])
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertEqual(picker.textFields.matching(identifier: "home.goToDeck.search").count, 1)
        XCTAssertTrue(waitForValue(search, ""))
        XCTAssertTrue(list.waitForExistence(timeout: 10))
        XCTAssertTrue(go.waitForExistence(timeout: 10))
        // Presentation must focus the native query without a click or responder override.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("QZ90 QZ91")
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91"))
        XCTAssertTrue(firstRow.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(firstRow, paths[0]))
        XCTAssertFalse(firstRow.frame.isEmpty)
        XCTAssertTrue(list.frame.contains(firstRow.frame))
        XCTAssertTrue(firstRow.isHittable)
        XCTAssertFalse(secondRow.exists)
        XCTAssertTrue(go.isEnabled)

        let resized = expectation(description: "The main window resized with the picker query retained")
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: Notification.Name(bundle + ".testhooks.windowResized"),
            object: nil, queue: .main) { _ in resized.fulfill() }
        defer { center.removeObserver(observer) }
        center.postNotificationName(Notification.Name(bundle + ".testhooks.resizeWindow"),
            object: "minimum", userInfo: nil, deliverImmediately: true)
        wait(for: [resized], timeout: 10)
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91"))
        XCTAssertTrue(firstRow.exists)
        XCTAssertFalse(firstRow.frame.isEmpty)
        XCTAssertTrue(list.frame.contains(firstRow.frame))
        XCTAssertTrue(firstRow.isHittable)
        XCTAssertTrue(go.isEnabled)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 0)
        XCTAssertTrue(list.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, pageIDs[0]).waitForExistence(timeout: 10))

        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .shift])
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertTrue(list.waitForExistence(timeout: 10))
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertTrue(go.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(search, ""))
        try click(search, app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("QZ90 QZ91")
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91"))
        XCTAssertTrue(firstRow.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("/")
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91/"))
        // More text must reach the same native editor after slash, without refocusing it.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("QZ93")
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91/QZ93"))
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertTrue(picker.exists)
        XCTAssertTrue(list.exists)
        XCTAssertTrue(firstRow.waitForNonExistence(timeout: 10))
        XCTAssertTrue(secondRow.waitForNonExistence(timeout: 10))
        let createRow = picker.descendants(matching: .any).matching(identifier: "home.goToDeck.create").firstMatch
        XCTAssertTrue(createRow.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(createRow, "Create deck named QZ90 QZ91/QZ93"))
        XCTAssertFalse(createRow.frame.isEmpty)
        XCTAssertTrue(list.frame.contains(createRow.frame))
        XCTAssertTrue(createRow.isHittable)
        XCTAssertTrue(go.isEnabled)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 0)
        XCTAssertTrue(list.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, pageIDs[0]).waitForExistence(timeout: 10))

        XCTAssertFalse(app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label == %@", "home.sidebar.deck.", "QZ90 QZ91/QZ93")).firstMatch.exists,
            "Escape must not create the offered deck")

        // The visible toolbar action must expose the same native picker and real rows.
        let toolbarAction = app.toolbars.buttons.matching(identifier: "home.collection.goToDeck").firstMatch
        XCTAssertTrue(toolbarAction.waitForExistence(timeout: 10))
        XCTAssertTrue(toolbarAction.isHittable)
        try click(toolbarAction, app: app, bundle: bundle)
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertTrue(list.waitForExistence(timeout: 10))
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertTrue(go.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(search, ""))
        try click(search, app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("QZ90")
        XCTAssertTrue(waitForValue(search, "QZ90"))
        XCTAssertTrue(firstRow.waitForExistence(timeout: 10))
        XCTAssertTrue(secondRow.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(secondRow, paths[1]))
        XCTAssertFalse(secondRow.frame.isEmpty)
        XCTAssertTrue(list.frame.contains(secondRow.frame))
        XCTAssertTrue(secondRow.isHittable)
        try click(firstRow, app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.downArrow.rawValue, modifierFlags: [])
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 0)
        XCTAssertTrue(list.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, pageIDs[1]).waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, pageIDs[0]).exists)

        // Exercise the deck page's native key path once, without forcing a responder.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("/")
        XCTAssertTrue(picker.waitForExistence(timeout: 10), "Slash from the deck page must reach Home's native focus path")
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertTrue(list.waitForExistence(timeout: 10))
        XCTAssertTrue(go.waitForExistence(timeout: 10))
        try click(picker.buttons.matching(identifier: "home.goToDeck.cancel").firstMatch, app: app, bundle: bundle)
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 0)
        XCTAssertTrue(list.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, pageIDs[1]).waitForExistence(timeout: 10))
    }

    func testGoToDeckNativeCreateAndCaseInsensitiveExactMatch() throws {
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

        let name = "QZCreate" + UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(8)
        let picker = app.sheets.firstMatch
        let search = picker.textFields.matching(identifier: "home.goToDeck.search").firstMatch
        let list = picker.descendants(matching: .any).matching(identifier: "home.goToDeck.list").firstMatch
        let createRow = picker.descendants(matching: .any).matching(identifier: "home.goToDeck.create").firstMatch
        let go = picker.buttons.matching(identifier: "home.goToDeck.go").firstMatch
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .shift])
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertTrue(list.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(search, ""))
        XCTAssertFalse(createRow.exists, "Blank input must not offer creation")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText(name)
        XCTAssertTrue(waitForValue(search, name))
        XCTAssertTrue(createRow.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(createRow, "Create deck named " + name))
        XCTAssertFalse(createRow.frame.isEmpty)
        XCTAssertTrue(list.frame.contains(createRow.frame))
        XCTAssertTrue(createRow.isHittable)
        try click(createRow, app: app, bundle: bundle)
        XCTAssertTrue(go.isEnabled)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 0)
        XCTAssertTrue(list.waitForNonExistence(timeout: 10))
        let pagePrefix = "home.deck.page."
        let page = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", pagePrefix)).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        // N14.4: the Mac names the open deck in the window toolbar, not in the page's content.
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "value == %@", name)).firstMatch.waitForExistence(timeout: 10))
        let pageID = page.identifier
        XCTAssertTrue(pageID.hasPrefix(pagePrefix))
        XCTAssertFalse(pageID.dropFirst(pagePrefix.count).isEmpty)
        let rowID = "home.goToDeck.deck." + pageID.dropFirst(pagePrefix.count)

        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .shift])
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertTrue(list.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(search, ""))
        XCTAssertFalse(createRow.exists)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText(name.lowercased())
        XCTAssertTrue(waitForValue(search, name.lowercased()))
        let actualRow = picker.descendants(matching: .any).matching(identifier: rowID).firstMatch
        XCTAssertTrue(actualRow.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(actualRow, name))
        XCTAssertTrue(element(app, "home.goToDeck.checkingName").waitForNonExistence(timeout: 10))
        XCTAssertFalse(createRow.exists, "An exact name match with different case must suppress creation")
        XCTAssertFalse(actualRow.frame.isEmpty)
        XCTAssertTrue(list.frame.contains(actualRow.frame))
        XCTAssertTrue(actualRow.isHittable)
        try click(actualRow, app: app, bundle: bundle)
        XCTAssertTrue(go.isEnabled)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
        XCTAssertEqual(app.sheets.count, 0)
        XCTAssertTrue(list.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, pageID).waitForExistence(timeout: 10))
        // N14.4: the deck's name is the window's toolbar title.
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "value == %@", name)).firstMatch.waitForExistence(timeout: 10))
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
