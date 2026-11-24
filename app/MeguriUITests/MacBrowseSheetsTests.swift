import Foundation
import XCTest

/// Browse's command sheets on the Mac (U276; `browser/workspace` V8.1, V8.2,
/// V8.3; N21.6), over the generated typical recipe on the real core: each
/// sheet opened by its menu key on the selected rows, empty, filled and,
/// where its input can be refused, with the error; Return runs it, the
/// table after it, and Edit › Undo named after it. Save Search's sheet
/// with Save disabled while its name is empty. Each state is attached as a
/// capture, light and dark.
@MainActor
final class MacBrowseSheetsTests: XCTestCase {
    #if os(macOS)
    func testSheetsLight() throws { try sheets(appearance: "light") }
    func testSheetsDark() throws { try sheets(appearance: "dark") }

    private func sheets(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 3 }, "\(table.outlineRows.count)")
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        // Wrong places are reported, and the later captures still made.
        continueAfterFailure = true

        // Change Deck… ⌘D: the deck search, a filter, the create row; Return moves.
        app.typeKey("d", modifierFlags: .command)
        let changeDeck = element(app, "browse.sheet.changeDeck")
        XCTAssertTrue(changeDeck.waitForExistence(timeout: 10), "⌘D opens Change Deck")
        let deckList = element(app, "browse.sheet.changeDeck.list")
        XCTAssertTrue(waitUntil { deckList.outlineRows.count + deckList.tableRows.count > 0 }, "the decks are listed")
        sheetCapture(app, "change-deck-empty-\(appearance)")
        app.typeText("a")
        sheetCapture(app, "change-deck-filtered-\(appearance)")
        app.typeText("bqz Moved")
        let create = element(app, "browse.sheet.changeDeck.create")
        XCTAssertTrue(create.waitForExistence(timeout: 10), "no deck has the name: the create row")
        sheetCapture(app, "change-deck-create-\(appearance)")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(waitUntil { !changeDeck.exists }, "Return moves the cards")
        afterCapture(app, "change-deck-after-\(appearance)")
        XCTAssertTrue(undoTitle(app).hasSuffix("Change Deck"), undoTitle(app))

        // Set Due Date… ⇧⌘D: the remembered text, a range's preview, an error.
        let firstRow = table.outlineRows.element(boundBy: 0)
        let rowBefore = rowText(firstRow)
        app.typeKey("d", modifierFlags: [.command, .shift])
        let due = element(app, "browse.sheet.setDueDate")
        XCTAssertTrue(due.waitForExistence(timeout: 10), "⇧⌘D opens Set Due Date")
        sheetCapture(app, "set-due-date-open-\(appearance)")
        app.typeText("3-7")
        XCTAssertTrue(element(app, "browse.sheet.setDueDate.preview").waitForExistence(timeout: 5))
        sheetCapture(app, "set-due-date-range-\(appearance)")
        app.typeKey("a", modifierFlags: .command)
        app.typeText("3--4")
        XCTAssertTrue(element(app, "browse.sheet.setDueDate.error").waitForExistence(timeout: 5), "3--4 is refused")
        XCTAssertFalse(element(app, "browse.sheet.setDueDate.confirm").isEnabled, "Set waits for a valid text")
        sheetCapture(app, "set-due-date-error-\(appearance)")
        app.typeKey("a", modifierFlags: .command)
        app.typeText("2")
        XCTAssertTrue(waitUntil { self.element(app, "browse.sheet.setDueDate.confirm").isEnabled })
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(waitUntil { !due.exists })
        XCTAssertTrue(waitUntil { self.rowText(firstRow) != rowBefore }, "the row's Due cell changes in place: \(rowBefore)")
        afterCapture(app, "set-due-date-after-\(appearance)")
        XCTAssertTrue(undoTitle(app).hasSuffix("Set Due Date"), undoTitle(app))

        // Forget… ⌥⌘N: the two options and "Review history is kept.".
        app.typeKey("n", modifierFlags: [.command, .option])
        let forget = element(app, "browse.sheet.forget")
        XCTAssertTrue(forget.waitForExistence(timeout: 10), "⌥⌘N opens Forget")
        XCTAssertTrue(element(app, "browse.sheet.forget.kept").exists)
        sheetCapture(app, "forget-open-\(appearance)")
        element(app, "browse.sheet.forget.resetCounts").click()
        sheetCapture(app, "forget-filled-\(appearance)")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(waitUntil { !forget.exists })
        afterCapture(app, "forget-after-\(appearance)")
        XCTAssertTrue(undoTitle(app).hasSuffix("Forget"), undoTitle(app))

        // Add Tags… ⇧⌘A: the token field; Return makes the typed text a token and adds it.
        app.typeKey("a", modifierFlags: [.command, .shift])
        let add = element(app, "browse.sheet.addTags")
        XCTAssertTrue(add.waitForExistence(timeout: 10), "⇧⌘A opens Add Tags")
        XCTAssertFalse(element(app, "browse.sheet.addTags.confirm").isEnabled, "Add waits for a tag")
        sheetCapture(app, "add-tags-empty-\(appearance)")
        app.typeText("qzalpha qzouter::inner")
        XCTAssertTrue(waitUntil { self.element(app, "browse.sheet.addTags.confirm").isEnabled })
        sheetCapture(app, "add-tags-filled-\(appearance)")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(waitUntil { !add.exists })
        afterCapture(app, "add-tags-after-\(appearance)")
        XCTAssertTrue(undoTitle(app).hasSuffix("Add Tags"), undoTitle(app))

        // Remove Tags… ⌥⇧⌘A: completion from the notes' own tags.
        app.typeKey("a", modifierFlags: [.command, .option, .shift])
        let remove = element(app, "browse.sheet.removeTags")
        XCTAssertTrue(remove.waitForExistence(timeout: 10), "⌥⇧⌘A opens Remove Tags")
        sheetCapture(app, "remove-tags-empty-\(appearance)")
        app.typeText("qzou")
        screen(app, "remove-tags-completion-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])
        if remove.exists {
            app.typeText("ter")
            app.typeKey(.return, modifierFlags: [])
            XCTAssertTrue(waitUntil { !remove.exists })
            XCTAssertTrue(undoTitle(app).hasSuffix("Remove Tags"), undoTitle(app))
            afterCapture(app, "remove-tags-after-\(appearance)")
        }

        // Esc cancels a sheet: nothing changes.
        app.typeKey("d", modifierFlags: .command)
        XCTAssertTrue(changeDeck.waitForExistence(timeout: 10))
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(waitUntil { !changeDeck.exists }, "Esc cancels")
    }

    /// Save Search's sheet in the commands' frame: the title over the field,
    /// Save disabled while the name is empty.
    func testSaveSearchSheetLight() throws { try saveSearch(appearance: "light") }
    func testSaveSearchSheetDark() throws { try saveSearch(appearance: "dark") }

    private func saveSearch(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        element(app, "browse.sidebar.state.suspended").click()
        XCTAssertTrue(waitUntil { self.resultsTable(app).outlineRows.count == 2 })
        try app.bringToFront(bundleIdentifier: bundle)
        // T13: the Saved Searches section's context menu.
        let header = element(app, "browse.sidebar.savedSearches")
        XCTAssertTrue(header.waitForExistence(timeout: 10), "the sidebar's Saved Searches section")
        header.rightClick()
        let item = app.menuItems.matching(NSPredicate(format: "title BEGINSWITH 'Save Search'")).firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: 5), "Save Search… in the section's menu")
        item.click()
        let sheet = element(app, "browse.saveSearch")
        XCTAssertTrue(sheet.waitForExistence(timeout: 10))
        continueAfterFailure = true
        XCTAssertFalse(element(app, "browse.saveSearch.confirm").isEnabled, "Save waits for a name")
        sheetCapture(app, "save-search-empty-\(appearance)")
        app.typeText("Qz Suspended")
        XCTAssertTrue(waitUntil { self.element(app, "browse.saveSearch.confirm").isEnabled })
        sheetCapture(app, "save-search-filled-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(waitUntil { !sheet.exists })
    }

    // MARK: Helpers

    private func launch(appearance: String) throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1", directoryHint: .isDirectory),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            appearance: appearance).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 30),
                      "Stage the generated typical-1 fixture through tools/mg")
        return (app, bundle)
    }

    private func openBrowse(_ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.browse").click()
        XCTAssertTrue(resultsTable(app).waitForExistence(timeout: 20), "Browse's table")
    }

    private func press(_ row: XCUIElement) {
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
    }

    private func resultsTable(_ app: XCUIApplication) -> XCUIElement {
        app.outlines.matching(identifier: "browse.results.table").firstMatch
    }

    /// The texts of a row's cells, joined.
    private func rowText(_ row: XCUIElement) -> String {
        row.staticTexts.allElementsBoundByIndex.map { "\($0.label)\($0.value.map { "\($0)" } ?? "")" }.joined(separator: "|")
    }

    /// Edit › Undo's title, read from the menu bar.
    private func undoTitle(_ app: XCUIApplication) -> String {
        app.menuBars.menuItems.matching(NSPredicate(format: "title BEGINSWITH 'Undo'")).firstMatch.title
    }

    /// The table after a command: its rows are read again first.
    private func afterCapture(_ app: XCUIApplication, _ name: String) {
        RunLoop.current.run(until: Date().addingTimeInterval(1.5))
        capture(app, name)
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// A sheet hangs from the window: the window's capture holds both.
    private func sheetCapture(_ app: XCUIApplication, _ name: String) {
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        capture(app, name)
    }

    private func screen(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func waitUntil(timeout: TimeInterval = 10, _ condition: @escaping () -> Bool) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return condition()
    }
    #else
    func testMacOnly() throws { throw XCTSkip("Browse's sheets are the Mac's in this version (U278, U279)") }
    #endif
}
