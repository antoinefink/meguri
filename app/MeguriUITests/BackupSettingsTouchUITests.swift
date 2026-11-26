#if os(iOS)
import Foundation
import Vision
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
        XCTAssertTrue(app.buttons["settings.backups.create"].firstMatch.waitForExistence(timeout: 10))
        let window = app.windows.firstMatch
        XCTAssertTrue(wait(timeout: 10) { app.state == .runningForeground && window.exists })
        let field = window.textFields["settings.backups.limit.interval"].firstMatch
        func visible() -> Bool {
            field.exists && !field.frame.isEmpty && window.frame.contains(field.frame)
        }
        if !visible() {
            for _ in 0..<4 {
                window.swipeUp()
                if wait(timeout: 1, visible) { break }
            }
        }
        XCTAssertTrue(wait(timeout: 10) {
            app.state == .runningForeground && visible() && field.isEnabled && field.isHittable
        }, "The native interval field must be visible and actionable")
        XCTAssertEqual(field.label, "Interval in minutes")
        XCTAssertEqual(try XCTUnwrap(field.value as? String), "30")

        let screenshot = window.screenshot()
        let attachment = XCTAttachment(screenshot: screenshot)
        attachment.name = "interval-native-window"
        attachment.lifetime = .keepAlways
        add(attachment)
        let image = try XCTUnwrap(screenshot.image.cgImage)
        let viewport = window.frame, frame = field.frame
        // Vision uses normalized image coordinates with the origin at the bottom left.
        let input = CGRect(x: (frame.minX - viewport.minX) / viewport.width,
                           y: (viewport.maxY - frame.maxY) / viewport.height,
                           width: frame.width / viewport.width, height: frame.height / viewport.height)
        let area = CGRect(x: 0, y: input.minY - input.height, width: 1, height: 3 * input.height)
            .intersection(CGRect(x: 0, y: 0, width: 1, height: 1))
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = ["en-US"]
        request.usesLanguageCorrection = false
        request.regionOfInterest = area
        var diagnostics = "Normalized bottom-left coordinates: field=\(input), area=\(area)\n"
        defer {
            let text = XCTAttachment(string: diagnostics)
            text.name = "interval-ocr"
            text.lifetime = .keepAlways
            add(text)
        }
        try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
        struct Word {
            let text: String
            let box: CGRect
            let observation: Int
            let range: Range<String.Index>
        }
        let words = try NSRegularExpression(pattern: #"(?i)\b(interval|30|min)\b"#)
        var recognized: [Word] = []
        for (index, observation) in try XCTUnwrap(request.results).enumerated() {
            let line = try XCTUnwrap(observation.topCandidates(1).first)
            diagnostics += "\(index): \(line.string.debugDescription) \(observation.boundingBox)\n"
            for match in words.matches(in: line.string, range: NSRange(line.string.startIndex..., in: line.string)) {
                let range = try XCTUnwrap(Range(match.range, in: line.string))
                let box = try XCTUnwrap(try line.boundingBox(for: range)).boundingBox
                let word = String(line.string[range]).lowercased()
                diagnostics += "  \(word): \(box)\n"
                if area.contains(CGPoint(x: box.midX, y: box.midY)) {
                    recognized.append(Word(text: word, box: box, observation: index, range: range))
                }
            }
        }
        func word(_ text: String) throws -> Word {
            let matches = recognized.filter { $0.text == text }
            XCTAssertEqual(matches.count, 1, "Expected one \(text) in the interval area; see interval-ocr")
            return try XCTUnwrap(matches.first)
        }
        let caption = try word("interval"), value = try word("30"), unit = try word("min")
        func precedes(_ left: Word, _ right: Word) -> Bool {
            if left.box.maxX <= right.box.minX { return true }
            // Accurate OCR can return the same word union for multiple substring ranges.
            return left.observation == right.observation && left.range.upperBound <= right.range.lowerBound
                && left.box.minX <= right.box.minX && left.box.maxX <= right.box.maxX
        }
        XCTAssertTrue(value.box.intersects(input), "OCR must identify the fixture field's 30; see interval-ocr")
        XCTAssertTrue(precedes(caption, value) && precedes(value, unit)
            && caption.box.minY <= value.box.midY && value.box.midY <= caption.box.maxY
            && unit.box.minY <= value.box.midY && value.box.midY <= unit.box.maxY
            // Reject a multi-line union even when Vision merges all three substrings.
            && [caption, value, unit].allSatisfy { input.minY <= $0.box.minY && $0.box.maxY <= input.maxY },
            "Interval, 30 and min must share one ordered row; see interval-ocr and interval-native-window")
    }

    private func wait(timeout: TimeInterval, _ condition: @escaping () -> Bool) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)],
            timeout: timeout) == .completed
    }
}
#endif
