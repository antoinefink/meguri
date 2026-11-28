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
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, "home.deck.customStudy").exists, "Cards waiting: Study alone (N14.3)")
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
            XCTAssertEqual(try visible("home.filteredStudy." + field, app: app, bundle: bundle).value as? String, value)
        }
        capture(app, "filtered-preview-reopened-definition-retains-edited-delays")
        try click("home.filteredStudy.cancel", app: app, bundle: bundle)
        // Both deck-page keyboard entries reach the filtered definition editor.
        let shortcuts: [XCUIElement.KeyModifierFlags] = [[], [.command, .option]]
        for modifiers in shortcuts {
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeKey("o", modifierFlags: modifiers)
            XCTAssertTrue(element(app, "home.filteredStudy.name").waitForExistence(timeout: 10))
            XCTAssertEqual(element(app, "home.filteredStudy.name").value as? String, "Synthetic Preview")
            try click("home.filteredStudy.cancel", app: app, bundle: bundle)
        }
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
        let source = try selectDeck("Custom Source", app: app, bundle: bundle)
        let sourceID = try XCTUnwrap(HomeControls.deckID(source.identifier))
        try click("home.deck.study", app: app, bundle: bundle)
        try answer("easy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.done").waitForExistence(timeout: 15))
        XCTAssertTrue(element(app, "review.session.summary").exists)
        XCTAssertFalse(element(app, "review.session.customStudy").exists)
        XCTAssertFalse(app.buttons["Custom Study"].exists)
        capture(app, "custom-study-synthetic-finished-review-summary-and-done")
        try click("review.session.done", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.page." + sourceID).waitForExistence(timeout: 15),
                      "Done returns to the same Custom Source deck page")
        XCTAssertTrue(element(app, "review.session.done").waitForNonExistence(timeout: 10))
        try click("home.deck.customStudy", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.available").waitForExistence(timeout: 10))
        try click("home.customStudy.cancel", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, "home.deck.page." + sourceID).waitForExistence(timeout: 15),
                      "Cancel returns to the Custom Source deck page")
        XCTAssertTrue(element(app, "home.deck.customStudy").exists)
        // Capture after the regular scheduled answer, before any preview answer.
        let expected = try browseIDs("deck:\"Custom Source\" -deck:\"Custom Source::Literal\"", count: 2, app: app, bundle: bundle)
        let excluded = try browseIDs("deck:\"Custom Source::Literal\"", count: 1, app: app, bundle: bundle)
        let outside = try browseIDs("deck:\"Outside Source\"", count: 1, app: app, bundle: bundle)
        let before = try schedules(expected, app: app, bundle: bundle)
        capture(app, "custom-preview-home-schedule-before")
        try selectDeck("Custom Source", app: app, bundle: bundle)
        // Reopen the finished normal deck page's Custom Study after cancellation.
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
        @MainActor func search(_ text: String) throws {
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
            // A cleared search removes Card Info's content, not its inspector.
            // Never toggle an already-open pane just because its next card is loading.
            if !element(app, "browse.inspector").exists {
                app.typeKey("i", modifierFlags: [.command, .option])
            }
            let tabs = element(app, "browse.inspector.tabs")
            XCTAssertTrue(tabs.waitForExistence(timeout: 10), "Inspector remains visible between searches")
            let info = tabs.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Info")).firstMatch
            XCTAssertTrue(info.waitForExistence(timeout: 10))
            @MainActor func infoSelected() -> Bool { info.isSelected || (info.value as? Int) == 1 || (info.value as? String) == "1" }
            if !infoSelected() { info.click() }
            XCTAssertTrue(wait { infoSelected() }, "Info tab is selected independently of its loading content")
            XCTAssertTrue(element(app, "cardInfo.row.card_id").waitForExistence(timeout: 15))
            // AppKit need not export a SwiftUI row's accessibilityValue as
            // XCUIElement.value. C8's fresh JSON identifies the selected card.
            let raw = try rawSchedule(card, app: app, bundle: bundle)
            var values: [String: String] = [:]
            for key in ["card_id", "deck", "due", "interval", "stability", "difficulty", "card_type", "reviews", "lapses"] {
                let field = element(app, "cardInfo.row." + key)
                XCTAssertTrue(field.waitForExistence(timeout: 10), key)
                let value = infoRowText(field)
                XCTAssertFalse(value.isEmpty, key)
                values[key] = value
            }
            values["raw.schedule"] = raw
            result[card] = values
        }
        return result
    }

    /// Keep the actual native row text in the before/after comparison, including
    /// exports that put text in a label or child rather than the row's value.
    private func infoRowText(_ row: XCUIElement) -> String {
        ([row] + row.descendants(matching: .staticText).allElementsBoundByIndex)
            .flatMap { [$0.label, $0.value as? String ?? ""] }
            .filter { !$0.isEmpty }
            .joined(separator: "\n")
    }

    /// C8's existing user export reads the stored row. Compare exact schedule
    /// values, including ease, remaining steps and data's FSRS s/d/lrt, without
    /// modification/sync stamps or preview review-log rows that legitimately change.
    private func rawSchedule(_ card: String, app: XCUIApplication, bundle: String) throws -> String {
        let copy = element(app, "cardInfo.copyDebug")
        let scroll = app.scrollViews.matching(identifier: "cardInfo.card").firstMatch
        XCTAssertTrue(scroll.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        XCTAssertTrue(bringIntoView(copy, in: scroll, app: app), "Copy Debug Info is visible in its scroll view")
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        defer { pasteboard.clearContents() }
        let previousChange = pasteboard.changeCount
        try click("cardInfo.copyDebug", app: app, bundle: bundle)
        XCTAssertTrue(wait {
            guard pasteboard.changeCount != previousChange,
                  let json = pasteboard.string(forType: .string),
                  let dump = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
                  let row = dump["card"] as? [String: Any] else { return false }
            return (row["id"] as? NSNumber)?.stringValue == card
        }, "Wait for this selected card's asynchronous debug export: " + card)
        let json = try XCTUnwrap(pasteboard.string(forType: .string))
        let dump = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
        let row = try XCTUnwrap(dump["card"] as? [String: Any])
        XCTAssertEqual((row["id"] as? NSNumber)?.stringValue, card, "Debug export belongs to the selected card")
        var schedule: [String: Any] = [:]
        for key in ["id", "nid", "did", "ord", "type", "queue", "due", "ivl", "factor", "reps", "lapses", "left", "odue", "odid", "data"] {
            schedule[key] = try XCTUnwrap(row[key], "Stored schedule column " + key)
        }
        // Both reads are made at home, before assignment or after release.
        XCTAssertEqual((row["odid"] as? NSNumber)?.int64Value, 0)
        XCTAssertEqual((row["odue"] as? NSNumber)?.int64Value, 0)
        let data = try XCTUnwrap(row["data"] as? String)
        let memory = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(data.utf8)) as? [String: Any])
        for key in ["s", "d", "lrt"] {
            XCTAssertNotNil(memory[key], "Synthetic reviewed cards exercise stored " + key)
        }
        return String(decoding: try JSONSerialization.data(withJSONObject: schedule, options: [.sortedKeys]), as: UTF8.self)
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
        // Browse owns a different sidebar. Restore the native Home/Today section
        // before asking its deck tree for a row, including after schedule reads.
        try click("shell.root.sidebar.home", app: app, bundle: bundle)
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
        let target = try visible(id, app: app, bundle: bundle)
        target.click()
    }
    private func visible(_ id: String, app: XCUIApplication, bundle: String) throws -> XCUIElement {
        try app.bringToFront(bundleIdentifier: bundle)
        let target = element(app, id)
        let filtered = element(app, "home.filteredStudy.sheet")
        let custom = element(app, "home.customStudy.sheet")
        let form = filtered.exists ? filtered : custom
        let prefix = filtered.exists ? "home.filteredStudy." : "home.customStudy."
        // Footer buttons are outside the Form; inputs may not have an AX node
        // until scrolled into view, so do not require target.exists to scroll.
        if form.exists, id.hasPrefix(prefix), ![prefix + "cancel", prefix + "save"].contains(id) {
            let scroll = form.scrollViews.firstMatch
            XCTAssertTrue(scroll.waitForExistence(timeout: 10), "Native study form scroll view")
            XCTAssertTrue(bringIntoView(target, in: scroll, app: app), id + " is inside the form viewport")
        }
        XCTAssertTrue(wait { target.exists && target.isEnabled && target.isHittable }, id)
        return target
    }
    private func bringIntoView(_ target: XCUIElement, in scroll: XCUIElement, app: XCUIApplication) -> Bool {
        for _ in 0..<20 {
            let viewport = scroll.frame.intersection(app.windows.firstMatch.frame)
            guard !viewport.isEmpty else { return false }
            let frame = target.exists ? target.frame : .zero
            if !frame.isEmpty, viewport.contains(frame), target.isHittable { return true }
            // isHittable alone can be true for clipped grouped-form controls.
            // Follow the target in either direction, including on a reopened form.
            let reach = viewport.height / 3
            let delta = frame.isEmpty ? -reach : max(-reach, min(reach, viewport.midY - frame.midY))
            scroll.scroll(byDeltaX: 0, deltaY: delta)
        }
        return false
    }
    private func fill(_ id: String, _ value: String, app: XCUIApplication, bundle: String) throws {
        try click(id, app: app, bundle: bundle)
        app.typeKey("a", modifierFlags: .command); app.typeText(value)
    }
    private func choose(_ id: String, _ title: String, app: XCUIApplication, bundle: String) throws {
        try click(id, app: app, bundle: bundle)
        let item = menuItem(title, app: app)
        XCTAssertTrue(item.waitForExistence(timeout: 10))
        XCTAssertTrue(item.isEnabled, title)
        try app.bringToFront(bundleIdentifier: bundle)
        item.click()
    }
    private func assertCommandAvailability(filtered: Bool, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        app.menuBars.menuBarItems["Deck"].click()
        // SwiftUI command identifiers are often absent from AppKit's menu tree.
        for title in ["Rebuild", "Empty"] {
            let command = menuItem(title, app: app)
            XCTAssertTrue(command.waitForExistence(timeout: 10))
            XCTAssertEqual(command.isEnabled, filtered, title)
        }
        let options = menuItem(filtered ? "Edit Filtered Deck" : "Deck Options", app: app)
        XCTAssertTrue(options.waitForExistence(timeout: 10))
        XCTAssertTrue(options.isEnabled)
        let custom = menuItem("Custom Study", app: app)
        XCTAssertTrue(custom.waitForExistence(timeout: 10))
        XCTAssertEqual(custom.isEnabled, !filtered)
        // Both labels without an ellipsis (owner, 2026-11-27); ⇧⌘F is the menu bar's alone.
        let create = menuItem("Create Filtered Deck", app: app)
        XCTAssertTrue(create.waitForExistence(timeout: 10))
        XCTAssertTrue(create.isEnabled)
        capture(app, filtered ? "filtered-deck-enabled-native-commands" : "normal-deck-disabled-filtered-commands")
        app.typeKey(.escape, modifierFlags: [])
    }
    private func menuItem(_ title: String, app: XCUIApplication) -> XCUIElement {
        app.menuItems.matching(NSPredicate(format: "title == %@ OR label == %@", title, title)).firstMatch
    }
    private func deckPage(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
    }
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func wait(_ condition: @MainActor () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(15)
        while !condition() {
            guard Date() < deadline else { return false }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return true
    }
    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
#endif
