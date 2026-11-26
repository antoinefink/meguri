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
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
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

    func testAnAuditWithoutAnElementReportsNotSuppliedWithoutLookup() throws {
        var lookups = 0
        let finding = AccessibilityAudit.capture(
            Optional<Int>.none, variant: "default", auditType: "contrast", detail: "Contrast failed",
            suppressions: [AuditSuppression(identifier: "", auditType: "contrast", reason: "synthetic")]
        ) { _ in
            lookups += 1
            throw NSError(domain: "must.not.be.called", code: 1)
        }
        XCTAssertEqual(lookups, 0)
        XCTAssertFalse(finding.suppressed)
        XCTAssertEqual(finding.description, "default: contrast on an element without identifier (\"\", no element): Contrast failed")
        var expected = unresolvedReportFields
        expected["elementResolution"] = "notSupplied"
        #if os(macOS)
        XCTAssertFalse(finding.counts)
        let status = "pass"
        #else
        XCTAssertTrue(finding.counts)
        let status = "fail"
        #endif
        XCTAssertEqual(try resolutionReport(finding, status: status) as NSDictionary, expected as NSDictionary)
    }

    func testAResolvedAuditElementKeepsItsSnapshotAndReportFields() throws {
        var lookups = 0
        let finding = AccessibilityAudit.capture(
            17, variant: "default", auditType: "contrast", detail: "Contrast failed",
            suppressions: [AuditSuppression(identifier: "sample.retry", auditType: "contrast", reason: "synthetic")]
        ) { element in
            lookups += 1
            XCTAssertEqual(element, 17)
            return .init(identifier: "sample.retry", label: "Try Again", elementType: .button, frame: CGRect(x: 12, y: 34, width: 56, height: 78))
        }
        XCTAssertEqual(lookups, 1)
        XCTAssertTrue(finding.suppressed)
        XCTAssertFalse(finding.counts)
        XCTAssertNil(finding.excluded)
        XCTAssertEqual(finding.description, "default: contrast on sample.retry: Contrast failed")
        let expected: [String: Any] = [
            "state": "typical", "variant": "default", "identifier": "sample.retry", "label": "Try Again",
            "element": "type \(XCUIElement.ElementType.button.rawValue) at 12,34 56×78", "auditType": "contrast",
            "detail": "Contrast failed", "suppressed": true, "elementResolution": "resolved",
        ]
        XCTAssertEqual(try resolutionReport(finding, status: "pass") as NSDictionary, expected as NSDictionary)
    }

    func testAFailedAuditSnapshotReportsOnlyItsNSErrorDomainAndSignedCode() throws {
        var lookups = 0
        let finding = AccessibilityAudit.capture(
            17, variant: "default", auditType: "contrast", detail: "Contrast failed",
            suppressions: [AuditSuppression(identifier: "", auditType: "contrast", reason: "synthetic")]
        ) { _ in
            lookups += 1
            throw NSError(domain: "meguri.synthetic.snapshot", code: -1977, userInfo: [
                NSLocalizedDescriptionKey: "private localized description",
                NSDebugDescriptionErrorKey: "private debug description",
                "arbitraryPayload": ["secret": "private userInfo"],
            ])
        }
        XCTAssertEqual(lookups, 1)
        XCTAssertFalse(finding.suppressed)
        XCTAssertEqual(finding.description, "default: contrast on an element without identifier (\"\", no element): Contrast failed")
        var expected = unresolvedReportFields
        expected["elementResolution"] = "snapshotFailed"
        expected["snapshotErrorDomain"] = "meguri.synthetic.snapshot"
        expected["snapshotErrorCode"] = -1977
        #if os(macOS)
        XCTAssertFalse(finding.counts)
        let status = "pass"
        #else
        XCTAssertTrue(finding.counts)
        let status = "fail"
        #endif
        let report = try resolutionReport(finding, status: status)
        XCTAssertEqual(report["snapshotErrorCode"] as? Int, -1977)
        XCTAssertEqual(report as NSDictionary, expected as NSDictionary)
    }

    private var unresolvedReportFields: [String: Any] {
        var fields: [String: Any] = [
            "state": "typical", "variant": "default", "identifier": "", "label": "", "element": "no element",
            "auditType": "contrast", "detail": "Contrast failed", "suppressed": false,
        ]
        #if os(macOS)
        fields["excluded"] = "a window element with neither identifier nor label, not the app's (U253's sweep)"
        #endif
        return fields
    }

    private func resolutionReport(_ finding: AuditFinding, status: String) throws -> [String: Any] {
        let data = AccessibilityAudit.screenReportData("sample.deckOverview", states: [
            .init(state: "typical", findings: [finding], touchTargets: []),
        ])
        let json = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(json["status"] as? String, status)
        let states = try XCTUnwrap(json["states"] as? [String: [String: Any]])
        XCTAssertEqual(states["typical"]?["status"] as? String, status)
        let findings = try XCTUnwrap(json["findings"] as? [[String: Any]])
        XCTAssertEqual(findings.count, 1)
        let nested = try XCTUnwrap(states["typical"]?["findings"] as? [[String: Any]])
        XCTAssertEqual(nested as NSArray, findings as NSArray)
        return try XCTUnwrap(findings.first)
    }

    /// On the Mac, findings on the window's own elements (no identifier and
    /// no label, or `_XCUI:` controls) are listed, not failed; an element
    /// with an identifier or a label is the app's and fails. iOS fails every
    /// finding.
    func testOnlyTheWindowsOwnElementsAreLeftOutOfTheMacAudit() throws {
        #if os(macOS)
        XCTAssertNotNil(AccessibilityAudit.systemElement(identifier: "", label: ""))
        XCTAssertNotNil(AccessibilityAudit.systemElement(identifier: "_XCUI:CloseWindow", label: "close button"))
        #else
        XCTAssertNil(AccessibilityAudit.systemElement(identifier: "", label: ""))
        #endif
        XCTAssertNil(AccessibilityAudit.systemElement(identifier: "", label: "No Cards"))
        XCTAssertNil(AccessibilityAudit.systemElement(identifier: "sample.deckOverview.retry", label: ""))
        func finding(label: String) -> AuditFinding {
            AuditFinding(variant: "empty, light", identifier: "", label: label, element: "type 48 at 0,0 10×10", auditType: "contrast",
                         detail: "Contrast failed", suppressed: false, excluded: AccessibilityAudit.systemElement(identifier: "", label: label))
        }
        func status(_ findings: [AuditFinding]) throws -> String? {
            let data = AccessibilityAudit.screenReportData("sample.deckOverview", states: [.init(state: "empty", findings: findings, touchTargets: [])])
            return try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])["status"] as? String
        }
        #if os(macOS)
        XCTAssertEqual(try status([finding(label: "")]), "pass")
        #else
        XCTAssertEqual(try status([finding(label: "")]), "fail")
        #endif
        XCTAssertEqual(try status([finding(label: "No Cards")]), "fail")
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
        let report = XCTAttachment(string: "source: \(reading.source)\(reading.complete ? "" : " (stopped at the step limit)")\n" + reading.utterances.joined(separator: "\n"))
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
        XCTAssertEqual(reading, VoiceOver.Traversal(utterances: ["Spanish, Heading", "Delete, Button", "Delete, Button", "Delete, Button", "Done, Button"], complete: true))
    }

    /// A reading that runs to the limit keeps every utterance and says it
    /// stopped there: 61 elements that all say "Delete, Button" are 61
    /// utterances, not one, and a repeating sequence is not taken for a
    /// wrap-around (the service names no element).
    func testVoiceOverKeepsAReadingThatRunsToTheLimit() {
        let same = VoiceOver.traverse(first: "Delete, Button", limit: 60) { "Delete, Button" }
        XCTAssertEqual(same.utterances.count, 61)
        XCTAssertTrue(same.utterances.allSatisfy { $0 == "Delete, Button" })
        XCTAssertFalse(same.complete)
        let cycle = ["Spanish, Heading", "Delete, Button", "Delete, Button", "Done, Button"]
        var step = 0
        let repeating = VoiceOver.traverse(first: cycle[0], limit: 11) {
            step += 1
            return cycle[step % cycle.count]
        }
        XCTAssertEqual(repeating, VoiceOver.Traversal(utterances: cycle + cycle + cycle, complete: false))
        var count = 0
        let long = VoiceOver.traverse(first: "Row 0", limit: 5) {
            count += 1
            return "Row \(count)"
        }
        XCTAssertEqual(long, VoiceOver.Traversal(utterances: ["Row 0", "Row 1", "Row 2", "Row 3", "Row 4", "Row 5"], complete: false))
    }
}
