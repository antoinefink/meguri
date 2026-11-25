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
            let deckPage = try study("Synthetic Summary", app: app, bundle: bundle)
            try clickReviewButton("review.session.reveal", app: app, bundle: bundle)
            XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
            try clickReviewButton("review.session.rate.easy", app: app, bundle: bundle)
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
            XCTAssertTrue(element(app, deckPage).waitForExistence(timeout: 10), "Done returns to the same deck")
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
        let modifiers: [XCUIElement.KeyModifierFlags] = [.command, .control, .option, .shift]
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
        app.typeKey(.tab, modifierFlags: [])
        try clickReviewButton("review.session.reveal", app: app, bundle: bundle)
        XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
        let good = app.buttons["review.session.rate.good"]
        XCTAssertTrue(wait { good.exists && good.isEnabled && good.isHittable })
        // Start within the long answer, with text continuing behind the grades.
        // Capture before changing native focus.
        card.scroll(byDeltaX: 0, deltaY: 10000)
        card.scroll(byDeltaX: 0, deltaY: -750)
        capture(app.windows.firstMatch, "review-\(appearance)-long-scroll-over-grades-before-focus-synthetic")
        // Focus the actual native grade button after reveal, without touching WebKit.
        inject("focus-review-control", bundle: bundle)
        XCTAssertEqual(try audio(app, bundle: bundle).focus, "native")
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
        let running = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
            .first { !$0.isTerminated })
        let application = AXUIElementCreateApplication(running.processIdentifier)
        func waitForKeyAddWindow() {
            XCTAssertTrue(wait {
                var focused: CFTypeRef?
                guard AXUIElementCopyAttributeValue(application, kAXFocusedWindowAttribute as CFString, &focused) == .success,
                      let focused, CFGetTypeID(focused) == AXUIElementGetTypeID() else { return false }
                var identifier: CFTypeRef?
                return AXUIElementCopyAttributeValue(focused as! AXUIElement, kAXIdentifierAttribute as CFString, &identifier) == .success
                    && identifier as? String == "editor.add.window"
            }, "Add must be the native focused window before keyboard input; AX trusted=\(AXIsProcessTrusted())")
            XCTAssertEqual(app.windows.matching(identifier: "editor.add.window").count, 1, "New Note reuses the single Add window")
        }
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
        app.typeKey("n", modifierFlags: .command)
        waitForKeyAddWindow()
        app.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(window.waitForNonExistence(timeout: 10), "an unchanged post-add draft closes without discard")

        app.typeKey("n", modifierFlags: .command)
        window = app.windows["editor.add.window"]
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        waitForKeyAddWindow()
        XCTAssertTrue(wait { fields.count == 2 && fields.element(boundBy: 0).isEnabled && fields.element(boundBy: 0).isHittable })
        fields.element(boundBy: 0).click(); app.typeText("Synthetic unsaved draft")
        app.typeKey("w", modifierFlags: .command)
        let discard = window.sheets.buttons["Discard"], keep = window.sheets.buttons["Keep Editing"]
        XCTAssertTrue(discard.waitForExistence(timeout: 10) && keep.exists)
        keep.click(); XCTAssertTrue(window.exists)
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(discard.waitForExistence(timeout: 10)); discard.click()
        XCTAssertTrue(window.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, "browse.results.table").exists)
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        waitForKeyAddWindow()
        XCTAssertTrue(wait { fields.count == 2 })
        XCTAssertFalse((fields.element(boundBy: 0).value as? String ?? "").contains("Synthetic unsaved draft"))
        XCTAssertFalse(window.buttons["editor.add.save"].isEnabled, "discarded input must not return on reopen")
        XCTAssertTrue(wait { fields.element(boundBy: 0).isEnabled && fields.element(boundBy: 0).isHittable })
        fields.element(boundBy: 0).click()
        app.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(window.waitForNonExistence(timeout: 10))
    }

    func testNewNoteReentryUsesTheSelectedDeckAndKeepsTheDirtyDraft() throws {
        let (app, _) = try launch()
        defer { app.terminate() }
        try selectDeck("Synthetic Summary", app: app)
        app.typeKey("n", modifierFlags: .command)
        let window = app.windows["editor.add.window"]
        XCTAssertTrue(window.waitForExistence(timeout: 15))
        let deck = window.popUpButtons["editor.add.deck"]
        XCTAssertTrue(wait { (deck.value as? String ?? "").contains("Synthetic Summary") })
        window.popUpButtons["editor.add.type"].click(); app.menuItems["Meguri Basic"].click()
        let fields = window.descendants(matching: .any).matching(identifier: "editor.fields").firstMatch.textViews
        XCTAssertTrue(wait { fields.count == 2 })
        fields.element(boundBy: 0).click(); app.typeText("Synthetic retained draft")
        fields.element(boundBy: 1).click(); app.typeText("Synthetic retained answer")
        try selectDeck("Synthetic Long Audio", app: app)
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(wait { (deck.value as? String ?? "").contains("Synthetic Long Audio") })
        XCTAssertEqual(app.windows.matching(identifier: "editor.add.window").count, 1)
        XCTAssertTrue((fields.element(boundBy: 0).value as? String ?? "").contains("Synthetic retained draft"))
        XCTAssertTrue((fields.element(boundBy: 1).value as? String ?? "").contains("Synthetic retained answer"))
        app.typeKey("w", modifierFlags: .command)
        let discard = window.sheets.buttons["Discard"]
        XCTAssertTrue(discard.waitForExistence(timeout: 10)); discard.click()
        XCTAssertTrue(window.waitForNonExistence(timeout: 10))
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        XCTAssertTrue(wait { (deck.value as? String ?? "").contains("Synthetic Long Audio") })
        XCTAssertTrue(wait { fields.count == 2 })
        XCTAssertFalse((fields.element(boundBy: 0).value as? String ?? "").contains("Synthetic retained draft"))
        XCTAssertFalse(window.buttons["editor.add.save"].isEnabled)
        app.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(window.waitForNonExistence(timeout: 10))
    }

    func testMainCloseLeavesCleanAddOpenAndReopensTheSameWorkspace() throws {
        let (app, _) = try launch()
        defer { app.terminate() }
        try selectDeck("Synthetic Summary", app: app)
        let main = app.windows.containing(.any, identifier: "shell.root.sidebar.browse").firstMatch
        app.typeKey("n", modifierFlags: .command)
        let add = app.windows["editor.add.window"]
        XCTAssertTrue(add.waitForExistence(timeout: 15))
        try selectDeck("Synthetic Summary", app: app)
        app.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(main.waitForNonExistence(timeout: 10), "the main window must continue its own close")
        XCTAssertTrue(add.exists && add.sheets.count == 0, "main close must not cancel Add")
        app.menuBars.menuBarItems["Window"].click(); app.menuItems["Meguri"].click()
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10))
        XCTAssertTrue(add.exists, "reopening main preserves the collection-owned Add window")
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(wait { (add.popUpButtons["editor.add.deck"].value as? String ?? "").contains("Synthetic Summary") })
        app.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(add.waitForNonExistence(timeout: 10))
    }

    func testMainCloseCancelsCleanEditThenClosesAndQuitStillGuardsDirtyAdd() throws {
        let (app, bundle) = try launch(extra: ["-MeguriIntegrations", "on"])
        defer { app.terminate() }
        try study("Synthetic Long Audio", app: app, bundle: bundle)
        app.typeKey("e", modifierFlags: .command)
        let edit = app.windows["review.editCurrent.window"]
        XCTAssertTrue(edit.waitForExistence(timeout: 15))
        let main = app.windows.containing(.any, identifier: "review.session.card").firstMatch
        main.click(); app.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(edit.waitForNonExistence(timeout: 10))
        XCTAssertTrue(main.waitForNonExistence(timeout: 10), "clean Edit cancellation must continue main close")
        app.menuBars.menuBarItems["Window"].click(); app.menuItems["Meguri"].click()
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 15))
        element(app, "review.session.close").click()
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10))
        app.typeKey("n", modifierFlags: .command)
        let add = app.windows["editor.add.window"]
        XCTAssertTrue(add.waitForExistence(timeout: 15))
        let fields = add.descendants(matching: .any).matching(identifier: "editor.fields").firstMatch.textViews
        XCTAssertTrue(wait { fields.count > 0 })
        fields.element(boundBy: 0).click(); app.typeText("Synthetic quit guard")
        app.typeKey("q", modifierFlags: .command)
        let keep = add.sheets.buttons["Keep Editing"]
        XCTAssertTrue(keep.waitForExistence(timeout: 10)); keep.click()
        XCTAssertTrue(add.exists && element(app, "home.deck.study").exists, "Keep Editing refuses actual termination")
        app.typeKey("q", modifierFlags: .command)
        let discard = add.sheets.buttons["Discard"]
        XCTAssertTrue(discard.waitForExistence(timeout: 10)); discard.click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
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
        let fieldScroll = app.scrollViews["browse.inspector.fields.scroll"]
        XCTAssertTrue(fieldScroll.waitForExistence(timeout: 10) && fieldScroll.isHittable)
        for _ in 0..<8 where !edited.isHittable { fieldScroll.scroll(byDeltaX: 0, deltaY: -450) }
        XCTAssertTrue(edited.isHittable)
        edited.click(); app.typeKey("a", modifierFlags: .command); app.typeText("Synthetic field twelve saved")
        app.typeKey(.tab, modifierFlags: [])
        table.outlineRows.element(boundBy: 1).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
        XCTAssertTrue(wait { table.outlineRows.element(boundBy: 1).isSelected && fields.count == 13 })
        // A sibling card shares the editor. Leave this note and return so the
        // saved value must survive a new editor load, without its old focus.
        let otherDeck = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "browse.sidebar.deck.", "Synthetic Summary")).firstMatch
        XCTAssertTrue(otherDeck.waitForExistence(timeout: 10)); otherDeck.click()
        XCTAssertTrue(wait { table.outlineRows.count == 1 && !element(app, "editor.fields").exists })
        filter.click()
        XCTAssertTrue(wait { table.outlineRows.count == 2 })
        table.outlineRows.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
        XCTAssertTrue(wait { fields.count == 13 })
        for _ in 0..<8 where !edited.isHittable { fieldScroll.scroll(byDeltaX: 0, deltaY: -450) }
        XCTAssertTrue(edited.isHittable)
        XCTAssertTrue(wait { (fields.element(boundBy: 11).value as? String ?? "").contains("Synthetic field twelve saved") })
        XCTAssertTrue((fields.element(boundBy: 0).value as? String ?? "").contains("Synthetic many-field prompt"))
        capture(app.windows.firstMatch, "browse-many-field-audio-edited-field-twelve-synthetic")
    }

    private func launch(appearance: String = "light", extra: [String] = []) throws -> (XCUIApplication, String) {
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw XCTSkip("Mac UI VM only") }
        continueAfterFailure = false
        let build = try BuildUnderTest.current(), bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Isolated synthetic development container required") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/review-editor-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            windowSize: "1440x900", appearance: appearance, extra: extra).arguments
        app.launch(); try app.bringToFront(bundleIdentifier: bundle)
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 25))
        return (app, bundle)
    }
    private func selectDeck(_ name: String, app: XCUIApplication) throws {
        let rows = HomeControls.deckRows(app, labelContaining: name)
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 15))
        (HomeControls.hittable(rows) ?? rows.firstMatch).click()
        XCTAssertTrue(wait { element(app, "home.deck.title").label == name }, "the selected deck page must finish changing")
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
    }
    @discardableResult
    private func study(_ name: String, app: XCUIApplication, bundle: String) throws -> String {
        try selectDeck(name, app: app)
        let page = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        let pageIdentifier = page.identifier
        let study = element(app, "home.deck.study")
        XCTAssertTrue(wait { study.exists && study.isEnabled && study.isHittable }); study.clickOrTap()
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        try app.bringToFront(bundleIdentifier: bundle)
        return pageIdentifier
    }
    private func clickReviewButton(_ identifier: String, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        let button = app.buttons[identifier]
        XCTAssertTrue(wait { button.exists && button.isEnabled && button.isHittable }, "\(identifier) must be ready for a visible click")
        button.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).clickOrTap()
    }
    private func inject(_ command: String, bundle: String) {
        DistributedNotificationCenter.default().postNotificationName(.init(bundle + ".testhooks.reviewInput"),
            object: command, userInfo: nil, deliverImmediately: true)
    }
    private func audio(_ app: XCUIApplication, bundle: String) throws -> (count: UInt64, sides: String, focus: String) {
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
        XCTAssertEqual(fields?["live"], "true", "the snapshot must read the live collection and audio controller")
        return (try XCTUnwrap(UInt64(fields?["requests"] ?? "")), fields?["sides"] ?? "", fields?["focus"] ?? "none")
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
