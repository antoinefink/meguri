import Foundation
import XCTest
#if os(macOS)

/// `decks/deck-list` H12 and contract N24.4 on the Mac, over the generated local-review-1
/// recipe: a double-click on a sidebar deck row, and Return on a selected deck row or on the
/// Today row, start a review, once; S typed into the deck search field types it. S on the
/// sidebar itself is not bound on the Mac: the table's type-select owns letters.
@MainActor
final class MacDeckListKeysTests: XCTestCase {
    func testDoubleClickOnADeckRowStartsItsReviewOnce() throws {
        let (app, bundle) = try launchFixture()
        let row = deckRow(app, path: "Local Review › Cards")
        try app.bringToFront(bundleIdentifier: bundle)
        row.doubleClick()
        let review = reviewScreen(app)
        XCTAssertTrue(review.waitForExistence(timeout: 20), "A double-click starts the deck's review")
        XCTAssertEqual(reviewWindows(app).count, 1, "One review, not one per click")
        try closeReview(app, bundle: bundle)
        // The second click's own action never starts another review once this one ended.
        XCTAssertFalse(reviewScreen(app).waitForExistence(timeout: 3))
        XCTAssertTrue(element(app, "home.deck.page.\(deckID(row))").waitForExistence(timeout: 10),
            "The first click opened the deck page, which shows again after the review")
    }

    func testReturnOnASelectedDeckRowStartsItsReview() throws {
        try assertKeyStartsDeckReview("\r")
    }

    func testReturnOnTheTodayRowStartsStudy() throws {
        let (app, bundle) = try launchFixture()
        let today = element(app, "shell.root.sidebar.home")
        try click(today, app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.today.studyAll").waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(reviewScreen(app).waitForExistence(timeout: 20), "Return on the Today row starts Study All Decks")
        XCTAssertEqual(reviewWindows(app).count, 1)
        try closeReview(app, bundle: bundle)
    }

    func testSTypedIntoTheDeckSearchFieldDoesNotStartAReview() throws {
        let (app, bundle) = try launchFixture()
        let row = deckRow(app, path: "Local Review › Cards")
        try click(row, app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.page.\(deckID(row))").waitForExistence(timeout: 10))
        let field = app.searchFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 10), "The sidebar's deck search field (N20.1)")
        try click(field, app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("s", modifierFlags: [])
        XCTAssertFalse(reviewScreen(app).waitForExistence(timeout: 3), "S in a text input types it (R197)")
        XCTAssertEqual(field.value as? String, "s")
    }

    // MARK: Support

    private func assertKeyStartsDeckReview(_ key: String) throws {
        let (app, bundle) = try launchFixture()
        let row = deckRow(app, path: "Local Review › Cards")
        try click(row, app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.page.\(deckID(row))").waitForExistence(timeout: 10),
            "A click opens the deck page and selects the row")
        XCTAssertFalse(reviewScreen(app).exists, "A click alone never starts a review")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(key, modifierFlags: [])
        XCTAssertTrue(reviewScreen(app).waitForExistence(timeout: 20), "Return starts the review")
        XCTAssertEqual(reviewWindows(app).count, 1)
        try closeReview(app, bundle: bundle)
    }

    private func launchFixture() throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1", directoryHint: .isDirectory),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let home = element(app, "shell.root.sidebar.home")
        XCTAssertTrue(home.waitForExistence(timeout: 20))
        try click(home, app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.collection.screen").waitForExistence(timeout: 20),
            "Stage the generated local-review-1 fixture through tools/mg")
        return (app, bundle)
    }

    private func deckRow(_ app: XCUIApplication, path: String) -> XCUIElement {
        let rows = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label == %@", "home.sidebar.deck.", path))
        XCTAssertTrue(rows.element(boundBy: 0).waitForExistence(timeout: 20))
        XCTAssertEqual(rows.count, 1)
        return rows.element(boundBy: 0)
    }

    private func deckID(_ row: XCUIElement) -> String { String(row.identifier.dropFirst("home.sidebar.deck.".count)) }

    private func reviewWindows(_ app: XCUIApplication) -> XCUIElementQuery {
        app.windows.containing(NSPredicate(format: "identifier == %@", "review.session.screen"))
    }

    private func reviewScreen(_ app: XCUIApplication) -> XCUIElement { element(app, "review.session.screen") }

    private func closeReview(_ app: XCUIApplication, bundle: String) throws {
        let close = element(app, "review.session.close")
        XCTAssertTrue(close.waitForExistence(timeout: 20))
        try click(close, app: app, bundle: bundle)
        XCTAssertTrue(reviewScreen(app).waitForNonExistence(timeout: 10))
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).element(boundBy: 0)
    }

    private func click(_ target: XCUIElement, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        target.clickOrTap()
    }
}
#endif
