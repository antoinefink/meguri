import Foundation
import XCTest
#if os(iOS)

/// Review tap zones (U180) on iPhone and iPad, by taps on the card only: a
/// tap anywhere reveals, the left half answers Again and the right half
/// Good, while taps on the card's own link and audio control do neither.
/// Undo takes back a tap answer as it does a button answer.
@MainActor
final class TapZoneUITests: XCTestCase {
    func testReviewingByTapsOnly() throws {
        let app = try launch()
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deck.row.", "Tap Zones")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20), "Stage the generated tap-zones-1 fixture in the VM")
        row.tap()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        study.tap()
        let card = element(app, "review.session.card")
        XCTAssertTrue(card.waitForExistence(timeout: 20))
        XCTAssertTrue(waitForQuestion(app), "The first card's question")
        capture(app, "01-question")
        XCTAssertEqual(answered(app), 0)

        // The card's link is the card's: no reveal.
        let link = moreLink(app)
        XCTAssertTrue(link.waitForExistence(timeout: 10), "The front's link")
        link.tap()
        XCTAssertFalse(waitForAnswer(app, timeout: 2), "A tap on the card's link does not reveal")
        capture(app, "02-question-after-link-tap")

        // A tap on the card reveals.
        tap(card, x: 0.25)
        XCTAssertTrue(waitForAnswer(app), "A tap on the question reveals")
        capture(app, "03-answer")

        // The audio control and the link are the card's: no answer.
        let play = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Play audio")).firstMatch
        XCTAssertTrue(play.waitForExistence(timeout: 10), "The back's audio control")
        play.tap()
        XCTAssertFalse(waitForAnswered(app, 1, timeout: 2), "A tap on the audio control does not answer")
        XCTAssertTrue(isAnswer(app))
        let answerLink = moreLink(app)
        XCTAssertTrue(answerLink.waitForExistence(timeout: 10))
        answerLink.tap()
        XCTAssertFalse(waitForAnswered(app, 1, timeout: 2), "A tap on the card's link does not answer")
        capture(app, "04-answer-after-control-taps")

        // The right half answers Good: the next card's question, and the
        // card is done for the session (the total stays).
        let total = self.total(app)
        tap(card, x: 0.8)
        XCTAssertTrue(waitForAnswered(app, 1), "A tap on the right half answers")
        XCTAssertEqual(self.total(app), total, "Good: the card leaves the session")
        XCTAssertTrue(waitForQuestion(app), "The next card's question")
        capture(app, "05-next-question-after-good")

        // Undo takes the tap answer back, as for a button.
        let undo = element(app, "review.session.back")
        XCTAssertTrue(waitForLabel(undo, "Undo Answer"), undo.label)
        undo.tap()
        XCTAssertTrue(waitForAnswered(app, 0), "Undo takes the answer back")
        capture(app, "06-after-undo")

        // Again on the left half.
        if isAnswer(app) == false {
            tap(card, x: 0.75)
            XCTAssertTrue(waitForAnswer(app))
        }
        let before = self.total(app)
        tap(card, x: 0.2)
        XCTAssertTrue(waitForAnswered(app, 1), "A tap on the left half answers")
        // Again: the card comes back as a learning card, so the total grows.
        XCTAssertEqual(self.total(app).map { $0 - 1 }, before, "Again: the card stays in the session")
        XCTAssertTrue(waitForQuestion(app))
        capture(app, "07-next-question-after-again")

        // Two more cards by taps alone.
        for count in 2...3 {
            tap(card, x: 0.5)
            XCTAssertTrue(waitForAnswer(app), "Reveal \(count)")
            tap(card, x: 0.85)
            XCTAssertTrue(waitForAnswered(app, count), "Good \(count)")
            XCTAssertTrue(waitForQuestion(app))
        }
        capture(app, "08-after-four-answers")
    }

    // MARK: Helpers

    /// The front's link ("more N": the session's card order is the scheduler's).
    private func moreLink(_ app: XCUIApplication) -> XCUIElement {
        app.links.matching(NSPredicate(format: "label BEGINSWITH %@", "more ")).firstMatch
    }

    private func tap(_ card: XCUIElement, x: CGFloat) {
        // Double Tap Prevention (250 ms after an answer, and after the answer
        // appears) is behaviour under test elsewhere: these taps come after it.
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        // Low on the card, below its text and above the answer area that
        // floats over the card's bottom (N16.9): plain card, not a control.
        let app = XCUIApplication()
        let bar = [element(app, "review.session.rate.good"), element(app, "review.session.reveal")]
            .filter { $0.exists && !$0.frame.isEmpty }.map(\.frame.minY).min() ?? card.frame.maxY
        let y = card.frame.minY + (bar - Self.answerAreaPadding - card.frame.minY) * 0.85
        card.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: card.frame.width * x, dy: y - card.frame.minY)).tap()
    }
    /// A margin above the answer area's buttons (its padding there is 12 pt).
    private static let answerAreaPadding: CGFloat = 16

    func testATapBesideTheAnswerButtonsDoesNothing() throws {
        let app = try launch()
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "home.deck.row.", "Tap Zones")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        row.tap()
        element(app, "home.deck.study").tap()
        let card = element(app, "review.session.card")
        XCTAssertTrue(card.waitForExistence(timeout: 20))
        XCTAssertTrue(waitForQuestion(app))
        // Level with Show Answer, beside it: no reveal.
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        let reveal = element(app, "review.session.reveal").frame
        app.windows.firstMatch.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: max(8, reveal.minX / 2), dy: reveal.midY)).tap()
        XCTAssertFalse(waitForAnswer(app, timeout: 2), "A tap beside Show Answer does not reveal")
        tap(card, x: 0.5)
        XCTAssertTrue(waitForAnswer(app))
        // Level with the buttons, left of Again in the area's padding: the
        // card shows there under the answer area, but a tap is never a grade.
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        let again = element(app, "review.session.rate.again").frame, good = element(app, "review.session.rate.good").frame
        let gapPoint = CGPoint(x: max(4, again.minX / 2), y: good.midY)
        app.windows.firstMatch.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: gapPoint.x, dy: gapPoint.y)).tap()
        XCTAssertFalse(waitForAnswered(app, 1, timeout: 2), "A tap beside the buttons does not answer")
        XCTAssertTrue(isAnswer(app))
        capture(app, "09-answer-after-tap-beside-buttons")
    }

    private func isAnswer(_ app: XCUIApplication) -> Bool {
        let good = element(app, "review.session.rate.good")
        return good.exists && good.isHittable && good.isEnabled
    }

    private func waitForAnswer(_ app: XCUIApplication, timeout: TimeInterval = 10) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if isAnswer(app) { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return false
    }

    private func waitForQuestion(_ app: XCUIApplication, timeout: TimeInterval = 10) -> Bool {
        let reveal = element(app, "review.session.reveal")
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if reveal.exists && reveal.isEnabled && !isAnswer(app) { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return false
    }

    /// Answers given, from the session progress ("2 of 6").
    private func answered(_ app: XCUIApplication) -> Int? {
        let progress = element(app, "review.session.progress")
        guard progress.exists, let value = progress.value as? String,
              let first = value.split(separator: " ").first else { return nil }
        return Int(first)
    }

    /// The session's size, from the progress ("2 of 6").
    private func total(_ app: XCUIApplication) -> Int? {
        let progress = element(app, "review.session.progress")
        guard progress.exists, let value = progress.value as? String,
              let last = value.split(separator: " ").last else { return nil }
        return Int(last)
    }

    private func waitForAnswered(_ app: XCUIApplication, _ count: Int, timeout: TimeInterval = 10) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if answered(app) == count { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return false
    }

    private func waitForLabel(_ element: XCUIElement, _ label: String, timeout: TimeInterval = 10) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", label), object: element)
        return XCTWaiter().wait(for: [expectation], timeout: timeout) == .completed
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "tap-zones-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func launch() throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        // A pool simulator can be left turned by an earlier run.
        XCUIDevice.shared.orientation = .portrait
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/tap-zones-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", "compact"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
#endif
