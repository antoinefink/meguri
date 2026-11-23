import Foundation
import XCTest

/// Browse's inspector on the Mac (U273, restyled as Option A; `browser/workspace`
/// V7, V12; N21.3, N21.4, N21.5, N21.8), over the generated typical recipe on the real core:
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
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 3 }, "rows: \(table.outlineRows.count)")

        // N21.3: the toggle is the toolbar's last item, after the Add menu.
        let toggle = element(app, "browse.toolbar.inspector"), add = element(app, "home.collection.add")
        XCTAssertTrue(toggle.waitForExistence(timeout: 10) && add.exists)
        XCTAssertLessThan(add.frame.maxX, toggle.frame.minX + 1, "the inspector toggle comes last: \(add.frame) \(toggle.frame)")

        // No Selection.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        XCUIElement.perform(withKeyModifiers: .command) { press(table.outlineRows.element(boundBy: 0)) }
        XCTAssertTrue(element(app, "browse.inspector.empty").waitForExistence(timeout: 10), "No Selection")
        capture(app, "no-selection-\(appearance)")

        // Several rows: the summary and the commands.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        XCUIElement.perform(withKeyModifiers: .shift) { press(table.outlineRows.element(boundBy: 3)) }
        XCTAssertTrue(element(app, "browse.inspector.summary.title").waitForExistence(timeout: 10), "V7's summary")
        sleep(1)
        capture(app, "several-rows-\(appearance)")

        // A card with a sound and an image (the Español deck's notes).
        try showDeck(app, bundle, containing: "Español")
        press(table.outlineRows.element(boundBy: 0))
        // The Note tab: its Deck pop-up, and the fields' page (the tab's own container is
        // folded into the pane's scroll view in the accessibility tree).
        let note = element(app, "browse.inspector.deck")
        XCTAssertTrue(note.waitForExistence(timeout: 10), "the Note tab")
        XCTAssertTrue(element(app, "browse.inspector.deck").exists && element(app, "browse.inspector.type").exists)
        XCTAssertTrue(element(app, "browse.inspector.tags").exists && element(app, "browse.inspector.formatting").exists)
        let fields = element(app, "editor.fields").textViews
        XCTAssertTrue(shows(fields, "palabra"), "the selected row's note")
        XCTAssertFalse(element(app, "browse.inspector.format.bold").isEnabled, "Option A: the bar is dimmed until a field has the focus")
        sleep(2)
        capture(app, "sound-and-image-\(appearance)")

        // Right to left, then CJK with a sound.
        try showDeck(app, bundle, containing: "العربية")
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, "كلمة"), "the selected row's note")
        sleep(1)
        capture(app, "right-to-left-\(appearance)")
        // V12: a field emptied, then behind its row on the note's next showing, then shown.
        try app.bringToFront(bundleIdentifier: bundle)
        fields.element(boundBy: 1).click()
        app.typeKey("a", modifierFlags: [.command])
        app.typeKey(.delete, modifierFlags: [])
        press(table.outlineRows.element(boundBy: 1))
        XCTAssertTrue(waitUntil(timeout: 20) { fields.count > 1 && !(fields.element(boundBy: 1).value as? String ?? "").isEmpty },
                      "the next row's note, whose second field is filled")
        press(table.outlineRows.element(boundBy: 0))
        let empty = element(app, "browse.inspector.emptyFields")
        XCTAssertTrue(empty.waitForExistence(timeout: 10), "the emptied field hides behind its row")
        let shown = fields.count
        try app.bringToFront(bundleIdentifier: bundle)
        empty.click()
        XCTAssertTrue(waitUntil { fields.count > shown }, "every field shows: \(fields.count) after \(shown)")
        sleep(1)
        capture(app, "empty-fields-open-\(appearance)")
        empty.click()
        try showDeck(app, bundle, containing: "語彙")
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, "単語"), "the selected row's note")
        sleep(2)
        capture(app, "cjk-with-sound-\(appearance)")

        // A field being edited, the formatting bar in use and ⌘I (F14) reaching the field.
        try app.bringToFront(bundleIdentifier: bundle)
        let first = fields.element(boundBy: 1)
        first.click()
        app.typeKey(.downArrow, modifierFlags: [.command])
        app.typeText(" kerplunk")
        app.typeKey(.leftArrow, modifierFlags: [.option, .shift])
        let bold = element(app, "browse.inspector.format.bold")
        XCTAssertTrue(waitUntil { bold.isEnabled }, "Bold acts on the focused field")
        bold.click()
        app.typeKey(.rightArrow, modifierFlags: [])
        app.typeText(" vintor")
        app.typeKey(.leftArrow, modifierFlags: [.option, .shift])
        app.typeKey("i", modifierFlags: [.command])
        capture(app, "field-being-edited-\(appearance)")

        // Many tags, wrapping in their section.
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "browse.inspector.tags").click()
        app.typeText("alpha bravo charlie delta echo foxtrot golf hotel india juliett kilo lima ")
        app.typeKey(.tab, modifierFlags: [])
        sleep(1)
        capture(app, "many-tags-\(appearance)")

        // V12, W2: choosing another row saves the edit first, one "Edit Note" step.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 1))
        try app.bringToFront(bundleIdentifier: bundle)
        app.menuBars.menuBarItems["Edit"].click()
        XCTAssertTrue(app.menuItems["Undo Edit Note"].waitForExistence(timeout: 10), "V8.3: the save's step")
        app.typeKey(.escape, modifierFlags: [])

        // A long note: many lines typed; one scroll view for the whole pane.
        try app.bringToFront(bundleIdentifier: bundle)
        fields.element(boundBy: 0).click()
        app.typeKey(.downArrow, modifierFlags: [.command])
        for line in 1...14 { app.typeText("\rline \(line) of a long field that wraps across the pane") }
        capture(app, "long-note-\(appearance)")
        let bar = element(app, "browse.inspector.formatting")
        XCTAssertTrue(bar.frame.maxY <= app.windows.firstMatch.frame.maxY + 1, "the bar stays at the pane's bottom edge")

        // N21.3: under about 900 points the pane hides; Return shows it over the table.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        try resize(app, bundle: bundle, to: "820x700")
        XCTAssertTrue(waitUntil { !note.exists }, "the pane hid itself")
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(note.waitForExistence(timeout: 10), "Return shows it over the table")
        sleep(1)
        capture(app, "narrow-over-table-\(appearance)")
    }

    /// Browse's sidebar: the deck row whose name holds `name` sets the Deck token. The
    /// table has that deck's results once the status line counts what the row counts
    /// (the previous results stay shown while a search runs).
    private func showDeck(_ app: XCUIApplication, _ bundle: String, containing name: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        let row = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "browse.sidebar.deck.", name)).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10), "the deck row \(name)")
        // "90 cards" on the row; "90 of 415 cards · 1 selected" on the status line.
        let count = (row.value as? String ?? "").split(separator: " ").first.map(String.init) ?? ""
        row.click()
        let status = element(app, "browse.status.text")
        XCTAssertTrue(waitUntil(timeout: 30) { !count.isEmpty && (status.value as? String ?? "").hasPrefix(count + " ") },
                      "the deck's own results: \(name) \(count) \(status.value as? String ?? "")")
        try app.bringToFront(bundleIdentifier: bundle)
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
        XCTAssertTrue(resultsTable(app).waitForExistence(timeout: 20), "Browse's table")
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

    /// The inspector shows the selected row's note: its first field holds `text` (the
    /// previous note's page stays shown while the next one is read).
    private func shows(_ fields: XCUIElementQuery, _ text: String) -> Bool {
        waitUntil(timeout: 20) { fields.count > 0 && (fields.element(boundBy: 0).value as? String ?? "").contains(text) }
    }

    /// SwiftUI's Table is an outline in the Mac's accessibility tree.
    private func resultsTable(_ app: XCUIApplication) -> XCUIElement {
        app.outlines.matching(identifier: "browse.results.table").firstMatch
    }

    /// A row is clicked at its point: XCTest's hit test reads a SwiftUI table's rows as not hittable.
    private func press(_ row: XCUIElement) {
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
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
