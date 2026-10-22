import Foundation
import XCTest
#if os(macOS)
import AppKit
import ApplicationServices
#endif

/// Actual production shell/core/pages. Seed only the first launch: the second
/// launch must reuse the database and must not resubmit a recorded answer.
@MainActor
final class LocalReviewJourneyTests: XCTestCase {
    #if os(macOS)
    /// Real key events and native accessibility focus run only in the UI VM.
    func testInitialKeyboardReviewAndNativeCloseBackActivation() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundleIdentifier = try build.bundleIdentifier
        guard bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let fixture = build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1")
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false).arguments
        app.launch()
        defer { app.terminate() }
        addTeardownBlock { @MainActor in app.terminate() }
        let enable = element(app, "home.policy.enable")
        if enable.waitForExistence(timeout: 5) {
            try app.bringToFront(bundleIdentifier: bundleIdentifier)
            enable.clickOrTap()
        }
        let deck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "home.deck.row.", "Cards")).firstMatch
        XCTAssertTrue(deck.waitForExistence(timeout: 20), "Generate and stage local-review-1 in the VM before this suite")
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        deck.clickOrTap()
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        study.clickOrTap()
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        // No card click: Study must yield a keyboard-usable first question.
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("4", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        XCTAssertTrue(element(app, "review.session.back").isEnabled)
        // Leave one earlier answer so Back stays enabled after the undo.
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("4", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))

        let back = try focusNativeControl("review.session.back", in: app, bundleIdentifier: bundleIdentifier)
        let question = try cardProgressSnapshot(app, answer: false)
        XCTAssertEqual(question.progress, "0 new, 0 learning, 2 to review")
        XCTAssertFalse(element(app, "review.session.redo").exists)
        attach(app, "native-back-question-before-return")
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("\r", modifierFlags: [])
        assertNativeFocus(back)
        XCTAssertEqual(try cardProgressSnapshot(app, answer: false), question,
            "Return on focused Back must not undo, reveal or rate")
        XCTAssertFalse(element(app, "review.session.redo").exists)
        attach(app, "native-back-return-keeps-question")

        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.redo").waitForExistence(timeout: 10), "Space on Back must undo; a review shortcut must not consume its key")
        XCTAssertTrue(element(app, "review.session.reveal").exists)
        assertNativeFocus(back) // The restored page must leave Back in control.
        let undone = try cardProgressSnapshot(app, answer: false)
        XCTAssertEqual(undone.progress, "0 new, 0 learning, 3 to review")
        XCTAssertNotEqual(undone.cardText, question.cardText, "Back must restore the previous card")
        attach(app, "native-back-space-undoes")

        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        element(app, "review.session.reveal").clickOrTap()
        XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
        let close = try focusNativeControl("review.session.close", in: app, bundleIdentifier: bundleIdentifier)
        let answer = try cardProgressSnapshot(app, answer: true)
        XCTAssertEqual(answer.progress, undone.progress)
        attach(app, "native-close-answer-before-return")
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("\r", modifierFlags: [])
        assertNativeFocus(close)
        XCTAssertEqual(try cardProgressSnapshot(app, answer: true), answer,
            "Return on focused Close must not dismiss, rate or advance")
        XCTAssertTrue(element(app, "review.session.redo").exists)
        attach(app, "native-close-return-keeps-answer")

        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(study.waitForExistence(timeout: 10), "Space on Close must dismiss the answer; its key must not rate Good")
        XCTAssertFalse(element(app, "review.session.screen").exists)
        XCTAssertFalse(element(app, "review.session.rate.good").exists)
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answers recorded"),
            "Closing must retain only the one answer left after undo")
        XCTAssertTrue(waitForText(element(app, "home.deck.counts"), "0 new · 0 learning · 3 due"),
            "Closing must not rate the restored card")
        attach(app, "native-space-close")
    }

    private struct CardProgressSnapshot: Equatable {
        let cardText: [String]
        let progress: String
    }

    private func cardProgressSnapshot(_ app: XCUIApplication, answer: Bool) throws -> CardProgressSnapshot {
        // macOS publishes the production WebKit text through app.staticTexts.
        let expected = answer ? "Answer [0-9]+" : "Question [0-9]+"
        let sideText = app.staticTexts.matching(NSPredicate(format: "label MATCHES %@ OR value MATCHES %@", expected, expected)).firstMatch
        XCTAssertTrue(sideText.waitForExistence(timeout: 10), "Rendered card text must identify the current side")
        let pattern = "^(Question|Answer) [0-9]+$"
        let texts = app.staticTexts.matching(NSPredicate(format: "label MATCHES %@ OR value MATCHES %@", pattern, pattern))
        let cardText = try texts.allElementsBoundByIndex.map { text in
            try XCTUnwrap([text.label, text.value as? String].compactMap { $0 }.first {
                $0.range(of: pattern, options: .regularExpression) != nil
            }, "Card accessibility text must contain a fixture question or answer")
        }.sorted()
        XCTAssertFalse(cardText.isEmpty, "A missing card snapshot cannot establish continuity")
        XCTAssertEqual(cardText.contains { $0.hasPrefix("Answer ") }, answer)
        let progressElement = element(app, "review.session.progress")
        XCTAssertTrue(progressElement.waitForExistence(timeout: 10))
        let progress = try XCTUnwrap(progressElement.value as? String, "Session progress must publish its counts")
        XCTAssertFalse(progress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        XCTAssertEqual(element(app, "review.session.reveal").exists, !answer)
        for rating in ["again", "hard", "good", "easy"] {
            XCTAssertEqual(element(app, "review.session.rate.\(rating)").exists, answer)
        }
        XCTAssertFalse(element(app, "review.session.refresh").exists)
        XCTAssertFalse(element(app, "review.session.done").exists)
        return CardProgressSnapshot(cardText: cardText, progress: progress)
    }

    private struct NativeFocusTarget {
        let control: AXUIElement
        let application: AXUIElement
        let identifier: String
        let diagnostics: String
    }

    private func focusNativeControl(_ identifier: String, in app: XCUIApplication, bundleIdentifier: String) throws -> NativeFocusTarget {
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        let running = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: bundleIdentifier)
            .first { !$0.isTerminated })
        let application = AXUIElementCreateApplication(running.processIdentifier)
        let deadline = Date().addingTimeInterval(5)
        var attempts = 0, windowCount = 0, rootCount = 0, visitedCount = 0, duplicates = 0, maxDepth = 0
        var readErrors: [String: Int] = [:]
        var stopReason: String?
        var matchedDiagnostics = "matchedControl=none"
        func diagnostics() -> String {
            let errors = readErrors.sorted { $0.key < $1.key }.map { "\($0.key):\($0.value)" }.joined(separator: ",")
            return "trusted=\(AXIsProcessTrusted()); pid=\(running.processIdentifier); attempts=\(attempts); windows=\(windowCount); roots=\(rootCount); visited=\(visitedCount); duplicates=\(duplicates); maxDepth=\(maxDepth); \(matchedDiagnostics); AXReadErrors[attribute/code:count]=[\(errors)]"
        }
        func read(_ node: AXUIElement, _ attribute: String) -> CFTypeRef? {
            var value: CFTypeRef?
            let error = AXUIElementCopyAttributeValue(node, attribute as CFString, &value)
            guard error == .success else {
                readErrors["\(attribute)/\(error.rawValue)", default: 0] += 1
                switch error {
                case .attributeUnsupported, .noValue, .cannotComplete, .invalidUIElement: break
                default: stopReason = "AXReadError(\(error.rawValue))"
                }
                return nil
            }
            if value == nil { stopReason = "AXSuccessWithoutValue(\(attribute))" }
            return value
        }
        func findInWindows() -> AXUIElement? {
            windowCount = 0; rootCount = 0; visitedCount = 0; duplicates = 0; maxDepth = 0
            var roots: [AXUIElement] = []
            if let value = read(application, kAXWindowsAttribute) {
                guard let windows = value as? [AXUIElement] else {
                    stopReason = "AXUnexpectedType(AXWindows)"; return nil
                }
                windowCount = windows.count
                roots.append(contentsOf: windows)
            }
            // These public window roots may become available before AXWindows.
            for attribute in [kAXFocusedWindowAttribute, kAXMainWindowAttribute] {
                if let value = read(application, attribute) {
                    guard CFGetTypeID(value) == AXUIElementGetTypeID() else {
                        stopReason = "AXUnexpectedType(\(attribute))"; return nil
                    }
                    roots.append(value as! AXUIElement)
                }
            }
            rootCount = roots.count
            var pending = roots.reversed().map { ($0, 0) }
            var visited: [CFHashCode: [AXUIElement]] = [:]
            while let (node, depth) = pending.popLast(), stopReason == nil {
                let hash = CFHash(node)
                if visited[hash, default: []].contains(where: { CFEqual($0, node) }) {
                    duplicates += 1
                    continue
                }
                guard visitedCount < 4096, depth <= 64 else {
                    stopReason = "AXWalkLimit"; return nil
                }
                visited[hash, default: []].append(node)
                visitedCount += 1
                maxDepth = max(maxDepth, depth)
                if let value = read(node, kAXIdentifierAttribute) {
                    guard let nativeIdentifier = value as? String else {
                        stopReason = "AXUnexpectedType(AXIdentifier)"; return nil
                    }
                    if nativeIdentifier == identifier {
                        let role = read(node, kAXRoleAttribute) as? String
                        var settable = DarwinBoolean(false)
                        let settableError = AXUIElementIsAttributeSettable(node, kAXFocusedAttribute as CFString, &settable)
                        matchedDiagnostics = "matchedIdentifier=\(nativeIdentifier); matchedRole=\(role ?? "<unavailable>"); AXFocusedSettable=\(settable.boolValue); AXFocusedSettableError=\(settableError.rawValue)"
                        // SwiftUI may publish the identifier on a wrapper too.
                        if role == kAXButtonRole { return node }
                    }
                }
                if let value = read(node, kAXChildrenAttribute) {
                    guard let children = value as? [AXUIElement] else {
                        stopReason = "AXUnexpectedType(AXChildren)"; return nil
                    }
                    pending.append(contentsOf: children.reversed().map { ($0, depth + 1) })
                }
            }
            return nil
        }
        var found: AXUIElement?
        repeat {
            attempts += 1
            found = findInWindows()
            if found != nil || stopReason != nil || Date() >= deadline { break }
            // Wait only for window/control publication or transient AX reads.
            // Focus writes and keyboard actions are never retried.
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        } while true
        let control = try XCTUnwrap(found, "Native control missing: \(identifier); reason=\(stopReason ?? "AXReadinessTimeout"); \(diagnostics())")
        let focusError = AXUIElementSetAttributeValue(control, kAXFocusedAttribute as CFString, kCFBooleanTrue)
        let target = NativeFocusTarget(control: control, application: application, identifier: identifier,
            diagnostics: "Native AX focus set error=\(focusError.rawValue); \(diagnostics())")
        XCTAssertEqual(focusError, .success, target.diagnostics)
        assertNativeFocus(target)
        return target
    }

    private func assertNativeFocus(_ target: NativeFocusTarget) {
        func read(_ node: AXUIElement, _ attribute: String) -> (value: CFTypeRef?, error: AXError) {
            var value: CFTypeRef?
            let error = AXUIElementCopyAttributeValue(node, attribute as CFString, &value)
            return (value, error)
        }
        let deadline = Date().addingTimeInterval(2)
        var observation = ""
        repeat {
            let focused = read(target.control, kAXFocusedAttribute)
            if focused.error == .success, focused.value as? Bool == true { return }

            let applicationFocus = read(target.application, kAXFocusedUIElementAttribute)
            observation = "AXFocusedError=\(focused.error.rawValue); AXFocused=\(String(describing: focused.value as? Bool)); AXFocusedUIElementError=\(applicationFocus.error.rawValue)"
            if applicationFocus.error == .success, let value = applicationFocus.value,
               CFGetTypeID(value) == AXUIElementGetTypeID() {
                let nativeFocused = value as! AXUIElement
                let identifier = read(nativeFocused, kAXIdentifierAttribute)
                let role = read(nativeFocused, kAXRoleAttribute)
                observation += "; focusedIdentifier=\(identifier.value as? String ?? "<unavailable>"); focusedIdentifierError=\(identifier.error.rawValue); focusedRole=\(role.value as? String ?? "<unavailable>"); focusedRoleError=\(role.error.rawValue)"
                // Undo can replace the native button. Accept only its exact identity and role.
                if identifier.error == .success, identifier.value as? String == target.identifier,
                   role.error == .success, role.value as? String == kAXButtonRole { return }
            } else {
                observation += "; focusedIdentifier=<unavailable>; focusedRole=<unavailable>"
            }
            if Date() >= deadline { break }
            // Publication readiness only: never repeat a focus write or key action.
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        } while true
        XCTFail("Native keyboard focus readiness timeout; expectedIdentifier=\(target.identifier); expectedRole=\(kAXButtonRole); trusted=\(AXIsProcessTrusted()); \(observation); \(target.diagnostics)")
    }
    #endif

    func testStudyRevealFourRatingsUndoWaitAndRestart() throws {
        let (app, bundleIdentifier) = try startFreshReview()
        for rating in ["again", "hard", "good", "easy"] {
            try reveal(app, bundleIdentifier: bundleIdentifier)
            for name in ["again", "hard", "good", "easy"] {
                let button = element(app, "review.session.rate.\(name)")
                XCTAssertTrue(button.exists)
                XCTAssertTrue(button.label.contains(","), "rating must include its actual interval")
            }
            try click(element(app, "review.session.rate.\(rating)"), in: app, bundleIdentifier: bundleIdentifier)
        }
        let back = element(app, "review.session.back")
        XCTAssertTrue(back.waitForExistence(timeout: 10))
        XCTAssertTrue(back.isEnabled)
        try click(back, in: app, bundleIdentifier: bundleIdentifier)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        XCTAssertTrue(element(app, "review.session.redo").exists)
        var answers = 3 // Four ratings, less the undone Easy.
        for _ in 0..<20 {
            let state = reviewState(app)
            if state == "review.session.refresh" { break }
            guard state == "review.session.reveal" else {
                attach(app, "waiting-unexpected-state")
                XCTFail("Expected a question or waiting for learning, got \(state)")
                return
            }
            try reveal(app, bundleIdentifier: bundleIdentifier)
            try click(element(app, "review.session.rate.easy"), in: app, bundleIdentifier: bundleIdentifier)
            answers += 1
        }
        let waiting = element(app, "review.session.refresh").waitForExistence(timeout: 10)
        attach(app, "review-waiting-for-learning")
        XCTAssertTrue(waiting, "Again at the pinned clock must leave learning waiting, not exhausted")
        XCTAssertTrue(waitingTitle(app).exists)
        XCTAssertFalse(element(app, "review.session.done").exists)
        // Close the waiting session through its real control, without advancing time.
        try click(element(app, "review.session.close"), in: app, bundleIdentifier: bundleIdentifier)
        try assertSavedAndRestart(app, bundleIdentifier: bundleIdentifier, answers: answers, evidence: "waiting")
    }

    func testAllEasyExhaustionAndRestart() throws {
        let (app, bundleIdentifier) = try startFreshReview()
        var answers = 0
        for _ in 0..<20 {
            let state = reviewState(app)
            if state == "review.session.done" { break }
            guard state == "review.session.reveal" else {
                attach(app, "exhaustion-unexpected-state")
                XCTFail("All-Easy must reach exhaustion, got \(state)")
                return
            }
            try reveal(app, bundleIdentifier: bundleIdentifier)
            try click(element(app, "review.session.rate.easy"), in: app, bundleIdentifier: bundleIdentifier)
            answers += 1
        }
        let exhausted = element(app, "review.session.done").waitForExistence(timeout: 10)
        attach(app, "review-all-easy-exhausted")
        XCTAssertTrue(exhausted, "all-Easy text fixture did not exhaust within the safety cap")
        XCTAssertFalse(element(app, "review.session.refresh").exists)
        XCTAssertFalse(waitingTitle(app).exists)
        XCTAssertEqual(answers, 4, "each of the four due text cards must be answered exactly once")
        try click(element(app, "review.session.done"), in: app, bundleIdentifier: bundleIdentifier)
        try assertSavedAndRestart(app, bundleIdentifier: bundleIdentifier, answers: answers, evidence: "exhausted")
    }

    private func startFreshReview() throws -> (XCUIApplication, String) {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundleIdentifier = try build.bundleIdentifier
        guard bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = launchArguments(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"))
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let enable = element(app, "home.policy.enable")
        if enable.waitForExistence(timeout: 5) {
            try click(enable, in: app, bundleIdentifier: bundleIdentifier)
        }
        try openDeck(app, bundleIdentifier: bundleIdentifier)
        let freshCounts = waitForText(element(app, "home.deck.counts"), "0 new · 0 learning · 4 due")
        if !freshCounts { attach(app, "review-fresh-counts") }
        XCTAssertTrue(freshCounts, "first launch must seed four due text cards")
        try click(element(app, "home.deck.study"), in: app, bundleIdentifier: bundleIdentifier)
        return (app, bundleIdentifier)
    }

    private func assertSavedAndRestart(_ app: XCUIApplication, bundleIdentifier: String, answers: Int, evidence: String) throws {
        assertSavedOutcome(app, answers: answers, evidence: "\(evidence)-closed")
        app.terminate()
        // No fixture and no study input: reuse the database without replaying answers.
        app.launchArguments = launchArguments()
        app.launch()
        XCTAssertTrue(element(app, "home.session.recorded").waitForExistence(timeout: 20))
        try openDeck(app, bundleIdentifier: bundleIdentifier)
        assertSavedOutcome(app, answers: answers, evidence: "\(evidence)-restarted")
    }

    private func assertSavedOutcome(_ app: XCUIApplication, answers: Int, evidence: String) {
        let recorded = waitForText(element(app, "home.session.recorded"), "\(answers) answers recorded")
        // Home can appear before finishReview's asynchronous owner refresh applies.
        // Later-today learning is outside these counts; only the waiting case
        // must show the separate Waiting for Learning state before Close.
        let counts = waitForText(element(app, "home.deck.counts"), "0 new · 0 learning · 0 due")
        attach(app, "review-\(evidence)")
        XCTAssertTrue(recorded, "saved record must contain exactly the accepted, non-undone answers")
        XCTAssertTrue(counts, "authoritative deck counts must reflect the reviews before and after restart")
        XCTAssertFalse(element(app, "home.collection.error").exists)
    }

    private func launchArguments(fixture: URL? = nil) -> [String] {
        MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false).arguments
    }

    private func openDeck(_ app: XCUIApplication, bundleIdentifier: String) throws {
        let deck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "home.deck.row.", "Cards")).firstMatch
        XCTAssertTrue(deck.waitForExistence(timeout: 20), "Generate and stage local-review-1 in the VM before this suite")
        try click(deck, in: app, bundleIdentifier: bundleIdentifier)
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10))
    }

    private func reviewState(_ app: XCUIApplication) -> String {
        let ready = app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@",
            ["review.session.reveal", "review.session.refresh", "review.session.done"])).firstMatch
        let appeared = ready.waitForExistence(timeout: 10)
        if !appeared { attach(app, "review-state-timeout") }
        XCTAssertTrue(appeared, "review must settle on a question, waiting or exhaustion")
        return ready.identifier
    }

    private func reveal(_ app: XCUIApplication, bundleIdentifier: String) throws {
        let button = element(app, "review.session.reveal")
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        try click(button, in: app, bundleIdentifier: bundleIdentifier)
        XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
    }

    private func click(_ target: XCUIElement, in app: XCUIApplication, bundleIdentifier: String) throws {
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        target.clickOrTap()
    }

    private func waitingTitle(_ app: XCUIApplication) -> XCUIElement {
        app.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@",
            "Waiting for Learning", "Waiting for Learning")).firstMatch
    }

    private func waitForText(_ element: XCUIElement, _ text: String) -> Bool {
        let predicate = NSPredicate(format: "exists == true AND (label == %@ OR value == %@)", text, text)
        return XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: 20) == .completed
    }
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
    private func attach(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot()); attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
