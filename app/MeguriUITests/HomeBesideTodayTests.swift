import Foundation
import XCTest
#if os(iOS)
import UIKit
#endif

/// The deck list beside Today (the opened Duo's, native-interface-contract N14.10, N26.1)
/// over the routes' sample decks, since no test can set the Duo's fold position (N13.4),
/// and Today's heading on the iPad (N26.2) over the generated typical recipe.
@MainActor
final class HomeBesideTodayTests: XCTestCase {
    /// N26.1: the List selects the row itself, so the system draws its highlight; a row
    /// painted with a background would leave its cell unselected. The production tree over
    /// the routes' sample decks: Today selected, then a deck (id 2) selected.
    func testListSelectsTheRowBesideTheDetail() throws {
        let app = try launch(route: "home/deck-list/beside-today")
        XCTAssertTrue(app.buttons["home.today.row"].waitForExistence(timeout: 20))
        XCTAssertTrue(cell(app, holding: "home.today.row").waitForSelected(), "Today's row is the list's selection")
        XCTAssertFalse(cell(app, holding: "home.deck.row.2").isSelected)
        XCTAssertTrue(app.staticTexts["home.collection.decksHeader"].exists, "The decks sit under their own header")
        app.terminate()

        // Deck 2 is a subdeck ("Japanese::Vocabulary" under deck 1): its row comes through
        // HomeDeckBranch's child branch (AnyView), so this checks a nested row's tag.
        let deckApp = try launch(route: "home/deck-list/beside-deck")
        XCTAssertTrue(deckApp.buttons["home.deck.row.2"].waitForExistence(timeout: 20))
        XCTAssertTrue(cell(deckApp, holding: "home.deck.row.2").waitForSelected(), "The shown deck's row is the selection")
        XCTAssertFalse(cell(deckApp, holding: "home.today.row").isSelected)
    }

    /// N26.2 on the iPad's Today: the totals are the title's subtitle, on the title's
    /// leading edge, which the page's groups share.
    func testTodayTotalsAreTheTitleSubtitle() throws {
        #if os(iOS)
        guard UIDevice.current.userInterfaceIdiom == .pad else {
            throw XCTSkip("Only the iPad's sidebar layout shows Today without forcing a size class")
        }
        #endif
        let app = try launch(fixture: "build/fixtures/recipes/typical-1")
        XCTAssertTrue(app.buttons["home.today.studyAll"].waitForExistence(timeout: 30), "Stage the generated typical-1 fixture in the VM")
        try assertTodayHeading(app)
    }

    private func assertTodayHeading(_ app: XCUIApplication) throws {
        XCTAssertFalse(app.staticTexts["home.today.totals"].exists, "The totals are no longer a row of the list")
        let title = app.navigationBars.staticTexts["Today"].firstMatch
        XCTAssertTrue(title.waitForExistence(timeout: 10))
        let subtitle = app.navigationBars.staticTexts.matching(NSPredicate(format: "label ENDSWITH %@", " to review")).firstMatch
        XCTAssertTrue(subtitle.exists, "Today's totals are the navigation subtitle")
        XCTAssertGreaterThan(subtitle.frame.minY, title.frame.minY, "The subtitle sits under the title")
        XCTAssertEqual(subtitle.frame.minX, title.frame.minX, accuracy: 1, "The title and its subtitle share a leading edge")
        let studied = app.otherElements["home.today.studied"]
        XCTAssertTrue(studied.exists)
        XCTAssertEqual(studied.frame.minX, title.frame.minX, accuracy: 1, "The page's groups start at the title's leading edge")
    }

    private func cell(_ app: XCUIApplication, holding identifier: String) -> XCUIElement {
        app.cells.containing(.button, identifier: identifier).firstMatch
    }

    private func launch(route: String? = nil, fixture: String? = nil) throws -> XCUIApplication {
        continueAfterFailure = false
        #if os(macOS)
        throw XCTSkip("The deck list beside Today and Today's touch heading are iOS layouts")
        #else
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Test hooks and an isolated development test container are required")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: fixture.map { build.repositoryRoot.appending(path: $0) },
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            route: route).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
        #endif
    }
}

private extension XCUIElement {
    @MainActor func waitForSelected(timeout: TimeInterval = 10) -> Bool {
        let selected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "selected == true"), object: self)
        return XCTWaiter.wait(for: [selected], timeout: timeout) == .completed
    }
}
