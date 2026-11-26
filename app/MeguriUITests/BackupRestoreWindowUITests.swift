#if os(macOS)
import AppKit
import CoreML
import Foundation
import Vision
import XCTest

/// R3–R4: a compact production Window with the existing 26-entry inert preset.
/// Separate from BackupRecoveryJourneyTests; no collection or restore is needed.
@MainActor
final class BackupRestoreWindowUITests: XCTestCase {
    func testCompactWindowScrollsToOldestArchiveAndCancelsItsConfirmation() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.configuration == .debug, bundle == "com.antoinefink.meguri.dev" else {
            throw XCTSkip("Requires Debug Meguri Dev in the test VM")
        }
        let app = XCUIApplication(bundleIdentifier: bundle)
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
            route: "settings/backups/restore/overflow").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }

        let anchor = "settings.backups.cancel"
        // The .screen route also owns a Launcher main window. Keep the scene
        // binding even when modal AX hides its descendants or reorders windows.
        let window = app.windows["settings.backups.restoreWindow"]
        let appeared = window.waitForExistence(timeout: 10)
        do {
            if appeared && !window.frame.isEmpty {
                try app.bringToFront(bundleIdentifier: bundle)
                // Activate this window through its titlebar, without selecting a row.
                window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
                    .withOffset(CGVector(dx: 0, dy: 12)).click()
            }
        } catch {
            attachReadiness(window, app: app, focus: "Activation failed: \(error)")
            throw error
        }
        // Attach before any readiness assertion, including the missing-window case.
        attachReadiness(window, app: app, focus: appeared ? "Titlebar activation requested" : "Window did not appear")
        XCTAssertTrue(appeared, "The production restore scene must exist")
        let windowCancel = element(window, anchor)
        XCTAssertTrue(wait {
            window.title == "Restore from Backup" && !window.frame.isEmpty
                && windowCancel.exists && windowCancel.isEnabled && windowCancel.isHittable
        }, "The bound restore window must have its exact title and native Cancel affordance")
        let content = element(window, "settings.backups.restoreContent")
        let first = entry(window, "backup-2026-11-25-17.25.00.colpkg")
        let filename = "backup-2026-11-01-08.00.00.colpkg"
        let oldest = entry(window, filename)
        let apply = element(window, "settings.backups.apply")
        XCTAssertTrue(wait {
            first.exists && !first.frame.isEmpty
                && apply.exists && apply.isEnabled && apply.isHittable
        }, "The initial archive and Restore action must be ready")
        // Resolve the expensive history queries once, after native readiness.
        // Prefer the enclosing scroll viewport; SwiftUI may expose an outline
        // instead of a Table, or merge restoreContent into it.
        var history: XCUIElement?
        for type in [XCUIElement.ElementType.scrollView, .table, .outline] {
            history = window.descendants(matching: type).allElementsBoundByIndex.first {
                self.entry($0, "backup-2026-11-25-17.25.00.colpkg").exists
                    && !$0.frame.isEmpty && $0.frame.intersection(window.frame).contains(first.frame)
            }
            if history != nil { break }
        }
        let viewport = try XCTUnwrap(history, "The bound window must contain the initial history viewport")
        XCTAssertFalse(app.sheets.firstMatch.exists, "The restore window must have no attached sheet")
        // Contract: 240-point initial content. Allow native layout/titlebar slack,
        // while rejecting a window whose initial height grows with all 26 rows.
        XCTAssertTrue(wait {
            let contentFrame = content.exists ? content.frame : viewport.frame
            return !contentFrame.isEmpty && contentFrame.height <= 320 && window.frame.height <= 400
        },
            "Long history must retain a modest initial content and window height")
        XCTAssertTrue(wait {
            !oldest.exists || oldest.frame.isEmpty
                || !viewport.frame.intersection(window.frame).intersects(oldest.frame)
        }, "The final archive must initially be outside the visible history viewport")

        let initialViewport = viewport.frame.intersection(window.frame)
        let firstCell = viewport.descendants(matching: .cell)
            .containing(.staticText, identifier: "settings.backups.entry.backup-2026-11-25-17.25.00.colpkg").firstMatch
        let oldestCell = viewport.descendants(matching: .cell)
            .containing(.staticText, identifier: "settings.backups.entry." + filename).firstMatch
        let scrollbars = viewport.descendants(matching: .scrollBar).allElementsBoundByIndex.filter {
            $0.frame.height > $0.frame.width && $0.frame.intersects(initialViewport)
        }
        XCTAssertEqual(scrollbars.count, 1, "The same history viewport must own one native vertical scrollbar")
        let scrollbar = try XCTUnwrap(scrollbars.first)
        let thumbs = scrollbar.descendants(matching: .valueIndicator).allElementsBoundByIndex
        XCTAssertEqual(thumbs.count, 1, "The native scrollbar must expose its own thumb")
        let thumb = try XCTUnwrap(thumbs.first)
        func attachHistory(_ state: String) {
            self.attachHistory(window, viewport: viewport, firstCell: firstCell, oldest: oldest,
                oldestCell: oldestCell, scrollbar: scrollbar, thumb: thumb, state: state)
        }
        attachHistory("compact-initial-history")
        XCTAssertTrue(firstCell.exists && !firstCell.frame.isEmpty && initialViewport.contains(firstCell.frame),
            "The initial archive data cell must fit completely within the viewport")
        assertFooterSeparation(window.frame, viewportFrame: viewport.frame, cellFrame: firstCell.frame,
            cancelFrame: windowCancel.frame, restoreFrame: apply.frame)
        let trackFrame = scrollbar.frame
        let thumbFrame = thumb.frame
        XCTAssertTrue(!trackFrame.isEmpty
            && !thumbFrame.isEmpty && trackFrame.contains(thumbFrame) && initialViewport.contains(thumbFrame),
            "The same native vertical track must contain its visible thumb")
        func normalizedScrollValue() -> Double? {
            let value = scrollbar.value
            if let number = value as? NSNumber { return number.doubleValue }
            if let string = value as? String { return Double(string) }
            return nil
        }
        let initialScrollValue = normalizedScrollValue()
        try app.bringToFront(bundleIdentifier: bundle)
        // Focus the real filename-bound data cell through its visible midpoint
        // in the bound native window, then use the Table's ordinary End key.
        let firstCellFrame = firstCell.frame
        let windowFrame = window.frame
        XCTAssertTrue(viewport.frame.intersection(windowFrame).contains(firstCellFrame)
            && !firstCellFrame.isEmpty)
        window.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: firstCellFrame.midX - windowFrame.minX,
                dy: firstCellFrame.midY - windowFrame.minY)).click()
        let firstSelected = wait { first.value as? String == "Selected" }
        attachHistory("first-archive-focused")
        XCTAssertTrue(firstSelected, "The native Table must select the exact first filename before keyboard scrolling")
        window.typeKey(XCUIKeyboardKey.end.rawValue, modifierFlags: [])
        let reachedEnd = wait { normalizedScrollValue() == 1 }
        // continueAfterFailure is false: save the actual outcome before asserting.
        if !reachedEnd {
            attachHistory("after-native-scroll")
        }
        XCTAssertTrue(reachedEnd, "One native End must reach the scrollbar's exposed normalized value of 1")
        let finalViewport = viewport.frame.intersection(window.frame)
        XCTAssertTrue(scrollbar.exists && thumb.exists && !scrollbar.frame.isEmpty && !thumb.frame.isEmpty
            && scrollbar.frame.contains(thumb.frame) && finalViewport.contains(thumb.frame),
            "The same native vertical scrollbar must retain its own visible thumb after End")
        if let initialScrollValue, let finalScrollValue = normalizedScrollValue() {
            XCTAssertGreaterThan(finalScrollValue, initialScrollValue,
                "The native normalized scrollbar value must show progress after End")
        }
        try app.bringToFront(bundleIdentifier: bundle)
        XCTAssertTrue(windowCancel.exists && windowCancel.isEnabled && windowCancel.isHittable
            && apply.exists && apply.isEnabled && apply.isHittable)
        // Capture native geometry and the window-relative pointer anchor before
        // recognizing pixels. AX queries for offscreen cells can perturb the table.
        let proofWindowFrame = window.frame
        let proofViewportFrame = viewport.frame
        let cancelFrame = windowCancel.frame
        let restoreFrame = apply.frame
        let pointerOrigin = window.coordinate(withNormalizedOffset: .zero)
        let physicalRow = try oldestArchiveBand(window.screenshot(), windowFrame: proofWindowFrame)
        assertFooterSeparation(proofWindowFrame, viewportFrame: proofViewportFrame, cellFrame: physicalRow,
            cancelFrame: cancelFrame, restoreFrame: restoreFrame)
        // No live AX reads or diagnostics between this physical proof and click.
        pointerOrigin.withOffset(CGVector(dx: physicalRow.midX - proofWindowFrame.minX,
            dy: physicalRow.midY - proofWindowFrame.minY)).click()
        // Same filename-bound AX value as the functional journey. Native Table
        // can put isSelected on an ancestor instead of the identified entry.
        XCTAssertTrue(wait {
            oldest.value as? String == "Selected" && apply.isEnabled && apply.isHittable
        }, "Selection must belong to the exact old filename")
        let parser = DateFormatter()
        parser.locale = Locale(identifier: "en_US_POSIX")
        parser.calendar = Calendar(identifier: .gregorian)
        parser.timeZone = .gmt
        parser.dateFormat = "'backup-'yyyy-MM-dd-HH.mm.ss'.colpkg'"
        let date = try XCTUnwrap(parser.date(from: filename))
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US")
        formatter.calendar = parser.calendar
        formatter.timeZone = .gmt
        formatter.setLocalizedDateFormatFromTemplate("yMMMdjmmss")
        let timestamp = formatter.string(from: date)
        XCTAssertTrue(oldest.label.contains(filename) && oldest.label.contains(timestamp))
        capture(window, "oldest-archive-selected")
        let title = "Replace the current collection with the backup from \(timestamp)?"
        let titlePredicate = NSPredicate(format: "label == %@ OR value == %@", title, title)
        let restorePredicate = NSPredicate(format: "(label == %@ OR title == %@) AND identifier != %@",
            "Restore", "Restore", "settings.backups.apply")
        let cancelPredicate = NSPredicate(format: "(label == %@ OR title == %@) AND identifier != %@", "Cancel", "Cancel", anchor)
        let confirmationTitle = app.staticTexts.matching(titlePredicate).firstMatch
        let destructiveRestore = confirmationAction(app, matching: restorePredicate)
        let confirmationContext = "Selected archive: \(filename); expected title: \(title)"
        try app.bringToFront(bundleIdentifier: bundle)
        apply.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()

        let prompts = app.descendants(matching: .any).matching(NSPredicate(format: "elementType IN %@", [
            XCUIElement.ElementType.popover.rawValue, XCUIElement.ElementType.menu.rawValue,
            XCUIElement.ElementType.sheet.rawValue, XCUIElement.ElementType.alert.rawValue,
            XCUIElement.ElementType.dialog.rawValue,
        ]))
        var prompt: XCUIElement?
        XCTAssertTrue(wait {
            prompt = prompts.allElementsBoundByIndex.first {
                $0.staticTexts.matching(titlePredicate).firstMatch.exists
                    && self.confirmationAction($0, matching: restorePredicate).exists
                    && self.confirmationAction($0, matching: cancelPredicate).exists
                    && !self.element($0, anchor).exists
            }
            return prompt != nil
        }, "The native confirmation must contain its exact title, destructive Restore and Cancel. \(confirmationContext)")
        let confirmation = try XCTUnwrap(prompt)
        let cancel = confirmationAction(confirmation, matching: cancelPredicate)
        XCTAssertTrue(wait {
            confirmationTitle.exists && destructiveRestore.exists
                && cancel.exists && cancel.isEnabled && cancel.isHittable
        }, "The confirmation must be ready to cancel. \(confirmationContext)")
        capture(window, "oldest-archive-confirmation")
        try app.bringToFront(bundleIdentifier: bundle)
        cancel.click()
        // Native menus can reuse the prompt's AX index after dismissal.
        XCTAssertTrue(wait { !confirmationTitle.exists && !destructiveRestore.exists },
            "Cancel must dismiss the confirmation. \(confirmationContext); "
                + "title remains=\(confirmationTitle.exists); destructive Restore remains=\(destructiveRestore.exists)")
        try app.bringToFront(bundleIdentifier: bundle)
        window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
            .withOffset(CGVector(dx: 0, dy: 12)).click()
        XCTAssertTrue(wait {
            oldest.exists && oldest.value as? String == "Selected" && apply.isEnabled
                && windowCancel.exists && windowCancel.isEnabled && windowCancel.isHittable
        }, "Cancelling returns to the same selected archive")
        XCTAssertFalse(app.sheets.firstMatch.exists)
        XCTAssertFalse(element(window, "settings.backups.progress").exists, "Cancellation must not start restoration")
        attachHistory("oldest-archive-cancelled")

        // Escape cancels the same dated confirmation without changing selection.
        try app.bringToFront(bundleIdentifier: bundle)
        apply.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
        let reopened = wait { confirmationTitle.exists && destructiveRestore.exists }
        capture(window, "oldest-archive-escape-confirmation")
        XCTAssertTrue(reopened, "The same selected archive must produce the same dated confirmation")
        try app.bringToFront(bundleIdentifier: bundle)
        window.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        let escaped = wait {
            !confirmationTitle.exists && !destructiveRestore.exists && oldest.exists
                && oldest.value as? String == "Selected" && apply.isEnabled && apply.isHittable
                && windowCancel.exists && windowCancel.isEnabled && windowCancel.isHittable
        }
        attachHistory("oldest-archive-escape-cancelled")
        XCTAssertTrue(escaped, "Escape must cancel confirmation and retain the exact selected archive")
        XCTAssertFalse(element(window, "settings.backups.progress").exists, "Escape must not start restoration")
        try app.bringToFront(bundleIdentifier: bundle)
        window.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(window.waitForNonExistence(timeout: 10), "Cmd-W must close the same bound restore window")
        XCTAssertFalse(element(app, "settings.backups.progress").exists, "Closing must not start restoration")
    }

    private func assertFooterSeparation(_ windowFrame: CGRect, viewportFrame: CGRect, cellFrame: CGRect,
                                        cancelFrame: CGRect, restoreFrame: CGRect) {
        let visible = viewportFrame.intersection(windowFrame)
        XCTAssertFalse(visible.isEmpty, "The measured history viewport must intersect its restore window")
        XCTAssertFalse(cellFrame.isEmpty, "The complete archive band must have positive dimensions")
        XCTAssertTrue(visible.contains(cellFrame), "The complete archive band must fit within the measured viewport")
        for (id, buttonFrame) in [("settings.backups.cancel", cancelFrame), ("settings.backups.apply", restoreFrame)] {
            XCTAssertFalse(buttonFrame.isEmpty)
            XCTAssertTrue(windowFrame.contains(buttonFrame), "The native action must remain inside its window")
            XCTAssertLessThanOrEqual(viewportFrame.maxY, buttonFrame.minY, "History must end above \(id)")
            XCTAssertLessThanOrEqual(cellFrame.maxY, buttonFrame.minY, "The complete archive must end above \(id)")
            XCTAssertFalse(viewportFrame.intersects(buttonFrame) || cellFrame.intersects(buttonFrame),
                "Neither the history viewport nor its complete data cell may overlap \(id)")
        }
    }

    private func oldestArchiveBand(_ screenshot: XCUIScreenshot, windowFrame: CGRect) throws -> CGRect {
        let attachment = XCTAttachment(screenshot: screenshot)
        attachment.name = "native-backups-mac-final-physical-proof-synthetic"
        attachment.lifetime = .keepAlways
        add(attachment)
        var diagnostics = "Window screen frame (top-left points): \(windowFrame)\n"
        func attachDiagnostics() {
            let text = XCTAttachment(string: diagnostics)
            text.name = "native-backups-mac-final-physical-ocr-synthetic"
            text.lifetime = .keepAlways
            add(text)
        }
        guard let image = NSBitmapImageRep(data: screenshot.pngRepresentation)?.cgImage else {
            diagnostics += "Could not decode the bound window screenshot.\n"
            attachDiagnostics()
            return try XCTUnwrap(nil as CGRect?, "The bound window screenshot must provide a CGImage")
        }
        let scaleX = CGFloat(image.width) / windowFrame.width
        let scaleY = CGFloat(image.height) / windowFrame.height
        // Physically isolate the date column so Vision cannot merge in Size.
        // Keep FULL height, including the footer, to detect encroaching date text.
        let crop = CGRect(x: 0, y: 0, width: (CGFloat(image.width) / 2).rounded(.down),
            height: CGFloat(image.height))
        diagnostics += "Image=\(image.width)x\(image.height); pixels/point=\(scaleX),\(scaleY)\n"
            + "Date-column crop (top-left pixels)=\(crop)\n"
        guard let cropped = image.cropping(to: crop) else {
            diagnostics += "Could not crop the full-height date column.\n"
            attachDiagnostics()
            return try XCTUnwrap(nil as CGRect?, "The bound window date column must be available")
        }
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .fast
        request.recognitionLanguages = ["en-US"]
        request.usesLanguageCorrection = false
        request.minimumTextHeight = 0.01
        do {
            let devices = try request.supportedComputeStageDevices
            let cpu = try XCTUnwrap(devices[.main]?.first { device in
                if case .cpu = device { return true }
                return false
            }, "Native window OCR requires a supported CPU device for the main compute stage")
            request.setComputeDevice(cpu, for: .main)
            try VNImageRequestHandler(cgImage: cropped, options: [:]).perform([request])
        } catch {
            diagnostics += "Recognition failed: \(error)\n"
            attachDiagnostics()
            throw error
        }
        var dates: [(normalized: String, frame: CGRect)] = []
        for observation in request.results ?? [] {
            guard let text = observation.topCandidates(1).first?.string else { continue }
            let normalized = text.lowercased().components(separatedBy: CharacterSet.alphanumerics.inverted).joined()
            let box = observation.boundingBox
            // Vision is normalized bottom-left in the cropped image. CGImage
            // pixels and XCTest screen/window points have top-left origins.
            let pixels = CGRect(x: crop.minX + box.minX * CGFloat(cropped.width),
                y: crop.minY + (1 - box.maxY) * CGFloat(cropped.height),
                width: box.width * CGFloat(cropped.width), height: box.height * CGFloat(cropped.height))
            let frame = CGRect(x: windowFrame.minX + pixels.minX / scaleX,
                y: windowFrame.minY + pixels.minY / scaleY,
                width: pixels.width / scaleX, height: pixels.height / scaleY)
            diagnostics += "\(text.debugDescription) -> \(normalized); Vision=\(box); pixels=\(pixels); screen=\(frame)\n"
            dates.append((normalized, frame))
        }
        // Literal dates in this en_US synthetic fixture; no fuzzy OCR matching.
        let oldest = dates.filter { $0.normalized == "nov12026at80000am" }
        let previous = dates.filter { $0.normalized == "nov22026at80000am" }
        var band: CGRect?
        if let last = oldest.first?.frame, let preceding = previous.first?.frame {
            let pitch = last.midY - preceding.midY
            band = CGRect(x: last.minX, y: last.midY - pitch / 2, width: last.width, height: pitch)
            diagnostics += "Nov 1 matches=\(oldest.count); Nov 2 matches=\(previous.count); pitch=\(pitch)pt; band=\(String(describing: band))\n"
        }
        attachDiagnostics()
        XCTAssertEqual(oldest.count, 1, "Exactly one complete Nov 1 timestamp must be rendered; see final OCR diagnostics")
        XCTAssertEqual(previous.count, 1, "Exactly one complete Nov 2 timestamp must be rendered; see final OCR diagnostics")
        let last = try XCTUnwrap(oldest.first?.frame)
        let preceding = try XCTUnwrap(previous.first?.frame)
        let row = try XCTUnwrap(band)
        let pitch = last.midY - preceding.midY
        XCTAssertTrue((16...40).contains(pitch), "Adjacent one-line dates must have a positive, reasonable native row pitch")
        XCTAssertLessThan(preceding.maxY, last.minY, "Nov 2 must appear fully above Nov 1 in newest-first order")
        XCTAssertEqual(last.minX, preceding.minX, accuracy: 2, "The dates must align in the same column")
        XCTAssertEqual(last.height, preceding.height, accuracy: 2, "The dates must use the same one-line geometry")
        let previousBand = CGRect(x: preceding.minX, y: preceding.midY - pitch / 2,
            width: preceding.width, height: pitch)
        XCTAssertTrue(row.contains(last) && previousBand.contains(preceding),
            "Both complete date glyphs must fit their inferred one-line row bands")
        for date in [last, preceding] {
            XCTAssertTrue(date.minX > windowFrame.minX && date.maxX < windowFrame.minX + crop.width / scaleX,
                "The physical crop must contain each complete date without clipping its glyphs")
        }
        return row
    }

    private func attachHistory(_ window: XCUIElement, viewport: XCUIElement, firstCell: XCUIElement,
                               oldest: XCUIElement, oldestCell: XCUIElement, scrollbar: XCUIElement,
                               thumb: XCUIElement, state: String) {
        // Full native-window pixels and hierarchy retain synthetic drawing evidence.
        capture(window, state)
        func describe(_ name: String, _ control: XCUIElement) -> String {
            guard control.exists else { return "\(name): exists=false\n" }
            return "\(name): frame=\(control.frame); hittable=\(control.isHittable); "
                + "enabled=\(control.isEnabled); value=\(String(describing: control.value))\n"
        }
        let details = "Binding: app.windows[settings.backups.restoreWindow]; state=\(state)\n"
            + describe("window", window) + describe("viewport", viewport)
            + "Visible viewport/window intersection: \(viewport.frame.intersection(window.frame))\n"
            + describe("vertical scrollbar", scrollbar) + describe("thumb", thumb)
            + describe("first data cell", firstCell) + describe("oldest", oldest)
            + describe("oldest data cell", oldestCell)
            + describe("Cancel", element(window, "settings.backups.cancel"))
            + describe("Restore", element(window, "settings.backups.apply")) + window.debugDescription
        let attachment = XCTAttachment(string: details)
        attachment.name = "native-backups-mac-\(state)-geometry-synthetic"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func attachReadiness(_ window: XCUIElement, app: XCUIApplication, focus: String) {
        let frontmost = NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "none"
        var details = "Binding: app.windows[settings.backups.restoreWindow]\n"
            + "Focus: \(focus); frontmost=\(frontmost); appState=\(app.state.rawValue)\n"
            + "Window exists: \(window.exists)\n"
        if window.exists {
            details += "Identifier: \(window.identifier); title: \(window.title); frame: \(window.frame)\n"
            for id in ["settings.backups.cancel", "settings.backups.apply", "settings.backups.restoreContent"] {
                let control = element(window, id)
                details += "\(id): exists=\(control.exists)"
                if control.exists { details += "; frame=\(control.frame); enabled=\(control.isEnabled)" }
                details += "\n"
            }
            // Only this public synthetic restore window, never the app or desktop.
            details += window.debugDescription
        }
        let attachment = XCTAttachment(string: details)
        attachment.name = "native-backups-mac-readiness-synthetic"
        attachment.lifetime = .keepAlways
        add(attachment)
        if window.exists && !window.frame.isEmpty { capture(window, "readiness") }
    }

    private func confirmationAction(_ context: XCUIElement, matching caption: NSPredicate) -> XCUIElement {
        context.descendants(matching: .any).matching(NSPredicate(format: "elementType IN %@", [
            XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.menuItem.rawValue,
        ])).matching(caption).firstMatch
    }

    private func element(_ context: XCUIElement, _ identifier: String) -> XCUIElement {
        context.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func entry(_ window: XCUIElement, _ filename: String) -> XCUIElement {
        element(window, "settings.backups.entry." + filename)
    }

    private func wait(_ condition: @escaping () -> Bool) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)],
            timeout: 10) == .completed
    }

    private func capture(_ window: XCUIElement, _ state: String) {
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = "native-backups-mac-\(state)-synthetic"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
