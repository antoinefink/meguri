import Foundation
import Instrumentation
internal import MeguriCoreBindings

/// A card action of `browser/workspace` V8.1 (`browser/bulk-actions` B1:
/// on the selected cards, or every card of each selected note in Notes
/// mode).
public enum BulkCardAction: Sendable, Hashable {
    /// Change Deck to an existing deck (B7.2).
    case changeDeck(deck: Int64)
    /// Change Deck to a typed name: that deck, or a new one made in the same
    /// step (B7.2 step 0).
    case changeDeckToName(String)
    /// Flag 1 to 7, or 0 for No Flag; it sets, it does not toggle.
    case setFlag(UInt8)
    case suspend
    /// The suspended target cards only.
    case unsuspend
    case bury
    /// The buried target cards only.
    case unbury
    /// The text as typed (`setDueBrowser`).
    case setDueDate(String)
    /// Reset with its two choices.
    case forget(restorePosition: Bool, resetCounts: Bool)

    var raw: MeguriCoreBindings.BulkCardAction {
        switch self {
        case .changeDeck(let deck): .changeDeck(deck: deck)
        case .changeDeckToName(let name): .changeDeckToName(name: name)
        case .setFlag(let flag): .setFlag(flag: flag)
        case .suspend: .suspend
        case .unsuspend: .unsuspend
        case .bury: .bury
        case .unbury: .unbury
        case .setDueDate(let days): .setDueDate(days: days)
        case .forget(let restore, let counts): .forget(restorePosition: restore, resetCounts: counts)
        }
    }
}

/// A note action of V8.1 (B1: on the selected notes, or the notes of the
/// selected cards in Cards mode).
public enum BulkNoteAction: Sendable, Hashable {
    /// The tag entry's text (B9).
    case addTags(String)
    case removeTags(String)
    /// B11: the notes and every card of them.
    case deleteNotes

    var raw: MeguriCoreBindings.BulkNoteAction {
        switch self {
        case .addTags(let text): .addTags(text: text)
        case .removeTags(let text): .removeTags(text: text)
        case .deleteNotes: .deleteNotes
        }
    }
}

/// What an action did (`browser/bulk-actions` B16, `browser/bulk-framework`
/// K5): the rows written, the ids it was given after its selection was
/// resolved, Delete Notes' cards removed, and the commit.
public struct BulkActionResult: Sendable, Hashable {
    public let affected: UInt64
    public let considered: UInt64
    /// Delete Notes only (B11).
    public let cardsRemoved: UInt64?
    public let summary: MutationSummary
    public init(affected: UInt64, considered: UInt64, cardsRemoved: UInt64?, summary: MutationSummary) {
        self.affected = affected
        self.considered = considered
        self.cardsRemoved = cardsRemoved
        self.summary = summary
    }
}

/// A started bulk action: its progress and cancel (honoured until the
/// commit starts, K3), and its result once it ends. A refusal, the
/// selection's included (K1), is the job's failure.
public final class CoreBulkActionJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.BulkActionJob
    init(job: CoreJob, raw: MeguriCoreBindings.BulkActionJob) {
        self.job = job
        self.raw = raw
    }

    public func value() async throws(BridgeError) -> BulkActionResult {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.bulkActionJobResult) {
            guard let done = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return BulkActionResult(affected: done.affected, considered: done.considered, cardsRemoved: done.cardsRemoved,
                                    summary: MutationSummary(done.summary))
        }
    }
}

/// Browse's bulk actions (U87) as jobs admitted by the review service
/// (`browser/bulk-framework` K2), on a selection the job resolves in the
/// core (K1, `browser/result-sets` H12), so a large selection never crosses
/// the FFI as ids. A selection whose handle was released, or whose revision
/// changed since it was taken, is refused with `invalidArgument` and nothing
/// is written.
public enum CoreBulkActions {
    public static func start(_ action: BulkCardAction, on selection: BrowseSelection) async throws(BridgeError)
        -> CoreBulkActionJob {
        try await CoreEventStream.shared.start()
        let raw = selection.raw, rawAction = action.raw
        return try await MaintenanceLane.shared.call(.startBulkCardActionOnSelection) {
            let start = try MeguriCoreBindings.startBulkCardActionOnSelection(selection: raw, action: rawAction)
            return CoreBulkActionJob(job: try Jobs.track(start.handle), raw: start.result)
        }
    }

    public static func start(_ action: BulkNoteAction, on selection: BrowseSelection) async throws(BridgeError)
        -> CoreBulkActionJob {
        try await CoreEventStream.shared.start()
        let raw = selection.raw, rawAction = action.raw
        return try await MaintenanceLane.shared.call(.startBulkNoteActionOnSelection) {
            let start = try MeguriCoreBindings.startBulkNoteActionOnSelection(selection: raw, action: rawAction)
            return CoreBulkActionJob(job: try Jobs.track(start.handle), raw: start.result)
        }
    }
}
