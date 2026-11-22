import Foundation
import XCTest

/// N26.5 (Antoine, 2026-11-22): a deck row with subdecks, which carries the leading
/// disclosure, is as tall as one without, in the tab layout's list and in the list beside
/// Today; the row and its disclosure keep a 44-point target. The production tree over the
/// routes' sample decks: deck 1 ("Japanese") has a subdeck, deck 2 ("Vocabulary") none.
@MainActor
final class HomeDeckRowHeightTests: XCTestCase {
    func testRowsWithAndWithoutSubdecksAreEquallyTall() throws {
        continueAfterFailure = false
        #if os(macOS)
        throw XCTSkip("The Mac's deck list is the sidebar's, with the system's own rows")
        #else
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        for route in ["home/deck-list/typical", "home/deck-list/beside-today"] {
            let app = XCUIApplication()
            app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: route).arguments
            app.launch()
            let parent = app.buttons["home.deck.row.1"]
            let child = app.buttons["home.deck.row.2"]
            XCTAssertTrue(parent.waitForExistence(timeout: 20), route)
            XCTAssertTrue(child.exists, route)
            let parentCell = app.cells.containing(.button, identifier: "home.deck.row.1").firstMatch
            let childCell = app.cells.containing(.button, identifier: "home.deck.row.2").firstMatch
            XCTAssertEqual(parentCell.frame.height, childCell.frame.height, accuracy: 0.5,
                           "\(route): a row with a disclosure is as tall as one without")
            let disclosure = app.buttons["home.deck.disclosure.1"]
            XCTAssertTrue(disclosure.exists, route)
            for target in [parent, child, disclosure] {
                XCTAssertGreaterThanOrEqual(target.frame.height, 44, "\(route): \(target.identifier) keeps its target")
            }
            app.terminate()
        }
        #endif
    }
}
