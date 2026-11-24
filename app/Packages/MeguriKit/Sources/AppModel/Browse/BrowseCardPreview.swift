import CardRendering
import CoreBridge
import Foundation
import Observation

/// The inspector's card preview (`browser/workspace` V12, `browser/preview`
/// P8): the shown card as review renders it, in the review's own page
/// pair, so the note type's styling shows as the user wrote it. Audio does
/// not play by itself; a click flips question and answer. It renders
/// after the fields show (V17) and again after each save.
@Observable @MainActor
public final class BrowseCardPreview {
    public private(set) var pair: CardPagePair?
    /// The side shown.
    public private(set) var side: CardRenderSide = .question
    /// The card shown, once its question is on screen.
    public private(set) var card: Int64?
    /// The last render failed (a template error, a missing card).
    public private(set) var failed = false
    /// The card shown has sounds: the preview's header offers to play them.
    public private(set) var hasAudio = false

    /// The collection's media, bound as review binds it.
    @ObservationIgnored var resolver: @MainActor () async -> MediaPathResolver? = { nil }
    @ObservationIgnored var render: @MainActor (Int64) async throws -> CardPayload = { id in
        #if os(macOS)
        let platform: CardRenderPlatform = .mac
        #else
        let platform: CardRenderPlatform = .ios
        #endif
        let rendered = try await Core.renderCard(id: id, mode: .preview, platform: platform)
        return CardPayload(rendered, audioPolicy: CardAudioPolicy(autoplay: false, interrupt: true, replayQuestion: false))
    }
    @ObservationIgnored private var appearance: CardRuntime.Appearance = .light
    @ObservationIgnored private var loaded = false
    @ObservationIgnored private var token: UInt64 = 0
    /// Serialize page mutations: an answer reveal must finish before another
    /// side or card prepares the same pair. The latest selection wins.
    @ObservationIgnored private var presentation: Task<Void, Never>?
    @ObservationIgnored private var displayedSide: CardRenderSide = .question
    @ObservationIgnored private var requestedCard: Int64?
    @ObservationIgnored private var committing: (CardUpdate, CheckedContinuation<Void, any Error>)?
    /// Shows not finished yet; a flip waits until there are none.
    @ObservationIgnored private(set) var showing = 0
    /// Raised by `close`: a load of a closed collection's pair lands nothing.
    @ObservationIgnored private var generation: UInt64 = 0
    @ObservationIgnored private var payload: CardPayload?

    init() {}

    /// A new card starts on Front; a save of the visible card keeps its side.
    func show(card id: Int64) {
        cancelCommit()
        token &+= 1
        let token = token
        if requestedCard != id { side = .question }
        requestedCard = id
        showing += 1
        let previous = presentation
        presentation = Task {
            await previous?.value
            guard token == self.token else { showing -= 1; return }
            defer { showing -= 1 }
            do {
                guard let pair = try await readyPair(), token == self.token else { return }
                let payload = try await render(id)
                guard token == self.token else { return }
                let update = try await pair.prepare(payload)
                guard token == self.token else { return }
                try await present(pair, update: update)
                self.payload = payload
                hasAudio = !payload.questionAudio.isEmpty || !payload.answerAudio.isEmpty
                displayedSide = .question
                card = id
                try await presentSelectedSide(token: token)
                if token == self.token { failed = false }
            } catch {
                if token == self.token { failed = true }
            }
        }
    }

    /// The segmented control sets its side directly, even during a render.
    public func selectSide(_ value: CardRenderSide) {
        guard requestedCard != nil, side != value else { return }
        side = value
        pair?.pauseAudio()
        let token = token, previous = presentation
        presentation = Task {
            await previous?.value
            do { try await presentSelectedSide(token: token) }
            catch { if token == self.token { failed = true } }
        }
    }

    /// V12: clicking the card uses the same selection as the native picker.
    public func flip() { selectSide(side == .question ? .answer : .question) }

    private func presentSelectedSide(token: UInt64) async throws {
        guard let pair, let payload else { return }
        while token == self.token, displayedSide != side {
            let target = side
            if target == .answer {
                try await pair.showAnswer()
            } else {
                let update = try await pair.prepare(payload)
                guard token == self.token else { return }
                try await present(pair, update: update)
            }
            guard token == self.token else { return }
            displayedSide = target
        }
    }

    /// The side shown's sounds, from the start (they never play by themselves).
    public func replay() {
        guard showing == 0, displayedSide == side, !failed else { return }
        pair?.replay()
    }

    /// The system appearance the card is drawn in.
    public func setAppearance(_ value: CardRuntime.Appearance) {
        guard value != appearance else { return }
        appearance = value
        guard let pair else { return }
        Task { try? await pair.setAppearance(value) }
    }

    /// Nothing to show (no card, several rows).
    func clear() {
        cancelCommit()
        token &+= 1
        card = nil
        requestedCard = nil
        side = .question
        hasAudio = false
        payload = nil
        pair?.pauseAudio()
    }

    func close() {
        cancelCommit()
        token &+= 1
        generation &+= 1
        loading = nil
        pair?.close()
        pair = nil
        loaded = false
        requestedCard = nil
        side = .question
        hasAudio = false
        card = nil
        payload = nil
    }

    /// Answer reveal requires the question's native presentation to commit.
    /// Join the pair's acknowledgement instead of racing its CA completion.
    private func present(_ pair: CardPagePair, update: CardUpdate) async throws {
        try await withCheckedThrowingContinuation { continuation in
            committing = (update, continuation)
            do { try pair.present(update) }
            catch { committing = nil; continuation.resume(throwing: error) }
        }
    }

    private func cancelCommit() {
        let pending = committing
        committing = nil
        pending?.1.resume(throwing: CancellationError())
    }

    /// The pair, made and loaded once; callers meanwhile wait for the same load.
    private func readyPair() async throws -> CardPagePair? {
        if let pair, loaded { return pair }
        if let loading { return try await loading.value }
        let generation = generation
        let work = Task { () throws -> CardPagePair? in
            if pair == nil {
                guard let resolver = await resolver(), generation == self.generation else { return nil }
                let made = CardPagePair(resolver: resolver, appearance: appearance)
                made.configureLinks(preferExternalBrowser: true, approvals: CardSchemeApprovals())
                made.onCommitted = { [weak self] update, _ in
                    guard let self, let pending = committing, pending.0 == update else { return }
                    committing = nil
                    pending.1.resume()
                }
                pair = made
            }
            guard let pair else { return nil }
            try await pair.load()
            guard generation == self.generation else { return nil }
            loaded = true
            return pair
        }
        loading = work
        defer { if generation == self.generation { loading = nil } }
        return try await work.value
    }
    @ObservationIgnored private var loading: Task<CardPagePair?, any Error>?
}
