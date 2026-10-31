import Foundation
import XCTest
#if os(macOS)
import AppKit
#else
import UIKit
import notify
#endif

/// tools/mg ui-test --platform iphone|ipad|mac (test VM). These drive
/// real controls and production pages/presenters; no host input or kill SPI.
@MainActor
final class CardWebViewBehaviorUITests: XCTestCase {
    private var bundleIdentifier = ""
    override func setUp() {
        super.setUp()
        continueAfterFailure = false
    }
    private func launch(route: String = "card/behaviors") throws -> XCUIApplication {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("this configuration has no fixture routes") }
        bundleIdentifier = try build.bundleIdentifier
        #if os(macOS)
        guard bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("daily-driver Mac products cannot be launched by a test") }
        #endif
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: route).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        guard wait({ self.state(app).contains("ready card=1 side=question") }) else {
            throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Card fixture did not become ready within 10 seconds: \(state(app))"])
        }
        return app
    }
    private func state(_ app: XCUIApplication) -> String {
        let field = app.descendants(matching: .any).matching(identifier: "card.behavior.state").firstMatch
        return field.value as? String ?? field.label
    }
    private func report(_ app: XCUIApplication) throws -> [String: Any] {
        let text = state(app)
        let start = try XCTUnwrap(text.firstIndex(of: "{"), text)
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(text[start...].utf8)) as? [String: Any])
    }
    private func assertSingleActiveCardRoot(_ app: XCUIApplication, file: StaticString = #filePath, line: UInt = #line) {
        let a = app.descendants(matching: .any).matching(identifier: "review.card.page.a")
        let b = app.descendants(matching: .any).matching(identifier: "review.card.page.b")
        XCTAssertEqual(a.count + b.count, 1, "exactly one named page root must be exported", file: file, line: line)
        XCTAssertNotEqual(a.firstMatch.exists, b.firstMatch.exists,
                          "the idle root must be absent", file: file, line: line)
    }
    private func click(_ element: XCUIElement, in app: XCUIApplication) throws {
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        #endif
        element.clickOrTap()
    }
    private func control(_ id: String, in app: XCUIApplication) throws {
        #if os(iOS)
        if Self.sheetActionIDs.contains(id) {
            let list = try openActions(in: app)
            let button = list.buttons["card.behavior.\(id)"]
            var previous: [String]?
            for _ in 0..<40 {
                if fullyVisible(button, in: visibleListBounds(list, in: app)) {
                    try click(button, in: app)
                    guard wait({ !list.exists }) else {
                        throw AuditError(description: "Card actions did not dismiss after selecting \(id)")
                    }
                    return
                }
                let position = actionPositions(in: list, bounds: visibleListBounds(list, in: app))
                guard position != previous else { break }
                previous = position
                scrollActions(list, in: app)
            }
            throw AuditError(description: "Native Card actions row \(id) never became fully visible and hittable")
        }
        #endif
        let button = app.descendants(matching: .any).matching(identifier: "card.behavior.\(id)").firstMatch
        if !button.exists || !button.isHittable {
            try click(app.descendants(matching: .any).matching(identifier: "card.behavior.actions").firstMatch, in: app)
        }
        XCTAssert(button.waitForExistence(timeout: 3), "missing control \(id)")
        try click(button, in: app)
    }

    #if os(iOS)
    private static let sheetActionIDs = [
        "confirm", "prompt", "separator", "fallback", "graphic", "zoom", "resize",
        "refresh", "suspend", "resume", "probe", "recover", "focus",
    ]

    private func openActions(in app: XCUIApplication) throws -> XCUIElement {
        let list = app.descendants(matching: .any).matching(identifier: "card.behavior.actions.list").firstMatch
        if !list.exists {
            try click(app.buttons["card.behavior.actions"], in: app)
        }
        guard list.waitForExistence(timeout: 5) else {
            throw AuditError(description: "Native Card actions List did not appear")
        }
        return list
    }

    private func fullyVisible(_ element: XCUIElement, in bounds: CGRect) -> Bool {
        guard element.exists else { return false }
        let frame = element.frame
        return !frame.isEmpty && [frame.minX, frame.minY, frame.width, frame.height].allSatisfy(\.isFinite)
            && !bounds.isEmpty && bounds.contains(frame) && element.isHittable
    }

    private func visibleListBounds(_ list: XCUIElement, in app: XCUIApplication) -> CGRect {
        var bounds = list.frame.intersection(app.windows.firstMatch.frame)
        // List content may extend beneath the native navigation bar.
        for bar in app.navigationBars.allElementsBoundByIndex where bar.exists && bar.frame.intersects(bounds) {
            let top = max(bounds.minY, bar.frame.maxY)
            bounds = CGRect(x: bounds.minX, y: top, width: bounds.width, height: max(0, bounds.maxY - top))
        }
        return bounds
    }

    private func actionPositions(in list: XCUIElement, bounds: CGRect) -> [String] {
        Self.sheetActionIDs.compactMap { id in
            let row = list.buttons["card.behavior.\(id)"]
            guard row.exists else { return nil }
            let frame = row.frame
            guard !frame.isEmpty, frame.intersects(bounds),
                  [frame.minX, frame.minY, frame.width, frame.height].allSatisfy(\.isFinite) else { return nil }
            return "\(id):\(frame.minY.rounded()):\(frame.height.rounded())"
        }
    }

    private func scrollActions(_ list: XCUIElement, in app: XCUIApplication) {
        let bounds = visibleListBounds(list, in: app)
        guard !bounds.isEmpty else { return }
        let origin = list.coordinate(withNormalizedOffset: .zero)
        let start = origin.withOffset(CGVector(dx: bounds.midX - list.frame.minX,
                                               dy: bounds.midY - list.frame.minY))
        let end = start.withOffset(CGVector(dx: 0, dy: -bounds.height * 0.3))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .fast, thenHoldForDuration: 0)
    }

    private func auditActions(in app: XCUIApplication, variant: String, suppressions: [AuditSuppression]) throws -> AccessibilityAudit.StateResult {
        func capture(_ label: String) {
            let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
            shot.name = "card-behavior \(variant) \(label)"
            shot.lifetime = .keepAlways
            add(shot)
        }
        capture("main")
        let list = try openActions(in: app)
        func recordGeometry(_ phase: String, viewport: Int) {
            let bounds = visibleListBounds(list, in: app)
            let signature = actionPositions(in: list, bounds: bounds)
            let refreshFrame = list.buttons["card.behavior.refresh"].frame
            print("card-behavior audit geometry variant=\(variant) viewport=\(viewport) phase=\(phase) actionPositions=\(signature) visibleListBounds=\(bounds) card.behavior.refresh.frame=\(refreshFrame)")
        }
        let close = app.buttons["card.behavior.actions.close"]
        defer {
            if close.exists { close.tap() }
            XCTAssert(wait { !list.exists }, "Card actions did not close after audit (\(variant))")
        }
        var covered = Set<String>()
        let required = Set(Self.sheetActionIDs + ["actions.close"])
        var positions = Set<[String]>()
        var findings: [AuditFinding] = []
        var targets: [TouchTargetViolation] = []
        for position in 0..<40 {
            let bounds = visibleListBounds(list, in: app)
            let signature = actionPositions(in: list, bounds: bounds)
            guard positions.insert(signature).inserted else { break }
            let label = "question sheet, \(variant), viewport \(position + 1)"
            // Count only complete, hittable rows inside this native viewport.
            let visible = Self.sheetActionIDs.filter {
                fullyVisible(list.buttons["card.behavior.\($0)"], in: bounds)
            }
            let closeVisible = fullyVisible(close, in: app.windows.firstMatch.frame)
            capture("sheet viewport \(position + 1)")
            do {
                defer { recordGeometry("after", viewport: position + 1) }
                recordGeometry("before", viewport: position + 1)
                findings += try AccessibilityAudit.run(app, variant: label, suppressions: suppressions)
            }
            targets += try TouchTargets.violations(in: app).map {
                TouchTargetViolation(identifier: "\(label): \($0.identifier)", label: $0.label,
                                     size: $0.size, minimum: $0.minimum)
            }
            covered.formUnion(visible)
            if closeVisible { covered.insert("actions.close") }
            if covered == required { break }
            scrollActions(list, in: app)
        }
        guard covered == required else {
            throw AuditError(description: "Card actions audit (\(variant)) never audited fully visible, hittable controls: \(required.subtracting(covered).sorted().joined(separator: ", "))")
        }
        return AccessibilityAudit.StateResult(state: "question sheet, \(variant)", findings: findings, touchTargets: targets)
    }
    #endif

    private func wait(_ condition: @escaping () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(10)
        while !condition() {
            guard Date() < deadline else { return false }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return true
    }

    #if os(macOS)
    private func startMacVoiceOverInSettings() throws {
        let settings = XCUIApplication(bundleIdentifier: "com.apple.systempreferences")
        settings.launch()
        defer { settings.terminate() }
        settings.activate()
        func setupError(_ message: String) -> NSError {
            NSError(domain: "CardWebViewBehaviorUITests.VoiceOverSetup", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: message])
        }
        let switches = settings.switches.matching(identifier: "AX_VOICEOVER_ENABLED")
        if switches.count == 0 {
            for identifier in ["com.apple.settings.accessibility", "AX_FEATURE_VOICEOVER"] {
                let buttons = settings.buttons.matching(identifier: identifier)
                guard buttons.firstMatch.waitForExistence(timeout: 10), buttons.count == 1 else {
                    throw setupError("Expected one Settings button: \(identifier)")
                }
                buttons.element.click()
            }
        }
        guard switches.firstMatch.waitForExistence(timeout: 10), switches.count == 1 else {
            throw setupError("Expected one Settings VoiceOver switch")
        }
        let enabled: Bool
        switch switches.element.value {
        case let value as NSNumber where value == 0 || value == 1:
            enabled = value == 1
        case let value as String where value == "0" || value == "1":
            enabled = value == "1"
        default:
            throw setupError("Settings VoiceOver switch has an unknown value")
        }
        if !enabled { switches.element.click() }
        guard wait({ NSWorkspace.shared.isVoiceOverEnabled }) else {
            throw setupError("Settings did not enable OS VoiceOver within 10 seconds")
        }
    }
    #endif

    func testNativeConfirmAndAcceptedEmptyPrompt() throws {
        let app = try launch()
        try control("confirm", in: app)
        #if os(macOS)
        let confirmSheet = app.sheets.firstMatch
        XCTAssert(confirmSheet.waitForExistence(timeout: 5))
        let confirm = confirmSheet.buttons["OK"]
        #else
        let confirm = app.buttons["OK"]
        #endif
        XCTAssert(confirm.waitForExistence(timeout: 5))
        try click(confirm, in: app)
        XCTAssert(wait { self.state(app).contains("dialog=true") }, state(app))
        try control("prompt", in: app)
        #if os(macOS)
        let promptSheet = app.sheets.firstMatch
        XCTAssert(promptSheet.waitForExistence(timeout: 5))
        let prompt = promptSheet.buttons["OK"]
        #else
        let prompt = app.buttons["OK"]
        #endif
        XCTAssert(prompt.waitForExistence(timeout: 5))
        try click(prompt, in: app)
        XCTAssert(wait { self.state(app).contains("dialog=empty") }, state(app))
        try control("prompt", in: app)
        #if os(macOS)
        let cancelSheet = app.sheets.firstMatch
        XCTAssert(cancelSheet.waitForExistence(timeout: 5))
        let cancel = cancelSheet.buttons["Cancel"]
        #else
        let cancel = app.buttons["Cancel"]
        #endif
        XCTAssert(cancel.waitForExistence(timeout: 5))
        try click(cancel, in: app)
        XCTAssert(wait { self.state(app).contains("dialog=cancelled") }, state(app))
        XCTAssert(state(app).contains("side=question"), state(app))
    }

    func testResizeKeyboardTypedTextAndForegroundKeepDocumentThenCallbackRecoversIt() throws {
        let app = try launch()
        let initial = try report(app)
        let web = app.webViews.firstMatch
        XCTAssert(web.waitForExistence(timeout: 5))
        try control("focus", in: app)
        let input = web.textFields["Typed answer"]
        XCTAssert(input.waitForExistence(timeout: 5))
        try click(input, in: app)
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        #endif
        input.typeText("ka")
        XCTAssert(wait { self.state(app).contains("\"text\":\"ka\"") }, state(app))
        #if !os(macOS)
        notify_post("\(bundleIdentifier).testhooks.toggleSizeClass")
        try control("refresh", in: app)
        XCTAssertEqual(try report(app)["marker"] as? Double, initial["marker"] as? Double)
        notify_post("\(bundleIdentifier).testhooks.toggleSizeClass")
        #endif
        try control("resize", in: app)
        try control("refresh", in: app)
        XCTAssert(wait { ((try? self.report(app)["resizes"] as? Int) ?? 0) > 0 }, state(app))
        let resized = try report(app)
        XCTAssertEqual(resized["marker"] as? Double, initial["marker"] as? Double)
        XCTAssertEqual(resized["text"] as? String, "ka")
        // A real native editor owns the keyboard across a healthy probe.
        let editor = app.textFields["card.behavior.editor"]
        try click(editor, in: app)
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        #endif
        editor.typeText("native")
        try control("probe", in: app)
        XCTAssert(wait { self.state(app).contains("probe=1") }, state(app))
        XCTAssertEqual(try report(app)["marker"] as? Double, initial["marker"] as? Double)
        try control("recover", in: app) // Controlled public callback, not a process kill.
        let recovered = wait {
            guard let current = try? self.report(app) else { return false }
            return current["text"] as? String == "ka" && current["marker"] as? Double != initial["marker"] as? Double
        }
        if !recovered {
            // One diagnostic read after the original deadline; never a recovery retry.
            let before = state(app).components(separatedBy: " | ").first ?? "unavailable"
            var refreshResult = "requested"
            do { try control("refresh", in: app) }
            catch { refreshResult = "control-unavailable" }
            let after = state(app).components(separatedBy: " | ").first ?? "unavailable"
            let diagnostic = XCTAttachment(string: "before refresh: \(before)\nafter refresh (\(refreshResult)): \(after)")
            diagnostic.name = "recovery-deadline-diagnostics"
            diagnostic.lifetime = .keepAlways
            add(diagnostic)
        }
        XCTAssert(recovered, state(app))
        XCTAssertEqual(editor.value as? String, "native")
        try click(editor, in: app)
        app.typeText("-before")
        XCTAssertEqual(editor.value as? String, "native-before")
        try control("reveal", in: app)
        XCTAssert(wait { self.state(app).contains("side=answer") && self.state(app).contains("target=answer") }, state(app))
        XCTAssert(app.staticTexts["Behavior answer"].waitForExistence(timeout: 5))
        assertSingleActiveCardRoot(app)
        let pageReport = try report(app)
        XCTAssertEqual(pageReport["pageCount"] as? Int, 2)
        XCTAssertEqual(pageReport["distinctPages"] as? Bool, true)
        XCTAssertEqual(pageReport["currentHidden"] as? Bool, false)
        XCTAssertEqual(pageReport["idleHidden"] as? Bool, true)
        app.typeText("-after")
        XCTAssertEqual(editor.value as? String, "native-before-after")
    }

    func testNativeZoomFrameAndSameDocumentLinks() throws {
        let app = try launch()
        let before = try report(app)
        let beforeData = try JSONSerialization.data(withJSONObject: ["report": before], options: [.sortedKeys])
        let beforeAttachment = XCTAttachment(data: beforeData, uniformTypeIdentifier: "public.json")
        beforeAttachment.name = "same-document-link-before-ax-query"
        beforeAttachment.lifetime = .keepAlways
        add(beforeAttachment)
        let web = app.webViews.firstMatch
        let height = try XCTUnwrap(before["height"] as? Double)
        XCTAssertEqual(try XCTUnwrap(before["viewport"] as? Double), height, accuracy: 1)
        XCTAssertEqual(try XCTUnwrap(before["bottom"] as? Double), try XCTUnwrap(before["layoutHeight"] as? Double), accuracy: 1)
        #if os(macOS)
        let native = try XCTUnwrap(before["native"] as? [String: Any])
        func rect(_ key: String) throws -> CGRect {
            let values = try XCTUnwrap(native[key] as? [String: Double])
            return CGRect(x: try XCTUnwrap(values["x"]), y: try XCTUnwrap(values["y"]),
                          width: try XCTUnwrap(values["width"]), height: try XCTUnwrap(values["height"]))
        }
        let nativeFrame = try rect("frame")
        for bounds in [try rect("contentBounds"), try rect("safeContentLayoutRect")] {
            XCTAssertGreaterThanOrEqual(nativeFrame.minX, bounds.minX)
            XCTAssertGreaterThanOrEqual(nativeFrame.minY, bounds.minY)
            XCTAssertLessThanOrEqual(nativeFrame.maxX, bounds.maxX)
            XCTAssertLessThanOrEqual(nativeFrame.maxY, bounds.maxY)
        }
        XCTAssertEqual(Double(nativeFrame.height), height, accuracy: 1)
        #else
        XCTAssertGreaterThan(web.frame.minY, app.windows.firstMatch.frame.minY)
        XCTAssertLessThanOrEqual(web.frame.maxY, app.windows.firstMatch.frame.maxY)
        let native = try XCTUnwrap(before["native"] as? [String: Double])
        let top = try XCTUnwrap(native["y"]), nativeHeight = try XCTUnwrap(native["height"])
        let left = try XCTUnwrap(native["x"]), nativeWidth = try XCTUnwrap(native["width"])
        let windowHeight = try XCTUnwrap(native["windowHeight"]), bottom = try XCTUnwrap(native["safeBottom"])
        let windowWidth = try XCTUnwrap(native["windowWidth"]), right = try XCTUnwrap(native["safeRight"])
        XCTAssertGreaterThanOrEqual(top, try XCTUnwrap(native["safeTop"]))
        XCTAssertLessThanOrEqual(top + nativeHeight, windowHeight - bottom)
        XCTAssertGreaterThanOrEqual(left, try XCTUnwrap(native["safeLeft"]))
        XCTAssertLessThanOrEqual(left + nativeWidth, windowWidth - right)
        XCTAssertEqual(nativeHeight, height, accuracy: 1)
        #endif
        let script = web.links["Run card script"]
        let scriptExists = script.exists
        var scriptDiagnostic: [String: Any] = ["exists": scriptExists, "isHittable": script.isHittable, "frame": NSNull()]
        if scriptExists {
            let frame = script.frame
            if [frame.minX, frame.minY, frame.width, frame.height].allSatisfy(\.isFinite) {
                scriptDiagnostic["frame"] = ["x": frame.minX, "y": frame.minY, "width": frame.width, "height": frame.height]
            } else {
                scriptDiagnostic["status"] = "invalidFrame"
            }
        }
        let linkDiagnostic: [String: Any] = ["state": state(app), "report": (try? report(app)) ?? [:],
                                           "script": scriptDiagnostic]
        let diagnosticData = try JSONSerialization.data(withJSONObject: linkDiagnostic, options: [.sortedKeys])
        let attachment = XCTAttachment(data: diagnosticData, uniformTypeIdentifier: "public.json")
        attachment.name = "same-document-link-before-native-click"
        attachment.lifetime = .keepAlways
        add(attachment)
        try click(script, in: app)
        try control("refresh", in: app)
        XCTAssertEqual(try report(app)["scriptRan"] as? Bool, true)
        XCTAssertEqual(try report(app)["root"] as? String, "meguri-media://media/")
        try control("zoom", in: app)
        XCTAssert(wait { self.state(app).contains("zoom=150") }, state(app))
        XCTAssertEqual(try report(app)["marker"] as? Double, before["marker"] as? Double)
        try control("next", in: app)
        XCTAssert(wait { self.state(app).contains("card=2 side=question") }, state(app))
    }

    #if os(macOS) || (os(iOS) && targetEnvironment(simulator))
    /// tools/mg ui-test on Mac/iPhone/iPad in the VM only. The public service
    /// reads the current OS cursor; forward traversal proves its destination.
    func testVoiceOverAnswerTargetWithExternalKeyboardOwner() throws {
        try assertVoiceOverAnswerTargetWithExternalKeyboardOwner(route: "card/behaviors", layoutIndex: 0)
    }

    func testVoiceOverSeparatorAnswerTargetWithExternalKeyboardOwner() throws {
        try assertVoiceOverAnswerTargetWithExternalKeyboardOwner(route: "card/behaviors/separator", layoutIndex: 1)
    }

    func testVoiceOverFallbackAnswerTargetWithExternalKeyboardOwner() throws {
        try assertVoiceOverAnswerTargetWithExternalKeyboardOwner(route: "card/behaviors/fallback", layoutIndex: 2)
    }

    func testVoiceOverLabelledGraphicAnswerTargetWithExternalKeyboardOwner() throws {
        try assertVoiceOverAnswerTargetWithExternalKeyboardOwner(route: "card/behaviors/graphic", layoutIndex: 3)
    }

    @MainActor
    private func assertVoiceOverAnswerTargetWithExternalKeyboardOwner(route: String, layoutIndex: Int) throws {
        let app = try launch(route: route)
        try control("zoom", in: app)
        XCTAssert(wait { self.state(app).contains("zoom=150") }, state(app))
        let baselineReport = try report(app)
        let savedZoom = try XCTUnwrap(baselineReport["pageZoom"] as? Double)
        XCTAssertEqual(savedZoom, 1.5)
        let unpinchedVisualScale = try XCTUnwrap(baselineReport["visualScale"] as? Double)
        XCTAssertTrue(unpinchedVisualScale.isFinite)
        XCTAssertGreaterThan(unpinchedVisualScale, 0)
        #if os(iOS)
        let unpinchedNativeZoom = try XCTUnwrap(baselineReport["nativeZoom"] as? Double)
        XCTAssertTrue(unpinchedNativeZoom.isFinite)
        XCTAssertGreaterThan(unpinchedNativeZoom, 0)
        // Exercise touch zoom before enabling the screen reader.
        let cardSurface = app.webViews.firstMatch
        cardSurface.pinch(withScale: 2, velocity: 1)
        try control("refresh", in: app)
        let gestureReport = try report(app)
        let questionGestureScale = try XCTUnwrap(gestureReport["visualScale"] as? Double)
        let questionNativeZoom = try XCTUnwrap(gestureReport["nativeZoom"] as? Double)
        XCTAssertTrue(questionGestureScale.isFinite && questionNativeZoom.isFinite)
        XCTAssertGreaterThan(questionGestureScale, unpinchedVisualScale)
        XCTAssertGreaterThan(questionNativeZoom, unpinchedNativeZoom)
        XCTAssertEqual(try XCTUnwrap(gestureReport["pageZoom"] as? Double), savedZoom)
        #endif
        let voiceOver = XCUIDevice.shared.voiceOverService
        addTeardownBlock { @MainActor in try? XCUIDevice.shared.voiceOverService.disable() }
        #if os(macOS)
        try startMacVoiceOverInSettings()
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        #endif
        try voiceOver.enable()
        @MainActor func attachSpeech(_ utterance: String, name: String) {
            print("\(name): \(utterance)")
            let attachment = XCTAttachment(string: utterance)
            attachment.name = name
            attachment.lifetime = .keepAlways
            add(attachment)
        }
        @MainActor func attachRevealedScreen(_ layoutIndex: Int) {
            let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            screenshot.name = "layout-\(layoutIndex)-revealed-screen"
            screenshot.lifetime = .keepAlways
            add(screenshot)
            let hierarchy = XCTAttachment(string: app.debugDescription)
            hierarchy.name = "layout-\(layoutIndex)-revealed-hierarchy"
            hierarchy.lifetime = .keepAlways
            add(hierarchy)
        }
        @MainActor func activateWithVoiceOver(_ element: XCUIElement) throws {
            guard element.waitForExistence(timeout: 3) else {
                throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 2,
                              userInfo: [NSLocalizedDescriptionKey: "missing native VO control: \(element)"])
            }
            #if os(macOS)
            try click(element, in: app)
            #else
            let label = element.label
            guard !label.isEmpty else {
                throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 3,
                              userInfo: [NSLocalizedDescriptionKey: "native VO control has an empty label: \(element)"])
            }
            // With VoiceOverTouch, one tap selects. Verify the actual native
            // focused element and service speech before the activation gesture.
            element.tap()
            guard wait({ (try? self.report(app)["voiceFocus"] as? String) == label }) else {
                throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 4,
                              userInfo: [NSLocalizedDescriptionKey: "VoiceOver did not focus native control \(label) within 10 seconds: \(state(app))"])
            }
            guard wait({ (try? voiceOver.currentSpeech().utterance.contains(label)) == true }) else {
                throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 5,
                              userInfo: [NSLocalizedDescriptionKey: "VoiceOver must speak the native control before activation within 10 seconds: \(label)"])
            }
            element.doubleTap()
            #endif
        }
        let nativeEditor = app.textFields["card.behavior.editor"]
        try activateWithVoiceOver(nativeEditor)
        nativeEditor.typeText("v\(layoutIndex)")
        let retainedText = try XCTUnwrap(nativeEditor.value as? String)
        attachSpeech(try voiceOver.currentSpeech().utterance, name: "layout-\(layoutIndex)-before-reveal")
        // This unique visible control avoids transient Menu ownership.
        try activateWithVoiceOver(app.buttons["card.behavior.reveal"])
        XCTAssert(wait { self.state(app).contains("side=answer") }, state(app))
        attachSpeech(try voiceOver.currentSpeech().utterance, name: "layout-\(layoutIndex)-on-revealed")
        let answerReport = try report(app)
        XCTAssertEqual(try XCTUnwrap(answerReport["pageZoom"] as? Double), savedZoom)
        #if os(iOS)
        XCTAssertEqual(try XCTUnwrap(answerReport["visualScale"] as? Double),
                       questionGestureScale, accuracy: 0.01)
        XCTAssertEqual(try XCTUnwrap(answerReport["nativeZoom"] as? Double),
                       questionNativeZoom, accuracy: 0.01)
        #endif
        XCTAssert(wait { (try? voiceOver.currentSpeech().utterance.contains("Behavior answer")) == true },
                  "The current native VoiceOver cursor must identify the answer")
        attachSpeech(try voiceOver.currentSpeech().utterance, name: "layout-\(layoutIndex)-answer-cursor")
        attachRevealedScreen(layoutIndex)
        // Verify automatic reveal ownership BEFORE deliberate VO traversal
        // to the card's input could change keyboard focus.
        app.typeText("x")
        XCTAssertEqual(nativeEditor.value as? String, retainedText + "x")
        let nextSpeech = try voiceOver.moveForward().utterance
        attachSpeech(nextSpeech, name: "layout-\(layoutIndex)-next1")
        XCTAssertTrue(nextSpeech.contains("Typed answer"), "The next VO element must follow the answer destination")
        if layoutIndex == 1 && !nextSpeech.contains("Typed answer") {
            attachSpeech(try voiceOver.moveForward().utterance, name: "layout-\(layoutIndex)-next2")
        }
        assertSingleActiveCardRoot(app)
        XCTAssertEqual(answerReport["pageCount"] as? Int, 2)
        XCTAssertEqual(answerReport["distinctPages"] as? Bool, true)
        XCTAssertEqual(answerReport["currentHidden"] as? Bool, false)
        XCTAssertEqual(answerReport["idleHidden"] as? Bool, true)
        XCTAssertFalse(app.staticTexts["Behavior question"].exists, "idle question content must stay excluded")
        // Exercise the actual visible native action with VoiceOver still enabled.
        try activateWithVoiceOver(app.buttons["card.behavior.next"])
        XCTAssert(wait { self.state(app).contains("card=2 side=question") }, state(app))
        let nextQuestionReport = try report(app)
        XCTAssertEqual(try XCTUnwrap(nextQuestionReport["pageZoom"] as? Double), savedZoom)
        XCTAssertEqual(try XCTUnwrap(nextQuestionReport["visualScale"] as? Double), unpinchedVisualScale, accuracy: 0.01)
        #if os(iOS)
        XCTAssertEqual(try XCTUnwrap(nextQuestionReport["nativeZoom"] as? Double), unpinchedNativeZoom, accuracy: 0.01)
        #endif
    }
    #endif

    func testCardSurfaceAccessibilityAudit() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("this configuration has no fixture routes") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("daily-driver Mac products cannot be launched by a test") }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let supplemental: (@MainActor (XCUIApplication, String) throws -> AccessibilityAudit.StateResult)?
        #if os(iOS)
        supplemental = { app, variant in
            try self.auditActions(in: app, variant: variant, suppressions: suppressions)
        }
        #else
        supplemental = nil
        #endif
        let result = try AccessibilityAudit.runAllVariants(
            state: "question", launch: MeguriLaunch(locale: "en_US", animations: false, route: "card/behaviors"),
            waitFor: "card.behavior.state", suppressions: suppressions, ready: { app in
                var currentState = ""
                var questionExists = false
                let completed = self.wait {
                    currentState = self.state(app)
                    if currentState.hasPrefix("failed") { return true }
                    questionExists = app.staticTexts["Behavior question"].exists
                    return currentState.hasPrefix("ready card=1 side=question") && questionExists
                }
                guard completed, currentState.hasPrefix("ready card=1 side=question"), questionExists else {
                    let reason = currentState.hasPrefix("failed") ? "failed" : "did not become ready within 10 seconds"
                    throw AuditError(description: "Card audit precondition \(reason): state=\(currentState), Behavior question present=\(questionExists)")
                }
            }, supplemental: supplemental)
        add(AccessibilityAudit.screenReport("card.behavior.surface", states: [result]))
        AccessibilityAudit.assertClean(result.findings)
        TouchTargets.assertMet(result.touchTargets)
    }
}
