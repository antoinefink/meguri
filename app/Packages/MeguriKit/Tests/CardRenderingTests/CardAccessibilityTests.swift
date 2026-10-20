@testable import CardRendering
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct CardAccessibilityTests {
        #if os(macOS)
        @Test func reportedVisibleOffscreenWindowCannotAuthorizeNativeEffects() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "q", answer: "<h2 id=answer>Isolated answer</h2>"))
            let page = fixture.pair.current
            var posts = 0
            page.accessibilityEffects.post = { _ in posts += 1 }
            // The default native gate is independent of the fake presenter
            // and the fixture window's deliberately reported visibility.
            #expect(fixture.presenter.isActive)
            #expect(!CardAccessibilityEffects().isActive(page.webView))
            _ = try await fixture.pair.showAnswer()
            #expect(page.accessibilityTarget?.elementID == "answer")
            #expect(posts == 0)
        }

        #endif

        @Test func realWebKitHitTestingTargetsAnswerSeparatorAndFallbackAtNativeZoom() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let answers = ["<h2 id=answer>Native answer</h2>",
                           "<div style='height:1600px'>Before separator</div><hr id=answer><h2>Native answer</h2>",
                           "<h2>Native answer</h2><p>After target</p>"]
            fixture.pair.cardZoom = CardZoom(percent: 150)
            for (index, html) in answers.enumerated() {
                try await fixture.present(.init(cardID: Int64(index + 1), question: "q", answer: html))
                let page = fixture.pair.current
                var posted: Any?
                // Capture only. Never post to host VO or activate a window.
                page.accessibilityEffects = CardAccessibilityEffects(isActive: { _ in true }, post: { posted = $0 })
                fixture.pair.ownsKeyboardFocus = false
                #if !os(macOS)
                page.webView.scrollView.setZoomScale(2, animated: false)
                #endif
                _ = try await fixture.pair.showAnswer()
                #expect(page.accessibilityTarget?.elementID == (index == 2 ? "qa" : "answer"))
                let element = try #require(posted as? NSObject, "native hit testing must resolve a concrete answer element")
                #if os(macOS)
                let accessible = try #require(element as? any NSAccessibilityProtocol)
                let nativeText = (accessible.accessibilityValue() as? String)
                    ?? accessible.accessibilityTitle() ?? ""
                #else
                let nativeText = element.accessibilityLabel ?? element.accessibilityValue ?? ""
                #endif
                #expect(nativeText.contains("Native answer"), "the concrete native target must identify the answer, not a generic web area")
                #expect(try await fixture.bool("return document.activeElement.id!=='answer' && document.activeElement.id!=='typeans';") == true)
            }
        }

        @Test func lateRevealCannotInstallOrAnnounceAnOldAnswer() async throws {
            var comparison: CheckedContinuation<String, Never>?
            let fixture = try await CardBehaviorFixture(compare: { _, _ in
                await withCheckedContinuation { comparison = $0 }
            })
            defer { comparison?.resume(returning: ""); fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "<input id=typeans>", answer: "<h2 id=answer>Old answer</h2><span data-meguri-type-answer></span>", typeIn: .init(expected: "ka")))
            let old = fixture.pair.current
            var announcements = 0
            old.onAccessibilityTarget = { _ in announcements += 1 }
            let reveal = Task { try await fixture.pair.showAnswer() }
            try await CardBehaviorFixture.until("comparison did not start") { comparison != nil }
            try await fixture.present(.init(cardID: 2, question: "<p id=fresh>Current question</p>", answer: "a"))
            let pending = comparison; comparison = nil
            pending?.resume(returning: "<code>ka</code>")
            await #expect(throws: CardPage.Failure.self) { try await reveal.value }
            #expect(announcements == 0 && fixture.pair.current.accessibilityTarget == nil && old.accessibilityTarget == nil)
            #expect(try await fixture.bool("return document.getElementById('fresh').textContent==='Current question' && !document.getElementById('answer');") == true)
        }

        @Test func nativeEditorKeepsItsActualResponderAcrossRevealAndRecovery() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "<input id=typeans>", answer: "<h2 id=answer>Answer</h2>"))
            fixture.pair.ownsKeyboardFocus = false
            #if os(macOS)
            let field = NSTextField(string: "native editor")
            field.frame = CGRect(x: 0, y: 0, width: 200, height: 44)
            fixture.pair.container.addSubview(field)
            let window = try #require(fixture.pair.container.window)
            #expect(window.makeFirstResponder(field))
            let responder = window.firstResponder
            #else
            let field = UITextField(frame: CGRect(x: 0, y: 0, width: 200, height: 44))
            fixture.pair.container.addSubview(field)
            #expect(field.becomeFirstResponder())
            #endif
            _ = try await fixture.pair.showAnswer()
            #if os(macOS)
            #expect(window.firstResponder === responder)
            #else
            #expect(field.isFirstResponder)
            #endif
            var recovered = false
            fixture.pair.onRecovery = { _, _ in recovered = true }
            fixture.pair.current.webViewWebContentProcessDidTerminate(fixture.pair.current.webView)
            try await CardBehaviorFixture.until("focus recovery missing") { recovered }
            #if os(macOS)
            #expect(window.firstResponder === responder)
            #else
            #expect(field.isFirstResponder)
            #endif
        }
        @Test func answerTargetAndFallbackExposeOnlyCurrentPageWithoutKeyboardFocus() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            for marker in [true, false] {
                try await fixture.present(.init(cardID: marker ? 1 : 2, question: "<input id=typeans><p>question</p>",
                                                answer: (marker ? "<h2 id=answer>Answer start</h2>" : "") + "<p>Answer body</p>"))
                let page = fixture.pair.current
                fixture.pair.ownsKeyboardFocus = false
                _ = try await fixture.pair.showAnswer()
                #expect(page.accessibilityTarget?.elementID == (marker ? "answer" : "qa"))
                #expect(page.accessibilityTarget?.text.contains(marker ? "Answer start" : "Answer body") == true)
                #expect(page.accessibilityTarget?.update == page.update)
                #expect(try await fixture.bool("return document.activeElement.id!=='answer' && document.activeElement.id!=='typeans';") == true)
                #if os(macOS)
                #expect(!page.webView.isAccessibilityHidden() && fixture.pair.idle.webView.isAccessibilityHidden())
                #expect(!fixture.pair.idle.webView.canBecomeKeyView)
                #else
                #expect(!page.webView.accessibilityElementsHidden && fixture.pair.idle.webView.accessibilityElementsHidden)
                #endif
                await fixture.pair.suspend()
                let saved = page.accessibilityTarget
                await page.announceAnswer()
                #expect(page.accessibilityTarget == saved)
                await fixture.pair.resume()
            }
        }
    }

    @Suite(.timeLimit(.minutes(2))) struct WebInspectorPolicyTests {
        @Test func everyDistributionAndSettingUpdatesActualPagesAndRecovery() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            for build in CardInspectionBuild.allCases {
                for enabled in [false, true] {
                    fixture.pair.inspectorPolicy = WebInspectorPolicy(build: build, enabled: enabled)
                    let expected = enabled && [.debug, .profile, .personalRelease].contains(build)
                    #expect(fixture.pair.pages.allSatisfy { $0.webView.isInspectable == expected })
                }
            }
            #expect(!WebInspectorPolicy(build: .unknown, enabled: true).isInspectable)
            #expect(!WebInspectorPolicy(build: .appStore, enabled: true).isInspectable)
            #expect(WebInspectorPolicy(build: .personalRelease).enabled)
            #expect(!WebInspectorPolicy(build: .debug).enabled && !WebInspectorPolicy(build: .profile).enabled)
            fixture.pair.inspectorPolicy = .init(build: .personalRelease, enabled: true)
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            var recovered = false
            fixture.pair.onRecovery = { _, _ in recovered = true }
            fixture.pair.current.webViewWebContentProcessDidTerminate(fixture.pair.current.webView)
            try await CardBehaviorFixture.until("inspector recovery missing") { recovered }
            #expect(fixture.pair.pages.allSatisfy { $0.webView.isInspectable })
            fixture.pair.inspectorPolicy = .init(build: .appStore, enabled: true)
            #expect(fixture.pair.pages.allSatisfy { !$0.webView.isInspectable })
        }
    }
}
