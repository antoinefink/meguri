#if os(macOS)
import AppKit
import Foundation
import XCTest

/// Real collection and production forms, on synthetic MG fixtures only. Every
/// attachment is the actual app window; never the workstation's screen.
@MainActor final class FilteredStudyUITests: XCTestCase {
    func testBrowseCreatesTwoTermPreviewAndFilteredMenuEditsEmptiesAndRebuilds() throws {
        let (app, bundle) = try launch(recipe: "local-review-1")
        defer { app.terminate() }
        try selectDeck("Local Review", app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("f", modifierFlags: [.command, .shift])
        let name = element(app, "home.filteredStudy.name")
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        XCTAssertTrue((element(app, "home.filteredStudy.first.search").value as? String ?? "").contains("is:due"))
        try click("home.filteredStudy.cancel", app: app, bundle: bundle)
        XCTAssertTrue(name.waitForNonExistence(timeout: 10))

        // The actual Browse query is serialized, including its tokens.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("b", modifierFlags: [.command, .shift])
        app.typeKey("f", modifierFlags: .command)
        app.typeKey("a", modifierFlags: .command)
        app.typeText("tag:local-review")
        app.typeKey(.return, modifierFlags: [])
        app.typeKey("f", modifierFlags: [.command, .shift])
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        XCTAssertTrue((element(app, "home.filteredStudy.first.search").value as? String ?? "").contains("tag:local-review"))
        try fill("home.filteredStudy.name", "Synthetic Preview", app: app, bundle: bundle)
        try fill("home.filteredStudy.first.limit", "2", app: app, bundle: bundle)
        try choose("home.filteredStudy.first.order", "Due first", app: app, bundle: bundle)
        try click("home.filteredStudy.secondEnabled", app: app, bundle: bundle)
        try fill("home.filteredStudy.second.search", "tag:local-review", app: app, bundle: bundle)
        try fill("home.filteredStudy.second.limit", "2", app: app, bundle: bundle)
        try choose("home.filteredStudy.second.order", "Oldest note first", app: app, bundle: bundle)
        capture(app, "filtered-study-editor-filled-name-and-two-searches-synthetic")
        try click("home.filteredStudy.reschedule", app: app, bundle: bundle)
        try fill("home.filteredStudy.again", "60", app: app, bundle: bundle)
        capture(app, "filtered-study-editor-filled-two-overlapping-searches-preview-synthetic")
        try click("home.filteredStudy.save", app: app, bundle: bundle)
        XCTAssertTrue(name.waitForNonExistence(timeout: 20))
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10))
        let page = deckPage(app).identifier
        XCTAssertFalse(element(app, "home.deck.empty.addNote").exists)

        // The former filtered refusal path now opens the editor and retains both terms.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .option])
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        XCTAssertEqual(name.value as? String, "Synthetic Preview")
        XCTAssertEqual(element(app, "home.filteredStudy.second.limit").value as? String, "2")
        capture(app, "filtered-study-editor-reopened-real-definition-synthetic")
        try click("home.filteredStudy.cancel", app: app, bundle: bundle)
        XCTAssertTrue(name.waitForNonExistence(timeout: 10))
        try menuAction("home.commands.emptyFilteredDeck", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.rebuild").waitForExistence(timeout: 15))
        XCTAssertFalse(element(app, "home.deck.study").exists)
        XCTAssertTrue(element(app, page).exists, "Empty keeps the selected filtered deck")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 15), "Undo restores real membership")
        try menuAction("home.commands.emptyFilteredDeck", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.rebuild").waitForExistence(timeout: 15))
        try click("home.deck.rebuild", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 15))
        try click("home.deck.study", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 15), "Filtered study reaches a real Core card")
        try click("review.session.reveal", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
        try click("review.session.rate.easy", app: app, bundle: bundle)
    }

    func testFinishedNormalDeckBuildsCustomPreviewStudiesAndInspectsTheHomeSchedule() throws {
        let (app, bundle) = try launch(recipe: "review-editor-1")
        defer { app.terminate() }
        try selectDeck("Synthetic Summary", app: app, bundle: bundle)
        try click("home.deck.study", app: app, bundle: bundle)
        try click("review.session.reveal", app: app, bundle: bundle)
        try click("review.session.rate.easy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.done").waitForExistence(timeout: 15))
        try click("review.session.customStudy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.mode").waitForExistence(timeout: 15))
        XCTAssertTrue(element(app, "home.customStudy.available").exists)
        capture(app, "custom-study-real-availability-from-finished-normal-deck-synthetic")
        try choose("home.customStudy.mode", "Study by state or tags", app: app, bundle: bundle)
        try choose("home.customStudy.state", "All cards (preview)", app: app, bundle: bundle)
        try fill("home.customStudy.amount", "1", app: app, bundle: bundle)
        try fill("home.customStudy.excludeTags", "unused::tag*", app: app, bundle: bundle)
        capture(app, "custom-study-filled-one-card-all-state-literal-tag-preview-synthetic")
        try click("home.customStudy.save", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 20))
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 15))
        let sessionPage = deckPage(app).identifier
        try click("home.deck.study", app: app, bundle: bundle)
        try click("review.session.reveal", app: app, bundle: bundle)
        try click("review.session.rate.easy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.done").waitForExistence(timeout: 15))
        XCTAssertFalse(element(app, "review.session.customStudy").exists, "Custom Study starts from a normal deck")
        try click("review.session.done", app: app, bundle: bundle)
        XCTAssertTrue(element(app, sessionPage).waitForExistence(timeout: 15))
        XCTAssertTrue(element(app, "home.deck.rebuild").waitForExistence(timeout: 15))

        // Inspect the actual returned card's schedule through production Browse/Card Info.
        let row = try selectDeck("Synthetic Summary", app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        row.rightClick()
        let id = try XCTUnwrap(HomeControls.deckID(row.identifier))
        try click("home.deckMenu.browse.\(id)", app: app, bundle: bundle)
        let table = element(app, "browse.results.table")
        XCTAssertTrue(table.waitForExistence(timeout: 15))
        let card = table.outlineRows.firstMatch
        XCTAssertTrue(card.waitForExistence(timeout: 15))
        try app.bringToFront(bundleIdentifier: bundle)
        card.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
        app.typeKey("i", modifierFlags: [.command, .option])
        XCTAssertTrue(element(app, "cardInfo.row.deck").waitForExistence(timeout: 15))
        XCTAssertEqual(element(app, "cardInfo.row.deck").value as? String, "Synthetic Summary")
        XCTAssertFalse((element(app, "cardInfo.row.interval").value as? String ?? "").isEmpty)
        XCTAssertFalse((element(app, "cardInfo.row.due").value as? String ?? "").isEmpty)
        capture(app, "custom-study-preview-returned-card-home-due-and-interval-synthetic")
    }

    // Literal recipe paths let MG discover and stage both fixtures in the VM.
    private func launch(recipe: String) throws -> (XCUIApplication, String) {
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw XCTSkip("Mac UI VM only") }
        continueAfterFailure = false
        let build = try BuildUnderTest.current(), bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Synthetic development container required") }
        let localReview = build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1")
        let reviewEditor = build.repositoryRoot.appending(path: "build/fixtures/recipes/review-editor-1")
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: recipe == "local-review-1" ? localReview : reviewEditor,
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            windowSize: "1440x1000", appearance: "light").arguments
        app.launch()
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 25))
        try app.bringToFront(bundleIdentifier: bundle)
        return (app, bundle)
    }
    @discardableResult private func selectDeck(_ name: String, app: XCUIApplication, bundle: String) throws -> XCUIElement {
        let rows = HomeControls.deckRows(app, labelContaining: name)
        XCTAssertTrue(wait { HomeControls.hittable(rows) != nil })
        let row = try XCTUnwrap(HomeControls.hittable(rows))
        let id = try XCTUnwrap(HomeControls.deckID(row.identifier))
        try app.bringToFront(bundleIdentifier: bundle)
        row.click()
        XCTAssertTrue(element(app, "home.deck.page.\(id)").waitForExistence(timeout: 15))
        return row
    }
    private func click(_ id: String, app: XCUIApplication, bundle: String) throws {
        let target = element(app, id)
        try app.bringToFront(bundleIdentifier: bundle)
        let scroll = app.sheets.scrollViews.firstMatch
        if scroll.exists {
            for _ in 0..<8 {
                if target.exists && target.isHittable { break }
                scroll.scroll(byDeltaX: 0, deltaY: -250)
            }
        }
        XCTAssertTrue(wait { target.exists && target.isEnabled && target.isHittable }, id)
        target.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
    }
    private func fill(_ id: String, _ value: String, app: XCUIApplication, bundle: String) throws {
        try click(id, app: app, bundle: bundle)
        app.typeKey("a", modifierFlags: .command); app.typeText(value)
    }
    private func choose(_ id: String, _ title: String, app: XCUIApplication, bundle: String) throws {
        try click(id, app: app, bundle: bundle)
        let item = app.menuItems.matching(NSPredicate(format: "title == %@ OR label == %@", title, title)).firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        item.click()
    }
    private func menuAction(_ id: String, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        app.menuBars.menuBarItems["Deck"].click()
        try click(id, app: app, bundle: bundle)
    }
    private func deckPage(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
    }
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func wait(_ condition: @escaping () -> Bool) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)], timeout: 15) == .completed
    }
    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
#endif
