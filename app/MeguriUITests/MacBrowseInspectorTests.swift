import Foundation
import XCTest

/// Browse's inspector on the Mac (U273; `browser/workspace` V7, V12; N21.3,
/// N21.4, N21.5, N21.8), over the generated typical recipe on the real core:
/// the toggle last in the toolbar; one card with its preview, Deck and Type,
/// fields and tags; the empty fields opened; the formatting bar in use; an
/// edit saved when another row is chosen, as Edit › Undo's "Undo Edit Note";
/// a long note scrolling in the pane; a card with an image; several rows'
/// summary; no selection; the pane hidden in a narrow window and shown over
/// the table by Return. Each step attaches its window as a capture.
@MainActor
final class MacBrowseInspectorTests: XCTestCase {
    #if os(macOS)
    func testInspectorLight() throws { try inspector(appearance: "light") }
    func testInspectorDark() throws { try inspector(appearance: "dark") }

    private func inspector(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        let table = app.tables["browse.results.table"]
        XCTAssertTrue(waitUntil { table.tableRows.count > 3 }, "rows: \(table.tableRows.count)")

        // N21.3: the toggle is the toolbar's last item, after the Add menu.
        let toggle = element(app, "browse.toolbar.inspector"), add = element(app, "home.collection.add")
        XCTAssertTrue(toggle.waitForExistence(timeout: 10) && add.exists)
        XCTAssertLessThan(add.frame.maxX, toggle.frame.minX + 1, "the inspector toggle comes last: \(add.frame) \(toggle.frame)")

        // One card: its note in the pane.
        try app.bringToFront(bundleIdentifier: bundle)
        table.tableRows.element(boundBy: 0).click()
        let note = element(app, "browse.inspector.note")
        XCTAssertTrue(note.waitForExistence(timeout: 10), "the Note tab")
        XCTAssertTrue(element(app, "browse.inspector.deck").exists && element(app, "browse.inspector.type").exists)
        XCTAssertTrue(element(app, "browse.inspector.tags").exists && element(app, "browse.inspector.formatting").exists)
        let fields = note.textViews
        XCTAssertTrue(waitUntil { fields.count > 0 }, "the note's fields")
        sleep(1)
        capture(app, "one-card-\(appearance)")

        // V12: the empty fields behind their row, then shown.
        let empty = element(app, "browse.inspector.emptyFields")
        if empty.exists {
            let shown = fields.count
            try app.bringToFront(bundleIdentifier: bundle)
            empty.click()
            XCTAssertTrue(waitUntil { fields.count > shown }, "every field shows: \(fields.count) after \(shown)")
            capture(app, "empty-fields-open-\(appearance)")
            empty.click()
            XCTAssertTrue(waitUntil { fields.count == shown })
        }

        // The formatting bar: a word typed, selected and made bold.
        try app.bringToFront(bundleIdentifier: bundle)
        let first = fields.element(boundBy: 0)
        first.click()
        app.typeKey(.downArrow, modifierFlags: [.command])
        app.typeText(" kerplunk")
        app.typeKey(.leftArrow, modifierFlags: [.option, .shift])
        let bold = element(app, "browse.inspector.format.bold")
        XCTAssertTrue(waitUntil { bold.isEnabled }, "Bold acts on the focused field")
        bold.click()
        capture(app, "formatting-bar-\(appearance)")
        // F14: ⌘I reaches Format › Italic (the menu's key equivalent) while the field has the focus.
        app.typeKey(.rightArrow, modifierFlags: [])
        app.typeText(" vintor")
        app.typeKey(.leftArrow, modifierFlags: [.option, .shift])
        app.typeKey("i", modifierFlags: [.command])
        capture(app, "format-key-cmd-i-\(appearance)")

        // V12, W2: choosing another row saves the edit first, one "Edit Note" step.
        try app.bringToFront(bundleIdentifier: bundle)
        table.tableRows.element(boundBy: 1).click()
        try app.bringToFront(bundleIdentifier: bundle)
        app.menuBars.menuBarItems["Edit"].click()
        let undo = app.menuItems["Undo Edit Note"]
        XCTAssertTrue(undo.waitForExistence(timeout: 10), "V8.3: the save's step")
        screen(app, "edit-menu-undo-edit-note-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])

        // A long note: many lines typed; the fields scroll inside the pane.
        try app.bringToFront(bundleIdentifier: bundle)
        fields.element(boundBy: 0).click()
        app.typeKey(.downArrow, modifierFlags: [.command])
        for line in 1...40 { app.typeText("\rline \(line)") }
        capture(app, "long-note-\(appearance)")
        XCTAssertTrue(note.frame.maxY <= app.windows.firstMatch.frame.maxY + 1, "the pane stays in the window")

        // Several rows: the summary and the commands.
        try app.bringToFront(bundleIdentifier: bundle)
        table.tableRows.element(boundBy: 2).click()
        XCUIElement.perform(withKeyModifiers: .shift) { table.tableRows.element(boundBy: 5).click() }
        let title = element(app, "browse.inspector.summary.title")
        XCTAssertTrue(title.waitForExistence(timeout: 10), "V7's summary")
        XCTAssertTrue(title.label.contains("Selected"), title.label)
        sleep(1)
        capture(app, "several-rows-\(appearance)")

        // No selection.
        try app.bringToFront(bundleIdentifier: bundle)
        table.tableRows.element(boundBy: 2).click()
        XCUIElement.perform(withKeyModifiers: .command) { table.tableRows.element(boundBy: 2).click() }
        XCTAssertTrue(element(app, "browse.inspector.empty").waitForExistence(timeout: 10), "No Selection")
        capture(app, "no-selection-\(appearance)")

        // A card with an image: the search finds the notes that reference one.
        try app.bringToFront(bundleIdentifier: bundle)
        let search = app.toolbars.searchFields.firstMatch.exists ? app.toolbars.searchFields.firstMatch : app.searchFields.element(boundBy: 1)
        search.click()
        app.typeText("png\r")
        XCTAssertTrue(waitUntil { table.tableRows.count > 0 })
        sleep(1)
        try app.bringToFront(bundleIdentifier: bundle)
        table.tableRows.element(boundBy: 0).click()
        XCTAssertTrue(note.waitForExistence(timeout: 10))
        sleep(2)
        capture(app, "card-with-image-\(appearance)")

        // N21.3: under about 900 points the pane hides; Return shows it over the table.
        try resize(app, bundle: bundle, to: "820x700")
        XCTAssertTrue(waitUntil { !note.exists }, "the pane hid itself")
        capture(app, "narrow-hidden-\(appearance)")
        try app.bringToFront(bundleIdentifier: bundle)
        table.tableRows.element(boundBy: 0).click()
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(note.waitForExistence(timeout: 10), "Return shows it over the table")
        sleep(1)
        capture(app, "narrow-over-table-\(appearance)")
    }

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
            windowSize: "1440x900", appearance: appearance).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 30),
                      "Stage the generated typical-1 fixture through tools/mg")
        return (app, bundle)
    }

    private func openBrowse(_ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.browse").click()
        XCTAssertTrue(app.tables["browse.results.table"].waitForExistence(timeout: 20), "Browse's table")
    }

    private func resize(_ app: XCUIApplication, bundle: String, to size: String) throws {
        let resized = expectation(description: "The main window resized")
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: Notification.Name(bundle + ".testhooks.windowResized"),
            object: nil, queue: .main) { _ in resized.fulfill() }
        defer { center.removeObserver(observer) }
        center.postNotificationName(Notification.Name(bundle + ".testhooks.resizeWindow"), object: size,
            userInfo: nil, deliverImmediately: true)
        wait(for: [resized], timeout: 10)
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
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
    #endif
}
