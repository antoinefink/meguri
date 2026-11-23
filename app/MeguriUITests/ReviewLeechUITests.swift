import Foundation
import XCTest

/// The leech notice as one icon in the review's top controls (N16.11), on the
/// `leech-review` recipe: a threshold of 1 makes the next Again on a review
/// card a leech answer. The icon comes first in the trailing controls, stays
/// through the next card's reveal, shows its details on a press and goes when
/// that card is answered. Captures, light and dark, for the owner's review.
@MainActor
final class ReviewLeechUITests: XCTestCase {
    func testTaggedLeechLight() throws { try checkLeech(deck: "Leech Tag", dark: false, suspended: false) }
    func testSuspendedLeechDark() throws { try checkLeech(deck: "Leech Suspend", dark: true, suspended: true) }

    private func checkLeech(deck: String, dark: Bool, suspended: Bool) throws {
        let (app, bundleIdentifier) = try open(deck, dark: dark)
        let name = "\(suspended ? "suspended" : "tagged")-\(dark ? "dark" : "light")"
        let icon = element(app, "review.session.leech")
        XCTAssertFalse(icon.exists, "No icon before a leech answer")

        // The first card's Again is a leech answer; the next card shows the icon.
        try reveal(app, bundleIdentifier)
        try press(element(app, "review.session.rate.again"), app, bundleIdentifier)
        XCTAssertTrue(icon.waitForExistence(timeout: 10), "The icon appears with the next card")
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        #if os(iOS)
        // First in the trailing group: [leech][undo][more].
        let undo = element(app, "review.session.back"), more = element(app, "review.session.more")
        XCTAssertTrue(undo.exists && more.exists)
        XCTAssertLessThan(icon.frame.maxX, undo.frame.minX + 1, "The icon comes before Undo")
        XCTAssertLessThan(undo.frame.maxX, more.frame.minX + 1, "Undo comes before More")
        XCTAssertLessThan(abs(icon.frame.midY - undo.frame.midY), 4, "One row with Undo")
        #else
        let close = element(app, "review.session.close")
        XCTAssertGreaterThan(icon.frame.minX, close.frame.maxX, "A trailing toolbar item")
        #endif
        capture(app, "leech-\(name)-question")

        // It stays through the reveal of that card.
        try reveal(app, bundleIdentifier)
        XCTAssertTrue(icon.exists, "The icon stays on the answer side")
        capture(app, "leech-\(name)-answer")

        // A press shows what it means.
        try press(icon, app, bundleIdentifier)
        let details = element(app, "review.session.leech.details")
        XCTAssertTrue(details.waitForExistence(timeout: 10))
        settle()
        // The Mac exposes a text's words as its value, touch as its label.
        let text = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", "leech threshold", "leech threshold")).firstMatch
        XCTAssertTrue(text.exists, "The details say what a leech is")
        let suspendedText = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", "unsuspend", "unsuspend")).firstMatch
        XCTAssertEqual(suspendedText.exists, suspended, "The suspended variant says the card is suspended")
        capture(app, "leech-\(name)-popover")
        dismiss(app, details)

        // Answering that card ends it; the card after has no icon.
        XCTAssertTrue(icon.exists, "Closing the details keeps the icon")
        try press(element(app, "review.session.rate.good"), app, bundleIdentifier)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        let gone = XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),
                                                                    object: icon)], timeout: 10)
        XCTAssertEqual(gone, .completed, "The icon goes with the card it belongs to")
        capture(app, "leech-\(name)-next")
    }

    // MARK: Helpers

    private func open(_ deck: String, dark: Bool) throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundleIdentifier = try build.bundleIdentifier
        guard build.hasHooks, bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        #if os(iOS)
        XCUIDevice.shared.orientation = .portrait
        XCUIDevice.shared.appearance = dark ? .dark : .light
        addTeardownBlock { @MainActor in XCUIDevice.shared.appearance = .light }
        let extra = ["-MeguriSizeClass", "compact"]
        let appearance: String? = nil
        #else
        let extra: [String] = []
        let appearance: String? = dark ? "dark" : "light"
        #endif
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/leech-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            appearance: appearance, extra: extra).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let rows = HomeControls.deckRows(app, labelContaining: deck)
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 20), "Stage the generated leech-review-1 fixture in the VM")
        try press(HomeControls.hittable(rows) ?? rows.firstMatch, app, bundleIdentifier)
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        try press(study, app, bundleIdentifier)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        settle()
        return (app, bundleIdentifier)
    }

    private func reveal(_ app: XCUIApplication, _ bundleIdentifier: String) throws {
        let button = element(app, "review.session.reveal")
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        try press(button, app, bundleIdentifier)
        XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
        settle()
    }

    private func press(_ target: XCUIElement, _ app: XCUIApplication, _ bundleIdentifier: String) throws {
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        #endif
        XCTAssertTrue(target.waitForExistence(timeout: 10))
        target.clickOrTap()
    }

    private func dismiss(_ app: XCUIApplication, _ details: XCUIElement) {
        #if os(macOS)
        app.typeKey(.escape, modifierFlags: [])
        #else
        // A popover on every width: a tap outside it closes it, and only it.
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.45)).tap()
        #endif
        let closed = XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),
                                                                      object: details)], timeout: 10)
        XCTAssertEqual(closed, .completed, "The details close")
        settle()
    }

    private func settle(_ seconds: TimeInterval = 0.6) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
