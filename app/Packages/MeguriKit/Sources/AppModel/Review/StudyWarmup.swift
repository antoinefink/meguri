import CardRendering
import CoreBridge
import Foundation
import Observation

/// Warm card pages, and the first card of the deck a visible Study targets
/// (R1813, Antoine 2026-11-21: "make the Study click instant"). Study then
/// starts its session on pages that are already loaded, with that card
/// already prepared in the idle page.
///
/// Reads only, before any session: the queue head a session would show
/// first (`sched/review-session` C3: no `curDeck`, no hook, no queue kept)
/// and that card's render. Nothing here starts a session, selects a deck,
/// writes the study day, touches undo or changes counts; no audio plays and
/// nothing is presented. The session uses the prepared card only when its
/// own first head item has the same card, state token and render key, and
/// its own render the same payload (C3, C6, `ReviewPages.present`); any
/// other card is prepared again as without a warm-up.
@Observable
final class StudyWarmup {
    /// The card prepared for `deckID`: the head item the peek gave.
    struct Prepared: Equatable {
        let deckID: Int64
        let item: ReviewHeadItem
    }
    /// Who set the target: a view's own token, so a screen that disappears
    /// after the next one appeared clears nothing.
    struct Owner: Hashable { let id: UUID }

    /// The one warm surface, until a session takes it or it is released.
    private(set) var surface: (any ReviewCardSurface)?
    @ObservationIgnored private var surfaceResolver: MediaPathResolver?
    @ObservationIgnored private(set) var target: Int64?
    @ObservationIgnored private var targetOwner: Owner?
    @ObservationIgnored private(set) var prepared: Prepared?
    /// A change may have moved the head since `prepared` was read: it is
    /// checked again before Study may use it.
    @ObservationIgnored private(set) var stale = false
    @ObservationIgnored private var revision: UInt64 = 0
    @ObservationIgnored private var work: Task<Void, Never>?
    @ObservationIgnored private var pressure: (any DispatchSourceMemoryPressure)?

    // Seams. Production reads the core; the collection supplies admission and media.
    @ObservationIgnored var makeSurface: (MediaPathResolver) -> any ReviewCardSurface
    @ObservationIgnored var peek: (Int64) async throws -> ReviewPeekResult = { try await Core.peekFirstCard(deckID: $0) }
    /// The card's review render (`render/card-render-pipeline`): a reader snapshot, no write.
    @ObservationIgnored var render: (Int64) async throws -> CardPayload = { id in
        #if os(macOS)
        let platform = CardRenderPlatform.mac
        #else
        let platform = CardRenderPlatform.ios
        #endif
        return CardPayload(try await Core.renderCard(id: id, mode: .review, platform: platform), audioPolicy: .init())
    }
    /// Whether warm work may run now: an open, ready collection in the
    /// foreground with no session.
    @ObservationIgnored var admitted: () -> Bool = { false }
    /// The collection's current media binding, never refreshed from here.
    @ObservationIgnored var resolver: () -> MediaPathResolver? = { nil }
    /// How long a change waits before the head is read again, so a burst of
    /// changes (a sync) reads it once.
    @ObservationIgnored var settleDelay: Duration = .milliseconds(250)

    init(makeSurface: @escaping (MediaPathResolver) -> any ReviewCardSurface) {
        self.makeSurface = makeSurface
        let source = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .main)
        source.setEventHandler { [weak self] in
            MainActor.assumeIsolated { self?.release() }
        }
        source.activate()
        pressure = source
    }

    isolated deinit {
        pressure?.cancel()
        work?.cancel()
        surface?.close()
    }

    // MARK: What the screens ask

    /// A screen whose primary action is Study for `deckID` appeared, or its
    /// deck changed. Only this one deck is prepared.
    func setTarget(_ deckID: Int64?, owner: Owner) {
        guard let deckID else { clearTarget(owner); return }
        targetOwner = owner
        guard target != deckID else { return }
        target = deckID
        dropPrepared()
        schedule()
    }

    /// The screen of `owner` disappeared: its card is dropped.
    func clearTarget(_ owner: Owner) {
        guard targetOwner == owner else { return }
        targetOwner = nil
        target = nil
        dropPrepared()
        bump()
    }

    // MARK: What the collection tells

    /// Pages are wanted: after the collection opened, after a session ended,
    /// back in the foreground.
    func warm() { schedule() }

    /// Something may have changed what a session would show first: a change
    /// set, a sync, a new study day, an options change, a count refresh.
    /// The prepared card is held back until the head is read again.
    func invalidate() {
        guard surface != nil else { return }
        if prepared != nil { stale = true }
        schedule(after: settleDelay)
    }

    /// Media came or went: a prepared card may have been settled without a
    /// file it shows, which its payload does not tell. Prepare it again.
    func mediaChanged() {
        guard surface != nil else { return }
        dropPrepared()
        schedule(after: settleDelay)
    }

    /// Releases the pages: memory pressure, the background, a new media
    /// binding, the collection closing or being replaced.
    func release() {
        bump()
        prepared = nil; stale = false
        let old = surface
        surface = nil; surfaceResolver = nil
        old?.close()
    }

    /// Study takes the pages for its session, with the prepared card only
    /// when it is for this deck and was read after the last change. Pages
    /// for another media binding, or that cannot serve a session, are closed.
    func take(deckID: Int64, resolver: MediaPathResolver) -> (surface: any ReviewCardSurface, preloaded: Bool)? {
        bump()
        guard let surface else { return nil }
        let current = prepared
        let fresh = !stale
        self.surface = nil; prepared = nil; stale = false
        guard surfaceResolver === resolver, surface.reusable else {
            surfaceResolver = nil
            surface.close()
            return nil
        }
        surfaceResolver = nil
        let preloaded = fresh && current?.deckID == deckID
        if !preloaded { surface.discardFirst() }
        return (surface, preloaded)
    }

    /// Waits for the current warm work (tests).
    func settled() async {
        while let task = work {
            await task.value
            if work == task { work = nil }
        }
    }

    // MARK: The work

    private func bump() {
        revision &+= 1
        work?.cancel()
        work = nil
    }

    private func dropPrepared() {
        guard prepared != nil else { return }
        prepared = nil; stale = false
        surface?.discardFirst()
    }

    private func schedule(after delay: Duration = .zero) {
        bump()
        let revision = revision
        work = Task { [weak self] in
            if delay > .zero { try? await Task.sleep(for: delay) }
            guard let self, !Task.isCancelled, revision == self.revision else { return }
            await self.run(revision)
        }
    }

    private func run(_ revision: UInt64) async {
        func current() -> Bool { !Task.isCancelled && revision == self.revision && admitted() }
        guard current(), let resolver = resolver() else { return }
        if let surface, surfaceResolver !== resolver || !surface.reusable {
            release()
            return schedule()
        }
        let surface: any ReviewCardSurface
        if let existing = self.surface { surface = existing }
        else {
            surface = makeSurface(resolver)
            self.surface = surface; surfaceResolver = resolver
        }
        do {
            try await surface.load()
            guard current(), surface === self.surface, let deck = target else { return }
            let head = try await peek(deck)
            guard current(), surface === self.surface, target == deck else { return }
            guard case .card(let item) = head else {
                // Nothing to show, or a start that would be refused: nothing to prepare.
                dropPrepared()
                return
            }
            if let prepared, prepared.deckID == deck, Self.sameCard(prepared.item, item) {
                stale = false
                return
            }
            dropPrepared()
            let rendered = try await render(item.cardId)
            guard current(), surface === self.surface, target == deck else { return }
            try await surface.prepareFirst(item: item, payload: rendered)
            guard current(), surface === self.surface, target == deck else {
                if surface === self.surface { surface.discardFirst() }
                return
            }
            prepared = Prepared(deckID: deck, item: item)
            stale = false
        } catch {
            // A warm-up is an optimisation: Study prepares as without one.
            if surface === self.surface, revision == self.revision { dropPrepared(); surface.discardFirst() }
        }
    }

    /// The identity a session's first head item must have to use a prepared
    /// page (`sched/review-session` C3, C6).
    static func sameCard(_ a: ReviewHeadItem, _ b: ReviewHeadItem) -> Bool {
        a.cardId == b.cardId && a.stateToken == b.stateToken && a.renderKey == b.renderKey
    }
}
