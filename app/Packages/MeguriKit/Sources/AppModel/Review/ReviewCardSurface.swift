import CardRendering
import CoreBridge
import Foundation
import QuartzCore

public protocol ReviewCardSurface: AnyObject {
    var pair: CardPagePair? { get }
    var onAction: ((CardBridgeAction) -> Void)? { get set }
    var onRecovery: (() -> Void)? { get set }
    var editing: Bool { get }
    func load() async throws
    func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws
    func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws
    func preload(item: ReviewHeadItem) async throws
    func reveal(inputTime: TimeInterval) async throws
    func admitNativeInput(_ input: NativeReviewInput, coordinator: UUID, session: UInt64, generation: UInt64,
                          deck: Int64, item: ReviewHeadItem, rating: ReviewRating, instant: Int64,
                          eligible: Bool) throws -> NativeReviewInput.AnswerUse
    func suspend() async
    func resume() async
    func close()
}

extension ReviewCardSurface {
    public func admitNativeInput(_ input: NativeReviewInput, coordinator: UUID, session: UInt64, generation: UInt64,
                                 deck: Int64, item: ReviewHeadItem, rating: ReviewRating, instant: Int64,
                                 eligible: Bool) throws -> NativeReviewInput.AnswerUse {
        guard let pair else { throw NativeReviewInput.Refusal(reason: "missing native renderer") }
        try input.validateMediaCurrent(pair: pair, coordinator: coordinator, session: session, generation: generation)
        return try input.admitFacts(coordinator: coordinator, session: session, generation: generation,
            deck: deck, item: item, rating: rating, instant: instant, eligible: eligible)
    }
}

/// Coalesced filenames, bounded even when a review stays paused during sync.
struct ReviewMediaArrivals {
    private var names: Set<String> = []
    private var bytes = 0
    var isEmpty: Bool { names.isEmpty }

    mutating func insert(_ arrivals: [String]) -> Bool {
        for name in arrivals where !names.contains(name) {
            let size = name.utf8.count
            guard names.count < 65_536, size <= 16 * 1_024 * 1_024 - bytes else { return false }
            names.insert(name); bytes += size
        }
        return true
    }
    mutating func take() -> [String] {
        let result = names.sorted()
        names.removeAll(); bytes = 0
        return result
    }
}

/// Persistent page pair, shared by all adaptive presentations of one session.
public final class ReviewPages: ReviewCardSurface {
    public let pair: CardPagePair?
    public var onAction: ((CardBridgeAction) -> Void)?
    public var onRecovery: (() -> Void)?
    public private(set) var editing = false
    public let presenter = NativeCardBehaviorPresenter()
    var inputActive = true { didSet { establishInitialFocus() } }
    private var initialFocus = ReviewInitialFocus()
    private var hasPresented = false
    private var prepared: (ReviewRenderKey, CardUpdate)?
    private var mediaRevision: UInt64 = 0
    private var cards: [ReviewRenderKey: CardPayload] = [:]
    private var renderRevision: UInt64 = 0
    private var closed = false
    private var priorityHead: [ReviewRenderKey] = []
    private var currentMediaRefs: [String] = []
    private(set) var visibleMediaNames: [String] = []
    // Internal render-provider seam for offscreen page tests without a core host.
    var renderHead: (UInt64, [ReviewRenderKey], CardRenderPlatform) async throws -> [CardPayload] = { session, keys, platform in
        try await Core.renderSessionHead(sessionID: session, keys: keys, platform: platform)
            .map { CardPayload($0, audioPolicy: .init()) }
    }
    private var domEditable = false
    private var composing = false
    private var committedMediaUpdate: CardUpdate?
    var onMediaReady: (() -> Void)?
    public convenience init(resolver: MediaPathResolver) {
        self.init(pair: CardPagePair(resolver: resolver))
    }
    init(pair: CardPagePair) {
        self.pair = pair
        pair.onCommitted = { [weak self] update, _ in
            guard let self, let pair = self.pair, pair.current.update == update else { return }
            self.observeHost(pair.current, update: update, purpose: .preload)
            self.hasPresented = true
            self.committedMediaUpdate = update
            self.establishInitialFocus()
            self.onMediaReady?()
        }
        pair.behaviorPresenter = presenter
        pair.configureLinks(preferExternalBrowser: false, approvals: CardSchemeApprovals())
        pair.onAction = { [weak self] in self?.onAction?($0) }
        pair.onRecovery = { [weak self] _, _ in self?.onRecovery?() }
        pair.onInteraction = { [weak self] signal, body in
            guard let self else { return }
            if signal == "focus" { domEditable = body["editable"] as? Bool == true }
            if signal == "composition" { composing = body["active"] as? Bool == true }
            editing = domEditable || composing
        }
    }
    public func load() async throws {
        guard !closed, let pair else { throw CancellationError() }
        try await pair.load()
        try Task.checkCancellation()
        guard !closed else { throw CancellationError() }
        do { try await pair.installHostScript(CardRuntime.reviewHostScript) }
        catch is CardHostReporting.InstallationFailure {
            // Reporting is optional; the pair has already disabled admission.
        }
        try Task.checkCancellation()
        guard !closed else { throw CancellationError() }
    }
    /// Issuance is synchronous and never waits for a result. Native retains
    /// only matching slot facts and owns cancellation/deadline/revision guards.
    private func observeHost(_ page: CardPage, update: CardUpdate, purpose: CardHostResourcePurpose) {
        guard !closed, page.update == update,
              let context = page.hostReports.context, context.update == update else { return }
        page.hostReports.probe(.background) { _, _ in }
        // A cancellation callback or geometry change must not give the second
        // request a newer context than the observation point captured above.
        guard page.hostReports.context == context else { return }
        page.hostReports.probe(.resources, purpose: purpose) { _, _ in }
    }
    /// Gives the first presented card its initial native focus.
    public func establishInitialFocus() {
        guard let pages = pair else { return }
        let responderIsOwned = ReviewInputOwnership.initialNativeInputOwned(in: pages.current.webView)
        let acquired = initialFocus.claim(
            presented: hasPresented, active: inputActive, nativeInputOwned: responderIsOwned)
        guard acquired else { return }
        pages.ownsKeyboardFocus = true
        ReviewInputOwnership.focusCard(pages.current.webView)
    }
    private func payload(_ rendered: CardPayload, item: ReviewHeadItem) -> CardPayload {
        var result = rendered
        result.audioPolicy = CardAudioPolicy(
            autoplay: item.homePreset.autoplay, interrupt: true,
            replayQuestion: !item.homePreset.skipQuestionWhenReplayingAnswer)
        return result
    }
    public func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws {
        guard !closed else { throw CancellationError() }
        renderRevision &+= 1
        let revision = renderRevision
        let missing = items.filter { cards[$0.renderKey] == nil }
        #if os(macOS)
        let platform: CardRenderPlatform = .mac
        #else
        let platform: CardRenderPlatform = .ios
        #endif
        if !missing.isEmpty {
            let values = try await renderHead(sessionID, missing.map(\.renderKey), platform)
            try Task.checkCancellation()
            guard !closed, revision == renderRevision else { throw CancellationError() }
            guard values.count == missing.count else { throw BridgeError.internal("Incomplete rendered head") }
            for (item, card) in zip(missing, values) { cards[item.renderKey] = card }
        }
        try Task.checkCancellation()
        guard !closed, revision == renderRevision else { throw CancellationError() }
        cards = cards.filter { key, _ in items.contains { $0.renderKey == key } }
        priorityHead = items.prefix(3).map(\.renderKey)
        if items.isEmpty { currentMediaRefs = [] }
        updateVisibleMedia()
    }
    public func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws {
        guard let pair else { throw BridgeError.internal("Missing review pages") }
        guard let rendered = cards[item.renderKey] else { throw BridgeError.internal("Missing rendered card") }
        let update: CardUpdate
        if let prepared, prepared.0 == item.renderKey { update = prepared.1 }
        else {
            update = try await pair.prepare(payload(rendered, item: item))
            observeHost(pair.idle, update: update, purpose: .warmup)
        }
        try Task.checkCancellation()
        guard !closed else { throw CancellationError() }
        prepared = nil; editing = false; domEditable = false; composing = false
        pair.ownsKeyboardFocus = inputActive && ReviewInputOwnership.cardFocused(in: pair.current.webView)
        CATransaction.begin()
        defer { CATransaction.commit() }
        _ = try pair.present(update, inputTime: inputTime)
        currentMediaRefs = rendered.mediaRefs
        updateVisibleMedia()
        if !inputActive { pair.pauseAudio() }
    }
    public func preload(item: ReviewHeadItem) async throws {
        guard let pair else { return }
        guard let rendered = cards[item.renderKey] else { throw BridgeError.internal("Missing preloaded card") }
        let revision = mediaRevision
        let update = try await pair.prepare(payload(rendered, item: item))
        // An arrival can be delivered while the idle page is still settling.
        // Do not publish that older preparation after its invalidation.
        guard !closed, !Task.isCancelled, revision == mediaRevision else { return }
        observeHost(pair.idle, update: update, purpose: .preload)
        prepared = (item.renderKey, update)
    }
    public func reveal(inputTime: TimeInterval) async throws {
        _ = try await pair?.showAnswer(inputTime: inputTime)
        committedMediaUpdate = pair?.current.update
        if let pair, let update = pair.current.update {
            observeHost(pair.current, update: update, purpose: .preload)
        }
        if !inputActive { pair?.pauseAudio() }
    }
    /// False keeps arrivals pending until the native presentation is committed.
    func retryMedia(_ names: [String]) async throws -> Bool {
        guard inputActive, let pair, let committedMediaUpdate,
              committedMediaUpdate == pair.current.update, pair.current.presented,
              !pair.current.closed else { return false }
        // Only the visible side is retried in place. Every idle preparation
        // predates this arrival, including resources absent from the visible side.
        // The next present re-prepares idle; the current DOM stays untouched.
        mediaRevision &+= 1
        prepared = nil
        try await pair.mediaArrived(names)
        return true
    }
    public func suspend() async { pair?.ownsKeyboardFocus = false; await pair?.suspend() }
    public func resume() async {
        await pair?.resume()
        guard !Task.isCancelled, let pair, let update = pair.current.update else { return }
        observeHost(pair.current, update: update, purpose: .warmup)
    }
    private func updateVisibleMedia() {
        visibleMediaNames = Set(currentMediaRefs + priorityHead.flatMap { cards[$0]?.mediaRefs ?? [] }).sorted()
    }
    func clearVisibleMedia() {
        renderRevision &+= 1
        priorityHead = []; currentMediaRefs = []; visibleMediaNames = []
    }
    public func close() {
        closed = true; clearVisibleMedia()
        mediaRevision &+= 1; prepared = nil; committedMediaUpdate = nil; pair?.close()
    }
}
