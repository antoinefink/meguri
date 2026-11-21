import CoreBridge
import Foundation
import Observation

/// N20.7 (decision 4B): Delete Deck asks only when cards would be lost.
/// Collection-owned, so a layout switch keeps a pending question.
@Observable @MainActor
public final class DeckDeletionModel {
    /// N20.7's message line, from S = `decks` − 1 and C = `cards` (C ≥ 1).
    public nonisolated enum Message: Hashable, Sendable {
        case oneCard
        case cards(UInt64)
        case subdeckAndOneCard
        case subdeckAndCards(UInt64)
        case subdecksAndOneCard(UInt64)
        case subdecksAndCards(UInt64, UInt64)

        /// None when no card would be lost: that delete asks nothing.
        public init?(subdecks: UInt64, cards: UInt64) {
            switch (subdecks, cards) {
            case (_, 0): return nil
            case (0, 1): self = .oneCard
            case (0, _): self = .cards(cards)
            case (1, 1): self = .subdeckAndOneCard
            case (1, _): self = .subdeckAndCards(cards)
            case (_, 1): self = .subdecksAndOneCard(subdecks)
            default: self = .subdecksAndCards(subdecks, cards)
            }
        }
    }

    /// Where the question shows on touch at regular width (N20.7): beside the
    /// row whose menu asked, or in the system's place for the menu bar.
    public nonisolated enum Anchor: Hashable, Sendable {
        case window
        /// A row of one deck list (`place` names the list, so two lists showing
        /// the same deck do not both present it).
        case row(place: String)
    }

    /// The question: title `Delete “<name>”?` and one message line.
    public struct Confirmation: Hashable, Sendable {
        public let deckID: Int64
        public let anchor: Anchor
        /// The deck's last name component.
        public let name: String
        public let subdecks: UInt64
        public let cards: UInt64
        public let message: Message
        fileprivate let summary: DeckDeletionSummary
        fileprivate let generation: UInt64
    }

    /// The alert `Couldn’t Delete “<name>”` after a delete that wrote nothing.
    public struct Failure: Hashable, Sendable {
        public nonisolated enum Reason: Hashable, Sendable {
            /// D8 step 4's backup failed (`SafetyBackupFailed`).
            case backupFailed
            case other
        }
        public let deckID: Int64
        public let name: String
        public let reason: Reason
    }

    public private(set) var confirmation: Confirmation?
    public private(set) var failure: Failure?
    /// Reading the summary or deleting: the deck menus and the Deck menu are disabled.
    public private(set) var busy = false
    private weak var owner: ReviewCollection?
    private var revision: UInt64 = 0
    private var task: Task<Void, Never>?
    // Tests replace the two calls; production uses U28's deck service.
    @ObservationIgnored var readSummary: @MainActor ([Int64]) async throws -> DeckDeletionSummary
    /// The decks and the confirmed losses (`decks/decks` D8 step 6): the last
    /// summary read before the delete.
    @ObservationIgnored var deleteDecks: @MainActor ([Int64], DeckDeletionSummary) async throws -> DeckDeletion

    init(owner: ReviewCollection) {
        self.owner = owner
        readSummary = { @MainActor in try await CoreDeckService().deckDeletionSummary(ids: $0) }
        deleteDecks = { @MainActor ids, confirmed in
            let job = try await CoreDeckService().startDeleteDecks(ids: ids, confirmed: confirmed)
            // Planning and a queued write observe cancellation; an admitted write completes.
            return try await withTaskCancellationHandler {
                try await job.value()
            } onCancel: {
                Task { try? await job.job.cancel() }
            }
        }
    }

    /// The deck menu's and Deck › Delete Deck's enablement (N4, N20.4): a deck
    /// that exists, filtered included, outside review, with no sheet up and no
    /// other deck write, question or alert pending.
    public func canDelete(deckID: Int64, host: AppNavigation.SheetHost = .main) -> Bool {
        guard let owner, !busy, confirmation == nil, failure == nil,
              owner.deckWriteAllowed(host: host) else { return false }
        return owner.fullDeckName(deckID) != nil
    }

    /// Delete Deck: reads D8.5's summary; with no card to lose it deletes at
    /// once, otherwise it sets `confirmation`.
    public func delete(deckID: Int64, host: AppNavigation.SheetHost = .main, anchor: Anchor = .window) {
        guard canDelete(deckID: deckID, host: host), let owner else { return }
        let name = Self.lastComponent(owner: owner, deckID: deckID)
        start(deckID: deckID, name: name, anchor: anchor, shown: nil, owner: owner)
    }

    /// The destructive button. The summary is read again first; when it no
    /// longer matches the question, the new one is asked instead (N20.7).
    public func confirm() {
        guard let pending = confirmation, let owner else { return }
        confirmation = nil
        guard !busy, pending.generation == owner.generation, owner.deckWriteAllowed(host: .main) else {
            // A confirmed delete that cannot start (a sync hold, another deck
            // write) says so instead of vanishing: nothing was deleted.
            failure = Failure(deckID: pending.deckID, name: pending.name, reason: .other)
            return
        }
        start(deckID: pending.deckID, name: pending.name, anchor: pending.anchor, shown: pending.summary, owner: owner)
    }

    /// Cancel, Esc, or a tap outside the popover: nothing changes.
    public func cancel() { confirmation = nil }

    public func dismissFailure() { failure = nil }

    public func waitForCompletion() async { await task?.value }

    private func start(deckID: Int64, name: String, anchor: Anchor, shown: DeckDeletionSummary?, owner: ReviewCollection) {
        // Reserve synchronously, before the task can yield to another request.
        revision &+= 1; busy = true; failure = nil
        let current = revision, expected = owner.generation
        task = Task {
            defer { busy = false; task = nil }
            let isCurrent = { self.revision == current && owner.generation == expected }
            // The deck and its descendants before any write: a refresh after the
            // commit can drop them from the tree, and a lost result needs them.
            let processed = owner.deckSubtree(deckID)
            do {
                var summary = try await readSummary([deckID])
                // A deck that is already gone (a sync removed it) writes nothing.
                guard isCurrent(), summary.decks > 0 else { return }
                var shown = shown
                while true {
                    let subdecks = summary.decks - 1
                    if summary != shown, let message = Message(subdecks: subdecks, cards: summary.cards) {
                        confirmation = Confirmation(deckID: deckID, anchor: anchor, name: name, subdecks: subdecks,
                            cards: summary.cards, message: message, summary: summary, generation: expected)
                        return
                    }
                    // Backgrounding before the write starts stops it; the alert says nothing was deleted.
                    try Task.checkCancellation()
                    let deleted: DeckDeletion
                    do {
                        // D8 step 6: this read is what the delete may remove.
                        deleted = try await deleteDecks([deckID], summary)
                    } catch BridgeError.deckRefused(.lossesChanged(let decks, let cards)) {
                        // Something arrived after that read; nothing was written. The
                        // fresh counts are a new read: ask with them, or delete at once
                        // when no card would be lost (N20.7). Each repeat needs another write.
                        guard isCurrent() else { return }
                        summary = DeckDeletionSummary(decks: decks, cards: cards)
                        shown = nil
                        continue
                    }
                    guard isCurrent(), deleted.summary.generation == expected else { return }
                    let removed = deleted.summary.changes.decks.filter { $0.change == .removed }.map(\.id)
                    await finish(processed: processed.union(removed), owner: owner, generation: expected)
                    return
                }
            } catch BridgeError.commitCompleted {
                // The write committed; only its result was lost.
                guard isCurrent() else { return }
                await finish(processed: processed, owner: owner, generation: expected)
            } catch {
                guard isCurrent() else { return }
                failure = Failure(deckID: deckID, name: name,
                    reason: Self.reason((error as? BridgeError) ?? .internal("Deck deletion failed")))
            }
        }
    }

    /// N20.7, N14.1: a shown deck page among the processed decks (deleted, or
    /// the default deck emptied) returns the workspace to its root.
    private func finish(processed: Set<Int64>, owner: ReviewCollection, generation: UInt64) async {
        owner.navigation.deckPagesRemoved(Set(processed.map { DeckID(rawValue: $0) }))
        _ = try? await owner.refreshDeckNames(generation: generation)
        // Edit › Undo Delete Deck (R173) reads the new step.
        owner.deckWriteFinished()
    }

    /// Background and close: a queued delete stops; an admitted one completes.
    func cancelAndDrain() async {
        task?.cancel()
        await task?.value
    }

    /// The collection was replaced or closed: drop the question and any result.
    func invalidate() {
        revision &+= 1
        task?.cancel()
        confirmation = nil; failure = nil
    }

    static func reason(_ error: BridgeError) -> Failure.Reason {
        switch error {
        case .maintenance(.safetyBackupFailed): .backupFailed
        // The core reports the deletion's backup failure as an internal error
        // with this engine-defined prefix (`EngineError::SafetyBackupFailed`).
        case .internal(let detail) where detail.hasPrefix("safety backup failed"): .backupFailed
        default: .other
        }
    }

    private static func lastComponent(owner: ReviewCollection, deckID: Int64) -> String {
        if let node = owner.deck(deckID) { return node.name }
        let path = owner.fullDeckName(deckID) ?? ""
        return path.components(separatedBy: "::").last ?? path
    }
}
