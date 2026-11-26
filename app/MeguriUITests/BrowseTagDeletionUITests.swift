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

        let tag = try revealTag(app, name: name)
        if let restoredChild { _ = try revealTag(app, name: restoredChild) }
        let grammarRow = try revealTag(app, name: "grammar")
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
        if let restoredChild { XCTAssertFalse(sidebarTag(app, name: restoredChild).exists) }
        XCTAssertTrue(grammarRow.exists, "Unrelated native tags survive deletion")
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
        guard sidebarTag(app, name: name).waitForExistence(timeout: 15) else {
            throw navigationFailure(app, "Undo did not restore the native tag row for \(name)")
        }
        if let restoredChild {
            // Undo restores the real child row, reached through its parent's
            // native triangle if the restored branch is collapsed.
            _ = try revealTag(app, name: restoredChild)
        }
        try revealTag(app, name: name).click()
        XCTAssertTrue(waitUntil { self.firstCount(app) == matching && self.status(app).contains(" of ") },
                      "Undo restores tags on the surviving notes: \(status(app))")
        capture(app.windows.firstMatch, "tag-delete-\(name)-undo-restored-\(appearance)")
    }

    private func sidebar(_ app: XCUIApplication) -> XCUIElement {
        app.outlines.matching(identifier: "browse.sidebar.list").firstMatch
    }

    private func tagIdentifier(_ name: String) -> String {
        "browse.sidebar.tag." + name.replacingOccurrences(of: ":", with: "_")
    }

    private func sidebarTag(_ app: XCUIApplication, name: String) -> XCUIElement {
        sidebar(app).descendants(matching: .any).matching(identifier: tagIdentifier(name)).firstMatch
    }

    private func revealTag(_ app: XCUIApplication, name: String) throws -> XCUIElement {
        try expandSidebarRow(app, identifier: "browse.sidebar.tags")
        let components = name.components(separatedBy: "::")
        for depth in 1..<components.count {
            try expandSidebarRow(app, identifier: tagIdentifier(components.prefix(depth).joined(separator: "::")))
        }
        let tag = sidebarTag(app, name: name)
        try reveal(app, target: tag, identifier: tagIdentifier(name))
        return tag
    }

    private func expandSidebarRow(_ app: XCUIApplication, identifier: String) throws {
        let rows = sidebar(app).outlineRows.containing(.any, identifier: identifier)
        // AppKit may not expose an offscreen row until it has been scrolled in.
        for _ in 0..<8 where !rows.firstMatch.exists {
            let list = sidebar(app)
            list.scroll(byDeltaX: 0, deltaY: -list.frame.height / 2)
        }
        guard rows.firstMatch.waitForExistence(timeout: 10), rows.count == 1 else {
            throw navigationFailure(app, "Expected one native sidebar row for \(identifier)")
        }
        // MacHomeHierarchyTests uses this row/cell-scoped system identifier.
        // Never click the header's Other: it is the label, not the disclosure.
        let controls = rows.firstMatch.cells.descendants(matching: .any)
            .matching(identifier: "NSOutlineViewDisclosureButtonKey")
        guard controls.count == 1 else {
            throw navigationFailure(app, "Expected one native disclosure in \(identifier)")
        }
        let control = controls.firstMatch
        guard [.disclosureTriangle, .button].contains(control.elementType) else {
            throw navigationFailure(app, "Unexpected disclosure control type in \(identifier)")
        }
        try reveal(app, target: control, identifier: identifier)
        guard let expanded = expansion(control) else {
            throw navigationFailure(app, "Unknown native expansion state in \(identifier)")
        }
        if !expanded {
            control.click()
            guard waitUntil(timeout: 10, { self.expansion(control) == true }) else {
                throw navigationFailure(app, "Native disclosure did not expand \(identifier)")
            }
        }
    }

    private func expansion(_ control: XCUIElement) -> Bool? {
        if let value = control.value as? String {
            if value == "1" { return true }
            if value == "0" { return false }
        }
        if let value = control.value as? NSNumber {
            if value == NSNumber(value: 1) { return true }
            if value == NSNumber(value: 0) { return false }
        }
        // Native section disclosure buttons can expose their action instead
        // of a numeric triangle value.
        if control.elementType == .button {
            if control.label == "Hide" || control.title == "Hide" { return true }
            if control.label == "Show" || control.title == "Show" { return false }
        }
        return nil
    }

    private func reveal(_ app: XCUIApplication, target: XCUIElement, identifier: String) throws {
        let list = sidebar(app)
        // Tags is the last section. Scroll only this native sidebar, leaving
        // the table and inspector alone; these are navigation steps, not retries.
        for _ in 0..<8 {
            if target.exists && target.isHittable && target.isEnabled { return }
            list.scroll(byDeltaX: 0, deltaY: -list.frame.height / 2)
        }
        guard target.exists && target.isHittable && target.isEnabled else {
            throw navigationFailure(app, "Could not reveal native sidebar control \(identifier)")
        }
    }

    private func navigationFailure(_ app: XCUIApplication, _ message: String) -> NSError {
        // A window snapshot includes the actual sidebar, controls and search
        // ancestry without exporting the system menu bar's recent-file menus.
        var lines = [message]
        func walk(_ node: XCUIElementSnapshot, depth: Int) {
            lines.append(String(repeating: "  ", count: depth)
                + "\(node.elementType) id=\(node.identifier) label=\(node.label) title=\(node.title)"
                + " value=\(String(describing: node.value)) enabled=\(node.isEnabled) frame=\(node.frame)")
            for child in node.children { walk(child, depth: depth + 1) }
        }
        do { walk(try app.windows.firstMatch.snapshot(), depth: 0) }
        catch { lines.append("Snapshot failed: \(error)") }
        let hierarchy = XCTAttachment(string: lines.joined(separator: "\n"))
        hierarchy.name = "tag-delete-navigation-hierarchy.txt"
        hierarchy.lifetime = .keepAlways
        add(hierarchy)
        capture(app.windows.firstMatch, "tag-delete-navigation-failure")
        return NSError(domain: "BrowseTagDeletionUITests.Navigation", code: 1,
                       userInfo: [NSLocalizedDescriptionKey: message])
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
}
#endif
