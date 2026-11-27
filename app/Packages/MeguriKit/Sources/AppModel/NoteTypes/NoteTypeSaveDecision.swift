import CoreBridge
import Foundation

/// The sync service as `notetypes/draft-save` D3 reads it at Save.
public nonisolated enum NoteTypeSaveService: Sendable, Hashable {
    /// Meguri Sync with a ready binding (MP5).
    case meguriSync
    /// Off, or Meguri Sync chosen but not set up (D3).
    case other
}

/// What Save does for one summary and service (D3's table).
public nonisolated struct NoteTypeSaveDecision: Sendable, Hashable {
    public enum Confirm: Sendable, Hashable { case save, saveAndUpload }
    public enum Route: Sendable, Hashable {
        /// D5: back up when schema-classified, save on this device.
        case local
        /// D6: Meguri Sync's structure save job (sync first, save, upload; U269).
        case meguriSync
    }

    /// The sheet shows before anything is submitted.
    public let asks: Bool
    /// D4's paragraph about replacing the collection on Meguri Sync.
    public let syncParagraph: Bool
    public let confirm: Confirm
    /// The confirm button is destructive (content is lost) and never the default.
    public let destructive: Bool
    public let route: Route

    /// D3. A save that loses content always asks, whatever its reasons say.
    public static func decide(_ summary: NotetypeDraftSummary, service: NoteTypeSaveService) -> NoteTypeSaveDecision {
        let structural = !summary.reasons.isEmpty || summary.losesContent
        switch (service, structural) {
        case (_, false):
            return .init(asks: false, syncParagraph: false, confirm: .save, destructive: false, route: .local)
        case (.meguriSync, true):
            return .init(asks: true, syncParagraph: true, confirm: .saveAndUpload, destructive: summary.losesContent, route: .meguriSync)
        case (.other, true):
            return summary.losesContent
                ? .init(asks: true, syncParagraph: false, confirm: .save, destructive: true, route: .local)
                : .init(asks: false, syncParagraph: false, confirm: .save, destructive: false, route: .local)
        }
    }

    /// D5 step 4: whether the summary read right before submitting shows the
    /// sheet again (with its counts): D3 applied again asks, and a loss count
    /// grew, a loss the person did not confirm appeared, or the service
    /// changed what D3 shows. When D3 no longer asks, nothing is shown.
    public static func asksAgain(confirmed: NotetypeConfirmedLosses, shown: NoteTypeSaveDecision,
                                 latest: NotetypeDraftSummary, service: NoteTypeSaveService) -> Bool {
        let again = decide(latest, service: service)
        let shapeChanged = again.asks != shown.asks || again.syncParagraph != shown.syncParagraph || again.route != shown.route
        return again.asks && (shapeChanged || exceeds(latest, confirmed))
    }

    /// D8: each live loss compared with its own confirmed count; a category
    /// absent from the confirmed losses was confirmed as 0. A count that
    /// shrinks or stays equal does not ask again.
    public static func exceeds(_ summary: NotetypeDraftSummary, _ confirmed: NotetypeConfirmedLosses) -> Bool {
        summary.changes.contains { change in
            switch change {
            case .fieldRemoved(let ord, _, let notes): notes > (confirmed.fields[ord] ?? 0)
            case .templateRemoved(let ord, _, let cards, _): cards > (confirmed.templates[ord] ?? 0)
            default: false
            }
        }
    }
}

/// One row of the Save sheet (D4), in D4's order: the losses first.
public nonisolated struct NoteTypeSaveRow: Sendable, Hashable {
    public enum Kind: Sendable, Hashable {
        /// "Delete Card Type “<name>”": its cards (0: "No cards.") and the notes left with none.
        case deleteCardType(name: String, cards: UInt64, notesWithoutCards: UInt64)
        /// "Delete Field “<name>”": its notes with content (0: "Empty in every note.").
        case deleteField(name: String, notes: UInt64)
        case addCardType(name: String, cards: UInt64)
        case addField(name: String)
        case reorderFields
        case reorderCardTypes
        case sortBy(name: String)
    }
    public let kind: Kind
    public init(kind: Kind) { self.kind = kind }
    /// Shown in red: something is lost.
    public var loses: Bool {
        switch kind {
        case .deleteCardType(_, let cards, _): cards > 0
        case .deleteField(_, let notes): notes > 0
        default: false
        }
    }

    /// D4's rows for a summary: losses first, then the rest in D2's order;
    /// renames, text and styling are not listed.
    public static func rows(_ summary: NotetypeDraftSummary) -> [NoteTypeSaveRow] {
        let rows: [NoteTypeSaveRow] = summary.changes.compactMap { change in
            switch change {
            case .fieldAdded(let name): .init(kind: .addField(name: name))
            case .fieldRemoved(_, let name, let notes): .init(kind: .deleteField(name: name, notes: notes))
            case .fieldsReordered: .init(kind: .reorderFields)
            case .sortFieldChanged(let name): .init(kind: .sortBy(name: name))
            case .templateAdded(let name, let cards): .init(kind: .addCardType(name: name, cards: cards))
            case .templateRemoved(_, let name, let cards, let notes): .init(kind: .deleteCardType(name: name, cards: cards, notesWithoutCards: notes))
            case .templatesReordered: .init(kind: .reorderCardTypes)
            case .fieldRenamed, .templateRenamed, .notetypeRenamed, .textOrStylingChanged: nil
            }
        }
        return rows.filter(\.loses) + rows.filter { !$0.loses }
    }
}
