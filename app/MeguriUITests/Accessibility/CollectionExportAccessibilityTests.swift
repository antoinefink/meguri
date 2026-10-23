import Foundation
import XCTest
#if os(iOS)
import UIKit
#endif

@MainActor final class CollectionExportAccessibilityTests: XCTestCase {
    func testExportStatesPassAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("VM development build required") }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["error", "typical", "loading"].map { state in
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
                    try self.scrollToRequiredContent(scroll, close: close, app: app, build: build,
                                                     identifiers: required) { visible, viewport, portions, auditFullFrames in
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
                        let proof = XCTAttachment(string: "Conservative content viewport (48pt inset above native button, not the exact background frame): \(viewport)\nScroll frame: \(scroll.frame)\nWindow frame: \(app.windows.firstMatch.frame)\nFull frames:\n\(frames)\nParagraph portions:\n\(portions.joined(separator: "\n"))\nNative Done/Cancel: \(close.frame)")
                        proof.name = "export visibility \(label)"
                        proof.lifetime = .keepAlways
                        self.add(proof)
                        if auditFullFrames {
                            var types: XCUIAccessibilityAuditType = .all
                            #if os(iOS)
                            if UIDevice.current.userInterfaceIdiom == .phone,
                               state == "error", variant == "light, largest accessibility size",
                               visible.contains("settings.importExport.prepare") {
                                types = .all.subtracting(.contrast)
                            }
                            #endif
                            findings += try AccessibilityAudit.run(app, variant: label, types: types,
                                suppressions: visibleSuppressions)
                        }
                        touchTargets += try TouchTargets.violations(in: app)
                    }
                    return AccessibilityAudit.StateResult(state: state, findings: findings, touchTargets: touchTargets)
                })
        }
        add(AccessibilityAudit.screenReport("settings.importExport", states: states))
        for state in states { AccessibilityAudit.assertClean(state.findings); TouchTargets.assertMet(state.touchTargets) }
    }

    /// Controls require full frames; prose requires continuous observed coverage
    /// from its beginning to its end, with exact accessible text throughout.
    private func scrollToRequiredContent(_ scroll: XCUIElement, close: XCUIElement,
                                         app: XCUIApplication,
                                         build: BuildUnderTest, identifiers: [String],
                                         onVisible: ([String], CGRect, [String], Bool) throws -> Void) throws {
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
        var auditedFullFrames: Set<Int> = []
        var gesture = 0
        // Bound observations too: audits may scroll without consuming a gesture.
        for _ in 0..<128 {
            let scrollFrame = scroll.frame
            var viewport = scrollFrame.intersection(app.windows.firstMatch.frame)
            XCTAssertTrue(close.exists && close.isEnabled && close.isHittable,
                          "The foreground Done/Cancel action must remain reachable")
            let closeFrame = close.frame
            XCTAssertFalse(closeFrame.isEmpty)
            XCTAssertTrue(app.windows.firstMatch.frame.contains(closeFrame),
                          "The entire Done/Cancel frame must remain visible")
            var top = viewport.minY
            #if os(iOS)
            let navigationBar = app.navigationBars["Export Collection"].firstMatch
            if navigationBar.exists, navigationBar.frame.intersects(viewport) {
                top = max(top, navigationBar.frame.maxY)
            }
            #endif
            // Test-only 48pt inset: two 24pt production Spacing.large padding
            // steps. UITests does not link DesignSystem. Retained 035747Z-85354
            // showed about 30pt from the native button to the covered edge, so
            // this boundary sits conservatively above the opaque footer; it
            // does not measure the exact background frame.
            let bottom = min(viewport.maxY, closeFrame.minY - 48)
            viewport = CGRect(x: viewport.minX, y: top, width: viewport.width, height: max(0, bottom - top))
            guard !viewport.isEmpty else { throw AuditError(description: "Export scroll viewport is empty") }
            let frames = targets.map(\.frame)
            for index in targets.indices {
                guard targets[index].exists, !frames[index].isEmpty,
                      frames[index].width <= viewport.width,
                      paragraphs[identifiers[index]] != nil || frames[index].height <= viewport.height else {
                    throw AuditError(description: "\(identifiers[index]) must have a nonempty frame that fits the conservative content viewport: \(frames[index]) in \(viewport)")
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
                    let delta = frames[index].height - previous.height
                    let signedDelta = String(format: "%+.17g", Double(delta))
                    let numerics = "\(identifiers[index]), baseline height \(previous.height)pt, current height \(frames[index].height)pt, signed delta \(signedDelta)pt, frame \(frames[index]), viewport \(viewport), gesture \(gesture)"
                    let proof = XCTAttachment(string: numerics)
                    proof.name = "export paragraph height delta \(identifiers[index]) gesture \(gesture)"
                    proof.lifetime = .keepAlways
                    add(proof)
                    guard abs(delta) <= 1e-6 else {
                        throw AuditError(description: "Paragraph geometry changed during coverage: \(numerics)")
                    }
                }
            }
            let visible = targets.indices.filter {
                viewport.contains(frames[$0]) && (targets[$0].elementType != .button
                    || (targets[$0].isEnabled && targets[$0].isHittable))
            }
            let newFullFrames = visible.contains(where: { !auditedFullFrames.contains($0) })
            var portions: [String] = []
            for index in targets.indices where !checked.contains(index)
                && paragraphs[identifiers[index]] != nil && !viewport.contains(frames[index]) {
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
                coverage[index] = (coverage[index]?.height ?? frame.height, end)
                portions.append("\(identifiers[index]), gesture \(gesture): frame \(frame), visible \(portion), text offsets \(start)...\(end), cumulative 0...\(end) of \(frame.height)")
                if portion.maxY == frame.maxY { checked.insert(index) }
            }
            if newFullFrames || !portions.isEmpty {
                for index in visible {
                    XCTAssertTrue(viewport.contains(targets[index].frame),
                                  "The entire content frame must be visible: \(identifiers[index])")
                }
                try onVisible(visible.map { identifiers[$0] }, viewport, portions, newFullFrames)
                checked.formUnion(visible)
                auditedFullFrames.formUnion(visible)
                // The system audit can change native scroll geometry. Only new
                // coverage enters here; resample before choosing any gesture.
                continue
            }
            if checked.count == targets.count { return }
            guard gesture < 48, let next = targets.indices.first(where: { !checked.contains($0) }) else { break }
            let offset: CGFloat
            if paragraphs[identifiers[next]] != nil {
                // First expose the beginning; after observed coverage, advance
                // overlapping portions toward the end even if the whole frame
                // could fit with precise alignment. Do not recenter prose.
                offset = coverage[next] == nil
                    ? frames[next].minY - (viewport.minY + viewport.height / 6)
                    : frames[next].maxY - (viewport.maxY - viewport.height / 10)
            } else {
                offset = frames[next].midY - viewport.midY
            }
            guard abs(offset) > 1 else { break }
            gesture += 1
            // A pan of at most 60% of the viewport preserves 40% overlap and
            // stays wholly inside gestureArea. Short 119pt pans in iPad run
            // 20261023T041457Z-97877 moved content only 35–50pt per gesture.
            let distance = min(abs(offset), viewport.height * 0.6) * (offset > 0 ? 1.0 : -1.0)
            #if os(macOS)
            scroll.scroll(byDeltaX: 0, deltaY: -distance)
            #else
            let gestureArea = viewport.insetBy(dx: viewport.width / 10, dy: viewport.height / 10)
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
                let proof = XCTAttachment(string: "Target: \(identifiers[next])\nOffset: \(offset), movement: \(movement)\nCoverage viewport before gesture: \(viewport)\nScroll frame before gesture: \(scrollFrame), after: \(scroll.frame)\nNative Done/Cancel frame before gesture: \(closeFrame), after: \(close.frame)\nBoundary uses the conservative 48pt inset, not the exact footer background frame.\n\(frameChanges)")
                proof.name = "export scroll no progress frames gesture \(gesture)"
                proof.lifetime = .keepAlways
                add(proof)
                throw AuditError(description: "No meaningful scoped scroll progress for \(identifiers[next]) after gesture \(gesture)")
            }
        }
        let missing = targets.indices.filter { !checked.contains($0) }.map { identifiers[$0] }
        throw AuditError(description: "Export content needs full frames or continuous beginning-to-end paragraph coverage, and actions must be hittable, after at most 48 scoped scroll gestures and 128 observations: \(missing)\n\(app.debugDescription)")
    }
}
