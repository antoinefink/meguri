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
        let (app, bundleIdentifier) = try startFreshReview()
        defer { app.terminate() }
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        let firstQuestion = try cardSnapshot(app, answer: false)
        // No card click: Study must yield a keyboard-usable first question.
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("4", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        let secondQuestion = try cardSnapshot(app, answer: false)
        XCTAssertNotEqual(secondQuestion.cardText, firstQuestion.cardText)
        try assertStudyHistory(app, bundleIdentifier: bundleIdentifier, undo: true, redo: false)
        // Accept a second answer using Return, then restore it through Study.
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.rate.easy").waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("\r", modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        let thirdQuestion = try cardSnapshot(app, answer: false)
        XCTAssertNotEqual(thirdQuestion.cardText, secondQuestion.cardText)

        try performEditCommand("shell.command.undo", in: app, bundleIdentifier: bundleIdentifier)
        try assertQuestion(secondQuestion, in: app)
        try assertStudyHistory(app, bundleIdentifier: bundleIdentifier, undo: true, redo: true)
        try performEditCommand("shell.command.redo", in: app, bundleIdentifier: bundleIdentifier)
        try assertQuestion(thirdQuestion, in: app)
        try assertStudyHistory(app, bundleIdentifier: bundleIdentifier, undo: true, redo: false)
        try performEditCommand("shell.command.undo", in: app, bundleIdentifier: bundleIdentifier)
        try assertQuestion(secondQuestion, in: app)
        try assertStudyHistory(app, bundleIdentifier: bundleIdentifier, undo: true, redo: true)

        // N3 keeps Undo/Redo in native menus; the touch-only Back focus case
        // therefore moves to Back to Decks, the Mac's actual native Close.
        let closeQuestion = try focusNativeControl("review.session.close", in: app, bundleIdentifier: bundleIdentifier)
        let question = try cardSnapshot(app, answer: false)
        assertNativeFocus(closeQuestion)
        attach(app, "native-close-question-before-return")
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("\r", modifierFlags: [])
        assertNativeFocus(closeQuestion)
        XCTAssertEqual(try cardSnapshot(app, answer: false), question,
            "Return on focused Back to Decks must not dismiss, undo, reveal or rate")
        attach(app, "native-close-return-keeps-question")

        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        element(app, "review.session.reveal").clickOrTap()
        XCTAssertTrue(element(app, "review.session.rate.good").waitForExistence(timeout: 10))
        let close = try focusNativeControl("review.session.close", in: app, bundleIdentifier: bundleIdentifier)
        let answer = try cardSnapshot(app, answer: true)
        attach(app, "native-close-answer-before-return")
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey("\r", modifierFlags: [])
        assertNativeFocus(close)
        XCTAssertEqual(try cardSnapshot(app, answer: true), answer,
            "Return on focused Close must not dismiss, rate or advance")
        assertNativeFocus(close)
        attach(app, "native-close-return-keeps-answer")

        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10), "Space on Close must dismiss the answer; its key must not rate Good")
        XCTAssertFalse(element(app, "review.session.screen").exists)
        XCTAssertFalse(element(app, "review.session.rate.good").exists)
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answer recorded"),
            "Closing must retain only the one answer left after undo")
        XCTAssertTrue(waitForDeckCounts(app, due: 3),
            "Closing must not rate the restored card")
        attach(app, "native-space-close")
        try assertSavedAndRestart(app, bundleIdentifier: bundleIdentifier, answers: 1, evidence: "native-close", due: 3)
    }

    /// N20.4: one Undo and one Redo, in Edit, titled after the step (R173).
    private func editMenu(_ app: XCUIApplication) -> XCUIElement {
        app.menuBars.menuBarItems["Edit"]
    }

    private func performEditCommand(_ identifier: String, in app: XCUIApplication, bundleIdentifier: String) throws {
        let menu = editMenu(app)
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        try click(menu, in: app, bundleIdentifier: bundleIdentifier)
        let command = menu.menuItems.matching(identifier: identifier).firstMatch
        XCTAssertTrue(command.waitForExistence(timeout: 10))
        XCTAssertTrue(command.isEnabled, "Edit > \(command.title) must be available to the card")
        try click(command, in: app, bundleIdentifier: bundleIdentifier)
    }

    private func assertStudyHistory(_ app: XCUIApplication, bundleIdentifier: String, undo: Bool, redo: Bool) throws {
        XCTAssertFalse(app.menuBars.menuBarItems["Study"].exists, "The Study menu is gone (N20.4)")
        let menu = editMenu(app)
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        try click(menu, in: app, bundleIdentifier: bundleIdentifier)
        for (identifier, verb, enabled) in [("shell.command.undo", "Undo", undo), ("shell.command.redo", "Redo", redo)] {
            let command = menu.menuItems.matching(identifier: identifier).firstMatch
            XCTAssertTrue(command.waitForExistence(timeout: 10))
            XCTAssertEqual(command.isEnabled, enabled, "Edit > \(verb) must respect history and native input ownership")
            // R173: "Undo Answer Card" while there is a step, plain "Undo" otherwise.
            XCTAssertEqual(command.title, enabled ? "\(verb) Answer Card" : verb)
        }
        // Escape belongs to the open menu and must not exit the review.
        try app.bringToFront(bundleIdentifier: bundleIdentifier)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(element(app, "review.session.screen").exists)
    }

    private struct CardSnapshot: Equatable {
        let cardText: [String]
    }

    private func assertQuestion(_ expected: CardSnapshot, in app: XCUIApplication) throws {
        // Restoration is asynchronous: wait for the expected card's text,
        // rather than accepting the still-visible question from before it.
        for text in expected.cardText {
            let rendered = app.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", text, text)).firstMatch
            XCTAssertTrue(rendered.waitForExistence(timeout: 10), "Restoration must show \(text)")
        }
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        XCTAssertEqual(try cardSnapshot(app, answer: false), expected, "Restoration must preserve exact card identity and question state")
    }

    private func cardSnapshot(_ app: XCUIApplication, answer: Bool) throws -> CardSnapshot {
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
        XCTAssertEqual(element(app, "review.session.reveal").exists, !answer)
        for rating in ["again", "hard", "good", "easy"] {
            XCTAssertEqual(element(app, "review.session.rate.\(rating)").exists, answer)
        }
        XCTAssertFalse(element(app, "review.session.refresh").exists)
        XCTAssertFalse(element(app, "review.session.done").exists)
        return CardSnapshot(cardText: cardText)
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
        guard let answers = try reachWaiting(app, bundleIdentifier: bundleIdentifier) else { return }
        // Close the waiting session through its real control, without advancing time.
        try click(element(app, "review.session.close"), in: app, bundleIdentifier: bundleIdentifier)
        try assertSavedAndRestart(app, bundleIdentifier: bundleIdentifier, answers: answers, evidence: "waiting")
    }

    func testAllEasyExhaustionAndRestart() throws {
        let (app, bundleIdentifier) = try startFreshReview()
        guard let answers = try reachExhaustion(app, bundleIdentifier: bundleIdentifier) else { return }
        try click(element(app, "review.session.done"), in: app, bundleIdentifier: bundleIdentifier)
        try assertSavedAndRestart(app, bundleIdentifier: bundleIdentifier, answers: answers, evidence: "exhausted")
    }

    #if os(iOS)
    func testTerminalActionsAtLargestAccessibilitySize() throws {
        let appearance = XCUIDevice.shared.appearance
        defer { XCUIDevice.shared.appearance = appearance }
        let variant = AccessibilityAudit.Variant(largestText: true, dark: false)
        variant.apply()
        let extra = variant.launchArguments

        let (waitingApp, waitingBundle) = try startFreshReview(extra: extra)
        guard let waitingAnswers = try reachWaiting(waitingApp, bundleIdentifier: waitingBundle) else { return }
        let checkNow = try readyTerminalButton("review.session.refresh", in: waitingApp)
        checkNow.clickOrTap()
        // The pinned clock can yield waiting again. This checks availability,
        // not independent evidence that the retry callback executed.
        XCTAssertEqual(reviewState(waitingApp), "review.session.refresh")
        XCTAssertTrue(waitingTitle(waitingApp).waitForExistence(timeout: 10))
        XCTAssertTrue(element(waitingApp, "review.session.refresh").isEnabled)
        XCTAssertFalse(element(waitingApp, "review.session.done").exists)
        // Close the waiting session through its real control, without advancing time.
        try click(element(waitingApp, "review.session.close"), in: waitingApp, bundleIdentifier: waitingBundle)
        try assertSavedAndRestart(waitingApp, bundleIdentifier: waitingBundle, answers: waitingAnswers,
            evidence: "ax5-waiting", extra: extra)
        waitingApp.terminate()

        let (finishedApp, finishedBundle) = try startFreshReview(extra: extra)
        guard let finishedAnswers = try reachExhaustion(finishedApp, bundleIdentifier: finishedBundle) else { return }
        let done = try readyTerminalButton("review.session.done", in: finishedApp)
        done.clickOrTap()
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),
            object: element(finishedApp, "review.session.screen"))
        XCTAssertEqual(XCTWaiter().wait(for: [dismissed], timeout: 10), .completed,
            "Done must dismiss the actual review presentation")
        XCTAssertFalse(element(finishedApp, "review.session.done").exists)
        try assertSavedAndRestart(finishedApp, bundleIdentifier: finishedBundle, answers: finishedAnswers,
            evidence: "ax5-exhausted", extra: extra)
    }

    private func readyTerminalButton(_ identifier: String, in app: XCUIApplication) throws -> XCUIElement {
        let owners = app.scrollViews.matching(identifier: "review.session.status")
        let scroll = owners.firstMatch
        XCTAssertTrue(scroll.waitForExistence(timeout: 10))
        XCTAssertEqual(owners.count, 1, "Terminal content must have one owning status scroll viewport")
        let buttons = scroll.descendants(matching: .button).matching(identifier: identifier)
        let button = buttons.firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        XCTAssertEqual(buttons.count, 1, "The action must be a unique native button inside the status scroll")
        let enabled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND enabled == true"), object: button)
        XCTAssertEqual(XCTWaiter().wait(for: [enabled], timeout: 10), .completed)
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 10))

        func validFrame(_ frame: CGRect) -> Bool {
            !frame.isNull && !frame.isInfinite && frame.width > 0 && frame.height > 0 &&
                [frame.minX, frame.minY, frame.maxX, frame.maxY, frame.width, frame.height].allSatisfy(\.isFinite)
        }
        var diagnostics = ""
        // Only this owning scroll may move, at most three times. Choose the
        // direction from the actual button frame, including on short screens.
        for gestures in 0...3 {
            XCTAssertEqual(owners.count, 1)
            XCTAssertEqual(buttons.count, 1)
            let windowFrame = try XCTUnwrap(validFrame(window.frame) ? window.frame : nil, "Invalid app window frame")
            let scrollFrame = try XCTUnwrap(validFrame(scroll.frame) ? scroll.frame : nil, "Invalid status scroll frame")
            let intersection = scrollFrame.intersection(windowFrame)
            let viewport = try XCTUnwrap(validFrame(intersection) ? intersection : nil, "Status viewport must intersect the app window")
            let frame = try XCTUnwrap(validFrame(button.frame) ? button.frame : nil, "Invalid terminal button frame")
            diagnostics = "action=\(identifier); gestures=\(gestures); button=\(frame); viewport=\(viewport); window=\(windowFrame)"
            if viewport.contains(frame) {
                let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                    button.exists && button.isEnabled && button.isHittable &&
                        validFrame(window.frame) && validFrame(scroll.frame) && validFrame(button.frame) &&
                        validFrame(scroll.frame.intersection(window.frame)) &&
                        scroll.frame.intersection(window.frame).contains(button.frame)
                }, object: button)
                let result = XCTWaiter().wait(for: [ready], timeout: 10)
                return try XCTUnwrap(result == .completed ? button : nil, "Terminal action is not fully visible and hittable; \(diagnostics)")
            }
            guard gestures < 3 else { break }
            if frame.maxY > viewport.maxY {
                scroll.swipeUp()
            } else if frame.minY < viewport.minY {
                scroll.swipeDown()
            } else {
                break
            }
        }
        XCTFail("Terminal action is not fully inside its owning viewport; \(diagnostics)")
        throw NSError(domain: "LocalReviewJourneyTests.TerminalReadiness", code: 1)
    }
    #endif

    private func reachWaiting(_ app: XCUIApplication, bundleIdentifier: String) throws -> Int? {
        #if os(macOS)
        var easyQuestion: CardSnapshot?
        #endif
        for rating in ["again", "hard", "good", "easy"] {
            #if os(macOS)
            if rating == "easy" { easyQuestion = try cardSnapshot(app, answer: false) }
            #endif
            try reveal(app, bundleIdentifier: bundleIdentifier)
            for name in ["again", "hard", "good", "easy"] {
                let button = element(app, "review.session.rate.\(name)")
                XCTAssertTrue(button.exists)
                XCTAssertTrue(button.label.contains(","), "rating must include its actual interval")
            }
            try click(element(app, "review.session.rate.\(rating)"), in: app, bundleIdentifier: bundleIdentifier)
        }
        #if os(macOS)
        try performEditCommand("shell.command.undo", in: app, bundleIdentifier: bundleIdentifier)
        try assertQuestion(try XCTUnwrap(easyQuestion), in: app)
        try assertStudyHistory(app, bundleIdentifier: bundleIdentifier, undo: true, redo: true)
        #else
        let back = element(app, "review.session.back")
        XCTAssertTrue(back.waitForExistence(timeout: 10))
        XCTAssertTrue(back.isEnabled)
        try click(back, in: app, bundleIdentifier: bundleIdentifier)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 10))
        XCTAssertTrue(element(app, "review.session.redo").exists)
        #endif
        var answers = 3 // Four ratings, less the undone Easy.
        for _ in 0..<20 {
            let state = reviewState(app)
            if state == "review.session.refresh" { break }
            guard state == "review.session.reveal" else {
                attach(app, "waiting-unexpected-state")
                XCTFail("Expected a question or waiting for learning, got \(state)")
                return nil
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
        return answers
    }

    private func reachExhaustion(_ app: XCUIApplication, bundleIdentifier: String) throws -> Int? {
        var answers = 0
        for _ in 0..<20 {
            let state = reviewState(app)
            if state == "review.session.done" { break }
            guard state == "review.session.reveal" else {
                attach(app, "exhaustion-unexpected-state")
                XCTFail("All-Easy must reach exhaustion, got \(state)")
                return nil
            }
            // N16.1, N16.2: the bottom-edge bar says how far the session is, and
            // a collection with sync off shows no sync status button in review.
            let progress = element(app, "review.session.progress")
            XCTAssertTrue(progress.waitForExistence(timeout: 10))
            XCTAssertEqual(progress.value as? String, "\(answers) of 4")
            XCTAssertFalse(element(app, "sync.status.button").exists)
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
        // N16.5: the number of cards and the time; no bar on the end screen.
        let summary = element(app, "review.session.summary")
        XCTAssertTrue(summary.exists)
        XCTAssertTrue(summary.label.hasPrefix("4"), "the end of the session must state the four cards, got \(summary.label)")
        XCTAssertFalse(element(app, "review.session.progress").exists)
        return answers
    }

    private func startFreshReview(extra: [String] = []) throws -> (XCUIApplication, String) {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("generated fixtures require a hook build") }
        let bundleIdentifier = try build.bundleIdentifier
        guard bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("development container required") }
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = launchArguments(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"), extra: extra)
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        try openDeck(app, bundleIdentifier: bundleIdentifier)
        let freshCounts = waitForDeckCounts(app, due: 4)
        if !freshCounts { attach(app, "review-fresh-counts") }
        XCTAssertTrue(freshCounts, "first launch must seed four due text cards")
        try click(element(app, "home.deck.study"), in: app, bundleIdentifier: bundleIdentifier)
        return (app, bundleIdentifier)
    }

    private func assertSavedAndRestart(_ app: XCUIApplication, bundleIdentifier: String, answers: Int, evidence: String, extra: [String] = [], due: Int = 0) throws {
        assertSavedOutcome(app, answers: answers, evidence: "\(evidence)-closed", due: due)
        app.terminate()
        // No fixture and no study input: reuse the database without replaying answers.
        app.launchArguments = launchArguments(extra: extra)
        app.launch()
        XCTAssertTrue(element(app, "home.session.recorded").waitForExistence(timeout: 20))
        try openDeck(app, bundleIdentifier: bundleIdentifier)
        assertSavedOutcome(app, answers: answers, evidence: "\(evidence)-restarted", due: due)
    }

    private func assertSavedOutcome(_ app: XCUIApplication, answers: Int, evidence: String, due: Int = 0) {
        let expectedCaption = answers == 1 ? "1 answer recorded" : "\(answers) answers recorded"
        let recorded = waitForText(element(app, "home.session.recorded"), expectedCaption)
        // Home can appear before finishReview's asynchronous owner refresh applies.
        // Later-today learning is outside these counts; only the waiting case
        // must show the separate Waiting for Learning state before Close.
        let counts = waitForDeckCounts(app, due: due)
        attach(app, "review-\(evidence)")
        XCTAssertTrue(recorded, "saved record must contain exactly the accepted, non-undone answers")
        XCTAssertTrue(counts, "authoritative deck counts must reflect the reviews before and after restart")
        XCTAssertFalse(element(app, "home.collection.error").exists)
    }

    private func launchArguments(fixture: URL? = nil, extra: [String] = []) -> [String] {
        MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false, extra: extra).arguments
    }

    private func openDeck(_ app: XCUIApplication, bundleIdentifier: String) throws {
        // N14: the tab layout's deck list row, or the sidebar's row where the detail shows Today.
        let rows = HomeControls.deckRows(app, labelContaining: "Cards")
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 20), "Generate and stage local-review-1 in the VM before this suite")
        try click(HomeControls.hittable(rows) ?? rows.firstMatch, in: app, bundleIdentifier: bundleIdentifier)
        // The page, not Study: a deck with nothing waiting offers Add Note in its place (N14.3).
        let page = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 10))
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

    private func waitForDeckCounts(_ app: XCUIApplication, due: Int) -> Bool {
        // N14.3, N14.4: nothing waiting replaces the counts with one line, and the Mac
        // shows the counts as the window's subtitle; HomeControls reads all three forms.
        var ready = HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: due)
        XCTAssertTrue(ready, "The deck page must show exactly 0 new, 0 learning and \(due) to review")
        #if !os(macOS)
        guard due > 0 else { return ready }
        // Where touch pages show the counts, each stays a unique labelled numeric field.
        for (name, label, value) in [("new", "New", 0), ("learning", "Learning", 0), ("due", "To Review", due)] {
            let fields = app.staticTexts.matching(identifier: "home.deck.counts.\(name)")
            let predicate = NSPredicate(format: "exists == true AND label == %@ AND value == %@", label, String(value))
            let matched = XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: fields.firstMatch)], timeout: 20) == .completed
            XCTAssertEqual(fields.count, 1, "Each deck count must be a unique numeric accessibility field")
            XCTAssertTrue(matched, "\(label) must expose the exact numeric value \(value)")
            ready = ready && matched
        }
        #endif
        return ready
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
