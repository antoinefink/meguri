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

        @Test(arguments: [false, true])
        func answerPresentationKeepsTokenUntilItsOwnUpdate(earlyReveal: Bool) async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let page = pair.idle
            // Observe the actual posts before native admission. Keep WebKit's
            // wrapped handler objects alive, and forward every message unchanged.
            try await page.run("""
                window.paintMessages = [];
                window.paintHandlers = ['meguriCard', 'meguriVisible'].map(function (name) {
                  const handler = webkit.messageHandlers[name];
                  const post = handler.postMessage;
                  handler.postMessage = function (message) {
                    if (message.signal === 'visible' || message.phase === 'visible') paintMessages.push(message);
                    return post.call(handler, message);
                  };
                  return handler;
                });
                """, world: CardPage.world)
            let question = try await pair.prepare(CardPayload(cardID: 9, question: """
                <p id=question>question</p><script>
                onShownHook.push(function () {
                  return new Promise(function (resolve) { window.releaseQuestion = resolve; });
                });
                (function () {
                  const show = _showAnswer;
                  window._showAnswer = function (answer, classes) {
                    window.releaseAnswer = function () { show(answer, classes); };
                  };
                })();
                </script>
                """, answer: "<p id=answer>answer</p>"))
            _ = try pair.present(question)
            // Exercise a reveal as soon as native presentation has begun, while
            // question shown hooks remain held, and the already-shown path.
            var revealing: Task<SwapToken, any Error>?
            if earlyReveal {
                try await CardBehaviorFixture.until("native question presentation began") { page.presented }
                revealing = Task { try await pair.showAnswer() }
            }
            defer { revealing?.cancel() }
            try await CardBehaviorFixture.until("question paint and held shown hook") {
                let held = try await page.webView.callAsyncJavaScript("return typeof releaseQuestion === 'function' && document.getElementById('question') !== null;", contentWorld: .page) as? Bool
                guard held == true else { return false }
                return try await page.webView.callAsyncJavaScript("return paintMessages.length === 2;", contentWorld: CardPage.world) as? Bool == true
            }
            try await page.run("paintMessages.length = 0;", world: CardPage.world)
            try await page.run("releaseQuestion();")
            if !earlyReveal {
                try await page.waitForShown(question)
                revealing = Task { try await pair.showAnswer() }
            }
            let reveal = try #require(revealing)
            try await CardBehaviorFixture.until("answer entry queued behind presentation") {
                try await page.webView.callAsyncJavaScript("return typeof releaseAnswer === 'function';", contentWorld: .page) as? Bool == true
            }
            let answer = try #require(page.update)
            #expect(answer.card == question.card && answer.side == .answer && answer.generation != question.generation)
            // The runtime still has the shown question. Give any wrongly bound
            // visibleReady its full two-frame path before releasing the answer.
            try await page.run("await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));", world: CardPage.world)
            #expect(try await page.string("return JSON.stringify(paintMessages);", world: CardPage.world) == "[]")
            try await page.run("releaseAnswer();")
            let token = try await Spike.deadline("identity-bound answer reveal", timeout: .seconds(3)) { try await reveal.value }
            try await CardBehaviorFixture.until("answer visible and confirmation posts") {
                try await page.webView.callAsyncJavaScript("return paintMessages.length === 2;", contentWorld: CardPage.world) as? Bool == true
            }
            #expect(try await page.webView.callAsyncJavaScript("""
                return JSON.stringify(paintMessages) === JSON.stringify([
                  {signal:'visible', token:token, side:'answer', card:card, generation:generation},
                  {token:token, card:card, generation:generation, side:'answer', phase:'visible'}
                ]);
                """, arguments: ["token": token.sequence, "card": answer.card, "generation": answer.generation], contentWorld: CardPage.world) as? Bool == true)
            try await CardBehaviorFixture.until("native accepted answer paint identity") { page.acceptsVisibleConfirmation() }
            #expect(pair.current === page && page.update == answer)
            #expect(pair.diagnostics.entries.isEmpty)
        }

        #if os(macOS)
        @Test func ownedAccessibilityChildrenFollowSynchronousSwapsAndClearOnClose() async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            let views = pair.pages.map(\.webView)
            func expectCurrentChildren() throws {
                #expect(pair.container.isAccessibilityElement())
                #expect(pair.container.accessibilityRole() == .group)
                #expect(pair.current.webView.accessibilityParent() as? NSView === pair.container)
                let children = try #require(pair.container.accessibilityChildren())
                let navigation = try #require(pair.container.accessibilityChildrenInNavigationOrder())
                #expect(children.count == 1)
                #expect(navigation.count == 1)
                #expect(children.first as? NSView === pair.current.webView)
                #expect(navigation.first as? NSView === pair.current.webView)
                #expect(pair.container.subviews.count == 2)
                #expect(views.allSatisfy { $0.superview === pair.container })
            }
            try expectCurrentChildren()
            try await pair.load()
            for cardID in 1...2 {
                let previousIdle = pair.idle.webView
                let update = try await pair.prepare(.init(cardID: Int64(cardID), question: "question", answer: "answer"))
                _ = try pair.present(update)
                #expect(pair.current.webView === previousIdle)
                try expectCurrentChildren()
                try await pair.current.waitForShown(update)
            }
            pair.close()
            #expect(pair.container.accessibilityChildren()?.isEmpty == true)
            #expect(pair.container.accessibilityChildrenInNavigationOrder()?.isEmpty == true)
        }
        #endif
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
            // that same page presents card3; native must silently ignore it.
            let diagnostics = pair.diagnostics.entries
            try await nativePage.run("flushDelayed();", world: CardPage.world)
            try await Task.sleep(for: .milliseconds(40))
            #expect(!state.contains("typedText"))
            #expect(pair.diagnostics.entries == diagnostics)
        }

        @Test(arguments: [false, true])
        func queuedRuntimeMessagesKeepActualEmissionIdentityAcrossPageReuse(sameCard: Bool) async throws {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let page = pair.idle
            // Retain the actual posts from both producers, not reconstructed
            // messages or identities read from the page after it is reused.
            try await page.run("""
                window.holdOldEmission = true;
                window.queuedEmission = [];
                window.emissionHandlers = ['meguriCard', 'meguriVisible'].map(function (name) {
                  const handler = webkit.messageHandlers[name];
                  const post = handler.postMessage;
                  handler.postMessage = function (message) {
                    if (holdOldEmission && typeof message.card === 'number') {
                      queuedEmission.push({name:name, message:message});
                      // Native prepare/shown readiness must still complete;
                      // retain those actual receipts for delayed redelivery too.
                      if (message.signal !== 'settled' && message.signal !== 'shown') return;
                    }
                    return post.call(handler, message);
                  };
                  return {name:name, handler:handler, post:post};
                });
                window.flushEmission = function () {
                  return Promise.all(queuedEmission.map(function (entry) {
                    const target = emissionHandlers.find(h => h.name === entry.name);
                    return target.post.call(target.handler, entry.message);
                  }));
                };
                """, world: CardPage.world)
            let first = try await pair.prepare(CardPayload(cardID: 11, question: "<input id=typeans>", answer: "answer"))
            let firstToken = try pair.present(first)
            try await page.waitForShown(first)
            try await CardBehaviorFixture.until("held actual settled and visible confirmations") {
                try await page.webView.callAsyncJavaScript("""
                    return queuedEmission.some(e => e.message.phase === 'settled') &&
                           queuedEmission.some(e => e.message.phase === 'visible') &&
                           queuedEmission.some(e => e.message.signal === 'visible');
                    """, contentWorld: CardPage.world) as? Bool == true
            }
            try await page.run("""
                const input = document.getElementById('typeans');
                input.value = 'old typed text';
                input.dispatchEvent(new Event('input', {bubbles:true}));
                """)
            #expect(try await page.webView.callAsyncJavaScript("""
                const messages = queuedEmission.map(e => e.message);
                const paints = messages.filter(m => m.phase);
                return paints.length === 2 && paints.every(m =>
                  Object.keys(m).sort().join(',') === 'card,generation,phase,side,token' &&
                  m.card === card && m.generation === generation && m.side === 'question') &&
                  paints.some(m => m.phase === 'settled' && m.token === generation) &&
                  paints.some(m => m.phase === 'visible' && m.token === token) &&
                  messages.some(m => m.signal === 'typedText' && m.card === card &&
                    m.generation === generation && m.side === 'question' && m.text === 'old typed text');
                """, arguments: ["card": first.card, "generation": first.generation, "token": firstToken.sequence],
                contentWorld: CardPage.world) as? Bool == true)
            try await page.run("holdOldEmission = false;", world: CardPage.world)
            let second = try await pair.prepare(CardPayload(cardID: 12, question: "second", answer: "answer"))
            _ = try pair.present(second)
            try await pair.current.waitForShown(second)
            let preparing = Task {
                try await pair.prepare(CardPayload(cardID: sameCard ? 11 : 13, question: """
                    <p>fresh</p><script>
                    onUpdateHook.push(function () {
                      return new Promise(function (resolve) { window.releaseFreshUpdate = resolve; });
                    });
                    </script>
                    """, answer: "answer"))
            }
            defer { preparing.cancel() }
            try await CardBehaviorFixture.until("superseding runtime update holds readiness") {
                try await page.webView.callAsyncJavaScript("return typeof releaseFreshUpdate === 'function';", contentWorld: .page) as? Bool == true
            }
            let freshIdentity = try #require(page.update)
            #expect(pair.idle === page && freshIdentity.generation > first.generation && !page.settled)
            #expect(freshIdentity.card == (sameCard ? first.card : 13))
            var signals: [String] = []
            var actions: [CardBridgeAction] = []
            var confirmations = 0
            let signalHandler = page.onSignal
            let actionHandler = page.onAction
            let confirmationHandler = page.onConfirmation
            page.onSignal = { _, signal, _ in signals.append(signal) }
            page.onAction = { _, action in actions.append(action) }
            page.onConfirmation = { _, _ in confirmations += 1 }
            let diagnostics = pair.diagnostics.entries
            try await page.run("await flushEmission();", world: CardPage.world)
            #expect(pair.diagnostics.entries == diagnostics && pair.diagnostics.templateErrors.isEmpty)
            #expect(signals.isEmpty && actions.isEmpty && confirmations == 0)
            #expect(page.update == freshIdentity && !page.settled && page.typedValue == nil)
            #expect(page.visibleToken == nil && !page.acceptsVisibleConfirmation())
            page.onSignal = signalHandler
            page.onAction = actionHandler
            page.onConfirmation = confirmationHandler
            try await page.run("releaseFreshUpdate();")
            let fresh = try await preparing.value
            #expect(fresh == freshIdentity && page.settled)
            _ = try pair.present(fresh)
            try await page.waitForShown(fresh)
            try await CardBehaviorFixture.until("current runtime still confirms paint") { page.acceptsVisibleConfirmation() }
            #expect(pair.current === page)
            #expect(page.update == fresh)
            #expect(pair.diagnostics.entries == diagnostics)
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
