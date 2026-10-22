import Foundation
import XCTest

@MainActor
final class CollectionEntryAccessibilityTests: XCTestCase {
    func testEveryImportStatePassesTheAuditAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["empty", "typical", "overflow", "error", "loading"].map { state in
            try AccessibilityAudit.runAllVariants(
                state: state, launch: MeguriLaunch(locale: "en_US", animations: false, route: "collection/import/\(state)"),
                waitFor: "onboarding.collectionImport.content", suppressions: suppressions,
                supplemental: { app, variant in
                    guard state == "overflow" else {
                        return AccessibilityAudit.StateResult(state: state, findings: [], touchTargets: [])
                    }
                    let content = app.descendants(matching: .any).matching(identifier: "onboarding.collectionImport.content").firstMatch
                    try self.scrollToBottom(content, app: app, build: build)
                    XCTAssertTrue(app.buttons["onboarding.collectionImport.confirm"].isHittable, "replacement confirmation must remain reachable after scrolling")
                    XCTAssertEqual(app.buttons["onboarding.collectionImport.confirm"].label, "Replace Collection")
                    XCTAssertTrue(app.buttons["onboarding.collectionImport.choose"].isHittable, "file selection must remain reachable after scrolling")
                    return AccessibilityAudit.StateResult(
                        state: state,
                        findings: try AccessibilityAudit.run(app, variant: "overflow scrolled, \(variant)", suppressions: suppressions),
                        touchTargets: try TouchTargets.violations(in: app)
                    )
                }
            )
        }
        add(AccessibilityAudit.screenReport("onboarding.collectionImport", states: states))
        for state in states {
            AccessibilityAudit.assertClean(state.findings)
            TouchTargets.assertMet(state.touchTargets)
        }
    }

    /// Scoped coverage of Home's shared import control in no-file and ready content.
    func testHomeEmptyAndTypicalImportControlPassesTheAudit() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["empty", "typical"].map { state in
            try AccessibilityAudit.runAllVariants(
                state: state, launch: MeguriLaunch(locale: "en_US", animations: false, route: "home/deck-list/\(state)"),
                waitFor: "home.collection.screen", suppressions: suppressions,
                ready: { app in
                    try self.scrollToBottom(app.scrollViews.firstMatch, app: app, build: build)
                    let action = app.buttons["home.collection.import"]
                    XCTAssertTrue(action.isHittable)
                    XCTAssertEqual(action.label, "Import Collection")
                }
            )
        }
        add(AccessibilityAudit.screenReport("home.deckList", states: states))
        for state in states {
            AccessibilityAudit.assertClean(state.findings)
            TouchTargets.assertMet(state.touchTargets)
        }
    }

    private func scrollToBottom(_ scroll: XCUIElement, app: XCUIApplication, build: BuildUnderTest) throws {
        XCTAssertTrue(scroll.exists)
        try app.bringToFront(bundleIdentifier: try build.bundleIdentifier)
        #if os(macOS)
        scroll.scroll(byDeltaX: 0, deltaY: -10_000)
        #else
        // Move through the long sheet to reach its bottom controls.
        scroll.swipeUp()
        scroll.swipeUp()
        #endif
    }
}
