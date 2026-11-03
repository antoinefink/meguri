import CoreBridge
import Foundation
import Instrumentation
#if DEBUG
import OSLog
#endif
import QuartzCore
import WebKit

/// Owned by a session model, never by its SwiftUI representable.
public final class CardPagePair {
    public let container: CardPlatformView
    public let pages: [CardPage]
    public private(set) var current: CardPage
    public private(set) var idle: CardPage
    public let diagnostics: CardDiagnostics
    public var ownsKeyboardFocus = false {
        didSet {
            for page in pages { page.setFocusOwnership(ownsKeyboardFocus) }
            #if os(macOS)
            container.window?.recalculateKeyViewLoop()
            #endif
        }
    }
    public var onAction: ((CardBridgeAction) -> Void)?
    public var onSignal: ((String, CardUpdate) -> Void)?
    public var onInteraction: ((String, [String: Any]) -> Void)?
    public var onHostEvent: ((CardPage, CardHostEvent) -> Void)?
    public var onTypedText: ((CardTypedValue) -> Void)?
    public var onOpenRequest: ((CardOpenRequest) -> Void)?
    public var onRecovery: ((CardUpdate, CardUpdate) -> Void)?
    /// Forwards native backend observations. No observer means no polling.
    public var onPlaybackObservation: ((CardAudioController.Entry, CardAudioController.PlaybackObservation) -> Void)? {
        get { audio.onPlaybackObservation }
        set { audio.onPlaybackObservation = newValue }
    }
    /// Synchronous facts for the guarded development status request only.
    @_spi(MeguriTestHooks) public var nativeReviewAudioReadiness: CardAudioController.Readiness {
        audio.nativeReviewReadiness
    }
    public var typedValue: CardTypedValue? { current.typedValue }
    public var behaviorPresenter: (any CardBehaviorPresenting)? {
        get { current.behavior.presenter }
        set { for page in pages { page.behavior.presenter = newValue } }
    }
    public var cardZoom = CardZoom() {
        didSet { for page in pages { page.webView.pageZoom = cardZoom.scale } }
    }
    public var inspectorPolicy = WebInspectorPolicy() {
        didSet { for page in pages { page.webView.isInspectable = inspectorPolicy.isInspectable } }
    }
    /// After presentation and its host-context acknowledgement attempt. A
    /// missing/failing host remains unavailable; acknowledgement is not paint.
    public var onCommitted: ((CardUpdate, SwapToken) -> Void)?
    /// Frame probes end intervals independently of the double-rAF receipt.
    public let swaps: SwapRecorder
    public let reveals: SwapRecorder
    private let audio: CardAudioController
    private var generation: UInt64 = 0
    private var suspended = false
    private var lifecycleSuspended = false
    private var preparedAnswer: (question: CardUpdate, answer: CardUpdate)?
    private var closed = false
    private var commitSerial: UInt64 = 0
    private var hostInstallation = UUID()
    private var hostSource: String?
    private var recovery: Task<Void, Never>?
    private var recoveryID = UUID()
    private var htmlAudioOwner: UUID?
    private var lifecycle: Task<Void, Never>?
    private var presentationWork: Task<Void, Never>?
    private var presentationEpoch = UUID()
    private var physicalStop: Task<Void, Never>?
    private var stopBindings: [(page: CardPage, update: CardUpdate?, document: UUID)] = []
    private var stopWaiters: [UUID: AsyncStream<Void>.Continuation] = [:]
    private let compare: (TypeAnswerDescriptor, String) async throws -> String

    public init(configuration: CardWebConfiguration = .shared, resolver: MediaPathResolver,
                platform: CardRuntime.Platform = PlatformClasses.current, appearance: CardRuntime.Appearance = .light,
                reviewExtra: String = "", size: CGSize = CGSize(width: 400, height: 600),
                audio: CardAudioController? = nil,
                compare: @escaping (TypeAnswerDescriptor, String) async throws -> String = { try await Core.compareTypedAnswer($0, typed: $1) },
                onSample: @escaping @MainActor (VisibleSample) -> Void = { _ in },
                makeWebView: (CGRect, WKWebViewConfiguration) -> CardNativeWebView = { CardNativeWebView(frame: $0, configuration: $1) }) {
        let diagnostics = CardDiagnostics()
        self.diagnostics = diagnostics
        self.audio = audio ?? CardAudioController(resolver: resolver, diagnostics: diagnostics)
        self.compare = compare
        swaps = SwapRecorder(onSample: onSample)
        reveals = SwapRecorder(interval: IntervalNames.revealToAnswerVisible, onSample: onSample)
        container = CardPlatformView(frame: CGRect(origin: .zero, size: size))
        let a = CardPage(configuration: configuration, resolver: resolver, platform: platform, appearance: appearance,
                         reviewExtra: reviewExtra, identifier: "review.card.page.a", size: size, diagnostics: diagnostics, makeWebView: makeWebView)
        let b = CardPage(configuration: configuration, resolver: resolver, platform: platform, appearance: appearance,
                         reviewExtra: reviewExtra, identifier: "review.card.page.b", size: size, diagnostics: diagnostics, makeWebView: makeWebView)
        pages = [a, b]; current = a; idle = b
        container.addSubview(b.webView)
        container.addSubview(a.webView)
        #if os(macOS)
        container.setAccessibilityElement(true)
        container.setAccessibilityRole(.group)
        container.setAccessibilityChildren([current.webView])
        container.setAccessibilityChildrenInNavigationOrder([current.webView])
        #endif
        for page in pages {
            page.webView.isInspectable = inspectorPolicy.isInspectable
            page.onTypedText = { [weak self, weak page] value in
                guard let self, let page, page === self.current, page.behaviorAvailable,
                      !self.closed, !self.suspended, !self.lifecycleSuspended else { return }
                self.onTypedText?(value)
            }
            page.onProcessTermination = { [weak self] page in self?.processTerminated(page) }
            page.behavior.onOpenRequest = { [weak self] request in self?.onOpenRequest?(request) }
            page.hostReports.onEvent = { [weak self, weak page] event in
                guard let self, let page else { return }
                self.onHostEvent?(page, event)
            }
            page.onAction = { [weak self] page, action in
                guard let self, page === self.current, !self.suspended, !self.lifecycleSuspended else { return }
                if case .play(let side, let index) = action, let payload = page.payload, let update = page.update {
                    self.audio.playEntry(payload: payload, update: update, side: side, index: index)
                }
                self.onAction?(action)
            }
            page.onSignal = { [weak self] page, signal, body in
                guard let self, let update = page.update else { return }
                if signal == "mediaPlayback", page === self.current, page.behaviorAvailable, !self.suspended, !self.lifecycleSuspended {
                    if body["playing"] as? Bool == true {
                        if self.htmlAudioOwner == nil {
                            let lease = UUID()
                            self.htmlAudioOwner = lease
                            Task { @MainActor [weak self] in
                                guard self?.htmlAudioOwner == lease else { return }
                                do { try await AudioSessionCoordinator.shared.activate(lease) }
                                catch {
                                    if self?.htmlAudioOwner == lease {
                                        self?.htmlAudioOwner = nil
                                        self?.diagnostics.record("audioSession", update: update)
                                    }
                                }
                                if self?.htmlAudioOwner != lease { AudioSessionCoordinator.shared.release(lease) }
                            }
                        }
                    } else { self.releaseHTMLAudio() }
                }
                self.onSignal?(signal, update)
                if page === self.current, page.behaviorAvailable, !self.suspended, !self.lifecycleSuspended { self.onInteraction?(signal, body) }
            }
            page.onConfirmation = { [weak self] page, body in
                guard let self, page === self.current, page.presented else { return }
                let recorder = page.update?.side == .answer ? self.reveals : self.swaps
                recorder.receive(body, at: ProcessInfo.processInfo.systemUptime)
            }
        }
        let events = AudioSessionCoordinator.shared.events()
        lifecycle = Task { [weak self] in
            for await event in events {
                guard let self else { return }
                // Record lifecycle changes synchronously here. A pending
                // WebKit operation must leave this event loop free to advance.
                guard !self.closed else { return }
                switch event {
                case .stop:
                    self.lifecycleSuspended = true
                    self.beginSuspension()
                case .pause:
                    for page in self.pages { page.pauseMediaWithoutWaiting() }
                    self.releaseHTMLAudio()
                case .available:
                    self.lifecycleSuspended = false
                    _ = self.beginResumption()
                }
            }
        }
    }

    isolated deinit { close() }

    public func load() async throws {
        for page in pages {
            let reconstruct = page === idle && page.needsReconstruction
            if reconstruct { try page.prepareReplacementDocument() }
            try await page.load()
            if reconstruct, let hostSource { try await page.hostReports.install(hostSource) }
        }
    }

    /// Settles runtime/appearance, then awaits host-context publication.
    /// An absent/failing host leaves probes unavailable without failing render.
    @discardableResult
    public func prepare(_ payload: CardPayload) async throws -> CardUpdate {
        guard !closed, !suspended, !lifecycleSuspended else { throw CardPage.Failure.closed }
        generation += 1
        let update = CardUpdate(card: payload.cardID, generation: generation, side: .question)
        let target = idle
        let reconstruct = target.needsReconstruction
        if reconstruct { try target.prepareReplacementDocument() }
        if let previous = target.update { audio.cancelPreparation(previous) }
        target.begin(payload, update: update)
        audio.prepare(payload: payload, update: update)
        do {
            try await withTaskCancellationHandler {
                try Task.checkCancellation()
                if reconstruct {
                    try await target.load()
                    guard target.update == update, !closed else { throw CardPage.Failure.superseded }
                    if let hostSource { try await target.hostReports.install(hostSource) }
                }
                try await target.start(html: payload.question, answer: payload.answer, update: update)
                try await target.waitForSettled(update)
                try await target.refreshAppearance()
                try Task.checkCancellation()
                guard target.update == update, target.settled else { throw CardPage.Failure.superseded }
                await target.hostReports.publish()
                try Task.checkCancellation()
                guard !closed, !suspended, !lifecycleSuspended, target.update == update, target.settled else { throw CardPage.Failure.superseded }
            } onCancel: {
                Task { @MainActor [weak target] in
                    if target?.update == update { target?.cancelUpdate() }
                }
            }
            return update
        } catch {
            if target.update == update { target.invalidate(); audio.cancelPreparation(update) }
            throw error
        }
    }

    /// The reorder is first, synchronous and in the caller's CA transaction.
    /// Return the token to the independent frame probe; never end it at rAF.
    @discardableResult
    public func present(_ update: CardUpdate, inputTime: TimeInterval = ProcessInfo.processInfo.systemUptime) throws -> SwapToken {
        guard !closed, !suspended, !lifecycleSuspended, idle.update == update, idle.settled else { throw CardPage.Failure.notPrepared }
        let old = current
        let next = idle
        cancelPresentationWork()
        cancelRecovery()
        Self.raise(next.webView, in: container)
        next.beginPresentation()
        (current, idle) = (next, old)
        next.webView.setIdle(false)
        old.webView.setIdle(true)
        #if os(macOS)
        container.setAccessibilityChildren([next.webView])
        container.setAccessibilityChildrenInNavigationOrder([next.webView])
        container.window?.recalculateKeyViewLoop()
        #endif
        old.leavePresentation()
        old.queueUnpresent()
        releaseHTMLAudio()
        focus(next)
        let token = swaps.beginSwap(to: update.card, inputTime: inputTime)
        next.visibleToken = token.sequence
        if let payload = next.payload { audio.beginTransition(to: update, policy: payload.audioPolicy) }
        if let previous = preparedAnswer { audio.cancelPreparation(previous.answer) }
        generation += 1
        let answer = CardUpdate(card: update.card, generation: generation, side: .answer)
        preparedAnswer = (update, answer)
        if let payload = next.payload { audio.prepare(payload: payload, update: answer) }
        commitSerial += 1
        let serial = commitSerial
        CATransaction.setCompletionBlock { [weak self, weak next] in
            Task { @MainActor in
                guard let self, !self.closed, !self.suspended, !self.lifecycleSuspended, self.commitSerial == serial, let next, self.current === next, next.update == update else { return }
                if let payload = next.payload { self.audio.present(payload: payload, update: update) }
                do { try await next.present(token: token.sequence) }
                catch { next.diagnostics.record("presentFailed", update: update) }
                guard next.update == update, self.current === next, !self.closed, !self.suspended,
                      !self.lifecycleSuspended, self.commitSerial == serial else { return }
                self.onCommitted?(update, token)
            }
        }
        return token
    }

    /// Returns after matching shown hooks and host-context publication.
    @discardableResult
    public func showAnswer(inputTime: TimeInterval = ProcessInfo.processInfo.systemUptime) async throws -> SwapToken {
        let page = current
        guard !closed, !suspended, !lifecycleSuspended, !page.needsReconstruction, page.presented, let previous = page.update, previous.side == .question, let payload = page.payload else { throw CardPage.Failure.notPrepared }
        #if DEBUG
        let logger = Logger(subsystem: "Meguri", category: "ReviewAnswerStage")
        logger.debug("questionShown.begin")
        #endif
        try await page.waitForShown(previous)
        #if DEBUG
        logger.debug("questionShown.end")
        logger.debug("typedRead.begin")
        #endif
        let typed = await page.typedText()
        #if DEBUG
        logger.debug("typedRead.end")
        #endif
        try Task.checkCancellation()
        guard page.update == previous, current === page, !closed, !suspended, !lifecycleSuspended else { throw CardPage.Failure.superseded }
        page.captureTypedText(typed)
        #if !os(macOS)
        let pinch = page.webView.scrollView.zoomScale
        #endif
        var answer = payload.answer
        if let descriptor = payload.typeIn {
            #if DEBUG
            logger.debug("typedCompare.begin")
            #endif
            let comparison = try await compare(descriptor, typed)
            #if DEBUG
            logger.debug("typedCompare.end")
            #endif
            answer = answer.replacingOccurrences(of: TypeAnswerDescriptor.placeholderHTML, with: comparison)
        }
        try Task.checkCancellation()
        guard current === page, page.update == previous, !suspended, !lifecycleSuspended else { throw CardPage.Failure.superseded }
        let update: CardUpdate
        if let preparedAnswer, preparedAnswer.question == previous { update = preparedAnswer.answer }
        else { generation += 1; update = CardUpdate(card: previous.card, generation: generation, side: .answer) }
        preparedAnswer = nil
        let token = reveals.beginSwap(to: update.card, inputTime: inputTime)
        cancelPresentationWork()
        page.begin(payload, update: update)
        page.visibleToken = token.sequence
        audio.present(payload: payload, update: update)
        #if DEBUG
        logger.debug("answerIdentity.begin")
        #endif
        try await page.run("meguriCard.present(token, card, generation);",
                           arguments: ["token": token.sequence, "card": update.card, "generation": update.generation], world: CardPage.world)
        #if DEBUG
        logger.debug("answerIdentity.end")
        logger.debug("answerStart.begin")
        #endif
        try await page.start(html: answer, update: update)
        #if DEBUG
        logger.debug("answerStart.end")
        logger.debug("answerShown.begin")
        #endif
        try await page.waitForShown(update)
        #if DEBUG
        logger.debug("answerShown.end")
        #endif
        guard current === page, page.update == update, !closed, !suspended, !lifecycleSuspended else { throw CardPage.Failure.superseded }
        #if !os(macOS)
        page.webView.scrollView.setZoomScale(pinch, animated: false)
        #endif
        #if DEBUG
        logger.debug("accessibility.begin")
        #endif
        await page.announceAnswer()
        #if DEBUG
        logger.debug("accessibility.end")
        logger.debug("hostContext.begin")
        #endif
        await page.hostReports.publish()
        #if DEBUG
        logger.debug("hostContext.end")
        #endif
        try Task.checkCancellation()
        guard !closed, !suspended, !lifecycleSuspended, current === page, page.update == update, page.presented else { throw CardPage.Failure.superseded }
        #if DEBUG
        logger.debug("complete")
        #endif
        return token
    }

    /// Executes in these two documents only. Adding to the shared user-content
    /// controller would leak the review host into previews/other sessions.
    public func installHostScript(_ source: String) async throws {
        let installation = UUID()
        hostInstallation = installation
        hostSource = source
        for page in pages { page.hostReports.disable() }
        do {
            try await load()
            for page in pages {
                try Task.checkCancellation()
                guard hostInstallation == installation, !closed else { throw CardPage.Failure.superseded }
                try await page.hostReports.install(source)
            }
            guard hostInstallation == installation, !closed else { throw CardPage.Failure.superseded }
        } catch {
            if hostInstallation == installation { for page in pages { page.hostReports.disable() } }
            throw error
        }
    }
    public func setAppearance(_ value: CardRuntime.Appearance) async throws {
        for page in pages { try await page.setAppearance(value) }
    }
    public func setCardState(flag: UInt8, marked: Bool) async throws { try await current.setState(flag: flag, marked: marked) }
    public func setAudioPolicy(_ policy: CardAudioPolicy) { current.setAudioPolicy(policy) }

    public func configureLinks(preferExternalBrowser: Bool, approvals: CardSchemeApprovals) {
        for page in pages {
            page.behavior.preferExternalBrowser = preferExternalBrowser
            page.behavior.approvals = approvals
        }
    }

    public func replay() {
        guard !closed, !suspended, !lifecycleSuspended, current.behaviorAvailable, let update = current.update, let payload = current.payload else { return }
        audio.replay(payload: payload, update: update)
    }
    public func pauseAudio() { audio.pause() }
    public func resumeAudio() { if !closed && !suspended && !lifecycleSuspended { audio.resume() } }
    public func seekAudio(by seconds: TimeInterval) async { await audio.seek(by: seconds) }

    /// Returns names with no retryable element/stylesheet on the visible side.
    @discardableResult public func mediaArrived(_ names: [String]) async throws -> [String] {
        guard current.behaviorAvailable else { return names }
        let page = current
        page.beginHostMutation()
        let update = page.update
        let result: String
        do {
            result = try await page.string("return JSON.stringify(meguriCard.mediaArrived(names));", arguments: ["names": names], world: CardPage.world)
        } catch {
            page.endHostMutation()
            throw error
        }
        page.endHostMutation()
        await page.hostReports.publish()
        try Task.checkCancellation()
        guard current === page, page.update == update, page.presented, !closed, !suspended, !lifecycleSuspended else { throw CardPage.Failure.superseded }
        return (try? JSONDecoder().decode([String].self, from: Data(result.utf8))) ?? names
    }
    public func suspend() async {
        guard !closed else { return }
        suspended = true
        beginSuspension()
        await waitForPhysicalStop()
    }

    private func cancelPresentationWork() {
        presentationEpoch = UUID()
        presentationWork?.cancel()
        presentationWork = nil
    }

    /// Close page admission synchronously before starting asynchronous cleanup.
    private func beginSuspension() {
        cancelPresentationWork()
        cancelRecovery()
        commitSerial += 1
        preparedAnswer = nil
        audio.stop()
        releaseHTMLAudio()
        idle.invalidate()
        for page in pages { page.suspendPresentation() }
        // Availability may cancel foreground work, but cannot erase the
        // physical pause. Repeated stops replace at most two pending bindings
        // in one worker, including a replacement visit stopped in the meantime.
        stopBindings = pages.map { ($0, $0.update, $0.documentID) }
        guard physicalStop == nil else { return }
        physicalStop = Task { [weak self] in
            guard let self else { return }
            defer {
                self.physicalStop = nil
                self.stopBindings.removeAll()
                self.finishStopWaiters()
            }
            while !self.stopBindings.isEmpty {
                let (page, update, document) = self.stopBindings.removeFirst()
                guard !self.closed, !Task.isCancelled else { return }
                guard page.update == update, page.documentID == document else { continue }
                try? await page.unpresent()
            }
        }
    }

    private func finishStopWaiters() {
        let waiting = stopWaiters.values
        stopWaiters.removeAll()
        for waiter in waiting { waiter.finish() }
    }

    private func waitForPhysicalStop() async {
        guard physicalStop != nil else { return }
        let id = UUID()
        let waiting = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        stopWaiters[id] = waiting.continuation
        defer {
            stopWaiters.removeValue(forKey: id)
            waiting.continuation.finish()
        }
        // AsyncStream ends on caller cancellation, releasing a superseded
        // resume without cancelling the independently owned physical stop.
        for await _ in waiting.stream { break }
    }

    public func resume() async {
        guard !closed else { return }
        suspended = false
        let previousEpoch = presentationEpoch
        let pendingResume = beginResumption()
        let activeEpoch = presentationEpoch
        await withTaskCancellationHandler {
            await pendingResume?.value
        } onCancel: {
            Task { @MainActor [weak self] in
                guard let self, self.presentationEpoch == activeEpoch, activeEpoch != previousEpoch else { return }
                self.cancelPresentationWork()
            }
        }
    }

    /// Replace the pending presentation task. Recheck the captured epoch and
    /// both document bindings after each await; cancellation releases probes.
    private func beginResumption() -> Task<Void, Never>? {
        cancelPresentationWork()
        guard !closed, !suspended, !lifecycleSuspended else { return nil }
        let resumeEpoch = presentationEpoch, boundPage = current
        let updateBindings = pages.map { $0.update }, documentBindings = pages.map { $0.documentID }
        let pendingResume = Task { [weak self, weak boundPage] in
            guard let self, let boundPage else { return }
            defer { if self.presentationEpoch == resumeEpoch { self.presentationWork = nil } }
            @MainActor func canContinue() -> Bool {
                !Task.isCancelled && self.presentationEpoch == resumeEpoch && !self.closed && !self.suspended
                    && !self.lifecycleSuspended && self.current === boundPage
                    && self.pages.map({ $0.update }) == updateBindings && self.pages.map({ $0.documentID }) == documentBindings
            }
            await self.waitForPhysicalStop()
            guard canContinue() else { return }
            for probedPage in self.pages {
                guard canContinue() else { return }
                _ = await probedPage.probeLiveness()
                guard canContinue() else { return }
            }
            if boundPage.needsReconstruction { self.scheduleRecovery() }
            else if self.recovery == nil {
                guard canContinue() else { return }
                try? await boundPage.resumePresentation()
                guard canContinue() else { return }
            }
        }
        presentationWork = pendingResume
        return pendingResume
    }

    public func close() {
        guard !closed else { return }
        closed = true; commitSerial += 1; hostInstallation = UUID()
        #if os(macOS)
        container.setAccessibilityChildren([])
        container.setAccessibilityChildrenInNavigationOrder([])
        #endif
        cancelPresentationWork()
        physicalStop?.cancel(); physicalStop = nil
        stopBindings.removeAll()
        finishStopWaiters()
        cancelRecovery()
        audio.close()
        lifecycle?.cancel(); lifecycle = nil
        releaseHTMLAudio()
        for page in pages { page.close() }
        swaps.finish(); reveals.finish()
        onHostEvent = nil
        onTypedText = nil; onRecovery = nil; onOpenRequest = nil
        onAction = nil; onSignal = nil; onInteraction = nil; onCommitted = nil
    }

    private func releaseHTMLAudio() {
        if let htmlAudioOwner { AudioSessionCoordinator.shared.release(htmlAudioOwner) }
        htmlAudioOwner = nil
    }

    /// Check both documents when U187 enters the foreground. Keep healthy DOM;
    /// an unavailable probe supplies no evidence of process termination.
    @discardableResult public func probeForegroundLiveness(timeoutMs: Int = 1000) async -> [CardPageLiveness] {
        guard !closed else { return [.cancelled, .cancelled] }
        let probeEpoch = presentationEpoch, visiblePage = current
        let updateBindings = pages.map { $0.update }, documentBindings = pages.map { $0.documentID }
        var observations: [CardPageLiveness] = []
        for checkedPage in pages {
            observations.append(await checkedPage.probeLiveness(timeoutMs: timeoutMs))
            guard !closed, !Task.isCancelled, presentationEpoch == probeEpoch, current === visiblePage,
                  pages.map({ $0.update }) == updateBindings, pages.map({ $0.documentID }) == documentBindings else { return observations }
        }
        if visiblePage.needsReconstruction && !suspended && !lifecycleSuspended { scheduleRecovery() }
        return observations
    }

    private func processTerminated(_ page: CardPage) {
        guard !closed else { return }
        if page !== current { page.invalidate(); return }
        cancelPresentationWork()
        commitSerial += 1; preparedAnswer = nil
        audio.stop(); releaseHTMLAudio()
        cancelRecovery()
        if !suspended && !lifecycleSuspended { scheduleRecovery() }
    }
    private func cancelRecovery() {
        recoveryID = UUID()
        recovery?.cancel(); recovery = nil
    }
    private func scheduleRecovery() {
        guard !closed, !suspended, !lifecycleSuspended, recovery == nil,
              current.needsReconstruction, let priorBinding = current.update,
              let cardContent = current.payload, let sideHTML = current.renderedHTML else { return }
        let surface = current
        let savedInput = surface.retainedTypedText
        let job = UUID()
        let scriptInstallation = hostInstallation
        #if !os(macOS)
        let savedScale = surface.webView.scrollView.zoomScale
        #endif
        recoveryID = job
        generation += 1
        let replacement = CardUpdate(card: priorBinding.card, generation: generation, side: priorBinding.side)
        recovery = Task { [weak self, weak surface] in
            guard let self, let surface else { return }
            defer { if self.recoveryID == job { self.recovery = nil } }
            @MainActor func validateVisit() throws {
                try Task.checkCancellation()
                guard !self.closed, !self.suspended, !self.lifecycleSuspended, self.recoveryID == job,
                      self.current === surface, surface.update == replacement else { throw CardPage.Failure.superseded }
            }
            do {
                guard !self.closed, self.recoveryID == job, self.current === surface, surface.update == priorBinding else { return }
                try surface.prepareReplacementDocument(restoreValue: true)
                surface.begin(cardContent, update: replacement, preservingVisit: true)
                surface.renderedHTML = sideHTML
                try await surface.load()
                try validateVisit()
                if let hostCode = self.hostSource {
                    guard scriptInstallation == self.hostInstallation else { throw CardPage.Failure.superseded }
                    try await surface.hostReports.install(hostCode)
                    try validateVisit()
                }
                try await surface.start(html: sideHTML, answer: cardContent.answer, update: replacement, reconstruction: true)
                try await surface.waitForSettled(replacement)
                try validateVisit()
                try await surface.restoreTypedText(savedInput, for: replacement)
                try validateVisit()
                let measurements = replacement.side == .answer ? self.reveals : self.swaps
                let receipt = measurements.beginSwap(to: replacement.card, inputTime: ProcessInfo.processInfo.systemUptime)
                try await surface.presentRecovered(token: receipt.sequence, update: replacement, text: savedInput)
                try validateVisit()
                try await surface.pauseMedia()
                try validateVisit()
                #if !os(macOS)
                surface.webView.scrollView.setZoomScale(savedScale, animated: false)
                #endif
                surface.finishRecovery()
                if replacement.side == .answer { await surface.announceAnswer(allowFocus: false) }
                try validateVisit()
                self.onRecovery?(priorBinding, replacement)
                try validateVisit()
                self.onCommitted?(replacement, receipt)
            } catch {
                if !self.closed, self.recoveryID == job, surface.update == replacement {
                    surface.suspendHost()
                    self.diagnostics.record("recoveryUnavailable", update: replacement)
                }
            }
        }
    }

    private func focus(_ page: CardPage) {
        guard ownsKeyboardFocus else { return }
        #if os(macOS)
        _ = page.webView.window?.makeFirstResponder(page.webView)
        #else
        _ = page.webView.becomeFirstResponder()
        #endif
    }
    static func raise(_ view: CardNativeWebView, in container: CardPlatformView) {
        #if os(macOS)
        container.sortSubviews({ a, b, context in
            let top = context.map { Unmanaged<CardNativeWebView>.fromOpaque($0).takeUnretainedValue() }
            if a === top { return .orderedDescending }
            if b === top { return .orderedAscending }
            return .orderedSame
        }, context: Unmanaged.passUnretained(view).toOpaque())
        #else
        container.bringSubviewToFront(view)
        #endif
    }
}
