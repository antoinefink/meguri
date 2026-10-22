import Foundation
import XCTest

/// Run only in the UI VM through tools/mg a11y --suite LocalReviewAccessibilityTests.
@MainActor
final class LocalReviewAccessibilityTests: XCTestCase {
    func testLocalReviewScreensAtEveryTextSizeAndAppearance() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundleIdentifier = try build.bundleIdentifier
        guard bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let suppressions = try AuditSuppressions.bundled()
        let screens = ["home.deckList", "home.deckPage", "review.question", "review.answer"]
        var results: [String: [AccessibilityAudit.StateResult]] = [:]
        #if !os(macOS)
        let appearance = XCUIDevice.shared.appearance
        defer { XCUIDevice.shared.appearance = appearance }
        #endif

        for variant in AccessibilityAudit.Variant.all {
            variant.apply()
            let app = XCUIApplication()
            app.launchArguments = MeguriLaunch(
                fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
                now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
                locale: "en_US", animations: false, extra: variant.launchArguments
            ).arguments
            app.launch()
            defer { app.terminate() }
            addTeardownBlock { @MainActor in app.terminate() }

            let deck = app.buttons["home.deck.row.1884308400002"].firstMatch
            let enable = element(app, "home.policy.enable")
            let policy = element(app, "home.policy.sheet")
            try wait("collection ready or FSRS admission, \(variant)") {
                enable.isHittable || (deck.isHittable && !policy.exists)
            }
            if enable.isHittable {
                try app.bringToFront(bundleIdentifier: bundleIdentifier)
                enable.clickOrTap()
            }
            try wait("native Home, \(variant)") { deck.isHittable && !policy.exists }
            try require(element(app, "home.collection.screen"), "native Home, \(variant)")
            // Check the native Home background, never the user card's CSS.
            let shown = try AccessibilityAudit.Appearance.shown(by: app)
            guard shown.dark == variant.dark else {
                throw AuditError(description: "native Home, \(variant): appearance did not apply (median luminance \(shown.luminance))")
            }
            results["home.deckList", default: []].append(try audit(app, variant: variant, suppressions: suppressions))

            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            deck.clickOrTap()
            let study = element(app, "home.deck.study")
            try require(study, "deck page, \(variant)")
            try wait("Study enabled, \(variant)") { study.isEnabled && study.isHittable }
            results["home.deckPage", default: []].append(try audit(app, variant: variant, suppressions: suppressions))

            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            study.clickOrTap()
            let reveal = element(app, "review.session.reveal")
            try require(reveal, "question controls, \(variant)")
            let question = app.webViews.staticTexts.matching(NSPredicate(format: "label MATCHES %@", "Question [0-9]+")).firstMatch
            try require(question, "rendered question, \(variant)")
            try wait("question ready, \(variant)") { reveal.isEnabled && reveal.isHittable && question.isHittable }
            results["review.question", default: []].append(try audit(app, variant: variant, suppressions: suppressions))

            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            reveal.clickOrTap()
            let good = element(app, "review.session.rate.good")
            try require(good, "answer controls, \(variant)")
            let answer = app.webViews.staticTexts.matching(NSPredicate(format: "label MATCHES %@", "Answer [0-9]+")).firstMatch
            try require(answer, "rendered answer, \(variant)")
            try wait("answer ready, \(variant)") { good.isEnabled && good.isHittable && answer.isHittable }
            results["review.answer", default: []].append(try audit(app, variant: variant, suppressions: suppressions))
        }

        // variant throws above and cannot yield a passing partial report.
        for screen in screens {
            let states = try XCTUnwrap(results[screen])
            XCTAssertEqual(states.count, AccessibilityAudit.Variant.all.count, screen)
            add(AccessibilityAudit.screenReport(screen, states: states))
        }
        for screen in screens {
            for state in results[screen] ?? [] {
                AccessibilityAudit.assertClean(state.findings)
                TouchTargets.assertMet(state.touchTargets)
            }
        }
    }

    private func audit(_ app: XCUIApplication, variant: AccessibilityAudit.Variant,
                       suppressions: [AuditSuppression]) throws -> AccessibilityAudit.StateResult {
        let findings = try AccessibilityAudit.run(app, variant: variant.description, suppressions: suppressions)
        let targets = try TouchTargets.violations(in: app)
        return .init(state: variant.description, findings: findings, touchTargets: targets)
    }

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func require(_ element: XCUIElement, _ description: String) throws {
        guard element.waitForExistence(timeout: 20) else {
            throw AuditError(description: "\(description) did not appear; stage local-review-1 in the VM before this suite")
        }
    }

    private func wait(_ description: String, until ready: () -> Bool) throws {
        let deadline = Date().addingTimeInterval(20)
        while !ready() {
            guard Date() < deadline else {
                throw AuditError(description: "\(description) did not become ready")
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
    }
}
