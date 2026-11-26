import Foundation
import XCTest

#if os(iOS)
import UIKit

/// W12/S12 through the production shell: Settings on iPhone, sidebar on iPad.
/// The same case runs against the public typical recipe on both devices.
@MainActor
final class TouchNoteTypeDeletionUITests: XCTestCase {
    override func setUpWithError() throws {
        try super.setUpWithError()
        #if os(iOS) && !targetEnvironment(simulator)
        throw XCTSkip("The host-generated typical-1 fixture is staged in the test VM; physical iOS runners cannot read host paths")
        #endif
    }

    func testNativeDeleteConfirmsCountsCancelsThenRemovesOnlyTarget() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The public recipe requires an isolated development test container")
        }
        let originalOrientation = XCUIDevice.shared.orientation
        XCUIDevice.shared.orientation = UIDevice.current.userInterfaceIdiom == .pad ? .landscapeLeft : .portrait
        let app = XCUIApplication()
        // MG discovers and stages this existing recipe from its literal path.
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false,
            extra: ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryL"]).arguments
        addTeardownBlock { @MainActor in
            app.terminate()
            XCUIDevice.shared.orientation = originalOrientation
        }
        app.launch()
        try openNoteTypes(app)

        // Use the public recipe's record identifiers: displayed names can truncate.
        let reversedTypeRow = element(app, "noteTypes.list.row.1884308400001")
        let basicTypeRow = element(app, "noteTypes.list.row.1884308400000")
        XCTAssertTrue(reversedTypeRow.waitForExistence(timeout: 30), "Stage typical-1 through tools/mg")
        XCTAssertTrue(basicTypeRow.exists)
        XCTAssertNotEqual(reversedTypeRow.identifier, basicTypeRow.identifier)

        openDeleteMenu(app, target: reversedTypeRow)
        capture(app, "touch-note-type-delete-native-menu")
        element(app, "noteTypes.list.delete").tap()
        assertConfirmation(app)
        capture(app, "touch-note-type-delete-confirmation-90-notes-180-cards")
        app.alerts.firstMatch.buttons["Cancel"].tap()
        XCTAssertTrue(app.alerts.firstMatch.waitForNonExistence(timeout: 10))
        XCTAssertTrue(reversedTypeRow.waitForExistence(timeout: 10))
        XCTAssertTrue(reversedTypeRow.isHittable, "Cancel returns to the unchanged list")
        XCTAssertTrue(basicTypeRow.exists)
        capture(app, "touch-note-type-delete-cancelled-list")

        openDeleteMenu(app, target: reversedTypeRow)
        element(app, "noteTypes.list.delete").tap()
        assertConfirmation(app) // Cancel retained the exact 90-note / 180-card question.
        app.alerts.firstMatch.buttons["Delete"].tap()
        XCTAssertTrue(app.alerts.firstMatch.waitForNonExistence(timeout: 30))
        XCTAssertTrue(element(app, "noteTypes.list.table").waitForExistence(timeout: 10))
        XCTAssertTrue(reversedTypeRow.waitForNonExistence(timeout: 30), "The confirmed type is removed from the real list")
        XCTAssertTrue(basicTypeRow.waitForExistence(timeout: 10))
        XCTAssertTrue(basicTypeRow.isHittable)
        XCTAssertTrue(element(app, "noteTypes.list.row.1884308400000").exists, "The second named type survives")
        capture(app, "touch-note-type-delete-resulting-list")
    }

    func testCompactSampleShellUsesItsOwnDeletionModelForEmptyType() throws {
        try sampleShellEmptyQuestion(sizeClass: "compact")
    }

    func testRegularIPadSampleShellUsesItsOwnDeletionModelForEmptyType() throws {
        guard UIDevice.current.userInterfaceIdiom == .pad else { throw XCTSkip("Regular iPad shell") }
        try sampleShellEmptyQuestion(sizeClass: "regular")
    }

    func testNativeAlertReconfirmsFreshCountsAfterConfirmingStaleCounts() throws {
        let app = try launchDeletionSample(largestText: false)
        assertSampleConfirmation(app, notes: 412, cards: 786)
        let alert = app.alerts.firstMatch
        alert.buttons["Delete and Upload"].tap()

        // The real UIKit action dismisses the old alert. The model's checked
        // read returns synchronously, so renewal can race that dismissal.
        // Require the new native question without opening another menu.
        assertSampleConfirmation(app, notes: 413, cards: 788)
        XCTAssertFalse(alert.staticTexts[sampleMessage(notes: 412, cards: 786)].exists)
        capture(app, "touch-note-type-delete-renewed-native-alert")
        alert.buttons["Cancel"].tap()
        XCTAssertTrue(alert.waitForNonExistence(timeout: 10))

        // Reopening after Cancel shows the restored list can present the latest counts.
        let row = sampleVocabularyRow(app)
        openDeleteMenu(app, target: row)
        element(app, "noteTypes.list.delete").tap()
        assertSampleConfirmation(app, notes: 413, cards: 788)
        alert.buttons["Cancel"].tap()
        XCTAssertTrue(alert.waitForNonExistence(timeout: 10))
        XCTAssertTrue(row.exists && row.isHittable)
    }

    func testLargestDynamicTypeNativeWarningCanBeReadAndCancelled() throws {
        let app = try launchDeletionSample(largestText: true)
        assertSampleConfirmation(app, notes: 412, cards: 786)
        let alert = app.alerts.firstMatch
        let message = alert.staticTexts[sampleMessage(notes: 412, cards: 786)]
        let scroll = alert.scrollViews.containing(.staticText, identifier: message.label).firstMatch
        let viewport = scroll.exists ? scroll : alert
        XCTAssertTrue(viewport.frame.intersects(message.frame), "The warning starts in the native viewport")
        XCTAssertGreaterThanOrEqual(message.frame.minY, viewport.frame.minY - 1,
                                    "The losses at the start of the warning are readable")
        capture(app, "touch-note-type-delete-largest-text-warning-start")

        // UIKit may scroll this long warning. Read through the backup paragraph
        // instead of applying the regular-size content-height assertion.
        for _ in 0..<12 {
            if message.frame.maxY <= viewport.frame.maxY + 1 { break }
            XCTAssertTrue(scroll.exists, "An overflowing warning needs native scrolling")
            scroll.swipeUp()
        }
        XCTAssertLessThanOrEqual(message.frame.maxY, viewport.frame.maxY + 1,
                                 "The end of the warning, including the backup paragraph, is reachable")
        capture(app, "touch-note-type-delete-largest-text-warning-end")
        let cancel = alert.buttons["Cancel"]
        for _ in 0..<6 {
            if cancel.isHittable { break }
            alert.swipeUp()
        }
        XCTAssertTrue(cancel.isEnabled && cancel.isHittable, "Cancel stays reachable at the largest text size")
        cancel.tap()
        XCTAssertTrue(alert.waitForNonExistence(timeout: 10))
        XCTAssertTrue(element(app, "noteTypes.list.table").waitForExistence(timeout: 10))
        XCTAssertTrue(sampleVocabularyRow(app).isHittable, "Cancel retains Vocabulary")
        capture(app, "touch-note-type-delete-largest-text-cancelled")
    }

    private func sampleVocabularyRow(_ app: XCUIApplication) -> XCUIElement {
        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        search.tap()
        search.typeText("Vocabulary")
        let row = element(app, "noteTypes.list.row.1700000100009")
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        return row
    }

    private func launchDeletionSample(largestText: Bool) throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The sample shell requires the development test container")
        }
        let originalOrientation = XCUIDevice.shared.orientation
        let tablet = UIDevice.current.userInterfaceIdiom == .pad
        XCUIDevice.shared.orientation = tablet ? .landscapeLeft : .portrait
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: true,
            route: "notetypes/delete/meguri-sync", extra: [
                "-MeguriSizeClass", tablet ? "regular" : "compact",
                "-UIPreferredContentSizeCategoryName",
                largestText ? "UICTContentSizeCategoryAccessibilityXXXL" : "UICTContentSizeCategoryL",
            ]).arguments
        addTeardownBlock { @MainActor in
            app.terminate()
            XCUIDevice.shared.orientation = originalOrientation
        }
        app.launch()
        return app
    }

    private func sampleMessage(notes: Int, cards: Int) -> String {
        let device = UIDevice.current.userInterfaceIdiom == .pad ? "iPad" : "iPhone"
        return "Its \(notes) notes and their \(cards) cards will be deleted.\n\n"
            + "Meguri Sync will be replaced. These changes alter the structure of your collection, so they cannot be merged. "
            + "Meguri syncs this \(device) first, saves, then uploads this collection in place of the one on Meguri Sync.\n\n"
            + "Sync your other devices before you continue. Changes on them that have not synced will be lost.\n\n"
            + "A backup of the collection is kept on this \(device)."
    }

    private func assertSampleConfirmation(_ app: XCUIApplication, notes: Int, cards: Int) {
        let alert = app.alerts.firstMatch
        XCTAssertTrue(alert.waitForExistence(timeout: 20))
        XCTAssertTrue(alert.staticTexts[sampleMessage(notes: notes, cards: cards)].waitForExistence(timeout: 10),
                      "The native alert must show the fresh losses and every sync/backup paragraph")
        XCTAssertEqual(app.alerts.count, 1)
        XCTAssertTrue(alert.staticTexts["Delete “Vocabulary”?"].exists)
        XCTAssertEqual(alert.buttons.count, 2)
        XCTAssertTrue(alert.buttons["Delete and Upload"].isEnabled)
        XCTAssertTrue(alert.buttons["Cancel"].isEnabled)
        XCTAssertFalse(element(app, "noteTypes.list.saveSheet").exists)
        XCTAssertFalse(element(app, "noteTypes.delete.sheet").exists)
    }

    private func sampleShellEmptyQuestion(sizeClass: String) throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The sample shell requires the development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
            route: "notetypes/list/typical", extra: ["-MeguriSizeClass", sizeClass]).arguments
        addTeardownBlock { @MainActor in app.terminate() }
        app.launch()
        XCTAssertTrue(element(app, "noteTypes.list.table").waitForExistence(timeout: 20))
        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        search.tap()
        search.typeText("Phrases")
        let row = element(app, "noteTypes.list.row.1700000100007")
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        openDeleteMenu(app, target: row)
        element(app, "noteTypes.list.delete").tap()
        let alert = app.alerts.firstMatch
        XCTAssertTrue(alert.waitForExistence(timeout: 10))
        XCTAssertTrue(alert.staticTexts["Delete “Phrases”?"].exists)
        let device = UIDevice.current.userInterfaceIdiom == .pad ? "iPad" : "iPhone"
        XCTAssertTrue(alert.staticTexts["It has no notes.\n\nA backup of the collection is kept on this \(device)."].exists)
        XCTAssertTrue(alert.buttons["Delete"].isEnabled)
        XCTAssertFalse(element(app, "noteTypes.list.saveSheet").exists)
        alert.buttons["Cancel"].tap()
        XCTAssertTrue(alert.waitForNonExistence(timeout: 10))
        XCTAssertTrue(row.exists, "Cancel retains the sample shell's empty type")
    }

    private func openNoteTypes(_ app: XCUIApplication) throws {
        let sidebar = app.buttons.matching(identifier: "shell.root.sidebar.noteTypes")
        // Native tab titles also cover the known iOS 27 tab-identifier gap.
        let settingsTabs = app.tabBars.buttons.matching(NSPredicate(
            format: "identifier == %@ OR label == %@", "shell.root.tab.settings", "Settings"))
        var entry: XCUIElement?
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            entry = HomeControls.hittable(sidebar) ?? HomeControls.hittable(settingsTabs)
            return entry != nil
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [ready], timeout: 30), .completed)
        let control = try XCTUnwrap(entry, "The native shell must offer Settings or Note Types")
        control.tap()
        if control.identifier != "shell.root.sidebar.noteTypes" {
            let settings = element(app, "settings.root.content")
            XCTAssertTrue(settings.waitForExistence(timeout: 10))
            let link = element(app, "settings.root.noteTypes")
            // Collection follows Sync in the flat native Form; reveal its lazy row.
            for _ in 0..<6 {
                if link.exists && link.isHittable { break }
                settings.swipeUp()
            }
            XCTAssertTrue(link.exists && link.isHittable, "Settings › Collection exposes Note Types")
            link.tap()
        }
        XCTAssertTrue(element(app, "noteTypes.list.table").waitForExistence(timeout: 20))
    }

    private func openDeleteMenu(_ app: XCUIApplication, target: XCUIElement) {
        XCTAssertTrue(target.isHittable)
        target.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).press(forDuration: 1)
        let action = element(app, "noteTypes.list.delete")
        XCTAssertTrue(action.waitForExistence(timeout: 10))
        XCTAssertTrue(action.isEnabled && action.isHittable)
    }

    private func assertConfirmation(_ app: XCUIApplication) {
        let alert = app.alerts.firstMatch
        XCTAssertTrue(alert.waitForExistence(timeout: 10))
        XCTAssertEqual(app.alerts.count, 1)
        let title = alert.staticTexts["Delete “Meguri Basic and Reverse”?"]
        XCTAssertTrue(title.exists)
        let device = UIDevice.current.userInterfaceIdiom == .pad ? "iPad" : "iPhone"
        let message = alert.staticTexts["Its 90 notes and their 180 cards will be deleted.\n\nA backup of the collection is kept on this \(device)."]
        XCTAssertTrue(message.exists, "Exact losses and backup instructions belong to the native alert")
        let confirm = alert.buttons["Delete"]
        let cancel = alert.buttons["Cancel"]
        XCTAssertTrue(confirm.isEnabled && confirm.isHittable)
        XCTAssertTrue(cancel.isEnabled && cancel.isHittable)
        XCTAssertEqual(alert.buttons.count, 2)
        XCTAssertFalse(element(app, "noteTypes.list.saveSheet").exists)
        XCTAssertFalse(element(app, "noteTypes.delete.sheet").exists)

        // This ordinary-sized fixture needs no scrolling. Compare the alert to
        // its actual content, not a phone/pad's dimensions: a large blank form
        // or alert must fail even when every label and action remains visible.
        let content = [title, message, confirm, cancel].reduce(CGRect.null) { $0.union($1.frame) }
        XCTAssertFalse(content.isEmpty)
        XCTAssertTrue(alert.frame.contains(content), "All confirmation content fits inside the alert")
        XCTAssertLessThan(alert.frame.height, content.height * 1.5,
                          "The system alert must hug its content without a large blank region")
    }

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let png = XCUIScreen.main.screenshot().pngRepresentation
        let attachment = XCTAttachment(data: png, uniformTypeIdentifier: "public.png")
        attachment.name = name + ".png"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
