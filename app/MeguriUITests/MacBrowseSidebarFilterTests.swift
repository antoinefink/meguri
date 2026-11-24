import Foundation
import XCTest

/// Browse's sidebar as a view of the query on the Mac (`search/filter-tokens`
/// T9, decided 2026-11-23; `browser/workspace` V6, V1), over the generated
/// typical recipe on the real core: a second click on a selected row removes
/// its filter, Return does the same, a saved search shows selected while the
/// query is its own, the filter bar's Clear deselects at once, and the table's
/// first row starts below its column header, also when Browse is shown again.
/// N21.1, N21.2 (2026-11-24): a selected filter row is selected to
/// accessibility while the list's own selection stays on the Browse row, and
/// the query's tokens show in the filter bar over the table.
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
        XCTAssertTrue(listSelected(app, "shell.root.sidebar.browse"), "the list's own selection stays on Browse")
        XCTAssertFalse(listSelected(app, "browse.sidebar.state.new"), "a filter row draws its own selected state")
        // The token shows in the filter bar, with Filter, Save Search… and Clear.
        let token = element(app, "browse.search.token.0")
        XCTAssertTrue(token.waitForExistence(timeout: 10), "the Card State token in the filter bar")
        XCTAssertTrue(element(app, "browse.filterBar.add").exists && element(app, "browse.filterBar.save").exists
            && element(app, "browse.filterBar.clear").exists, "Filter, Save Search… and Clear")
        let table = app.outlines.matching(identifier: "browse.results.table").firstMatch
        XCTAssertLessThanOrEqual(token.frame.maxY, table.frame.minY + 1, "the bar is over the table: \(token.frame) \(table.frame)")
        capture(app, "sidebar-new-selected")
        // The same row again: the filter goes, and so does the highlight.
        new.click()
        XCTAssertTrue(waitUntil { self.statusText(app) == whole }, statusText(app))
        XCTAssertFalse(selected(app, "browse.sidebar.state.new"))
        XCTAssertTrue(waitUntil { !token.exists }, "no token, no filter bar")
        capture(app, "sidebar-new-removed")

        // A saved search whose token no section lists shows selected itself.
        element(app, "browse.sidebar.saved.dueToday").click()
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, statusText(app))
        XCTAssertTrue(waitUntil { self.selected(app, "browse.sidebar.saved.dueToday") }, "Due Today shows selected")
        capture(app, "sidebar-due-today-selected")
        // The filter bar's Clear: the row is deselected at once.
        element(app, "browse.filterBar.clear").click()
        XCTAssertTrue(waitUntil { self.statusText(app) == whole }, statusText(app))
        XCTAssertFalse(selected(app, "browse.sidebar.saved.dueToday"), "Clear deselects Due Today")
        capture(app, "sidebar-cleared")
    }

    /// T9: Return and Space on the one selected filter row are a second click,
    /// each from a fresh launch so one result does not decide the other.
    /// The model's rule is `BrowseModelTests` (`activateByKey`).
    func testReturnOnTheSelectedRowRemovesItsFilter() throws { try keyRemovesTheFilter(XCUIKeyboardKey.return.rawValue, "Return") }
    func testSpaceOnTheSelectedRowRemovesItsFilter() throws { try keyRemovesTheFilter(" ", "Space") }

    private func keyRemovesTheFilter(_ key: String, _ name: String) throws {
        let (app, bundle) = try launch()
        try openBrowse(app, bundle)
        let whole = statusText(app)
        element(app, "browse.sidebar.state.new").click()
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, statusText(app))
        app.typeKey(key, modifierFlags: [])
        // Debug builds: the view that has the keys after the click.
        let readout = element(app, "browse.debug.responder")
        XCTAssertTrue(waitUntil { self.statusText(app) == whole },
                      "\(name) removes it: \(statusText(app)); keys at: \(readout.exists ? readout.label : "unknown")")
    }

    /// T9: the arrow keys move through the rows without changing the query,
    /// and a click on the row they stand on is a click on it all the same.
    func testAClickOnTheRowTheArrowKeysStandOnFiltersByIt() throws {
        let (app, bundle) = try launch()
        try openBrowse(app, bundle)
        let whole = statusText(app)
        try arrowDownToDueToday(app, whole)
        element(app, "browse.sidebar.saved.dueToday").click()
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, "the click filters: \(statusText(app))")
        XCTAssertTrue(waitUntil { self.selected(app, "browse.sidebar.saved.dueToday") }, "Due Today shows selected")
        XCTAssertTrue(waitUntil { self.listSelected(app, "shell.root.sidebar.browse") }, "the list's selection is back on Browse")
    }

    /// T9: a ⌘-click after the arrow keys toggles the clicked row alone; the
    /// row the keys stood on was not clicked.
    func testACommandClickAfterTheArrowKeysTogglesOnlyTheClickedRow() throws {
        let (app, bundle) = try launch()
        try openBrowse(app, bundle)
        let whole = statusText(app)
        try arrowDownToDueToday(app, whole)
        XCUIElement.perform(withKeyModifiers: .command) { element(app, "browse.sidebar.state.new").click() }
        XCTAssertTrue(waitUntil { self.statusText(app).contains(" of ") }, "the ⌘-click filters: \(statusText(app))")
        XCTAssertTrue(waitUntil { self.selected(app, "browse.sidebar.state.new") }, "New shows selected")
        XCTAssertFalse(selected(app, "browse.sidebar.saved.dueToday"), "Due Today was not clicked")
        XCTAssertTrue(element(app, "browse.search.token.0").waitForExistence(timeout: 10), "New's token")
        XCTAssertFalse(element(app, "browse.search.token.1").exists, "one token only")
    }

    /// Down from the Browse row: the keys stand on Due Today, the query unchanged.
    private func arrowDownToDueToday(_ app: XCUIApplication, _ whole: String) throws {
        app.typeKey(.downArrow, modifierFlags: [])
        XCTAssertTrue(waitUntil { self.listSelected(app, "browse.sidebar.saved.dueToday") }, "the keys stand on Due Today")
        XCTAssertEqual(statusText(app), whole, "the arrow keys do not change the query")
        XCTAssertFalse(selected(app, "browse.sidebar.saved.dueToday"), "Due Today is not a filter yet")
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

    /// The filter row `id` reads as selected: the query holds its value (T9).
    private func selected(_ app: XCUIApplication, _ id: String) -> Bool {
        let row = element(app, id)
        return row.exists && row.isSelected
    }

    /// The sidebar row holding `id` carries the list's own selection.
    private func listSelected(_ app: XCUIApplication, _ id: String) -> Bool {
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
