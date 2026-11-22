import Foundation
import Instrumentation
internal import MeguriCoreBindings

// The Note Types section's save and its Fields tab's two pure reads (U281):
// the draft apply op (`notetypes/structure-changes` S1, with the confirmed
// losses of `notetypes/draft-save` D5 step 5), V5's check of a typed field
// name (`notetypes/validation` V5) and the field names template texts refer
// to (`render/template-syntax` T11.1), from which Used On is shown.

/// What the draft apply op did (S1.3).
public struct NotetypeStructureSaved: Sendable, Hashable {
    public let summary: MutationSummary
    public let id: Int64
    public let name: String
    /// False when the draft held no change.
    public let written: Bool
    public let notesRewritten: UInt64
    public let cardsAdded: UInt64
    public let cardsRemoved: UInt64
    public let cardsRenumbered: UInt64
    public let notesWithoutCards: UInt64
    public let reasons: [SchemaChangeReason]
    public init(summary: MutationSummary, id: Int64, name: String, written: Bool, notesRewritten: UInt64 = 0, cardsAdded: UInt64 = 0,
                cardsRemoved: UInt64 = 0, cardsRenumbered: UInt64 = 0, notesWithoutCards: UInt64 = 0, reasons: [SchemaChangeReason] = []) {
        self.summary = summary
        self.id = id
        self.name = name
        self.written = written
        self.notesRewritten = notesRewritten
        self.cardsAdded = cardsAdded
        self.cardsRemoved = cardsRemoved
        self.cardsRenumbered = cardsRenumbered
        self.notesWithoutCards = notesWithoutCards
        self.reasons = reasons
    }
    init(_ raw: MeguriCoreBindings.NotetypeStructureSaved) {
        self.init(summary: MutationSummary(raw.summary), id: raw.id, name: raw.name, written: raw.written,
                  notesRewritten: raw.notesRewritten, cardsAdded: raw.cardsAdded, cardsRemoved: raw.cardsRemoved,
                  cardsRenumbered: raw.cardsRenumbered, notesWithoutCards: raw.notesWithoutCards,
                  reasons: raw.reasons.map(SchemaChangeReason.init))
    }
}

/// Why V5 refuses a typed field name.
public enum NotetypeFieldNameRefusal: Sendable, Hashable {
    /// It starts with `#`, `^` or `/`.
    case badFirstCharacter
    /// It holds `:`, `{` or `}`.
    case badCharacter
    /// Another field has a name equal under unicase.
    case duplicate
}

/// V5's answer for a typed field name.
public enum NotetypeFieldNameCheck: Sendable, Hashable {
    /// Empty once `"` is removed and the ends trimmed: nothing changes.
    case ignored
    /// The name to use.
    case accepted(String)
    case refused(NotetypeFieldNameRefusal)
    init(_ raw: MeguriCoreBindings.NotetypeFieldNameCheck) {
        switch raw {
        case .ignored: self = .ignored
        case .accepted(let name): self = .accepted(name)
        case .refused(let refusal):
            switch refusal {
            case .badFirstCharacter: self = .refused(.badFirstCharacter)
            case .badCharacter: self = .refused(.badCharacter)
            case .duplicate: self = .refused(.duplicate)
            }
        }
    }
}

extension CoreNotetypeService {
    /// S1's draft apply op as a job: planning reports progress and can be
    /// cancelled; a refusal (`Stale`, `LossesChanged` with the new summary,
    /// a V3 failure) comes back as the job's error and writes nothing.
    public func startNotetypeStructureSave(_ draft: NotetypeStructureDraft) async throws(BridgeError) -> CoreNotetypeReadJob<NotetypeStructureSaved> {
        try await CoreEventStream.shared.start()
        let request = draft.raw
        return try await MaintenanceLane.shared.call(.startNotetypeStructureSave) {
            let raw = try MeguriCoreBindings.startNotetypeStructureSave(draft: request)
            let result = raw.result
            return CoreNotetypeReadJob(job: try Jobs.track(raw.handle), resultCall: .notetypeStructureJobResult) {
                try result.result().map(NotetypeStructureSaved.init)
            }
        }
    }

    /// V5 for `typed` against the draft's field names in order; `renaming`
    /// is the position of the field being renamed, nil for a new one.
    public func notetypeCheckFieldName(_ typed: String, fields: [String], renaming: UInt32?) async throws(BridgeError) -> NotetypeFieldNameCheck {
        try await QueryLane.shared.call(.notetypeCheckFieldName) {
            NotetypeFieldNameCheck(try MeguriCoreBindings.notetypeCheckFieldName(typed: typed, fields: fields, renaming: renaming))
        }
    }

    /// T11.1 for each text: the field names it refers to, each once, in
    /// order of first appearance (special names and unknown names included).
    public func templateFieldReferences(_ texts: [String]) async throws(BridgeError) -> [[String]] {
        try await QueryLane.shared.call(.templateFieldReferences) {
            try MeguriCoreBindings.templateFieldReferences(texts: texts)
        }
    }
}
