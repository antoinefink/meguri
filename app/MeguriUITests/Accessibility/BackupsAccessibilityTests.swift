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
                        // runAllVariants has already run a full audit and TouchTargets.
                        // Either scan can scroll native lazy content away before this closure.
                        @MainActor func verify(_ element: XCUIElement, detail: String,
                                               reach: () throws -> Bool,
                                               checks: (XCUIElement) throws -> Void) throws {
                            let scrolled = try reach()
                            let visible = try XCTUnwrap(element.exists ? element : nil,
                                "Reached backup content disappeared before verification: \(detail)\n\(app.debugDescription)")
                            try checks(visible)
                            if scrolled { try audit(detail) }
                        }
                        if surface == "settings" {
                            #if os(iOS)
                            let settings = app.collectionViews["settings.backups.content"].firstMatch
                            #else
                            let settings: XCUIElement = app
                            #endif
                            var order = ["create"]
                            if state == "loading" { order += ["progress", "cancelOperation"] }
                            order += ["restore", "folder", "export", "mediaExplanation"]
                            if state != "error" { order += ["limit.interval", "limit.daily", "limit.weekly", "limit.monthly"] }
                            if state == "typical" || state == "overflow" { order += ["server", "serverExplanation"] }
                            let neighbours = order.map {
                                settings.descendants(matching: .any).matching(identifier: "settings.backups.\($0)").firstMatch
                            }
                            @MainActor func check(_ element: XCUIElement, _ detail: String, passive: Bool = false,
                                                  checks: (XCUIElement) throws -> Void) throws {
                                try verify(element, detail: detail, reach: {
                                    let index = try XCTUnwrap(order.firstIndex(of: detail))
                                    return try self.reach(element, detail: detail, in: app, scope: settings,
                                                          preceding: Array(neighbours.prefix(index)), passive: passive)
                                }, checks: checks)
                            }
                            try check(settings.buttons["settings.backups.create"].firstMatch, "create",
                                      passive: state == "loading" || state == "error") { create in
                                XCTAssertEqual(create.label, "Back Up Now")
                                XCTAssertEqual(create.isEnabled, state != "loading" && state != "error")
                            }
                            if state == "loading" {
                                let progress = settings.descendants(matching: .any).matching(identifier: "settings.backups.progress").firstMatch
                                try check(progress, "progress", passive: true) { _ in
                                    self.assertProgress("settings.backups.progress", label: "Creating Backup…", in: app)
                                }
                                try check(settings.buttons["settings.backups.cancelOperation"].firstMatch, "cancelOperation") { cancel in
                                    XCTAssertEqual(cancel.label, "Cancel")
                                    XCTAssertTrue(cancel.isEnabled)
                                }
                            }
                            for name in ["restore", "folder", "export"] {
                                try check(settings.buttons["settings.backups.\(name)"].firstMatch, name) { control in
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
                            }
                            try check(settings.staticTexts["settings.backups.mediaExplanation"].firstMatch,
                                      "mediaExplanation") { media in
                                self.assertStaticText(media, contains: "Device backups contain only the collection")
                            }
                            if state != "error" {
                                for field in ["interval", "daily", "weekly", "monthly"] {
                                    try check(settings.textFields["settings.backups.limit.\(field)"].firstMatch, "limit.\(field)") { input in
                                        XCTAssertFalse(input.label.isEmpty)
                                        XCTAssertEqual(input.isEnabled, state != "loading")
                                        if state == "overflow" && (field == "interval" || field == "daily") {
                                            XCTAssertEqual(input.value as? String, "4294967295")
                                        }
                                    }
                                }
                            }
                            if state == "typical" || state == "overflow" {
                                let link = settings.descendants(matching: .any).matching(identifier: "settings.backups.server").firstMatch
                                try check(link, "server") { XCTAssertEqual($0.label, "Server Backups in Browser…") }
                                let disclosure = settings.staticTexts["settings.backups.serverExplanation"].firstMatch
                                try check(disclosure, "serverExplanation", passive: true) {
                                    self.assertStaticText($0, contains: "Scheduled server backups include media")
                                    self.assertStaticText($0, contains: "Restoring there replaces the server collection")
                                }
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
                            let applyQuery = restoreScope.buttons["settings.backups.apply"].firstMatch
                            let apply = try XCTUnwrap(applyQuery.exists ? applyQuery : nil, "Expected the native Restore action")
                            XCTAssertEqual(apply.label, "Restore…")
                            XCTAssertEqual(apply.isEnabled, state == "typical" || state == "overflow")
                            let cancelQuery = restoreScope.buttons["settings.backups.cancel"].firstMatch
                            let cancelAction = try XCTUnwrap(cancelQuery.exists ? cancelQuery : nil, "Expected the native Cancel action")
                            XCTAssertEqual(cancelAction.label, "Cancel")
                            XCTAssertTrue(cancelAction.isEnabled)
                            if state == "empty" || state == "error" {
                                #if os(iOS)
                                let textScope = restoreScope.descendants(matching: .any).matching(identifier: "settings.backups.restoreContent").firstMatch
                                #else
                                let textScope = restoreScope
                                #endif
                                let emptyHeading = "No Backups Yet"
                                let emptyDescription = "Create a backup on this device, or check the configured backup folder."
                                let errorHeading = "Unable to Load Backups"
                                let errorDescription = "The backup could not be read or written. Check file access and free space, then try again."
                                let heading = state == "empty" ? emptyHeading : errorHeading
                                let description = state == "empty" ? emptyDescription : errorDescription
                                XCTAssertFalse(self.staticText(state == "empty" ? errorHeading : emptyHeading, in: textScope).exists)
                                XCTAssertFalse(self.staticText(state == "empty" ? errorDescription : emptyDescription, in: textScope).exists)
                                if state == "error" {
                                    XCTAssertFalse(self.staticText("The backup was not restored. Try another backup or resolve the problem and try again.", in: textScope).exists)
                                }
                                let title = self.staticText(heading, in: textScope)
                                let explanation = self.staticText(description, in: textScope)
                                let retry = textScope.buttons["settings.backups.retry"].firstMatch
                                let order = [title, explanation, retry]
                                for (index, item) in [(title, "list heading", state == "empty"),
                                                      (explanation, "list explanation", state == "empty"),
                                                      (retry, "list retry", false)].enumerated() {
                                    let (element, detail, passive) = item
                                    try verify(element, detail: detail, reach: {
                                        try self.reach(element, detail: detail, in: app, scope: restoreScope, preceding: Array(order.prefix(index)), passive: passive)
                                    }) { visible in
                                        if detail == "list retry" {
                                            XCTAssertEqual(app.buttons.matching(identifier: "settings.backups.retry").count, 1)
                                            XCTAssertEqual(visible.label, "Try Again")
                                            XCTAssertTrue(visible.isEnabled)
                                        } else {
                                            // Recheck the exact local text only after reaching this item.
                                            let expected = detail == "list heading" ? heading : description
                                            XCTAssertTrue(visible.label == expected || visible.value as? String == expected)
                                        }
                                    }
                                }
                            } else if state == "loading" {
                                let progress = restoreScope.descendants(matching: .any).matching(identifier: "settings.backups.loading").firstMatch
                                try verify(progress, detail: "loading", reach: {
                                    try self.reach(progress, detail: "loading", in: app, scope: restoreScope, passive: true)
                                }) { _ in
                                    self.assertProgress("settings.backups.loading", label: "Loading Backups…", in: app)
                                }
                            } else {
                                let names = ["backup-2026-11-25-17.25.00.colpkg", "server-copy-2026-11-24-09.10.00.colpkg",
                                             "server-copy-2026-11-23-09.10.00.unverified.anki2", "backup-2026-11-22-08.00.00.colpkg"]
                                for (index, name) in names.enumerated() {
                                    let row = restoreScope.descendants(matching: .any).matching(identifier: "settings.backups.entry.\(name)").firstMatch
                                    #if os(iOS)
                                    let scrolled = try self.reachHistoryEntry(index, names: names, in: app)
                                    #else
                                    let scrolled = try self.reach(row, detail: "settings.backups.entry.\(name)", in: app, scope: restoreScope)
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
                                XCTAssertTrue(apply.exists, "Restore action must remain available after history audits")
                                XCTAssertEqual(apply.label, "Restore…")
                                XCTAssertTrue(apply.isEnabled && apply.isHittable)
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
        XCTAssertEqual(progress.label, expectedLabel,
                       "Expected exact progress label for \(identifier): \(progress.debugDescription)", file: file, line: line)
        #if os(macOS)
        XCTAssertEqual(progress.elementType, .activityIndicator,
                       "Expected ActivityIndicator for \(identifier): \(progress.debugDescription)", file: file, line: line)
        guard let value = progress.value as? NSNumber else {
            XCTFail("Expected numeric progress value for \(identifier): \(progress.debugDescription)", file: file, line: line)
            return
        }
        XCTAssertEqual(value, NSNumber(value: 0),
                       "Expected exact progress value for \(identifier): \(progress.debugDescription)", file: file, line: line)
        #else
        XCTAssertTrue(progress.elementType == .activityIndicator || progress.elementType == .progressIndicator,
                      "Expected ActivityIndicator or ProgressIndicator for \(identifier); found \(progress.elementType)\n\(progress.debugDescription)",
                      file: file, line: line)
        XCTAssertEqual(progress.value as? String, "In progress",
                       "Expected exact progress value for \(identifier): \(progress.debugDescription)", file: file, line: line)
        #endif
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
            XCTFail("Expected static text containing \(expected.debugDescription)", file: file, line: line)
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
        return try reachTouch(rows[index], target: "settings.backups.entry.\(names[index])", in: history, window: window,
                              preceding: Array(rows.prefix(index)), passive: false)
    }

    private struct TouchScrollProgress {
        let indicatorFrame: CGRect?
        let nativeValue: String?
        let fraction: Double?
        let visibleFrames: [String: CGRect]

        var summary: String {
            let sample = visibleFrames.sorted { $0.key < $1.key }.prefix(3).map { "\($0.key)=\($0.value)" }
            return "offset=\(nativeValue ?? "unavailable"), visible=\(visibleFrames.count) \(sample), indicator=\(String(describing: indicatorFrame))"
        }
    }

    /// Read the active native List/ScrollView only. Some virtualized snapshots
    /// expose no cells at all, but still expose a real percentage on the scrollbar.
    private func touchProgress(in scroll: XCUIElement, viewport: CGRect) throws -> TouchScrollProgress {
        let snapshot = try scroll.snapshot()
        var indicator: (any XCUIElementSnapshot)?
        var frames: [String: CGRect] = [:]
        @MainActor func visit(_ node: any XCUIElementSnapshot) {
            let label = node.label.lowercased()
            let vertical = node.elementType == .scrollBar || label.contains("vertical scroll bar")
                || label.contains("vertical scrolling indicator") || label.contains("vertical scroll indicator")
            if vertical && node.frame.height > node.frame.width && node.frame.intersects(viewport) {
                indicator = node
                return
            }
            switch node.elementType {
            case .cell, .staticText, .button, .textField, .link, .image, .switch, .activityIndicator, .progressIndicator:
                if !node.frame.isEmpty && node.frame.intersects(viewport) {
                    let key = "\(node.elementType.rawValue)|\(node.identifier)|\(node.label)"
                    frames[key] = node.frame
                }
            default: break
            }
            for child in node.children { visit(child) }
        }
        visit(snapshot)
        let value = indicator.flatMap { $0.value }.map { String(describing: $0) }
        let percent = value.flatMap { value -> Double? in
            guard let range = value.range(of: #"[0-9]+(?:[.,][0-9]+)?(?=\s*%)"#, options: .regularExpression) else { return nil }
            return Double(value[range].replacingOccurrences(of: ",", with: "."))
        }
        return TouchScrollProgress(indicatorFrame: indicator?.frame, nativeValue: value,
                                   fraction: percent.map { $0 / 100 }, visibleFrames: frames)
    }

    /// Gesture bounds exclude this surface's navigation bar even without an
    /// indicator. A transient scrollbar thumb never defines content visibility.
    /// Toolbars can report the whole device and are not clipping witnesses.
    private func touchGestureBounds(in scroll: XCUIElement, window: XCUIElement) -> CGRect {
        var viewport = scroll.frame.intersection(window.frame)
        guard !viewport.isEmpty else { return .null }
        for bar in window.navigationBars.allElementsBoundByIndex {
            let frame = bar.frame
            let relevant = bar.identifier == "Backups" || bar.identifier == "Restore from Backup"
                || bar.label == "Backups" || bar.label == "Restore from Backup"
                || bar.buttons.matching(identifier: "settings.backups.cancel").firstMatch.exists
                || bar.buttons.matching(identifier: "settings.backups.apply").firstMatch.exists
            // Do not clip an unrelated iPad sidebar's navigation or status areas.
            guard relevant, frame.contains(CGPoint(x: viewport.midX, y: frame.midY)),
                  frame.intersection(viewport).width >= viewport.width / 2,
                  frame.minY <= viewport.midY, frame.maxY > viewport.minY,
                  frame.maxY < viewport.maxY else { continue }
            viewport = CGRect(x: viewport.minX, y: frame.maxY, width: viewport.width,
                              height: viewport.maxY - frame.maxY)
        }
        return viewport
    }

    /// Audits can finish at 100% with every lazy cell virtualized. Recover using
    /// native offset and local semantic order, with both drag ends inside content.
    private func reachTouch(_ element: XCUIElement, target: String, in scroll: XCUIElement, window: XCUIElement,
                            preceding: [XCUIElement], passive: Bool,
                            canScroll: Bool = true) throws -> Bool {
        _ = try XCTUnwrap(scroll.exists ? scroll : nil, "Expected the native backup content container")
        _ = try XCTUnwrap(window.exists ? window : nil, "Expected the window containing the backup content anchor")
        var scrolled = false
        var trace: [String] = []
        var traceSaved = false
        func saveTrace() {
            if !trace.isEmpty && !traceSaved {
                traceSaved = true
                let attachment = XCTAttachment(string: "target=\(target)\n" + trace.joined(separator: "\n"))
                attachment.name = "Backups native scroll trace"
                attachment.lifetime = .deleteOnSuccess
                add(attachment)
            }
        }
        func saveFailureEvidence() {
            saveTrace()
            let image = XCTAttachment(screenshot: window.screenshot())
            image.name = "Backups unreachable content"
            image.lifetime = .keepAlways
            add(image)
            let tree = XCTAttachment(string: window.debugDescription)
            tree.name = "Backups final native scroll tree"
            tree.lifetime = .keepAlways
            add(tree)
        }
        defer { saveTrace() }
        for _ in 0..<24 {
            let scrollFrame = scroll.frame
            let viewport = scrollFrame.intersection(window.frame)
            guard !viewport.isEmpty else { break }
            let frame = element.exists ? element.frame : .null
            if !frame.isEmpty && viewport.contains(frame) && element.isHittable { return scrolled }
            // Only newly checked passive content may exceed the window. Every
            // original target still requires full visibility and a hit target.
            let visible = passive && frame.height > window.frame.height
                ? viewport.intersects(frame) : viewport.contains(frame)
            if passive && !frame.isEmpty && visible { return scrolled }
            guard canScroll else { break }
            let earlierVisible = preceding.contains { row in
                guard row.exists else { return false }
                let frame = row.frame
                return !frame.isEmpty && viewport.intersects(frame)
            }
            let interior = touchGestureBounds(in: scroll, window: window).insetBy(dx: 1, dy: 1)
            guard interior.width > 0, interior.height > 0 else { break }
            let before: TouchScrollProgress
            do { before = try touchProgress(in: scroll, viewport: viewport) }
            catch { saveFailureEvidence(); throw error }
            let step = interior.height * (earlierVisible ? 1.0 / 3.0 : 0.75)
            let seekEarlier: Bool
            if !frame.isEmpty {
                seekEarlier = frame.midY < interior.midY
            } else if let offset = before.fraction, offset == 1 || offset == 0 {
                seekEarlier = offset == 1
            } else {
                seekEarlier = !earlierVisible
            }
            let distance = frame.isEmpty ? step : min(step, abs(interior.midY - frame.midY))
            let move = seekEarlier ? distance : -distance
            var x = interior.midX
            if let track = before.indicatorFrame, x >= track.minX && x <= track.maxX {
                let left = max(0, track.minX - interior.minX)
                let right = max(0, interior.maxX - track.maxX)
                x = left >= right ? interior.minX + left / 2 : track.maxX + right / 2
            }
            let startPoint = CGPoint(x: x, y: interior.midY - move / 2)
            let endPoint = CGPoint(x: x, y: interior.midY + move / 2)
            guard distance > 0, interior.contains(startPoint), interior.contains(endPoint),
                  before.indicatorFrame?.contains(startPoint) != true,
                  before.indicatorFrame?.contains(endPoint) != true else { break }
            let origin = scroll.coordinate(withNormalizedOffset: .zero)
            let start = origin.withOffset(CGVector(dx: startPoint.x - scrollFrame.minX, dy: startPoint.y - scrollFrame.minY))
            let end = origin.withOffset(CGVector(dx: endPoint.x - scrollFrame.minX, dy: endPoint.y - scrollFrame.minY))
            let direction = seekEarlier ? "earlier" : "later"
            let gesture = "window=\(window.frame), scroll=\(scrollFrame), viewport=\(viewport), input=\(interior), gesture=\(startPoint)->\(endPoint)"
            trace.append("\(direction): before {\(before.summary)}; \(gesture); after=pending")
            scrolled = true
            start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.2)
            let after: TouchScrollProgress
            do { after = try touchProgress(in: scroll, viewport: viewport) }
            catch { saveFailureEvidence(); throw error }
            let beforeNodes = Set(before.visibleFrames.keys)
            let afterNodes = Set(after.visibleFrames.keys)
            let identitiesChanged = beforeNodes != afterNodes
            let commonNodes = beforeNodes.intersection(afterNodes)
            let movedNodes = commonNodes.filter { before.visibleFrames[$0] != after.visibleFrames[$0] }.sorted()
            let contentMoved = identitiesChanged || !movedNodes.isEmpty
            let offsetMoved = before.fraction != nil && after.fraction != nil && before.fraction != after.fraction
            let movement = movedNodes.prefix(3).map { "\($0): \(before.visibleFrames[$0]!) -> \(after.visibleFrames[$0]!)" }
            trace[trace.count - 1] = "\(direction): before {\(before.summary)} after {\(after.summary)}, identitiesChanged=\(identitiesChanged), moved=\(movement); \(gesture)"
            // A rounded percentage is corroborated by content identities and frames.
            // Equal percentages also detect inert gestures with virtualized rows.
            let sameOffset = before.fraction != nil && before.fraction == after.fraction
            if !contentMoved && !offsetMoved && (!commonNodes.isEmpty || sameOffset) {
                saveFailureEvidence()
                XCTFail("Inert native backup gesture; target=\(target); \(trace.last!)")
                return scrolled
            }
        }
        saveFailureEvidence()
        XCTFail("Unreachable backup content: \(target); scroll=\(scroll.frame), window=\(window.frame); \(trace.last ?? "no valid native gesture")")
        return scrolled
    }
    #endif

    /// Lazy native Forms and Lists must bring every tested row into the viewport,
    /// including at AX5. A missing or unreachable row fails, never skips its audit.
    /// Returns whether scrolling changed the viewport and requires another audit.
    private func reach(_ element: XCUIElement, detail: String, in app: XCUIApplication, scope: XCUIElement? = nil,
                       preceding: [XCUIElement] = [], passive: Bool = false) throws -> Bool {
        let scope = scope ?? app
        _ = try XCTUnwrap(scope.exists ? scope : nil, "Expected the native backup verification scope")
        #if os(iOS)
        if scope.elementType == .collectionView {
            let window = app.windows.containing(.collectionView, identifier: "settings.backups.content").firstMatch
            return try reachTouch(element, target: detail, in: scope, window: window, preceding: preceding, passive: passive)
        }
        let identifier = "settings.backups.restoreContent"
        let content = scope.descendants(matching: .any).matching(identifier: identifier).firstMatch
        let window = app.windows.containing(.any, identifier: identifier).firstMatch
        // SwiftUI may assign restoreContent to the ScrollView itself or its
        // enclosing native container. Never select the underlying iPad sidebar.
        let namedScroll = scope.scrollViews.matching(identifier: identifier).firstMatch
        let childScroll = content.scrollViews.firstMatch
        let errorScroll = scope.scrollViews.containing(.any, identifier: "settings.backups.listFailure").firstMatch
        let scroll = namedScroll.exists ? namedScroll : (childScroll.exists ? childScroll : errorScroll)
        return try reachTouch(element, target: detail, in: scroll.exists ? scroll : content, window: window,
                              preceding: preceding, passive: passive, canScroll: scroll.exists)
        #else
        let list = scope.collectionViews.firstMatch
        let scroll = list.exists ? list : scope.scrollViews.firstMatch
        let window = scope.elementType == .window ? scope : app.windows.firstMatch
        var scrolled = false
        for _ in 0..<24 {
            let viewport = (scroll.exists ? scroll.frame : window.frame).intersection(window.frame)
            guard !viewport.isEmpty else { break }
            let frame = element.exists ? element.frame : .null
            if !frame.isEmpty && viewport.contains(frame) && element.isHittable { return scrolled }
            let visible = passive && frame.height > window.frame.height
                ? viewport.intersects(frame) : viewport.contains(frame)
            if passive && !frame.isEmpty && visible { return scrolled }
            guard scroll.exists else { break }
            let step = viewport.height / 3
            let move = frame.isEmpty ? -step : max(-step, min(step, viewport.midY - frame.midY))
            try app.bringToFront(bundleIdentifier: bundle)
            scroll.scroll(byDeltaX: 0, deltaY: move)
            scrolled = true
        }
        XCTFail("Unreachable backup content: \(detail)\n\(app.debugDescription)")
        return scrolled
        #endif
    }
}
