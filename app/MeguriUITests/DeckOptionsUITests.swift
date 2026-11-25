import Foundation
import XCTest
#if os(macOS)

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
        // Use the guest's available screen so the native panel's row labels
        // and all five pickers fit together in the evidence attachment.
        let (app, bundle) = try launch(extra: ["-MeguriWindowSize", "large"])
        defer { app.terminate() }
        try app.bringToFront(bundleIdentifier: bundle)
        let deckMenu = app.menuBars.menuBarItems["Deck"]
        deckMenu.click()
        let options = menuItem(deckMenu, titled: "Deck Options…")
        XCTAssertTrue(options.waitForExistence(timeout: 10) && options.isEnabled)
        options.click()
        let save = element(app, "deckOptions.options.save")
        XCTAssertTrue(save.waitForExistence(timeout: 20))
        let gather = app.popUpButtons["deckOptions.options.order.newGatherPriority"]
        XCTAssertFalse(gather.exists, "Advanced is collapsed whenever the page opens")
        try app.bringToFront(bundleIdentifier: bundle)
        let advanced = app.buttons["deckOptions.options.advanced"]
        XCTAssertTrue(advanced.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForValue(advanced, "Collapsed"))
        XCTAssertTrue(advanced.isHittable)
        advanced.click()
        XCTAssertTrue(waitForValue(advanced, "Expanded"))
        XCTAssertTrue(gather.waitForExistence(timeout: 10))
        let orderRows = ["newGatherPriority", "newSortOrder", "newMix", "interdayLearningMix", "reviewOrder"]
        let review = app.popUpButtons["deckOptions.options.order.reviewOrder"]
        let scroll = app.scrollViews.containing(.popUpButton, identifier: "deckOptions.options.order.reviewOrder").firstMatch
        XCTAssertTrue(bringIntoView(review, in: scroll, app: app))
        for row in orderRows {
            let picker = app.popUpButtons["deckOptions.options.order." + row]
            XCTAssertTrue(picker.waitForExistence(timeout: 10),
                          "Advanced exposes the actual native picker for \(row)")
        }
        capture(app, "deck-options-five-native-pickers")
        for row in orderRows {
            let picker = app.popUpButtons["deckOptions.options.order." + row]
            XCTAssertTrue(picker.isHittable && app.windows.firstMatch.frame.contains(picker.frame),
                          "The native panel screenshot shows the picker for \(row)")
        }
        for label in ["New card gather order", "New card sort order", "New/review order",
                      "Interday learning/review order", "Review sort order"] {
            let text = app.staticTexts[label].firstMatch
            XCTAssertTrue(text.exists && app.windows.firstMatch.frame.contains(text.frame),
                          "The native panel screenshot shows the row label \(label)")
        }
        XCTAssertFalse(save.isEnabled, "Expanding the disclosure never changes the draft")
        XCTAssertTrue(bringIntoView(advanced, in: scroll, app: app))
        advanced.click()
        XCTAssertTrue(waitForValue(advanced, "Collapsed"))
        for row in orderRows {
            XCTAssertFalse(app.popUpButtons["deckOptions.options.order." + row].exists)
        }
        XCTAssertFalse(save.isEnabled, "Collapsing the disclosure never changes the draft")
        advanced.click()
        XCTAssertTrue(gather.waitForExistence(timeout: 10))
        XCTAssertTrue(bringIntoView(gather, in: scroll, app: app))
        // This is the native menu's spec order, including the nonsequential value 5.
        let gatherLabels = ["Deck", "Deck, then random notes", "Ascending position", "Descending position", "Random notes", "Random cards"]
        let gatherMenu = openPopupMenu(gather, containing: "Deck, then random notes")
        XCTAssertEqual(popupTitles(gatherMenu), gatherLabels)
        popupItem(gatherMenu, titled: "Deck, then random notes").click()
        XCTAssertTrue(waitForEnabled(save))
        XCTAssertTrue(bringIntoView(review, in: scroll, app: app))
        let reviewMenu = openPopupMenu(review, containing: "Descending intervals")
        XCTAssertEqual(popupTitles(reviewMenu),
                       ["Due date, then random", "Due date, then deck", "Deck, then due date", "Ascending intervals",
                        "Descending intervals", "Easy cards first", "Difficult cards first", "Ascending retrievability",
                        "Descending retrievability", "Relative overdueness", "Random", "Order added", "Latest added first"])
        popupItem(reviewMenu, titled: "Descending intervals").click()
        XCTAssertTrue(waitForValue(review, "Descending intervals"))
        XCTAssertTrue(bringIntoView(review, in: scroll, app: app))
        capture(app, "deck-options-expanded")
        let newPreset = app.buttons["deckOptions.options.newPreset"]
        XCTAssertTrue(bringIntoView(newPreset, in: scroll, app: app))
        XCTAssertTrue(waitForEnabled(newPreset))
        XCTAssertTrue(newPreset.isHittable)
        newPreset.click()
        let name = element(app, "deckOptions.newPreset.name")
        let create = element(app, "deckOptions.newPreset.create")
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        XCTAssertFalse(create.isEnabled)
        name.clickOrTap(); app.typeText("   ")
        XCTAssertFalse(create.isEnabled, "Whitespace is not a name")
        app.typeKey("a", modifierFlags: .command); app.typeText(" Native Options ")
        XCTAssertTrue(waitForEnabled(create))
        capture(app, "deck-options-new-preset-name")
        create.clickOrTap()
        XCTAssertTrue(name.waitForNonExistence(timeout: 20))
        let preset = app.popUpButtons["deckOptions.options.presetPicker"]
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
        let footer = app.staticTexts["deckOptions.options.presetFooter"]
        let savedPresetTitle = "Native Options (used by 1 deck)"
        XCTAssertFalse(save.isEnabled)

        // Observe the closed popup: dismissing its tracking menu with Escape
        // here can reach the page's leave command or consume the next click.
        XCTAssertTrue(waitForValue(preset, "Native Options"), "The saved name is trimmed and selected")
        XCTAssertTrue(waitForValue(footer, "Used by this deck only."),
                      "The saved preset is assigned to this deck only")
        // A second staged preset is discarded through the existing leave prompt.
        XCTAssertTrue(waitForEnabled(newPreset), "New Preset is enabled after reopening")
        XCTAssertTrue(newPreset.isHittable, "New Preset is hittable after reopening")
        newPreset.click()
        XCTAssertTrue(name.waitForExistence(timeout: 10), "New Preset presents its naming field")
        name.clickOrTap(); app.typeText("Discarded Native")
        XCTAssertTrue(waitForEnabled(create), "The second name enables Create")
        create.clickOrTap()
        XCTAssertTrue(name.waitForNonExistence(timeout: 20), "Create closes the naming sheet")
        XCTAssertTrue(waitForValue(preset, "Discarded Native"), "The second preset is selected in the draft")
        let cancel = app.buttons["deckOptions.options.cancel"]
        cancel.click()
        // Native confirmation containers vary across macOS versions.
        let prompt = app.descendants(matching: .any).matching(NSPredicate(
            format: "elementType IN %@", [XCUIElement.ElementType.sheet.rawValue,
                                         XCUIElement.ElementType.alert.rawValue,
                                         XCUIElement.ElementType.dialog.rawValue]))
            .containing(.button, identifier: "Discard").firstMatch
        let discard = prompt.buttons["Discard"]
        XCTAssertTrue(prompt.waitForExistence(timeout: 10), "The leave prompt contains its actual Discard button")
        XCTAssertTrue(discard.isEnabled && discard.isHittable, "The prompt's Discard button is actionable")
        // The system may replace the cancel-role source title with Cancel.
        let keep = prompt.buttons.matching(NSPredicate(
            format: "label == %@ OR label == %@", "Cancel", "Keep Editing")).firstMatch
        XCTAssertTrue(keep.exists && keep.isEnabled && keep.isHittable, "The prompt offers native cancellation")
        keep.click()
        XCTAssertTrue(prompt.waitForNonExistence(timeout: 10), "Cancelling dismisses the prompt")
        XCTAssertTrue(save.exists && waitForEnabled(save), "Cancelling the confirmation keeps the staged draft")
        XCTAssertTrue(waitForValue(preset, "Discarded Native"), "The unsaved preset survives cancellation")
        cancel.click()
        XCTAssertTrue(discard.waitForExistence(timeout: 10), "Leaving again presents Discard")
        XCTAssertTrue(discard.isEnabled && discard.isHittable, "The presented prompt's Discard button is actionable")
        discard.click()
        XCTAssertTrue(prompt.waitForNonExistence(timeout: 10), "Discard dismisses the leave prompt")
        XCTAssertTrue(save.waitForNonExistence(timeout: 20), "Discard closes Deck Options")
        app.typeKey("o", modifierFlags: [.command, .option])
        XCTAssertTrue(save.waitForExistence(timeout: 20), "Deck Options reopens after Discard")
        preset.click()
        let keptPresetMenu = preset.menus.firstMatch
        XCTAssertTrue(popupItem(keptPresetMenu, titled: savedPresetTitle).waitForExistence(timeout: 10),
                      "The final native menu retains the saved preset and its full used-by count")
        XCTAssertTrue(!keptPresetMenu.children(matching: .menuItem).matching(NSPredicate(
            format: "title BEGINSWITH %@", "Discarded Native")).firstMatch.exists,
                      "The final native menu contains no discarded preset")
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
        let collectionUndo = menuItem(edit, titled: "Undo Deck Options")
        XCTAssertTrue(collectionUndo.waitForExistence(timeout: 10))
        XCTAssertTrue(collectionUndo.isEnabled, "A real collection step is available before opening")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])

        element(app, "deckOptions.options.newPreset").clickOrTap()
        let sheet = app.sheets.containing(.textField, identifier: "deckOptions.newPreset.name").firstMatch
        let name = sheet.textFields["deckOptions.newPreset.name"]
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        XCTAssertTrue(sheet.buttons["deckOptions.newPreset.cancel"].exists)
        try app.bringToFront(bundleIdentifier: bundle)
        name.clickOrTap(); app.typeText("Q")
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(waitForValue(name, ""), "The sheet keeps the field's native text undo")
        app.typeKey("z", modifierFlags: [.command, .shift])
        XCTAssertTrue(waitForValue(name, "Q"))

        // Full keyboard access moves from Name to the native Cancel button.
        // Ordinary typing proves Name no longer receives keys; Space below
        // proves button focus after all menus have returned keyboard ownership.
        app.typeKey("\t", modifierFlags: [])
        app.typeKey("x", modifierFlags: [])
        XCTAssertTrue(waitForValue(name, "Q"), "After Tab, ordinary typing does not enter Name")
        edit.click()
        let undo = edit.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", "Undo")).firstMatch
        XCTAssertTrue(undo.waitForExistence(timeout: 10))
        XCTAssertFalse(undo.isEnabled, "Focus outside Name must not expose collection Undo")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        let file = app.menuBars.menuBarItems["File"]
        file.click()
        let importItem = menuItem(file, titled: "Import…")
        let exportItem = menuItem(file, titled: "Export Collection…")
        XCTAssertTrue(importItem.waitForExistence(timeout: 10) && exportItem.exists)
        XCTAssertFalse(importItem.isEnabled)
        XCTAssertFalse(exportItem.isEnabled)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        let view = app.menuBars.menuBarItems["View"]
        view.click()
        let browse = menuItem(view, titled: "Browse")
        XCTAssertTrue(browse.waitForExistence(timeout: 10))
        XCTAssertFalse(browse.isEnabled)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        app.typeKey("z", modifierFlags: .command)
        app.typeKey("b", modifierFlags: [.command, .shift])
        XCTAssertTrue(name.exists, "Window commands leave the naming sheet in place")
        XCTAssertTrue(waitForValue(name, "Q"), "The command checks leave Name unchanged")
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(name.waitForNonExistence(timeout: 10))
        XCTAssertTrue(sheet.waitForNonExistence(timeout: 10), "Space activates the focused Cancel button")
        XCTAssertTrue(save.exists, "Closing returns to the same Deck Options page")
        XCTAssertTrue(waitForValue(limit, changed), "The saved collection step was never undone behind the sheet")
        edit.click()
        XCTAssertTrue(collectionUndo.waitForExistence(timeout: 10))
        XCTAssertTrue(collectionUndo.isEnabled, "Closing restores the collection's Undo")
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
    private func menuItem(_ menu: XCUIElement, titled title: String) -> XCUIElement {
        // Native Mac menus can expose their captions as either title or label.
        menu.menuItems.matching(NSPredicate(format: "title == %@ OR label == %@", title, title)).firstMatch
    }
    private func popupItem(_ menu: XCUIElement, titled title: String) -> XCUIElement {
        menu.children(matching: .menuItem).matching(NSPredicate(format: "title == %@", title)).firstMatch
    }

    private func openPopupMenu(_ picker: XCUIElement, containing title: String,
                               file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        picker.click()
        let menu = picker.menus.firstMatch
        XCTAssertTrue(popupItem(menu, titled: title).waitForExistence(timeout: 10), file: file, line: line)
        return menu
    }

    private func popupTitles(_ menu: XCUIElement) -> [String] {
        menu.children(matching: .menuItem).allElementsBoundByIndex.map(\.title).filter { !$0.isEmpty }
    }

    private func bringIntoView(_ control: XCUIElement, in scroll: XCUIElement, app: XCUIApplication) -> Bool {
        // A visible native control needs no scroll-area or scrollbar AX node.
        if control.exists, !control.frame.isEmpty,
           app.windows.firstMatch.frame.contains(control.frame), control.isHittable { return true }
        guard scroll.exists else { return false }
        for _ in 0..<8 {
            let viewport = scroll.frame.intersection(app.windows.firstMatch.frame)
            guard !viewport.isEmpty, !control.frame.isEmpty else { return false }
            if viewport.contains(control.frame), control.isHittable { return true }
            let reach = viewport.height / 3
            let move = max(-reach, min(reach, viewport.midY - control.frame.midY))
            scroll.scroll(byDeltaX: 0, deltaY: move)
        }
        return false
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
