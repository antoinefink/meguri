import Foundation
import XCTest

/// Where the deck tree and a deck's counts are on each layout (N14).
/// The tab layout lists decks as `home.deck.row.<id>`; the sidebar layouts
/// (Mac, iPad at regular width) show Today in the detail, so the sidebar's
/// `home.sidebar.deck.<id>` rows are their only deck tree.
@MainActor
enum HomeControls {
    static let rowPrefix = "home.deck.row."
    static let sidebarPrefix = "home.sidebar.deck."

    /// The deck's id from either selection control's identifier.
    static func deckID(_ identifier: String) -> String? {
        for prefix in [rowPrefix, sidebarPrefix] where identifier.hasPrefix(prefix) {
            return String(identifier.dropFirst(prefix.count))
        }
        return nil
    }

    /// The deck's selection controls, whichever layout shows.
    static func deckRows(_ app: XCUIApplication, deckID: String) -> XCUIElementQuery {
        app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@",
            [rowPrefix + deckID, sidebarPrefix + deckID]))
    }

    /// Selection controls whose accessibility label (the deck's full path) contains `text`.
    static func deckRows(_ app: XCUIApplication, labelContaining text: String) -> XCUIElementQuery {
        app.descendants(matching: .any).matching(NSPredicate(
            format: "(identifier BEGINSWITH %@ OR identifier BEGINSWITH %@) AND label CONTAINS %@",
            rowPrefix, sidebarPrefix, text))
    }

    /// The first of `query`'s elements that can take a tap or click now.
    /// The iPad's sidebar exposes a row's identifier on two elements.
    static func hittable(_ query: XCUIElementQuery) -> XCUIElement? {
        query.allElementsBoundByIndex.first { $0.exists && $0.isHittable }
    }

    static let nothingToStudy = "Nothing to study right now"

    /// The open deck page's new, learning and to-review counts.
    /// Touch pages show three labelled fields; the Mac shows them as the
    /// window's subtitle. "Nothing to study right now" (N14.3) is all zeros.
    /// Nil while the page shows neither.
    static func deckCounts(_ app: XCUIApplication) -> [Int]? {
        #if os(macOS)
        let texts = app.staticTexts.allElementsBoundByIndex.map { ($0.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? $0.label }
        if texts.contains(nothingToStudy) { return [0, 0, 0] }
        for text in texts {
            if let match = text.wholeMatch(of: /(\d+) new · (\d+) learning · (\d+) to review/),
               let new = Int(match.1), let learning = Int(match.2), let review = Int(match.3) {
                return [new, learning, review]
            }
        }
        return nil
        #else
        if app.staticTexts["home.deck.nothingDue"].firstMatch.exists { return [0, 0, 0] }
        var counts: [Int] = []
        for name in ["new", "learning", "due"] {
            let field = app.staticTexts.matching(identifier: "home.deck.counts.\(name)").firstMatch
            guard field.exists, let value = field.value as? String, let count = Int(value) else { return nil }
            counts.append(count)
        }
        return counts
        #endif
    }

    /// Waits until the open deck page shows exactly these counts.
    static func waitForDeckCounts(_ app: XCUIApplication, new: Int, learning: Int, review: Int, timeout: TimeInterval = 20) -> Bool {
        let expected = [new, learning, review]
        let shown = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in deckCounts(app) == expected }, object: nil)
        return XCTWaiter().wait(for: [shown], timeout: timeout) == .completed
    }
}
