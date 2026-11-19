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
                    // The welcome and the deck tree are system lists: a collection view on touch.
                    let list = app.collectionViews.firstMatch
                    try self.scrollToRequiredControls(list.exists ? list : app.scrollViews.firstMatch, app: app, build: build,
                                                      identifiers: ["home.collection.import"])
                    let action = app.buttons["home.collection.import"]
                    XCTAssertTrue(action.isHittable)
                    XCTAssertEqual(action.label, "Import a Collection File")
                }
            )
        }
        add(AccessibilityAudit.screenReport("home.deckList", states: states))
        for state in states {
            AccessibilityAudit.assertClean(state.findings)
            TouchTargets.assertMet(state.touchTargets)
        }
    }

    /// N18.1: the welcome's three rows, each brought into reach at every text size.
    func testWelcomePassesTheAuditAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let rows = [("home.collection.accountSync", "Set Up Sync"), ("home.collection.import", "Import a Collection File"),
                    ("home.collection.startEmpty", "Start Empty")]
        let state = try AccessibilityAudit.runAllVariants(
            state: "empty", launch: MeguriLaunch(locale: "en_US", animations: false, route: "home/deck-list/empty"),
            waitFor: "home.collection.screen", suppressions: suppressions,
            ready: { app in
                XCTAssertTrue(app.staticTexts["home.welcome.title"].exists)
                let list = app.collectionViews.firstMatch
                let scroll = list.exists ? list : app.scrollViews.firstMatch
                XCTAssertTrue(scroll.exists)
                try app.bringToFront(bundleIdentifier: try build.bundleIdentifier)
                // One row at a time, top to bottom: at the largest text sizes a system list
                // holds only the rows near the screen, so the three never exist together.
                for (identifier, title) in rows {
                    let row = app.buttons[identifier]
                    XCTAssertTrue(self.bringIntoReach(row, in: scroll, app: app), "\(identifier)\n\(app.debugDescription)")
                    XCTAssertTrue(row.isEnabled, identifier)
                    XCTAssertEqual(row.label, title)
                }
            }
        )
        add(AccessibilityAudit.screenReport("home.welcome", states: [state]))
        AccessibilityAudit.assertClean(state.findings)
        TouchTargets.assertMet(state.touchTargets)
    }

    /// N14.10: the deck list beside the detail, with Today and then a deck selected.
    /// No simulator here opens wide, so the audit reads the list through its routes.
    func testDeckListBesideTheDetailPassesTheAudit() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        throw XCTSkip("the Today row is an iPhone layout's")
        #else
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["beside-today", "beside-deck"].map { state in
            try AccessibilityAudit.runAllVariants(
                state: state, launch: MeguriLaunch(locale: "en_US", animations: false, route: "home/deck-list/\(state)"),
                waitFor: "home.today.row", suppressions: suppressions,
                ready: { app in
                    let today = app.buttons["home.today.row"]
                    XCTAssertTrue(today.isHittable)
                    XCTAssertEqual(today.label, "Today")
                    XCTAssertEqual(today.value as? String, "31 cards waiting")
                    XCTAssertEqual(today.isSelected, state == "beside-today")
                    XCTAssertEqual(app.buttons["home.deck.row.2"].isSelected, state == "beside-deck")
                    XCTAssertFalse(app.buttons["home.deck.row.1"].isSelected)
                    XCTAssertLessThan(today.frame.minY, app.buttons["home.deck.row.1"].frame.minY)
                }
            )
        }
        add(AccessibilityAudit.screenReport("home.deckList.besideDetail", states: states))
        for state in states {
            AccessibilityAudit.assertClean(state.findings)
            TouchTargets.assertMet(state.touchTargets)
        }
        #endif
    }

    /// Scrolls until `control` is wholly on screen and hittable. A control the list has
    /// not made yet is further down.
    private func bringIntoReach(_ control: XCUIElement, in scroll: XCUIElement, app: XCUIApplication) -> Bool {
        for _ in 0..<12 {
            let viewport = scroll.frame.intersection(app.windows.firstMatch.frame)
            guard !viewport.isEmpty else { return false }
            let frame = control.exists ? control.frame : nil
            if let frame, !frame.isEmpty, viewport.contains(frame), control.isHittable { return true }
            let reach = viewport.height / 3
            let move = frame.map { max(-reach, min(reach, viewport.midY - $0.midY)) } ?? -reach
            #if os(macOS)
            scroll.scroll(byDeltaX: 0, deltaY: move)
            #else
            // A set distance and a hold: a swipe carries on past a row half the screen tall.
            let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 0, dy: move)),
                        withVelocity: .slow, thenHoldForDuration: 0.2)
            #endif
        }
        return false
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
            guard !viewport.isEmpty else { break }
            guard controls.allSatisfy({ $0.exists }) else {
                // A system list makes its rows as they scroll in: at the largest text sizes a later row is not there yet.
                guard gesture < 24 else { break }
                #if os(macOS)
                scroll.scroll(byDeltaX: 0, deltaY: -viewport.height / 2)
                #else
                scroll.swipeUp()
                #endif
                continue
            }
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
            #if os(macOS)
            let towardsBottom = target.maxY > viewport.maxY
            scroll.scroll(byDeltaX: 0, deltaY: (towardsBottom ? -1 : 1) * viewport.height / 2)
            #else
            // Drag by a set distance and hold. At the largest text sizes a row can be
            // more than half the screen tall, and a swipe overshoots it both ways.
            let reach = viewport.height / 3
            let move = max(-reach, min(reach, viewport.midY - target.midY))
            let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 0, dy: move)),
                        withVelocity: .slow, thenHoldForDuration: 0.2)
            #endif
        }
        XCTFail("Every required control must be fully inside the scroll viewport and hittable after at most 24 gestures: \(identifiers)\n\(app.debugDescription)")
    }
}
