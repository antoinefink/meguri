import Foundation
import XCTest

/// S9.3: actual native Delete dispatch over the existing public typical recipe.
/// Kept separate from the context-menu tag-deletion suite.
@MainActor final class MacBrowseSidebarDeleteKeyTests: XCTestCase {
    #if os(macOS)
    func testBackspaceDeletesTheClickedTagWithoutAQuestionAndOrdinaryUndoRestoresIt() throws {
        let (app, bundle, whole) = try launch()
        let tag = element(app, "browse.sidebar.tag.es")
        try click(app, tag, bundle)
        XCTAssertTrue(waitUntil { self.status(app).contains(" of ") && self.count(app) > 0 })
        let matching = count(app)
        try key(app, .delete, bundle)
        XCTAssertTrue(element(app, "browse.tag.deleted").waitForExistence(timeout: 10), "the List's native Delete command ran")
        XCTAssertFalse(app.sheets.firstMatch.exists, "tag deletion has no confirmation")
        XCTAssertTrue(waitUntil { !tag.exists })
        XCTAssertTrue(element(app, "browse.empty.noResults").waitForExistence(timeout: 15))

        try click(app, element(app, "browse.filterBar.clear"), bundle)
        XCTAssertTrue(waitUntil { self.count(app) == whole && !self.status(app).contains(" of ") }, "all cards survive")
        try click(app, app.menuBars.menuBarItems["Edit"], bundle)
        let undo = app.menuItems["Undo Delete Tag"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5) && undo.isEnabled)
        try click(app, undo, bundle)
        XCTAssertTrue(tag.waitForExistence(timeout: 15))
        try click(app, tag, bundle)
        XCTAssertTrue(waitUntil { self.count(app) == matching && self.status(app).contains(" of ") })
    }

    func testSearchEditorTableAndBrowseRowKeepDeleteAwayFromTheRetainedTagSelection() throws {
        let (app, bundle, whole) = try launch()
        let tag = element(app, "browse.sidebar.tag.es")
        try click(app, tag, bundle)
        XCTAssertTrue(waitUntil { self.status(app).contains(" of ") && self.count(app) > 0 })
        let matching = count(app)
        let table = app.outlines.matching(identifier: "browse.results.table").firstMatch
        XCTAssertTrue(table.outlineRows.firstMatch.waitForExistence(timeout: 10))
        try click(app, table.outlineRows.firstMatch, bundle)
        try key(app, .delete, bundle)
        assertTagSurvives(app, tag, count: matching)

        if !element(app, "editor.fields").exists {
            try click(app, element(app, "browse.toolbar.inspector"), bundle)
        }
        let field = element(app, "editor.fields").textViews.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        try click(app, field, bundle)
        try key(app, "a", bundle, modifiers: .command)
        try app.bringToFront(bundleIdentifier: bundle)
        field.typeText("Synthetic keyboard focusZ")
        try key(app, .delete, bundle)
        XCTAssertTrue(waitUntil { field.value as? String == "Synthetic keyboard focus" }, "the editor receives Backspace")
        assertTagSurvives(app, tag, count: matching)

        // Restore sidebar focus, then move it to the sidebar's native search.
        try click(app, tag, bundle)
        XCTAssertTrue(waitUntil { self.count(app) == whole && !self.status(app).contains(" of ") })
        try click(app, tag, bundle)
        XCTAssertTrue(waitUntil { self.count(app) == matching && self.status(app).contains(" of ") })
        let search = app.searchFields.element(boundBy: 0)
        try click(app, search, bundle)
        try key(app, "a", bundle, modifiers: .command)
        try key(app, .delete, bundle)
        XCTAssertTrue(waitUntil { (search.value as? String ?? "").isEmpty || search.value as? String == search.placeholderValue },
                      "the search field receives Backspace")
        try app.bringToFront(bundleIdentifier: bundle)
        search.typeText("es")
        XCTAssertTrue(tag.waitForExistence(timeout: 10))
        assertTagSurvives(app, tag, count: matching)

        // Browse remains the List's restored selection; clicking that group
        // must clear the acted-on tag even though the query still highlights it.
        try click(app, tag, bundle)
        XCTAssertTrue(waitUntil { self.count(app) == whole && !self.status(app).contains(" of ") })
        try click(app, tag, bundle)
        XCTAssertTrue(waitUntil { self.count(app) == matching && self.status(app).contains(" of ") })
        try click(app, element(app, "shell.root.sidebar.browse"), bundle)
        try key(app, .delete, bundle)
        assertTagSurvives(app, tag, count: matching)
    }

    private func launch() throws -> (XCUIApplication, String, Int) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("The public synthetic recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1", directoryHint: .isDirectory),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            appearance: "light").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let browse = element(app, "shell.root.sidebar.browse")
        XCTAssertTrue(browse.waitForExistence(timeout: 30), "Stage typical-1 through tools/mg fixtures")
        try click(app, browse, bundle)
        XCTAssertTrue(waitUntil { self.count(app) > 0 && !self.status(app).contains("Searching") })
        let whole = count(app)
        let search = app.searchFields.element(boundBy: 0)
        try click(app, search, bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        search.typeText("es")
        XCTAssertTrue(element(app, "browse.sidebar.tag.es").waitForExistence(timeout: 10))
        return (app, bundle, whole)
    }

    private func assertTagSurvives(_ app: XCUIApplication, _ tag: XCUIElement, count expected: Int) {
        XCTAssertTrue(tag.exists)
        XCTAssertFalse(element(app, "browse.tag.deleted").exists)
        XCTAssertFalse(app.sheets.firstMatch.exists)
        XCTAssertTrue(waitUntil { self.count(app) == expected }, "neither tags nor notes were deleted")
    }

    private func click(_ app: XCUIApplication, _ target: XCUIElement, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        XCTAssertTrue(target.waitForExistence(timeout: 5) && target.isHittable)
        target.click()
    }

    private func key(_ app: XCUIApplication, _ key: XCUIKeyboardKey, _ bundle: String,
                     modifiers: XCUIKeyModifierFlags = []) throws {
        try self.key(app, key.rawValue, bundle, modifiers: modifiers)
    }

    private func key(_ app: XCUIApplication, _ key: String, _ bundle: String,
                     modifiers: XCUIKeyModifierFlags = []) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(key, modifierFlags: modifiers)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func status(_ app: XCUIApplication) -> String {
        let value = element(app, "browse.status.text")
        return value.value as? String ?? value.label
    }

    private func count(_ app: XCUIApplication) -> Int {
        Int(status(app).prefix { $0.isNumber || $0 == "," }.filter(\.isNumber)) ?? 0
    }

    private func waitUntil(timeout: TimeInterval = 15, _ condition: () -> Bool) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return condition()
    }
    #else
    func testMacOnly() throws { throw XCTSkip("Native Browse Delete routing is a macOS unit") }
    #endif
}
