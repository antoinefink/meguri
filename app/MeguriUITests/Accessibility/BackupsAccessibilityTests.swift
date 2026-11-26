import Foundation
import XCTest

@MainActor
final class BackupsAccessibilityTests: XCTestCase {
    private var bundle = ""

    func testSettingsAndRestoreStatesPassAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        bundle = try build.bundleIdentifier
        #if os(macOS)
        guard bundle.hasSuffix(".dev") else { throw XCTSkip("VM development build required") }
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
                                self.assertProgress("settings.backups.progress", label: "Creating Backup…", in: app)
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
                            self.assertStaticText(media, contains: "Device backups contain only the collection")
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
                                XCTAssertTrue(disclosure.exists, "Expected server backup explanation: \(disclosure.debugDescription)")
                                self.assertStaticText(disclosure, contains: "Scheduled server backups include media")
                                self.assertStaticText(disclosure, contains: "Restoring there replaces the server collection")
                            }
                        } else {
                            #if os(macOS)
                            // The native route also opens a Launcher main window.
                            let window = app.windows["settings.backups.restoreWindow"]
                            XCTAssertTrue(window.waitForExistence(timeout: 10))
                            try app.bringToFront(bundleIdentifier: self.bundle)
                            if !window.frame.isEmpty {
                                window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
                                    .withOffset(CGVector(dx: 0, dy: 12)).click()
                            }
                            let cancel = window.buttons[anchor].firstMatch
                            XCTAssertTrue(self.wait {
                                window.title == "Restore from Backup" && !window.frame.isEmpty
                                    && cancel.exists && cancel.isEnabled && cancel.isHittable
                            }, "The bound restore window must be ready")
                            XCTAssertEqual(window.title, "Restore from Backup")
                            XCTAssertEqual(window.descendants(matching: .any).matching(identifier: anchor).count, 1)
                            XCTAssertFalse(window.sheets.firstMatch.exists)
                            let restoreScope: XCUIElement = window
                            #else
                            let restoreScope: XCUIElement = app
                            #endif
                            let apply = app.buttons["settings.backups.apply"].firstMatch
                            XCTAssertEqual(apply.label, "Restore…")
                            XCTAssertEqual(apply.isEnabled, state == "typical" || state == "overflow")
                            XCTAssertEqual(app.buttons["settings.backups.cancel"].firstMatch.label, "Cancel")
                            XCTAssertTrue(app.buttons["settings.backups.cancel"].firstMatch.isEnabled)
                            if state == "empty" || state == "error" {
                                let emptyTitle = self.staticText("No Backups Yet", in: restoreScope)
                                let emptyExplanation = self.staticText("Create a backup on this device, or check the configured backup folder.", in: restoreScope)
                                let errorTitle = self.staticText("Unable to Load Backups", in: restoreScope)
                                let errorExplanation = self.staticText("The backup could not be read or written. Check file access and free space, then try again.", in: restoreScope)
                                XCTAssertEqual(emptyTitle.exists, state == "empty", "Expected empty heading only in empty state: \(restoreScope.debugDescription)")
                                XCTAssertEqual(emptyExplanation.exists, state == "empty", "Expected empty explanation only in empty state: \(restoreScope.debugDescription)")
                                XCTAssertEqual(errorTitle.exists, state == "error", "Expected failure heading only in error state: \(restoreScope.debugDescription)")
                                XCTAssertEqual(errorExplanation.exists, state == "error", "Expected failure explanation only in error state: \(restoreScope.debugDescription)")
                                if state == "error" {
                                    if try self.reach(errorTitle, in: app) { try audit("list failure heading") }
                                    if try self.reach(errorExplanation, in: app) { try audit("list failure explanation") }
                                    XCTAssertFalse(self.staticText("The backup was not restored. Try another backup or resolve the problem and try again.", in: restoreScope).exists)
                                }
                                let retry = app.buttons["settings.backups.retry"].firstMatch
                                if state == "error", try self.reach(retry, in: app) { try audit("list failure retry") }
                                XCTAssertEqual(app.buttons.matching(identifier: "settings.backups.retry").count, 1)
                                XCTAssertEqual(retry.label, "Try Again")
                                XCTAssertTrue(retry.isEnabled)
                            } else if state == "loading" {
                                self.assertProgress("settings.backups.loading", label: "Loading Backups…", in: app)
                            } else {
                                let names = ["backup-2026-11-25-17.25.00.colpkg", "server-copy-2026-11-24-09.10.00.colpkg",
                                             "server-copy-2026-11-23-09.10.00.unverified.anki2", "backup-2026-11-22-08.00.00.colpkg"]
                                for (index, name) in names.enumerated() {
                                    let row = app.descendants(matching: .any).matching(identifier: "settings.backups.entry.\(name)").firstMatch
                                    #if os(iOS)
                                    let scrolled = try self.reachHistoryEntry(index, names: names, in: app)
                                    #else
                                    let scrolled = try self.reach(row, in: app)
                                    #endif
                                    guard row.exists else {
                                        XCTFail("Reached backup entry disappeared before its semantics were read: \(name)\n\(app.debugDescription)")
                                        return .init(state: "\(surface).\(state)", findings: findings, touchTargets: targets)
                                    }
                                    let label = row.label, value = row.value as? String
                                    XCTAssertTrue(label.contains(name), "The exact filename is read with the row")
                                    let sizes: [Int64] = [28_400_000, 27_900_000, 27_800_000, 27_600_000]
                                    let size = sizes[index].formatted(.byteCount(style: .file).locale(Locale(identifier: "en_US")))
                                    XCTAssertTrue(label.contains(size))
                                    // Native List may put isSelected on an ancestor. Check
                                    // the exact entry's AX value for both selection outcomes.
                                    XCTAssertEqual(value, index == (state == "overflow" ? 1 : 0) ? "Selected" : "Not selected")
                                    if index == 1 { XCTAssertTrue(label.contains("Server copy")) }
                                    if index == 2 || (index == 3 && state == "overflow") {
                                        XCTAssertTrue(label.contains("Can’t be restored"))
                                    }
                                    if scrolled { try audit("entry.\(index)") }
                                }
                                // The real native confirmation discloses the selected older copy's
                                // date and the provider's publication/media decision. No restore runs.
                                try app.bringToFront(bundleIdentifier: self.bundle)
                                apply.clickOrTap()
                                var calendar = Calendar(identifier: .gregorian)
                                calendar.timeZone = .gmt
                                let selectedDate = try XCTUnwrap(calendar.date(from: DateComponents(
                                    year: 2026, month: 11, day: state == "overflow" ? 24 : 25,
                                    hour: state == "overflow" ? 9 : 17, minute: state == "overflow" ? 10 : 25, second: 0)))
                                let formatter = DateFormatter()
                                formatter.locale = Locale(identifier: "en_US")
                                formatter.calendar = calendar; formatter.timeZone = .gmt
                                formatter.setLocalizedDateFormatFromTemplate("yMMMdjmmss")
                                let date = formatter.string(from: selectedDate)
                                let title = "Replace the current collection with the backup from \(date)?"
                                let message = state == "overflow"
                                    ? "Meguri backs up the current collection first. Media files are kept. Restoring does not upload this collection to Meguri Sync. Review the collection choice in Sync before publishing."
                                    : "Meguri backs up the current collection first. Media files are kept."
                                // Bind the action and text to the smallest native prompt.
                                #if os(macOS)
                                let contexts = app.descendants(matching: .any).matching(NSPredicate(format: "elementType IN %@", [
                                    XCUIElement.ElementType.popover.rawValue, XCUIElement.ElementType.menu.rawValue,
                                    XCUIElement.ElementType.sheet.rawValue, XCUIElement.ElementType.alert.rawValue,
                                    XCUIElement.ElementType.dialog.rawValue,
                                ]))
                                #else
                                let contexts = app.alerts
                                #endif
                                var found: XCUIElement?
                                XCTAssertTrue(self.wait {
                                    found = contexts.allElementsBoundByIndex.filter {
                                        self.confirmationRestore(in: $0).exists
                                            && self.containsText(title, in: $0) && self.containsText(message, in: $0)
                                            && !$0.descendants(matching: .any).matching(identifier: anchor).firstMatch.exists
                                    }.min { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }
                                    return found != nil
                                }, "Expected native Restore confirmation for \(date)")
                                let confirmation = try XCTUnwrap(found,
                                    "Expected native Restore confirmation: \(restoreScope.debugDescription)")
                                let restore = self.confirmationRestore(in: confirmation)
                                let cancel = self.confirmationAction("Cancel", excluding: anchor, in: confirmation)
                                for action in [cancel, restore] {
                                    XCTAssertTrue(action.exists && action.isEnabled && action.isHittable,
                                        "The same native prompt must offer visible, enabled Cancel and Restore")
                                    #if os(iOS)
                                    XCTAssertFalse(action.frame.isEmpty)
                                    XCTAssertTrue(confirmation.frame.contains(action.frame))
                                    #endif
                                }
                                #if os(iOS)
                                XCTAssertEqual(confirmation.elementType, .alert)
                                #endif
                                XCTAssertTrue(self.containsText(title, in: confirmation))
                                XCTAssertTrue(self.containsText(message, in: confirmation))
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

    private func assertProgress(_ identifier: String, label expectedLabel: String, in app: XCUIApplication,
                                file: StaticString = #filePath, line: UInt = #line) {
        let matches = app.descendants(matching: .any).matching(identifier: identifier)
        let count = matches.count
        guard count == 1 else {
            XCTFail("Expected exactly one native progress element with identifier \(identifier); found \(count)\n\(app.debugDescription)",
                    file: file, line: line)
            return
        }
        let progress = matches.element
        XCTAssertTrue(progress.elementType == .activityIndicator || progress.elementType == .progressIndicator,
                      "Expected ActivityIndicator or ProgressIndicator for \(identifier); found \(progress.elementType)\n\(progress.debugDescription)",
                      file: file, line: line)
        XCTAssertEqual(progress.label, expectedLabel,
                       "Expected exact progress label for \(identifier): \(progress.debugDescription)", file: file, line: line)
        XCTAssertEqual(progress.value as? String, "In progress",
                       "Expected exact progress value for \(identifier): \(progress.debugDescription)", file: file, line: line)
    }

    private func confirmationRestore(in context: XCUIElement) -> XCUIElement {
        confirmationAction("Restore", excluding: "settings.backups.apply", in: context)
    }

    private func confirmationAction(_ title: String, excluding identifier: String, in context: XCUIElement) -> XCUIElement {
        #if os(macOS)
        let caption = NSPredicate(format: "(label == %@ OR title == %@) AND identifier != %@",
            title, title, identifier)
        return context.descendants(matching: .any).matching(NSPredicate(format: "elementType IN %@", [
            XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.menuItem.rawValue,
        ])).matching(caption).firstMatch
        #else
        return context.buttons.matching(NSPredicate(format: "label == %@ AND identifier != %@", title, identifier)).firstMatch
        #endif
    }

    private func containsText(_ expected: String, in context: XCUIElement) -> Bool {
        context.label.contains(expected) || (context.value as? String)?.contains(expected) == true
            || context.descendants(matching: .any).matching(NSPredicate(
                format: "label CONTAINS %@ OR value CONTAINS %@", expected, expected)).firstMatch.exists
    }

    private func wait(_ condition: @escaping () -> Bool) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)],
            timeout: 5) == .completed
    }

    /// Native static text can expose its displayed string as either label or value.
    /// Match content only, never identifiers or text from another element.
    private func staticText(_ expected: String, in scope: XCUIElement, contains: Bool = false) -> XCUIElement {
        scope.staticTexts.matching(NSPredicate(
            format: contains ? "label CONTAINS %@ OR value CONTAINS %@" : "label == %@ OR value == %@",
            expected, expected)).firstMatch
    }

    private func assertStaticText(_ element: XCUIElement, contains expected: String,
                                  file: StaticString = #filePath, line: UInt = #line) {
        guard element.exists else {
            XCTFail("Expected static text containing \(expected.debugDescription): \(element.debugDescription)", file: file, line: line)
            return
        }
        let label = element.label, value = element.value as? String
        XCTAssertTrue(label.contains(expected) || value?.contains(expected) == true,
                      "Expected static text containing \(expected.debugDescription); label=\(label.debugDescription), value=\(String(describing: value))\n\(element.debugDescription)",
                      file: file, line: line)
    }

    #if os(iOS)
    private func reachHistoryEntry(_ index: Int, names: [String], in app: XCUIApplication) throws -> Bool {
        let history = app.collectionViews.matching(identifier: "settings.backups.restoreContent").firstMatch
        guard history.waitForExistence(timeout: 5) else {
            XCTFail("Expected the native backup history collection view")
            return false
        }
        let window = app.windows.containing(.collectionView, identifier: "settings.backups.restoreContent").firstMatch
        let rows = names.map { name in
            history.descendants(matching: .any).matching(identifier: "settings.backups.entry.\(name)").firstMatch
        }
        let target = rows[index]
        var scrolled = false
        for _ in 0..<24 {
            let historyFrame = history.frame
            let viewport = historyFrame.intersection(window.frame)
            let frame = target.exists ? target.frame : .null
            if !frame.isEmpty && viewport.contains(frame) && target.isHittable { return scrolled }
            // An audit can leave even the next row above the viewport. Missing
            // lazy cells have no AX frame: use only existing known neighbours.
            let earlierVisible = rows.prefix(index).contains { row in
                guard row.exists else { return false }
                let frame = row.frame
                return !frame.isEmpty && viewport.intersects(frame)
            }
            let step = viewport.height * (earlierVisible ? 1.0 / 3.0 : 0.75)
            let move = frame.isEmpty
                ? (earlierVisible ? -step : step)
                : max(-step, min(step, viewport.midY - frame.midY))
            // Centre the bounded drag in the actual history viewport so both
            // endpoints stay inside it, including the larger move toward the top.
            let start = history.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: viewport.midX - historyFrame.minX, dy: viewport.midY - historyFrame.minY - move / 2))
            start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 0, dy: move)),
                        withVelocity: .slow, thenHoldForDuration: 0.2)
            scrolled = true
        }
        XCTFail("Unreachable backup history entry: \(names[index])\n\(app.debugDescription)")
        return scrolled
    }
    #endif

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
            try app.bringToFront(bundleIdentifier: bundle)
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
