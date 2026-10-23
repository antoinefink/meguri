import Foundation
import XCTest

@MainActor final class CollectionExportAccessibilityTests: XCTestCase {
    func testExportStatesPassAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("VM development build required") }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["typical", "error", "loading"].map { state in
            try AccessibilityAudit.runAllVariants(state: state,
                launch: MeguriLaunch(locale: "en_US", animations: false, route: "settings/import-export/\(state)"),
                waitFor: "settings.importExport.content", suppressions: suppressions,
                supplemental: { app, _ in
                    if state == "loading" {
                        let progress = app.descendants(matching: .any)["settings.importExport.progress"].firstMatch
                        XCTAssertTrue(progress.exists)
                        XCTAssertEqual(progress.elementType, .progressIndicator)
                        XCTAssertGreaterThanOrEqual(progress.frame.width, 44)
                        XCTAssertGreaterThanOrEqual(progress.frame.height, 44)
                        XCTAssertEqual(progress.label, "Preparing collection export…")
                    }
                    return AccessibilityAudit.StateResult(state: state, findings: [], touchTargets: [])
                })
        }
        add(AccessibilityAudit.screenReport("settings.importExport", states: states))
        for state in states { AccessibilityAudit.assertClean(state.findings); TouchTargets.assertMet(state.touchTargets) }
    }
}
