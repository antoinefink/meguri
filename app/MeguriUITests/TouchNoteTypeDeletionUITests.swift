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
            locale: "en_US", animations: false).arguments
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
        element(app, "noteTypes.delete.cancel").tap()
        XCTAssertTrue(element(app, "noteTypes.delete.sheet").waitForNonExistence(timeout: 10))
        XCTAssertTrue(reversedTypeRow.waitForExistence(timeout: 10))
        XCTAssertTrue(reversedTypeRow.isHittable, "Cancel returns to the unchanged list")
        XCTAssertTrue(basicTypeRow.exists)
        capture(app, "touch-note-type-delete-cancelled-list")

        openDeleteMenu(app, target: reversedTypeRow)
        element(app, "noteTypes.list.delete").tap()
        assertConfirmation(app)
        element(app, "noteTypes.delete.confirm").tap()
        XCTAssertTrue(element(app, "noteTypes.delete.sheet").waitForNonExistence(timeout: 30))
        XCTAssertTrue(element(app, "noteTypes.list.table").waitForExistence(timeout: 10))
        XCTAssertTrue(reversedTypeRow.waitForNonExistence(timeout: 30), "The confirmed type is removed from the real list")
        XCTAssertTrue(basicTypeRow.waitForExistence(timeout: 10))
        XCTAssertTrue(basicTypeRow.isHittable)
        XCTAssertTrue(element(app, "noteTypes.list.row.1884308400000").exists, "The second named type survives")
        capture(app, "touch-note-type-delete-resulting-list")
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
        let title = element(app, "noteTypes.delete.title")
        XCTAssertTrue(title.waitForExistence(timeout: 10))
        XCTAssertEqual(title.label, "Delete “Meguri Basic and Reverse”?")
        XCTAssertEqual(element(app, "noteTypes.delete.counts").label,
                       "Its 90 notes and their 180 cards will be deleted.")
        let confirm = element(app, "noteTypes.delete.confirm")
        XCTAssertEqual(confirm.label, "Delete")
        XCTAssertTrue(confirm.isEnabled && confirm.isHittable)
        XCTAssertTrue(element(app, "noteTypes.delete.cancel").isHittable)
    }

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let png = UIDevice.current.userInterfaceIdiom == .pad ? XCUIScreen.main.screenshot().pngRepresentation : app.screenshot().pngRepresentation
        let attachment = XCTAttachment(data: png, uniformTypeIdentifier: "public.png")
        attachment.name = name + ".png"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
