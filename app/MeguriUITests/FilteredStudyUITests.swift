#if os(macOS)
import AppKit
import Foundation
import XCTest

/// Real collection and production forms, on synthetic MG fixtures only. Every
/// attachment is the actual app window; never the workstation's screen.
@MainActor final class FilteredStudyUITests: XCTestCase {
    func testBrowsePreviewOverlapsEmptyUndoRebuildAndTimedReturnPreservesSchedules() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        let eligible = try browseIDs("tag:filtered-ui -is:suspended", count: 2, app: app, bundle: bundle)
        let suspended = try browseIDs("tag:filtered-ui is:suspended", count: 1, app: app, bundle: bundle)
        let before = try schedules(eligible, app: app, bundle: bundle)
        capture(app, "filtered-preview-home-schedule-before")

        try selectDeck("Preview Source", app: app, bundle: bundle)
        try assertCommandAvailability(filtered: false, app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("f", modifierFlags: [.command, .shift])
        XCTAssertTrue(element(app, "home.filteredStudy.name").waitForExistence(timeout: 10))
        XCTAssertTrue((element(app, "home.filteredStudy.first.search").value as? String ?? "").contains("is:due"))
        try click("home.filteredStudy.cancel", app: app, bundle: bundle)

        _ = try browseIDs("tag:filtered-ui", count: 3, app: app, bundle: bundle)
        app.typeKey("f", modifierFlags: [.command, .shift])
        XCTAssertTrue(element(app, "home.filteredStudy.name").waitForExistence(timeout: 10))
        XCTAssertTrue((element(app, "home.filteredStudy.first.search").value as? String ?? "").contains("tag:filtered-ui"))
        try fill("home.filteredStudy.name", "Synthetic Preview", app: app, bundle: bundle)
        try fill("home.filteredStudy.first.limit", "1", app: app, bundle: bundle)
        try choose("home.filteredStudy.first.order", "Due first", app: app, bundle: bundle)
        try click("home.filteredStudy.secondEnabled", app: app, bundle: bundle)
        try fill("home.filteredStudy.second.search", "tag:filtered-ui", app: app, bundle: bundle)
        try fill("home.filteredStudy.second.limit", "5", app: app, bundle: bundle)
        try choose("home.filteredStudy.second.order", "Oldest note first", app: app, bundle: bundle)
        try click("home.filteredStudy.reschedule", app: app, bundle: bundle)
        try fill("home.filteredStudy.again", "60", app: app, bundle: bundle)
        try fill("home.filteredStudy.hard", "120", app: app, bundle: bundle)
        try fill("home.filteredStudy.good", "0", app: app, bundle: bundle)
        capture(app, "filtered-preview-two-overlapping-searches-and-three-delays")
        try click("home.filteredStudy.save", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.filteredStudy.name").waitForNonExistence(timeout: 20))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2))
        let page = deckPage(app).identifier
        XCTAssertEqual(try browseIDs("deck:\"Synthetic Preview\"", count: 2, app: app, bundle: bundle), eligible,
                       "Second term fills from remaining eligible cards; no duplicates or suspended match")
        XCTAssertEqual(try browseIDs("tag:filtered-ui is:suspended", count: 1, app: app, bundle: bundle), suspended)
        let row = try selectDeck("Synthetic Preview", app: app, bundle: bundle)
        let id = try XCTUnwrap(HomeControls.deckID(row.identifier))
        try assertCommandAvailability(filtered: true, app: app, bundle: bundle)
        try click("home.filteredDeck.edit.\(id)", app: app, bundle: bundle)
        for (field, value) in [("again", "60"), ("hard", "120"), ("good", "0"), ("second.limit", "5")] {
            XCTAssertEqual(element(app, "home.filteredStudy." + field).value as? String, value)
        }
        capture(app, "filtered-preview-reopened-definition-retains-edited-delays")
        try click("home.filteredStudy.cancel", app: app, bundle: bundle)
        try click("home.filteredDeck.empty.\(id)", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.rebuild").waitForExistence(timeout: 15))
        XCTAssertFalse(element(app, "home.deck.study").exists)
        XCTAssertTrue(element(app, page).exists)
        XCTAssertEqual(try schedules(eligible, app: app, bundle: bundle), before, "Empty restores both home schedules")
        // Reads in Browse add no undo step: Undo must restore this exact membership.
        try selectDeck("Synthetic Preview", app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("z", modifierFlags: .command)
        XCTAssertEqual(try browseIDs("deck:\"Synthetic Preview\"", count: 2, app: app, bundle: bundle), eligible)
        try selectDeck("Synthetic Preview", app: app, bundle: bundle)
        try click("home.filteredDeck.empty.\(id)", app: app, bundle: bundle)
        try click("home.deck.rebuild", app: app, bundle: bundle)
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2))
        try timedPreviewReturn(app, bundle: bundle, deck: "Synthetic Preview", cards: eligible, before: before,
                               capturePrefix: "filtered-preview")
    }

    func testFinishedNormalDeckCustomPreviewUsesLiteralTagsAndPreservesSchedules() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        try selectDeck("Custom Source", app: app, bundle: bundle)
        try click("home.deck.study", app: app, bundle: bundle)
        try answer("easy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.done").waitForExistence(timeout: 15))
        try click("review.session.customStudy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.available").waitForExistence(timeout: 10))
        capture(app, "custom-study-finished-review-native-entry")
        try click("home.customStudy.cancel", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 10))
        try click("review.session.done", app: app, bundle: bundle)
        // Capture after the regular scheduled answer, before any preview answer.
        let expected = try browseIDs("deck:\"Custom Source\" -deck:\"Custom Source::Literal\"", count: 2, app: app, bundle: bundle)
        let excluded = try browseIDs("deck:\"Custom Source::Literal\"", count: 1, app: app, bundle: bundle)
        let outside = try browseIDs("deck:\"Outside Source\"", count: 1, app: app, bundle: bundle)
        let before = try schedules(expected, app: app, bundle: bundle)
        capture(app, "custom-preview-home-schedule-before")
        try selectDeck("Custom Source", app: app, bundle: bundle)
        // The normal deck page also offers Custom Study when nothing is due.
        try click("home.deck.customStudy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.available").waitForExistence(timeout: 10))
        capture(app, "custom-study-finished-normal-deck-real-availability")
        try choose("home.customStudy.mode", "Study by state or tags", app: app, bundle: bundle)
        try choose("home.customStudy.state", "All cards (preview)", app: app, bundle: bundle)
        try fill("home.customStudy.amount", "10", app: app, bundle: bundle)
        try fill("home.customStudy.includeTags", "custom-ui", app: app, bundle: bundle)
        try fill("home.customStudy.excludeTags", "exclude::tag*", app: app, bundle: bundle)
        capture(app, "custom-study-literal-star-exclusion-with-lookalike-present")
        try click("home.customStudy.save", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 20))
        XCTAssertEqual(try browseIDs("deck:\"Custom Study Session\"", count: 2, app: app, bundle: bundle), expected,
                       "Literal star excluded, wildcard lookalike retained, scope excludes outside deck")
        XCTAssertEqual(try browseIDs("deck:\"Custom Source::Literal\"", count: 1, app: app, bundle: bundle), excluded)
        XCTAssertEqual(try browseIDs("deck:\"Outside Source\"", count: 1, app: app, bundle: bundle), outside)
        try selectDeck("Custom Study Session", app: app, bundle: bundle)
        try timedPreviewReturn(app, bundle: bundle, deck: "Custom Study Session", cards: expected, before: before,
                               capturePrefix: "custom-preview")
    }

    /// Positive Again enters the real timed queue. Reopen the same installed collection
    /// first before the deadline, then past it, using only the supported clock option.
    private func timedPreviewReturn(_ app: XCUIApplication, bundle: String, deck: String,
                                    cards: Set<String>, before: [String: [String: String]], capturePrefix: String) throws {
        try click("home.deck.study", app: app, bundle: bundle)
        try answer("again", app: app, bundle: bundle)
        try answer("easy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.refresh").waitForExistence(timeout: 15), "Timed card is waiting, not finished")
        XCTAssertFalse(element(app, "review.session.reveal").exists)
        capture(app, capturePrefix + "-again-waits-for-configured-delay")
        try click("review.session.close", app: app, bundle: bundle)
        app.terminate()
        try reopen(app, bundle: bundle, now: "2030-01-15T10:00:00+01:00")
        // A queue-4 card remains assigned across process restart, before it is due.
        let waiting = try browseIDs("deck:\"\(deck)\"", count: 1, app: app, bundle: bundle)
        XCTAssertTrue(waiting.isSubset(of: cards))
        try selectDeck(deck, app: app, bundle: bundle)
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 0))
        XCTAssertFalse(element(app, "home.deck.study").exists, "Reopen must honor the positive delay with learn-ahead off")
        capture(app, capturePrefix + "-reopened-before-deadline-keeps-timed-member")
        app.terminate()
        try reopen(app, bundle: bundle, now: "2030-01-15T10:02:00+01:00")
        try selectDeck(deck, app: app, bundle: bundle)
        try click("home.deck.study", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 15))
        capture(app, capturePrefix + "-timed-repeat-reopened")
        try answer("easy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.done").waitForExistence(timeout: 15))
        XCTAssertFalse(element(app, "review.session.customStudy").exists)
        try click("review.session.done", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.rebuild").waitForExistence(timeout: 15))
        XCTAssertEqual(try browseIDs("deck:\"\(deck)\"", count: 0, app: app, bundle: bundle), [])
        XCTAssertEqual(try schedules(cards, app: app, bundle: bundle), before,
                       "Same card ids, home decks, due, interval and memory survive Again → reopen → finish exactly")
        capture(app, capturePrefix + "-returned-home-schedule-exactly-preserved")
    }

    private func answer(_ rating: String, app: XCUIApplication, bundle: String) throws {
        try click("review.session.reveal", app: app, bundle: bundle)
        try click("review.session.rate." + rating, app: app, bundle: bundle)
    }

    private func browseIDs(_ query: String, count: Int, app: XCUIApplication, bundle: String) throws -> Set<String> {
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("b", modifierFlags: [.command, .shift])
        XCTAssertTrue(element(app, "browse.toolbar.mode").waitForExistence(timeout: 15))
        func search(_ text: String) throws {
            let clear = element(app, "browse.filterBar.clear")
            if clear.exists { try click("browse.filterBar.clear", app: app, bundle: bundle) }
            app.typeKey("f", modifierFlags: .command)
            app.typeKey("a", modifierFlags: .command)
            app.typeText(text)
            app.typeKey(.return, modifierFlags: [])
        }
        // Establish a visible empty result before each read, so a preceding
        // equal-sized asynchronous result cannot satisfy the membership check.
        if count == 0 {
            try search("tag:filtered-ui")
            XCTAssertTrue(element(app, "browse.results.table").waitForExistence(timeout: 15))
        } else {
            try search("cid:0")
            XCTAssertTrue(element(app, "browse.empty.noResults").waitForExistence(timeout: 15))
        }
        try search(query)
        if count == 0 {
            XCTAssertTrue(element(app, "browse.empty.noResults").waitForExistence(timeout: 15))
            return []
        }
        let table = element(app, "browse.results.table")
        XCTAssertTrue(wait { table.exists && table.outlineRows.count == count }, query)
        let ids = table.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "browse.table.row."))
        let result = Set(ids.allElementsBoundByIndex.map { String($0.identifier.dropFirst("browse.table.row.".count)) })
        XCTAssertEqual(result.count, count, "Read actual card identities, not just a nonempty table")
        return result
    }

    private func schedules(_ cards: Set<String>, app: XCUIApplication, bundle: String) throws -> [String: [String: String]] {
        var result: [String: [String: String]] = [:]
        for card in cards.sorted() {
            XCTAssertEqual(try browseIDs("cid:" + card, count: 1, app: app, bundle: bundle), [card])
            let row = element(app, "browse.results.table").outlineRows.firstMatch
            row.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
            if !element(app, "cardInfo.row.card_id").exists {
                app.typeKey("i", modifierFlags: [.command, .option])
            }
            XCTAssertTrue(wait { self.element(app, "cardInfo.row.card_id").value as? String == card })
            var values: [String: String] = [:]
            for key in ["card_id", "deck", "due", "interval", "stability", "difficulty", "card_type", "reviews", "lapses"] {
                let field = element(app, "cardInfo.row." + key)
                XCTAssertTrue(field.waitForExistence(timeout: 10), key)
                let value = try XCTUnwrap(field.value as? String, key)
                XCTAssertFalse(value.isEmpty, key)
                values[key] = value
            }
            result[card] = values
        }
        return result
    }

    private func launch() throws -> (XCUIApplication, String) {
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw XCTSkip("Mac UI VM only") }
        continueAfterFailure = false
        let build = try BuildUnderTest.current(), bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Synthetic development container required") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/filtered-study-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            windowSize: "1440x1000", appearance: "light").arguments
        app.launch()
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 25))
        try app.bringToFront(bundleIdentifier: bundle)
        return (app, bundle)
    }

    private func reopen(_ app: XCUIApplication, bundle: String, now: String) throws {
        // No Fixture: preserve the database modified by the preceding answers.
        app.launchArguments = MeguriLaunch(now: now, timeZone: "Europe/Paris", locale: "en_US", animations: false,
            windowSize: "1440x1000", appearance: "light").arguments
        app.launch()
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 25))
        try app.bringToFront(bundleIdentifier: bundle)
    }
    @discardableResult private func selectDeck(_ name: String, app: XCUIApplication, bundle: String) throws -> XCUIElement {
        let rows = app.descendants(matching: .any).matching(NSPredicate(format:
            "(identifier BEGINSWITH %@ OR identifier BEGINSWITH %@) AND label == %@",
            HomeControls.rowPrefix, HomeControls.sidebarPrefix, name))
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
    private func assertCommandAvailability(filtered: Bool, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        app.menuBars.menuBarItems["Deck"].click()
        for action in ["rebuildFilteredDeck", "emptyFilteredDeck"] {
            let command = element(app, "home.commands." + action)
            XCTAssertTrue(command.waitForExistence(timeout: 10))
            XCTAssertEqual(command.isEnabled, filtered, action)
        }
        let custom = element(app, "home.commands.customStudy")
        XCTAssertTrue(custom.waitForExistence(timeout: 10))
        XCTAssertEqual(custom.isEnabled, !filtered)
        capture(app, filtered ? "filtered-deck-enabled-native-commands" : "normal-deck-disabled-filtered-commands")
        app.typeKey(.escape, modifierFlags: [])
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
