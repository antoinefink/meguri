@testable import CardRendering
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct ProcessRecoveryTests {
        // These exercise the production native termination callback with a
        // controlled invocation, not a real OS process kill or private API.
        @Test func currentQuestionRecoversSilentTextAndPairHostInstallation() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let scripts = fixture.configuration.configuration.userContentController.userScripts.count
            try await fixture.pair.installHostScript("window.hostInstalled=true;window.meguriReviewHost={receive(message){window.lastHostMessage=message;return true;}};")
            try await fixture.present(.init(cardID: 7, question: """
                <input id=typeans><script>
                sessionStorage.setItem('questionRuns',String(Number(sessionStorage.getItem('questionRuns')||0)+1));
                window.inputEvents=0;document.addEventListener('input',()=>inputEvents++);document.addEventListener('change',()=>inputEvents++);
                if(sessionStorage.getItem('questionRuns')==='2') {
                  const property=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');
                  Object.defineProperty(document.getElementById('typeans'),'value',{
                    get(){return property.get.call(this)},
                    set(value){property.set.call(this,value);this.dispatchEvent(new Event('input',{bubbles:true}))}
                  });
                }
                </script>
                """, answer: "a"))
            let page = fixture.pair.current, other = fixture.pair.idle, old = try #require(page.update)
            var values: [CardTypedValue] = [], recoveries: [CardUpdate] = [], actions: [CardBridgeAction] = []
            fixture.pair.onTypedText = { values.append($0) }
            fixture.pair.onRecovery = { _, new in recoveries.append(new) }
            fixture.pair.onAction = { actions.append($0) }
            try await fixture.script("const i=document.getElementById('typeans');i.value='ka';i.dispatchEvent(new Event('input',{bubbles:true}));")
            try await CardBehaviorFixture.until("input was not mirrored") { values.last?.text == "ka" }
            let epoch = page.hostReports.viewRevision
            page.webViewWebContentProcessDidTerminate(page.webView)
            try await CardBehaviorFixture.until("question recovery missing") { recoveries.count == 1 }
            #expect(fixture.pair.current === page && fixture.pair.idle === other && fixture.pair.pages.count == 2)
            #expect(page.update?.card == 7 && page.update?.side == .question && page.update != old)
            #expect(try await fixture.bool("return document.getElementById('typeans').value==='ka' && getTypedAnswer()==='ka' && inputEvents===0 && sessionStorage.getItem('questionRuns')==='2';") == true)
            #expect(page.typedValue?.text == "ka" && values.count == 1 && actions.isEmpty)
            #expect(page.hostReports.viewRevision > epoch && page.hostReports.context?.update == page.update)
            #expect(try await page.string("return String(hostInstalled);", world: CardPage.world) == "true")
            #expect(fixture.configuration.configuration.userContentController.userScripts.count == scripts)
            fixture.configuration.bridge.receive(name: CardRuntime.signalHandlerName, world: CardRuntime.interactionWorldName,
                                                  body: ["signal": "typedText", "card": old.card, "generation": old.generation, "side": old.sideName, "text": "stale"], page: page)
            #expect(page.typedValue?.text == "ka" && values.count == 1)
        }

        @Test func answerRecoveryRetainsComparisonAndDoesNotRerenderQuestion() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 8, question: "<input id=typeans><script>sessionStorage.setItem('qRuns',String(Number(sessionStorage.getItem('qRuns')||0)+1))</script>",
                                            answer: "<hr id=answer><span data-meguri-type-answer></span><script>sessionStorage.setItem('aRuns',String(Number(sessionStorage.getItem('aRuns')||0)+1))</script>",
                                            typeIn: .init(expected: "ka")))
            try await fixture.script("document.getElementById('typeans').value='ka';")
            _ = try await fixture.pair.showAnswer()
            let page = fixture.pair.current, old = page.update
            var recovered = false
            fixture.pair.onRecovery = { _, _ in recovered = true }
            page.webViewWebContentProcessDidTerminate(page.webView)
            try await CardBehaviorFixture.until("answer recovery missing") { recovered }
            #expect(page.update?.side == .answer && page.update != old && page.typedValue?.text == "ka")
            #expect(try await fixture.bool("return document.getElementById('comparison').textContent==='ka' && getTypedAnswer()==='ka' && sessionStorage.getItem('qRuns')==='1' && sessionStorage.getItem('aRuns')==='2';") == true)
        }

        @Test func noTypedObservationPreservesTemplateDefaultButAcceptedEmptyRestoresEmpty() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "<input id=typeans value=seed>", answer: "a"))
            var recovered = 0
            fixture.pair.onRecovery = { _, _ in recovered += 1 }
            #expect(fixture.pair.typedValue == nil)
            let page = fixture.pair.current
            page.webViewWebContentProcessDidTerminate(page.webView)
            try await CardBehaviorFixture.until("default-value recovery missing") { recovered == 1 }
            #expect(try await fixture.bool("return document.getElementById('typeans').value==='seed';") == true)
            try await fixture.script("const i=document.getElementById('typeans');i.value='';i.dispatchEvent(new Event('input',{bubbles:true}));")
            try await CardBehaviorFixture.until("empty input was not mirrored") { fixture.pair.typedValue?.text == "" }
            page.webViewWebContentProcessDidTerminate(page.webView)
            try await CardBehaviorFixture.until("empty-value recovery missing") { recovered == 2 }
            #expect(try await fixture.bool("return document.getElementById('typeans').value==='';") == true)
        }

        @Test func idleDeathIsLazyAndHealthyMissingHostNeverReloads() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "<input id=typeans><script>window.marker={};window.saved=marker;</script>", answer: "a"))
            let page = fixture.pair.current, idle = fixture.pair.idle, update = page.update
            let live = await fixture.pair.probeForegroundLiveness()
            #expect(live.count == 2 && live.allSatisfy { if case .healthy = $0 { true } else { false } })
            #expect(page.update == update && page.hostReports.context == nil)
            #expect(try await fixture.bool("return marker===saved;") == true)
            idle.webViewWebContentProcessDidTerminate(idle.webView)
            #expect(idle.needsReconstruction && !idle.settled && idle.update == nil)
            _ = await fixture.pair.probeForegroundLiveness()
            #expect(idle.needsReconstruction && page.update == update)
            _ = try await fixture.pair.prepare(.init(cardID: 2, question: "new question", answer: "a"))
            #expect(!idle.needsReconstruction && idle.settled)
            // The native timeout seam never declares a hung process dead.
            page.livenessEvaluation = { _ in }
            #expect(await page.probeLiveness(timeoutMs: 20) == .unavailable(update))
            #expect(page.terminations == 0 && page.update == update)
            page.livenessEvaluation = nil
            #expect(await page.probeLiveness() == .healthy(update))
        }

        @Test func backgroundTerminationRestoresQuestionWithoutReplayingMedia() async throws {
            let fixture = try await CardBehaviorFixture(video: true)
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 9, question: "<input id=typeans><video id=media src=clip.mp4 autoplay muted playsinline loop></video>", answer: "a"))
            let page = fixture.pair.current, old = page.update
            try await fixture.script("const i=document.getElementById('typeans');i.value='ka';i.dispatchEvent(new Event('input',{bubbles:true}));await document.getElementById('media').play();")
            try await CardBehaviorFixture.until("typed observation missing") { fixture.pair.typedValue?.text == "ka" }
            await fixture.pair.suspend()
            page.webViewWebContentProcessDidTerminate(page.webView)
            #expect(page.update == old && page.needsReconstruction)
            var recovered = false
            fixture.pair.onRecovery = { _, _ in recovered = true }
            await fixture.pair.resume()
            try await CardBehaviorFixture.until("background recovery missing") { recovered }
            #expect(page.update != old && page.update?.side == .question)
            #expect(try await fixture.bool("return document.getElementById('typeans').value==='ka' && document.getElementById('media').paused;") == true)
        }

        @Test func foregroundErrorUsesTerminationPathAndExpiredProbesCannotRecoverLater() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            let page = fixture.pair.current, old = page.update
            var late: (@MainActor (Result<Bool, any Error>) -> Void)?
            page.livenessEvaluation = { late = $0 }
            #expect(await page.probeLiveness(timeoutMs: 20) == .unavailable(old))
            let error = NSError(domain: WKErrorDomain, code: WKError.Code.webContentProcessTerminated.rawValue)
            late?(.failure(error))
            #expect(page.terminations == 0 && page.update == old)
            page.livenessEvaluation = { $0(.failure(error)) }
            var recovered = false
            fixture.pair.onRecovery = { _, _ in recovered = true }
            _ = await fixture.pair.probeForegroundLiveness()
            page.livenessEvaluation = nil
            try await CardBehaviorFixture.until("foreground error did not recover") { recovered }
            #expect(page.terminations == 1 && page.update != old)
            var waiting = false
            page.livenessEvaluation = { _ in waiting = true }
            let probe = Task { await page.probeLiveness() }
            try await CardBehaviorFixture.until("cancel probe did not start") { waiting }
            fixture.pair.close()
            #expect(await probe.value == .cancelled)
        }

        @Test func recoveryCannotOverwriteNewCardOrReopenClosedPair() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "<input id=typeans><script>onUpdateHook.push(()=>new Promise(r=>setTimeout(r,120)));if(typeof pycmd==='function')pycmd('ans');</script>", answer: "a"))
            let dead = fixture.pair.current
            let old = dead.update
            let fresh = try await fixture.pair.prepare(.init(cardID: 2, question: "<p id=fresh>fresh</p>", answer: "a"))
            var recoveries = 0
            fixture.pair.onRecovery = { _, _ in recoveries += 1 }
            dead.webViewWebContentProcessDidTerminate(dead.webView)
            try await CardBehaviorFixture.until("recovery did not start") { dead.update != old }
            _ = try fixture.pair.present(fresh)
            try await fixture.pair.current.waitForShown(fresh)
            try await fixture.pair.current.run("await new Promise(r=>setTimeout(r,160));")
            #expect(fixture.pair.current.update == fresh && recoveries == 0)
            #expect(try await fixture.bool("return document.getElementById('fresh').textContent==='fresh';") == true)
            fixture.pair.current.webViewWebContentProcessDidTerminate(fixture.pair.current.webView)
            fixture.pair.close()
            try await Task.sleep(for: .milliseconds(30))
            #expect(fixture.pair.pages.allSatisfy(\.closed) && recoveries == 0)
        }
    }
}
