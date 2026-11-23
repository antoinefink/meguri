import Foundation
import XCTest

/// Browse's commands on the Mac (U275; `browser/workspace` V7, V8, V1;
/// N21.2, N21.3, N21.5, N21.8, N23.6), over the generated typical recipe
/// (two suspended cards, one buried) on the real core: the toolbar's order,
/// the sidebar field's prompt, a row's context menu with one and with
/// several rows in both modes, Unsuspend and its undo, the Note and Card
/// menus, the Delete question, the kept scroll position and Esc in the
/// search field. Each case attaches its window (or, with a menu open, the
/// VM's screen) as a capture.
@MainActor
final class MacBrowseCommandsTests: XCTestCase {
    #if os(macOS)
    func testToolbarSidebarAndContextMenusLight() throws { try toolbarSidebarAndContextMenus(appearance: "light") }
    func testToolbarSidebarAndContextMenusDark() throws { try toolbarSidebarAndContextMenus(appearance: "dark") }

    /// N21.3, N23.6: the title, the Cards / Notes control right after it, then
    /// the trailing group: the sync button, the search field, the Add menu.
    /// N20.6: the sidebar's field reads the system's "Search". N21.5: the
    /// row's context menu, with one row and several, in Cards and Notes.
    private func toolbarSidebarAndContextMenus(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        let mode = element(app, "browse.toolbar.mode")
        let sync = element(app, "sync.status.button")
        let add = element(app, "home.collection.add")
        let search = app.toolbars.searchFields.firstMatch.exists ? app.toolbars.searchFields.firstMatch : app.searchFields.element(boundBy: 1)
        XCTAssertTrue(mode.waitForExistence(timeout: 10) && sync.exists && add.exists && search.exists,
                      "mode \(mode.exists), sync \(sync.exists), add \(add.exists), search \(search.exists)")
        capture(app, "toolbar-\(appearance)")
        let window = app.windows.firstMatch.frame
        XCTAssertLessThan(mode.frame.midX, window.midX, "The Cards / Notes control sits after the title, not centred: \(mode.frame)")
        XCTAssertLessThan(mode.frame.maxX, sync.frame.minX, "Then the sync button")
        XCTAssertLessThan(sync.frame.maxX, search.frame.minX + 1, "Then the search field: \(sync.frame) \(search.frame)")
        XCTAssertLessThan(search.frame.maxX, add.frame.minX + 1, "Then the Add menu: \(search.frame) \(add.frame)")

        let sidebarField = app.searchFields.element(boundBy: 0)
        XCTAssertTrue(sidebarField.exists)
        XCTAssertEqual(sidebarField.placeholderValue, "Search", "N20.6: the system's own prompt")

        // Card State: Suspended finds the recipe's two suspended cards.
        element(app, "browse.sidebar.state.suspended").click()
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count == 2 }, "two suspended cards: \(table.outlineRows.count)")
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        capture(app, "suspended-selected-\(appearance)")

        // One row: Unsuspend shows, Suspend does not; the later sheets are there, disabled.
        press(table.outlineRows.element(boundBy: 0), right: true)
        let unsuspend = app.menuItems["browse.menu.unsuspend"]
        XCTAssertTrue(unsuspend.waitForExistence(timeout: 5))
        XCTAssertFalse(app.menuItems["browse.menu.suspend"].exists, "V8.1: Suspend is not shown for a suspended card")
        XCTAssertEqual(unsuspend.title, "Unsuspend Card")
        XCTAssertFalse(app.menuItems["browse.menu.changeDeck"].isEnabled, "Change Deck… waits for U276")
        XCTAssertTrue(app.menuItems["browse.menu.bury"].isEnabled)
        screen(app, "context-menu-one-row-cards-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])

        // Several rows: their counts.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        // ⌥⌘A: Select All anywhere in Browse (N21.8).
        app.typeKey("a", modifierFlags: [.command, .option])
        XCTAssertTrue(waitUntil { self.statusText(app).contains("2 selected") },
                      statusText(app))
        press(table.outlineRows.element(boundBy: 1), right: true)
        XCTAssertTrue(app.menuItems["browse.menu.unsuspend"].waitForExistence(timeout: 5))
        XCTAssertEqual(app.menuItems["browse.menu.unsuspend"].title, "Unsuspend 2 Cards")
        screen(app, "context-menu-two-rows-cards-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])

        // Notes mode: the notes' cards.
        try app.bringToFront(bundleIdentifier: bundle)
        mode.buttons.element(boundBy: 1).click()
        XCTAssertTrue(waitUntil { table.outlineRows.count >= 1 })
        press(table.outlineRows.element(boundBy: 0))
        press(table.outlineRows.element(boundBy: 0), right: true)
        XCTAssertTrue(app.menuItems["browse.menu.delete"].waitForExistence(timeout: 5))
        screen(app, "context-menu-one-row-notes-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])
        app.typeKey("a", modifierFlags: [.command, .option])
        press(table.outlineRows.element(boundBy: 0), right: true)
        XCTAssertTrue(app.menuItems["browse.menu.delete"].waitForExistence(timeout: 5))
        screen(app, "context-menu-rows-notes-\(appearance)")
        app.typeKey(.escape, modifierFlags: [])
    }

    /// V8.1 to V8.3 on the real core: Unsuspend from the context menu, the
    /// row's state symbol before and after, Edit › Undo titled "Undo
    /// Unsuspend Card", and Redo.
    func testUnsuspendAndItsUndo() throws {
        let (app, bundle) = try launch(appearance: "light")
        try openBrowse(app, bundle)
        element(app, "browse.sidebar.state.suspended").click()
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count == 2 })
        try app.bringToFront(bundleIdentifier: bundle)
        let row = table.outlineRows.element(boundBy: 0)
        press(row)
        let state = row.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'browse.table.row.'")).firstMatch
        XCTAssertTrue(waitUntil { state.label == "Suspended" }, "before: \(state.label)")
        capture(app, "before-unsuspend")
        press(row, right: true)
        let unsuspend = app.menuItems["browse.menu.unsuspend"]
        XCTAssertTrue(unsuspend.waitForExistence(timeout: 5))
        unsuspend.click()
        // H10: the row stays in the results, now not suspended.
        XCTAssertTrue(waitUntil { state.label.isEmpty }, "after: \(state.label)")
        XCTAssertEqual(table.outlineRows.count, 2)
        capture(app, "after-unsuspend")

        let edit = app.menuBars.menuBarItems["Edit"]
        edit.click()
        let undo = app.menuItems["Undo Unsuspend Card"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5), "V8.3: the step is titled after the command")
        screen(app, "edit-menu-undo-unsuspend")
        undo.click()
        XCTAssertTrue(waitUntil { state.label == "Suspended" }, "undone: \(state.label)")
        edit.click()
        let redo = app.menuItems["Redo Unsuspend Card"]
        XCTAssertTrue(redo.waitForExistence(timeout: 5))
        redo.click()
        XCTAssertTrue(waitUntil { state.label.isEmpty })

        // The Card menu's Suspend Card (⇧⌘J) suspends it again, from the keyboard.
        try app.bringToFront(bundleIdentifier: bundle)
        press(row)
        app.typeKey("j", modifierFlags: [.command, .shift])
        XCTAssertTrue(waitUntil { state.label == "Suspended" }, "⇧⌘J: \(state.label)")
    }

    /// N21.8: the Note menu between Deck and Card, and the Card menu's
    /// Browse items with their keys.
    func testNoteAndCardMenus() throws {
        let (app, bundle) = try launch(appearance: "light")
        try openBrowse(app, bundle)
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 2 })
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        let bar = app.menuBars.menuBarItems
        let titles = bar.allElementsBoundByIndex.map(\.title)
        let deck = titles.firstIndex(of: "Deck"), note = titles.firstIndex(of: "Note"), card = titles.firstIndex(of: "Card")
        XCTAssertNotNil(note, "\(titles)")
        if let deck, let note, let card { XCTAssertTrue(deck < note && note < card, "\(titles)") }
        bar["Note"].click()
        for title in ["Edit Note", "Add Tags…", "Remove Tags…", "Change Note Type…", "Edit Note Type…", "Delete Note"] {
            XCTAssertTrue(app.menuItems[title].exists, "Note › \(title)")
        }
        XCTAssertTrue(app.menuItems["Edit Note Type…"].isEnabled)
        screen(app, "note-menu")
        app.typeKey(.escape, modifierFlags: [])
        bar["Card"].click()
        for title in ["Change Deck…", "Flag", "Bury Card", "Unbury", "Suspend Card", "Unsuspend", "Set Due Date…", "Forget…"] {
            XCTAssertTrue(app.menuItems.matching(NSPredicate(format: "title BEGINSWITH %@", title)).firstMatch.exists, "Card › \(title)")
        }
        screen(app, "card-menu")
        app.typeKey(.escape, modifierFlags: [])
        bar["View"].click()
        let view = app.menuBars.menus.firstMatch.menuItems.allElementsBoundByIndex.map(\.title).filter { !$0.isEmpty }
        let order = ["Browse", "Note Types", "Cards and Notes", "Card Info"].compactMap { view.firstIndex(of: $0) }
        XCTAssertEqual(order, order.sorted(), "N21.8's View order: \(view)")
        XCTAssertEqual(order.count, 4, "\(view)")
        screen(app, "view-menu")
        app.typeKey(.escape, modifierFlags: [])
    }

    /// V8.2: two notes ask first, with Cancel and Delete; Cancel deletes nothing.
    func testDeleteQuestionLight() throws { try deleteQuestion(appearance: "light") }
    func testDeleteQuestionDark() throws { try deleteQuestion(appearance: "dark") }

    private func deleteQuestion(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        element(app, "browse.sidebar.state.suspended").click()
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count == 2 })
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        app.typeKey("a", modifierFlags: .command)
        XCTAssertTrue(waitUntil { self.statusText(app).contains("2 selected") })
        app.typeKey(.delete, modifierFlags: .command)
        let sheet = app.sheets.firstMatch
        XCTAssertTrue(sheet.waitForExistence(timeout: 10), "⌘⌫ in the table asks for two notes")
        XCTAssertTrue(sheet.staticTexts["Delete 2 Notes?"].exists || sheet.descendants(matching: .any)["Delete 2 Notes?"].exists)
        XCTAssertTrue(sheet.buttons["Cancel"].exists && sheet.buttons["Delete"].exists)
        capture(app, "delete-question-\(appearance)")
        sheet.buttons["Cancel"].click()
        XCTAssertTrue(waitUntil { !sheet.exists })
        XCTAssertEqual(table.outlineRows.count, 2, "Cancel deletes nothing")
    }

    /// V1: leaving Browse for Today and coming back keeps the table's scroll position.
    func testScrollPositionIsKept() throws {
        let (app, bundle) = try launch(appearance: "light")
        try openBrowse(app, bundle)
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 100 }, "\(table.outlineRows.count)")
        try app.bringToFront(bundleIdentifier: bundle)
        for _ in 0..<8 { table.scroll(byDeltaX: 0, deltaY: -600) }
        let first = table.outlineRows.allElementsBoundByIndex.firstIndex { $0.isHittable } ?? 0
        XCTAssertGreaterThan(first, 20, "the table scrolled down")
        let shown = table.outlineRows.element(boundBy: first + 3)
        let id = shown.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'browse.table.row.'")).firstMatch.identifier
        XCTAssertFalse(id.isEmpty)
        element(app, "shell.root.sidebar.home").click()
        XCTAssertTrue(waitUntil { !table.exists })
        element(app, "shell.root.sidebar.browse").click()
        let row = app.descendants(matching: .any)[id]
        XCTAssertTrue(waitUntil { row.exists && row.isHittable }, "row \(id) is on screen again")
        XCTAssertFalse(table.outlineRows.element(boundBy: 0).isHittable, "not back at the top")
    }

    /// N21.2, T10: Esc first closes the suggestions, then empties the query.
    func testEscapeClosesSuggestionsThenEmptiesTheQuery() throws {
        let (app, bundle) = try launch(appearance: "light")
        try openBrowse(app, bundle)
        let field = app.toolbars.searchFields.firstMatch.exists ? app.toolbars.searchFields.firstMatch : app.searchFields.element(boundBy: 1)
        try app.bringToFront(bundleIdentifier: bundle)
        field.click()
        field.typeText("wor")
        let suggestion = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'browse.search.suggestion.'")).firstMatch
        XCTAssertTrue(suggestion.waitForExistence(timeout: 10), "suggestions show")
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(waitUntil { !suggestion.exists }, "the first Esc closes the suggestions")
        XCTAssertEqual(field.value as? String, "wor", "and keeps the query")
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(waitUntil { (field.value as? String ?? "").isEmpty || (field.value as? String) == field.placeholderValue },
                      "the second Esc empties it: \(String(describing: field.value))")
    }

    /// N21.8's ⌘1 to ⌘7 (Flag) and ⌘D (Change Deck…) are in the menu bar, disabled
    /// outside Browse: Note Types' own ⌘1, ⌘2, ⌘3 (the card part) still act.
    func testNoteTypesKeysStillWork() throws {
        let (app, bundle) = try launch(appearance: "light")
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.noteTypes").click()
        let row = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'noteTypes.list.row.'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        press(row)
        element(app, "noteTypes.list.tabCards").click()
        let part = element(app, "noteTypes.cards.part")
        XCTAssertTrue(part.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("2", modifierFlags: .command)
        let back = element(app, "noteTypes.cards.part.back")
        XCTAssertTrue(waitUntil { back.isSelected || (back.value as? Int) == 1 }, "⌘2 shows the back")
        app.typeKey("3", modifierFlags: .command)
        let styling = element(app, "noteTypes.cards.part.styling")
        XCTAssertTrue(waitUntil { styling.isSelected || (styling.value as? Int) == 1 }, "⌘3 shows the styling")
        app.typeKey("1", modifierFlags: .command)
        let front = element(app, "noteTypes.cards.part.front")
        XCTAssertTrue(waitUntil { front.isSelected || (front.value as? Int) == 1 }, "⌘1 shows the front")
        capture(app, "note-types-keys")
    }

    // MARK: Support

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

    /// A row is clicked at its point: XCTest's hit test reads a SwiftUI table's
    /// rows as not hittable.
    private func press(_ row: XCUIElement, right: Bool = false) {
        let point = row.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5))
        if right { point.rightClick() } else { point.click() }
    }

    /// SwiftUI's Table is an outline in the Mac's accessibility tree.
    private func resultsTable(_ app: XCUIApplication) -> XCUIElement {
        app.outlines.matching(identifier: "browse.results.table").firstMatch
    }

    private func statusText(_ app: XCUIApplication) -> String {
        element(app, "browse.status.text").value as? String ?? ""
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// With a menu open: the VM's screen, which shows the menu over the window.
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
    func testMacOnly() throws { throw XCTSkip("Browse's commands are the Mac's in this version (U278, U279)") }
    #endif
}
