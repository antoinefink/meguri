import Foundation
import XCTest
#if os(iOS)

/// Production collection actions over the generated, public typical fixture.
/// Run on both iPhone and iPad. MG discovers and stages typical-1 from this file.
/// PNG attachments contain no test-driver annotations.
@MainActor
final class TouchTagsUITests: XCTestCase {
    override func setUpWithError() throws {
        try super.setUpWithError()
        #if os(iOS) && !targetEnvironment(simulator)
        throw XCTSkip("The host-generated typical-1 fixture is staged in the test VM; physical iOS runners cannot read host paths")
        #endif
    }

    func testTabSettingsDeletesParentKeepsSiblingAndUndoes() throws {
        let app = try launch(sizeClass: "compact")
        try openTags(app, sidebar: false)
        try parentJourney(app, prefix: "touch-tags-tabs")
    }

    func testWideIPadSettingsDeletesParentKeepsSiblingAndUndoes() throws {
        guard UIDevice.current.userInterfaceIdiom == .pad else { throw XCTSkip("Wide iPad navigation") }
        XCUIDevice.shared.orientation = .landscapeLeft
        addTeardownBlock { @MainActor in XCUIDevice.shared.orientation = .portrait }
        let app = try launch(sizeClass: "regular")
        try openTags(app, sidebar: true)
        try parentJourney(app, prefix: "touch-tags-ipad")
    }

    func testNativeSwipeDeletesOnlyTheNamedLeafAndUndoReloads() throws {
        let app = try launch(sizeClass: "compact")
        try openTags(app, sidebar: false)
        let child = element(app, "tags.list.row.jlpt::n4")
        XCTAssertTrue(child.waitForExistence(timeout: 10))
        child.swipeLeft()
        let action = app.buttons["Delete Tag"].firstMatch
        XCTAssertTrue(action.waitForExistence(timeout: 10))
        XCTAssertTrue(action.isEnabled && action.isHittable)
        action.tap()
        XCTAssertTrue(child.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.alerts.firstMatch.exists)
        XCTAssertTrue(element(app, "tags.list.row.grammar").exists)
        capture(app, "touch-tags-leaf-post-delete")
        try undo(app)
        XCTAssertTrue(child.waitForExistence(timeout: 10))
        XCTAssertEqual(child.label, "jlpt › n4")
        capture(app, "touch-tags-leaf-undo")
    }

    private func parentJourney(_ app: XCUIApplication, prefix: String) throws {
        let parent = element(app, "tags.list.row.jlpt")
        let child = element(app, "tags.list.row.jlpt::n4")
        let sibling = element(app, "tags.list.row.grammar")
        XCTAssertTrue(parent.waitForExistence(timeout: 10), "The tree includes the implicit parent")
        XCTAssertTrue(child.waitForExistence(timeout: 10))
        XCTAssertEqual(child.label, "jlpt › n4", "Leaves show their full path")
        XCTAssertTrue(sibling.exists)
        capture(app, "\(prefix)-list")

        parent.press(forDuration: 1.2)
        let delete = app.buttons["Delete Tag"].firstMatch
        XCTAssertTrue(delete.waitForExistence(timeout: 10))
        XCTAssertTrue(delete.isEnabled)
        capture(app, "\(prefix)-action")
        delete.tap()
        XCTAssertTrue(parent.waitForNonExistence(timeout: 10))
        XCTAssertTrue(child.waitForNonExistence(timeout: 10), "Deleting a parent removes its child")
        XCTAssertFalse(app.alerts.firstMatch.exists, "Delete Tag needs no confirmation")
        XCTAssertTrue(sibling.exists, "Unrelated tags stay")
        capture(app, "\(prefix)-post-delete")

        try undo(app)
        XCTAssertTrue(parent.waitForExistence(timeout: 10))
        XCTAssertTrue(child.waitForExistence(timeout: 10), "Collection undo restores the child and reloads the list")
        XCTAssertTrue(sibling.exists)
        capture(app, "\(prefix)-undo")
    }

    private func openTags(_ app: XCUIApplication, sidebar: Bool) throws {
        let settings = sidebar
            ? element(app, "shell.root.sidebar.settings")
            : app.tabBars.buttons["Settings"].firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 20))
        XCTAssertTrue(settings.isHittable)
        settings.tap()
        let content = element(app, "settings.root.content")
        XCTAssertTrue(content.waitForExistence(timeout: 10))
        let tags = element(app, "settings.root.tags")
        // Native Forms scroll; no coordinate taps or synthetic navigation route.
        for _ in 0..<6 {
            if tags.exists, tags.isHittable { break }
            content.swipeUp()
        }
        XCTAssertTrue(element(app, "settings.root.noteTypes").exists,
            "Both collection management entries are discoverable together")
        XCTAssertTrue(tags.waitForExistence(timeout: 10))
        XCTAssertTrue(tags.isHittable)
        XCTAssertEqual(app.descendants(matching: .any).matching(identifier: "settings.root.tags").count, 1)
        XCTAssertEqual(app.descendants(matching: .any).matching(identifier: "settings.root.noteTypes").count, 1,
                       "Settings offers Note Types once")
        XCTAssertFalse(app.tabBars.buttons["Note Types"].exists)
        XCTAssertFalse(element(app, "shell.root.sidebar.browse").exists)
        XCTAssertFalse(app.tabBars.buttons["Browse"].exists, "This journey never opens Browse")
        capture(app, sidebar ? "touch-tags-ipad-settings" : "touch-tags-tab-settings")
        tags.tap()
        XCTAssertTrue(element(app, "tags.list.content").waitForExistence(timeout: 10))
    }

    private func undo(_ app: XCUIApplication) throws {
        let undo = element(app, "tags.list.undo")
        XCTAssertTrue(undo.waitForExistence(timeout: 10))
        let enabled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: undo)
        XCTAssertEqual(XCTWaiter().wait(for: [enabled], timeout: 10), .completed)
        XCTAssertTrue(undo.label.hasPrefix("Undo "), undo.label)
        undo.tap()
    }

    private func launch(sizeClass: String) throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Synthetic fixtures require an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1", directoryHint: .isDirectory),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", sizeClass]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let png = UIDevice.current.userInterfaceIdiom == .pad ? XCUIScreen.main.screenshot().pngRepresentation : app.screenshot().pngRepresentation
        let attachment = XCTAttachment(data: png, uniformTypeIdentifier: "public.png")
        attachment.name = "\(name).png"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
