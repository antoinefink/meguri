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
        capture(window, "compact-initial-history")

        try app.bringToFront(bundleIdentifier: bundle)
        // Scroll the native table to its end, without resizing the window or
        // relying on the AX tree to materialize offscreen rows synchronously.
        viewport.scroll(byDeltaX: 0, deltaY: -viewport.frame.height * 26)
        XCTAssertTrue(wait {
            oldest.exists && !oldest.frame.isEmpty && oldest.isHittable
                && viewport.frame.intersection(window.frame).contains(oldest.frame)
        }, "The exact oldest archive must become visible and actionable")
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
        try app.bringToFront(bundleIdentifier: bundle)
        apply.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()

        let title = "Replace the current collection with the backup from \(timestamp)?"
        let prompts = app.descendants(matching: .any).matching(NSPredicate(format: "elementType IN %@", [
            XCUIElement.ElementType.popover.rawValue, XCUIElement.ElementType.menu.rawValue,
            XCUIElement.ElementType.sheet.rawValue, XCUIElement.ElementType.alert.rawValue,
            XCUIElement.ElementType.dialog.rawValue,
        ]))
        var prompt: XCUIElement?
        XCTAssertTrue(wait {
            prompt = prompts.allElementsBoundByIndex.first {
                $0.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", title, title)).firstMatch.exists
                    && $0.buttons["Restore"].exists
                    && !self.element($0, anchor).exists
            }
            return prompt != nil
        }, "The native confirmation must name the selected November 1 date and time")
        let confirmation = try XCTUnwrap(prompt)
        let cancel = confirmation.buttons["Cancel"]
        XCTAssertTrue(wait { cancel.exists && cancel.isEnabled && cancel.isHittable })
        capture(window, "oldest-archive-confirmation")
        try app.bringToFront(bundleIdentifier: bundle)
        cancel.click()
        XCTAssertTrue(confirmation.waitForNonExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundle)
        window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
            .withOffset(CGVector(dx: 0, dy: 12)).click()
        XCTAssertTrue(wait {
            oldest.exists && oldest.value as? String == "Selected" && apply.isEnabled
                && windowCancel.exists && windowCancel.isEnabled && windowCancel.isHittable
        }, "Cancelling returns to the same selected archive")
        XCTAssertFalse(app.sheets.firstMatch.exists)
        XCTAssertFalse(element(window, "settings.backups.progress").exists, "Cancellation must not start restoration")
        window.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(window.waitForNonExistence(timeout: 10), "Cmd-W must close the same bound restore window")
        XCTAssertFalse(element(app, "settings.backups.progress").exists, "Closing must not start restoration")
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
