#if os(macOS)
import AppKit
import Foundation
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
        XCTAssertEqual(thumbs.count, 1, "Drag the native scrollbar's own thumb")
        let thumb = try XCTUnwrap(thumbs.first)
        func attachHistory(_ state: String) {
            self.attachHistory(window, viewport: viewport, firstCell: firstCell, oldest: oldest,
                oldestCell: oldestCell, scrollbar: scrollbar, thumb: thumb, state: state)
        }
        attachHistory("compact-initial-history")
        XCTAssertTrue(firstCell.exists && !firstCell.frame.isEmpty && initialViewport.contains(firstCell.frame),
            "The initial archive data cell must fit completely within the viewport")
        assertFooterSeparation(window, viewport: viewport, cell: firstCell)
        let trackFrame = scrollbar.frame
        let thumbFrame = thumb.frame
        let windowFrame = window.frame
        XCTAssertTrue(!trackFrame.isEmpty
            && !thumbFrame.isEmpty && trackFrame.contains(thumbFrame) && initialViewport.contains(thumbFrame),
            "The same native vertical track must contain its visible thumb")
        let startPoint = CGPoint(x: thumbFrame.midX, y: thumbFrame.midY)
        let endPoint = CGPoint(x: trackFrame.midX, y: trackFrame.maxY - thumbFrame.height / 2)
        XCTAssertTrue(initialViewport.contains(startPoint) && trackFrame.contains(startPoint)
            && initialViewport.contains(endPoint) && trackFrame.contains(endPoint),
            "Both native drag points must lie within the viewport and the actual scrollbar track")
        // ValueIndicator and ScrollView are passive, disabled AX objects. Anchor
        // input to the key restore window using their actual screen-space frames.
        let origin = window.coordinate(withNormalizedOffset: .zero)
        let start = origin.withOffset(CGVector(dx: startPoint.x - windowFrame.minX,
            dy: startPoint.y - windowFrame.minY))
        let end = origin.withOffset(CGVector(dx: endPoint.x - windowFrame.minX,
            dy: endPoint.y - windowFrame.minY))
        try app.bringToFront(bundleIdentifier: bundle)
        // One native mouse drag, using only this thumb and its vertical track.
        // Align the thumb's bottom with the track's bottom; AppKit clamps it.
        start.click(forDuration: 0.1, thenDragTo: end)
        let reachedOldest = wait {
            let visible = viewport.frame.intersection(window.frame)
            guard oldest.exists && oldestCell.exists else { return false }
            let archiveFrame = oldest.frame
            let cellFrame = oldestCell.frame
            return !visible.isEmpty && !archiveFrame.isEmpty && !cellFrame.isEmpty
                && visible.contains(archiveFrame) && visible.contains(cellFrame)
                && oldest.isHittable
        }
        // continueAfterFailure is false: save the actual outcome before asserting.
        attachHistory("after-native-scroll")
        XCTAssertTrue(reachedOldest, "The exact oldest archive and its complete data cell must become visible and actionable")
        assertFooterSeparation(window, viewport: viewport, cell: oldestCell)
        try app.bringToFront(bundleIdentifier: bundle)
        oldest.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
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
        let destructiveRestore = app.buttons.matching(restorePredicate).firstMatch
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
                    && $0.buttons.matching(restorePredicate).firstMatch.exists
                    && $0.buttons.matching(cancelPredicate).firstMatch.exists
                    && !self.element($0, anchor).exists
            }
            return prompt != nil
        }, "The native confirmation must contain its exact title, destructive Restore and Cancel. \(confirmationContext)")
        let confirmation = try XCTUnwrap(prompt)
        let cancel = confirmation.buttons.matching(cancelPredicate).firstMatch
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

    private func assertFooterSeparation(_ window: XCUIElement, viewport: XCUIElement, cell: XCUIElement) {
        let windowFrame = window.frame
        let viewportFrame = viewport.frame
        let cellFrame = cell.frame
        let visible = viewportFrame.intersection(windowFrame)
        XCTAssertFalse(visible.isEmpty, "The live history viewport must intersect its restore window")
        XCTAssertTrue(visible.contains(cellFrame), "The complete archive data cell must fit within the live viewport")
        for id in ["settings.backups.cancel", "settings.backups.apply"] {
            let button = element(window, id)
            let buttonFrame = button.frame
            XCTAssertTrue(button.exists && !buttonFrame.isEmpty && button.isEnabled && button.isHittable)
            XCTAssertTrue(windowFrame.contains(buttonFrame), "The native action must remain inside its window")
            XCTAssertLessThanOrEqual(viewportFrame.maxY, buttonFrame.minY, "History must end above \(id)")
            XCTAssertLessThanOrEqual(cellFrame.maxY, buttonFrame.minY, "The complete archive must end above \(id)")
            XCTAssertFalse(viewportFrame.intersects(buttonFrame) || cellFrame.intersects(buttonFrame),
                "Neither the history viewport nor its complete data cell may overlap \(id)")
        }
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
