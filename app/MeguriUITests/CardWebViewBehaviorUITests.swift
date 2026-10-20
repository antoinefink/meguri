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
    private func launch() throws -> XCUIApplication {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("this configuration has no fixture routes") }
        bundleIdentifier = try build.bundleIdentifier
        #if os(macOS)
        guard bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("daily-driver Mac products cannot be launched by a test") }
        #endif
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: "card/behaviors").arguments
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
    private func click(_ element: XCUIElement, in app: XCUIApplication) throws {
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        #endif
        element.clickOrTap()
    }
    private func control(_ id: String, in app: XCUIApplication) throws {
        let button = app.descendants(matching: .any).matching(identifier: "card.behavior.\(id)").firstMatch
        if !button.exists || !button.isHittable {
            try click(app.descendants(matching: .any).matching(identifier: "card.behavior.actions").firstMatch, in: app)
        }
        XCTAssert(button.waitForExistence(timeout: 3), "missing control \(id)")
        try click(button, in: app)
    }
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
        let confirm = app.buttons["OK"]
        XCTAssert(confirm.waitForExistence(timeout: 5))
        try click(confirm, in: app)
        XCTAssert(wait { self.state(app).contains("dialog=true") }, state(app))
        try control("prompt", in: app)
        let prompt = app.buttons["OK"]
        XCTAssert(prompt.waitForExistence(timeout: 5))
        try click(prompt, in: app)
        XCTAssert(wait { self.state(app).contains("dialog=empty") }, state(app))
        try control("prompt", in: app)
        let cancel = app.buttons["Cancel"]
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
        XCTAssert(wait {
            guard let current = try? self.report(app) else { return false }
            return current["text"] as? String == "ka" && current["marker"] as? Double != initial["marker"] as? Double
        }, state(app))
        XCTAssertEqual(editor.value as? String, "native")
        try control("reveal", in: app)
        XCTAssert(wait { self.state(app).contains("side=answer") && self.state(app).contains("target=answer") }, state(app))
        XCTAssert(app.staticTexts["Behavior answer"].waitForExistence(timeout: 5))
        XCTAssertNotEqual(app.webViews.matching(identifier: "review.card.page.a").firstMatch.exists,
                          app.webViews.matching(identifier: "review.card.page.b").firstMatch.exists,
                          "exactly one named page root must be exported; the idle root must be absent")
        let pageReport = try report(app)
        XCTAssertEqual(pageReport["pageCount"] as? Int, 2)
        XCTAssertEqual(pageReport["distinctPages"] as? Bool, true)
        XCTAssertEqual(pageReport["currentHidden"] as? Bool, false)
        XCTAssertEqual(pageReport["idleHidden"] as? Bool, true)
    }

    func testNativeZoomFrameAndSameDocumentLinks() throws {
        let app = try launch()
        let web = app.webViews.firstMatch
        let before = try report(app)
        let height = try XCTUnwrap(before["height"] as? Double)
        XCTAssertEqual(try XCTUnwrap(before["viewport"] as? Double), height, accuracy: 1)
        XCTAssertEqual(try XCTUnwrap(before["bottom"] as? Double), try XCTUnwrap(before["layoutHeight"] as? Double), accuracy: 1)
        XCTAssertGreaterThan(web.frame.minY, app.windows.firstMatch.frame.minY)
        XCTAssertLessThanOrEqual(web.frame.maxY, app.windows.firstMatch.frame.maxY)
        #if !os(macOS)
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
        let app = try launch()
        try control("zoom", in: app)
        XCTAssert(wait { self.state(app).contains("zoom=150") }, state(app))
        let baselineReport = try report(app)
        let savedZoom = try XCTUnwrap(baselineReport["pageZoom"] as? Double)
        XCTAssertEqual(savedZoom, 1.5)
        #if os(iOS)
        let unpinchedVisualScale = try XCTUnwrap(baselineReport["visualScale"] as? Double)
        let unpinchedNativeZoom = try XCTUnwrap(baselineReport["nativeZoom"] as? Double)
        XCTAssertTrue(unpinchedVisualScale.isFinite && unpinchedNativeZoom.isFinite)
        XCTAssertGreaterThan(unpinchedVisualScale, 0)
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
        defer { try? voiceOver.disable() }
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
        #if os(iOS)
        @MainActor func activateMenuItemWithVoiceOver(label: String) throws {
            func identifiesItem(_ utterance: String) -> Bool {
                // Speech adds traits after the localized label. Match the whole
                // label clause, never a substring of another fixture item.
                utterance.components(separatedBy: CharacterSet(charactersIn: ",\n"))
                    .contains { $0.trimmingCharacters(in: .whitespacesAndNewlines) == label }
            }
            var utterance = try voiceOver.currentSpeech().utterance
            var visited: Set<String> = []
            for moves in 0...16 {
                attachSpeech(utterance, name: "menu-\(label)-move-\(moves)")
                if identifiesItem(utterance) {
                    let selected = try voiceOver.currentSpeech().utterance
                    attachSpeech(selected, name: "menu-\(label)-selected")
                    guard identifiesItem(selected) else {
                        throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 6,
                                      userInfo: [NSLocalizedDescriptionKey: "VoiceOver selection changed before activating \(label): \(selected)"])
                    }
                    let window = app.windows.firstMatch
                    let frame = window.frame
                    let coordinate = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                    let point = coordinate.screenPoint
                    guard window.exists, frame.minX.isFinite, frame.minY.isFinite,
                          frame.width.isFinite, frame.height.isFinite, frame.width > 0, frame.height > 0,
                          point.x.isFinite, point.y.isFinite,
                          point.x > frame.minX, point.x < frame.maxX,
                          point.y > frame.minY, point.y < frame.maxY else {
                        throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 7,
                                      userInfo: [NSLocalizedDescriptionKey: "No finite interior app-window point for VoiceOver menu activation"])
                    }
                    // A VoiceOver double tap activates the selected item,
                    // independently of the gesture's position in the window.
                    coordinate.doubleTap()
                    return
                }
                guard visited.insert(utterance).inserted, moves < 16 else {
                    throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 8,
                                  userInfo: [NSLocalizedDescriptionKey: "VoiceOver menu traversal cycled or exhausted 16 moves before \(label): \(utterance)"])
                }
                utterance = try voiceOver.moveForward().utterance
            }
        }
        #endif
        @MainActor func activateWithVoiceOver(_ element: XCUIElement, nativeMenuItem: Bool = false) throws {
            guard element.waitForExistence(timeout: 3) else {
                throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 2,
                              userInfo: [NSLocalizedDescriptionKey: "missing native VO control: \(element)"])
            }
            let label = element.label
            guard !label.isEmpty else {
                throw NSError(domain: "CardWebViewBehaviorUITests.Precondition", code: 3,
                              userInfo: [NSLocalizedDescriptionKey: "native VO control has an empty label: \(element)"])
            }
            #if os(macOS)
            try click(element, in: app)
            #else
            if nativeMenuItem {
                try activateMenuItemWithVoiceOver(label: label)
                return
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
        @MainActor func voiceOverControl(_ id: String) throws {
            let button = app.descendants(matching: .any).matching(identifier: "card.behavior.\(id)").firstMatch
            if !button.exists || !button.isHittable {
                try activateWithVoiceOver(app.descendants(matching: .any).matching(identifier: "card.behavior.actions").firstMatch)
            }
            try activateWithVoiceOver(button, nativeMenuItem: true)
        }
        for layoutIndex in 0...3 {
            let failuresBeforeLayout = testRun?.failureCount ?? 0
            if layoutIndex > 0 {
                try voiceOverControl(["separator", "fallback", "graphic"][layoutIndex - 1])
                try voiceOverControl("next")
                XCTAssert(wait { self.state(app).contains("card=\(layoutIndex + 1) side=question") }, state(app))
                #if os(iOS)
                let nextQuestionReport = try report(app)
                XCTAssertEqual(try XCTUnwrap(nextQuestionReport["visualScale"] as? Double), unpinchedVisualScale, accuracy: 0.01)
                XCTAssertEqual(try XCTUnwrap(nextQuestionReport["nativeZoom"] as? Double), unpinchedNativeZoom, accuracy: 0.01)
                XCTAssertEqual(try XCTUnwrap(nextQuestionReport["pageZoom"] as? Double), savedZoom)
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
                           layoutIndex == 0 ? questionGestureScale : unpinchedVisualScale, accuracy: 0.01)
            XCTAssertEqual(try XCTUnwrap(answerReport["nativeZoom"] as? Double),
                           layoutIndex == 0 ? questionNativeZoom : unpinchedNativeZoom, accuracy: 0.01)
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
            XCTAssertNotEqual(app.webViews.matching(identifier: "review.card.page.a").firstMatch.exists,
                              app.webViews.matching(identifier: "review.card.page.b").firstMatch.exists,
                              "exactly one named page root must be exported; the idle root must be absent")
            XCTAssertEqual(answerReport["pageCount"] as? Int, 2)
            XCTAssertEqual(answerReport["distinctPages"] as? Bool, true)
            XCTAssertEqual(answerReport["currentHidden"] as? Bool, false)
            XCTAssertEqual(answerReport["idleHidden"] as? Bool, true)
            XCTAssertFalse(app.staticTexts["Behavior question"].exists, "idle question content must stay excluded")
            if (testRun?.failureCount ?? 0) > failuresBeforeLayout {
                throw NSError(domain: "CardWebViewBehaviorUITests.Diagnostic", code: 1,
                              userInfo: [NSLocalizedDescriptionKey: "Stopped after capturing failed layout \(layoutIndex); recorded assertions remain failures"])
            }
        }
    }
    #endif

    func testCardSurfaceAccessibilityAudit() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("this configuration has no fixture routes") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("daily-driver Mac products cannot be launched by a test") }
        #endif
        let result = try AccessibilityAudit.runAllVariants(
            state: "question", launch: MeguriLaunch(locale: "en_US", animations: false, route: "card/behaviors"),
            waitFor: "card.behavior.state", suppressions: AuditSuppressions.bundled())
        add(AccessibilityAudit.screenReport("card.behavior.surface", states: [result]))
        AccessibilityAudit.assertClean(result.findings)
        TouchTargets.assertMet(result.touchTargets)
    }
}
