@testable import AppModel
@testable import CardRendering
import CoreBridge
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct ReviewPagesMediaTests {
        @Test(arguments: [false, true])
        func renderedHeadPriorityRoutesThroughItsCollectionIncludingCacheHits(emptyFirst: Bool) async throws {
            let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            let root = directory.appending(path: "collection.media", directoryHint: .isDirectory)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            let backend = PriorityBackend()
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            var launch = LaunchConfiguration()
            launch.collectionLocation = CollectionLocation(directory: directory)
            let owner = ReviewCollection(configuration: launch, navigation: AppNavigation(), sync: sync)
            owner.receiveLifecycle(.ready(generation: 0))
            owner.loadMediaIndex = { MediaNameSnapshot(generation: 0, root: root, names: []) }
            let resolver = try #require(await owner.mediaResolverForReview())
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: resolver)
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            var renders = 0
            pages.renderHead = { _, keys, _ in
                renders += 1
                return try keys.map { key in
                    let id = try #require((1...4).first { Self.item(Int64($0)).renderKey == key })
                    // Metadata, rather than HTML extraction, supplies priority.
                    return CardPayload(cardID: Int64(id), question: "<p>Question</p>", answer: "Answer",
                        mediaRefs: ["card\(id).png", "shared.mp3", "shared.mp3"])
                }
            }
            let client = HeadClient(item: Self.item(1))
            client.items = (1...4).map { Self.item(Int64($0)) }
            let model = ReviewCoordinator(deckID: 1, surface: pages,
                store: ReviewRecordStore(file: directory.appending(path: "session.json")),
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client)
            owner.presentReview(model)
            await model.start()
            #expect(model.phase == .question)
            // The actual owner callback ran before a backend was available.
            #expect(backend.visibleNames.isEmpty)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 0)
            await sync.waitForInputs()
            #expect(backend.visibleNames == [["card1.png", "card2.png", "card3.png", "shared.mp3"]])
            model.requestRefresh(); await model.settled(); await sync.waitForInputs()
            #expect(renders == 1 && backend.visibleNames.count == 1)
            client.items = (2...4).map { Self.item(Int64($0)) }
            model.requestRefresh(); await model.settled(); await sync.waitForInputs()
            #expect(renders == 1, "Advancing an already rendered head must use its cached payloads")
            #expect(backend.visibleNames == [
                ["card1.png", "card2.png", "card3.png", "shared.mp3"],
                ["card1.png", "card2.png", "card3.png", "card4.png", "shared.mp3"],
                ["card2.png", "card3.png", "card4.png", "shared.mp3"],
            ], "Keep the outgoing card's media in the set until its successor appears")
            if emptyFirst {
                client.items = []; client.terminal = .finished
                model.requestRefresh(); await model.settled(); await sync.waitForInputs()
                #expect(backend.visibleNames.last == [])
            }
            await owner.finishReview(); await sync.waitForInputs()
            #expect(owner.review == nil && backend.visibleNames.last == [])
            #expect(backend.visibleNames.count == 4)
            // A closed coordinator must neither retain nor overwrite a new owner.
            model.requestRefresh(); await model.settled(); await sync.waitForInputs()
            #expect(backend.visibleNames.count == 4)
            #expect(await sync.cancelAndDrain())
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
            func head(sessionID: UInt64) async throws -> ReviewHead { currentHead }
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

        private final class PriorityBackend: SyncCoordinatorBackend {
            var visibleNames: [[String]] = []
            var connection: CoreCloudConnectionService? { nil }
            var own: CloudOwnExecutor? { nil }
            func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
                if case .visibleMedia(let names) = event { visibleNames.append(names) }
                return .commands([])
            }
            func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
                .init(outputs: [], error: nil, deferred: false)
            }
            func beginAccountRead(transition: Bool) async throws -> UInt64 { 1 }
            func readAccount(_ read: UInt64) async -> SyncCommandResult {
                .init(outputs: [], error: nil, deferred: false, accountVerified: true)
            }
            func quiesce() async throws {}
            func close() async throws {}
        }

        private static func item(_ id: Int64) -> ReviewHeadItem {
            let state = ReviewSchedulingState.new(position: 1)
            let answer = ReviewAnswerState(state: state, delay: .days(amount: 1), left: 0, factor: 0,
                revlog: .init(interval: 1, lastInterval: 0, kind: 0, factor: 0),
                label: .init(lessThan: false, amount: "1", unit: .days))
            return ReviewHeadItem(cardId: id, noteId: id, kind: .new,
                stateToken: .init(bytes: Array(repeating: UInt8(id), count: 32)),
                renderKey: .init(bytes: Array(repeating: UInt8(id), count: 32)),
                nextStates: .init(current: state, again: answer, hard: answer, good: answer, easy: answer),
                predictedNext: .init(again: 2, hard: 2, good: 2, easy: 2), flag: 0, marked: false,
                homePreset: .init(id: 1, autoplay: false, skipQuestionWhenReplayingAnswer: false, showTimer: false,
                    stopTimerOnAnswer: false, maximumAnswerSeconds: 60, questionSeconds: 0, answerSeconds: 0,
                    questionAction: 0, answerAction: 0, waitForAudio: false, leechAction: 0))
        }
    }
}
