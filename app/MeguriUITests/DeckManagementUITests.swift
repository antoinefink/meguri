import Foundation
import XCTest
#if os(macOS)
import AppKit

/// Production naming form, sidebar deck search, menus and U28 operations over
/// the generated local-review fixture (N20).
@MainActor
final class DeckManagementUITests: XCTestCase {
    func testNameFormCreatesRenamesAndNestsAcrossResize() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        try app.bringToFront(bundleIdentifier: bundle)
        // File › New Deck… (⇧⌘N): Name empty, Parent Deck None (D19.3).
        app.typeKey("n", modifierFlags: [.command, .shift])
        let field = element(app, "home.deckName.name")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, ""))
        XCTAssertTrue(element(app, "home.deckName.parent").exists)
        XCTAssertFalse(element(app, "home.deckName.footer").exists, "None has no footer")
        XCTAssertFalse(element(app, "home.deckName.confirm").isEnabled)
        let initialName = (17...32).map { "QZ\($0)" }.joined(separator: "-")
        let renamedName = "QZ33"
        // The form opens with the focus in Name.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText(initialName)
        // The window hook changes the layout without rebuilding the owner.
        try resize(app, bundle: bundle)
        XCTAssertTrue(waitForValue(field, initialName))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        // Create returns to the new deck's page: the form closes (N20.3).
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(sidebarRow(app, label: initialName).waitForExistence(timeout: 10), "The saved name is the composed one")
        let page = deckPage(app)
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        let selectedPage = page.identifier

        // Deck › Rename… (⇧⌘R): the deck's own name and its parent, None here.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("r", modifierFlags: [.command, .shift])
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, initialName))
        XCTAssertFalse(element(app, "home.deckName.confirm").isEnabled, "An untouched rename changes nothing (D19.4)")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("a", modifierFlags: .command)
        app.typeText(renamedName)
        XCTAssertTrue(element(app, "home.deckName.confirm").isEnabled)
        try click(element(app, "home.deckName.confirm"), app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, selectedPage).waitForExistence(timeout: 10), "Rename preserves selected deck identity")
        let row = sidebarRow(app, label: renamedName)
        XCTAssertTrue(row.waitForExistence(timeout: 10))

        // The row's menu: New Subdeck… starts empty inside the deck it came from.
        try app.bringToFront(bundleIdentifier: bundle)
        row.rightClick()
        // The row's own menu item: the menu bar's Deck menu has items of the same title.
        let deckID = row.identifier.dropFirst("home.sidebar.deck.".count)
        let subdeck = app.menuItems.matching(identifier: "home.deckMenu.subdeck.\(deckID)").firstMatch
        XCTAssertTrue(subdeck.waitForExistence(timeout: 10))
        try click(subdeck, app: app, bundle: bundle)
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, ""))
        XCTAssertTrue(waitForLabel(element(app, "home.deckName.footer"), "Inside \(renamedName)."))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("QZ34")
        XCTAssertTrue(waitForValue(field, "QZ34"))
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        let childPage = page.identifier
        XCTAssertNotEqual(childPage, selectedPage)
        // N14.4: the Mac names the open deck in the toolbar, by its own name.
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "value == %@", "QZ34")).firstMatch.waitForExistence(timeout: 10))

        // Rename of the child: Name QZ34 inside QZ33, so the composed saved name was QZ33::QZ34.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("r", modifierFlags: [.command, .shift])
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, "QZ34"))
        XCTAssertTrue(waitForLabel(element(app, "home.deckName.footer"), "Inside \(renamedName)."))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(element(app, childPage).waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, "home.deckName.sheet").exists)

        // The parent's rename says its subdeck moves with it.
        try app.bringToFront(bundleIdentifier: bundle)
        row.rightClick()
        let rename = app.menuItems.matching(identifier: "home.deckMenu.rename.\(deckID)").firstMatch
        XCTAssertTrue(rename.waitForExistence(timeout: 10))
        try click(rename, app: app, bundle: bundle)
        XCTAssertTrue(waitForLabel(element(app, "home.deckName.footer"), "Its subdeck moves with it."))
        try click(element(app, "home.deckName.cancel"), app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))

        // Parent Deck chosen in its menu: New Deck… inside QZ33.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("n", modifierFlags: [.command, .shift])
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        app.typeText("QZ35")
        let parent = element(app, "home.deckName.parent")
        try click(parent, app: app, bundle: bundle)
        let choice = app.menuItems[renamedName].firstMatch
        XCTAssertTrue(choice.waitForExistence(timeout: 10))
        try click(choice, app: app, bundle: bundle)
        XCTAssertTrue(waitForLabel(element(app, "home.deckName.footer"), "Inside \(renamedName)."))
        try click(element(app, "home.deckName.confirm"), app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "value == %@", "QZ35")).firstMatch.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("r", modifierFlags: [.command, .shift])
        XCTAssertTrue(waitForLabel(element(app, "home.deckName.footer"), "Inside \(renamedName)."),
            "Saved as QZ33::QZ35: the parent chosen in the menu")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
    }

    func testSidebarSearchOpensHighlightedDecksAndKeepsTheQuery() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        let pageIDs = try create(["QZ90::QZ91", "QZ90::QZ92"], app: app, bundle: bundle)
        let search = app.searchFields.firstMatch
        let rowIDs = pageIDs.map { "home.deckSearch.deck." + $0.dropFirst("home.deck.page.".count) }
        let firstRow = element(app, rowIDs[0]), secondRow = element(app, rowIDs[1])

        // "/" from the deck page the form just opened puts the focus in the sidebar's field.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("/")
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText("QZ90 QZ91")
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91"), "\"/\" from the deck page must reach the field")
        XCTAssertTrue(firstRow.waitForExistence(timeout: 10))
        XCTAssertEqual(firstRow.label, "QZ90 › QZ91", "Results read as paths with ›, never ::")
        XCTAssertFalse(secondRow.exists)
        XCTAssertFalse(element(app, "shell.root.sidebar.home").exists, "Results take the whole sidebar")
        try resize(app, bundle: bundle)
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91"))
        XCTAssertTrue(firstRow.exists)
        // Return opens the highlighted first result; the query and results stay.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, pageIDs[0]).waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(search, "QZ90 QZ91"))
        XCTAssertTrue(firstRow.exists)
        XCTAssertFalse(element(app, "review.session.screen").exists, "A result never starts review (N20.5)")

        // Edit › Find… (⌘F); ↓ moves the highlight without opening it, Return opens it.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("f", modifierFlags: .command)
        app.typeKey("a", modifierFlags: .command)
        app.typeText("QZ90")
        XCTAssertTrue(waitForValue(search, "QZ90"))
        // R197: "/" typed in the field is text, not Find.
        app.typeText("/")
        XCTAssertTrue(waitForValue(search, "QZ90/"))
        app.typeKey(XCUIKeyboardKey.delete.rawValue, modifierFlags: [])
        XCTAssertTrue(waitForValue(search, "QZ90"))
        XCTAssertTrue(secondRow.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.downArrow.rawValue, modifierFlags: [])
        XCTAssertTrue(element(app, pageIDs[0]).exists, "The highlight does not change the detail")
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, pageIDs[1]).waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, pageIDs[0]).exists)

        // Esc in the field clears it and the tree returns, the opened deck kept.
        try click(search, app: app, bundle: bundle)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(secondRow.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, "shell.root.sidebar.home").waitForExistence(timeout: 10))
        XCTAssertTrue(element(app, pageIDs[1]).exists)

        // ⇧⌘O is Find…'s second key.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .shift])
        app.typeText("QZ92")
        XCTAssertTrue(waitForValue(search, "QZ92"))
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
    }

    func testSearchCreateRowOpensNewDeckFormAndAnExactMatchSuppressesIt() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        let name = "QZCreate" + UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(8)
        let search = app.searchFields.firstMatch
        let createRow = element(app, "home.deckSearch.create")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("f", modifierFlags: .command)
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertFalse(createRow.exists, "Blank input must not offer creation")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeText(name)
        XCTAssertTrue(createRow.waitForExistence(timeout: 10))
        XCTAssertTrue(createRow.label.contains("Create deck named “\(name)”"), createRow.label)
        // Nothing listed: the create row is highlighted, so Return opens the form.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        let field = element(app, "home.deckName.name")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(field, name), "The form opens with the raw query as its Name")
        XCTAssertFalse(element(app, "home.deckName.footer").exists, "Parent Deck None")
        XCTAssertFalse(sidebarRow(app, label: name).exists, "Nothing is created before Create")
        try click(element(app, "home.deckName.confirm"), app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
        let page = deckPage(app)
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "value == %@", name)).firstMatch.waitForExistence(timeout: 10))
        let rowID = "home.deckSearch.deck." + page.identifier.dropFirst("home.deck.page.".count)

        // The same name in other case is an exact match (D18): a result, no create row.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("f", modifierFlags: .command)
        app.typeKey("a", modifierFlags: .command)
        app.typeText(name.lowercased())
        XCTAssertTrue(waitForValue(search, name.lowercased()))
        XCTAssertTrue(element(app, rowID).waitForExistence(timeout: 10))
        XCTAssertTrue(createRow.waitForNonExistence(timeout: 10), "An exact name match with different case must suppress creation")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
    }

    // MARK: Support

    private func launch() throws -> (XCUIApplication, String) {
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
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 20))
        return (app, bundle)
    }
    /// Creates each typed path with New Deck… and returns the pages that opened.
    private func create(_ paths: [String], app: XCUIApplication, bundle: String) throws -> [String] {
        var pageIDs: [String] = []
        for path in paths {
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeKey("n", modifierFlags: [.command, .shift])
            let name = element(app, "home.deckName.name")
            XCTAssertTrue(name.waitForExistence(timeout: 10))
            try app.bringToFront(bundleIdentifier: bundle)
            // "::" in the Name still nests (D19.1).
            app.typeText(path)
            XCTAssertTrue(waitForValue(name, path))
            try click(element(app, "home.deckName.confirm"), app: app, bundle: bundle)
            XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
            if let previous = pageIDs.last { XCTAssertTrue(element(app, previous).waitForNonExistence(timeout: 10)) }
            let page = deckPage(app)
            XCTAssertTrue(page.waitForExistence(timeout: 10))
            pageIDs.append(page.identifier)
        }
        XCTAssertEqual(Set(pageIDs).count, paths.count)
        return pageIDs
    }
    private func resize(_ app: XCUIApplication, bundle: String) throws {
        let resized = expectation(description: "The main window resized")
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: Notification.Name(bundle + ".testhooks.windowResized"),
            object: nil, queue: .main) { _ in resized.fulfill() }
        defer { center.removeObserver(observer) }
        center.postNotificationName(Notification.Name(bundle + ".testhooks.resizeWindow"), object: "minimum",
            userInfo: nil, deliverImmediately: true)
        wait(for: [resized], timeout: 10)
    }
    private func deckPage(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
    }
    private func sidebarRow(_ app: XCUIApplication, label: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label == %@", "home.sidebar.deck.", label)).firstMatch
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
    private func waitForLabel(_ target: XCUIElement, _ text: String) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(
            format: "exists == true AND (label == %@ OR value == %@)", text, text), object: target)], timeout: 10) == .completed
    }
}
#endif
