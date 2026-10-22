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
                    if state == "loading" {
                        let value = app.descendants(matching: .any)
                            .matching(identifier: "onboarding.collectionImport.progress.value").firstMatch
                        XCTAssertTrue(value.exists)
                        XCTAssertEqual(value.elementType, .progressIndicator)
                        XCTAssertGreaterThanOrEqual(value.frame.width, 44)
                        XCTAssertGreaterThanOrEqual(value.frame.height, 44)
                        XCTAssertEqual(value.label, "Copying collection…")
                        XCTAssertEqual(value.value as? String, "42 of 100")
                        XCTAssertEqual(app.staticTexts["onboarding.collectionImport.progress.stage"].label, "Copying collection…")
                    }
                    guard state == "overflow" else {
                        return AccessibilityAudit.StateResult(state: state, findings: [], touchTargets: [])
                    }
                    let content = app.descendants(matching: .any).matching(identifier: "onboarding.collectionImport.content").firstMatch
                    var findings: [AuditFinding] = []
                    var touchTargets: [TouchTargetViolation] = []
                    try self.scrollToRequiredControls(content, app: app, build: build, identifiers: [
                        "onboarding.collectionImport.confirm", "onboarding.collectionImport.choose"
                    ]) { visible in
                        let viewport = content.frame.intersection(app.windows.firstMatch.frame)
                        for identifier in visible {
                            let control = app.buttons[identifier]
                            XCTAssertTrue(control.isHittable, "\(identifier) must remain reachable after scrolling")
                            XCTAssertTrue(viewport.contains(control.frame), "\(identifier)'s entire frame must fit the scroll viewport")
                        }
                        findings += try AccessibilityAudit.run(
                            app, variant: "overflow scrolled (\(visible.joined(separator: ", "))), \(variant)", suppressions: suppressions
                        )
                        touchTargets += try TouchTargets.violations(in: app)
                    }
                    XCTAssertEqual(app.buttons["onboarding.collectionImport.confirm"].label, "Replace Collection")
                    return AccessibilityAudit.StateResult(state: state, findings: findings, touchTargets: touchTargets)
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
