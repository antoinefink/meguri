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
    /// A flip's own count, so a flip never drops a card's render under way.
    @ObservationIgnored private var flips: UInt64 = 0
    /// Shows not finished yet; a flip waits until there are none.
    @ObservationIgnored private(set) var showing = 0
    /// Raised by `close`: a load of a closed collection's pair lands nothing.
    @ObservationIgnored private var generation: UInt64 = 0
    @ObservationIgnored private var payload: CardPayload?

    init() {}

    /// Shows `card`'s question; a newer call supersedes an older one.
    func show(card id: Int64) {
        token &+= 1
        let token = token
        showing += 1
        Task {
            defer { showing -= 1 }
            do {
                guard let pair = try await readyPair(), token == self.token else { return }
                let payload = try await render(id)
                guard token == self.token else { return }
                let update = try await pair.prepare(payload)
                guard token == self.token else { return }
                try pair.present(update)
                self.payload = payload
                hasAudio = !payload.questionAudio.isEmpty || !payload.answerAudio.isEmpty
                side = .question
                card = id
                failed = false
            } catch {
                if token == self.token { failed = true }
            }
        }
    }

    /// V12: a click flips question and answer.
    public func flip() {
        // A card on its way to the page goes first: a flip's own prepare would
        // supersede it and leave the preview on the previous row's card.
        guard showing == 0, let pair, let payload, card != nil else { return }
        flips &+= 1
        let flip = flips, token = token
        Task {
            do {
                if side == .question {
                    try await pair.showAnswer()
                    if flip == flips, token == self.token { side = .answer }
                } else {
                    let update = try await pair.prepare(payload)
                    guard flip == flips, token == self.token else { return }
                    try pair.present(update)
                    side = .question
                }
            } catch {}
        }
    }

    /// The side shown's sounds, from the start (they never play by themselves).
    public func replay() { pair?.replay() }

    /// The system appearance the card is drawn in.
    public func setAppearance(_ value: CardRuntime.Appearance) {
        guard value != appearance else { return }
        appearance = value
        guard let pair else { return }
        Task { try? await pair.setAppearance(value) }
    }

    /// Nothing to show (no card, several rows).
    func clear() {
        token &+= 1
        card = nil
        hasAudio = false
        payload = nil
        pair?.pauseAudio()
    }

    func close() {
        token &+= 1
        generation &+= 1
        loading = nil
        pair?.close()
        pair = nil
        loaded = false
        card = nil
        payload = nil
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
