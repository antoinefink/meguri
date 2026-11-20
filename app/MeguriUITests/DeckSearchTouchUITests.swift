import Foundation
import XCTest
#if os(iOS)

/// N20.1, N20.3 and D18 on touch: the tab layout's Search with its create row,
/// New Subdeck… with its preset parent, and the iPad sidebar's search.
@MainActor
final class DeckSearchTouchUITests: XCTestCase {
    func testTabLayoutSearchCreatesADeckThroughTheNewDeckForm() throws {
        let app = try launch(sizeClass: "compact")
        let name = "QZTouch" + UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(6)
        let search = element(app, "home.collection.search")
        XCTAssertTrue(search.waitForExistence(timeout: 20))
        search.tap()
        app.typeText(name)
        let create = element(app, "home.deckSearch.create")
        XCTAssertTrue(create.waitForExistence(timeout: 10), "No deck has the name: the create row follows D18's read")
        XCTAssertTrue(create.label.contains("Create deck named “\(name)”"), create.label)
        create.tap()
        let field = element(app, "home.deckName.name")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertEqual(field.value as? String, name, "The query fills Name; Parent Deck is None")
        XCTAssertFalse(element(app, "home.deckName.footer").exists)
        element(app, "home.deckName.confirm").tap()
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
        let title = element(app, "home.deck.title")
        XCTAssertTrue(title.waitForExistence(timeout: 10), "The created deck's page opens")
        XCTAssertEqual(title.label, name)
    }

    func testNewSubdeckStartsInsideTheDeckItCameFrom() throws {
        let app = try launch(sizeClass: "compact")
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deck.row.", "Local Review")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        row.press(forDuration: 1.2)
        let subdeck = app.buttons["New Subdeck…"].firstMatch
        XCTAssertTrue(subdeck.waitForExistence(timeout: 10))
        subdeck.tap()
        let field = element(app, "home.deckName.name")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        let footer = element(app, "home.deckName.footer")
        XCTAssertTrue(footer.waitForExistence(timeout: 10))
        XCTAssertEqual(footer.label, "Inside Local Review.")
        app.typeText("QZTouchChild")
        element(app, "home.deckName.confirm").tap()
        XCTAssertTrue(element(app, "home.deckName.sheet").waitForNonExistence(timeout: 10))
        let title = element(app, "home.deck.title")
        XCTAssertTrue(title.waitForExistence(timeout: 10))
        XCTAssertEqual(title.label, "QZTouchChild")
    }

    func testIPadSidebarSearchOpensTheDeckPage() throws {
        guard UIDevice.current.userInterfaceIdiom == .pad else { throw XCTSkip("The sidebar layout's search is iPad's") }
        let app = try launch(sizeClass: "regular")
        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout: 20))
        search.tap()
        app.typeText("Cards")
        let result = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deckSearch.deck.", "Local Review › Cards")).firstMatch
        XCTAssertTrue(result.waitForExistence(timeout: 10), "Results read as paths with ›")
        result.tap()
        let page = app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@",
            "home.deck.page." + result.identifier.dropFirst("home.deckSearch.deck.".count))).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10), "A tap opens the deck page")
        XCTAssertFalse(element(app, "review.session.screen").exists, "Never a review (N20.5)")
        XCTAssertEqual(search.value as? String, "Cards", "The query stays while the result is open")
    }

    /// N20.7 on touch: long press, Delete Deck, the system's confirmation, Delete Deck.
    func testLongPressDeleteDeckAsksThenDeletes() throws {
        let app = try launch(sizeClass: "compact")
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deck.row.", "Local Review")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        let deckID = row.identifier.dropFirst("home.deck.row.".count)
        row.press(forDuration: 1.2)
        let delete = element(app, "home.deckMenu.delete.\(deckID)")
        XCTAssertTrue(delete.waitForExistence(timeout: 10))
        delete.tap()
        // A confirmation dialog from the bottom, with the exact title and message.
        XCTAssertTrue(app.staticTexts["Delete “Local Review”?"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["Its subdeck and 4 cards will also be deleted."].exists)
        let cancel = app.buttons["Cancel"].firstMatch
        XCTAssertTrue(cancel.exists)
        cancel.tap()
        XCTAssertTrue(app.staticTexts["Delete “Local Review”?"].waitForNonExistence(timeout: 10))
        XCTAssertTrue(row.exists, "Cancel changes nothing")
        row.press(forDuration: 1.2)
        XCTAssertTrue(delete.waitForExistence(timeout: 10))
        delete.tap()
        let confirm = app.buttons.matching(NSPredicate(format: "label == %@", "Delete Deck")).element(boundBy: 0)
        XCTAssertTrue(app.staticTexts["Delete “Local Review”?"].waitForExistence(timeout: 10))
        XCTAssertTrue(confirm.waitForExistence(timeout: 10))
        confirm.tap()
        XCTAssertTrue(row.waitForNonExistence(timeout: 10), "Delete Deck removes the deck and its subdeck")
    }

    private func launch(sizeClass: String) throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", sizeClass]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
#endif
