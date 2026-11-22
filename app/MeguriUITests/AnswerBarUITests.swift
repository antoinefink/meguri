import Foundation
import XCTest
#if os(iOS)

/// The review's answer area over the card (N16.1, N16.9): the card runs on
/// under the answers, its last line scrolls clear of them, and the progress
/// bar follows the answers after one standard spacing. Captures for the
/// owner's review: long, short, dark and coloured cards, light and dark.
@MainActor
final class AnswerBarUITests: XCTestCase {
    func testLongCardScrollsClearOfTheAnswersLight() throws {
        let app = try open("Bar Long", dark: false)
        capture(app, "long-light-question")
        try reveal(app)
        try checkSpacing(app)
        capture(app, "long-light-answer-top")
        let card = element(app, "review.session.card")
        card.swipeUp(velocity: .slow)
        settle()
        capture(app, "long-light-answer-middle")
        try scrollToLastLine(app, card)
        capture(app, "long-light-answer-bottom")
    }

    func testShortCardIsNotCoveredLight() throws {
        let app = try open("Bar Short", dark: false)
        try reveal(app)
        let last = lastLine(app)
        XCTAssertTrue(last.waitForExistence(timeout: 10))
        XCTAssertLessThanOrEqual(last.frame.maxY, answerAreaTop(app), "A short card is not covered at rest")
        try checkSpacing(app)
        capture(app, "short-light-answer")
    }

    func testColouredCardLight() throws {
        let app = try open("Bar Colour", dark: false)
        capture(app, "colour-light-question")
        try reveal(app)
        capture(app, "colour-light-answer-top")
        try scrollToLastLine(app, element(app, "review.session.card"))
        capture(app, "colour-light-answer-bottom")
    }

    func testDarkCardDark() throws {
        let app = try open("Bar Dark", dark: true)
        capture(app, "dark-dark-question")
        try reveal(app)
        capture(app, "dark-dark-answer-top")
        let card = element(app, "review.session.card")
        card.swipeUp(velocity: .slow)
        settle()
        capture(app, "dark-dark-answer-middle")
        try scrollToLastLine(app, card)
        capture(app, "dark-dark-answer-bottom")
    }

    func testLongCardDark() throws {
        let app = try open("Bar Long", dark: true)
        capture(app, "long-dark-question")
        try reveal(app)
        try checkSpacing(app)
        let card = element(app, "review.session.card")
        card.swipeUp(velocity: .slow)
        settle()
        capture(app, "long-dark-answer-middle")
    }

    // MARK: Checks

    /// One standard spacing (8 pt) from the answer buttons to the progress bar.
    private func checkSpacing(_ app: XCUIApplication) throws {
        let good = element(app, "review.session.rate.good"), progress = element(app, "review.session.progress")
        XCTAssertTrue(good.waitForExistence(timeout: 10) && progress.exists)
        let gap = progress.frame.minY - good.frame.maxY
        XCTAssertEqual(gap, 8, accuracy: 1, "Answers to progress bar: \(gap) pt")
        // Above the home indicator, never under it.
        let window = app.windows.firstMatch.frame
        XCTAssertLessThan(progress.frame.maxY, window.maxY, "The progress bar keeps the bottom safe area")
    }

    private func scrollToLastLine(_ app: XCUIApplication, _ card: XCUIElement) throws {
        let last = lastLine(app)
        for _ in 0..<15 {
            if last.exists, !last.frame.isEmpty, last.frame.maxY <= answerAreaTop(app) { break }
            card.swipeUp()
            settle()
        }
        XCTAssertTrue(last.exists, "The card's last line")
        XCTAssertLessThanOrEqual(last.frame.maxY, answerAreaTop(app), "The last line scrolls fully above the answers")
        XCTAssertGreaterThan(last.frame.minY, card.frame.minY, "and stays on screen")
    }

    // MARK: Helpers

    private func answerAreaTop(_ app: XCUIApplication) -> CGFloat {
        [element(app, "review.session.rate.good"), element(app, "review.session.reveal")]
            .filter { $0.exists && !$0.frame.isEmpty }.map(\.frame.minY).min() ?? app.windows.firstMatch.frame.maxY
    }

    private func lastLine(_ app: XCUIApplication) -> XCUIElement {
        app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", "The last line")).firstMatch
    }

    private func reveal(_ app: XCUIApplication) throws {
        let reveal = element(app, "review.session.reveal")
        XCTAssertTrue(reveal.waitForExistence(timeout: 20))
        reveal.tap()
        let good = element(app, "review.session.rate.good")
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline, !(good.exists && good.isHittable) { settle(0.1) }
        XCTAssertTrue(good.isHittable, "The answer side")
        settle()
    }

    private func settle(_ seconds: TimeInterval = 0.6) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "answer-bar-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func open(_ deck: String, dark: Bool) throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        XCUIDevice.shared.orientation = .portrait
        XCUIDevice.shared.appearance = dark ? .dark : .light
        addTeardownBlock { @MainActor in XCUIDevice.shared.appearance = .light }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/answer-bar-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", "compact"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deck.row.", deck)).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20), "Stage the generated answer-bar-1 fixture in the VM")
        row.tap()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        study.tap()
        XCTAssertTrue(element(app, "review.session.card").waitForExistence(timeout: 20))
        settle()
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
#endif
