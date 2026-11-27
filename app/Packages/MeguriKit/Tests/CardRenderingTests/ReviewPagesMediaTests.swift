@testable import AppModel
@testable import CardRendering
import CoreBridge
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
#endif

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct ReviewPagesMediaTests {
        #if os(macOS)
        @Test(arguments: ["parked", "key-return", "native", "editing", "background", "reopen", "cancel", "resume-cancel", "identity", "ended"])
        func coordinatorRestoresSavedEditAfterPauseAndResume(owner: String) async throws {
            @MainActor final class State {
                var committedUpdate: CardUpdate?
                var replacementEditing = false
                var ready: Bool?
                var readinessChecks = 0
            }
            let state = State()
            let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair), host = SpikeWindow(pair.container)
            defer { pages.close(); host.close() }
            let client = HeadClient(item: Self.item(1))
            pages.renderHead = { _, keys, _ in
                keys.map { key in
                    CardPayload(cardID: key == Self.item(1).renderKey ? 1 : 2,
                        question: "<p>Question</p><input id=entry>", answer: "Answer")
                }
            }
            let model = ReviewCoordinator(deckID: 1, surface: pages,
                store: ReviewRecordStore(file: directory.appending(path: "session.json")),
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client)
            let committed = pair.onCommitted
            pair.onCommitted = { update, token in
                if state.replacementEditing { pair.onInteraction?("focus", ["editable": true]) }
                committed?(update, token)
                state.committedUpdate = update
            }
            await model.start()
            try await pair.current.waitForShown(try #require(pair.current.update))
            try await CardBehaviorFixture.until("initial edit card committed") { state.committedUpdate == pair.current.update }
            let old = pair.current.webView, window = try #require(old.window)
            pair.ownsKeyboardFocus = true
            try #require(window.makeFirstResponder(old))
            try #require(model.beginEditingNote())
            try await model.pause()
            #expect(model.paused && model.editingNote && !pages.inputActive)
            model.endEditingNote(saved: owner != "cancel")
            #expect(model.paused && !model.editingNote && window.firstResponder === old)

            // Model just OS/presenter readiness; the coordinator, page swap,
            // commit and responder restoration all run their production paths.
            state.ready = owner == "key-return" || owner == "reopen" ? nil : owner != "background"
            pages.editedCardWindowReady = { [weak model, weak pages] _, _ in
                state.readinessChecks += 1
                #expect(model?.paused == false && model?.editingNote == false && pages?.inputActive == true)
                return state.ready
            }
            let input = NSTextView()
            pair.container.addSubview(input)
            if owner == "native" { try #require(window.makeFirstResponder(input)) }
            if owner == "identity" { client.item = Self.item(2) }
            state.replacementEditing = owner == "editing"
            if owner == "ended" { model.generationEnded() }
            if owner == "resume-cancel" {
                let gate = CardAudioControllerTests.Hold()
                defer { gate.release() }
                client.beforeHead = { await gate.wait() }
                let resuming = Task { try await model.resume() }
                try await Spike.deadline("edit resume head held", timeout: .seconds(3)) { await gate.reached() }
                resuming.cancel(); gate.release()
                do { try await resuming.value; Issue.record("Cancelled edit resume succeeded") }
                catch is CancellationError {}
                #expect(model.paused && state.readinessChecks == 0 && window.firstResponder === old)
                client.beforeHead = nil
            }
            try await model.resume()
            if owner == "ended" {
                #expect(model.phase == .ended && state.readinessChecks == 0)
                return
            }
            try await pair.current.waitForShown(try #require(pair.current.update))
            try await CardBehaviorFixture.until("resumed edit card committed") { state.committedUpdate == pair.current.update }
            #expect(!model.paused && pages.inputActive && model.phase == .question)
            if owner == "cancel" {
                #expect(pair.current.webView === old && state.readinessChecks == 0)
            } else {
                #expect(pair.current.webView !== old && pair.idle.webView === old)
                #expect(pair.container.subviews.last === pair.current.webView)
                #expect(!old.acceptsFirstResponder)
            }
            if owner == "key-return" {
                try #require(state.readinessChecks > 0)
                #expect(window.firstResponder === old)
                state.ready = true
                pages.establishInitialFocus()
            } else if owner == "reopen" {
                try #require(model.beginEditingNote())
                state.ready = true
                model.endEditingNote(saved: false)
            }
            if owner == "parked" || owner == "key-return" {
                #expect(state.readinessChecks > 0)
                #expect(window.firstResponder === pair.current.webView)
                #expect(pair.ownsKeyboardFocus)
                #expect(try await pair.current.string("return document.activeElement.id;") != "entry")
            } else if owner == "native" {
                #expect(window.firstResponder === input)
            } else {
                #expect(window.firstResponder === old)
            }
            // Each request is consumed once, including a refusal: a later
            // key return/attachment must leave the person's new input alone.
            state.ready = true
            try #require(window.makeFirstResponder(input))
            let checks = state.readinessChecks
            pages.establishInitialFocus(); pages.establishInitialFocus()
            #expect(window.firstResponder === input && state.readinessChecks == checks)
            if owner == "identity" || owner == "cancel" || owner == "resume-cancel" { #expect(checks == 0) }
            #expect(await model.end())
        }

        @Test(arguments: [false, true])
        func editedCardRefreshFocusesDisplayedPageAndPreservesOtherInput(answer: Bool) async throws {
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair), host = SpikeWindow(pair.container)
            defer { pages.close(); host.close() }
            let item = Self.item(1)
            pages.renderHead = { _, _, _ in
                [CardPayload(cardID: 1, question: "<p>Edited question</p><input id=entry>", answer: "<p>Edited answer</p><input id=entry>")]
            }
            try await pages.load()
            try await pages.render(sessionID: 5, items: [item])
            try await pages.present(item: item, inputTime: ProcessInfo.processInfo.systemUptime)
            try await pair.current.waitForShown(try #require(pair.current.update))
            let old = pair.current.webView
            let window = try #require(old.window)
            pair.ownsKeyboardFocus = true
            #expect(window.makeFirstResponder(old))
            pages.inputActive = false
            pair.ownsKeyboardFocus = false
            // Save refreshes through the production double buffer while the
            // editor owns input; AppKit retains the old page's responder.
            try await pages.present(item: item, inputTime: ProcessInfo.processInfo.systemUptime)
            try await pair.current.waitForShown(try #require(pair.current.update))
            if answer { try await pages.reveal(inputTime: ProcessInfo.processInfo.systemUptime) }
            #expect(pair.current.webView !== old && pair.idle.webView === old)
            #expect(pair.container.subviews.last === pair.current.webView)
            #expect(window.firstResponder === old)
            #expect(!old.acceptsFirstResponder)
            #expect(ReviewInputOwnership.restoreEditedCardResponder(in: pair))
            #expect(window.firstResponder === pair.current.webView)
            #expect(window.firstResponder !== pair.idle.webView)
            #expect(!old.acceptsFirstResponder, "the parked page stays ineligible for native input")
            #expect(try await pair.current.string("return document.activeElement.id;") != "entry")
            try await pair.current.run("document.getElementById('entry').focus(); document.getElementById('entry').value='retained';")
            #expect(!ReviewInputOwnership.restoreEditedCardResponder(in: pair))
            #expect(try await pair.current.string("return document.activeElement.value;") == "retained")
            // A native field is never a repair target.
            let input = NSTextView()
            pair.container.addSubview(input)
            #expect(window.makeFirstResponder(input))
            #expect(!ReviewInputOwnership.restoreEditedCardResponder(in: pair))
            #expect(window.firstResponder === input)
            input.removeFromSuperview()
        }
        #endif

        @Test func coordinatorCapturesAudioBeforeRendering() async throws {
            let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            let fixture = MediaFixture()
            fixture.add("/q.mp3", Data([1]), "audio/mpeg")
            let resolver = try await fixture.resolved()
            let backend = CardAudioControllerTests.Backend(), session = CardAudioControllerTests.Session()
            let audio = CardAudioController(resolver: resolver, backend: backend, session: session)
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: resolver, audio: audio)
            let pages = ReviewPages(pair: pair), window = SpikeWindow(pair.container)
            let gate = CardAudioControllerTests.Hold()
            defer { gate.release(); pages.close(); window.close() }
            let first = Self.item(1, autoplay: true)
            pages.renderHead = { _, _, _ in
                await gate.wait()
                return [CardPayload(cardID: 1, question: "Question", answer: "Answer", questionAudio: [.sound(name: "q.mp3")])]
            }
            let model = ReviewCoordinator(deckID: 1, surface: pages,
                store: ReviewRecordStore(file: directory.appending(path: "session.json")),
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: HeadClient(item: first))
            let start = Task { await model.start() }
            defer { start.cancel() }
            try await Spike.deadline("coordinator render held", timeout: .seconds(3)) { await gate.reached() }
            audio.sessionEvent(.pause)
            gate.release()
            await start.value
            try await CardBehaviorFixture.until("coordinator card committed") { pair.current.presented }
            try await pair.current.waitForShown(try #require(pair.current.update))
            await audio.transportSettled()
            #expect(model.phase == .question && audio.current == nil && backend.events.isEmpty)
            #expect(session.continuityOwners.isEmpty)
            pair.replay()
            #expect(await backend.nextPlay() == "q.mp3")
        }

        @Test(arguments: [false, true])
        func coordinatorPauseReleasesIdleOutputBeforeHeldCoreReturns(mediaUnavailable: Bool) async throws {
            let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            let fixture = MediaFixture()
            fixture.add("/q.mp3", Data([1]), "audio/mpeg")
            let resolver = try await fixture.resolved()
            let backend = CardAudioControllerTests.Backend(), session = CardAudioControllerTests.Session()
            let audio = CardAudioController(resolver: resolver, backend: backend, session: session)
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: resolver, audio: audio)
            let pages = ReviewPages(pair: pair), window = SpikeWindow(pair.container)
            let gate = CardAudioControllerTests.Hold()
            defer { gate.release(); pages.close(); window.close() }
            let first = Self.item(1, autoplay: true), client = HeadClient(item: first)
            pages.renderHead = { _, _, _ in [CardPayload(cardID: 1, question: "Question", answer: "Answer",
                questionAudio: [.sound(name: "q.mp3")])] }
            let model = ReviewCoordinator(deckID: 1, surface: pages,
                store: ReviewRecordStore(file: directory.appending(path: "session.json")),
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client)
            await model.start()
            #expect(await backend.nextPlay() == "q.mp3")
            await audio.transportSettled()
            backend.clips.first?.onEnd?(true)
            await audio.transportSettled()
            #expect(audio.current == nil && session.continuityOwners.count == 1)
            client.beforeHead = { await gate.wait() }
            model.requestRefresh()
            try await Spike.deadline("core head held", timeout: .seconds(3)) { await gate.reached() }
            if mediaUnavailable { model.mediaUnavailable() } else { model.beginPause() }
            // Check before releasing the original Core operation.
            #expect(session.continuityOwners.isEmpty && session.owners.isEmpty)
            gate.release()
            await model.settled()
            #expect(session.continuityOwners.isEmpty && backend.events == ["q.mp3"])
        }



        @Test(arguments: [false, true, nil] as [Bool?])
        func lateHeadRenderCannotRestoreObsoletePriority(close: Bool?) async throws {
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair)
            defer { pages.close() }
            let held = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            let release = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            defer { held.continuation.finish(); release.continuation.finish() }
            pages.renderHead = { _, keys, _ in
                if keys == [Self.item(1).renderKey] {
                    held.continuation.yield(())
                    for await _ in release.stream { break }
                    return [CardPayload(cardID: 1, question: "Old", answer: "Answer", mediaRefs: ["old.png"])]
                }
                return [CardPayload(cardID: 2, question: "Fresh", answer: "Answer", mediaRefs: ["fresh.png"])]
            }
            let old = Task { try await pages.render(sessionID: 5, items: [Self.item(1)]) }
            defer { old.cancel() }
            try await Self.receipt(held.stream, "old head render held")
            if close == true { pages.close() }
            else if close == false { old.cancel() }
            else { try await pages.render(sessionID: 5, items: [Self.item(2)]) }
            release.continuation.yield(())
            do { try await old.value; Issue.record("Obsolete render was admitted") }
            catch is CancellationError {}
            #expect(pages.visibleMediaNames == (close == nil ? ["fresh.png"] : []))
        }

        // Real owner -> coordinator -> ReviewPages -> CardPagePair delivery.
        // Only core rendering/scheduling are synthetic; no FFI host is opened.
        @Test(arguments: [false, true])
        func ownerArrivalInvalidatesCompletedAndInFlightNextPage(holdSettlement: Bool) async throws {
            let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            let root = directory.appending(path: "collection.media", directoryHint: .isDirectory)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            var launch = LaunchConfiguration()
            launch.collectionLocation = CollectionLocation(directory: directory)
            let owner = ReviewCollection(configuration: launch, navigation: AppNavigation())
            owner.loadMediaIndex = { MediaNameSnapshot(generation: 0, root: root, names: []) }
            let resolver = try #require(await owner.mediaResolverForReview())
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let gate = SettlementGate()
            let controller = configuration.configuration.userContentController
            controller.add(gate, contentWorld: CardPage.world, name: "testPreloadHeld")
            defer {
                gate.events.continuation.finish()
                controller.removeScriptMessageHandler(forName: "testPreloadHeld", contentWorld: CardPage.world)
            }
            let pair = CardPagePair(configuration: configuration, resolver: resolver)
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            let first = Self.item(1), next = Self.item(2)
            let payloads = [
                first.renderKey: CardPayload(cardID: 1,
                    question: "<input id=typeans><script>window.runs=(window.runs||0)+1</script>", answer: "a"),
                next.renderKey: CardPayload(cardID: 2,
                    question: "<img id=late src=late.png><script>window.runs=(window.runs||0)+1</script>", answer: "b"),
            ]
            pages.renderHead = { _, keys, _ in
                try keys.map { key in try #require(payloads[key]) }
            }
            let client = HeadClient(item: first)
            let model = ReviewCoordinator(deckID: 1, surface: pages,
                store: ReviewRecordStore(file: directory.appending(path: "session.json")),
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client)
            let commits = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            defer { commits.continuation.finish() }
            let attachment = pages.onMediaReady
            pages.onMediaReady = { attachment?(); commits.continuation.yield(()) }
            var deliveries: [[String]] = []
            let deliver = model.retryMedia
            model.retryMedia = { names in
                deliveries.append(names)
                return try await deliver(names)
            }
            owner.presentReview(model)
            await model.start()
            #expect(model.phase == .question)
            try await Self.receipt(commits.stream, "first review presentation")
            let visible = pair.current
            let firstUpdate = try #require(visible.update)
            try await visible.waitForShown(firstUpdate)
            try await visible.run("window.savedInput=document.getElementById('typeans');savedInput.value='retained';savedInput.dispatchEvent(new Event('input',{bubbles:true}));")

            // Render the second payload through the internal provider, then use
            // the same preload method the coordinator's background task calls.
            try await pages.render(sessionID: 5, items: [first, next])
            let idle = pair.idle
            if holdSettlement { try await idle.run(Self.holdNextSettlement, world: CardPage.world) }
            let preload = Task { try await pages.preload(item: next) }
            defer { preload.cancel() }
            if holdSettlement {
                try await Self.receipt(gate.events.stream, "next page settled receipt held")
                #expect(!idle.settled)
            } else {
                try await preload.value
                #expect(idle.settled)
            }
            let oldPreparation = try #require(idle.update)
            #expect(oldPreparation.card == 2 && oldPreparation.side == .question)
            #expect(try await idle.webView.callAsyncJavaScript(
                "const image=document.getElementById('late');return image.complete && image.naturalWidth===0;",
                contentWorld: .page) as? Bool == true)

            // A case fallback requires the owner's updated journal snapshot.
            // This resource exists only on idle, so visible-side retry cannot
            // repair it and there will be no later arrival event.
            try TestMedia.png().write(to: root.appending(path: "LaTe.PNG"))
            owner.loadMediaIndex = {
                MediaNameSnapshot(generation: 0, root: root,
                    names: [TrackedMediaName(name: "LaTe.PNG", caseKey: "late.png")])
            }
            owner.scheduleMediaRefresh(names: ["LaTe.PNG", "LaTe.PNG"], generation: 0)
            let refreshed = await owner.mediaResolverForReview()
            #expect(refreshed === resolver)
            await model.settled()
            #expect(deliveries == [["LaTe.PNG"]])
            #expect(model.phase == .question && model.notice == nil)
            #expect(pair.current === visible && visible.update == firstUpdate && visible.presented)
            try await Self.expectRetainedInput(visible)

            if holdSettlement { try await idle.run("releasePreloadSettlement();", world: CardPage.world) }
            try await preload.value
            // The old preparation really completed; its later completion must
            // not put a stale update back into ReviewPages' prepared cache.
            #expect(idle.settled && idle.update == oldPreparation)
            client.item = next
            model.requestRefresh(question: true)
            await model.settled()
            try await Self.receipt(commits.stream, "second review presentation")
            #expect(model.phase == .question && model.notice == nil)
            #expect(pair.current === idle)
            let presented = try #require(pair.current.update)
            #expect(presented.card == 2 && presented.side == .question)
            #expect(presented.generation > oldPreparation.generation)
            try await pair.current.waitForShown(presented)
            #expect(try await pair.current.webView.callAsyncJavaScript(
                "const image=document.getElementById('late');await image.decode();return image.complete && image.naturalWidth===4;",
                contentWorld: .page) as? Bool == true)
            #expect(try await pair.current.string("return String(window.runs);") == "2")
            try await Self.expectRetainedInput(visible)
            #expect(deliveries == [["LaTe.PNG"]])
            #expect(await model.end())
        }

        @Test func reviewLoadAttachesBundledProducerAndObservesLifecycleSlots() async throws {
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            let first = Self.item(1), next = Self.item(2)
            pages.renderHead = { _, keys, _ in
                keys.map { key in
                    CardPayload(cardID: key == first.renderKey ? 1 : 2,
                        question: "<style>body { background-color: rgb(0 128 255); }</style><p>Question</p>",
                        answer: "<style>body { background-color: transparent; }</style><p>Answer</p>")
                }
            }
            try await pages.load()
            for page in [pair.current, pair.idle] {
                #expect(try await page.string("return typeof meguriReviewHost.receive;", world: CardPage.world) == "function")
                #expect(try await page.string("return typeof meguriReviewHost;") == "undefined")
            }
            try await pages.render(sessionID: 5, items: [first, next])
            try await pages.preload(item: first)
            try await CardBehaviorFixture.until("production prepared probes") {
                pair.idle.hostReports.slotResources == .resources(purpose: .preload, state: .ready,
                    imagesPending: 0, fontsPending: 0, stylesPending: 0) && pair.idle.hostReports.slotBackground != nil
            }
            let prepared = try #require(pair.idle.update)
            try await pages.present(item: first, inputTime: ProcessInfo.processInfo.systemUptime)
            try await CardBehaviorFixture.until("production presentation probes") {
                pair.current.update == prepared && pair.current.hostReports.context?.visibility == .presented &&
                    pair.current.hostReports.slotResources == .resources(purpose: .preload, state: .ready,
                        imagesPending: 0, fontsPending: 0, stylesPending: 0)
            }
            try await pages.preload(item: next)
            try await CardBehaviorFixture.until("production next-slot probes") {
                pair.idle.hostReports.slotResources != nil && pair.idle.hostReports.slotBackground != nil
            }
            #expect(pair.current.update == prepared && pair.idle.update?.card == 2)
            try await pages.reveal(inputTime: ProcessInfo.processInfo.systemUptime)
            try await CardBehaviorFixture.until("production answer probes") {
                pair.current.hostReports.slotBackground == .background(red: 0, green: 0, blue: 0, alpha: 0) &&
                    pair.current.hostReports.slotResources == .resources(purpose: .preload, state: .ready,
                        imagesPending: 0, fontsPending: 0, stylesPending: 0)
            }
            #expect(pair.current.update?.side == .answer)
            await pages.suspend()
            #expect(pair.current.hostReports.context == nil)
            await pages.resume()
            try await CardBehaviorFixture.until("production resume probes") {
                pair.current.hostReports.slotResources == .resources(purpose: .warmup, state: .ready,
                    imagesPending: 0, fontsPending: 0, stylesPending: 0)
            }
        }

        @Test(arguments: [false, true])
        func failedProducerInstallationDoesNotStopOwnerOrReconstruction(loadBeforePrepare: Bool) async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            // A real isolated-world assignment error in every new document.
            // No receiver or readiness result is supplied by this fixture.
            configuration.configuration.userContentController.addUserScript(WKUserScript(source: """
                Object.defineProperty(window, 'meguriReviewHost', {
                  value: undefined, writable: false, configurable: false
                });
                """, injectionTime: .atDocumentStart, forMainFrameOnly: true, in: CardPage.world))
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            var unavailable = 0
            pair.onHostEvent = { _, event in if event == .unavailable { unavailable += 1 } }
            try await pages.load()
            #expect(unavailable > 0)
            // The explicit installation API still reports the evaluation error.
            await #expect(throws: CardHostReporting.InstallationFailure.self) {
                try await pair.installHostScript(CardRuntime.reviewHostScript)
            }
            let first = Self.item(1), next = Self.item(2)
            pages.renderHead = { _, keys, _ in
                keys.map { key in
                    CardPayload(cardID: key == first.renderKey ? 1 : 2,
                        question: "<p id=question>Rendered question</p>", answer: "<p id=answer>Rendered answer</p>")
                }
            }
            try await pages.render(sessionID: 5, items: [first, next])
            try await pages.preload(item: first)
            try await pages.present(item: first, inputTime: ProcessInfo.processInfo.systemUptime)
            try await pair.current.waitForShown(try #require(pair.current.update))
            #expect(try await pair.current.string("return document.getElementById('question').textContent;") == "Rendered question")

            let idle = pair.idle
            idle.webViewWebContentProcessDidTerminate(idle.webView)
            #expect(idle.needsReconstruction)
            if loadBeforePrepare { try await pair.load() }
            try await pages.preload(item: next)
            #expect(!idle.needsReconstruction && idle.settled)
            try await pages.present(item: next, inputTime: ProcessInfo.processInfo.systemUptime)
            try await pair.current.waitForShown(try #require(pair.current.update))
            try await pages.reveal(inputTime: ProcessInfo.processInfo.systemUptime)
            let beforeRecovery = try #require(pair.current.update)
            #expect(beforeRecovery.card == 2 && beforeRecovery.side == .answer)
            var recoveries = 0
            pages.onRecovery = { recoveries += 1 }
            pair.current.webViewWebContentProcessDidTerminate(pair.current.webView)
            try await CardBehaviorFixture.until("rendering recovery despite failed producer installation") { recoveries == 1 }
            #expect(pair.current.update != beforeRecovery && pair.current.update?.side == .answer)
            #expect(try await pair.current.string("return document.getElementById('answer').textContent;") == "Rendered answer")
            for page in pair.pages {
                #expect(page.hostReports.context == nil && page.hostReports.slotResources == nil && page.hostReports.slotBackground == nil)
                #expect(try await page.string("return typeof meguriReviewHost;", world: CardPage.world) == "undefined")
                var outcomes: [CardHostProbeOutcome] = []
                page.hostReports.probe(.resources, purpose: .warmup) { _, outcome in outcomes.append(outcome) }
                #expect(outcomes == [.unavailable])
            }
        }

        private static func expectRetainedInput(_ page: CardPage) async throws {
            #expect(try await page.webView.callAsyncJavaScript(
                "return window.runs===1 && document.getElementById('typeans')===savedInput && savedInput.value==='retained';",
                contentWorld: .page) as? Bool == true)
        }

        private static func receipt(_ stream: AsyncStream<Void>, _ name: String) async throws {
            try await Spike.deadline(name, timeout: .seconds(3)) {
                for await _ in stream { return }
                throw Spike.Failure("Missing \(name)")
            }
        }

        // Like delayedObserverKeepsEmissionIdentityAndArrivalPreservesDOM,
        // retain the actual handler and the unmodified emission identity.
        // Holding the receipt leaves pair.prepare suspended deterministically.
        private static let holdNextSettlement = """
            (function () {
              const handler = webkit.messageHandlers.meguriCard;
              const post = handler.postMessage;
              let held = null;
              let enabled = true;
              window.preloadSignalHandler = handler;
              handler.postMessage = function (message) {
                if (enabled && message.signal === 'settled' && message.card === 2) {
                  held = message;
                  webkit.messageHandlers.testPreloadHeld.postMessage(null);
                } else post.call(handler, message);
              };
              window.releasePreloadSettlement = function () {
                enabled = false;
                if (held) { post.call(handler, held); held = null; }
              };
            })();
            """

        private final class SettlementGate: NSObject, WKScriptMessageHandler {
            let events = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
                events.continuation.yield(())
            }
        }

        private final class HeadClient: ReviewSessionClient {
            var item: ReviewHeadItem
            var items: [ReviewHeadItem]?
            var terminal: ReviewTerminalState = .active
            init(item: ReviewHeadItem) { self.item = item }
            func start(deckID: Int64) async throws -> ReviewStartResult {
                .started(sessionId: 5, generation: 0, head: currentHead, timeLim: 0)
            }
            var currentHead: ReviewHead {
                ReviewHead(items: items ?? [item], counts: .init(new: 1, learning: 0, review: 0), validUntil: Int64.max, terminalState: terminal)
            }
            var beforeHead: (() async -> Void)?
            func head(sessionID: UInt64) async throws -> ReviewHead { await beforeHead?(); return currentHead }
            func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus { .unavailable(reason: .none) }
            func end(sessionID: UInt64) async throws {}
            func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome {
                throw BridgeError.internal("Answer is outside the media attachment fixture")
            }
            func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead {
                throw BridgeError.internal("Restore is outside the media attachment fixture")
            }
            func studyState(deckID: Int64) async throws -> ReviewDeckStudyState {
                throw BridgeError.internal("Summary is outside the media attachment fixture")
            }
        }

        private static func item(_ id: Int64, autoplay: Bool = false) -> ReviewHeadItem {
            let state = ReviewSchedulingState.new(position: 1)
            let answer = ReviewAnswerState(state: state, delay: .days(amount: 1), left: 0, factor: 0,
                revlog: .init(interval: 1, lastInterval: 0, kind: 0, factor: 0),
                label: .init(lessThan: false, amount: "1", unit: .days))
            return ReviewHeadItem(cardId: id, noteId: id, kind: .new,
                stateToken: .init(bytes: Array(repeating: UInt8(id), count: 32)),
                renderKey: .init(bytes: Array(repeating: UInt8(id), count: 32)),
                nextStates: .init(current: state, again: answer, hard: answer, good: answer, easy: answer),
                predictedNext: .init(again: 2, hard: 2, good: 2, easy: 2), flag: 0, marked: false,
                homePreset: .init(id: 1, autoplay: autoplay, skipQuestionWhenReplayingAnswer: false, maximumAnswerSeconds: 60, questionSeconds: 0, answerSeconds: 0,
                    questionAction: 0, answerAction: 0, waitForAudio: false, leechAction: 0))
        }
    }
}
