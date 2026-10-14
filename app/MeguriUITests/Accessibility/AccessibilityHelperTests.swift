import Foundation
import XCTest

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
    }

    func testTheSampleScreenPassesTheAuditAtEveryTextSizeAndAppearance() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        let findings = try AccessibilityAudit.runAllVariants(
            launch: MeguriLaunch(locale: "en_US", animations: false, route: "sample/deck-overview/typical"),
            waitFor: "sample.deckOverview.title",
            suppressions: try AuditSuppressions.bundled()
        )
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: "sample/deck-overview/typical").arguments
        app.launch()
        defer { app.terminate() }
        XCTAssert(app.buttons["sample.deckOverview.study"].waitForExistence(timeout: 15))
        let touchTargets = try TouchTargets.violations(in: app)
        add(AccessibilityAudit.screenReport("sample.deckOverview", findings: findings, touchTargets: touchTargets))
        AccessibilityAudit.assertClean(findings)
        TouchTargets.assertMet(touchTargets)
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
}
