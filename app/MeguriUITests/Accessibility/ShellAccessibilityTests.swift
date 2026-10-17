import Foundation
import XCTest

/// The app shell's states (`shell.root`: typical, loading, error, routes
/// `shell/root/<state>`) pass the audit and the touch-target check in every
/// variant (R1977, R1978, R1980). The iPhone audits the tab layout, the iPad
/// and the Mac the sidebar layout. Run with `tools/mg a11y --suite
/// ShellAccessibilityTests`.
@MainActor
final class ShellAccessibilityTests: XCTestCase {
    func testEveryStateOfTheShellPassesTheAuditAtEveryTextSizeAndAppearance() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["typical", "loading", "error"].map { state in
            try AccessibilityAudit.runAllVariants(
                state: state, launch: MeguriLaunch(locale: "en_US", animations: false, route: "shell/root/\(state)"),
                waitFor: "testhooks.shell.root.home", suppressions: suppressions
            )
        }
        add(AccessibilityAudit.screenReport("shell.root", states: states))
        for state in states {
            AccessibilityAudit.assertClean(state.findings)
            TouchTargets.assertMet(state.touchTargets)
        }
    }

    /// The split view's sidebar toggle is the system's (no identifier, in a
    /// navigation bar): the touch-target check leaves it out. The app's own
    /// small controls stay in (AccessibilityHelperTests' touch-target test).
    func testTheSystemsSidebarToggleIsLeftOutOfTheTouchTargetCheck() throws {
        #if os(macOS)
        throw XCTSkip("the Mac's sidebar toggle is in the window's toolbar, which the check measures")
        #else
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: "shell/root/typical", extra: ["-MeguriSizeClass", "regular"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let toggle = app.navigationBars.buttons.matching(NSPredicate(format: "identifier == ''")).firstMatch
        XCTAssert(toggle.waitForExistence(timeout: 15), "no system button in a navigation bar")
        let elements = TouchTargets.elements(of: try app.snapshot(), visible: app.windows.firstMatch.frame)
        XCTAssertFalse(elements.contains { $0.frame == toggle.frame }, "the check measures the system's \(toggle.label) button")
        #endif
    }
}
