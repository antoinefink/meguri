import Foundation
import XCTest

#if os(macOS)
/// W12/S12 in the actual native workspace, using MG's existing typical-1
/// recipe. Run only in the macOS test VM; screenshots are native elements.
@MainActor
final class NoteTypeDeletionUITests: XCTestCase {
    // The three typical-1 fixture rows, confirmed in Mac VM run 064156Z.
    // The list's StaticText has a truncated value and no label on macOS.
    private let reverseID = "1884308400001"
    private let basicID = "1884308400000"
    private let clozeID = "1884308400002"
    private let reverseName = "Meguri Basic and Reverse"
    private let basicWindowTitle = "Meguri Basic – 178 notes · 178 cards"
    private let reverseWindowTitle = "Meguri Basic and Reverse – 90 notes · 180 cards"

    func testListDeleteConfirmsRealCountsCancelsDeletesAndUndoes() throws {
        let (app, bundle) = try launch()
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.noteTypes").click()
        let target = noteTypeRow(app, id: reverseID)
        XCTAssertTrue(target.waitForExistence(timeout: 20))
        let identifier = target.identifier
        let remaining = noteTypeRow(app, id: basicID)
        XCTAssertTrue(remaining.exists)
        XCTAssertTrue(noteTypeRow(app, id: clozeID).exists)
        XCTAssertEqual(listRows(app).count, 3, "The typical-1 recipe has exactly three note types")
        select(remaining, in: app, windowTitle: basicWindowTitle)
        select(target, in: app, windowTitle: reverseWindowTitle)
        capture(app.windows.firstMatch, "note-type-delete-list-before")

        let action = try openDeleteMenu(app, row: target, identifier: "noteTypes.list.delete",
                                        title: "Delete…", captureName: "note-type-delete-native-row-menu")
        action.click()
        assertCounts(app)
        capture(confirmation(app), "noteTypes.delete.sheet")

        // The native alert's default Return is Cancel, never Delete.
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(confirmation(app).waitForNonExistence(timeout: 10))
        XCTAssertTrue(target.exists, "Return must preserve the target note type")
        XCTAssertEqual(listRows(app).count, 3)
        try openDeleteMenu(app, row: target, identifier: "noteTypes.list.delete", title: "Delete…").click()
        // Fresh counts after a Return dismissal also prove that it did not
        // delete notes or cards while leaving the type behind.
        assertCounts(app)
        XCTAssertTrue(element(app, identifier).exists)
        confirmation(app).buttons["Cancel"].click()
        XCTAssertTrue(confirmation(app).waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, identifier).exists, "Cancel keeps the type and its notes")
        XCTAssertEqual(listRows(app).count, 3)
        capture(app.windows.firstMatch, "note-type-delete-cancelled")

        try openDeleteMenu(app, row: target, identifier: "noteTypes.list.delete", title: "Delete…").click()
        assertCounts(app)
        confirmation(app).buttons["Delete"].click()
        XCTAssertTrue(confirmation(app).waitForNonExistence(timeout: 20))
        XCTAssertTrue(element(app, identifier).waitForNonExistence(timeout: 30))
        XCTAssertTrue(remaining.exists)
        XCTAssertTrue(noteTypeRow(app, id: clozeID).exists)
        XCTAssertEqual(listRows(app).count, 2)
        // The freshly selected remaining type has its real detail, not the
        // removed type's fields or a stranded selection.
        assertWindowTitle(app, basicWindowTitle)
        XCTAssertTrue(element(app, "noteTypes.fields.table").waitForExistence(timeout: 10))
        capture(app.windows.firstMatch, "note-type-delete-remaining-type-selected")

        app.menuBars.menuBarItems["Edit"].click()
        let undo = try menuAction(app, identifier: "", title: "Undo Remove Note Type",
                                  captureName: "note-type-delete-native-undo-menu")
        undo.click()
        XCTAssertTrue(element(app, identifier).waitForExistence(timeout: 30))
        XCTAssertEqual(listRows(app).count, 3)
        select(target, in: app, windowTitle: reverseWindowTitle)
        try openDeleteMenu(app, row: target, identifier: "noteTypes.list.delete", title: "Delete…").click()
        assertCounts(app) // Undo restored both notes and their cards.
        capture(app.windows.firstMatch, "note-type-delete-undo-restored-counts")
        confirmation(app).buttons["Cancel"].click()
        XCTAssertTrue(confirmation(app).waitForNonExistence(timeout: 10))
    }

    func testBrowseNoteTypeRowOffersNativeDeleteAndCancel() throws {
        let (app, bundle) = try launch()
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.browse").click()
        let row = element(app, "browse.sidebar.notetype.\(reverseID)")
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        let sidebar = app.outlines.containing(.any, identifier: row.identifier).firstMatch
        XCTAssertTrue(sidebar.exists, "The production sidebar owns the fixture row: \(row.debugDescription)")
        let search = try MacBrowseSidebar(app: app).searchField()
        search.click()
        app.typeKey("a", modifierFlags: .command)
        search.typeText(reverseName)
        XCTAssertTrue(waitUntil { search.value as? String == self.reverseName })
        // Filtering removes the preceding sections that placed this row below
        // the viewport (y=997.5 in the VM evidence). Never click it offscreen.
        let action = try openDeleteMenu(app, row: row, identifier: "browse.sidebar.notetype.delete",
                                        title: "Delete Note Type…", captureName: "browse-note-type-native-delete-menu")
        action.click()
        assertCounts(app)
        capture(confirmation(app), "browse-note-type-delete-confirmation")
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue(confirmation(app).waitForNonExistence(timeout: 10))
        // Browse's delete action navigates to Note Types before presenting
        // the question. Cancel leaves the user in that workspace.
        let target = noteTypeRow(app, id: reverseID)
        XCTAssertTrue(target.waitForExistence(timeout: 10), "Cancel keeps the target in Note Types")
        XCTAssertEqual(listRows(app).count, 3)
        select(target, in: app, windowTitle: reverseWindowTitle)
        // Reopen here to read fresh counts after Cancel, then leave without a write.
        try openDeleteMenu(app, row: target, identifier: "noteTypes.list.delete", title: "Delete…").click()
        assertCounts(app)
        confirmation(app).buttons["Cancel"].click()
        XCTAssertTrue(confirmation(app).waitForNonExistence(timeout: 10))
        XCTAssertTrue(noteTypeRow(app, id: reverseID).waitForExistence(timeout: 10))
        XCTAssertEqual(listRows(app).count, 3)
    }

    private func launch() throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw XCTSkip("VM-only native interaction") }
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Synthetic recipe needs a development hook build") }
        let app = XCUIApplication()
        // The literal recipe path lets MG discover, generate and stage it in the VM.
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US",
            animations: false, appearance: "light").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "shell.root.sidebar.noteTypes").waitForExistence(timeout: 30))
        return (app, bundle)
    }

    /// NSAlert is exposed as a sheet of the presenting main window. Native
    /// buttons are reached by label; SwiftUI content identifiers are absent.
    private func confirmation(_ app: XCUIApplication) -> XCUIElement {
        app.windows["main"].sheets.firstMatch
    }

    private func assertCounts(_ app: XCUIApplication) {
        let alert = confirmation(app)
        XCTAssertTrue(alert.waitForExistence(timeout: 10), "A real native alert is attached to the main window")
        let title = "Delete “Meguri Basic and Reverse”?"
        assertText(alert.staticTexts.matching(NSPredicate(
            format: "label == %@ OR value == %@ OR title == %@", title, title, title)).firstMatch, title)
        for text in ["Its 90 notes and their 180 cards will be deleted.", "A backup of the collection is kept on this Mac."] {
            let content = alert.staticTexts.matching(NSPredicate(
                format: "label CONTAINS %@ OR value CONTAINS %@ OR title CONTAINS %@", text, text, text)).firstMatch
            XCTAssertTrue(content.exists, "Native alert includes \(text): \(alert.debugDescription)")
        }
        XCTAssertEqual(alert.buttons.count, 2)
        XCTAssertTrue(alert.buttons["Cancel"].isEnabled)
        XCTAssertTrue(alert.buttons["Delete"].isEnabled)
    }

    private func assertText(_ element: XCUIElement, _ expected: String) {
        XCTAssertTrue(element.waitForExistence(timeout: 10))
        XCTAssertTrue(waitUntil { element.label == expected || element.value as? String == expected || element.title == expected },
                      "Expected exact text \(expected): \(element.debugDescription)")
    }

    private func assertWindowTitle(_ app: XCUIApplication, _ expected: String) {
        let window = app.windows["main"]
        XCTAssertTrue(window.waitForExistence(timeout: 20))
        XCTAssertTrue(waitUntil { window.title == expected },
                      "Expected exact window title \(expected): \(window.debugDescription)")
    }

    private func select(_ row: XCUIElement, in app: XCUIApplication, windowTitle: String) {
        press(row, in: app)
        // Selection belongs to the native outline row, not its truncated text.
        let selected = app.outlineRows.containing(.any, identifier: row.identifier).firstMatch
        XCTAssertTrue(waitUntil { selected.exists && selected.isSelected }, "Expected selected note type \(row.identifier)")
        assertWindowTitle(app, windowTitle)
    }

    private func listRows(_ app: XCUIApplication) -> XCUIElementQuery {
        app.staticTexts.matching(NSPredicate(format: "identifier BEGINSWITH 'noteTypes.list.row.'"))
    }

    private func noteTypeRow(_ app: XCUIApplication, id: String) -> XCUIElement {
        listRows(app).matching(identifier: "noteTypes.list.row.\(id)").firstMatch
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func openDeleteMenu(_ app: XCUIApplication, row: XCUIElement, identifier: String,
                                title: String, captureName: String? = nil) throws -> XCUIElement {
        press(row, in: app, right: true)
        return try menuAction(app, identifier: identifier, title: title, captureName: captureName)
    }

    private func menuAction(_ app: XCUIApplication, identifier: String, title: String,
                            captureName: String? = nil) throws -> XCUIElement {
        // SwiftUI/AppKit can omit the menu identifier while retaining its title.
        let caption = NSPredicate(format: "label == %@ OR title == %@ OR value == %@", title, title, title)
        let match = identifier.isEmpty ? caption : NSCompoundPredicate(orPredicateWithSubpredicates: [
            NSPredicate(format: "identifier == %@", identifier), caption
        ])
        let action = app.menus.menuItems.matching(match).firstMatch
        XCTAssertTrue(action.waitForExistence(timeout: 10),
                      "Expected native menu action \(title): \(app.menus.debugDescription)")
        assertText(action, title)
        XCTAssertTrue(action.isEnabled && action.isHittable, "Menu action: \(action.debugDescription)")
        // Capture the open native menu containing this action, never an arbitrary
        // first menu, window or menu-bar proxy.
        if let captureName {
            let owners = app.menus.containing(match).allElementsBoundByIndex.filter {
                $0.menuItems.matching(match).firstMatch.isHittable
            }
            XCTAssertEqual(owners.count, 1, "One open menu owns \(title): \(app.menus.debugDescription)")
            capture(try XCTUnwrap(owners.first), captureName)
        }
        return action
    }

    private func press(_ row: XCUIElement, in app: XCUIApplication, right: Bool = false) {
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        let scroll = app.scrollViews.containing(.any, identifier: row.identifier).firstMatch
        XCTAssertTrue(scroll.exists, "The native list scroll view owns \(row.identifier): \(row.debugDescription)")
        func visible() -> Bool {
            let viewport = scroll.frame.intersection(app.windows.firstMatch.frame)
            return !viewport.isEmpty && !row.frame.isEmpty && viewport.contains(row.frame) && row.isHittable
        }
        // Each step advances the native scroll view toward the row; this never
        // repeats a click or reopens a failed menu.
        for _ in 0..<8 {
            if visible() { break }
            let viewport = scroll.frame.intersection(app.windows.firstMatch.frame)
            guard !viewport.isEmpty, !viewport.contains(row.frame) else { break }
            let before = row.frame
            scroll.scroll(byDeltaX: 0, deltaY: (row.frame.midY > viewport.midY ? -1 : 1) * viewport.height / 2)
            XCTAssertTrue(waitUntil(timeout: 5) { row.frame != before || visible() },
                          "Scrolling must advance toward the target: \(scroll.debugDescription)")
        }
        XCTAssertTrue(waitUntil { visible() }, "Require a visible, hittable row before interaction: \(scroll.debugDescription)")
        if right { row.rightClick() } else { row.click() }
    }

    private func capture(_ element: XCUIElement, _ name: String) {
        XCTAssertTrue(element.exists, "Native screenshot source: \(name)")
        let attachment = XCTAttachment(screenshot: element.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func waitUntil(timeout: TimeInterval = 20, _ condition: @escaping () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return condition()
    }
}
#endif
