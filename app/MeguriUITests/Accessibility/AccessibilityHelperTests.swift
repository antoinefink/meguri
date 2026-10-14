import Foundation
import XCTest
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// The accessibility helpers (R1977, R1978, R1980) on the sample routes.
/// Screen suites (AE88 and the sweep) belong to their UI units and U253.
@MainActor
final class AccessibilityHelperTests: XCTestCase {
    private func launch(_ route: String, extra: [String] = []) throws -> XCUIApplication {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: route, extra: extra).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    // MARK: Suppressions

    func testTheCommittedSuppressionsLoad() throws {
        XCTAssertNoThrow(try AuditSuppressions.bundled())
    }

    func testASuppressionWithoutAReasonFailsAtLoad() {
        let json = #"{"version": 1, "suppressions": [{"identifier": "home.deckList.row.42", "auditType": "contrast"}]}"#
        XCTAssertThrowsError(try AuditSuppressions.parse(Data(json.utf8))) { error in
            XCTAssertEqual("\(error)", "suppression 1 (home.deckList.row.42, contrast): has no reason; every suppression needs a written reason (R1977)")
        }
        let blank = #"{"version": 1, "suppressions": [{"identifier": "home.deckList.row.42", "auditType": "contrast", "reason": "  "}]}"#
        XCTAssertThrowsError(try AuditSuppressions.parse(Data(blank.utf8)))
    }

    func testASuppressionNamesOneElementAndOneKnownAuditType() {
        let noElement = #"{"version": 1, "suppressions": [{"auditType": "contrast", "reason": "r"}]}"#
        XCTAssertThrowsError(try AuditSuppressions.parse(Data(noElement.utf8))) { error in
            XCTAssertTrue("\(error)".contains("exactly one element"), "\(error)")
        }
        let wildcard = #"{"version": 1, "suppressions": [{"identifier": "home.*", "auditType": "contrast", "reason": "r"}]}"#
        XCTAssertThrowsError(try AuditSuppressions.parse(Data(wildcard.utf8)))
        let unknownType = #"{"version": 1, "suppressions": [{"identifier": "a.b.c", "auditType": "all", "reason": "r"}]}"#
        XCTAssertThrowsError(try AuditSuppressions.parse(Data(unknownType.utf8))) { error in
            XCTAssertTrue("\(error)".contains("audit type must be one of"), "\(error)")
        }
        let valid = #"{"version": 1, "suppressions": [{"identifier": "a.b.c", "auditType": "hitRegion", "reason": "r"}]}"#
        XCTAssertEqual(try AuditSuppressions.parse(Data(valid.utf8)), [AuditSuppression(identifier: "a.b.c", auditType: "hitRegion", reason: "r")])
    }

    // MARK: Touch targets and the audit

    func testTheTouchTargetCheckNamesASmallButtonAndItsSize() throws {
        let app = try launch("sample/touch-targets")
        XCTAssert(app.buttons["sample.touchTargets.large"].waitForExistence(timeout: 15))
        let violations = try TouchTargets.violations(in: app)
        let small = try XCTUnwrap(violations.first { $0.identifier == "sample.touchTargets.small" }, "\(violations)")
        // The 20 pt button reports its symbol's frame, at most 20 pt a side.
        XCTAssertLessThanOrEqual(small.size.width, 20.5)
        XCTAssertLessThanOrEqual(small.size.height, 20.5)
        let size = "\(Int(small.size.width.rounded()))×\(Int(small.size.height.rounded())) pt"
        #if os(macOS)
        XCTAssertEqual(small.description, "sample.touchTargets.small is \(size); the minimum here is 28×28 pt (R1980)")
        #else
        XCTAssertEqual(small.description, "sample.touchTargets.small is \(size); the minimum here is 44×44 pt (R1980)")
        #endif
        XCTAssertFalse(violations.contains { $0.identifier == "sample.touchTargets.large" }, "\(violations)")
    }

    func testTheAuditsHitRegionIssueStaysUnsuppressedWithoutAReasonedEntry() throws {
        #if os(macOS)
        throw XCTSkip("the Mac audit reports no hitRegion issue for the sample's 20 pt button; the touch-target check covers the Mac (28 pt)")
        #else
        let app = try launch("sample/touch-targets")
        XCTAssert(app.buttons["sample.touchTargets.small"].waitForExistence(timeout: 15))
        let unsuppressed = try AccessibilityAudit.run(app, variant: "default", types: .hitRegion, suppressions: [])
        let issue = try XCTUnwrap(unsuppressed.first { $0.identifier == "sample.touchTargets.small" && $0.auditType == "hitRegion" }, "\(unsuppressed)")
        XCTAssertFalse(issue.suppressed)
        // Only an entry naming that element and that type, with a reason,
        // suppresses it.
        let other = try AccessibilityAudit.run(app, variant: "default", types: .hitRegion, suppressions: [
            AuditSuppression(identifier: "sample.touchTargets.small", auditType: "contrast", reason: "another type"),
            AuditSuppression(identifier: "sample.touchTargets.large", auditType: "hitRegion", reason: "another element"),
        ])
        XCTAssertTrue(other.contains { $0.identifier == "sample.touchTargets.small" && !$0.suppressed })
        let named = try AccessibilityAudit.run(app, variant: "default", types: .hitRegion, suppressions: [
            AuditSuppression(identifier: "sample.touchTargets.small", auditType: "hitRegion", reason: "a deliberate defect of the sample"),
        ])
        XCTAssertTrue(named.filter { $0.identifier == "sample.touchTargets.small" }.allSatisfy(\.suppressed))
        #endif
    }

    /// Every state of the sample screen (`sample/deck-overview/<state>`, one
    /// route per PreviewState) passes the audit and the touch-target check
    /// in every variant; the report lists each state.
    func testEveryStateOfTheSampleScreenPassesTheAuditAtEveryTextSizeAndAppearance() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        // The first Mac UI sweep found sufficientElementDescription and
        // contrast issues on elements with no identifier and no label,
        // outside the sample's own (identified) elements; the findings now
        // carry each element's type and frame, and U253's sweep decides how
        // the Mac audit treats them.
        throw XCTSkip("Mac audit findings on unidentified elements are left to U253's sweep")
        #else
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["empty", "typical", "overflow", "error", "loading"].map { state in
            try AccessibilityAudit.runAllVariants(
                state: state, launch: MeguriLaunch(locale: "en_US", animations: false, route: "sample/deck-overview/\(state)"),
                waitFor: "sample.deckOverview.screen", suppressions: suppressions
            )
        }
        add(AccessibilityAudit.screenReport("sample.deckOverview", states: states))
        for state in states {
            AccessibilityAudit.assertClean(state.findings)
            TouchTargets.assertMet(state.touchTargets)
        }
        #endif
    }

    /// The report fails a screen when one state fails, and names the state.
    func testTheScreenReportFailsAScreenWithOneFailingState() throws {
        let small = TouchTargetViolation(identifier: "sample.deckOverview.retry", label: "Try Again", size: CGSize(width: 64, height: 18), minimum: CGSize(width: 44, height: 44))
        let data = AccessibilityAudit.screenReportData("sample.deckOverview", states: [
            .init(state: "typical", findings: [], touchTargets: []),
            .init(state: "error", findings: [], touchTargets: [small]),
        ])
        let json = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(json["status"] as? String, "fail")
        let states = try XCTUnwrap(json["states"] as? [String: [String: Any]])
        XCTAssertEqual(states["typical"]?["status"] as? String, "pass")
        XCTAssertEqual(states["error"]?["status"] as? String, "fail")
        XCTAssertEqual(json["touchTargets"] as? [String], ["error: sample.deckOverview.retry is 64×18 pt; the minimum here is 44×44 pt (R1980)"])
    }

    /// The appearance check reads a dark screen as dark and a light one as light.
    func testTheAppearanceCheckTellsDarkFromLight() throws {
        for (white, dark) in [(CGFloat(0.05), true), (CGFloat(0.97), false)] {
            #if os(macOS)
            let image = NSImage(size: CGSize(width: 40, height: 40), flipped: false) { rect in
                NSColor(white: white, alpha: 1).setFill()
                NSBezierPath(rect: rect).fill()
                return true
            }
            #else
            let image = UIGraphicsImageRenderer(size: CGSize(width: 40, height: 40)).image { context in
                UIColor(white: white, alpha: 1).setFill()
                context.fill(CGRect(x: 0, y: 0, width: 40, height: 40))
            }
            #endif
            XCTAssertEqual(try AccessibilityAudit.Appearance.medianLuminance(image) < 0.5, dark)
        }
    }

    // MARK: VoiceOver

    func testVoiceOverReadsTheSampleScreenInOrder() throws {
        let app = try launch("sample/deck-overview/typical")
        XCTAssert(app.buttons["sample.deckOverview.study"].waitForExistence(timeout: 15))
        let reading = try VoiceOver.read(app)
        let report = XCTAttachment(string: "source: \(reading.source)\n" + reading.utterances.joined(separator: "\n"))
        report.name = "voiceover-reading"
        report.lifetime = .keepAlways
        add(report)
        VoiceOver.assertOrder(reading, contains: ["Spanish", "Study Now"])
    }

    /// Elements that say the same thing are all read: a repeated label does
    /// not end the reading.
    func testVoiceOverKeepsRepeatedLabels() {
        var script = ["Delete, Button", "Delete, Button", "Delete, Button", "Done, Button"][...]
        let reading = VoiceOver.traverse(first: "Spanish, Heading", limit: 60) { script.popFirst() }
        XCTAssertEqual(reading, ["Spanish, Heading", "Delete, Button", "Delete, Button", "Delete, Button", "Done, Button"])
    }

    /// A reading that runs to the limit because VoiceOver wrapped around to
    /// the start is cut where it starts over; one that does not repeat stops
    /// at the limit.
    func testVoiceOverEndsAtTheLimitAndCutsAWrapAround() {
        let cycle = ["Spanish, Heading", "Delete, Button", "Delete, Button", "Done, Button"]
        var step = 0
        let wrapped = VoiceOver.traverse(first: cycle[0], limit: 20) {
            step += 1
            return cycle[step % cycle.count]
        }
        XCTAssertEqual(wrapped, cycle)
        var count = 0
        let long = VoiceOver.traverse(first: "Row 0", limit: 5) {
            count += 1
            return "Row \(count)"
        }
        XCTAssertEqual(long, ["Row 0", "Row 1", "Row 2", "Row 3", "Row 4", "Row 5"])
    }
}
