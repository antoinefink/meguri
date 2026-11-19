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
                supplemental: { app, _ in
                    if state == "loading" {
                        let value = app.descendants(matching: .any)
                            .matching(identifier: "onboarding.collectionImport.progress.value").firstMatch
                        XCTAssertTrue(value.exists)
                        XCTAssertEqual(value.elementType, .progressIndicator)
                        XCTAssertGreaterThanOrEqual(value.frame.width, 44)
                        XCTAssertGreaterThanOrEqual(value.frame.height, 44)
                        // N17.2: one plain line names the step; it is the bar's own label.
                        XCTAssertEqual(value.label, "Importing…")
                    }
                    if state == "empty" || state == "typical" || state == "overflow" {
                        XCTAssertEqual(app.buttons["onboarding.collectionImport.confirm"].firstMatch.label, "Import")
                        // Only a replacement says what it replaces.
                        if state == "empty" {
                            XCTAssertFalse(app.descendants(matching: .any)["onboarding.collectionImport.preview.safety"].firstMatch.exists)
                        }
                        for removed in ["preview.destination", "preview.media.files", "preview.media.bytes", "explanation", "choose"] {
                            XCTAssertFalse(app.descendants(matching: .any)["onboarding.collectionImport.\(removed)"].firstMatch.exists, removed)
                        }
                    }
                    // The confirmation's actions sit in the bar: nothing has to be scrolled into reach.
                    return AccessibilityAudit.StateResult(state: state, findings: [], touchTargets: try TouchTargets.violations(in: app))
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
                    try self.scrollToRequiredControls(app.scrollViews.firstMatch, app: app, build: build,
                                                      identifiers: ["home.collection.import"])
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

    private func scrollToRequiredControls(_ scroll: XCUIElement, app: XCUIApplication, build: BuildUnderTest,
                                          identifiers: [String], onVisible: (([String]) throws -> Void)? = nil) throws {
        XCTAssertTrue(scroll.exists)
        try app.bringToFront(bundleIdentifier: try build.bundleIdentifier)
        let controls = identifiers.map { app.buttons[$0] }
        var checked: Set<Int> = []
        var individually = false
        for gesture in 0...24 {
            let viewport = scroll.frame.intersection(app.windows.firstMatch.frame)
            guard !viewport.isEmpty, controls.allSatisfy({ $0.exists }) else { break }
            let frames = controls.map(\.frame)
            let required = frames.reduce(CGRect.null) { $0.union($1) }
            let visible = controls.indices.filter {
                !frames[$0].isEmpty && viewport.contains(frames[$0]) && controls[$0].isHittable
            }
            if visible.count == controls.count {
                try onVisible?(identifiers)
                return
            }
            // When the controls cannot share a viewport, retain a full audit at
            // each reachable control rather than accepting partial visibility.
            individually = individually || required.height > viewport.height || required.width > viewport.width
            if individually {
                let newlyVisible = visible.filter { !checked.contains($0) }
                if !newlyVisible.isEmpty {
                    try onVisible?(newlyVisible.map { identifiers[$0] })
                    checked.formUnion(newlyVisible)
                }
                if checked.count == controls.count { return }
            }
            guard gesture < 24 else { break }
            let target = individually ? frames[controls.indices.first { !checked.contains($0) }!] : required
            let towardsBottom = target.maxY > viewport.maxY
            #if os(macOS)
            scroll.scroll(byDeltaX: 0, deltaY: (towardsBottom ? -1 : 1) * viewport.height / 2)
            #else
            if towardsBottom { scroll.swipeUp() } else { scroll.swipeDown() }
            #endif
        }
        XCTFail("Every required control must be fully inside the scroll viewport and hittable after at most 24 gestures: \(identifiers)\n\(app.debugDescription)")
    }
}
