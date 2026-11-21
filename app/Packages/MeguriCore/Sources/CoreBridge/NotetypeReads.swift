import Foundation
import Instrumentation
internal import MeguriCoreBindings

// The Note Types screens' reads (U267): the list in
// `notetypes/editor-actions` A2's order and its note counts; the save
// summary of a draft (`notetypes/draft-save` D2); the notes with content of
// each field (`notetypes/workspace` W3.1); the notes a draft's fronts give
// no card (W8); the delete question's counts (W12); and W7.1's near name.
// Each read other than the list and the near name is a job on one
// snapshot: progress, cancel, no write.

/// One field of a note type draft (`notetypes/structure-changes` S1.1).
public struct StructureDraftField: Sendable, Hashable {
    /// The base ordinal the field comes from; nil for a new field.
    public var origin: UInt32?
    public var name: String
    public var sticky: Bool
    public var rtl: Bool
    public var fontName: String
    public var fontSize: UInt32
    public var description: String
    public var plainText: Bool
    public var collapsed: Bool
    public var excludeFromSearch: Bool
    public init(origin: UInt32?, name: String, sticky: Bool, rtl: Bool, fontName: String, fontSize: UInt32, description: String, plainText: Bool, collapsed: Bool, excludeFromSearch: Bool) {
        self.origin = origin
        self.name = name
        self.sticky = sticky
        self.rtl = rtl
        self.fontName = fontName
        self.fontSize = fontSize
        self.description = description
        self.plainText = plainText
        self.collapsed = collapsed
        self.excludeFromSearch = excludeFromSearch
    }
    /// A stored field kept as it is, its position as origin.
    public init(_ field: NotetypeField) {
        self.init(origin: field.ord, name: field.name, sticky: field.sticky, rtl: field.rtl, fontName: field.fontName,
                  fontSize: field.fontSize, description: field.description, plainText: field.plainText,
                  collapsed: field.collapsed, excludeFromSearch: field.excludeFromSearch)
    }
    var raw: MeguriCoreBindings.StructureDraftField {
        .init(origin: origin, name: name, sticky: sticky, rtl: rtl, fontName: fontName, fontSize: fontSize, description: description,
              plainText: plainText, collapsed: collapsed, excludeFromSearch: excludeFromSearch)
    }
}

/// One card type of a note type draft (S1.1).
public struct StructureDraftTemplate: Sendable, Hashable {
    /// The base ordinal the card type comes from; nil for a new one.
    public var origin: UInt32?
    public var name: String
    public var front: String
    public var back: String
    public var browserFront: String
    public var browserBack: String
    public var targetDeckId: Int64
    public var browserFontName: String
    public var browserFontSize: UInt32
    public init(origin: UInt32?, name: String, front: String, back: String, browserFront: String, browserBack: String, targetDeckId: Int64, browserFontName: String, browserFontSize: UInt32) {
        self.origin = origin
        self.name = name
        self.front = front
        self.back = back
        self.browserFront = browserFront
        self.browserBack = browserBack
        self.targetDeckId = targetDeckId
        self.browserFontName = browserFontName
        self.browserFontSize = browserFontSize
    }
    /// A stored card type kept as it is, its position as origin.
    public init(_ template: NotetypeTemplate) {
        self.init(origin: template.ord, name: template.name, front: template.front, back: template.back,
                  browserFront: template.qFormatBrowser, browserBack: template.aFormatBrowser, targetDeckId: template.targetDeckId,
                  browserFontName: template.browserFontName, browserFontSize: template.browserFontSize)
    }
    var raw: MeguriCoreBindings.StructureDraftTemplate {
        .init(origin: origin, name: name, front: front, back: back, browserFront: browserFront, browserBack: browserBack,
              targetDeckId: targetDeckId, browserFontName: browserFontName, browserFontSize: browserFontSize)
    }
}

/// The losses a save sheet showed (`notetypes/draft-save` D8): per removed
/// base field ordinal its notes with content, per removed base card type
/// ordinal its cards. A category left out was confirmed as 0.
public struct NotetypeConfirmedLosses: Sendable, Hashable {
    public var fields: [UInt32: UInt64]
    public var templates: [UInt32: UInt64]
    public var kindChangeCards: UInt64
    public init(fields: [UInt32: UInt64] = [:], templates: [UInt32: UInt64] = [:], kindChangeCards: UInt64 = 0) {
        self.fields = fields
        self.templates = templates
        self.kindChangeCards = kindChangeCards
    }
    /// What a save takes back after showing `summary` (D5 step 5).
    public init(confirming summary: NotetypeDraftSummary) {
        self.init()
        for change in summary.changes {
            switch change {
            case .fieldRemoved(let ord, _, let notes): fields[ord] = notes
            case .templateRemoved(let ord, _, let cards, _): templates[ord] = cards
            default: break
            }
        }
    }
    var raw: MeguriCoreBindings.NotetypeConfirmedLosses {
        .init(fields: fields.sorted { $0.key < $1.key }.map { .init(ord: $0.key, count: $0.value) },
              templates: templates.sorted { $0.key < $1.key }.map { .init(ord: $0.key, count: $0.value) },
              kindChangeCards: kindChangeCards)
    }
}

/// A whole draft of one note type (S1.1): what the save and the reads of
/// D2 and W8 take.
public struct NotetypeStructureDraft: Sendable, Hashable {
    public var notetypeId: Int64
    /// `NotetypeRecord.base` as read.
    public var base: [UInt8]
    public var name: String
    public var css: String
    public var fields: [StructureDraftField]
    public var templates: [StructureDraftTemplate]
    /// A position in `fields`; nil keeps the base's sort field.
    public var sortField: UInt32?
    public var skipChecks: Bool
    public var confirmed: NotetypeConfirmedLosses
    public init(notetypeId: Int64, base: [UInt8], name: String, css: String, fields: [StructureDraftField], templates: [StructureDraftTemplate], sortField: UInt32?, skipChecks: Bool, confirmed: NotetypeConfirmedLosses = .init()) {
        self.notetypeId = notetypeId
        self.base = base
        self.name = name
        self.css = css
        self.fields = fields
        self.templates = templates
        self.sortField = sortField
        self.skipChecks = skipChecks
        self.confirmed = confirmed
    }
    /// The unchanged draft of a note type as read. `skipChecks`: true for
    /// the field editor's saves, false for the template editor's (V1).
    public init(_ record: NotetypeRecord, skipChecks: Bool) {
        self.init(notetypeId: record.id, base: record.base, name: record.name, css: record.css,
                  fields: record.fields.map(StructureDraftField.init), templates: record.templates.map(StructureDraftTemplate.init),
                  sortField: nil, skipChecks: skipChecks)
    }
    var raw: MeguriCoreBindings.NotetypeStructureDraft {
        .init(notetypeId: notetypeId, base: Data(base), name: name, css: css, fields: fields.map(\.raw), templates: templates.map(\.raw),
              sortField: sortField, skipChecks: skipChecks, confirmed: confirmed.raw)
    }
}

/// W3.1: the type's notes, and per stored field ordinal the notes whose
/// value there is not empty (`<br>` alone is empty).
public struct NotetypeFieldContentCounts: Sendable, Hashable {
    public let notes: UInt64
    public let fields: [UInt64]
    public init(notes: UInt64, fields: [UInt64]) {
        self.notes = notes
        self.fields = fields
    }
}

/// W8 for one card type of the draft, in the draft's order.
public struct NotetypeBlankFront: Sendable, Hashable {
    /// Every note the front gives no card, ascending (Show Notes).
    public let noteIds: [Int64]
    /// The position in the draft's fields of the one field that decides it
    /// (W8's reason line), or nil.
    public let field: UInt32?
    public init(noteIds: [Int64], field: UInt32?) {
        self.noteIds = noteIds
        self.field = field
    }
}

/// W12: the note type's notes and cards, which the removal takes back as
/// its confirmed counts.
public struct NotetypeRemovalCounts: Sendable, Hashable {
    public let notes: UInt64
    public let cards: UInt64
    public init(notes: UInt64, cards: UInt64) {
        self.notes = notes
        self.cards = cards
    }
}

/// A started read job: its progress and cancel, and its value once it ends.
public final class CoreNotetypeReadJob<Value: Sendable>: Sendable {
    public let job: CoreJob
    private let resultCall: BridgeCall
    private let read: @Sendable () throws -> Value?
    init(job: CoreJob, resultCall: BridgeCall, read: @escaping @Sendable () throws -> Value?) {
        self.job = job
        self.resultCall = resultCall
        self.read = read
    }
    public func value() async throws(BridgeError) -> Value {
        try await job.value()
        let read = read
        return try await MaintenanceLane.shared.call(resultCall) {
            guard let value = try read() else { throw BridgeError.internal("Successful job has no result") }
            return value
        }
    }
}

extension CoreNotetypeService {
    /// A2's ordered list: every note type by name under `unicase`.
    public func notetypeList() async throws(BridgeError) -> [NotetypeName] {
        try await QueryLane.shared.call(.notetypeList) { try MeguriCoreBindings.notetypeList().map(NotetypeName.init) }
    }

    /// A2's note counts, by note type id (0 for a type without notes).
    public func startNotetypeNoteCounts() async throws(BridgeError) -> CoreNotetypeReadJob<[Int64: UInt64]> {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startNotetypeNoteCounts) {
            let raw = try MeguriCoreBindings.startNotetypeNoteCounts()
            let result = raw.result
            return CoreNotetypeReadJob(job: try Jobs.track(raw.handle), resultCall: .notetypeNoteCountsJobResult) {
                try result.result().map { Dictionary(uniqueKeysWithValues: $0.map { ($0.id, $0.notes) }) }
            }
        }
    }

    /// W3.1's `field_content_counts`.
    public func startNotetypeFieldContentCounts(notetypeId: Int64) async throws(BridgeError) -> CoreNotetypeReadJob<NotetypeFieldContentCounts> {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startNotetypeFieldContentCounts) {
            let raw = try MeguriCoreBindings.startNotetypeFieldContentCounts(notetypeId: notetypeId)
            let result = raw.result
            return CoreNotetypeReadJob(job: try Jobs.track(raw.handle), resultCall: .notetypeFieldContentCountsJobResult) {
                try result.result().map { NotetypeFieldContentCounts(notes: $0.notes, fields: $0.fields) }
            }
        }
    }

    /// D2's `draft_summary`: the save's own count of the draft's losses.
    public func startNotetypeDraftSummary(_ draft: NotetypeStructureDraft) async throws(BridgeError) -> CoreNotetypeReadJob<NotetypeDraftSummary> {
        try await CoreEventStream.shared.start()
        let request = draft.raw
        return try await MaintenanceLane.shared.call(.startNotetypeDraftSummary) {
            let raw = try MeguriCoreBindings.startNotetypeDraftSummary(draft: request)
            let result = raw.result
            return CoreNotetypeReadJob(job: try Jobs.track(raw.handle), resultCall: .notetypeDraftSummaryJobResult) {
                try result.result().map(NotetypeDraftSummary.init)
            }
        }
    }

    /// W8's `notes_without_card` for every card type of the draft (none for
    /// a cloze note type).
    public func startNotetypeNotesWithoutCard(_ draft: NotetypeStructureDraft) async throws(BridgeError) -> CoreNotetypeReadJob<[NotetypeBlankFront]> {
        try await CoreEventStream.shared.start()
        let request = draft.raw
        return try await MaintenanceLane.shared.call(.startNotetypeNotesWithoutCard) {
            let raw = try MeguriCoreBindings.startNotetypeNotesWithoutCard(draft: request)
            let result = raw.result
            return CoreNotetypeReadJob(job: try Jobs.track(raw.handle), resultCall: .notetypeNotesWithoutCardJobResult) {
                try result.result().map { $0.map { NotetypeBlankFront(noteIds: $0.noteIds, field: $0.field) } }
            }
        }
    }

    /// W12's counts for the delete question.
    public func startNotetypeRemovalCounts(notetypeId: Int64) async throws(BridgeError) -> CoreNotetypeReadJob<NotetypeRemovalCounts> {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startNotetypeRemovalCounts) {
            let raw = try MeguriCoreBindings.startNotetypeRemovalCounts(notetypeId: notetypeId)
            let result = raw.result
            return CoreNotetypeReadJob(job: try Jobs.track(raw.handle), resultCall: .notetypeRemovalCountsJobResult) {
                try result.result().map { NotetypeRemovalCounts(notes: $0.notes, cards: $0.cards) }
            }
        }
    }

    /// W7.1: the position in `fields` of the name to suggest for `unknown`.
    public func notetypeNearFieldName(_ unknown: String, fields: [String]) async throws(BridgeError) -> UInt32? {
        try await QueryLane.shared.call(.notetypeNearFieldName) {
            try MeguriCoreBindings.notetypeNearFieldName(unknown: unknown, fields: fields)
        }
    }
}
