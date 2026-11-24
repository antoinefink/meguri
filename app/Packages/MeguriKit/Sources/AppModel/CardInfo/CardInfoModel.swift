import CoreBridge
import Foundation
import Observation

/// Reads a card's Card Info snapshot (`stats/card-info` C1): pure reads on
/// the query lane.
@MainActor public protocol CardInfoReading: AnyObject {
    func cardInfo(_ cardID: Int64) async throws -> CardInfoResult
    func debugJSON(_ cardID: Int64) async throws -> String?
}

@MainActor public final class CoreCardInfoReader: CardInfoReading {
    public init() {}
    public func cardInfo(_ cardID: Int64) async throws -> CardInfoResult { try await Core.cardInfo(cardID) }
    public func debugJSON(_ cardID: Int64) async throws -> String? { try await Core.cardInfoDebugJSON(cardID) }
}

/// One card's Card Info (U274; `browser/workspace` V13, `docs/design/
/// screen-inventory.yaml` `cardInfo.card`): the stats rows, the review
/// history and the forgetting curve of the card a host names.
///
/// - **No flash.** A new card's snapshot is read while the one shown stays;
///   it replaces it once read. A card named meanwhile drops the older read.
/// - **Live** (C10, R1260): a change set that names what the snapshot read
///   (its card, note, note type, decks, preset, a review-log row) reads it
///   again in place, and so does a new day for a day-number due (C10.2).
///   The core's watch (`card_info_watch`, U103) is not built: until it is,
///   any added review-log row reads the shown card again, since a change set
///   does not say whose row it is.
@Observable @MainActor
public final class CardInfoModel {
    public nonisolated enum Content: Equatable, Sendable {
        /// No card named (no row, several rows): the host says why.
        case none
        /// The first read of a card is under way and nothing is shown yet.
        case loading
        case snapshot(CardInfoSnapshot)
        case unavailable(CardInfoUnavailable)
        /// The read failed for another reason.
        case failed
    }

    public private(set) var content: Content = .none
    /// The card named last.
    public private(set) var cardID: Int64?
    /// The forgetting curve's range the person chose for this card (C7.3:
    /// otherwise the curve's default); kept while the card refreshes.
    public var range: CardInfoCurveRangeKind?

    /// The curve's range shown: the chosen one when offered, else the default.
    public var shownRange: CardInfoCurveRange? {
        guard case .snapshot(let snapshot) = content, let curve = snapshot.curve else { return nil }
        let kind = range.flatMap { kind in curve.ranges.contains { $0.kind == kind } ? kind : nil } ?? curve.defaultRange
        return curve.ranges.first { $0.kind == kind } ?? curve.ranges.last
    }

    @ObservationIgnored private let reader: any CardInfoReading
    @ObservationIgnored private var serial: UInt64 = 0
    @ObservationIgnored private var task: Task<Void, Never>?

    public init(reader: any CardInfoReading = CoreCardInfoReader()) {
        self.reader = reader
    }

    /// The host names a card, or none.
    public func show(card: Int64?) {
        guard card != cardID || content == .failed else { return }
        if card != cardID { range = nil }
        cardID = card
        guard let card else {
            serial &+= 1
            task?.cancel()
            content = .none
            return
        }
        read(card)
    }

    /// Reads the card named again, in place.
    public func refresh() {
        guard let cardID else { return }
        read(cardID)
    }

    private func read(_ card: Int64) {
        serial &+= 1
        let serial = serial, reader = reader
        // A first card shows nothing until it is read; another card's snapshot stays meanwhile.
        if case .snapshot = content {} else if case .unavailable = content {} else { content = .loading }
        task = Task {
            let next: Content
            do {
                switch try await reader.cardInfo(card) {
                case .found(let snapshot): next = .snapshot(snapshot)
                case .unavailable(let kind): next = .unavailable(kind)
                }
            } catch {
                next = .failed
            }
            guard serial == self.serial, !Task.isCancelled else { return }
            if next != content { content = next }
        }
    }

    /// The read under way, if any (tests).
    func settled() async { await task?.value }

    /// C10: a committed change set; the shown card is read again when it
    /// names something the snapshot read.
    public func changesCommitted(_ changes: ChangeSet) {
        guard cardID != nil, affects(changes) else { return }
        refresh()
    }

    /// Whether a change set can change the snapshot shown.
    func affects(_ changes: ChangeSet) -> Bool {
        if changes.wholeCollection || changes.collectionRow { return true }
        guard case .snapshot(let snapshot) = content else {
            // An unavailable card may become available (Check Database, an undo).
            return content != .none
        }
        let deps = snapshot.dependencies
        if changes.cards.contains(where: { $0.id == deps.card }) || changes.notes.contains(where: { $0.id == deps.note }) {
            return true
        }
        if changes.notetypes.contains(where: { $0.id == deps.notetype }) { return true }
        if changes.decks.contains(where: { $0.id == deps.deck || $0.id == deps.homeDeck }) { return true }
        if changes.deckConfigs.contains(where: { $0.id == deps.preset }) { return true }
        if !changes.revlogAdded.isEmpty { return true }
        if changes.revlogRemoved.contains(where: deps.revlog.contains) { return true }
        return changes.configKeys.contains { $0.name == "rollover" || $0.name == "creationOffset" }
    }

    /// A new scheduler day, a zone change or local midnight (C10.1, C10.2).
    public func dayChanged() { refresh() }

    /// The collection went: nothing of it stays.
    public func reset() {
        serial &+= 1
        task?.cancel()
        cardID = nil
        range = nil
        content = .none
    }

    /// C8: Copy Debug Info's JSON for the card shown.
    public func debugJSON() async -> String? {
        guard let cardID else { return nil }
        return try? await reader.debugJSON(cardID)
    }
}
