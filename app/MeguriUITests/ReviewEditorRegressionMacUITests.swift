#if os(macOS)
import AppKit
import Foundation
import Synchronization
import XCTest

/// Production app, collection, native input and field runtime. MG detects and
/// stages the authored review-editor-1 recipe named below. VM only.
@MainActor final class ReviewEditorRegressionMacUITests: XCTestCase {
    func testSummarySpaceReturnAndEnterUseDoneOnce() throws {
        for key in [" ", "\r", "\u{3}"] {
            let (app, bundle) = try launch()
            defer { app.terminate() }
            try study("Synthetic Summary", app: app, bundle: bundle)
            app.typeKey(" ", modifierFlags: [])
            XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
            app.typeKey("\r", modifierFlags: [])
            let done = element(app, "review.session.done")
            XCTAssertTrue(done.waitForExistence(timeout: 15), "the final grade leaves the summary visible")
            XCTAssertTrue(element(app, "review.session.summary").label.hasPrefix("1"))
            inject("repeat-space", bundle: bundle)
            inject("repeat-enter", bundle: bundle)
            inject("stale-enter", bundle: bundle)
            // Snapshot is a round trip after the injected native events.
            _ = try audio(app, bundle: bundle)
            XCTAssertTrue(done.exists, "held and pre-presentation key events cannot dismiss the summary")
            capture(app.windows.firstMatch, "review-summary-one-card-before-done-\(key == " " ? "space" : key == "\r" ? "return" : "enter")")
            app.typeKey(key, modifierFlags: [])
            XCTAssertTrue(done.waitForNonExistence(timeout: 10))
            XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10), "Done returns to the same deck")
            app.terminate()
        }
    }

    func testReplayOwnershipAndDarkLongScrollOverNativeButtons() throws { try replayAndScroll(appearance: "dark") }
    func testReplayOwnershipAndLightLongScrollOverNativeButtons() throws { try replayAndScroll(appearance: "light") }

    private func replayAndScroll(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        defer { app.terminate() }
        try study("Synthetic Long Audio", app: app, bundle: bundle)
        let card = element(app, "review.session.card")
        let before = try audio(app, bundle: bundle)
        app.typeKey("r", modifierFlags: [])
        let question = try audio(app, bundle: bundle)
        XCTAssertEqual(question.count, before.count + 1)
        XCTAssertEqual(question.sides, "q", "the existing controller queues the question audio")
        inject("repeat-r", bundle: bundle)
        let modifiers: [XCUIKeyModifierFlags] = [.command, .control, .option, .shift]
        for flags in modifiers { app.typeKey("r", modifierFlags: flags) }
        XCTAssertEqual(try audio(app, bundle: bundle).count, question.count)
        capture(app.windows.firstMatch, "review-replay-question-\(appearance)-synthetic")

        // Scroll content behind Show Answer without changing its native layout.
        card.scroll(byDeltaX: 0, deltaY: -750)
        capture(app.windows.firstMatch, "review-\(appearance)-long-scroll-over-reveal-synthetic")
        for _ in 0..<8 where !app.textFields["Synthetic typed answer"].isHittable {
            card.scroll(byDeltaX: 0, deltaY: -700)
        }
        let input = app.textFields["Synthetic typed answer"]
        XCTAssertTrue(input.waitForExistence(timeout: 10) && input.isHittable)
        input.click(); app.typeText("r")
        XCTAssertEqual(try audio(app, bundle: bundle).count, question.count, "typed answer owns r")
        XCTAssertTrue(element(app, "review.session.reveal").exists)
        element(app, "review.session.reveal").click()
        XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
        card.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.25)).click()
        app.typeKey("r", modifierFlags: [])
        let answer = try audio(app, bundle: bundle)
        XCTAssertEqual(answer.count, question.count + 1)
        XCTAssertTrue(answer.sides.split(separator: ",").contains("a"), "the existing replay policy queues answer audio")
        card.scroll(byDeltaX: 0, deltaY: 750)
        capture(app.windows.firstMatch, "review-\(appearance)-long-scroll-over-grades-synthetic")
        // Capture after another scroll too: text must never paint through any button.
        card.scroll(byDeltaX: 0, deltaY: -260)
        capture(app.windows.firstMatch, "review-\(appearance)-long-scroll-over-grades-second-position-synthetic")
    }

    func testAddWindowPickersAddAnotherMainWorkspaceAndDiscard() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        app.typeKey("n", modifierFlags: .command)
        var window = app.windows["editor.add.window"]
        XCTAssertTrue(window.waitForExistence(timeout: 15))
        let deck = window.popUpButtons["editor.add.deck"], type = window.popUpButtons["editor.add.type"]
        XCTAssertTrue(deck.waitForExistence(timeout: 10) && type.exists)
        type.click(); app.menuItems["Meguri Basic"].click()
        deck.click(); app.menuItems["Synthetic Summary"].click()
        let add = window.buttons["editor.add.save"]
        XCTAssertFalse(add.isEnabled, "blank input cannot add a note")
        capture(window, "add-note-native-window-with-type-and-deck-pickers-synthetic")
        let fields = window.descendants(matching: .any).matching(identifier: "editor.fields").firstMatch.textViews
        XCTAssertTrue(wait { fields.count == 2 })
        fields.element(boundBy: 0).click(); app.typeText("Synthetic added prompt")
        fields.element(boundBy: 1).click(); app.typeText("Synthetic added answer")
        XCTAssertTrue(wait { add.isEnabled }); add.click()
        XCTAssertTrue(wait { window.exists && !add.isEnabled })
        XCTAssertTrue(element(app, "editor.add.result").waitForExistence(timeout: 10), "Add another keeps the window and clears the fields")

        // Its main window remains independently usable while Add stays open.
        element(app, "shell.root.sidebar.browse").click()
        XCTAssertTrue(element(app, "browse.results.table").waitForExistence(timeout: 15))
        XCTAssertTrue(window.exists)
        app.typeKey("f", modifierFlags: .command)
        XCTAssertTrue(window.exists, "Find in the main workspace leaves Add open")
        window.click()
        app.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(window.waitForNonExistence(timeout: 10), "an unchanged post-add draft closes without discard")

        app.typeKey("n", modifierFlags: .command)
        window = app.windows["editor.add.window"]
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        app.typeText("Synthetic unsaved draft")
        app.typeKey("w", modifierFlags: .command)
        let discard = window.sheets.buttons["Discard"], keep = window.sheets.buttons["Keep Editing"]
        XCTAssertTrue(discard.waitForExistence(timeout: 10) && keep.exists)
        keep.click(); XCTAssertTrue(window.exists)
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(discard.waitForExistence(timeout: 10)); discard.click()
        XCTAssertTrue(window.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, "browse.results.table").exists)
    }

    func testManyFieldMultiCardBrowseInspectorEditsFieldTwelve() throws {
        let (app, _) = try launch(appearance: "dark")
        defer { app.terminate() }
        element(app, "shell.root.sidebar.browse").click()
        let filter = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "browse.sidebar.deck.", "Synthetic Many Fields")).firstMatch
        XCTAssertTrue(filter.waitForExistence(timeout: 15)); filter.click()
        let table = app.outlines["browse.results.table"]
        XCTAssertTrue(wait { table.outlineRows.count == 2 })
        table.outlineRows.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
        let fields = element(app, "editor.fields").textViews
        XCTAssertTrue(wait { fields.count == 13 }, "13 nonempty fields, with the last empty field behind its disclosure")
        XCTAssertTrue(element(app, "browse.inspector.preview").waitForExistence(timeout: 15))
        XCTAssertFalse(element(app, "browse.inspector.error").exists)
        capture(app.windows.firstMatch, "browse-many-field-audio-working-inspector-preview-synthetic")
        let edited = fields.element(boundBy: 11)
        for _ in 0..<8 where !edited.isHittable { edited.scroll(byDeltaX: 0, deltaY: -450) }
        XCTAssertTrue(edited.isHittable)
        edited.click(); app.typeKey("a", modifierFlags: .command); app.typeText("Synthetic field twelve saved")
        app.typeKey(.tab, modifierFlags: [])
        table.outlineRows.element(boundBy: 1).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
        XCTAssertTrue(wait { (fields.element(boundBy: 11).value as? String ?? "").contains("Synthetic field twelve saved") })
        XCTAssertTrue((fields.element(boundBy: 0).value as? String ?? "").contains("Synthetic many-field prompt"))
        capture(app.windows.firstMatch, "browse-many-field-audio-edited-field-twelve-synthetic")
    }

    private func launch(appearance: String = "light") throws -> (XCUIApplication, String) {
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw XCTSkip("Mac UI VM only") }
        continueAfterFailure = false
        let build = try BuildUnderTest.current(), bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Isolated synthetic development container required") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/review-editor-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            windowSize: "1440x900", appearance: appearance).arguments
        app.launch(); try app.bringToFront(bundleIdentifier: bundle)
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 25))
        return (app, bundle)
    }
    private func study(_ name: String, app: XCUIApplication, bundle: String) throws {
        let rows = HomeControls.deckRows(app, labelContaining: name)
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 15))
        (HomeControls.hittable(rows) ?? rows.firstMatch).click()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10)); study.click()
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        try app.bringToFront(bundleIdentifier: bundle)
    }
    private func inject(_ command: String, bundle: String) {
        DistributedNotificationCenter.default().postNotificationName(.init(bundle + ".testhooks.reviewInput"),
            object: command, userInfo: nil, deliverImmediately: true)
    }
    private func audio(_ app: XCUIApplication, bundle: String) throws -> (count: UInt64, sides: String) {
        let reply = expectation(description: "Actual review audio queue")
        let received = Mutex<[String: String]?>(nil)
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: .init(bundle + ".testhooks.reviewInputReply"), object: nil, queue: .main) { note in
            guard let text = note.object as? String, let data = text.data(using: .utf8),
                  let value = try? JSONSerialization.jsonObject(with: data) as? [String: String] else { return }
            received.withLock { $0 = value }; reply.fulfill()
        }
        defer { center.removeObserver(observer) }
        inject("snapshot", bundle: bundle)
        wait(for: [reply], timeout: 10)
        let fields = received.withLock { $0 }
        return (try XCTUnwrap(UInt64(fields?["requests"] ?? "")), fields?["sides"] ?? "")
    }
    private func capture(_ window: XCUIElement, _ name: String) {
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func wait(_ condition: () -> Bool) -> Bool {
        let end = Date().addingTimeInterval(15)
        while Date() < end { if condition() { return true }; RunLoop.current.run(until: Date().addingTimeInterval(0.1)) }
        return condition()
    }
}
#endif
