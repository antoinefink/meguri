import Foundation
import XCTest
#if os(iOS)

/// N20.8 on touch: Undo and one More menu (Edit Note | Bury Card, Suspend
/// Card), Bury Card moving on to the next card, and Undo named after it.
@MainActor
final class ReviewActionsTouchUITests: XCTestCase {
    func testMoreMenuBuriesTheCardAndUndoNamesTheStep() throws {
        let app = try launch()
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deck.row.", "Local Review")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        row.tap()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        study.tap()
        let more = element(app, "review.session.more")
        XCTAssertTrue(more.waitForExistence(timeout: 20))
        XCTAssertEqual(more.label, "More")
        XCTAssertFalse(element(app, "review.session.editNote").exists, "No separate Edit Note button")
        let reveal = element(app, "review.session.reveal")
        XCTAssertTrue(reveal.waitForExistence(timeout: 20))
        more.tap()
        // The menu's items, in their groups (the system shows them as buttons).
        for title in ["Edit Note", "Bury Card", "Suspend Card"] {
            XCTAssertTrue(app.buttons[title].waitForExistence(timeout: 10), title)
        }
        app.buttons["Bury Card"].tap()
        // The next card shows on its question side; Undo names the bury.
        let undo = element(app, "review.session.back")
        XCTAssertTrue(undo.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForLabel(undo, "Undo Bury Card"), undo.label)
        XCTAssertTrue(reveal.waitForExistence(timeout: 10))
        undo.tap()
        XCTAssertTrue(reveal.waitForExistence(timeout: 10), "The buried card comes back on its question side")
        XCTAssertFalse(waitForLabel(undo, "Undo Bury Card", timeout: 2), "The bury is undone")
    }

    private func waitForLabel(_ element: XCUIElement, _ label: String, timeout: TimeInterval = 10) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", label), object: element)
        return XCTWaiter().wait(for: [expectation], timeout: timeout) == .completed
    }

    private func launch() throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", "compact"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
#endif
