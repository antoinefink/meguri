import Foundation
import XCTest

@MainActor
final class BackupsAccessibilityTests: XCTestCase {
    func testSettingsAndRestoreStatesPassAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("VM development build required") }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        var results: [AccessibilityAudit.StateResult] = []
        for surface in ["settings", "restore"] {
            for state in ["empty", "typical", "overflow", "error", "loading"] {
                let route = surface == "settings" ? "settings/backups/\(state)" : "settings/backups/restore/\(state)"
                let anchor = surface == "settings" ? "settings.backups.content" : "settings.backups.cancel"
                let result = try AccessibilityAudit.runAllVariants(
                    state: "\(surface).\(state)", launch: MeguriLaunch(locale: "en_US", animations: false, route: route),
                    waitFor: anchor, suppressions: suppressions,
                    supplemental: { app, variant in
                        var findings: [AuditFinding] = []
                        var targets: [TouchTargetViolation] = []
                        @MainActor func audit(_ detail: String) throws {
                            findings += try AccessibilityAudit.run(app, variant: "\(surface).\(state) \(detail), \(variant)",
                                                                  suppressions: suppressions)
                            targets += try TouchTargets.violations(in: app)
                        }
                        if surface == "settings" {
                            let create = app.buttons["settings.backups.create"].firstMatch
                            XCTAssertEqual(create.label, "Back Up Now")
                            XCTAssertEqual(create.isEnabled, state != "loading" && state != "error")
                            if state == "loading" {
                                XCTAssertEqual(app.progressIndicators["settings.backups.progress"].firstMatch.label, "Creating Backup…")
                                XCTAssertTrue(app.buttons["settings.backups.cancelOperation"].firstMatch.isEnabled)
                            }
                            let controls = ["restore", "folder", "export"]
                            for name in controls {
                                let control = app.buttons["settings.backups.\(name)"].firstMatch
                                if try self.reach(control, in: app) { try audit(name) }
                                let enabled = name == "folder" || (name == "restore" ? state != "loading" : state != "loading" && state != "error")
                                XCTAssertEqual(control.isEnabled, enabled)
                                if name == "folder" {
                                    #if os(macOS)
                                    XCTAssertEqual(control.label, "Show in Finder")
                                    #else
                                    XCTAssertEqual(control.label, "Show in Files")
                                    #endif
                                }
                            }
                            let media = app.staticTexts["settings.backups.mediaExplanation"].firstMatch
                            if try self.reach(media, in: app) { try audit("media explanation") }
                            XCTAssertTrue(media.label.contains("Backups don’t include media files"))
                            if state != "error" {
                                for field in ["interval", "daily", "weekly", "monthly"] {
                                    let input = app.textFields["settings.backups.limit.\(field)"].firstMatch
                                    if try self.reach(input, in: app) { try audit("limit.\(field)") }
                                    XCTAssertFalse(input.label.isEmpty)
                                    XCTAssertEqual(input.isEnabled, state != "loading")
                                    if state == "overflow" && (field == "interval" || field == "daily") {
                                        XCTAssertEqual(input.value as? String, "4294967295")
                                    }
                                }
                            }
                            if state == "typical" || state == "overflow" {
                                let link = app.descendants(matching: .any).matching(identifier: "settings.backups.server").firstMatch
                                if try self.reach(link, in: app) { try audit("server disclosure") }
                                XCTAssertEqual(link.label, "Server Backups in Browser…")
                                let disclosure = app.staticTexts["settings.backups.serverExplanation"].firstMatch
                                XCTAssertTrue(disclosure.exists)
                                XCTAssertTrue(disclosure.label.contains("Scheduled server backups include media"))
                                XCTAssertTrue(disclosure.label.contains("restoring there replaces the server collection"))
                            }
                        } else {
                            let apply = app.buttons["settings.backups.apply"].firstMatch
                            XCTAssertEqual(apply.label, "Restore…")
                            XCTAssertEqual(apply.isEnabled, state == "typical" || state == "overflow")
                            XCTAssertEqual(app.buttons["settings.backups.cancel"].firstMatch.label, "Cancel")
                            XCTAssertTrue(app.buttons["settings.backups.cancel"].firstMatch.isEnabled)
                            if state == "empty" || state == "error" {
                                let retry = app.buttons["settings.backups.retry"].firstMatch
                                XCTAssertEqual(retry.label, "Try Again")
                                XCTAssertTrue(retry.isEnabled)
                                if state == "error" {
                                    XCTAssertTrue(app.staticTexts["The backup could not be read or written. Check file access and free space, then try again."].exists)
                                }
                            } else if state == "loading" {
                                let progress = app.progressIndicators["settings.backups.loading"].firstMatch
                                XCTAssertTrue(progress.exists)
                                XCTAssertEqual(progress.label, "Loading Backups…")
                            } else {
                                let names = ["backup-2026-11-25-17.25.00.colpkg", "server-copy-2026-11-24-09.10.00.colpkg",
                                             "server-copy-2026-11-23-09.10.00.unverified.anki2", "backup-2026-11-22-08.00.00.colpkg"]
                                for (index, name) in names.enumerated() {
                                    let row = app.descendants(matching: .any).matching(identifier: "settings.backups.entry.\(name)").firstMatch
                                    if try self.reach(row, in: app) { try audit("entry.\(index)") }
                                    XCTAssertTrue(row.label.contains(name), "The exact filename is read with the row")
                                    let sizes: [Int64] = [28_400_000, 27_900_000, 27_800_000, 27_600_000]
                                    XCTAssertTrue(row.label.contains(ByteCountFormatter.string(fromByteCount: sizes[index], countStyle: .file)))
                                    XCTAssertEqual(row.isSelected, index == (state == "overflow" ? 1 : 0))
                                    if index == 1 { XCTAssertTrue(row.label.contains("Server copy")) }
                                    if index == 2 || (index == 3 && state == "overflow") {
                                        XCTAssertTrue(row.label.contains("Can’t be restored"))
                                    }
                                }
                                // The real native confirmation discloses the selected older copy's
                                // date and the provider's publication/media decision. No restore runs.
                                apply.tap()
                                XCTAssertTrue(app.buttons["Restore"].firstMatch.waitForExistence(timeout: 5))
                                let date = state == "overflow" ? "Nov 24, 2026" : "Nov 25, 2026"
                                XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", date)).firstMatch.exists)
                                let message = state == "overflow"
                                    ? "Restoring does not upload this collection to Meguri Sync."
                                    : "Media files are kept."
                                XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", message)).firstMatch.exists)
                                try audit("restore confirmation")
                            }
                        }
                        return .init(state: "\(surface).\(state)", findings: findings, touchTargets: targets)
                    })
                results.append(result)
            }
        }
        add(AccessibilityAudit.screenReport("settings.backups", states: results))
        for result in results {
            AccessibilityAudit.assertClean(result.findings)
            TouchTargets.assertMet(result.touchTargets)
        }
    }

    /// Lazy native Forms and Lists must bring every tested row into the viewport,
    /// including at AX5. A missing or unreachable row fails, never skips its audit.
    /// Returns whether scrolling changed the viewport and requires another audit.
    private func reach(_ element: XCUIElement, in app: XCUIApplication) throws -> Bool {
        let list = app.collectionViews.firstMatch
        let scroll = list.exists ? list : app.scrollViews.firstMatch
        var scrolled = false
        for _ in 0..<24 {
            let viewport = scroll.frame.intersection(app.windows.firstMatch.frame)
            let frame = element.exists ? element.frame : .null
            if !frame.isEmpty && viewport.contains(frame) && element.isHittable { return scrolled }
            let reach = viewport.height / 3
            let move = frame.isNull ? -reach : max(-reach, min(reach, viewport.midY - frame.midY))
            #if os(macOS)
            scroll.scroll(byDeltaX: 0, deltaY: move)
            #else
            let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 0, dy: move)),
                        withVelocity: .slow, thenHoldForDuration: 0.2)
            #endif
            scrolled = true
        }
        XCTFail("Unreachable backup content: \(element)\n\(app.debugDescription)")
        return scrolled
    }
}
