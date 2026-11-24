import Foundation
import XCTest
#if os(macOS)
import AppKit
import ApplicationServices

/// N20.8: the Mac's visible native review menu, using only the synthetic
/// local-review recipe in the isolated development container in the test VM.
@MainActor
final class ReviewActionsMacUITests: XCTestCase {
    func testToolbarMenuShowsReviewActions() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundleIdentifier = try build.bundleIdentifier
        guard build.hasHooks, bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US",
            animations: false, appearance: "light").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        try app.bringToFront(bundleIdentifier: bundleIdentifier)

        let rows = HomeControls.deckRows(app, labelContaining: "Local Review")
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 20), "Stage the synthetic local-review-1 recipe")
        (HomeControls.hittable(rows) ?? rows.firstMatch).click()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        study.click()
        let reveal = element(app, "review.session.reveal")
        XCTAssertTrue(reveal.waitForExistence(timeout: 20))
        let menu = element(app, "review.session.actions")
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertTrue(menu.isEnabled && menu.isHittable)
        let window = app.windows.firstMatch
        let close = element(app, "review.session.close")
        XCTAssertGreaterThan(menu.frame.midX, window.frame.midX, "In the toolbar's trailing corner")
        XCTAssertLessThan(abs(menu.frame.midY - close.frame.midY), 4, "In the same toolbar as Back to Decks")
        menu.click()

        // Menus are separate native windows. Capture the VM's screen before
        // assertions so a command, caption or keyboard-ownership failure keeps
        // the open menu and its actual AX roles, names and identifiers.
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "review-actions-mac-menu-open"
        attachment.lifetime = .keepAlways
        add(attachment)
        let hierarchy = XCTAttachment(string: menu.debugDescription)
        hierarchy.name = "review-actions-mac-menu-open-ax"
        hierarchy.lifetime = .keepAlways
        add(hierarchy)

        // AXDescription may be the label, or AppKit may expose only AXTitle.
        // Neither the identifier nor hover help is an accessible-name fallback.
        let name = try menu.snapshot()
        XCTAssertEqual(name.label.isEmpty ? name.title : name.label, "Review actions",
                       "Native menu name: title=\(name.title.debugDescription), label=\(name.label.debugDescription)")
        XCTAssertEqual(try nativeHelp(menu.identifier, bundleIdentifier: bundleIdentifier), "Review actions")

        // Like the native Service picker test, scope to this control's menu;
        // the menu bar contains commands with the same titles. AppKit menu
        // items need not retain SwiftUI accessibility identifiers.
        let items = menu.menuItems
        let titles = ["Edit Note", "Bury Card", "Suspend Card"]
        XCTAssertTrue(items.matching(NSPredicate(format: "title == %@", titles[0]))
            .firstMatch.waitForExistence(timeout: 10))
        XCTAssertEqual(items.allElementsBoundByIndex.map(\.title).filter { !$0.isEmpty }, titles,
                       "The complete native command set, in order (excluding separators)")
        var previous: XCUIElement?
        for title in titles {
            let matches = items.matching(NSPredicate(format: "title == %@", title))
            let item = matches.firstMatch
            XCTAssertTrue(item.waitForExistence(timeout: 10), title)
            XCTAssertEqual(matches.count, 1, "One \(title) in the open menu")
            XCTAssertEqual(item.elementType, .menuItem)
            XCTAssertEqual(item.title, title)
            XCTAssertTrue(item.isEnabled, title)
            if let previous { XCTAssertLessThan(previous.frame.midY, item.frame.midY, "Native menu order") }
            previous = item
        }

        app.typeKey(.escape, modifierFlags: [])
        let edit = items.matching(NSPredicate(format: "title == %@", "Edit Note")).firstMatch
        XCTAssertEqual(XCTWaiter().wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: edit)], timeout: 10), .completed)
        XCTAssertTrue(reveal.exists, "Dismissing the menu leaves the question unanswered")
        XCTAssertFalse(element(app, "review.session.rate.good").exists)

        menu.click()
        XCTAssertTrue(edit.waitForExistence(timeout: 10))
        XCTAssertTrue(edit.isEnabled && edit.isHittable)
        edit.click()
        let form = element(app, "review.editCurrent")
        XCTAssertTrue(form.waitForExistence(timeout: 15), "The toolbar menu opens Edit Note")
        let cancel = element(app, "review.editCurrent.cancel")
        XCTAssertTrue(cancel.waitForExistence(timeout: 10))
        XCTAssertTrue(cancel.isEnabled && cancel.isHittable)
        cancel.click()
        XCTAssertTrue(form.waitForNonExistence(timeout: 10), "Cancel closes the unchanged draft")
        XCTAssertTrue(reveal.exists, "Closing Edit Note returns to the unanswered card")
        XCTAssertFalse(element(app, "review.session.rate.good").exists)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    /// Read the actual hover help from the same native control, following the
    /// bounded window AX traversal used by ManualTransferFixtureKeyboardTests.
    private func nativeHelp(_ identifier: String, bundleIdentifier: String) throws -> String {
        let running = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: bundleIdentifier)
            .first { !$0.isTerminated })
        let application = AXUIElementCreateApplication(running.processIdentifier)
        func read(_ node: AXUIElement, _ attribute: String) -> CFTypeRef? {
            var value: CFTypeRef?
            let error = AXUIElementCopyAttributeValue(node, attribute as CFString, &value)
            if error != .success && error != .attributeUnsupported && error != .noValue {
                XCTFail("Reading \(attribute) for \(identifier): AX error \(error.rawValue)")
            }
            return value
        }
        let windows = try XCTUnwrap(read(application, kAXWindowsAttribute) as? [AXUIElement])
        var pending = windows.map { ($0, 0) }
        var visited = 0
        var matches: [AXUIElement] = []
        while let (node, depth) = pending.popLast(), visited < 2048 {
            visited += 1
            if read(node, kAXIdentifierAttribute) as? String == identifier { matches.append(node) }
            if depth < 48, let children = read(node, kAXChildrenAttribute) as? [AXUIElement] {
                pending.append(contentsOf: children.map { ($0, depth + 1) })
            }
        }
        XCTAssertTrue(pending.isEmpty, "Native menu lookup exceeded its node limit")
        XCTAssertEqual(matches.count, 1, "One native control for \(identifier)")
        let control = try XCTUnwrap(matches.first, "Missing native control \(identifier)")
        return try XCTUnwrap(read(control, kAXHelpAttribute) as? String, "Missing AXHelp for \(identifier)")
    }
}
#endif
