#if os(iOS)
import Foundation
import XCTest

@MainActor
final class BackupSettingsTouchUITests: XCTestCase {
    func testDefaultTextIntervalLabelFieldAndUnitShareARow() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        let app = XCUIApplication(bundleIdentifier: try build.bundleIdentifier)
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
            route: "settings/backups/typical",
            extra: ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryL"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let field = app.textFields["settings.backups.limit.interval"].firstMatch
        let label = app.staticTexts["Interval"].firstMatch
        let unit = app.staticTexts["min"].firstMatch
        XCTAssertTrue(app.buttons["settings.backups.create"].firstMatch.waitForExistence(timeout: 10))
        func visible() -> Bool {
            let viewport = app.windows.firstMatch.frame
            return [field, label, unit].allSatisfy {
                $0.exists && !$0.frame.isEmpty && viewport.contains($0.frame)
            } && field.isHittable
        }
        for _ in 0..<6 {
            if wait(timeout: 1, visible) { break }
            app.swipeUp()
        }
        XCTAssertTrue(wait(timeout: 10, visible), "Bring the existing interval controls into view")
        XCTAssertTrue(wait(timeout: 10) {
            let input = field.frame, caption = label.frame, suffix = unit.frame
            // Relative native AX geometry, independent of font metrics and device pixels.
            return caption.maxX <= input.minX && input.maxX <= suffix.minX
                && caption.minY <= input.midY && input.midY <= caption.maxY
                && suffix.minY <= input.midY && input.midY <= suffix.maxY
        }, "At default type, Interval, its number and min must align on one line")
    }

    private func wait(timeout: TimeInterval, _ condition: @escaping () -> Bool) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)],
            timeout: timeout) == .completed
    }
}
#endif
