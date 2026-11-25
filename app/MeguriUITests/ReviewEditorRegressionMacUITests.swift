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
        // Observe the real Show Answer click's responder before any further scroll or focus change.
        let revealed = try inputEvidence(bundle: bundle)
        XCTAssertEqual(revealed["focus"], "native", "Show Answer left responder \(revealed["firstResponderClass"] ?? "none")")
        app.typeKey("r", modifierFlags: [])
        let answer = try audio(app, bundle: bundle)
        XCTAssertEqual(answer.count, question.count + 1)
        XCTAssertTrue(answer.sides.split(separator: ",").contains("a"), "the existing replay policy queues answer audio")
        // Start within the long answer, with text continuing behind the grades.
        card.scroll(byDeltaX: 0, deltaY: 10000)
        card.scroll(byDeltaX: 0, deltaY: -750)
        capture(app.windows.firstMatch, "review-\(appearance)-long-scroll-over-grades-synthetic")
        // Capture after another scroll too: text must never paint through any button.
        card.scroll(byDeltaX: 0, deltaY: -260)
        capture(app.windows.firstMatch, "review-\(appearance)-long-scroll-over-grades-second-position-synthetic")
    }

    func testAddWindowPickersAddAnotherMainWorkspaceAndDiscard() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        func waitForKeyAddWindow() {
            XCTAssertTrue(wait {
                (try? inputEvidence(bundle: bundle)["addWindowKey"]) == "true"
            }, "Add must be the native key window before keyboard input")
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
        let (app, bundle) = try launch()
        defer { app.terminate() }
        let build = try BuildUnderTest.current()
        XCTAssertEqual(try build.bundleIdentifier, bundle)
        XCTAssertEqual(bundle, build.expectedBundleIdentifier)
        let main = app.windows["main"], add = app.windows["editor.add.window"]
        let deckPage = try selectDeck("Synthetic Summary", app: app)
        let page = main.descendants(matching: .any).matching(identifier: deckPage).firstMatch
        XCTAssertTrue(main.waitForExistence(timeout: 10) && page.exists, app.debugDescription)
        main.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(add.waitForExistence(timeout: 15), app.debugDescription)
        let deck = add.popUpButtons["editor.add.deck"], save = add.buttons["editor.add.save"]
        XCTAssertTrue(wait { deck.exists && (deck.value as? String ?? "").contains("Synthetic Summary") && save.exists })
        XCTAssertFalse(save.isEnabled, "Add starts with a clean draft")
        let collection = try inputEvidence(bundle: bundle)
        let identity = try XCTUnwrap(collection["collectionIdentity"])
        let generation = try XCTUnwrap(collection["collectionGeneration"])
        XCTAssertNotEqual(identity, "none"); XCTAssertNotEqual(generation, "none")
        XCTAssertEqual(collection["workspaceIdentifier"], "main")
        XCTAssertEqual(collection["workspaceMatchesMain"], "true")
        XCTAssertEqual(collection["collectionReady"], "true")
        for keyboard in [false, true] {
            if keyboard {
                let close = app.menuBars.menuBarItems["File"].menuItems.matching(NSPredicate(format: "title == %@", "Close Window"))
                XCTAssertEqual(close.count, 1, "File must contain one Close Window command")
                XCTAssertTrue(close.firstMatch.isEnabled)
                main.typeKey("w", modifierFlags: .command)
            } else {
                let close = main.buttons[XCUIIdentifierCloseWindow]
                XCTAssertTrue(wait { close.exists && close.isEnabled && close.isHittable }, app.debugDescription)
                close.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
            }
            XCTAssertTrue(main.waitForNonExistence(timeout: 10), "Main must close via \(keyboard ? "Cmd-W" : "red button"): \(app.debugDescription)")
            XCTAssertTrue(add.exists && add.sheets.count == 0, app.debugDescription)
            XCTAssertTrue(app.menuBars.menuBarItems["Window"].menuItems.matching(NSPredicate(format: "title == %@", "Meguri")).firstMatch.exists, app.debugDescription)
            // Send the real Dock reopen event to this exact Dev product.
            let configuration = NSWorkspace.OpenConfiguration()
            configuration.activates = true
            let opened = expectation(description: "Dock reopen of the exact Dev app")
            let result = Mutex<(bundle: String?, error: String?)?>(nil)
            NSWorkspace.shared.openApplication(at: build.appBundle, configuration: configuration) { running, error in
                result.withLock { $0 = (running?.bundleIdentifier, error?.localizedDescription) }
                opened.fulfill()
            }
            wait(for: [opened], timeout: 15)
            let outcome = try XCTUnwrap(result.withLock { $0 })
            XCTAssertNil(outcome.error); XCTAssertEqual(outcome.bundle, bundle)
            XCTAssertTrue(main.waitForExistence(timeout: 15), app.debugDescription)
            XCTAssertTrue(page.waitForExistence(timeout: 15), app.debugDescription)
            // Restore native main-window focus only after its real deck page returns.
            let rows = HomeControls.deckRows(app, labelContaining: "Synthetic Summary")
            XCTAssertTrue(wait { HomeControls.hittable(rows) != nil }, app.debugDescription)
            try XCTUnwrap(HomeControls.hittable(rows)).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
            let reopened = try inputEvidence(bundle: bundle)
            XCTAssertEqual(reopened["workspaceIdentifier"], "main")
            XCTAssertEqual(reopened["workspaceMatchesMain"], "true")
            XCTAssertEqual(reopened["workspaceKey"], "true")
            XCTAssertEqual(reopened["collectionIdentity"], identity)
            XCTAssertEqual(reopened["collectionGeneration"], generation)
            XCTAssertEqual(reopened["collectionReady"], "true")
            XCTAssertEqual(reopened["addWindowVisible"], "true")
            XCTAssertTrue(add.exists && add.sheets.count == 0 && save.exists && !save.isEnabled, app.debugDescription)
            XCTAssertTrue((deck.value as? String ?? "").contains("Synthetic Summary"))
        }
        main.typeKey("n", modifierFlags: .command)
        XCTAssertEqual(try inputEvidence(bundle: bundle)["addWindowKey"], "true")
        add.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(add.waitForNonExistence(timeout: 10), app.debugDescription)
    }

    func testMainCloseCancelsCleanEditThenClosesAndQuitStillGuardsDirtyAdd() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        try study("Synthetic Long Audio", app: app, bundle: bundle)
        app.typeKey("e", modifierFlags: .command)
        let edit = app.windows["review.editCurrent.window"]
        XCTAssertTrue(edit.waitForExistence(timeout: 15))
        let main = app.windows.containing(.any, identifier: "review.session.card").firstMatch
        // Target main's native close button, rather than its centre behind Edit.
        let close = main.buttons[XCUIIdentifierCloseWindow]
        XCTAssertTrue(wait { close.exists && close.isEnabled && close.isHittable })
        close.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
        XCTAssertTrue(edit.waitForNonExistence(timeout: 10))
        XCTAssertTrue(main.waitForNonExistence(timeout: 10), "clean Edit cancellation must continue main close")
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15), "the shipped default quits after the last window closes")

        // A new synthetic launch owns the dirty-Quit checks. Window-menu
        // reopening belongs only to a process with a surviving window or integration.
        let (fresh, _) = try launch()
        defer { fresh.terminate() }
        let deckPage = try selectDeck("Synthetic Long Audio", app: fresh)
        let page = element(fresh, deckPage)
        XCTAssertTrue(page.waitForExistence(timeout: 10))
        fresh.typeKey("n", modifierFlags: .command)
        let add = fresh.windows["editor.add.window"]
        XCTAssertTrue(add.waitForExistence(timeout: 15))
        let fields = add.descendants(matching: .any).matching(identifier: "editor.fields").firstMatch.textViews
        XCTAssertTrue(wait { fields.count > 0 })
        fields.element(boundBy: 0).click(); fresh.typeText("Synthetic quit guard")
        fresh.typeKey("q", modifierFlags: .command)
        let keep = add.sheets.buttons["Keep Editing"]
        XCTAssertTrue(keep.waitForExistence(timeout: 10)); keep.click()
        XCTAssertTrue(add.exists && element(fresh, "home.deck.study").exists, "Keep Editing refuses actual termination")
        fresh.typeKey("q", modifierFlags: .command)
        let discard = add.sheets.buttons["Discard"]
        XCTAssertTrue(discard.waitForExistence(timeout: 10)); discard.click()
        XCTAssertTrue(fresh.wait(for: .notRunning, timeout: 15))
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
        XCTAssertTrue(wait { table.outlineRows.count == 1 })
        table.outlineRows.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
        XCTAssertTrue(wait {
            table.outlineRows.element(boundBy: 0).isSelected && fields.count == 2
                && (fields.element(boundBy: 0).value as? String ?? "").contains("Synthetic summary question")
        }, "the summary note must replace the many-field editor before returning")
        filter.click()
        XCTAssertTrue(wait { table.outlineRows.count == 2 })
        table.outlineRows.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
        XCTAssertTrue(wait { table.outlineRows.element(boundBy: 0).isSelected && fields.count == 13 })
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
    @discardableResult
    private func selectDeck(_ name: String, app: XCUIApplication) throws -> String {
        let rows = HomeControls.deckRows(app, labelContaining: name)
        XCTAssertTrue(wait { HomeControls.hittable(rows) != nil })
        let row = try XCTUnwrap(HomeControls.hittable(rows))
        let deckID = try XCTUnwrap(HomeControls.deckID(row.identifier))
        let pageIdentifier = "home.deck.page.\(deckID)"
        row.click()
        let page = element(app, pageIdentifier)
        XCTAssertTrue(page.waitForExistence(timeout: 10), "the selected deck page must finish changing")
        let study = page.buttons["home.deck.study"].firstMatch
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        return pageIdentifier
    }
    @discardableResult
    private func study(_ name: String, app: XCUIApplication, bundle: String) throws -> String {
        let pageIdentifier = try selectDeck(name, app: app)
        let study = element(app, pageIdentifier).buttons["home.deck.study"].firstMatch
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
        let fields = try inputEvidence(bundle: bundle)
        return (try XCTUnwrap(UInt64(fields["requests"] ?? "")), fields["sides"] ?? "", fields["focus"] ?? "none")
    }
    private func inputEvidence(bundle: String) throws -> [String: String] {
        let reply = expectation(description: "Actual review input state")
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
        return try XCTUnwrap(fields)
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
