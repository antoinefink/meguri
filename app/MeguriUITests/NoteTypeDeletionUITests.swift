import Foundation
import XCTest

#if os(macOS)
/// W12/S12 in the actual native workspace, using MG's existing typical-1
/// recipe. Run only in the macOS test VM; screenshots are native elements.
@MainActor
final class NoteTypeDeletionUITests: XCTestCase {
    func testListDeleteConfirmsRealCountsCancelsDeletesAndUndoes() throws {
        let (app, bundle) = try launch()
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.noteTypes").click()
        let target = noteTypeRow(app, name: "Meguri Basic and Reverse")
        XCTAssertTrue(target.waitForExistence(timeout: 20))
        let identifier = target.identifier
        press(target)
        let remaining = noteTypeRow(app, name: "Meguri Basic")
        XCTAssertTrue(remaining.exists)
        capture(app.windows.firstMatch, "note-type-delete-list-before")

        press(target, right: true)
        let action = app.menuItems["noteTypes.list.delete"]
        XCTAssertTrue(action.waitForExistence(timeout: 10) && action.isEnabled)
        capture(app.menus.firstMatch, "note-type-delete-native-row-menu")
        action.click()
        assertCounts(app)
        capture(app.windows.firstMatch, "note-type-delete-native-confirmation-90-notes-180-cards")
        capture(element(app, "noteTypes.delete.counts"), "note-type-delete-counts-element")

        // Return must never accept this destructive question.
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(element(app, "noteTypes.delete.confirm").exists)
        XCTAssertTrue(element(app, identifier).exists)
        element(app, "noteTypes.delete.cancel").click()
        XCTAssertTrue(element(app, "noteTypes.delete.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, identifier).exists, "Cancel keeps the type and its notes")
        capture(app.windows.firstMatch, "note-type-delete-cancelled")

        press(element(app, identifier), right: true)
        XCTAssertTrue(action.waitForExistence(timeout: 10))
        action.click()
        assertCounts(app)
        element(app, "noteTypes.delete.confirm").click()
        XCTAssertTrue(element(app, "noteTypes.delete.sheet").waitForNonExistence(timeout: 20))
        XCTAssertTrue(element(app, identifier).waitForNonExistence(timeout: 30))
        XCTAssertTrue(remaining.exists)
        // The freshly selected remaining type has its real detail, not the
        // removed type's fields or a stranded selection.
        XCTAssertTrue(waitUntil { app.windows.firstMatch.toolbars.staticTexts["Meguri Basic"].exists })
        XCTAssertTrue(element(app, "noteTypes.fields.table").waitForExistence(timeout: 10))
        capture(app.windows.firstMatch, "note-type-delete-remaining-type-selected")

        press(remaining)
        app.menuBars.menuBarItems["Edit"].click()
        let undo = app.menuItems.matching(NSPredicate(format: "label == %@", "Undo Remove Note Type")).firstMatch
        XCTAssertTrue(undo.waitForExistence(timeout: 10) && undo.isEnabled)
        capture(app.menus.firstMatch, "note-type-delete-native-undo-menu")
        undo.click()
        XCTAssertTrue(element(app, identifier).waitForExistence(timeout: 30))
        press(element(app, identifier))
        XCTAssertTrue(waitUntil { app.windows.firstMatch.toolbars.staticTexts["Meguri Basic and Reverse"].exists })
        press(element(app, identifier), right: true)
        XCTAssertTrue(action.waitForExistence(timeout: 10))
        action.click()
        assertCounts(app) // Undo restored both notes and their cards.
        capture(app.windows.firstMatch, "note-type-delete-undo-restored-counts")
        element(app, "noteTypes.delete.cancel").click()
    }

    func testBrowseNoteTypeRowOffersNativeDeleteAndCancel() throws {
        let (app, bundle) = try launch()
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.browse").click()
        let row = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH 'browse.sidebar.notetype.' AND label BEGINSWITH %@",
            "Meguri Basic and Reverse")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        press(row, right: true)
        let action = app.menuItems["browse.sidebar.notetype.delete"]
        XCTAssertTrue(action.waitForExistence(timeout: 10) && action.isEnabled)
        capture(app.menus.firstMatch, "browse-note-type-native-delete-menu")
        action.click()
        assertCounts(app)
        capture(app.windows.firstMatch, "browse-note-type-delete-confirmation")
        element(app, "noteTypes.delete.cancel").click()
        XCTAssertTrue(element(app, "noteTypes.delete.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(noteTypeRow(app, name: "Meguri Basic and Reverse").waitForExistence(timeout: 10))
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

    private func assertCounts(_ app: XCUIApplication) {
        XCTAssertTrue(element(app, "noteTypes.delete.title").waitForExistence(timeout: 10))
        XCTAssertEqual(element(app, "noteTypes.delete.title").label, "Delete “Meguri Basic and Reverse”?")
        XCTAssertEqual(element(app, "noteTypes.delete.counts").label, "Its 90 notes and their 180 cards will be deleted.")
        XCTAssertTrue(element(app, "noteTypes.delete.confirm").isEnabled)
    }

    private func noteTypeRow(_ app: XCUIApplication, name: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH 'noteTypes.list.row.' AND (label == %@ OR label BEGINSWITH %@)", name, name + ",")).firstMatch
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func press(_ row: XCUIElement, right: Bool = false) {
        let point = row.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5))
        if right { point.rightClick() } else { point.click() }
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
