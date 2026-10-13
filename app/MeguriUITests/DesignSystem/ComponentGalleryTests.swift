import Foundation
import XCTest

/// The component gallery route (U9, R1999): launched through the test hook,
/// it shows every registered component state under
/// `designsystem.gallery.<component>.<state>`. The gallery lists its entries
/// in the summary line's accessibility value; this test checks that list
/// covers the components U9 ships and that each entry is on the screen.
@MainActor
final class ComponentGalleryTests: XCTestCase {
    /// The shared components and chrome U9 provides (plan, U9 Files).
    static let plannedComponents = [
        "countColumns", "sectionHeader", "emptyState", "errorState", "loadingState", "toast", "progressBar",
        "destructiveConfirmation", "controlBar", "floatingControl",
    ]

    func testGalleryRouteShowsEveryRegisteredComponentState() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: "designsystem/gallery").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }

        let summary = app.staticTexts["designsystem.gallery.summary"]
        XCTAssert(summary.waitForExistence(timeout: 15), "the gallery is not on screen")
        let identifiers = (summary.value as? String ?? "").split(separator: " ").map(String.init)
        continueAfterFailure = true

        for component in Self.plannedComponents {
            XCTAssert(identifiers.contains { $0.hasPrefix("designsystem.gallery.\(component).") }, "no \(component) entry in \(identifiers)")
        }
        let pattern = /designsystem\.gallery\.[a-z][A-Za-z0-9]*\.[a-z][A-Za-z0-9]*/
        for identifier in identifiers {
            XCTAssertNotNil(identifier.wholeMatch(of: pattern), "\(identifier) does not follow designsystem.gallery.<component>.<state>")
            let entry = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
            XCTAssert(entry.exists, "\(identifier) is listed but not on the gallery screen")
        }
    }
}
