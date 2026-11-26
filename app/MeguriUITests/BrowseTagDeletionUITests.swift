import Foundation
import XCTest

/// Real native menus and tag jobs over the existing PUBLIC typical recipe.
/// No sample route, injected mutation, new fixture or media is used.
#if os(macOS)
@MainActor
final class BrowseTagDeletionUITests: XCTestCase {
    func testSidebarDeleteLeafKeepsCardsAndUndoRestoresTag() throws {
        try deleteAndUndo(name: "es", mode: "Cards", appearance: "light")
    }

    func testSidebarDeleteImplicitParentKeepsNotesAndUndoRestoresChildren() throws {
        try deleteAndUndo(name: "jlpt", mode: "Notes", appearance: "dark", restoredChild: "jlpt::n4")
    }

    private func deleteAndUndo(name: String, mode: String, appearance: String, restoredChild: String? = nil) throws {
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
            appearance: appearance).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 30),
                      "Generate the existing fixture with tools/mg fixtures typical --seed 1")
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.browse").click()
        XCTAssertTrue(element(app, "browse.toolbar.mode").waitForExistence(timeout: 20))
        XCTAssertTrue(waitUntil { self.firstCount(app) > 0 })
        let picker = element(app, "browse.toolbar.mode")
        let choice = picker.descendants(matching: .any).matching(
            NSPredicate(format: "label == %@ OR title == %@ OR value == %@", mode, mode, mode)).firstMatch
        XCTAssertTrue(choice.exists)
        choice.click()
        XCTAssertTrue(waitUntil { self.firstCount(app) > 0 && self.status(app).localizedCaseInsensitiveContains(mode) && !self.status(app).contains("Searching") })
        let whole = firstCount(app)

        let search = try MacBrowseSidebar(app: app).searchField()
        search.click()
        search.typeText("Tags") // The localized section root for this en_US launch.
        XCTAssertTrue(waitUntil { search.value as? String == "Tags" })
        // A matching section root shows top-level tags; children must match themselves.
        let tag = sidebarTag(app, name: name)
        let grammarRow = sidebarTag(app, name: "grammar")
        XCTAssertTrue(tag.waitForExistence(timeout: 10) && tag.isHittable)
        XCTAssertTrue(grammarRow.waitForExistence(timeout: 10) && grammarRow.isHittable)
        if let restoredChild {
            filterSidebar(search, to: "n4")
            let child = sidebarTag(app, name: restoredChild)
            XCTAssertTrue(child.waitForExistence(timeout: 10) && child.isHittable)
            XCTAssertTrue(tag.exists && tag.isHittable, "A matching child reveals its implicit parent")
        }
        tag.click()
        XCTAssertTrue(waitUntil { self.status(app).contains(" of ") && self.firstCount(app) > 0 })
        let matching = firstCount(app)
        XCTAssertLessThan(matching, whole, "The selected tag matches a proper subset of the collection")
        capture(app.windows.firstMatch, "tag-delete-\(name)-before-\(appearance)")

        // Open the real list context menu on the actual tag; the action must
        // be visible and enabled before its menu is captured and invoked.
        try app.bringToFront(bundleIdentifier: bundle)
        tag.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).rightClick()
        let action = app.menuItems.matching(NSPredicate(format: "identifier == 'browse.sidebar.tag.delete' OR title == 'Delete Tag'"))
            .firstMatch
        XCTAssertTrue(action.waitForExistence(timeout: 5))
        XCTAssertTrue(action.isEnabled)
        let menu = app.menus.containing(NSPredicate(format: "identifier == 'browse.sidebar.tag.delete' OR title == 'Delete Tag'")).firstMatch
        XCTAssertTrue(menu.exists, "The open native tag menu is captured as an element")
        captureMenu(menu, "tag-delete-\(name)-native-menu-\(appearance)")
        // AppKit's menu-item click can outlast the transient success toast.
        action.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()

        let toast = element(app, "browse.tag.deleted")
        XCTAssertTrue(toast.waitForExistence(timeout: 5))
        // es has one card per note; jlpt is tested in Notes mode.
        let toastTexts = [toast.value as? String, toast.title, toast.label].compactMap { $0 }
        let expectedCount = matching.formatted(.number.locale(Locale(identifier: "en_US")))
        XCTAssertTrue(toastTexts.contains { $0.contains(expectedCount) },
                      "The toast counts notes actually updated: \(toastTexts)")
        XCTAssertTrue(element(app, "browse.empty.noResults").waitForExistence(timeout: 15),
                      "The active tag query is rerun after deletion")
        XCTAssertTrue(waitUntil { !tag.exists }, "The registry tree reloads, including implicit parents")
        if let restoredChild {
            XCTAssertFalse(sidebarTag(app, name: restoredChild).exists)
            filterSidebar(search, to: "Tags")
        }
        XCTAssertTrue(grammarRow.waitForExistence(timeout: 10), "Unrelated native tags survive deletion")
        XCTAssertFalse(tag.exists, "The deleted tag also stays absent under the matching section root")
        XCTAssertTrue(element(app, "browse.search.token.0").exists, "The active query stays available for editing")
        capture(app.windows.firstMatch, "tag-delete-\(name)-outcome-\(appearance)")

        element(app, "browse.filterBar.clear").click()
        XCTAssertTrue(waitUntil { self.firstCount(app) == whole && !self.status(app).contains(" of ") },
                      "Deleting a tag preserves the collection's \(mode.lowercased()): \(status(app))")
        capture(app.windows.firstMatch, "tag-delete-\(name)-surviving-\(mode.lowercased())-\(appearance)")

        // Clear can leave a search field's NSTextView owning Edit > Undo.
        // Give the native results table the keys before checking collection Undo,
        // using an existing row without editing a note or changing the query.
        let table = app.outlines.matching(identifier: "browse.results.table").firstMatch
        let row = table.outlineRows.element(boundBy: 0)
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        // SwiftUI's outline row itself can be non-hittable; use its visible area.
        let tableBounds = table.frame
        let visibleRow = row.frame.intersection(tableBounds)
        XCTAssertFalse(visibleRow.isNull || visibleRow.isEmpty, "An existing result must be visible")
        table.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: visibleRow.midX - tableBounds.minX, dy: visibleRow.midY - tableBounds.minY)).click()
        XCTAssertTrue(waitUntil { row.isSelected }, "The native results table receives the click")

        app.menuBars.menuBarItems["Edit"].click()
        let undoCaption = NSPredicate(format: "title == 'Undo Delete Tag' OR label == 'Undo Delete Tag'")
        let undo = app.menuItems.matching(undoCaption).firstMatch
        XCTAssertTrue(undo.waitForExistence(timeout: 5))
        XCTAssertTrue(undo.isEnabled)
        captureMenu(app.menus.containing(undoCaption).firstMatch,
                "tag-delete-\(name)-undo-menu-\(appearance)")
        undo.click()
        XCTAssertTrue(tag.waitForExistence(timeout: 15), "Undo restores the native tag row")
        if let restoredChild {
            filterSidebar(search, to: "n4")
            let child = sidebarTag(app, name: restoredChild)
            XCTAssertTrue(child.waitForExistence(timeout: 15) && child.isHittable)
            XCTAssertTrue(tag.exists && tag.isHittable, "Undo restores the implicit parent with its child")
            filterSidebar(search, to: "Tags")
        }
        XCTAssertTrue(grammarRow.waitForExistence(timeout: 10), "Undo preserves the unrelated tag")
        XCTAssertTrue(tag.isHittable)
        tag.click()
        XCTAssertTrue(waitUntil { self.firstCount(app) == matching && self.status(app).contains(" of ") },
                      "Undo restores tags on the surviving notes: \(status(app))")
        capture(app.windows.firstMatch, "tag-delete-\(name)-undo-restored-\(appearance)")
    }

    private func filterSidebar(_ search: XCUIElement, to text: String) {
        search.click()
        search.typeKey("a", modifierFlags: .command)
        search.typeText(text)
        XCTAssertTrue(waitUntil { search.value as? String == text })
    }

    private func sidebarTag(_ app: XCUIApplication, name: String) -> XCUIElement {
        // FilteredTagBranch's disclosure can inherit its row identifier;
        // the real row combines that identifier with the literal leaf label.
        let identifier = "browse.sidebar.tag." + name.replacingOccurrences(of: ":", with: "_")
        let label = name.components(separatedBy: "::").last ?? name
        return app.outlines.matching(identifier: "browse.sidebar.list").firstMatch
            .descendants(matching: .any).matching(NSPredicate(
                format: "identifier == %@ AND label == %@", identifier, label)).firstMatch
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func status(_ app: XCUIApplication) -> String {
        let status = element(app, "browse.status.text")
        return status.value as? String ?? status.label
    }

    private func firstCount(_ app: XCUIApplication) -> Int {
        let value = status(app).prefix { $0.isNumber || $0 == "," }
        return Int(value.filter(\.isNumber)) ?? 0
    }

    private func captureMenu(_ menu: XCUIElement, _ name: String) {
        capture(menu, name)
        // In the test VM, retain the open native menu together with its window.
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name + "-window.png"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func capture(_ element: XCUIElement, _ name: String) {
        let attachment = XCTAttachment(screenshot: element.screenshot())
        attachment.name = name + ".png"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func waitUntil(timeout: TimeInterval = 15, _ condition: () -> Bool) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return condition()
    }
}
#endif
