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
        let window = app.windows.containing(.collectionView, identifier: "settings.backups.content").firstMatch
        XCTAssertTrue(wait(timeout: 10) { app.state == .runningForeground && window.exists })
        let form = window.collectionViews["settings.backups.content"].firstMatch
        let field = window.textFields["settings.backups.limit.interval"].firstMatch
        reveal(field, in: app)
        XCTAssertTrue(wait(timeout: 10) {
            app.state == .runningForeground && field.isEnabled && field.isHittable
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

        // Measure actual native controls/cells before opening the keyboard. The
        // initial iPad capture above needs no scroll; smaller phones scroll only
        // when a target is outside the native viewport.
        var heights: [String: CGFloat] = [:]
        for name in ["interval", "daily", "weekly", "monthly"] {
            let id = "settings.backups.limit.\(name)"
            let input = form.textFields[id].firstMatch
            reveal(input, in: app)
            XCTAssertEqual(form.textFields.matching(identifier: id).count, 1, "One real editable field for \(name)")
            assertTarget(input)
            let cells = form.cells.containing(.textField, identifier: id)
            XCTAssertEqual(cells.count, 1, "One native enclosing cell for \(name)")
            let row = cells.firstMatch.frame
            XCTAssertTrue(row.contains(input.frame), "The input must stay inside its own row: \(name)")
            heights[name] = row.height
            XCTAssertEqual(row.height, 52, accuracy: 1, "Preserve the accepted native row height: \(name)")
        }
        let intervalHeight = try XCTUnwrap(heights["interval"])
        for name in ["daily", "weekly", "monthly"] {
            XCTAssertEqual(try XCTUnwrap(heights[name]), intervalHeight, accuracy: 1,
                           "All four rows, including Interval, retain native height parity")
        }

        // Reset the same inert fixture once after measuring Monthly, then move
        // forward in form order. Missing virtualized fields are known to be later.
        app.terminate()
        app.launch()
        XCTAssertTrue(app.buttons["settings.backups.create"].firstMatch.waitForExistence(timeout: 10))
        // Coordinates near the right edge put the caret after the existing
        // digits. app.typeText cannot implicitly focus a queried field. Each
        // assertion proves its visible live draft before the next focus change;
        // production persistence is covered by the real collection journeys.
        try editAtEdge("interval", old: "30", new: "31", top: true, in: app)
        try editAtEdge("daily", old: "12", new: "11", top: false, in: app)
        try editAtEdge("weekly", old: "10", new: "13", top: true, in: app)
        try editAtEdge("monthly", old: "9", new: "8", top: false, in: app)
    }

    func testRestoreRetryNativeTargetAndEdgeActions() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        let app = XCUIApplication(bundleIdentifier: try build.bundleIdentifier)
        addTeardownBlock { @MainActor in app.terminate() }
        // Each error-fixture launch is pure synthetic state. Retry changes only
        // that fixture to empty, giving both edge taps an observable action.
        for top in [true, false] {
            app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
                route: "settings/backups/restore/error",
                extra: ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryL"]).arguments
            app.launch()
            let retry = app.buttons["settings.backups.retry"].firstMatch
            XCTAssertTrue(wait(timeout: 10) { retry.exists && retry.isHittable })
            XCTAssertEqual(app.buttons.matching(identifier: "settings.backups.retry").count, 1)
            XCTAssertTrue(app.staticTexts["Unable to Load Backups"].exists)
            assertTarget(retry)
            let rect = retry.frame
            let point = CGPoint(x: rect.midX, y: top ? rect.minY + 2 : rect.maxY - 2)
            XCTAssertGreaterThan(abs(point.y - rect.midY), 9, "Tap outside the old 18-point text target")
            tap(point, in: app.windows.firstMatch)
            XCTAssertTrue(app.staticTexts["No Backups Yet"].waitForExistence(timeout: 5), "The real retry action must run")
            XCTAssertFalse(app.staticTexts["Unable to Load Backups"].exists)
            XCTAssertFalse(app.descendants(matching: .any).matching(identifier: "settings.backups.listFailure").firstMatch.exists)
            // The empty state uses the same real native button.
            XCTAssertTrue(wait(timeout: 5) { retry.exists && retry.isHittable })
            XCTAssertEqual(app.buttons.matching(identifier: "settings.backups.retry").count, 1)
            assertTarget(retry)
            app.terminate()
        }
    }

    private func assertTarget(_ element: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(element.isEnabled && element.isHittable, file: file, line: line)
        XCTAssertGreaterThanOrEqual(element.frame.width, 44, file: file, line: line)
        XCTAssertGreaterThanOrEqual(element.frame.height, 44, file: file, line: line)
    }

    private func reveal(_ element: XCUIElement, in app: XCUIApplication) {
        let window = app.windows.containing(.collectionView, identifier: "settings.backups.content").firstMatch
        let form = window.collectionViews["settings.backups.content"].firstMatch
        let bar = window.navigationBars["Backups"].firstMatch
        XCTAssertTrue(wait(timeout: 10) { form.exists && bar.exists })
        func viewport() -> CGRect {
            var rect = form.frame.intersection(window.frame)
            let top = max(rect.minY, bar.frame.maxY)
            rect = CGRect(x: rect.minX, y: top, width: rect.width, height: max(0, rect.maxY - top))
            let keyboard = app.keyboards.firstMatch
            if keyboard.exists && rect.intersects(keyboard.frame) {
                rect.size.height = max(0, keyboard.frame.minY - rect.minY)
            }
            return rect
        }
        func visible() -> Bool {
            element.exists && !element.frame.isEmpty && viewport().contains(element.frame) && element.isHittable
        }
        for _ in 0..<8 {
            if visible() { return }
            let bounds = viewport()
            guard !bounds.isNull, bounds.width > 0, bounds.height > 0 else { break }
            let frame = element.exists ? element.frame : .null
            if !frame.isEmpty && bounds.contains(frame) { break }
            // Use actual geometry for either direction; a missing lazy row is
            // later in our chronological traversal from a clean fixture launch.
            let earlier = !frame.isEmpty && frame.minY < bounds.minY
            let upper = CGPoint(x: bounds.midX, y: bounds.minY + bounds.height * 0.2)
            let lower = CGPoint(x: bounds.midX, y: bounds.minY + bounds.height * 0.8)
            let origin = form.coordinate(withNormalizedOffset: .zero)
            func coordinate(_ point: CGPoint) -> XCUICoordinate {
                origin.withOffset(CGVector(dx: point.x - form.frame.minX, dy: point.y - form.frame.minY))
            }
            // Both endpoints are inside the named detail form, below its native
            // navigation bar and above an overlapping software keyboard.
            coordinate(earlier ? upper : lower).press(forDuration: 0.05,
                thenDragTo: coordinate(earlier ? lower : upper), withVelocity: .slow, thenHoldForDuration: 0.2)
            if wait(timeout: 1, visible) { return }
        }
        XCTAssertTrue(wait(timeout: 5, visible), "Native scroll must reveal \(element.identifier)")
    }

    private func editAtEdge(_ name: String, old: String, new: String, top: Bool, in app: XCUIApplication) throws {
        let window = app.windows.containing(.collectionView, identifier: "settings.backups.content").firstMatch
        let input = window.collectionViews["settings.backups.content"].textFields["settings.backups.limit.\(name)"].firstMatch
        reveal(input, in: app)
        XCTAssertEqual(try XCTUnwrap(input.value as? String), old)
        let rect = input.frame
        let point = CGPoint(x: rect.maxX - 2, y: top ? rect.minY + 2 : rect.maxY - 2)
        XCTAssertGreaterThan(abs(point.y - rect.midY), 11, "Tap outside the old 22-point text target")
        tap(point, in: window)
        app.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: old.count) + new)
        XCTAssertTrue(wait(timeout: 5) { input.value as? String == new }, "The edge tap must focus and edit \(name) exactly")
    }

    private func tap(_ point: CGPoint, in window: XCUIElement) {
        window.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: point.x - window.frame.minX, dy: point.y - window.frame.minY)).tap()
    }

    private func wait(timeout: TimeInterval, _ condition: @escaping () -> Bool) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)],
            timeout: timeout) == .completed
    }
}
#endif
