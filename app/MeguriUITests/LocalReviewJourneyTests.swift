import Foundation
import XCTest
#if os(macOS)
import AppKit
import ApplicationServices
#endif

/// Actual production shell/core/pages. Seed only the first launch: the second
/// launch must reuse the database and must not resubmit a recorded answer.
@MainActor
final class LocalReviewJourneyTests: XCTestCase {
    #if os(macOS)
    /// Real key events and native accessibility focus run only in the UI VM.
    func testInitialKeyboardReviewAndNativeCloseBackActivation() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundleIdentifier = try build.bundleIdentifier
        guard bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let fixture = build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1")
        for activation in [" ", "\r"] {
            let app = XCUIApplication()
            app.launchArguments = MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
                locale: "en_US", animations: false).arguments
            app.launch()
            defer { app.terminate() }
            addTeardownBlock { @MainActor in app.terminate() }
            let enable = element(app, "home.policy.enable")
            if enable.waitForExistence(timeout: 5) {
                try app.bringToFront(bundleIdentifier: bundleIdentifier)
                enable.clickOrTap()
            }
            let deck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "home.deck.row.", "Cards")).firstMatch
            XCTAssertTrue(deck.waitForExistence(timeout: 20), "Generate and stage local-review-1 in the VM before this suite")
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            deck.clickOrTap()
            let study = element(app, "home.deck.study")
            XCTAssertTrue(study.waitForExistence(timeout: 10))
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            study.clickOrTap()
            XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
            // No card click: Study must yield a keyboard-usable first question.
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            app.typeKey(" ", modifierFlags: [])
            XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            app.typeKey("4", modifierFlags: [])
            XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
            XCTAssertTrue(element(app, "review.session.back").isEnabled)
            // Leave one earlier answer so Back stays enabled after the undo.
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            app.typeKey("\r", modifierFlags: [])
            XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            app.typeKey("4", modifierFlags: [])
            XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))

            let back = try focusNativeControl("review.session.back", in: app, bundleIdentifier: bundleIdentifier)
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            app.typeKey(activation, modifierFlags: [])
            XCTAssertTrue(element(app, "review.session.redo").waitForExistence(timeout: 10), "Back must undo; a review shortcut must not consume its key")
            XCTAssertTrue(element(app, "review.session.reveal").exists)
            assertNativeFocus(back) // The restored page must leave Back in control.
            XCTAssertFalse(element(app, "review.session.rate.good").exists)

            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            element(app, "review.session.reveal").clickOrTap()
            XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
            _ = try focusNativeControl("review.session.close", in: app, bundleIdentifier: bundleIdentifier)
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            app.typeKey(activation, modifierFlags: [])
            XCTAssertTrue(study.waitForExistence(timeout: 10), "Close must dismiss the answer; its key must not rate Good")
            XCTAssertFalse(element(app, "review.session.rate.good").exists)
            attach(app, activation == " " ? "native-space-close" : "native-return-close")
        }
    }

    private func focusNativeControl(_ identifier: String, in app: XCUIApplication, bundleIdentifier: String) throws -> AXUIElement {
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        let running = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: bundleIdentifier)
            .first { !$0.isTerminated })
        let application = AXUIElementCreateApplication(running.processIdentifier)
        func find(_ node: AXUIElement) -> AXUIElement? {
            var value: CFTypeRef?
            if AXUIElementCopyAttributeValue(node, kAXIdentifierAttribute as CFString, &value) == .success,
               value as? String == identifier { return node }
            var children: CFTypeRef?
            guard AXUIElementCopyAttributeValue(node, kAXChildrenAttribute as CFString, &children) == .success,
                  let nodes = children as? [AXUIElement] else { return nil }
            for child in nodes { if let found = find(child) { return found } }
            return nil
        }
        let control = try XCTUnwrap(find(application), "Native control missing: \(identifier)")
        XCTAssertEqual(AXUIElementSetAttributeValue(control, kAXFocusedAttribute as CFString, kCFBooleanTrue), .success)
        assertNativeFocus(control)
        return control
    }

    private func assertNativeFocus(_ control: AXUIElement) {
        var focused: CFTypeRef?
        XCTAssertEqual(AXUIElementCopyAttributeValue(control, kAXFocusedAttribute as CFString, &focused), .success)
        XCTAssertEqual(focused as? Bool, true)
    }
    #endif

    func testStudyRevealFourRatingsUndoFinishAndRestart() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        #endif
        let fixture = build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1")
        let app = XCUIApplication()
        continueAfterFailure = false
        app.launchArguments = MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let enable = element(app, "home.policy.enable")
        if enable.waitForExistence(timeout: 5) { enable.clickOrTap() }
        let deck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "home.deck.row.", "Cards")).firstMatch
        XCTAssertTrue(deck.waitForExistence(timeout: 20), "Generate local-review with tools/mg fixtures local-review --seed 1")
        deck.clickOrTap()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        study.clickOrTap()
        for rating in ["again", "hard", "good", "easy"] {
            reveal(app)
            for name in ["again", "hard", "good", "easy"] {
                let button = element(app, "review.session.rate.\(name)")
                XCTAssertTrue(button.exists)
                XCTAssertTrue(button.label.contains(","), "rating must include its actual interval")
            }
            element(app, "review.session.rate.\(rating)").clickOrTap()
        }
        let back = element(app, "review.session.back")
        XCTAssertTrue(back.waitForExistence(timeout: 10))
        XCTAssertTrue(back.isEnabled)
        back.clickOrTap()
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        // Drain remaining/requeued fixture cards using actual core outcomes.
        for _ in 0..<20 {
            if element(app, "review.session.done").exists { break }
            if element(app, "review.session.refresh").exists { break }
            reveal(app)
            element(app, "review.session.rate.easy").clickOrTap()
        }
        XCTAssertTrue(element(app, "review.session.done").waitForExistence(timeout: 10), "text fixture did not exhaust")
        element(app, "review.session.done").clickOrTap()
        let recorded = element(app, "home.session.recorded")
        XCTAssertTrue(recorded.waitForExistence(timeout: 10))
        let before = recorded.label
        attach(app, "review-completed")
        app.terminate()
        // Deliberately no -MeguriFixture here: installing again destroys proof.
        app.launchArguments = MeguriLaunch(now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false).arguments
        app.launch()
        XCTAssertTrue(recorded.waitForExistence(timeout: 20))
        XCTAssertEqual(recorded.label, before)
        // Hook launches start at Home; inspect this deck's detail counts again.
        XCTAssertTrue(deck.waitForExistence(timeout: 10))
        deck.clickOrTap()
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        let persistedCounts = element(app, "home.deck.counts")
        XCTAssertTrue(persistedCounts.waitForExistence(timeout: 10))
        XCTAssertTrue(persistedCounts.label.contains("0 due"), "answers were not preserved after process restart")
        attach(app, "review-restarted")
    }
    private func reveal(_ app: XCUIApplication) {
        let button = element(app, "review.session.reveal")
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        button.clickOrTap()
        XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
    }
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
    private func attach(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot()); attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
