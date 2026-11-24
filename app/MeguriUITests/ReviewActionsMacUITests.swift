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
        let menu = app.menuButtons["review.session.actions"]
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertTrue(menu.isEnabled && menu.isHittable)
        let window = app.windows.firstMatch
        let close = element(app, "review.session.close")
        XCTAssertGreaterThan(menu.frame.midX, window.frame.midX, "In the toolbar's trailing corner")
        XCTAssertLessThan(abs(menu.frame.midY - close.frame.midY), 4, "In the same toolbar as Back to Decks")
        menu.click()

        // Menus are separate native windows. Capture the VM's screen before
        // assertions so a command, caption or keyboard-ownership failure keeps
        // the open menu visible in the evidence.
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "review-actions-mac-menu-open"
        attachment.lifetime = .keepAlways
        add(attachment)
        XCTAssertEqual(menu.title, "Review actions")

        // Scope to this control's menu; the menu bar repeats these commands.
        let items = menu.menuItems
        let edit = items["review.menu.editNote"]
        XCTAssertTrue(edit.waitForExistence(timeout: 10))
        let commands = items.allElementsBoundByIndex
        XCTAssertEqual(commands.map(\.identifier),
                       ["review.menu.editNote", "", "review.menu.buryCard", "review.menu.suspendCard"])
        let titles = ["Edit Note", "", "Bury Card", "Suspend Card"]
        XCTAssertEqual(commands.map(\.title), titles,
                       "The complete native menu, in order (including the separator)")
        XCTAssertEqual(commands.map(\.isEnabled), [true, false, true, true],
                       "The separator is disabled and all three commands are enabled")

        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(edit.waitForNonExistence(timeout: 10))
        XCTAssertTrue(reveal.exists, "Dismissing the menu leaves the question unanswered")
        XCTAssertFalse(element(app, "review.session.rate.good").exists)

        menu.click()
        XCTAssertTrue(edit.waitForExistence(timeout: 10))
        XCTAssertTrue(edit.isEnabled && edit.isHittable)
        edit.click()
        let form = element(app, "review.editCurrent")
        XCTAssertTrue(form.waitForExistence(timeout: 15), "The toolbar menu opens Edit Note")
        let cancel = element(app, "review.editCurrent.cancel")
        XCTAssertTrue(cancel.waitForExistence(timeout: 10))
        XCTAssertTrue(cancel.isEnabled && cancel.isHittable)
        cancel.click()
        XCTAssertTrue(form.waitForNonExistence(timeout: 10), "Cancel closes the unchanged draft")
        XCTAssertTrue(reveal.exists, "Closing Edit Note returns to the unanswered card")
        XCTAssertFalse(element(app, "review.session.rate.good").exists)
    }

    /// Option A (2026-11-24): Edit Note is a sheet on the review window. Back
    /// to Decks cannot end the review under it, Cancel sits beside Done at
    /// its bottom trailing corner, and Esc returns to the same card.
    func testEditNoteSheetKeepsTheReview() throws {
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
        let close = element(app, "review.session.close")
        XCTAssertTrue(close.isEnabled, "Back to Decks acts while no note is edited")

        let menu = app.menuButtons["review.session.actions"]
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        menu.click()
        let edit = menu.menuItems["review.menu.editNote"]
        XCTAssertTrue(edit.waitForExistence(timeout: 10))
        edit.click()
        let form = element(app, "review.editCurrent")
        XCTAssertTrue(form.waitForExistence(timeout: 15), "Review actions › Edit Note opens the form")
        let sheet = app.sheets.firstMatch
        XCTAssertTrue(sheet.waitForExistence(timeout: 10), "The form is a sheet on the review window")
        let cancel = sheet.descendants(matching: .any).matching(identifier: "review.editCurrent.cancel").firstMatch
        let done = sheet.descendants(matching: .any).matching(identifier: "review.editCurrent.done").firstMatch
        XCTAssertTrue(cancel.waitForExistence(timeout: 10) && done.waitForExistence(timeout: 10))
        let window = app.windows.firstMatch
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = "review-edit-note-sheet"
        attachment.lifetime = .keepAlways
        add(attachment)

        XCTAssertFalse(close.isEnabled, "Back to Decks cannot end the review under the form")
        XCTAssertLessThan(abs(cancel.frame.midY - done.frame.midY), 4, "Cancel and Done share the bottom bar")
        XCTAssertLessThan(cancel.frame.maxX, done.frame.minX, "Cancel comes before Done")
        XCTAssertGreaterThan(cancel.frame.minX, sheet.frame.midX, "Both sit at the trailing end")
        XCTAssertGreaterThan(cancel.frame.midY, sheet.frame.midY, "Both sit at the bottom")

        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(form.waitForNonExistence(timeout: 10), "Esc closes the unchanged draft")
        XCTAssertTrue(reveal.waitForExistence(timeout: 10), "The review stays on the unanswered card")
        XCTAssertFalse(element(app, "review.session.rate.good").exists)
        XCTAssertTrue(close.isEnabled, "Back to Decks acts again once the form has closed")
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
#endif
