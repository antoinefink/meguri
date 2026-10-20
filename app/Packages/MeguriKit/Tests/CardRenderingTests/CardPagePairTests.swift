@testable import CardRendering
import CoreBridge
import Foundation
import Instrumentation
import Testing
import WebKit
#if os(macOS)
import AppKit
#endif

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct CardPagePairTests {
        func make(_ fixture: MediaFixture, configuration: CardWebConfiguration? = nil,
                  compare: @escaping (TypeAnswerDescriptor, String) async throws -> String = { _, text in "<code>\(text)</code>" }) async throws -> CardPagePair {
            CardPagePair(configuration: configuration ?? CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved(), compare: compare)
        }
        @Test func samePageAnswerLiveAppearanceStorageAndPairScopedScripts() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let pair = try await make(fixture, configuration: configuration)
            let other = try await make(fixture, configuration: configuration)
            let window = SpikeWindow(pair.container), otherWindow = SpikeWindow(other.container)
            defer { pair.close(); other.close(); window.close(); otherWindow.close() }
            try await pair.load(); try await other.load()
            try await pair.installHostScript("window.pairOnly = 1;")
            let hostWorld = CardPage.world
            #expect(try await other.current.webView.callAsyncJavaScript("return typeof pairOnly;", contentWorld: hostWorld) as? String == "undefined")
            let payload = CardPayload(cardID: 9, question: "<input id=typeans><script>window.sides=(window.sides||0)+1;localStorage.setItem('shared','yes');sessionStorage.setItem('page','yes')</script>",
                                      answer: "<span data-meguri-type-answer></span><script>window.sides+=1</script>",
                                      typeIn: .init(expected: "typed"))
            let update = try await pair.prepare(payload)
            _ = try pair.present(update)
            try await Task.sleep(for: .milliseconds(100))
            let page = pair.current.webView
            #expect(pair.idle.webView.idle && !page.idle)
            #if os(macOS)
            #expect(pair.idle.webView.isAccessibilityHidden())
            #expect(!pair.idle.webView.canBecomeKeyView)
            #else
            #expect(pair.idle.webView.accessibilityElementsHidden)
            #endif
            _ = try await page.callAsyncJavaScript("document.getElementById('typeans').value='typed';", contentWorld: .page)
            try await pair.setAppearance(.dark)
            try await pair.setCardState(flag: 2, marked: true)
            #expect(try await page.callAsyncJavaScript("return !document.getElementById('_flag').hidden && !document.getElementById('_mark').hidden && document.getElementById('_flag').style.color==='var(--flag-2)';", contentWorld: .page) as? Bool == true)
            #expect(try await page.callAsyncJavaScript("return window.sides;", contentWorld: .page) as? Int == 1)
            #expect(try await page.callAsyncJavaScript("return document.documentElement.classList.contains('night-mode');", contentWorld: .page) as? Bool == true)
            _ = try await pair.showAnswer()
            try await Task.sleep(for: .milliseconds(100))
            #expect(pair.current.webView === page)
            #expect(try await page.callAsyncJavaScript("return document.querySelector('code').textContent;", contentWorld: .page) as? String == "typed")
            #expect(try await page.callAsyncJavaScript("return window.sides;", contentWorld: .page) as? Int == 2)
            #expect(try await other.current.webView.callAsyncJavaScript("return localStorage.getItem('shared');", contentWorld: .page) as? String == "yes")
            // WebKit's null bridge is NSNull; assert the JavaScript contract
            // directly so a shared value (or undefined) cannot pass.
            #expect(try await other.current.webView.callAsyncJavaScript("return sessionStorage.getItem('page') === null;", contentWorld: .page) as? Bool == true)
            #expect(try await page.callAsyncJavaScript("return sessionStorage.getItem('page');", contentWorld: .page) as? String == "yes")
            #expect(try await page.callAsyncJavaScript("return typeof pairOnly;", contentWorld: .page) as? String == "undefined")
            let replacement = CardPlatformView(frame: pair.container.frame)
            pair.container.removeFromSuperview(); replacement.addSubview(pair.container)
            #expect(pair.current.webView === page && pair.pages.count == 2)
        }

        @Test func overlappingPrepareRejectsStaleSignalsAndDefersOnlyMatchingActions() async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            var actions: [CardBridgeAction] = []
            pair.onAction = { actions.append($0) }
            let old = Task { try await pair.prepare(CardPayload(cardID: 1, question: "<script>onUpdateHook.push(()=>new Promise(r=>setTimeout(r,60)));if(typeof pycmd==='function')pycmd('ans')</script>", answer: "a")) }
            try await Task.sleep(for: .milliseconds(20))
            let fresh = try await pair.prepare(CardPayload(cardID: 2, question: "<p id=question>fresh question</p><script>if(typeof pycmd==='function')pycmd('ans')</script>", answer: "a"))
            await #expect(throws: (any Error).self) { try await old.value }
            #expect(actions.isEmpty)
            let committed = AsyncStream<CardUpdate>.makeStream(bufferingPolicy: .bufferingOldest(1))
            defer { committed.continuation.finish() }
            pair.onCommitted = { update, _ in
                #expect(update == fresh)
                if update == fresh { committed.continuation.yield(update) }
            }
            _ = try pair.present(fresh)
            let presented = try await Spike.deadline("matching overlapping prepare commit", timeout: .seconds(3)) {
                for await update in committed.stream { return update }
                throw Spike.Failure("missing fresh commit callback")
            }
            #expect(presented == fresh && pair.current.update == fresh)
            #if os(macOS)
            #expect(actions == [.reveal])
            #else
            #expect(actions.isEmpty && pair.current.update?.side == .question)
            #expect(try await pair.current.webView.callAsyncJavaScript("return typeof pycmd==='undefined' && document.getElementById('question').textContent==='fresh question';", contentWorld: .page) as? Bool == true)
            #endif
            let cancelled = Task { try await pair.prepare(CardPayload(cardID: 3, question: "<p>cancelled</p>", answer: "a")) }
            cancelled.cancel()
            await #expect(throws: (any Error).self) { try await cancelled.value }
            #expect(pair.idle.update == nil)
        }

        @Test func delayedObserverKeepsEmissionIdentityAndArrivalPreservesDOM() async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let first = try await pair.prepare(CardPayload(cardID: 1, question: "<input id=typeans><img id=late src=late.png><script>window.runs=(window.runs||0)+1</script>", answer: "a"))
            _ = try pair.present(first)
            try await Task.sleep(for: .milliseconds(100))
            let page = pair.current.webView
            let nativePage = pair.current
            try await nativePage.run("""
                (function () {
                  const handler = webkit.messageHandlers.meguriCard;
                  const post = handler.postMessage;
                  window.delayedMessages = [];
                  window.testSignalHandler = handler;
                  handler.postMessage = function (message) {
                    if (message.signal === 'typedText') delayedMessages.push(message);
                    else post.call(handler, message);
                  };
                  window.flushDelayed = function () { for (const message of delayedMessages) post.call(handler, message); };
                })();
                """, world: CardPage.world)
            _ = try await page.callAsyncJavaScript("const input=document.getElementById('typeans');input.value='retained';input.dispatchEvent(new Event('input',{bubbles:true}));", contentWorld: .page)
            #expect(try await page.callAsyncJavaScript("return delayedMessages.length===1 && delayedMessages[0].card===card && delayedMessages[0].generation===generation && delayedMessages[0].side==='question';",
                                                     arguments: ["card": first.card, "generation": first.generation], contentWorld: CardPage.world) as? Bool == true)
            fixture.add("/late.png", try SpikeAssets.solidPNG(), "image/png")
            try await pair.mediaArrived(["late.png"])
            try await Task.sleep(for: .milliseconds(100))
            #expect(try await page.callAsyncJavaScript("return document.getElementById('late').naturalWidth > 0;", contentWorld: .page) as? Bool == true)
            #expect(try await page.callAsyncJavaScript("return window.runs;", contentWorld: .page) as? Int == 1)
            #expect(try await page.callAsyncJavaScript("return document.getElementById('typeans').value;", contentWorld: .page) as? String == "retained")
            let next = try await pair.prepare(CardPayload(cardID: 2, question: "two", answer: "a"))
            _ = try pair.present(next)
            try await Task.sleep(for: .milliseconds(100))
            let third = try await pair.prepare(CardPayload(cardID: 3, question: "three", answer: "a"))
            _ = try pair.present(third)
            try await Task.sleep(for: .milliseconds(100))
            var state: [String] = []
            pair.onInteraction = { signal, _ in state.append(signal) }
            // A real observer post emitted by card1 is delivered only after
            // that same page presents card3; native must reject its old id.
            try await nativePage.run("flushDelayed();", world: CardPage.world)
            try await Task.sleep(for: .milliseconds(40))
            #expect(!state.contains("typedText"))
        }

        @Test func lifecycleResumeDoesNotRestartHTMLPlaybackHeldInBackground() async throws {
            let fixture = MediaFixture()
            fixture.add("/clip.mp4", try await TestMedia.mp4(), "video/mp4")
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let prepared = try await pair.prepare(CardPayload(cardID: 1, question: "<video id=media src=clip.mp4 muted playsinline></video>", answer: "answer"))
            _ = try pair.present(prepared)
            try await Task.sleep(for: .milliseconds(100))
            await pair.suspend()
            let page = pair.current.webView
            _ = try await page.callAsyncJavaScript("await document.getElementById('media').play().catch(()=>{});", contentWorld: .page)
            await pair.resume()
            #expect(try await page.callAsyncJavaScript("return document.getElementById('media').paused;", contentWorld: .page) as? Bool == true)
            _ = try await page.callAsyncJavaScript("await document.getElementById('media').play();", contentWorld: .page)
            #expect(try await page.callAsyncJavaScript("return document.getElementById('media').paused;", contentWorld: .page) as? Bool == false)
        }

        @Test func sessionPausePreservesReviewEligibilityAndAvailabilityDoesNotRestartHTML() async throws {
            let fixture = MediaFixture()
            fixture.add("/clip.mp4", try await TestMedia.mp4(), "video/mp4")
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let prepared = try await pair.prepare(CardPayload(cardID: 1,
                question: "<video id=media src=clip.mp4 muted playsinline></video>", answer: "answer"))
            _ = try pair.present(prepared)
            try await pair.current.waitForShown(prepared)
            let page = pair.current.webView
            _ = try await page.callAsyncJavaScript("await document.getElementById('media').play();", contentWorld: .page)
            AudioSessionCoordinator.shared.emit(.pause)
            for _ in 0..<100 {
                if try await page.callAsyncJavaScript("return document.getElementById('media').paused;", contentWorld: .page) as? Bool == true { break }
                try await Task.sleep(for: .milliseconds(10))
            }
            #expect(try await page.callAsyncJavaScript("return document.getElementById('media').paused;", contentWorld: .page) as? Bool == true)
            #expect(pair.current.presented && pair.current.update == prepared)
            let next = CardPayload(cardID: 2, question: "next", answer: "answer")
            _ = try await pair.prepare(next) // A pause leaves explicit review actions eligible.

            _ = try await page.callAsyncJavaScript("await document.getElementById('media').play();", contentWorld: .page)
            AudioSessionCoordinator.shared.emit(.stop)
            AudioSessionCoordinator.shared.emit(.pause) // Inactivity cannot clear a background stop.
            for _ in 0..<100 {
                if !pair.current.presented { break }
                try await Task.sleep(for: .milliseconds(10))
            }
            try await Task.sleep(for: .milliseconds(40))
            #expect(!pair.current.presented)
            do {
                _ = try await pair.prepare(next)
                Issue.record("A pause after a lifecycle stop must leave preparation blocked")
            } catch CardPage.Failure.closed {
                // The lifecycle stop still blocks explicit review actions.
            } catch {
                Issue.record("Expected lifecycle closure, received \(error)")
            }

            AudioSessionCoordinator.shared.emit(.available)
            for _ in 0..<100 {
                if pair.current.presented { break }
                try await Task.sleep(for: .milliseconds(10))
            }
            #expect(pair.current.presented && pair.current.update == prepared)
            #expect(try await page.callAsyncJavaScript("return document.getElementById('media').paused;", contentWorld: .page) as? Bool == true)
            _ = try await page.callAsyncJavaScript("await document.getElementById('media').play();", contentWorld: .page)
            #expect(try await page.callAsyncJavaScript("return document.getElementById('media').paused;", contentWorld: .page) as? Bool == false)
            _ = try await pair.showAnswer()
            #expect(pair.current.update?.side == .answer)
        }

        @Test func explicitCloseReleasesPairAndHandlersDoNotOwnIt() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            var pair: CardPagePair? = try await make(fixture, configuration: configuration)
            weak let weakPair = pair
            weak let weakPage = pair?.current
            pair?.close(); pair = nil
            #expect(weakPair == nil && weakPage == nil)
            #expect(CardWebConfiguration.storeIdentifier != UUID(uuidString: "6D656775-7269-5350-494B-453230303032"))
        }

        @Test func aThousandCardsKeepBothSidesOnTheirOriginalTwoPages() async throws {
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved(),
                                    makeWebView: { SpikeWebView(frame: $0, configuration: $1) })
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let pages = pair.pages.map(\.webView)
            let identities = pages.map(ObjectIdentifier.init)
            let created = SpikeWebView.created.load(ordering: .relaxed)
            for card in 1...1000 {
                let html = "<p>synthetic</p><script>window.sideCount=(window.sideCount||0)+1</script>"
                let question = try await pair.prepare(CardPayload(cardID: Int64(card), question: html, answer: html))
                let target = pair.idle
                _ = try pair.present(question)
                try await target.waitForShown(question)
                _ = try await pair.showAnswer()
                let answer = try #require(target.update)
                try await target.waitForShown(answer)
                #expect(pair.current === target && answer.card == Int64(card) && answer.side == .answer)
            }
            #expect(pair.pages.map { ObjectIdentifier($0.webView) } == identities)
            #expect(SpikeWebView.created.load(ordering: .relaxed) == created)
            #expect(pair.container.subviews.count == 2)
            for page in pages {
                let measured = try #require(page as? SpikeWebView)
                #expect(measured.windowMoves == 1)
                #expect(try await measured.string("return String(window.sideCount);") == "1000")
            }
        }

        @Test func productionGlobalsAndForeignFramesCannotInvokeActions() async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            let baselineConfiguration = CardWebConfiguration(store: .nonPersistent(), media: fixture.handler)
            let baseline = SpikeWebView(frame: pair.container.frame, configuration: baselineConfiguration.configuration)
            let baselineWindow = SpikeWindow(baseline)
            defer { fixture.handler.unbind(baseline); baselineWindow.close() }
            let skeleton = CardRuntime.pageDocument(platform: PlatformClasses.current, appearance: .light, mediaRoot: MediaOrigin.url)
                .split(separator: "\n", omittingEmptySubsequences: false).filter { !$0.hasPrefix("<script src=\"/_anki/") }.joined(separator: "\n")
            try await baselineConfiguration.ready()
            try await fixture.bind(baseline, document: skeleton)
            try await baseline.open(MediaOrigin.url)
            pair.current.webView.configuration.userContentController.addUserScript(WKUserScript(
                source: "if(window.top!==window)webkit.messageHandlers.meguriCard.postMessage({signal:'command',card:1,generation:1,side:'question',command:'ans'});",
                injectionTime: .atDocumentStart, forMainFrameOnly: false, in: CardPage.world))
            try await pair.load()
            let baseNames = try await baseline.json([String].self, "return JSON.stringify(Object.getOwnPropertyNames(window));")
            #expect(try await pair.current.string("return document.documentElement.className;") == CardRuntime.htmlClasses(platform: PlatformClasses.current, appearance: .light))
            let agent = try await pair.current.string("return navigator.userAgent;")
            #expect(!agent.contains("Chrome"))
            if PlatformClasses.current == .iPad { #expect(agent.contains("iPad")) }
            let actualText = try await pair.current.string("return JSON.stringify(Object.getOwnPropertyNames(window));")
            let actualNames = try JSONDecoder().decode([String].self, from: Data(actualText.utf8))
            var expected: Set<String> = ["$", "jQuery", "anki", "require", "onUpdateHook", "onShownHook", "_showQuestion", "_showAnswer", "_updateQA", "_drawFlag", "_drawMark", "getTypedAnswer", "_typeAnsPress", "MathJax"]
            #if os(macOS)
            expected.formUnion(["ankiPlatform", "pycmd", "bridgeCommand"])
            #endif
            #expect(Set(actualNames).subtracting(baseNames) == expected)
            #expect(Set(baseNames).subtracting(actualNames).isEmpty)
            var actions: [CardBridgeAction] = []
            pair.onAction = { actions.append($0) }
            let update = try await pair.prepare(CardPayload(cardID: 1, question: "<iframe srcdoc=\"<p id=loaded>Untrusted frame</p><script>window.frameRan=true;parent.postMessage('untrusted','*')</script>\"></iframe><script>if(typeof pycmd!=='undefined')pycmd('ease3')</script>", answer: "answer"))
            _ = try pair.present(update)
            let page = pair.current.webView
            try await CardBehaviorFixture.until("foreign frame did not execute its srcdoc") {
                try await page.callAsyncJavaScript("const f=document.querySelector('iframe');return f.contentDocument?.getElementById('loaded')?.textContent==='Untrusted frame' && f.contentWindow.frameRan===true;", contentWorld: .page) as? Bool == true
            }
            try await CardBehaviorFixture.until("loaded foreign frame did not exercise bridge rejection") {
                pair.diagnostics.entries.contains { $0.kind == "bridgeOrigin" }
            }
            #expect(actions.isEmpty)
            let result = try await page.callAsyncJavaScript("return ['meguriCard','showAnswer','AnkiDroidJS','buttonAnswerEase1'].every(k=>typeof window[k]==='undefined');", contentWorld: .page) as? Bool
            #expect(result == true)
            #if os(macOS)
            #expect(try await page.callAsyncJavaScript("return typeof webkit;", contentWorld: .page) as? String == "undefined")
            #endif
            #expect(pair.diagnostics.entries.contains { $0.kind == "bridgeOrigin" })
        }

        #if os(macOS)
        @Test func swapHonorsNativeEditorFocusOwnership() async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let editor = NSTextField(frame: .zero)
            pair.container.addSubview(editor)
            _ = pair.container.window?.makeFirstResponder(editor)
            let before = try #require(pair.container.window?.firstResponder)
            #expect(editor.currentEditor() === before)
            pair.ownsKeyboardFocus = false
            let prepared = try await pair.prepare(CardPayload(cardID: 1, question: "<input id=typeans>", answer: "a"))
            _ = try pair.present(prepared)
            #expect(pair.container.window?.firstResponder === before)
            #expect(pair.idle.webView.idle && !pair.idle.webView.acceptsFirstResponder)
            try await Task.sleep(for: .milliseconds(100))
            #expect(pair.container.window?.firstResponder === before)
            #expect(try await pair.current.string("return document.activeElement.id;") != "typeans")
            // Explicit card interaction still focuses and retains typed input.
            try await pair.current.run("document.getElementById('typeans').focus(); document.getElementById('typeans').value='explicit';")
            #expect(await pair.current.typedText() == "explicit")
            #expect(try await pair.current.string("return document.activeElement.id;") == "typeans")
        }

        @Test func ownedPresentationStillAutomaticallyFocusesTypeAnswer() async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            pair.ownsKeyboardFocus = true
            let update = try await pair.prepare(CardPayload(cardID: 2, question: "<input id=typeans>", answer: "a"))
            _ = try pair.present(update)
            try await pair.current.waitForShown(update)
            #expect(try await pair.current.string("return document.activeElement.id;") == "typeans")
            #expect(!pair.current.webView.idle && pair.current.webView.reviewOwnsFocus)
            let responder = try #require(pair.container.window?.firstResponder as? NSView)
            #expect(responder === pair.current.webView || responder.isDescendant(of: pair.current.webView))
            #expect(!pair.idle.webView.canBecomeKeyView)
        }
        #endif

        #if !os(macOS)
        @Test func loopbackRejectedBeforeNavigationAndNoExtraPublicHandlers() async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let web = pair.current.webView
            for url in ["http://127.0.0.1:8765", "http://127.1:8765", "http://2130706433:8765",
                        "http://0x7f000001:8765", "http://0177.0.0.1:8765", "http://[::1]:8765",
                        "http://[0:0:0:0:0:0:0:1]:8765", "http://[::ffff:127.0.0.1]:8765",
                        "http://[::ffff:7fff:fffe]:8765"] {
                let result = try await web.callAsyncJavaScript("const start=performance.now();try{await fetch(url);return -1;}catch(e){return performance.now()-start;}",
                                                              arguments: ["url": url], contentWorld: .page) as? Double
                let elapsed = try #require(result)
                #expect(elapsed >= 0 && elapsed < 50, "\(url)")
            }
            #expect(try await web.callAsyncJavaScript("return typeof webkit.messageHandlers.cb.postMessage==='function' && typeof webkit.messageHandlers.meguri.postMessage==='function' && !webkit.messageHandlers.meguriCard;", contentWorld: .page) as? Bool == true)
        }
        #endif
    }
}

extension WebKitSuites.CardPagePairTests {
    @Test func identitylessMediaObserversKeepBothSidesWithoutInventingIdentity() async throws {
        try await WebTestHarness.withPage(log: false) { harness, page async throws in
            try await page.showPresentedQuestion("<img src='_question-missing.png'>")
            let question = try await harness.signal("mediaError", from: page)
            #expect(question.side == "question" && question["card"] == nil && question["generation"] == nil)
            try await page.showAnswer("<img src='_answer-missing.png'>")
            let answer = try await harness.signal("mediaError", count: 2, from: page)
            #expect(answer.side == "answer" && answer["card"] == nil && answer["generation"] == nil)
            #expect((answer["url"] as? String)?.hasSuffix("/_answer-missing.png") == true)
        }
    }
}
