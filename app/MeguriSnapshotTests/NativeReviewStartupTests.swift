#if os(macOS)
import AppKit
@testable import AppModel
@testable import CardRendering
import CoreBridge
import Foundation
import Review
import SwiftUI
import Testing
import WebKit

// Real app-hosted lane: tools/mg mac-hosted-test --suite NativeReviewStartupTests/productionReviewHostStartupLifecycle
// Foreground activation is confined to this own-app test window in the VM.
@MainActor @Suite(.serialized) struct NativeReviewStartupTests {
    @Test func productionReviewHostStartupLifecycle() async throws {
        for completion in ["present", "pause-resume", "close", "refresh-close", "media-error-close", "cancel-start"] {
            try await exerciseStartup(completion: completion)
        }
    }
    @Test func acceptedAnswerHeldPreparationDoesNotBlockPauseOrClose() async throws {
        for close in [false, true] {
            try await exerciseStartup(completion: close ? "answer-close" : "answer-pause")
        }
    }

    @Test func deadlineJoinsOwnedEndBeforeRemovingResources() async throws {
        for outcome in ["timeout", "failure", "cancel"] {
            try await exerciseDeadlineJoinsOwnedEndBeforeRemovingResources(outcome: outcome)
        }
    }

    private func exerciseDeadlineJoinsOwnedEndBeforeRemovingResources(outcome: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let surface = CleanupSurface()
        let store = ReviewRecordStore(file: root.appending(path: "session.json"))
        let client = HeadClient(item: Self.item(2))
        let model = ReviewCoordinator(deckID: 1, surface: surface, store: store,
            clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client)
        await model.start()
        let caller = Task {
            try await Self.deadline("cleanup-\(outcome)", unblocking: { surface.release() }) {
                let ended = await model.end() // Owns a task independent of this wrapper.
                if outcome == "failure" { throw CleanupFailure.expected }
                return ended
            }
        }
        await surface.requested()
        if outcome == "cancel" { caller.cancel() }
        if outcome == "failure" { surface.release() }
        let result = await caller.result
        switch result {
        case .success: Issue.record("Expected \(outcome) failure")
        case .failure(let error):
            if outcome == "timeout" { #expect((error as? DeadlineFailure)?.stage == "cleanup-timeout") }
            else if outcome == "cancel" { #expect(error is CancellationError) }
            else { #expect(error is CleanupFailure) }
        }
        #expect(surface.stopCompleted && surface.closed && model.phase == .ended)
        #expect(client.endCalls == 1 && model.sessionID == nil)
        #expect(try await store.load()?.endedAt != nil)
        // All original work has returned before removing its persistence root.
        try FileManager.default.removeItem(at: root)
        #expect(try await store.load() == nil)
    }

    private enum CleanupFailure: Error { case expected }
    private final class CleanupSurface: ReviewCardSurface {
        var pair: CardPagePair? { nil }
        var onAction: ((CardBridgeAction) -> Void)?
        var onRecovery: (() -> Void)?
        var editing = false
        var stopCompleted = false, closed = false
        private let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        private var stop: CheckedContinuation<Void, Never>?
        func load() async throws {}
        func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws {}
        func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws {}
        func preload(item: ReviewHeadItem) async throws {}
        func reveal(inputTime: TimeInterval) async throws {}
        func resume() async {}
        func suspend() async {
            await withCheckedContinuation { stop = $0; entered.continuation.yield(()) }
            stopCompleted = true
        }
        func requested() async { for await _ in entered.stream { return } }
        func release() { stop?.resume(); stop = nil; entered.continuation.finish() }
        func close() { closed = true }
    }
    // Mount the production SwiftUI/representable path in a real window.
    // A bare pair in SpikeWindow cannot catch phase-dependent host hiding.
    private func exerciseStartup(completion: String) async throws {
        try await SnapshotAppLifecycle.shared.withExclusiveAccess {
            let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: root) }
            let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let gate = SettlementGate()
            let controller = configuration.configuration.userContentController
            controller.add(gate, contentWorld: CardPage.world, name: "testPreloadHeld")
            controller.add(gate, contentWorld: .page, name: "testNativeFrames")
            controller.addUserScript(WKUserScript(source: Self.holdNextSettlement,
                injectionTime: .atDocumentEnd, forMainFrameOnly: true, in: CardPage.world))
            defer {
                gate.finish()
                controller.removeScriptMessageHandler(forName: "testPreloadHeld", contentWorld: CardPage.world)
                controller.removeScriptMessageHandler(forName: "testNativeFrames", contentWorld: .page)
            }
            let pair = CardPagePair(configuration: configuration, resolver: resolver)
            let pages = ReviewPages(pair: pair)
            pages.renderHead = { _, keys, _ in
                keys.map { key in
                    CardPayload(cardID: key == Self.item(1).renderKey ? 1 : 2,
                                question: "<p>Question</p>", answer: "Answer")
                }
            }
            let answerJourney = completion.hasPrefix("answer-")
            let client = HeadClient(item: Self.item(answerJourney ? 1 : 2))
            if answerJourney { client.answerItem = Self.item(2) }
            let model = ReviewCoordinator(deckID: 1, surface: pages,
                store: ReviewRecordStore(file: root.appending(path: "session.json")),
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client)
            var closing: Task<Bool, Never>?
            let hosting = NSHostingController(rootView: ReviewView(model: model, close: {
                if closing == nil { closing = Task { await model.end() } }
            }))
            let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 640, height: 480),
                styleMask: [.titled], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let previousPolicy = NSApp.activationPolicy()
            let activationTrace = ActivationTrace()
            let active = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            let occlusion = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            let activeObserver = NotificationCenter.default.addObserver(
                forName: NSApplication.didBecomeActiveNotification, object: NSApp, queue: .main
            ) { _ in
                activationTrace.record(.didBecomeActive)
                active.continuation.yield(())
            }
            let occlusionObserver = NotificationCenter.default.addObserver(
                forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main
            ) { _ in
                activationTrace.record(.windowOcclusionChanged)
                occlusion.continuation.yield(())
            }
            let activationNotifications: [(Notification.Name, ActivationTrace.Event)] = [
                (NSApplication.didResignActiveNotification, .didResignActive),
                (NSApplication.willHideNotification, .willHide),
                (NSApplication.didHideNotification, .didHide),
                (NSApplication.willUnhideNotification, .willUnhide),
                (NSApplication.didUnhideNotification, .didUnhide)
            ]
            let activationObservers = activationNotifications.map { name, event in
                NotificationCenter.default.addObserver(forName: name, object: NSApp, queue: .main) { _ in
                    // No AppKit reads or actor assumptions in notification callbacks.
                    activationTrace.record(event)
                }
            }
            defer {
                NotificationCenter.default.removeObserver(activeObserver)
                NotificationCenter.default.removeObserver(occlusionObserver)
                for observer in activationObservers { NotificationCenter.default.removeObserver(observer) }
                active.continuation.finish(); occlusion.continuation.finish()
            }
            window.contentViewController = hosting
            window.setContentSize(NSSize(width: 640, height: 480))
            let host = hosting.view
            activationTrace.record(.beforePolicyRequest, state: .capture(window))
            let policyAccepted = NSApp.setActivationPolicy(.regular)
            activationTrace.record(.policyRequestReturned, state: .capture(window), policyAccepted: policyAccepted)
            window.makeKeyAndOrderFront(nil)
            activationTrace.record(.windowOrdered, state: .capture(window))
            activationTrace.record(.beforeActivate, state: .capture(window))
            NSApp.activate(ignoringOtherApps: true)
            activationTrace.record(.afterActivate, state: .capture(window))
            defer {
                pages.close(); window.contentViewController = nil; window.close()
                NSApp.setActivationPolicy(previousPolicy)
            }
            func bounded<T: Sendable>(_ stage: String,
                _ body: @escaping @MainActor @Sendable () async throws -> T) async throws -> T {
                try await Self.deadline(stage, unblocking: {
                    if stage == "native host activation" {
                        activationTrace.record(.beforeFailureUnblocking, state: .capture(window))
                    }
                    pages.close(); gate.finish()
                }, body)
            }
            do {
                try await bounded("native host activation") {
                    activationTrace.record(.waiterEntry, state: .capture(window))
                    if !NSApp.isActive {
                        for await _ in active.stream {
                            if NSApp.isActive {
                                activationTrace.record(.predicateTrue, state: .capture(window))
                                break
                            }
                        }
                    } else {
                        activationTrace.record(.predicateTrue, state: .capture(window))
                    }
                    activationTrace.record(Task.isCancelled ? .waiterCancelled : .waiterCompleted,
                                           state: .capture(window))
                }
                activationTrace.record(.deadlineSucceeded, state: .capture(window))
            } catch {
                let outcome: ActivationTrace.Outcome = error is CancellationError ? .cancelled
                    : error is DeadlineFailure ? .expired : .failed
                // deadline has joined its work; the window and observers are still owned.
                print(activationTrace.failureRecord(outcome, afterJoin: .capture(window)))
                throw error
            }
            try await bounded("native host visibility") {
                if !window.occlusionState.contains(.visible) {
                    for await _ in occlusion.stream { if window.occlusionState.contains(.visible) { break } }
                }
            }
            window.layoutIfNeeded(); host.layoutSubtreeIfNeeded(); window.displayIfNeeded()
            try #require(NSApp.isActive && window.isVisible && window.occlusionState.contains(.visible))
            try #require(!host.visibleRect.isEmpty && host.window === window)
            let starting = Task { await model.start() }
            do {
                if answerJourney {
                    try await bounded("original question") { await starting.value }
                    try #require(model.phase == .question)
                    let first = pair.current
                    let firstUpdate = try #require(first.update)
                    try await bounded("original question shown") { try await first.waitForShown(firstUpdate) }
                    model.reveal()
                    try await bounded("original answer shown") { await model.settled() }
                    try #require(model.canRate)
                    model.rate(.good)
                    try await bounded("answer.next-held") {
                        for await _ in gate.events.stream { return }
                        throw DeadlineFailure(stage: "answer.next-held")
                    }
                    let preparedPage = pair.idle
                    try #require(preparedPage.update?.card == 2 && !preparedPage.settled)
                    #expect(model.record.answers.map(\.cardID) == [1] && client.answerCalls == 1)
                    // Independent frames still come from the real next document.
                    let drawable = try await bounded("next question native frames") {
                        try await preparedPage.run("""
                            requestAnimationFrame(() => requestAnimationFrame(() =>
                              webkit.messageHandlers.testNativeFrames.postMessage(!document.hidden)));
                            """, world: .page)
                        for await visible in gate.frames.stream { return visible }
                        throw DeadlineFailure(stage: "next question native frames")
                    }
                    try #require(drawable == true)
                    if completion == "answer-close" {
                        let ended = try await bounded("answer.close") { await model.end() }
                        #expect(ended && client.endCalls == 1 && model.phase == .ended && preparedPage.closed)
                    } else {
                        try await bounded("answer.pause") { try await model.pause() }
                        #expect(model.paused && model.sessionID == 5 && client.endCalls == 0)
                        #expect(preparedPage.update == nil && !preparedPage.presented)
                    }
                    // No release of the next card's receipt preceded pause/close.
                    let saved = try #require(try await ReviewRecordStore(file: root.appending(path: "session.json")).load())
                    #expect(saved.answers == model.record.answers && saved.answers.count == 1)
                    #expect((saved.endedAt != nil) == (completion == "answer-close"))
                    if completion == "answer-pause" {
                        try await preparedPage.run("releasePreloadSettlement();", world: CardPage.world)
                        try await bounded("resume retained post-answer head") { try await model.resume() }
                        #expect(model.canReveal && model.item?.cardId == 2 && client.headReads == 1)
                        let update = try #require(pair.current.update)
                        try await bounded("resumed next question shown") { try await pair.current.waitForShown(update) }
                        #expect(update.card == 2 && update.side == .question && pair.current.presented)
                        try Self.requireDrawable(pair.current.webView, in: host)
                    }
                    let ended = try await bounded("final accepted answer close") { await model.end() }
                    #expect(ended && client.endCalls == 1 && client.answerCalls == 1)
                    let endedRecord = try #require(try await ReviewRecordStore(file: root.appending(path: "session.json")).load())
                    #expect(endedRecord.answers == saved.answers && endedRecord.endedAt != nil)
                    await starting.value
                    _ = await closing?.value
                    return
                }
                // This is a real runtime receipt after its two nested rAFs. Hold
                // only delivery to keep the model in the production loading phase.
                try await bounded("production loading host settled") {
                    for await _ in gate.events.stream { return }
                    throw DeadlineFailure(stage: "production loading host settled")
                }
                #expect(model.phase == .loading)
                try #require(pair.container.window === window)
                #expect(!pair.ownsKeyboardFocus)
                #expect(!pair.idle.settled && !pair.idle.presented)
                let preparedPage = pair.idle
                // Independent page-world rAFs prove real rendering opportunity;
                // neither visibility nor runtime receipts are synthesized.
                let drawable = try await bounded("native document frames") {
                    try await preparedPage.run("""
                        requestAnimationFrame(() => requestAnimationFrame(() =>
                          webkit.messageHandlers.testNativeFrames.postMessage(!document.hidden)));
                        """, world: .page)
                    for await visible in gate.frames.stream { return visible }
                    throw DeadlineFailure(stage: "native document frames")
                }
                try #require(drawable == true)

                if completion == "present" {
                    try await preparedPage.run("releasePreloadSettlement();", world: CardPage.world)
                    try await bounded("production loading completes") { await starting.value }
                    #expect(model.phase == .question && pair.current === preparedPage)
                    let update = try #require(pair.current.update)
                    try await bounded("production question shown") {
                        try await preparedPage.waitForShown(update)
                    }
                    #expect(pair.current.presented && update.card == 2)
                    try Self.requireDrawable(preparedPage.webView, in: host)
                } else if completion == "pause-resume" {
                    try await bounded("startup.pause") { try await model.pause() }
                    await starting.value
                    #expect(model.paused && model.sessionID == 5 && preparedPage.update == nil)
                    #expect(client.endCalls == 0 && model.record.answers.isEmpty)
                    // Resume reads the retained core head after receipt delivery.
                    // Resuming prepares that head without submitting an answer.
                    try await preparedPage.run("releasePreloadSettlement();", world: CardPage.world)
                    try await bounded("resume startup") { try await model.resume() }
                    #expect(!model.paused && model.phase == .question && pair.current === preparedPage)
                    let update = try #require(pair.current.update)
                    try await bounded("resumed question shown") { try await preparedPage.waitForShown(update) }
                    #expect(pair.current.presented && client.headReads == 1)
                    try Self.requireDrawable(preparedPage.webView, in: host)
                } else {
                    if completion != "cancel-start" {
                        if completion == "refresh-close" {
                            model.requestRefresh()
                            #expect(model.phase == .transition)
                        } else if completion == "media-error-close" {
                            model.mediaUnavailable()
                            #expect(model.phase == .error)
                        }
                        let ended = try await bounded("startup.close") {
                            await model.end()
                        }
                        #expect(ended && model.phase == .ended && preparedPage.closed)
                    } else {
                        starting.cancel()
                        try await bounded("startup.cancel") { await starting.value }
                        #expect(model.phase == .error && preparedPage.update == nil)
                        // Deliver the delayed receipt to check cancellation stays terminal.
                        try await preparedPage.run("releasePreloadSettlement();", world: CardPage.world)
                    }
                    #expect(!preparedPage.presented && model.record.answers.isEmpty)
                }
                let ended = try await bounded("final close") { await model.end() }
                #expect(ended && client.endCalls == 1 && model.sessionID == nil)
                #expect(model.record.answers.isEmpty)
                await starting.value
                _ = await closing?.value
            } catch {
                // Release native waiters first, then join the original handles.
                // Cancelling the deadline's wrapper cannot join an owned end task.
                starting.cancel(); pages.close(); gate.finish()
                await starting.value
                _ = await closing?.value
                _ = await model.end()
                throw error
            }
        }
    }

    // Test-local fixed facts only. The lock covers all mutable storage, including
    // callback events; AppKit snapshots are captured separately on the main actor.
    private nonisolated final class ActivationTrace: @unchecked Sendable {
        enum Event: String, Sendable {
            case beforePolicyRequest, policyRequestReturned, windowOrdered, beforeActivate, afterActivate
            case didBecomeActive, didResignActive, willHide, didHide, willUnhide, didUnhide
            case windowOcclusionChanged, waiterEntry, predicateTrue, waiterCompleted, waiterCancelled
            case beforeFailureUnblocking, deadlineSucceeded
        }
        enum Outcome: String, Sendable { case expired, cancelled, failed }
        enum Policy: String, Sendable { case regular, accessory, prohibited, unknown }
        struct State: Sendable {
            let policy: Policy
            let active, hidden, visible, occlusionVisible, key, miniaturized, hasScreen: Bool

            @MainActor static func capture(_ window: NSWindow) -> State {
                let policy: Policy
                switch NSApp.activationPolicy() {
                case .regular: policy = .regular
                case .accessory: policy = .accessory
                case .prohibited: policy = .prohibited
                @unknown default: policy = .unknown
                }
                return State(policy: policy, active: NSApp.isActive, hidden: NSApp.isHidden,
                    visible: window.isVisible, occlusionVisible: window.occlusionState.contains(.visible),
                    key: window.isKeyWindow, miniaturized: window.isMiniaturized, hasScreen: window.screen != nil)
            }

            var json: String {
                "{\"policy\":\"\(policy.rawValue)\",\"active\":\(active),\"hidden\":\(hidden),"
                    + "\"visible\":\(visible),\"occlusionVisible\":\(occlusionVisible),\"key\":\(key),"
                    + "\"miniaturized\":\(miniaturized),\"hasScreen\":\(hasScreen)}"
            }
        }

        private let lock = NSLock()
        private let start = ContinuousClock.now
        private var events: [String] = []
        private var eventBytes = 0
        private var eventsTruncated = false
        private var beforeUnblocking: String?

        private var elapsedMs: Double {
            let elapsed = start.duration(to: .now).components
            return Double(elapsed.seconds) * 1_000 + Double(elapsed.attoseconds) / 1_000_000_000_000_000
        }

        func record(_ event: Event, state: State? = nil, policyAccepted: Bool? = nil) {
            lock.lock(); defer { lock.unlock() }
            var entry = "{\"event\":\"\(event.rawValue)\",\"elapsedMs\":\(elapsedMs)"
            if let state { entry += ",\"state\":\(state.json)" }
            if let policyAccepted { entry += ",\"policyAccepted\":\(policyAccepted)" }
            entry += "}"
            // Reserve the pre-cleanup failure fact even if earlier notifications flood the buffer.
            if event == .beforeFailureUnblocking { beforeUnblocking = entry }
            guard events.count < 48, eventBytes + entry.utf8.count + 1 <= 8_192 else {
                eventsTruncated = true
                return
            }
            events.append(entry)
            eventBytes += entry.utf8.count + 1
        }

        func failureRecord(_ outcome: Outcome, afterJoin: State) -> String {
            lock.lock(); defer { lock.unlock() }
            var prefix = "{\"trace\":\"nativeReviewActivationV1\",\"outcome\":\"\(outcome.rawValue)\","
                + "\"elapsedMs\":\(elapsedMs),\"beforeUnblockingCaptured\":\(beforeUnblocking != nil),"
            if let beforeUnblocking { prefix += "\"beforeUnblocking\":\(beforeUnblocking)," }
            prefix += "\"afterJoin\":\(afterJoin.json),"
            let record = prefix + "\"eventsTruncated\":\(eventsTruncated),\"outputTruncated\":false,"
                + "\"events\":[\(events.joined(separator: ","))]}"
            // Bound output bytes while keeping JSON valid.
            guard record.utf8.count <= 12_288 else {
                return prefix + "\"eventsTruncated\":true,\"outputTruncated\":true,\"events\":[]}"
            }
            return record
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
        let frames = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingOldest(1))
        func finish() { events.continuation.finish(); frames.continuation.finish() }
        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            if message.name == "testNativeFrames", let visible = message.body as? Bool {
                frames.continuation.yield(visible)
            } else if message.name == "testPreloadHeld" { events.continuation.yield(()) }
        }
    }

    private final class HeadClient: ReviewSessionClient {
        var endCalls = 0
        var headReads = 0
        var answerCalls = 0
        var answerItem: ReviewHeadItem?
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
        func head(sessionID: UInt64) async throws -> ReviewHead { headReads += 1; return currentHead }
        func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus { .unavailable(reason: .none) }
        func end(sessionID: UInt64) async throws { endCalls += 1 }
        func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome {
            guard let answerItem else { throw BridgeError.internal("Answer is outside the startup fixture") }
            answerCalls += 1
            self.item = answerItem
            return .accepted(headDiff: .init(head: currentHead, marks: [], removed: [item.cardId]),
                             leech: false, suspended: false, rolledOver: false)
        }
        func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead {
            throw BridgeError.internal("Restore is outside the startup fixture")
        }
        func studyState(deckID: Int64) async throws -> ReviewDeckStudyState {
            throw BridgeError.internal("Summary is outside the startup fixture")
        }
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
    private static func requireDrawable(_ page: NSView, in host: NSView) throws {
        var ancestor: NSView? = page
        while let view = ancestor {
            try #require(!view.isHidden && view.alphaValue > 0)
            if view === host { break }
            ancestor = view.superview
        }
        try #require(ancestor === host && !page.visibleRect.isEmpty)
    }

    private struct DeadlineFailure: Error { let stage: String }
    // Same three-second bounds as the original receipt test; no readiness delay.
    private static func deadline<T: Sendable>(_ stage: String,
        unblocking: @escaping @MainActor () -> Void = {},
        _ body: @escaping @MainActor @Sendable () async throws -> T) async throws -> T {
        let results = AsyncStream<Result<T, any Error>>.makeStream(bufferingPolicy: .bufferingOldest(1))
        let work = Task { @MainActor in
            do { results.continuation.yield(.success(try await body())) }
            catch { results.continuation.yield(.failure(error)) }
        }
        let timeout = Task {
            do { try await Task.sleep(for: .seconds(3)) }
            catch { return }
            results.continuation.yield(.failure(DeadlineFailure(stage: stage)))
        }
        let result: Result<T, any Error> = await withTaskCancellationHandler {
            for await result in results.stream { return result }
            return .failure(CancellationError())
        } onCancel: {
            work.cancel(); timeout.cancel(); results.continuation.finish()
        }
        work.cancel(); timeout.cancel()
        if case .failure = result { unblocking() }
        // Join the actual wrapper, including any original lifecycle handle it
        // awaits, before resources or the enclosing fixture can be removed.
        await work.value
        await timeout.value
        results.continuation.finish()
        return try result.get()
    }
}
#endif
