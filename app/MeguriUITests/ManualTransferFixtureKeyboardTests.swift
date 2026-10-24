import Foundation
import XCTest
#if os(macOS)
import AppKit
import ApplicationServices
#else
import UIKit
#endif

@MainActor final class ManualTransferFixtureKeyboardTests: XCTestCase {
    func testSyntheticMediaRenderEvidence() throws {
        try syntheticMediaRenderEvidence(
            states: ["media-consent", "media-progress", "media-partial", "media-complete", "media-unknown"],
            variants: AccessibilityAudit.Variant.all)
    }

    func testMediaCompleteAtLargestText() throws {
        #if os(macOS)
        throw XCTSkip("Mobile-only largest-text synthetic evidence")
        #else
        try syntheticMediaRenderEvidence(states: ["media-complete"],
            variants: [AccessibilityAudit.Variant(largestText: true, dark: false)])
        #endif
    }

    private func syntheticMediaRenderEvidence(states: [String], variants: [AccessibilityAudit.Variant]) throws {
        continueAfterFailure = false
        #if os(macOS)
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw XCTSkip("VM-only synthetic evidence") }
        #else
        let appearance = XCUIDevice.shared.appearance
        defer { XCUIDevice.shared.appearance = appearance }
        #endif
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("Requires development rendering hooks") }
        let bundle = try build.bundleIdentifier
        let disclosure = "Import and export both reconcile media in both directions. Server files are downloaded and pending local files are uploaded, even when the collection does not reference them. Counts become available after the media check."
        let unknown = "Current exclusions are unknown until a fresh media check completes. Recorded transfer counts retain saved effects from earlier attempts."
        for variant in variants {
            variant.apply()
            let variantName = (variant.dark ? "dark" : "light") + (variant.largestText ? "-ax5" : "-default")
            for state in states {
                let app = XCUIApplication()
                app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
                    route: "settings/anki-transfer/" + state, extra: variant.launchArguments).arguments
                app.launch()
                defer { app.terminate() }
                try app.bringToFront(bundleIdentifier: bundle)
                let marker = app.staticTexts["settings.anki.fixture"].firstMatch
                guard marker.waitForExistence(timeout: 10) else { throw AuditError(description: "Synthetic media marker missing") }
                try ManualTransferFixtureUI.guardFixture(app)
                guard try AccessibilityAudit.Appearance.shown(by: app).dark == variant.dark else {
                    throw AuditError(description: "Synthetic evidence appearance did not apply")
                }
                func capture(_ anchor: String, _ target: XCUIElement,
                             mode: ManualTransferFixtureUI.RevealMode = .textStart) throws {
                    try ManualTransferFixtureUI.reveal(target, in: app, bundle: bundle, mode: mode)
                    try ManualTransferFixtureUI.guardFixture(app)
                    _ = try ManualTransferFixtureUI.scrollSurface(app)
                    guard target.exists else { throw AuditError(description: "Synthetic screenshot anchor disappeared") }
                    let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
                    shot.name = "rf4-\(state)-\(variantName)-\(anchor).png"
                    shot.lifetime = .keepAlways
                    add(shot)
                }
                func text(_ fragment: String) -> XCUIElement { ManualTransferFixtureUI.text(fragment, in: app) }
                func button(_ id: String) -> XCUIElement { app.buttons["settings.anki." + id].firstMatch }
                switch state {
                case "media-consent":
                    try capture("disclosure-start", text(disclosure))
                    try capture("disclosure-end", text(disclosure), mode: .textEnd)
                    try capture("allow-start", button("startMedia"), mode: .actionable)
                    try capture("keep-later", button("cancelPreparedMedia"), mode: .actionable)
                case "media-progress":
                    try capture("receiving", text("Receiving media files…"))
                    try capture("active-cancel", button("cancel"), mode: .actionable)
                case "media-partial":
                    try capture("partial-result", text("Collection complete. The whole selected operation is incomplete."))
                    try capture("resume", button("resumeMedia.render-selected-media"), mode: .actionable)
                case "media-complete":
                    try capture("excluded-large-start", text("f-0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789.wav"))
                    try capture("excluded-large-end", text("f-0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789.wav"), mode: .textEnd)
                    try capture("excluded-large-reason", text("Excluded: over 100 MiB"))
                    try capture("excluded-empty-start", text("f-9876543210987654321098765432109876543210987654321098765432109876543210987654321098765432109876543210.png"))
                    try capture("excluded-empty-end", text("f-9876543210987654321098765432109876543210987654321098765432109876543210987654321098765432109876543210.png"), mode: .textEnd)
                    try capture("excluded-empty-reason", text("Excluded: empty file"))
                case "media-unknown":
                    try capture("unknown-counts-start", text(unknown))
                    try capture("unknown-counts-end", text(unknown), mode: .textEnd)
                    try capture("unacknowledged-effects", text("Additional server effects may have completed without a saved acknowledgement."))
                default: throw AuditError(description: "Unexpected synthetic evidence state")
                }
            }
        }
    }

    #if os(macOS)
    func testMacMediaDecisionCancelAndResumeChangeTheRenderedFixture() throws {
        continueAfterFailure = false
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw XCTSkip("VM-only keyboard evidence") }
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("Requires VM development hooks") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: "settings/anki-transfer/media-consent").arguments
        app.launch()
        defer { app.terminate() }
        XCTAssertTrue(app.staticTexts["settings.anki.fixture"].waitForExistence(timeout: 10))
        let bundle = try build.bundleIdentifier

        try activateWithSpace("settings.anki.startMedia", app: app, bundle: bundle)
        let progress = app.staticTexts["settings.anki.progress"].firstMatch
        try ManualTransferFixtureUI.reveal(progress, in: app, bundle: bundle, mode: .textStart)
        XCTAssertTrue(progress.exists)
        let receiving = ManualTransferFixtureUI.text("Receiving media files…", in: app)
        try ManualTransferFixtureUI.reveal(receiving, in: app, bundle: bundle, mode: .textStart)
        XCTAssertTrue(receiving.exists)
        XCTAssertFalse(app.buttons["settings.anki.startMedia"].exists, "Consent must be consumed by Start")

        try activateWithSpace("settings.anki.cancel", app: app, bundle: bundle)
        let result = app.staticTexts["settings.anki.result"].firstMatch
        try ManualTransferFixtureUI.reveal(result, in: app, bundle: bundle, mode: .textStart)
        XCTAssertTrue(ManualTransferFixtureUI.waitForText(result, containing: "Media stopped before completion"))
        XCTAssertFalse(app.buttons["settings.anki.cancel"].exists, "Cancel must drain the simulated operation")
        let cancelled = ManualTransferFixtureUI.text("Cancelled. The operation is partial; saved effects remain.", in: app)
        try ManualTransferFixtureUI.reveal(cancelled, in: app, bundle: bundle, mode: .textStart)
        XCTAssertTrue(cancelled.exists)

        try activateWithSpace("settings.anki.resumeMedia.render-selected-media", app: app, bundle: bundle)
        let start = app.buttons["settings.anki.startMedia"].firstMatch
        try ManualTransferFixtureUI.reveal(start, in: app, bundle: bundle)
        XCTAssertTrue(start.exists && start.isEnabled)
        XCTAssertFalse(app.staticTexts["settings.anki.progress"].exists, "Resume opens a fresh decision; it must not start media")
        let saved = ManualTransferFixtureUI.text("3200 files, 23 deletions, 234567890 file bytes recorded.", in: app)
        try ManualTransferFixtureUI.reveal(saved, in: app, bundle: bundle, mode: .textStart)
        XCTAssertTrue(saved.exists,
                      "Resume must preserve the simulated saved effects")
        try activateWithSpace("settings.anki.cancelPreparedMedia", app: app, bundle: bundle)
        try ManualTransferFixtureUI.reveal(result, in: app, bundle: bundle, mode: .textStart)
        XCTAssertTrue(ManualTransferFixtureUI.waitForText(result, containing: "Selected media remains for later"))
        XCTAssertFalse(app.buttons["settings.anki.startMedia"].exists)
        let resume = app.buttons["settings.anki.resumeMedia.render-selected-media"].firstMatch
        try ManualTransferFixtureUI.reveal(resume, in: app, bundle: bundle)
        XCTAssertTrue(resume.exists && resume.isEnabled)
        XCTAssertFalse(ManualTransferFixtureUI.text("The server acknowledged the collection.", in: app).exists)
    }

    private func activateWithSpace(_ identifier: String, app: XCUIApplication, bundle: String) throws {
        let button = app.buttons[identifier].firstMatch
        try ManualTransferFixtureUI.reveal(button, in: app, bundle: bundle)
        guard button.exists, button.isEnabled else { throw AuditError(description: "Revealed keyboard target is unavailable") }
        try app.bringToFront(bundleIdentifier: bundle)
        let running = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: bundle).first { !$0.isTerminated })
        let application = AXUIElementCreateApplication(running.processIdentifier)
        func read(_ node: AXUIElement, _ attribute: String) -> CFTypeRef? {
            var value: CFTypeRef?
            guard AXUIElementCopyAttributeValue(node, attribute as CFString, &value) == .success else { return nil }
            return value
        }
        func find() -> AXUIElement? {
            guard let windows = read(application, kAXWindowsAttribute) as? [AXUIElement] else { return nil }
            var pending = windows.map { ($0, 0) }, visited = 0
            while let (node, depth) = pending.popLast(), visited < 2048 {
                visited += 1
                if read(node, kAXIdentifierAttribute) as? String == identifier,
                   read(node, kAXRoleAttribute) as? String == kAXButtonRole { return node }
                if depth < 48, let children = read(node, kAXChildrenAttribute) as? [AXUIElement] {
                    pending.append(contentsOf: children.map { ($0, depth + 1) })
                }
            }
            return nil
        }
        let deadline = Date().addingTimeInterval(3)
        var found: AXUIElement?
        repeat {
            found = find()
            if found != nil { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        } while Date() < deadline
        let control = try XCTUnwrap(found, "Synthetic button unavailable: \(identifier); trusted=\(AXIsProcessTrusted())")
        let error = AXUIElementSetAttributeValue(control, kAXFocusedAttribute as CFString, kCFBooleanTrue)
        XCTAssertEqual(error, .success, "Synthetic button focus write: \(identifier)")
        let focusDeadline = Date().addingTimeInterval(2)
        while read(control, kAXFocusedAttribute) as? Bool != true && Date() < focusDeadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        XCTAssertEqual(read(control, kAXFocusedAttribute) as? Bool, true, "Synthetic button focus readiness: \(identifier)")
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(" ", modifierFlags: [])
    }
    #else
    func testIPadMediaKeyboardActivationRequiresSupportedKeyInjection() throws {
        guard UIDevice.current.userInterfaceIdiom == .pad else { throw XCTSkip("iPad-only keyboard evidence") }
        throw XCTSkip("Blocked: iOS XCTest has no supported arbitrary Tab/Space key injection or native button focus setter. typeKey(_:modifierFlags:) is macOS-only; typeText requires text input. No pointer action or AXPress is counted as iPad keyboard evidence.")
    }
    #endif
}

@MainActor enum ManualTransferFixtureUI {
    static func text(_ fragment: String, in app: XCUIApplication) -> XCUIElement {
        let predicate = fragment.allSatisfy(\.isNumber)
            ? NSPredicate(format: "label == %@ OR value == %@", fragment, fragment)
            : NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", fragment, fragment)
        return app.staticTexts.matching(predicate).firstMatch
    }
    static func waitForText(_ element: XCUIElement, containing text: String) -> Bool {
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            element.exists && (element.label.contains(text) || (element.value as? String)?.contains(text) == true)
        }, object: nil)
        return XCTWaiter.wait(for: [ready], timeout: 5) == .completed
    }
    static let routes: Set<String> = ["overflow", "media-consent", "media-progress", "media-partial", "media-complete", "media-unknown"]
        .reduce(into: Set<String>()) { $0.insert("settings/anki-transfer/" + $1) }

    static func guardFixture(_ app: XCUIApplication) throws {
        let arguments = app.launchArguments
        guard let routeIndex = arguments.firstIndex(of: "-MeguriRoute"), arguments.indices.contains(routeIndex + 1),
              routes.contains(arguments[routeIndex + 1]), app.state == .runningForeground,
              !app.sheets.firstMatch.exists, !app.alerts.firstMatch.exists,
              !app.secureTextFields.firstMatch.exists else {
            throw AuditError(description: "Expected foreground synthetic media route without sheets, alerts or secure fields")
        }
    }
    static func scrollSurface(_ app: XCUIApplication) throws -> XCUIElement {
        try guardFixture(app)
        let roots = app.descendants(matching: .any).matching(identifier: "settings.anki.content")
        let root = roots.firstMatch
        guard root.exists else {
            let rootExists = root.exists
            let window = app.windows.firstMatch
            let windowExists = window.exists
            let frame = windowExists ? window.frame : .zero
            throw AuditError(description:
                "queryCount=\(roots.count) rootExists=\(rootExists ? 1 : 0) " +
                "rootEnabled=\(rootExists ? (root.isEnabled ? 1 : 0) : -1) " +
                "rootType=\(rootExists ? Int(root.elementType.rawValue) : -1) " +
                "windowExists=\(windowExists ? 1 : 0) windowFrame=\(frame.origin.x),\(frame.origin.y),\(frame.width),\(frame.height) " +
                "fixtureExists=\(app.staticTexts["settings.anki.fixture"].firstMatch.exists ? 1 : 0) " +
                "appScrollViews=\(app.scrollViews.count) appCollectionViews=\(app.collectionViews.count) " +
                "rootScrollViews=\(rootExists ? root.scrollViews.count : -1) " +
                "rootCollectionViews=\(rootExists ? root.collectionViews.count : -1) " +
                "sheets=\(app.sheets.count) alerts=\(app.alerts.count)")
        }
        let window = app.windows.firstMatch
        guard window.exists else { throw AuditError(description: "E_RC_01") }
        if (root.elementType == .scrollView || root.elementType == .collectionView),
           valid(root.frame.intersection(window.frame)) { return root }
        for candidate in [root.scrollViews.firstMatch, root.collectionViews.firstMatch] where candidate.exists {
            if valid(candidate.frame.intersection(window.frame)) { return candidate }
        }
        #if os(macOS)
        if valid(root.frame.intersection(window.frame)) { return root }
        #endif
        throw AuditError(description: "E_RC_01")
    }
    private static func valid(_ rect: CGRect) -> Bool {
        !rect.isNull && !rect.isInfinite && !rect.isEmpty && rect.width.isFinite && rect.height.isFinite
    }
    static func viewport(_ surface: XCUIElement, app: XCUIApplication) throws -> CGRect {
        try guardFixture(app)
        let window = app.windows.firstMatch
        guard surface.exists, window.exists else { throw AuditError(description: "Synthetic viewport unavailable") }
        let windowFrame = window.frame
        let surfaceFrame = surface.frame
        let intersection = surfaceFrame.intersection(windowFrame)
        var content = intersection
        func geometry(_ name: String, _ frame: CGRect) -> String {
            let values = [frame.origin.x, frame.origin.y, frame.width, frame.height]
            return "\(name).finite=\(values.allSatisfy(\.isFinite)) " +
                "\(name).x=\(frame.origin.x.isFinite ? frame.origin.x : 0) " +
                "\(name).y=\(frame.origin.y.isFinite ? frame.origin.y : 0) " +
                "\(name).width=\(frame.width.isFinite ? frame.width : 0) " +
                "\(name).height=\(frame.height.isFinite ? frame.height : 0)"
        }
        var barGeometry = ""
        #if !os(macOS)
        var barCount = 0, ignoredCarrierCount = 0
        var bars: [String] = []
        var top = content.minY, bottom = content.maxY
        let types: [XCUIElement.ElementType] = [.navigationBar, .toolbar, .tabBar, .statusBar, .keyboard]
        for type in types {
            for element in app.descendants(matching: type).allElementsBoundByIndex {
                guard element.exists else { continue }
                let frame = element.frame
                let frameValid = valid(frame) && [frame.minX, frame.minY, frame.maxX, frame.maxY].allSatisfy(\.isFinite)
                let ignoredCarrier = type == .toolbar && frameValid && valid(windowFrame) &&
                    [windowFrame.minX, windowFrame.minY, windowFrame.maxX, windowFrame.maxY].allSatisfy(\.isFinite) &&
                    frame.contains(windowFrame)
                barCount += 1
                if ignoredCarrier { ignoredCarrierCount += 1 }
                if bars.count < 20 {
                    bars.append("bar.type=\(type.rawValue) bar.valid=\(frameValid) bar.ignoredCarrier=\(ignoredCarrier) " +
                        geometry("bar.frame", frame))
                }
                guard frameValid, !ignoredCarrier else { continue }
                let occlusion = frame.intersection(windowFrame).intersection(content)
                guard valid(occlusion) else { continue }
                if type != .keyboard && occlusion.midY < windowFrame.midY {
                    top = max(top, occlusion.maxY)
                } else {
                    bottom = min(bottom, occlusion.minY)
                }
            }
        }
        content = CGRect(x: content.minX, y: top, width: content.width, height: max(0, bottom - top))
        barGeometry = " barCount=\(barCount) ignoredCarrierCount=\(ignoredCarrierCount) recordedBarCount=\(bars.count) " +
            bars.joined(separator: " ")
        #endif
        let clipped = content.insetBy(dx: 4, dy: 8)
        guard valid(clipped), clipped.width > 24, clipped.height > 48 else {
            throw AuditError(description: "Synthetic viewport is empty " +
                geometry("window", windowFrame) + " " + geometry("surface", surfaceFrame) + " " +
                geometry("intersection", intersection) + " " + geometry("content", content) + " " +
                geometry("clipped", clipped) + barGeometry)
        }
        return clipped
    }
    static func scroll(_ surface: XCUIElement, down: Bool, app: XCUIApplication, bundle: String, step: CGFloat = 0.5) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        let visible = try viewport(surface, app: app)
        #if os(macOS)
        surface.scroll(byDeltaX: 0, deltaY: (down ? -1 : 1) * visible.height * step)
        #else
        let window = app.windows.firstMatch
        let origin = window.coordinate(withNormalizedOffset: .zero)
        let start = origin.withOffset(CGVector(dx: visible.midX - window.frame.minX,
                                               dy: visible.minY + visible.height * (down ? 0.5 + step / 2 : 0.5 - step / 2) - window.frame.minY))
        let end = origin.withOffset(CGVector(dx: visible.midX - window.frame.minX,
                                             dy: visible.minY + visible.height * (down ? 0.5 - step / 2 : 0.5 + step / 2) - window.frame.minY))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0)
        #endif
    }
    private struct VisibleContent: Equatable {
        let identity: String
        let frame: CGRect
    }
    private static func visibleContent(_ surface: XCUIElement, app: XCUIApplication) throws -> [VisibleContent] {
        let visible = try viewport(surface, app: app)
        let root = app.descendants(matching: .any).matching(identifier: "settings.anki.content").firstMatch
        guard root.exists else { throw AuditError(description: "Synthetic Settings Form unavailable") }
        let content = root.descendants(matching: .any).matching(NSPredicate(format: "elementType == %d OR elementType == %d",
            XCUIElement.ElementType.staticText.rawValue, XCUIElement.ElementType.button.rawValue))
        var result: [VisibleContent] = []
        for element in content.allElementsBoundByIndex {
            guard element.exists else { continue }
            let frame = element.frame
            guard valid(frame), valid(frame.intersection(visible)) else { continue }
            let identifier = element.identifier
            let identity = !identifier.isEmpty && identifier != "settings.anki.content"
                ? "id:" + identifier : "synthetic-label:" + element.label
            result.append(VisibleContent(identity: "\(element.elementType.rawValue):" + identity, frame: frame))
        }
        return result.sorted {
            if $0.identity != $1.identity { return $0.identity < $1.identity }
            if $0.frame.minY != $1.frame.minY { return $0.frame.minY < $1.frame.minY }
            if $0.frame.minX != $1.frame.minX { return $0.frame.minX < $1.frame.minX }
            if $0.frame.height != $1.frame.height { return $0.frame.height < $1.frame.height }
            return $0.frame.width < $1.frame.width
        }
    }
    enum RevealMode: Equatable { case actionable, visibleDisabled, textStart, textEnd }
    static func reveal(_ target: XCUIElement, in app: XCUIApplication, bundle: String,
                       mode: RevealMode = .actionable) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        let surface = try scrollSurface(app)
        var discoveryDown = true
        for gesture in 0...24 {
            let visible = try viewport(surface, app: app)
            var down = discoveryDown
            var step: CGFloat = 0.8
            let targetExists = target.exists
            if targetExists {
                step = 0.5
                let frame = target.frame
                if valid(frame) {
                    let fits: Bool
                    switch mode {
                    case .textStart, .textEnd:
                        if frame.height <= visible.height {
                            fits = visible.contains(frame)
                        } else if mode == .textStart {
                            fits = visible.intersects(frame) && frame.minY >= visible.minY && frame.minY < visible.maxY - 44
                        } else {
                            fits = visible.intersects(frame) && frame.maxY <= visible.maxY && frame.maxY > visible.minY + 44
                        }
                    case .actionable, .visibleDisabled: fits = visible.contains(frame)
                    }
                    if fits {
                        switch mode {
                        case .actionable: if target.isEnabled && target.isHittable { return }
                        case .visibleDisabled: if !target.isEnabled { return }
                        case .textStart, .textEnd: return
                        }
                    }
                    switch mode {
                    case .textStart: down = frame.minY > visible.minY
                    case .textEnd: down = frame.maxY > visible.maxY
                    case .actionable, .visibleDisabled: down = frame.midY >= visible.midY
                    }
                }
            }
            guard gesture < 24 else { break }
            let before = targetExists ? [] : try visibleContent(surface, app: app)
            try scroll(surface, down: down, app: app, bundle: bundle, step: step)
            if !targetExists, !before.isEmpty {
                var unchanged = true
                for _ in 0..<4 {
                    RunLoop.current.run(until: Date().addingTimeInterval(0.05))
                    if try visibleContent(surface, app: app) != before {
                        unchanged = false
                        break
                    }
                }
                if unchanged { discoveryDown.toggle() }
            }
        }
        attachRevealBoundary(target, surface: surface, app: app, mode: mode, budget: 24)
        throw AuditError(description: "Synthetic target did not become visible within 24 scoped gestures")
    }

    private static func attachRevealBoundary(_ target: XCUIElement, surface: XCUIElement,
                                             app: XCUIApplication, mode: RevealMode, budget: Int) {
        let targetExists = target.exists
        let surfaceExists = surface.exists
        let window = app.windows.firstMatch
        let windowExists = window.exists
        let modeName: String
        switch mode {
        case .actionable: modeName = "actionable"
        case .visibleDisabled: modeName = "visibleDisabled"
        case .textStart: modeName = "textStart"
        case .textEnd: modeName = "textEnd"
        }
        func geometry(_ name: String, _ frame: CGRect) -> String {
            "\(name).x=\(frame.origin.x) \(name).y=\(frame.origin.y) " +
            "\(name).width=\(frame.width) \(name).height=\(frame.height)"
        }
        var context = ["targetExists=\(targetExists)", "surfaceExists=\(surfaceExists)",
                       "windowExists=\(windowExists)", "mode=\(modeName)", "budget=\(budget)"]
        if targetExists { context.append(geometry("targetFrame", target.frame)) }
        let surfaceFrame = surfaceExists ? surface.frame : nil
        let windowFrame = windowExists ? window.frame : nil
        if let surfaceFrame { context.append(geometry("surfaceFrame", surfaceFrame)) }
        if let windowFrame { context.append(geometry("windowFrame", windowFrame)) }
        if let surfaceFrame, let windowFrame {
            context.append(geometry("clippedViewport", surfaceFrame.intersection(windowFrame).insetBy(dx: 4, dy: 8)))
        }
        XCTContext.runActivity(named: "rf4-fixture-reveal-boundary") { activity in
            let diagnostic = XCTAttachment(string: context.joined(separator: "\n"))
            diagnostic.name = "rf4-fixture-reveal-geometry"
            diagnostic.lifetime = .keepAlways
            activity.add(diagnostic)
            do {
                try guardFixture(app)
                _ = try scrollSurface(app)
            } catch { return }
            let shot = XCTAttachment(screenshot: window.screenshot())
            shot.name = "rf4-fixture-reveal-boundary"
            shot.lifetime = .keepAlways
            activity.add(shot)
        }
    }
}
