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
        checkBlurBehindButtons(app)
        try scrollToLastLine(app, card)
        capture(app, "long-light-answer-bottom")
    }

    /// About one screen long: its last lines would sit under the answers at rest.
    func testMediumCardScrollsClearOfTheAnswersLight() throws {
        let app = try open("Bar Medium", dark: false)
        try reveal(app)
        capture(app, "medium-light-answer-top")
        try scrollToLastLine(app, element(app, "review.session.card"))
        capture(app, "medium-light-answer-bottom")
    }

    /// A tap just right of Easy on a new card: Easy's own (the button's hit
    /// region reaches past its shape) sends the card out of the session; a
    /// tap that reached the card's right-half zone would answer Good and
    /// keep it, as a learning card, in the session.
    func testATapBesideEasyIsEasysNotTheCards() throws {
        let app = try open("Bar New", dark: false)
        try reveal(app)
        let progress = element(app, "review.session.progress")
        let total = Int((progress.value as? String)?.split(separator: " ").last ?? "")
        let easy = element(app, "review.session.rate.easy").frame, window = app.windows.firstMatch
        settle(0.4)
        window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: min(window.frame.maxX - 2, easy.maxX + 8) - window.frame.minX, dy: easy.midY - window.frame.minY)).tap()
        settle(1.5)
        let after = (progress.value as? String) ?? ""
        capture(app, "new-light-after-tap-beside-easy")
        let answered = Int(after.split(separator: " ").first ?? ""), totalAfter = Int(after.split(separator: " ").last ?? "")
        XCTAssertNotNil(total, "The progress before: \(String(describing: progress.value))")
        XCTAssertNotNil(answered, "The progress after: \(after)")
        // Either the area kept the tap (nothing answered), or Easy's own hit
        // region took it (the card leaves: same total). Never a zone's Good.
        XCTAssertTrue(answered == 0 || (answered == 1 && totalAfter == total), "Not Good from a zone: \(after)")
    }

    func testShortCardIsNotCoveredDark() throws {
        let app = try open("Bar Short", dark: true)
        try reveal(app)
        try checkSpacing(app)
        capture(app, "short-dark-answer")
    }

    func testShortCardIsNotCoveredLight() throws {
        let app = try open("Bar Short", dark: false)
        capture(app, "short-light-question")
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
        checkBlurBehindButtons(app)
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
        checkBlurBehindButtons(app)
    }

    // MARK: Checks

    /// The progress bar lies on the screen's very bottom edge, under the
    /// home indicator; the buttons sit just clear of the indicator's strip.
    private func checkSpacing(_ app: XCUIApplication) throws {
        let good = element(app, "review.session.rate.good"), progress = element(app, "review.session.progress")
        XCTAssertTrue(good.waitForExistence(timeout: 10) && progress.exists)
        let window = app.windows.firstMatch.frame
        XCTAssertEqual(progress.frame.maxY, window.maxY, accuracy: 1, "The bar's bottom is the screen's: \(progress.frame)")
        XCTAssertLessThanOrEqual(progress.frame.height, 12, "The bar's element is the bar, not the answer area: \(progress.frame)")
        // Every pool device has a home indicator: its strip (20 pt on iPad,
        // 34 pt on iPhone) is the only space between the buttons and the bar.
        let strip = window.maxY - good.frame.maxY
        XCTAssertTrue((20...40).contains(strip), "Buttons to the screen's bottom: \(strip) pt")
    }

    /// Behind each button the card shows only as texture (N16.9): in
    /// the parts of each button away from its label, the luminance spread
    /// stays small; sharp text behind (black on white, white on black) would
    /// span most of the range.
    private func checkBlurBehindButtons(_ app: XCUIApplication) {
        let screenshot = app.screenshot()
        guard let image = screenshot.image.cgImage else { return XCTFail("No screenshot pixels") }
        let scale = CGFloat(image.width) / app.windows.firstMatch.frame.width
        let width = image.width, height = image.height
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                                          bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            // A bitmap context's first row in memory is the image's top row.
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        guard drawn else { return XCTFail("No pixel context") }
        func luminance(_ x: Int, _ y: Int) -> Double {
            let i = (y * width + x) * 4
            return (0.2126 * Double(pixels[i]) + 0.7152 * Double(pixels[i + 1]) + 0.0722 * Double(pixels[i + 2])) / 255
        }
        for id in ["again", "hard", "good", "easy"] {
            let frame = element(app, "review.session.rate.\(id)").frame
            guard !frame.isEmpty else { continue }
            // The outer fifth on each side of the button, around its middle row.
            var low = 1.0, high = 0.0
            for band in [frame.minX + frame.width * 0.06 ... frame.minX + frame.width * 0.2,
                         frame.maxX - frame.width * 0.2 ... frame.maxX - frame.width * 0.06] {
                for px in stride(from: band.lowerBound, through: band.upperBound, by: 1) {
                    for py in stride(from: frame.midY - 8, through: frame.midY + 8, by: 1) {
                        let value = luminance(Int(px * scale), Int(py * scale))
                        low = min(low, value); high = max(high, value)
                    }
                }
            }
            XCTAssertLessThan(high - low, 0.3, "Luminance spread behind \(id): \(high - low)")
        }
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
