import Foundation
import XCTest

/// Real native menus and tag jobs over the existing PUBLIC typical recipe.
/// No sample route, injected mutation, new fixture or media is used.
@MainActor
final class BrowseTagDeletionUITests: XCTestCase {
    #if os(macOS)
    func testSidebarDeleteLeafKeepsCardsAndUndoRestoresTag() throws {
        try deleteAndUndo(name: "es", mode: "Cards", appearance: "light")
    }

    func testSidebarDeleteImplicitParentKeepsNotesAndUndoRestoresChildren() throws {
        try deleteAndUndo(name: "jlpt", mode: "Notes", appearance: "dark", restoredChild: "jlpt__n4")
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

        // Filtering the sidebar reveals Tags without relying on its saved fold.
        let sidebarSearch = app.searchFields.element(boundBy: 0)
        sidebarSearch.click()
        sidebarSearch.typeText(name)
        let tag = element(app, "browse.sidebar.tag.\(name)")
        XCTAssertTrue(tag.waitForExistence(timeout: 10))
        tag.click()
        XCTAssertTrue(waitUntil { self.status(app).contains(" of ") && self.firstCount(app) > 0 })
        let matching = firstCount(app)
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
        capture(menu, "tag-delete-\(name)-native-menu-\(appearance)")
        action.click()

        let toast = element(app, "browse.tag.deleted")
        XCTAssertTrue(toast.waitForExistence(timeout: 5))
        // es has one card per note; jlpt is tested in Notes mode.
        XCTAssertTrue(toast.label.contains(matching.formatted(.number.locale(Locale(identifier: "en_US")))),
                      "The toast counts notes actually updated: \(toast.label)")
        XCTAssertTrue(element(app, "browse.empty.noResults").waitForExistence(timeout: 15),
                      "The active tag query is rerun after deletion")
        XCTAssertTrue(waitUntil { !tag.exists }, "The registry tree reloads, including implicit parents")
        XCTAssertTrue(element(app, "browse.search.token.0").exists, "The active query stays available for editing")
        capture(app.windows.firstMatch, "tag-delete-\(name)-outcome-\(appearance)")

        element(app, "browse.filterBar.clear").click()
        XCTAssertTrue(waitUntil { self.firstCount(app) == whole && !self.status(app).contains(" of ") },
                      "Deleting a tag preserves the collection's \(mode.lowercased()): \(status(app))")
        capture(app.windows.firstMatch, "tag-delete-\(name)-surviving-\(mode.lowercased())-\(appearance)")

        app.menuBars.menuBarItems["Edit"].click()
        let undo = app.menuItems["Undo Delete Tag"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5))
        XCTAssertTrue(undo.isEnabled)
        capture(app.menus.containing(NSPredicate(format: "title == 'Undo Delete Tag'")).firstMatch,
                "tag-delete-\(name)-undo-menu-\(appearance)")
        undo.click()
        XCTAssertTrue(tag.waitForExistence(timeout: 15))
        if let restoredChild {
            // Filter by the child's own label: S6 matches labels, not full names.
            sidebarSearch.click()
            app.typeKey("a", modifierFlags: .command)
            sidebarSearch.typeText("n4")
            XCTAssertTrue(element(app, "browse.sidebar.tag.\(restoredChild)").waitForExistence(timeout: 10))
        }
        tag.click()
        XCTAssertTrue(waitUntil { self.firstCount(app) == matching && self.status(app).contains(" of ") },
                      "Undo restores tags on the surviving notes: \(status(app))")
        capture(app.windows.firstMatch, "tag-delete-\(name)-undo-restored-\(appearance)")
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
    #else
    func testMacNativeMenus() throws {
        throw XCTSkip("The product shell currently exposes Browse only on macOS; compact rollout remains a shell unit")
    }
    #endif
}
