import CoreBridge
import Foundation
import Instrumentation
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
            for page in pages { page.webView.reviewOwnsFocus = ownsKeyboardFocus }
            #if os(macOS)
            container.window?.recalculateKeyViewLoop()
            #endif
        }
    }
    public var onAction: ((CardBridgeAction) -> Void)?
    public var onSignal: ((String, CardUpdate) -> Void)?
    public var onInteraction: ((String, [String: Any]) -> Void)?
    public var onHostEvent: ((CardPage, CardHostEvent) -> Void)?
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
    private var htmlAudioOwner: UUID?
    private var lifecycle: Task<Void, Never>?
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
        for page in pages {
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
                if signal == "mediaPlayback", page === self.current, page.presented {
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
                if page === self.current, page.presented { self.onInteraction?(signal, body) }
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
                switch event {
                case .stop:
                    self.lifecycleSuspended = true
                    await self.pauseAll()
                case .pause:
                    for page in self.pages { try? await page.pauseMedia() }
                    self.releaseHTMLAudio()
                case .available:
                    self.lifecycleSuspended = false
                    if !self.suspended { try? await self.current.resumePresentation() }
                }
            }
        }
    }

    isolated deinit { close() }

    public func load() async throws {
        for page in pages { try await page.load() }
    }

    @discardableResult
    public func prepare(_ payload: CardPayload) async throws -> CardUpdate {
        guard !closed, !suspended, !lifecycleSuspended else { throw CardPage.Failure.closed }
        generation += 1
        let update = CardUpdate(card: payload.cardID, generation: generation, side: .question)
        let target = idle
        if let previous = target.update { audio.cancelPreparation(previous) }
        target.begin(payload, update: update)
        audio.prepare(payload: payload, update: update)
        do {
            try await withTaskCancellationHandler {
                try Task.checkCancellation()
                try await target.start(html: payload.question, answer: payload.answer, update: update)
                try await target.waitForSettled(update)
                try await target.refreshAppearance()
                try Task.checkCancellation()
                guard target.update == update, target.settled else { throw CardPage.Failure.superseded }
            } onCancel: {
                Task { @MainActor [weak target] in
                    if target?.update == update { target?.cancelUpdate() }
                }
            }
            Task { [weak target] in await target?.hostReports.publish() }
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
        Self.raise(next.webView, in: container)
        next.beginPresentation()
        (current, idle) = (next, old)
        next.webView.setIdle(false)
        old.webView.setIdle(true)
        #if os(macOS)
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
                guard next.update == update, self.current === next, !self.closed else { return }
                self.onCommitted?(update, token)
            }
        }
        return token
    }

    @discardableResult
    public func showAnswer(inputTime: TimeInterval = ProcessInfo.processInfo.systemUptime) async throws -> SwapToken {
        let page = current
        guard !closed, !suspended, !lifecycleSuspended, page.presented, let previous = page.update, previous.side == .question, let payload = page.payload else { throw CardPage.Failure.notPrepared }
        try await page.waitForShown(previous)
        let typed = await page.typedText()
        try Task.checkCancellation()
        var answer = payload.answer
        if let descriptor = payload.typeIn {
            let comparison = try await compare(descriptor, typed)
            answer = answer.replacingOccurrences(of: TypeAnswerDescriptor.placeholderHTML, with: comparison)
        }
        try Task.checkCancellation()
        guard current === page, page.update == previous, !suspended, !lifecycleSuspended else { throw CardPage.Failure.superseded }
        let update: CardUpdate
        if let preparedAnswer, preparedAnswer.question == previous { update = preparedAnswer.answer }
        else { generation += 1; update = CardUpdate(card: previous.card, generation: generation, side: .answer) }
        preparedAnswer = nil
        let token = reveals.beginSwap(to: update.card, inputTime: inputTime)
        page.begin(payload, update: update)
        page.visibleToken = token.sequence
        audio.present(payload: payload, update: update)
        try await page.run("meguriCard.present(token);", arguments: ["token": token.sequence], world: CardPage.world)
        try await page.start(html: answer, update: update)
        return token
    }

    /// Executes in these two documents only. Adding to the shared user-content
    /// controller would leak the review host into previews/other sessions.
    public func installHostScript(_ source: String) async throws {
        let installation = UUID()
        hostInstallation = installation
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

    public func replay() {
        guard !closed, !suspended, !lifecycleSuspended, current.presented, let update = current.update, let payload = current.payload else { return }
        audio.replay(payload: payload, update: update)
    }
    public func pauseAudio() { audio.pause() }
    public func resumeAudio() { if !closed && !suspended && !lifecycleSuspended { audio.resume() } }
    public func seekAudio(by seconds: TimeInterval) async { await audio.seek(by: seconds) }

    /// Returns names with no retryable element/stylesheet on the visible side.
    @discardableResult public func mediaArrived(_ names: [String]) async throws -> [String] {
        guard current.presented else { return names }
        let page = current
        page.beginHostMutation()
        defer { page.endHostMutation() }
        let result = try await page.string("return JSON.stringify(meguriCard.mediaArrived(names));", arguments: ["names": names], world: CardPage.world)
        return (try? JSONDecoder().decode([String].self, from: Data(result.utf8))) ?? names
    }
    public func suspend() async {
        suspended = true
        await pauseAll()
    }
    private func pauseAll() async {
        commitSerial += 1
        preparedAnswer = nil
        audio.stop()
        releaseHTMLAudio()
        idle.invalidate()
        for page in pages { page.suspendHost() }
        for page in pages { try? await page.unpresent() }
    }
    public func resume() async {
        suspended = false
        if !lifecycleSuspended { try? await current.resumePresentation() }
    }

    public func close() {
        guard !closed else { return }
        closed = true; commitSerial += 1; hostInstallation = UUID()
        audio.close()
        lifecycle?.cancel(); lifecycle = nil
        releaseHTMLAudio()
        for page in pages { page.close() }
        swaps.finish(); reveals.finish()
        onHostEvent = nil
        onAction = nil; onSignal = nil; onInteraction = nil; onCommitted = nil
    }

    private func releaseHTMLAudio() {
        if let htmlAudioOwner { AudioSessionCoordinator.shared.release(htmlAudioOwner) }
        htmlAudioOwner = nil
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
