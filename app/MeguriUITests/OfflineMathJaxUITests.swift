#if os(iOS)
import Foundation
import XCTest

/// Focused iOS VM route: a synthetic collection, real native review controls
/// and production CardRendering. MG discovers and stages this recipe path.
@MainActor final class OfflineMathJaxUITests: XCTestCase {
    func testQuestionAndAnswerScreenshotsWithRenderedMath() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Requires a hook build in an isolated development VM container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/mathjax-review-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", "compact"]
        ).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deck.row.", "Offline Mathematics")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        row.clickOrTap()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        study.clickOrTap()
        let reveal = element(app, "review.session.reveal")
        XCTAssertTrue(reveal.waitForExistence(timeout: 20))
        // These headings change only after the fixture's onShownHook has
        // checked real SVG paths, nonzero layout, assistive MathML and no
        // merror in the production DOM. Source TeX alone cannot pass.
        let question = app.webViews.staticTexts["Reaction yield"]
        XCTAssertTrue(question.waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertTrue(question.isHittable && reveal.isEnabled)
        capture(app, "offline-mathjax-question-reaction-rate")
        reveal.clickOrTap()
        let good = element(app, "review.session.rate.good")
        XCTAssertTrue(good.waitForExistence(timeout: 20))
        let answer = app.webViews.staticTexts["Product concentration"]
        XCTAssertTrue(answer.waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertTrue(question.exists, "The answer must retain the complete question through FrontSide")
        let result = app.webViews.staticTexts["About 3.79 millimoles per litre."]
        XCTAssertTrue(result.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(answer.isHittable && result.isHittable && good.isEnabled,
                      "The complete worked answer and native rating controls must be visible")
        capture(app, "offline-mathjax-answer-worked-integral")
    }

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
