@testable import CardRendering
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct CardDialogTests {
        @Test func idleDialogsFinishInsideRealScriptsAndSettleWithoutReplay() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let update = try await Spike.deadline("idle dialogs must not deadlock settling", timeout: .seconds(3)) {
                try await fixture.pair.prepare(.init(cardID: 1, question: """
                    <script>alert('idle');window.confirmed=confirm('idle');window.prompted=prompt('idle');window.afterDialogs=true;</script>
                    """, answer: "a"))
            }
            let idle = fixture.pair.idle
            let neutral: Bool? = try await idle.webView.callAsyncJavaScript("return afterDialogs && confirmed===false && prompted===null;", contentWorld: .page) as? Bool
            #expect(neutral == true && idle.settled && fixture.presenter.dialogs.isEmpty)
            _ = try fixture.pair.present(update)
            try await idle.waitForShown(update)
            #expect(fixture.presenter.dialogs.isEmpty)
        }

        @Test func visibleDialogsReturnChoiceAndEmptyPromptThenSuspendCompletesOnce() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            let page = fixture.pair.current
            let confirm = Task { try await page.string("return String(confirm('visible')); ") }
            try await CardBehaviorFixture.until("visible confirm absent") { fixture.presenter.dialogs.count == 1 }
            fixture.presenter.replies[0](.confirm(true))
            #expect(try await confirm.value == "true")
            let prompt = Task { try await page.string("return JSON.stringify(prompt('empty', 'seed')); ") }
            try await CardBehaviorFixture.until("visible prompt absent") { fixture.presenter.dialogs.count == 2 }
            fixture.presenter.replies[1](.prompt(""))
            #expect(try await prompt.value == "\"\"")
            let stale = Task { try await page.string("return JSON.stringify(prompt('stale')); ") }
            try await CardBehaviorFixture.until("stale prompt absent") { fixture.presenter.dialogs.count == 3 }
            await fixture.pair.suspend()
            #expect(try await stale.value == "null")
            fixture.presenter.replies[2](.prompt("too late"))
            fixture.presenter.replies[2](.prompt("twice"))
            #expect(fixture.presenter.dismissals == 1)
            // A suspended visible page cannot queue a second dialog.
            #expect(try await page.string("return String(confirm('suspended')); ") == "false")
            #expect(fixture.presenter.dialogs.count == 3)
        }

        @Test func replacementCloseAndUnavailablePresenterDrainNeutralCallbacks() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            var results: [CardDialogResult?] = []
            let page = fixture.pair.current
            page.behavior.dialog(.prompt("replace", defaultText: nil)) { results.append($0) }
            _ = try await fixture.pair.showAnswer()
            fixture.presenter.replies[0](.prompt("late"))
            #expect(results.count == 1 && results[0] == nil)
            fixture.presenter.isAvailable = false
            #expect(try await page.string("return String(confirm('unavailable')); ") == "false")
            fixture.presenter.isAvailable = true
            page.behavior.dialog(.alert("close")) { results.append($0) }
            fixture.pair.close()
            fixture.presenter.replies[1](.alert)
            #expect(results.count == 2 && results[1] == nil)
        }

        @Test func actualNativePresenterWithoutAHostReturnsImmediately() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let native = NativeCardBehaviorPresenter()
            fixture.pair.behaviorPresenter = native
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            #expect(!native.isAvailable)
            let result = try await Spike.deadline("unavailable native presenter", timeout: .seconds(3)) {
                try await fixture.pair.current.string("alert('a');return JSON.stringify([confirm('c'),prompt('p')]);")
            }
            #expect(result == "[false,null]")
        }
    }
}
