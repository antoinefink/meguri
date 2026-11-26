#if os(iOS)
import Foundation
import UIKit
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
        let scaleX = CGFloat(image.width) / viewport.width
        let scaleY = CGFloat(image.height) / viewport.height
        // CGImage cropping uses pixels with a top-left origin. Keep the full row
        // width and one field-height of context above and below the anchored field.
        let fieldPixels = CGRect(x: (frame.minX - viewport.minX) * scaleX,
                                 y: (frame.minY - viewport.minY) * scaleY,
                                 width: frame.width * scaleX, height: frame.height * scaleY)
        let cropPixels = CGRect(x: 0, y: fieldPixels.minY - fieldPixels.height,
                                width: CGFloat(image.width), height: 3 * fieldPixels.height).integral
            .intersection(CGRect(x: 0, y: 0, width: CGFloat(image.width), height: CGFloat(image.height)))
        var diagnostics = "Window=\(viewport), field=\(frame), image=\(image.width)x\(image.height)\n"
        diagnostics += "Scale=\(scaleX),\(scaleY); top-left pixels: field=\(fieldPixels), crop=\(cropPixels)\n"
        func attachDiagnostics() {
            let text = XCTAttachment(string: diagnostics)
            text.name = "interval-ocr"
            text.lifetime = .keepAlways
            add(text)
        }
        guard let cropped = image.cropping(to: cropPixels) else {
            attachDiagnostics()
            XCTFail("Could not crop the interval row; see interval-ocr")
            return
        }
        let cropAttachment = XCTAttachment(image: UIImage(cgImage: cropped))
        cropAttachment.name = "interval-native-row-crop"
        cropAttachment.lifetime = .keepAlways
        add(cropAttachment)
        // Every OCR box and the field anchor now uses this cropped image's
        // normalized Vision coordinates, with the origin at the bottom left.
        let input = CGRect(x: (fieldPixels.minX - cropPixels.minX) / cropPixels.width,
                           y: (cropPixels.maxY - fieldPixels.maxY) / cropPixels.height,
                           width: fieldPixels.width / cropPixels.width, height: fieldPixels.height / cropPixels.height)
        diagnostics += "Crop image=\(cropped.width)x\(cropped.height); normalized bottom-left field=\(input)\n"
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = ["en-US"]
        request.usesLanguageCorrection = false
        request.minimumTextHeight = 0.01
        do {
            try VNImageRequestHandler(cgImage: cropped, options: [:]).perform([request])
        } catch {
            diagnostics += "Recognition failed: \(error)\n"
            attachDiagnostics()
            throw error
        }
        struct Word {
            let text: String
            let box: CGRect
            let observation: Int
            let range: Range<String.Index>
        }
        let words = try NSRegularExpression(pattern: #"(?i)\b(interval|30|min)\b"#)
        var recognized: [Word] = []
        diagnostics += "Observations=\(request.results?.count ?? 0), minimumTextHeight=\(request.minimumTextHeight)\n"
        for (index, observation) in (request.results ?? []).enumerated() {
            let candidates = observation.topCandidates(3)
            diagnostics += "\(index): box=\(observation.boundingBox), candidates=\(candidates.map { $0.string.debugDescription })\n"
            guard let line = candidates.first else { continue }
            for match in words.matches(in: line.string, range: NSRange(line.string.startIndex..., in: line.string)) {
                guard let range = Range(match.range, in: line.string) else { continue }
                let word = String(line.string[range]).lowercased()
                do {
                    guard let box = try line.boundingBox(for: range)?.boundingBox else {
                        diagnostics += "  \(word): no word geometry\n"
                        continue
                    }
                    diagnostics += "  \(word): \(box), range=\(match.range)\n"
                    recognized.append(Word(text: word, box: box, observation: index, range: range))
                } catch {
                    diagnostics += "  \(word): word geometry failed: \(error)\n"
                }
            }
        }
        attachDiagnostics()
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
