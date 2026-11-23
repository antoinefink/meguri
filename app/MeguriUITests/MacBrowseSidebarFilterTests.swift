import Foundation
import XCTest

/// Browse's sidebar as a view of the query on the Mac (`search/filter-tokens`
/// T9, decided 2026-11-23; `browser/workspace` V6, V1), over the generated
/// typical recipe on the real core: a second click on a selected row removes
/// its filter, Return does the same, a saved search shows selected while the
/// query is its own, emptying the field deselects at once, and the table's
/// first row starts below its column header, also when Browse is shown again.
@MainActor
final class MacBrowseSidebarFilterTests: XCTestCase {
    #if os(macOS)
    func testASecondClickRemovesTheFilterAndTheRowsShowTheQuery() throws {
        let (app, bundle) = try launch()
        try openBrowse(app, bundle)
        let whole = statusText(app)
        XCTAssertFalse(whole.contains(" of "), "the whole collection first: \(whole)")

        // A click: New is the query's one Card State value, and shows selected.
        let new = element(app, "browse.sidebar.state.new")
        new.click()
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, statusText(app))
        XCTAssertTrue(waitUntil { self.selected(app, "browse.sidebar.state.new") }, "New shows selected")
        capture(app, "sidebar-new-selected")
        // The same row again: the filter goes, and so does the highlight.
        new.click()
        XCTAssertTrue(waitUntil { self.statusText(app) == whole }, statusText(app))
        XCTAssertFalse(selected(app, "browse.sidebar.state.new"))
        capture(app, "sidebar-new-removed")

        // Space, then Return, on the one selected row: the same as a second click.
        let readout = element(app, "browse.debug.responder")
        // Both keys are reported, then the case stops at a failure again.
        continueAfterFailure = true
        for (name, key) in [("Space", " "), ("Return", XCUIKeyboardKey.return.rawValue)] {
            new.click()
            XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, statusText(app))
            app.typeKey(key, modifierFlags: [])
            XCTAssertTrue(waitUntil { self.statusText(app) == whole },
                          "\(name) removes it: \(statusText(app)); keys at \(readout.exists ? readout.label : "unknown")")
        }
        continueAfterFailure = false

        // A saved search whose token no section lists shows selected itself.
        element(app, "browse.sidebar.saved.dueToday").click()
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, statusText(app))
        XCTAssertTrue(waitUntil { self.selected(app, "browse.sidebar.saved.dueToday") }, "Due Today shows selected")
        capture(app, "sidebar-due-today-selected")
        // Emptying the field (⌘A, ⌫): the row is deselected at once.
        let field = app.toolbars.searchFields.firstMatch.exists ? app.toolbars.searchFields.firstMatch : app.searchFields.element(boundBy: 1)
        field.click()
        app.typeKey("a", modifierFlags: .command)
        app.typeKey(.delete, modifierFlags: [])
        XCTAssertTrue(waitUntil { self.statusText(app) == whole }, statusText(app))
        XCTAssertFalse(selected(app, "browse.sidebar.saved.dueToday"), "the field emptied deselects Due Today")
        capture(app, "sidebar-field-emptied")
    }

    /// The first row sits below the column header after a sidebar click and
    /// after Browse is shown again (owner's Mac, 2026-11-23: it was half
    /// under the header).
    func testTheFirstRowStartsBelowTheColumnHeader() throws {
        let (app, bundle) = try launch()
        try openBrowse(app, bundle)
        let table = app.outlines.matching(identifier: "browse.results.table").firstMatch
        element(app, "browse.sidebar.state.review").click()
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, statusText(app))
        assertFirstRowBelowHeader(app, table, "after a sidebar click")
        capture(app, "first-row-after-click")
        element(app, "shell.root.sidebar.home").click()
        XCTAssertTrue(waitUntil { !table.exists })
        element(app, "shell.root.sidebar.browse").click()
        XCTAssertTrue(table.waitForExistence(timeout: 20))
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        assertFirstRowBelowHeader(app, table, "after Browse is shown again")
        capture(app, "first-row-after-return")
    }

    private func assertFirstRowBelowHeader(_ app: XCUIApplication, _ table: XCUIElement, _ when: String) {
        let first = table.outlineRows.element(boundBy: 0)
        XCTAssertTrue(waitUntil { first.exists }, "a first row \(when)")
        let header = app.buttons.matching(NSPredicate(format: "label == 'Sort Field' OR title == 'Sort Field'")).firstMatch
        let toolbar = app.toolbars.firstMatch
        if header.exists {
            XCTAssertGreaterThanOrEqual(first.frame.minY, header.frame.maxY - 1,
                                        "the first row starts below the header \(when): \(first.frame) \(header.frame)")
        } else {
            XCTAssertGreaterThanOrEqual(first.frame.minY, toolbar.frame.maxY + 20,
                                        "the first row starts below the toolbar and a header \(when): \(first.frame) \(toolbar.frame)")
        }
    }

    // MARK: Support

    private func launch() throws -> (XCUIApplication, String) {
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
            appearance: "light").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 30),
                      "Stage the generated typical-1 fixture through tools/mg")
        return (app, bundle)
    }

    private func openBrowse(_ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.browse").click()
        XCTAssertTrue(app.outlines.matching(identifier: "browse.results.table").firstMatch.waitForExistence(timeout: 20))
        XCTAssertTrue(waitUntil { !self.statusText(app).isEmpty })
    }

    /// The sidebar row holding `id` is selected in the list.
    private func selected(_ app: XCUIApplication, _ id: String) -> Bool {
        let list = app.outlines.matching(identifier: "browse.sidebar.list").firstMatch
        let row = list.outlineRows.containing(.any, identifier: id).firstMatch
        return row.exists && row.isSelected
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
    func testMacOnly() throws { throw XCTSkip("The sidebar's clicks are tested on the Mac in this version") }
    #endif
}
