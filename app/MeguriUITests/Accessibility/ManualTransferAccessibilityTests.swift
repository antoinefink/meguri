import XCTest

/// Initial all-types audits use the existing exact suppression receipt scopes.
/// No manual-transfer contrast or hit-target exception is added here.
@MainActor final class ManualTransferAccessibilityTests: XCTestCase {
    func testExplicitTransferSetupStatesAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires rendering routes") }
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["empty", "typical", "error", "loading"].map { state in
            try AccessibilityAudit.runAllVariants(state: state,
                launch: MeguriLaunch(locale: "en_US", animations: false, route: "settings/anki-transfer/\(state)"),
                waitFor: "settings.anki.content", suppressions: suppressions)
        }
        add(AccessibilityAudit.screenReport("settings.sync", states: states))
        for state in states { AccessibilityAudit.assertClean(state.findings); TouchTargets.assertMet(state.touchTargets) }
    }
}
