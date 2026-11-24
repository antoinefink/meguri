import Foundation
import XCTest

/// Browse's Find and Replace… and Find Duplicates… on the Mac (U277;
/// `browser/workspace` V9, V10; N21.6), over the generated typical recipe on
/// the real core: Find and Replace on three selected notes, empty, filled,
/// with an invalid pattern and with its 3-note preview; Return replaces, and
/// Edit › Undo is named after it. Find Duplicates on the field the three
/// notes now share: its groups become the Duplicates token; on the three
/// notes' Back it finds none and says so. Each state is attached as a
/// capture, light and dark.
@MainActor
final class MacBrowseFindTests: XCTestCase {
    #if os(macOS)
    func testFindSheetsLight() throws { try findSheets(appearance: "light") }
    func testFindSheetsDark() throws { try findSheets(appearance: "dark") }

    private func findSheets(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 3 }, "\(table.outlineRows.count)")
        // The Spanish deck's notes (one card each): "palabra <n>" in Front.
        let search = app.toolbars.searchFields.firstMatch.exists ? app.toolbars.searchFields.firstMatch : app.searchFields.element(boundBy: 1)
        try app.bringToFront(bundleIdentifier: bundle)
        search.click()
        search.typeText("palabra")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, statusText(app))
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        XCUIElement.perform(withKeyModifiers: .shift) { self.press(table.outlineRows.element(boundBy: 2)) }
        XCTAssertTrue(waitUntil { self.statusText(app).contains("3 selected") }, statusText(app))
        // Wrong places are reported, and the later captures still made.
        continueAfterFailure = true

        // Find and Replace… ⌥⌘F: on the three selected notes, the focus in Find.
        app.typeKey("f", modifierFlags: [.command, .option])
        let sheet = element(app, "browse.sheet.findReplace")
        XCTAssertTrue(sheet.waitForExistence(timeout: 10), "⌥⌘F opens Find and Replace")
        let notes = element(app, "browse.sheet.findReplace.notes")
        XCTAssertTrue(waitUntil { notes.radioButtons["3 Selected"].exists }, "Notes: 3 Selected")
        XCTAssertTrue(waitUntil { notes.radioButtons["3 Selected"].value as? Int == 1 }, "Selected is the default with a selection")
        XCTAssertFalse(element(app, "browse.sheet.findReplace.confirm").isEnabled, "Replace waits for notes that change")
        sheetCapture(app, "find-replace-empty-\(appearance)")

        // A pattern: the three notes' Front becomes "palabra"; first with an error.
        element(app, "browse.sheet.findReplace.regex").click()
        let find = element(app, "browse.sheet.findReplace.find")
        find.click()
        find.typeText("palabra \\d+(")
        XCTAssertTrue(element(app, "browse.sheet.findReplace.error").waitForExistence(timeout: 10), "the core's reason, inline")
        XCTAssertFalse(element(app, "browse.sheet.findReplace.confirm").isEnabled, "Replace waits for a valid pattern")
        sheetCapture(app, "find-replace-error-\(appearance)")
        find.typeKey(.delete, modifierFlags: [])
        let replace = element(app, "browse.sheet.findReplace.replace")
        replace.click()
        replace.typeText("palabra")
        let count = element(app, "browse.sheet.findReplace.count")
        XCTAssertTrue(waitUntil { self.label(count).contains("3 of 3 notes will change") }, label(count))
        XCTAssertTrue(waitUntil { self.element(app, "browse.sheet.findReplace.confirm").isEnabled })
        XCTAssertEqual(element(app, "browse.sheet.findReplace.confirm").title, "Replace in 3 Notes")
        sheetCapture(app, "find-replace-preview-\(appearance)")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(waitUntil { !sheet.exists }, "Return replaces")
        afterCapture(app, "find-replace-after-\(appearance)")
        XCTAssertTrue(waitUntil { self.undoTitle(app).hasSuffix("Find and Replace") }, undoTitle(app))

        // Find Duplicates… (Edit menu): it starts on the Sort Field, Front, which the three notes now share.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        menuItem(app, "Find Duplicates…").click()
        let duplicates = element(app, "browse.sheet.findDuplicates")
        XCTAssertTrue(duplicates.waitForExistence(timeout: 10), "Edit › Find Duplicates… opens its sheet")
        let field = element(app, "browse.sheet.findDuplicates.field")
        XCTAssertTrue(waitUntil { self.label(field).contains("Front") }, "starts on the Sort Field: \(self.label(field))")
        sheetCapture(app, "find-duplicates-open-\(appearance)")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(waitUntil { !duplicates.exists }, "Find replaces the query with the groups")
        XCTAssertTrue(waitUntil { table.outlineRows.count == 3 }, "the three notes, in Notes mode: \(table.outlineRows.count)")
        let token = element(app, "browse.search.token.0")
        XCTAssertTrue(waitUntil { token.exists }, "one Duplicates token")
        afterCapture(app, "find-duplicates-token-\(appearance)")

        // On the three notes' Back (All 3 Found): no group, and the sheet says so.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        menuItem(app, "Find Duplicates…").click()
        XCTAssertTrue(duplicates.waitForExistence(timeout: 10))
        XCTAssertTrue(waitUntil { self.label(field).contains("Front") })
        field.click()
        let back = app.menuItems.matching(NSPredicate(format: "title == 'Back'")).firstMatch
        XCTAssertTrue(back.waitForExistence(timeout: 5), "Back in the Field menu")
        back.click()
        let found = element(app, "browse.sheet.findDuplicates.notes").radioButtons["All 3 Found"]
        XCTAssertTrue(found.waitForExistence(timeout: 10), "All 3 Found")
        found.click()
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(element(app, "browse.sheet.findDuplicates.none").waitForExistence(timeout: 10), "No duplicates found.")
        XCTAssertTrue(duplicates.exists, "the sheet stays")
        sheetCapture(app, "find-duplicates-none-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(waitUntil { !duplicates.exists }, "Esc cancels")
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

    private func statusText(_ app: XCUIApplication) -> String {
        let status = element(app, "browse.status.text")
        return status.exists ? label(status) : ""
    }

    private func label(_ element: XCUIElement) -> String {
        [element.label, element.value.map { "\($0)" } ?? "", element.title]
            .joined(separator: " ")
    }

    private func menuItem(_ app: XCUIApplication, _ title: String) -> XCUIElement {
        app.menuBars.menuBarItems["Edit"].click()
        return app.menuBars.menuItems[title]
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
        RunLoop.current.run(until: Date().addingTimeInterval(0.5))
        capture(app, name)
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
    func testMacOnly() throws { throw XCTSkip("Browse's find sheets are the Mac's in this version (U278, U279)") }
    #endif
}
