import Foundation
import XCTest
#if os(macOS)
import AppKit
import ApplicationServices

/// Deck Options' keys on the Mac (N20.9, `sched/deck-options` O15.3): ⌥⌘O
/// opens the page on the shown deck, ⌘↩ saves as ⌘S does, and Save returns
/// to the deck page with the value kept.
@MainActor
final class DeckOptionsUITests: XCTestCase {
    func testOptionCommandOOpensThePageAndCommandReturnSaves() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundle = try build.bundleIdentifier
        guard bundle.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false).arguments
        app.launch()
        defer { app.terminate() }
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 20))

        let row = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "home.sidebar.deck.")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        row.clickOrTap()
        let page = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10))

        // Deck › Deck Options… (⌥⌘O).
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .option])
        let save = element(app, "deckOptions.options.save")
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        let field = element(app, "deckOptions.options.option.newPerDay")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        let original = (field.value as? String) ?? ""
        let changed = original == "37" ? "38" : "37"

        try app.bringToFront(bundleIdentifier: bundle)
        field.clickOrTap()
        app.typeKey("a", modifierFlags: .command)
        app.typeText(changed)
        XCTAssertTrue(waitForValue(field, changed))
        // Typed text counts as a change before it is committed (O15.3).
        XCTAssertTrue(waitForEnabled(save))

        // ⌘↩ saves: the page returns to the deck page.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("\r", modifierFlags: .command)
        XCTAssertTrue(save.waitForNonExistence(timeout: 20), "⌘↩ saves and closes the page")
        XCTAssertTrue(page.waitForExistence(timeout: 10))

        // The saved value is what the page shows when it opens again.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: [.command, .option])
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        XCTAssertTrue(waitForValue(field, changed))
        XCTAssertFalse(save.isEnabled, "A page that opens has nothing to save")
    }

    /// A value that fails validation, then its correction typed in the same
    /// field: ⌘S and then ⌘↩ save the correction (O15.3, CodeRabbit on #16).
    func testBothSaveShortcutsSaveACorrectionTypedAfterAnError() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        let save = element(app, "deckOptions.options.save")
        let field = element(app, "deckOptions.options.option.desiredRetention")
        for (shortcut, corrected) in [("s", "85%"), ("\r", "86%")] {
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeKey("o", modifierFlags: [.command, .option])
            XCTAssertTrue(save.waitForExistence(timeout: 20))
            try app.bringToFront(bundleIdentifier: bundle)
            field.clickOrTap()
            app.typeKey("a", modifierFlags: .command)
            app.typeText("50%\t")
            let issue = app.descendants(matching: .any).matching(NSPredicate(
                format: "identifier BEGINSWITH %@", "deckOptions.options.issue.desiredRetention")).firstMatch
            XCTAssertTrue(issue.waitForExistence(timeout: 10), "the out-of-range value shows its error")
            try app.bringToFront(bundleIdentifier: bundle)
            field.clickOrTap()
            app.typeKey("a", modifierFlags: .command)
            app.typeText(corrected)
            XCTAssertTrue(waitForEnabled(save), "a typed correction enables Save")
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeKey(shortcut, modifierFlags: .command)
            XCTAssertTrue(save.waitForNonExistence(timeout: 20), "\(shortcut == "s" ? "⌘S" : "⌘↩") saves the correction")
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeKey("o", modifierFlags: [.command, .option])
            XCTAssertTrue(save.waitForExistence(timeout: 20))
            XCTAssertTrue(waitForValue(field, corrected))
            // The next round's ⌥⌘O leaves this open, unchanged page as it is.
        }
    }

    func testExpandedDisplayOrderAndNewPresetThroughNativeDeckMenu() throws {
        let (app, bundle) = try launch()
        defer { app.terminate() }
        try app.bringToFront(bundleIdentifier: bundle)
        let deckMenu = app.menuBars.menuBarItems["Deck"]
        deckMenu.click()
        let options = deckMenu.menuItems["Deck Options…"]
        XCTAssertTrue(options.waitForExistence(timeout: 10) && options.isEnabled)
        options.click()
        let save = element(app, "deckOptions.options.save")
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        let gather = element(app, "deckOptions.options.order.newGatherPriority")
        XCTAssertFalse(gather.exists, "Advanced is collapsed whenever the page opens")
        try app.bringToFront(bundleIdentifier: bundle)
        let advanced = app.disclosureTriangles["Advanced"].firstMatch
        XCTAssertTrue(advanced.waitForExistence(timeout: 10))
        advanced.click()
        XCTAssertTrue(gather.waitForExistence(timeout: 10))
        for row in ["newSortOrder", "newMix", "interdayLearningMix", "reviewOrder"] {
            XCTAssertTrue(element(app, "deckOptions.options.order." + row).exists)
        }
        XCTAssertFalse(save.isEnabled, "Expanding the disclosure never changes the draft")
        capture(app, "deck-options-expanded")
        // This is the native menu's spec order, including the nonsequential value 5.
        gather.clickOrTap()
        let gatherLabels = ["Deck", "Deck, then random notes", "Ascending position", "Descending position", "Random notes", "Random cards"]
        let menu = app.menus.firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        let titles = menu.menuItems.allElementsBoundByIndex.map(\.title).filter { !$0.isEmpty }
        XCTAssertEqual(titles, gatherLabels)
        app.menuItems["Deck, then random notes"].firstMatch.click()
        XCTAssertTrue(waitForEnabled(save))
        element(app, "deckOptions.options.newPreset").clickOrTap()
        let name = element(app, "deckOptions.newPreset.name")
        let create = element(app, "deckOptions.newPreset.create")
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        XCTAssertFalse(create.isEnabled)
        name.clickOrTap(); app.typeText("   ")
        XCTAssertFalse(create.isEnabled, "Whitespace is not a name")
        app.typeKey("a", modifierFlags: .command); app.typeText("  Native Options  ")
        XCTAssertTrue(waitForEnabled(create))
        capture(app, "deck-options-new-preset-name")
        create.clickOrTap()
        XCTAssertTrue(name.waitForNonExistence(timeout: 20))
        let preset = element(app, "deckOptions.options.presetPicker")
        XCTAssertTrue(preset.exists)
        // Add uses defaults and selects a staged preset; it has not assigned the deck yet.
        XCTAssertTrue(waitForValue(element(app, "deckOptions.options.option.newPerDay"), "20"))
        capture(app, "deck-options-new-preset-staged")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("s", modifierFlags: .command)
        XCTAssertTrue(save.waitForNonExistence(timeout: 20))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("o", modifierFlags: []) // The deck-page O route opens the same saved draft.
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        XCTAssertFalse(gather.exists, "The disclosure opens collapsed after Save too")
        preset.clickOrTap()
        XCTAssertTrue(app.menuItems["Native Options (used by 1 deck)"].firstMatch.waitForExistence(timeout: 10))
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertFalse(save.isEnabled)
        // A second staged preset is discarded through the existing leave prompt.
        element(app, "deckOptions.options.newPreset").clickOrTap()
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        name.clickOrTap(); app.typeText("Discarded Native")
        create.clickOrTap()
        XCTAssertTrue(name.waitForNonExistence(timeout: 20))
        element(app, "deckOptions.options.cancel").clickOrTap()
        let discard = app.sheets.buttons["Discard"].firstMatch
        XCTAssertTrue(discard.waitForExistence(timeout: 10))
        discard.click()
        XCTAssertTrue(save.waitForNonExistence(timeout: 20))
        app.typeKey("o", modifierFlags: [.command, .option])
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        preset.clickOrTap()
        XCTAssertFalse(app.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", "Discarded Native")).firstMatch.exists)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
    }

    func testNewPresetBlocksWindowCommandsAndKeepsNativeTextUndo() throws {
        let (app, bundle) = try launch(extra: ["-AppleKeyboardUIMode", "3"])
        defer { app.terminate() }
        let save = element(app, "deckOptions.options.save")
        app.typeKey("o", modifierFlags: [.command, .option])
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        let limit = element(app, "deckOptions.options.option.newPerDay")
        let changed = (limit.value as? String) == "37" ? "38" : "37"
        limit.clickOrTap(); app.typeKey("a", modifierFlags: .command); app.typeText(changed)
        app.typeKey("s", modifierFlags: .command)
        XCTAssertTrue(save.waitForNonExistence(timeout: 20))
        app.typeKey("o", modifierFlags: [.command, .option])
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        let edit = app.menuBars.menuBarItems["Edit"]
        edit.click()
        XCTAssertTrue(edit.menuItems["Undo Deck Options"].isEnabled, "A real collection step is available before opening")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])

        element(app, "deckOptions.options.newPreset").clickOrTap()
        let name = element(app, "deckOptions.newPreset.name")
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        name.clickOrTap(); app.typeText("Q")
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(waitForValue(name, ""), "The sheet keeps the field's native text undo")
        app.typeKey("z", modifierFlags: [.command, .shift])
        XCTAssertTrue(waitForValue(name, "Q"))

        // Full keyboard access moves focus onto a native sheet button. Prove
        // that focus left Name before checking the collection-undo regression.
        app.typeKey("\t", modifierFlags: [])
        let running = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
            .first { !$0.isTerminated })
        let application = AXUIElementCreateApplication(running.processIdentifier)
        var focused: CFTypeRef?
        XCTAssertEqual(AXUIElementCopyAttributeValue(application, kAXFocusedUIElementAttribute as CFString, &focused), .success)
        let focusValue = try XCTUnwrap(focused)
        XCTAssertEqual(CFGetTypeID(focusValue), AXUIElementGetTypeID())
        let control = focusValue as! AXUIElement
        var role: CFTypeRef?
        XCTAssertEqual(AXUIElementCopyAttributeValue(control, kAXRoleAttribute as CFString, &role), .success)
        XCTAssertEqual(role as? String, kAXButtonRole as String)
        edit.click()
        let undo = edit.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", "Undo")).firstMatch
        XCTAssertTrue(undo.waitForExistence(timeout: 10))
        XCTAssertFalse(undo.isEnabled, "Focus outside Name must not expose collection Undo")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        let file = app.menuBars.menuBarItems["File"]
        file.click()
        XCTAssertFalse(file.menuItems["Import…"].isEnabled)
        XCTAssertFalse(file.menuItems["Export Collection…"].isEnabled)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        let view = app.menuBars.menuBarItems["View"]
        view.click()
        XCTAssertFalse(view.menuItems["Browse"].isEnabled)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        app.typeKey("z", modifierFlags: .command)
        app.typeKey("b", modifierFlags: [.command, .shift])
        XCTAssertTrue(name.exists, "Window commands leave the naming sheet in place")
        element(app, "deckOptions.newPreset.cancel").clickOrTap()
        XCTAssertTrue(name.waitForNonExistence(timeout: 10))
        XCTAssertTrue(save.exists, "Closing returns to the same Deck Options page")
        XCTAssertTrue(waitForValue(limit, changed), "The saved collection step was never undone behind the sheet")
        edit.click()
        XCTAssertTrue(edit.menuItems["Undo Deck Options"].isEnabled, "Closing restores the collection's Undo")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func launch(extra: [String] = []) throws -> (XCUIApplication, String) {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundle = try build.bundleIdentifier
        guard bundle.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false, extra: extra).arguments
        app.launch()
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 20))
        let row = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "home.sidebar.deck.")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        row.clickOrTap()
        XCTAssertTrue(app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch.waitForExistence(timeout: 10))
        return (app, bundle)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func waitForValue(_ target: XCUIElement, _ value: String) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND value == %@", value),
            object: target)], timeout: 10) == .completed
    }
    private func waitForEnabled(_ target: XCUIElement) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND enabled == true"),
            object: target)], timeout: 10) == .completed
    }
}
#endif
