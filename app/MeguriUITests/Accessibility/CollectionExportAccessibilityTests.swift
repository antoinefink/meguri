import Foundation
import XCTest

@MainActor final class CollectionExportAccessibilityTests: XCTestCase {
    func testExportStatesPassAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("VM development build required") }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["typical", "error", "loading"].map { state in
            try AccessibilityAudit.runAllVariants(state: state,
                launch: MeguriLaunch(locale: "en_US", animations: false, route: "settings/import-export/\(state)"),
                waitFor: "settings.importExport.content", suppressions: suppressions,
                supplemental: { app, variant in
                    #if os(macOS)
                    for identifier in ["settings.importExport.content", "settings.importExport.explanation"] {
                        XCTAssertTrue(app.descendants(matching: .any)[identifier].firstMatch.exists,
                            "Foreground export content must remain accessible: \(identifier)")
                    }
                    let controls = ["settings.importExport.close"] + (state == "typical"
                        ? ["settings.importExport.choose", "settings.importExport.discard"]
                        : state == "error" ? ["settings.importExport.prepare"] : [])
                    for identifier in controls {
                        XCTAssertTrue(app.buttons[identifier].firstMatch.exists,
                            "Foreground export controls must remain accessible: \(identifier)")
                    }
                    for identifier in ["shell.root.sidebarDecksEmpty", "shell.root.sidebar.home"] {
                        XCTAssertFalse(app.descendants(matching: .any)[identifier].firstMatch.exists,
                            "The underlying shell must leave the AX tree while the export sheet is active: \(identifier)")
                    }
                    #endif
                    let scroll = app.scrollViews["settings.importExport.content"].firstMatch
                    let close = app.buttons["settings.importExport.close"].firstMatch
                    let footer = app.descendants(matching: .any)["settings.importExport.footer"].firstMatch
                    XCTAssertTrue(close.exists)
                    XCTAssertEqual(close.label, state == "loading" ? "Cancel Export" : "Done")
                    let required = ["settings.importExport.explanation"] + (state == "typical"
                        ? ["settings.importExport.ready", "settings.importExport.readyDetail",
                           "settings.importExport.choose", "settings.importExport.discard"]
                        : state == "error" ? ["settings.importExport.message", "settings.importExport.prepare"]
                        : ["settings.importExport.progress"])
                    if state == "loading" {
                        let progress = scroll.descendants(matching: .any)["settings.importExport.progress"].firstMatch
                        XCTAssertTrue(progress.exists)
                        XCTAssertEqual(progress.elementType, .activityIndicator)
                        XCTAssertGreaterThanOrEqual(progress.frame.width, 44)
                        XCTAssertGreaterThanOrEqual(progress.frame.height, 44)
                        XCTAssertEqual(progress.label, "Preparing collection export…")
                    }
                    var findings: [AuditFinding] = []
                    var touchTargets: [TouchTargetViolation] = []
                    try self.scrollToRequiredContent(scroll, close: close, footer: footer, app: app, build: build,
                                                     identifiers: required) { visible, viewport, portions in
                        // Only full-frame visibility removes an exact contrast
                        // exception. A paragraph portion retains its exception.
                        let visibleSuppressions = suppressions.filter {
                            !($0.auditType == "contrast" && visible.contains($0.identifier))
                        }
                        let label = "\(state) scrolled, \(visible.count) full frames, \(portions.count) paragraph portions, \(variant)"
                        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
                        shot.name = "export coverage \(label)"
                        shot.lifetime = .keepAlways
                        self.add(shot)
                        let frames = visible.map {
                            "\($0): \(scroll.descendants(matching: .any)[$0].firstMatch.frame)"
                        }.joined(separator: "\n")
                        let proof = XCTAttachment(string: "Scroll viewport: \(viewport)\nFull frames:\n\(frames)\nParagraph portions:\n\(portions.joined(separator: "\n"))\nFooter: \(footer.frame)\nDone/Cancel: \(close.frame)")
                        proof.name = "export visibility \(label)"
                        proof.lifetime = .keepAlways
                        self.add(proof)
                        findings += try AccessibilityAudit.run(app, variant: label, suppressions: visibleSuppressions)
                        touchTargets += try TouchTargets.violations(in: app)
                    }
                    return AccessibilityAudit.StateResult(state: state, findings: findings, touchTargets: touchTargets)
                })
        }
        add(AccessibilityAudit.screenReport("settings.importExport", states: states))
        for state in states { AccessibilityAudit.assertClean(state.findings); TouchTargets.assertMet(state.touchTargets) }
    }

    /// Controls require full frames; tall prose requires continuous observed coverage
    /// from its beginning to its end, with exact accessible text throughout.
    private func scrollToRequiredContent(_ scroll: XCUIElement, close: XCUIElement, footer: XCUIElement,
                                         app: XCUIApplication,
                                         build: BuildUnderTest, identifiers: [String],
                                         onVisible: ([String], CGRect, [String]) throws -> Void) throws {
        guard scroll.exists, scroll.elementType == .scrollView else {
            throw AuditError(description: "Expected the exact settings.importExport.content ScrollView")
        }
        try app.bringToFront(bundleIdentifier: try build.bundleIdentifier)
        let targets = identifiers.map { scroll.descendants(matching: .any)[$0].firstMatch }
        for (index, target) in targets.enumerated() {
            guard target.waitForExistence(timeout: 10) else {
                throw AuditError(description: "Missing foreground export content: \(identifiers[index])")
            }
        }
        let paragraphs = [
            "settings.importExport.explanation": "The entire collection will be saved as a latest-format .colpkg. Media files, including images and audio, are excluded. Your saved reviews are included.",
            "settings.importExport.readyDetail": "This file captures the collection during preparation. Later reviews do not change this copy. Choose a destination to finish saving.",
            "settings.importExport.message": "The collection could not be exported. Check available space and collection access, then try again."
        ]
        var coverage: [Int: (height: CGFloat, end: CGFloat)] = [:]
        var checked: Set<Int> = []
        var gesture = 0
        while gesture <= 24 {
            let scrollFrame = scroll.frame
            var viewport = scrollFrame.intersection(app.windows.firstMatch.frame)
            XCTAssertTrue(close.exists && close.isEnabled && close.isHittable,
                          "The foreground Done/Cancel action must remain reachable")
            XCTAssertFalse(close.frame.isEmpty)
            XCTAssertTrue(app.windows.firstMatch.frame.contains(close.frame),
                          "The entire Done/Cancel frame must remain visible")
            guard footer.exists else {
                throw AuditError(description: "Missing the exact settings.importExport.footer container")
            }
            let footerFrame = footer.frame
            guard !footerFrame.isEmpty, footerFrame.contains(close.frame),
                  footerFrame.minY < close.frame.minY else {
                throw AuditError(description: "The padded export footer must contain Done/Cancel and begin above it: footer \(footerFrame), button \(close.frame)")
            }
            var top = viewport.minY
            #if os(iOS)
            let navigationBar = app.navigationBars["Export Collection"].firstMatch
            if navigationBar.exists, navigationBar.frame.intersects(viewport) {
                top = max(top, navigationBar.frame.maxY)
            }
            #endif
            // Native chrome and the entire padded footer cannot prove coverage.
            let bottom = min(viewport.maxY, footerFrame.minY)
            viewport = CGRect(x: viewport.minX, y: top, width: viewport.width, height: max(0, bottom - top))
            guard !viewport.isEmpty else { throw AuditError(description: "Export scroll viewport is empty") }
            let frames = targets.map(\.frame)
            for index in targets.indices {
                guard targets[index].exists, !frames[index].isEmpty,
                      frames[index].width <= viewport.width,
                      paragraphs[identifiers[index]] != nil || frames[index].height <= viewport.height else {
                    throw AuditError(description: "\(identifiers[index]) must have a nonempty frame that fits the actual scroll viewport: \(frames[index]) in \(viewport)")
                }
                if let text = paragraphs[identifiers[index]] {
                    #if os(macOS)
                    let exactText = targets[index].value as? String
                    #else
                    let exactText = targets[index].label
                    #endif
                    guard targets[index].identifier == identifiers[index],
                          targets[index].elementType == .staticText, exactText == text else {
                        throw AuditError(description: "Expected the exact accessible Text for \(identifiers[index]): \(text)")
                    }
                }
                if let previous = coverage[index], previous.height != frames[index].height {
                    throw AuditError(description: "Paragraph geometry changed during coverage: \(identifiers[index])")
                }
            }
            let visible = targets.indices.filter {
                viewport.contains(frames[$0]) && (targets[$0].elementType != .button
                    || (targets[$0].isEnabled && targets[$0].isHittable))
            }
            let newFullFrames = visible.contains(where: { !checked.contains($0) })
            var portions: [String] = []
            for index in targets.indices where !checked.contains(index)
                && paragraphs[identifiers[index]] != nil && frames[index].height > viewport.height {
                let frame = frames[index]
                let portion = frame.intersection(viewport)
                guard frame.minX >= viewport.minX, frame.maxX <= viewport.maxX,
                      !portion.isEmpty, portion.height >= viewport.height / 3 else { continue }
                let start = portion.minY - frame.minY
                let end = portion.maxY - frame.minY
                if let previous = coverage[index] {
                    guard start < previous.end else {
                        throw AuditError(description: "Scroll skipped unread paragraph content: \(identifiers[index]), covered 0...\(previous.end), observed \(start)...\(end)")
                    }
                    // Each further proof must extend the observed, gap-free range.
                    guard end > previous.end else { continue }
                } else {
                    guard start == 0 else { continue }
                }
                coverage[index] = (frame.height, end)
                portions.append("\(identifiers[index]), gesture \(gesture): frame \(frame), visible \(portion), text offsets \(start)...\(end), cumulative 0...\(end) of \(frame.height)")
                if portion.maxY == frame.maxY { checked.insert(index) }
            }
            if newFullFrames || !portions.isEmpty {
                for index in visible {
                    XCTAssertTrue(viewport.contains(targets[index].frame),
                                  "The entire content frame must be visible: \(identifiers[index])")
                }
                try onVisible(visible.map { identifiers[$0] }, viewport, portions)
                checked.formUnion(visible)
                // The system audit can change native scroll geometry. Only new
                // coverage enters here; resample before choosing any gesture.
                continue
            }
            if checked.count == targets.count { return }
            guard gesture < 24, let next = targets.indices.first(where: { !checked.contains($0) }) else { break }
            let offset: CGFloat
            if paragraphs[identifiers[next]] != nil, frames[next].height > viewport.height {
                // First expose the beginning; then advance overlapping portions
                // toward the end, rather than centring an oversized paragraph.
                offset = coverage[next] == nil
                    ? frames[next].minY - (viewport.minY + viewport.height / 6)
                    : frames[next].maxY - (viewport.maxY - viewport.height / 10)
            } else {
                offset = frames[next].midY - viewport.midY
            }
            guard abs(offset) > 1 else { break }
            gesture += 1
            #if os(macOS)
            let gestureArea = viewport.insetBy(dx: viewport.width / 10, dy: viewport.height / 10)
            let distance = min(abs(offset), gestureArea.height / 3) * (offset > 0 ? 1.0 : -1.0)
            scroll.scroll(byDeltaX: 0, deltaY: -distance)
            #else
            let gestureArea = viewport.insetBy(dx: viewport.width / 10, dy: viewport.height / 10)
            let distance = min(abs(offset), gestureArea.height / 3) * (offset > 0 ? 1.0 : -1.0)
            let origin = scroll.coordinate(withNormalizedOffset: .zero)
            let start = origin.withOffset(CGVector(dx: gestureArea.midX - scrollFrame.minX,
                dy: gestureArea.midY + distance / 2 - scrollFrame.minY))
            let end = origin.withOffset(CGVector(dx: gestureArea.midX - scrollFrame.minX,
                dy: gestureArea.midY - distance / 2 - scrollFrame.minY))
            start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0)
            #endif
            let afterFrames = targets.map(\.frame)
            let movement = frames[next].minY - afterFrames[next].minY
            guard offset > 0 ? movement > 1 : movement < -1 else {
                let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
                shot.name = "export scroll no progress gesture \(gesture)"
                shot.lifetime = .keepAlways
                add(shot)
                let frameChanges = targets.indices.map {
                    "\(identifiers[$0]): before \(frames[$0]), after \(afterFrames[$0])"
                }.joined(separator: "\n")
                let proof = XCTAttachment(string: "Target: \(identifiers[next])\nOffset: \(offset), movement: \(movement)\nCoverage viewport before gesture: \(viewport)\nScroll frame before gesture: \(scrollFrame), after: \(scroll.frame)\nFooter frame before gesture: \(footerFrame), after: \(footer.frame)\n\(frameChanges)")
                proof.name = "export scroll no progress frames gesture \(gesture)"
                proof.lifetime = .keepAlways
                add(proof)
                throw AuditError(description: "No meaningful scoped scroll progress for \(identifiers[next]) after gesture \(gesture)")
            }
        }
        let missing = targets.indices.filter { !checked.contains($0) }.map { identifiers[$0] }
        throw AuditError(description: "Export content needs full frames or continuous beginning-to-end paragraph coverage, and actions must be hittable, after at most 24 scoped scroll gestures: \(missing)\n\(app.debugDescription)")
    }
}
