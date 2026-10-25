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
    func suspend() async
    func resume() async
    func close()
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
            self?.hasPresented = true
            self?.committedMediaUpdate = update
            self?.establishInitialFocus()
            self?.onMediaReady?()
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
    public func load() async throws { try await pair?.load() }
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
        let missing = items.filter { cards[$0.renderKey] == nil }
        #if os(macOS)
        let platform: CardRenderPlatform = .mac
        #else
        let platform: CardRenderPlatform = .ios
        #endif
        if !missing.isEmpty {
            let values = try await renderHead(sessionID, missing.map(\.renderKey), platform)
            guard values.count == missing.count else { throw BridgeError.internal("Incomplete rendered head") }
            for (item, card) in zip(missing, values) { cards[item.renderKey] = card }
        }
        cards = cards.filter { key, _ in items.contains { $0.renderKey == key } }
    }
    public func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws {
        guard let pair else { throw BridgeError.internal("Missing review pages") }
        guard let rendered = cards[item.renderKey] else { throw BridgeError.internal("Missing rendered card") }
        let update: CardUpdate
        if let prepared, prepared.0 == item.renderKey { update = prepared.1 }
        else { update = try await pair.prepare(payload(rendered, item: item)) }
        prepared = nil; editing = false; domEditable = false; composing = false
        pair.ownsKeyboardFocus = inputActive && ReviewInputOwnership.cardFocused(in: pair.current.webView)
        CATransaction.begin()
        defer { CATransaction.commit() }
        _ = try pair.present(update, inputTime: inputTime)
        if !inputActive { pair.pauseAudio() }
    }
    public func preload(item: ReviewHeadItem) async throws {
        guard let pair else { return }
        guard let rendered = cards[item.renderKey] else { throw BridgeError.internal("Missing preloaded card") }
        let revision = mediaRevision
        let update = try await pair.prepare(payload(rendered, item: item))
        // An arrival can be delivered while the idle page is still settling.
        // Do not publish that older preparation after its invalidation.
        guard revision == mediaRevision else { return }
        prepared = (item.renderKey, update)
    }
    public func reveal(inputTime: TimeInterval) async throws {
        _ = try await pair?.showAnswer(inputTime: inputTime)
        committedMediaUpdate = pair?.current.update
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
    public func resume() async { await pair?.resume() }
    public func close() { mediaRevision &+= 1; prepared = nil; committedMediaUpdate = nil; pair?.close() }
}
