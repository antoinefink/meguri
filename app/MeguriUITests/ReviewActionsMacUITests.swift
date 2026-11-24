import Foundation
import XCTest
#if os(macOS)

/// N20.8: the Mac's visible native review menu, using only the synthetic
/// local-review recipe in the isolated development container in the test VM.
@MainActor
final class ReviewActionsMacUITests: XCTestCase {
    func testToolbarMenuShowsReviewActions() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundleIdentifier = try build.bundleIdentifier
        guard build.hasHooks, bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US",
            animations: false, appearance: "light").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        try app.bringToFront(bundleIdentifier: bundleIdentifier)

        let rows = HomeControls.deckRows(app, labelContaining: "Local Review")
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 20), "Stage the synthetic local-review-1 recipe")
        (HomeControls.hittable(rows) ?? rows.firstMatch).click()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        study.click()
        let reveal = element(app, "review.session.reveal")
        XCTAssertTrue(reveal.waitForExistence(timeout: 20))
        let menu = element(app, "review.session.actions")
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertTrue(menu.isEnabled && menu.isHittable)
        XCTAssertEqual(menu.label, "Review actions")
        let window = app.windows.firstMatch
        let close = element(app, "review.session.close")
        XCTAssertGreaterThan(menu.frame.midX, window.frame.midX, "In the toolbar's trailing corner")
        XCTAssertLessThan(abs(menu.frame.midY - close.frame.midY), 4, "In the same toolbar as Back to Decks")
        menu.click()

        var previous: XCUIElement?
        for (id, title) in [("editNote", "Edit Note"), ("buryCard", "Bury Card"), ("suspendCard", "Suspend Card")] {
            // Scope captions to the action ID: Mac menu items can have empty
            // AX labels while their native titles still identify the command.
            let items = app.menuItems.matching(identifier: "review.menu.\(id)")
            let item = items.firstMatch
            XCTAssertTrue(item.waitForExistence(timeout: 10), title)
            XCTAssertEqual(items.count, 1, "One \(title) in the open menu")
            XCTAssertEqual(items.matching(NSPredicate(format: "title == %@", title)).count, 1)
            XCTAssertTrue(item.isEnabled, title)
            if let previous { XCTAssertLessThan(previous.frame.midY, item.frame.midY, "Native menu order") }
            previous = item
        }

        // Menus are separate native windows. Capture the VM's screen so the
        // attachment includes the open menu, rather than only the app window.
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "review-actions-mac-menu-open"
        attachment.lifetime = .keepAlways
        add(attachment)

        app.typeKey(.escape, modifierFlags: [])
        let edit = app.menuItems.matching(identifier: "review.menu.editNote").firstMatch
        XCTAssertEqual(XCTWaiter().wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: edit)], timeout: 10), .completed)
        XCTAssertTrue(reveal.exists, "Dismissing the menu leaves the question unanswered")
        XCTAssertFalse(element(app, "review.session.rate.good").exists)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
#endif
