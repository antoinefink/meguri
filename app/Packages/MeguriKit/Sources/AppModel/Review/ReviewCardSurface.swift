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
    private var cards: [ReviewRenderKey: RenderedCard] = [:]
    private var domEditable = false
    private var composing = false
    public init(resolver: MediaPathResolver) {
        let pair = CardPagePair(resolver: resolver)
        self.pair = pair
        pair.onCommitted = { [weak self] _, _ in
            self?.hasPresented = true
            self?.establishInitialFocus()
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
    private func payload(_ rendered: RenderedCard, item: ReviewHeadItem) throws -> CardPayload {
        return CardPayload(rendered, audioPolicy: CardAudioPolicy(
            autoplay: item.homePreset.autoplay, interrupt: true,
            replayQuestion: !item.homePreset.skipQuestionWhenReplayingAnswer))
    }
    public func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws {
        let missing = items.filter { cards[$0.renderKey] == nil }
        #if os(macOS)
        let platform: CardRenderPlatform = .mac
        #else
        let platform: CardRenderPlatform = .ios
        #endif
        if !missing.isEmpty {
            let values = try await Core.renderSessionHead(sessionID: sessionID, keys: missing.map(\.renderKey), platform: platform)
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
        prepared = (item.renderKey, try await pair.prepare(payload(rendered, item: item)))
    }
    public func reveal(inputTime: TimeInterval) async throws {
        _ = try await pair?.showAnswer(inputTime: inputTime)
        if !inputActive { pair?.pauseAudio() }
    }
    public func suspend() async { pair?.ownsKeyboardFocus = false; await pair?.suspend() }
    public func resume() async { await pair?.resume() }
    public func close() { prepared = nil; pair?.close() }
}
