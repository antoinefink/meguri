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
        XCTAssert(wait { self.state(app).contains("ready card=1 side=question") }, state(app))
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
        XCTAssertEqual(app.webViews.count, 1, "the idle page must be excluded from accessibility")
    }

    func testNativeZoomFrameAndSameDocumentLinks() throws {
        let app = try launch()
        let web = app.webViews.firstMatch
        let before = try report(app)
        let height = try XCTUnwrap(before["height"] as? Double)
        XCTAssertEqual(try XCTUnwrap(before["viewport"] as? Double), height, accuracy: 1)
        XCTAssertEqual(try XCTUnwrap(before["bottom"] as? Double), height, accuracy: 1)
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

    #if os(iOS) && targetEnvironment(simulator)
    /// Use tools/mg ui-test in the simulator VM. Check the native VO target
    /// and its successor, then verify that typing still reaches the editor.
    func testVoiceOverAnswerTargetWithExternalKeyboardOwner() throws {
        let app = try launch()
        let voiceOver = XCUIDevice.shared.voiceOverService
        try voiceOver.enable()
        defer { try? voiceOver.disable() }
        for layoutIndex in 0...2 {
            if layoutIndex > 0 {
                try control(layoutIndex == 1 ? "separator" : "fallback", in: app)
                try control("next", in: app)
                XCTAssert(wait { self.state(app).contains("card=\(layoutIndex + 1) side=question") }, state(app))
            }
            let nativeEditor = app.textFields["card.behavior.editor"]
            try click(nativeEditor, in: app)
            nativeEditor.typeText("v\(layoutIndex)")
            let retainedText = try XCTUnwrap(nativeEditor.value as? String)
            try control("reveal", in: app)
            XCTAssert(wait { self.state(app).contains("side=answer") }, state(app))
            XCTAssert(wait { (try? self.report(app)["voiceFocus"] as? String) == "Behavior answer" }, state(app))
            XCTAssertTrue(try voiceOver.currentSpeech().utterance.contains("Behavior answer"))
            XCTAssertTrue(try voiceOver.moveForward().utterance.contains("Typed answer"), "The next VO element must follow the answer destination")
            // Send keyboard input directly; the native editor must retain it.
            app.typeText("x")
            XCTAssertEqual(nativeEditor.value as? String, retainedText + "x")
            XCTAssertEqual(app.webViews.count, 1)
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
