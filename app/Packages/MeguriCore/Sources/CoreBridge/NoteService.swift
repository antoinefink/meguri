import Foundation
import Instrumentation
internal import MeguriCoreBindings

public struct AddNoteRequest: Sendable, Hashable {
    public let notetypeId: Int64
    public let fields: [String]
    public let tags: [String]
    public let deckId: Int64
    public let guid: String?
    public let updateAddDefaults: Bool
    public init(notetypeId: Int64, fields: [String], tags: [String], deckId: Int64, guid: String?, updateAddDefaults: Bool) {
        self.notetypeId = notetypeId
        self.fields = fields
        self.tags = tags
        self.deckId = deckId
        self.guid = guid
        self.updateAddDefaults = updateAddDefaults
    }
    init(_ value: MeguriCoreBindings.AddNoteRequest) {
        notetypeId = value.notetypeId
        fields = value.fields
        tags = value.tags
        deckId = value.deckId
        guid = value.guid
        updateAddDefaults = value.updateAddDefaults
    }
    var raw: MeguriCoreBindings.AddNoteRequest { .init(notetypeId: notetypeId, fields: fields, tags: tags, deckId: deckId, guid: guid, updateAddDefaults: updateAddDefaults) }
}

public struct UpdateNoteRequest: Sendable, Hashable {
    public let id: Int64
    public let fields: [String]
    public let tags: [String]
    public init(id: Int64, fields: [String], tags: [String]) {
        self.id = id
        self.fields = fields
        self.tags = tags
    }
    init(_ value: MeguriCoreBindings.UpdateNoteRequest) {
        id = value.id
        fields = value.fields
        tags = value.tags
    }
    var raw: MeguriCoreBindings.UpdateNoteRequest { .init(id: id, fields: fields, tags: tags) }
}

public struct NoteMutation: Sendable, Hashable {
    public let summary: MutationSummary
    public let id: Int64
    public let cards: [Int64]
    public let written: Bool
    public init(summary: MutationSummary, id: Int64, cards: [Int64], written: Bool) {
        self.summary = summary
        self.id = id
        self.cards = cards
        self.written = written
    }
    init(_ value: MeguriCoreBindings.NoteMutation) {
        summary = MutationSummary(value.summary)
        id = value.id
        cards = value.cards
        written = value.written
    }
}

public struct NoteRecord: Sendable, Hashable {
    public let id: Int64
    public let notetypeId: Int64
    public let fields: [String]
    public let tags: [String]
    public let guid: String
    public init(id: Int64, notetypeId: Int64, fields: [String], tags: [String], guid: String) {
        self.id = id
        self.notetypeId = notetypeId
        self.fields = fields
        self.tags = tags
        self.guid = guid
    }
    init(_ value: MeguriCoreBindings.NoteRecord) {
        id = value.id
        notetypeId = value.notetypeId
        fields = value.fields
        tags = value.tags
        guid = value.guid
    }
}

public struct FieldCheck: Sendable, Hashable {
    public let state: FieldState
    public let duplicates: [Int64]
    public init(state: FieldState, duplicates: [Int64]) {
        self.state = state
        self.duplicates = duplicates
    }
    init(_ value: MeguriCoreBindings.FieldCheck) {
        state = FieldState(value.state)
        duplicates = value.duplicates
    }
}

public struct NoteDeletion: Sendable, Hashable {
    public let summary: MutationSummary?
    public let count: UInt64
    public init(summary: MutationSummary?, count: UInt64) {
        self.summary = summary
        self.count = count
    }
    init(_ value: MeguriCoreBindings.NoteDeletion) {
        summary = value.summary.map(MutationSummary.init)
        count = value.count
    }
}

public enum FieldState: Sendable, Hashable {
    case normal
    case empty
    case duplicate
    case missingCloze
    case notetypeNotCloze
    case fieldNotCloze
    init(_ value: MeguriCoreBindings.FieldState) {
        switch value {
        case .normal: self = .normal
        case .empty: self = .empty
        case .duplicate: self = .duplicate
        case .missingCloze: self = .missingCloze
        case .notetypeNotCloze: self = .notetypeNotCloze
        case .fieldNotCloze: self = .fieldNotCloze
        }
    }
}

/// Cancellation is observed before engine orchestration starts. The current
/// U25/deletion APIs do not accept a token after entry; that contract remains
/// pending and callers must await completion before acting on cancellation.
public final class CoreNoteDeletionJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.NoteDeletionJob
    init(job: CoreJob, raw: MeguriCoreBindings.NoteDeletionJob) { self.job = job; self.raw = raw }
    public func value() async throws(BridgeError) -> NoteDeletion {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.noteDeletionJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return NoteDeletion(result)
        }
    }
}

public protocol NoteService: Sendable {
    func startAddNotes(_ requests: [AddNoteRequest]) async throws(BridgeError) -> CoreNoteAdditionJob
    func addNote(_ request: AddNoteRequest) async throws(BridgeError) -> NoteMutation
    func updateNote(_ request: UpdateNoteRequest) async throws(BridgeError) -> NoteMutation
    func readNote(id: Int64) async throws(BridgeError) -> NoteRecord
    func checkNoteFields(notetypeId: Int64, fields: [String], ownNote: Int64?) async throws(BridgeError) -> FieldCheck
    func startDeleteNotes(ids: [Int64]) async throws(BridgeError) -> CoreNoteDeletionJob
}
public actor CoreNoteService: NoteService {
    public init() {}
    public func addNote(_ request: AddNoteRequest) async throws(BridgeError) -> NoteMutation {
        try await InteractiveLane.shared.call(.addNote) { NoteMutation(try MeguriCoreBindings.addNote(request: request.raw)) }
    }
    public func updateNote(_ request: UpdateNoteRequest) async throws(BridgeError) -> NoteMutation {
        try await InteractiveLane.shared.call(.updateNote) { NoteMutation(try MeguriCoreBindings.updateNote(request: request.raw)) }
    }
    public func readNote(id: Int64) async throws(BridgeError) -> NoteRecord {
        try await QueryLane.shared.call(.readNote) { NoteRecord(try MeguriCoreBindings.readNote(id: id)) }
    }
    public func checkNoteFields(notetypeId: Int64, fields: [String], ownNote: Int64?) async throws(BridgeError) -> FieldCheck {
        try await InteractiveLane.shared.call(.checkNoteFields) { FieldCheck(try MeguriCoreBindings.checkNoteFields(notetypeId: notetypeId, fields: fields, ownNote: ownNote)) }
    }
    public func startDeleteNotes(ids: [Int64]) async throws(BridgeError) -> CoreNoteDeletionJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startDeleteNotes) {
            let raw = try MeguriCoreBindings.startDeleteNotes(ids: ids)
            return CoreNoteDeletionJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
}
public struct AddedNote: Sendable, Hashable {
    public let id: Int64
    public let cards: [Int64]
    public init(id: Int64, cards: [Int64]) {
        self.id = id
        self.cards = cards
    }
    init(_ value: MeguriCoreBindings.AddedNote) {
        id = value.id
        cards = value.cards
    }
}

public struct BulkNoteAddition: Sendable, Hashable {
    public let summary: MutationSummary
    public let notes: [AddedNote]
    public init(summary: MutationSummary, notes: [AddedNote]) {
        self.summary = summary
        self.notes = notes
    }
    init(_ value: MeguriCoreBindings.BulkNoteAddition) {
        summary = MutationSummary(value.summary)
        notes = value.notes.map(AddedNote.init)
    }
}

public final class CoreNoteAdditionJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.NoteAdditionJob
    init(job: CoreJob, raw: MeguriCoreBindings.NoteAdditionJob) { self.job = job; self.raw = raw }
    public func value() async throws(BridgeError) -> BulkNoteAddition {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.noteAdditionJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return BulkNoteAddition(result)
        }
    }
}
extension CoreNoteService {
    public func startAddNotes(_ requests: [AddNoteRequest]) async throws(BridgeError) -> CoreNoteAdditionJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startAddNotes) {
            let raw = try MeguriCoreBindings.startAddNotes(requests: requests.map(\.raw))
            return CoreNoteAdditionJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
}
