import CoreBridge
import Foundation

/// The core reads and the save the Note Types section uses. Production is
/// `CoreNoteTypesBackend`; tests and the test hooks' routes pass their own.
/// Every call but the save reads one snapshot and writes nothing.
@MainActor public protocol NoteTypesBackend: AnyObject {
    /// `notetypes/editor-actions` A2's order, at once.
    func list() async throws -> [NotetypeName]
    /// A2's note counts by note type id.
    func noteCounts() async throws -> [Int64: UInt64]
    func read(_ id: Int64) async throws -> NotetypeRecord
    /// The note type's notes and cards (the detail's subtitle).
    func typeCounts(_ id: Int64) async throws -> NotetypeRemovalCounts
    /// W3.1, per stored field ordinal.
    func fieldContentCounts(_ id: Int64) async throws -> NotetypeFieldContentCounts
    /// `notetypes/draft-save` D2.
    func summary(_ draft: NotetypeStructureDraft) async throws -> NotetypeDraftSummary
    /// S1's op with the confirmed losses; `progress` gets (completed, total).
    func save(_ draft: NotetypeStructureDraft, progress: @escaping @MainActor (UInt64, UInt64?) -> Void) async throws
    /// `notetypes/validation` V5.
    func checkFieldName(_ typed: String, fields: [String], renaming: UInt32?) async throws -> NotetypeFieldNameCheck
    /// `render/template-syntax` T11.1 per text.
    func fieldReferences(_ texts: [String]) async throws -> [[String]]
    /// W5: per stored template ordinal, its cards.
    func templateCardCounts(_ id: Int64) async throws -> [UInt64]
    /// W7's red issues of the draft as it would be saved.
    func templateIssues(_ draft: NotetypeStructureDraft) async throws -> [NotetypeTemplateIssue]
    /// W8 per card type of the draft (none for a cloze note type).
    func notesWithoutCard(_ draft: NotetypeStructureDraft) async throws -> [NotetypeBlankFront]
    /// W7's "Use “<to>”" on one part's text.
    func replaceFieldReferences(_ text: String, from: String, to: String) async throws -> String
    /// W6's colouring of a Front or Back text, or of the styling.
    func syntax(_ text: String, styling: Bool) async throws -> [NotetypeSyntaxSpan]
    /// W10's Card Text row of a styling (pure: no collection is read).
    func cardText(css: String, rightToLeft: Bool) async throws -> CardTextRow?
    /// W10: one control's change on a styling (pure).
    func editCardText(css: String, change: CardTextChange, rightToLeft: Bool) async throws -> CardTextEdited
}

/// The section's backend over CoreBridge. A job whose task is cancelled
/// (the person moved on) is cancelled in the core too.
@MainActor public final class CoreNoteTypesBackend: NoteTypesBackend {
    private let service = CoreNotetypeService()
    public init() {}

    public func list() async throws -> [NotetypeName] { try await service.notetypeList() }
    public func noteCounts() async throws -> [Int64: UInt64] { try await run { try await self.service.startNotetypeNoteCounts() } }
    public func read(_ id: Int64) async throws -> NotetypeRecord { try await service.readNotetype(id: id) }
    public func typeCounts(_ id: Int64) async throws -> NotetypeRemovalCounts {
        try await run { try await self.service.startNotetypeRemovalCounts(notetypeId: id) }
    }
    public func fieldContentCounts(_ id: Int64) async throws -> NotetypeFieldContentCounts {
        try await run { try await self.service.startNotetypeFieldContentCounts(notetypeId: id) }
    }
    public func summary(_ draft: NotetypeStructureDraft) async throws -> NotetypeDraftSummary {
        try await run { try await self.service.startNotetypeDraftSummary(draft) }
    }
    public func save(_ draft: NotetypeStructureDraft, progress: @escaping @MainActor (UInt64, UInt64?) -> Void) async throws {
        let job = try await service.startNotetypeStructureSave(draft)
        let reports = Task { @MainActor in
            for await report in job.job.progress { progress(report.completed, report.total) }
        }
        defer { reports.cancel() }
        // A save is never cancelled by leaving the section: it runs to its end (D5 step 5).
        _ = try await job.value()
    }
    public func checkFieldName(_ typed: String, fields: [String], renaming: UInt32?) async throws -> NotetypeFieldNameCheck {
        try await service.notetypeCheckFieldName(typed, fields: fields, renaming: renaming)
    }
    public func fieldReferences(_ texts: [String]) async throws -> [[String]] {
        try await service.templateFieldReferences(texts)
    }
    public func templateCardCounts(_ id: Int64) async throws -> [UInt64] {
        try await run { try await self.service.startNotetypeTemplateCardCounts(notetypeId: id) }
    }
    public func templateIssues(_ draft: NotetypeStructureDraft) async throws -> [NotetypeTemplateIssue] {
        try await service.notetypeTemplateIssues(draft)
    }
    public func notesWithoutCard(_ draft: NotetypeStructureDraft) async throws -> [NotetypeBlankFront] {
        try await run { try await self.service.startNotetypeNotesWithoutCard(draft) }
    }
    public func replaceFieldReferences(_ text: String, from: String, to: String) async throws -> String {
        try await service.notetypeReplaceFieldReferences(text, from: from, to: to)
    }
    public func syntax(_ text: String, styling: Bool) async throws -> [NotetypeSyntaxSpan] {
        styling ? try await service.notetypeStylingSyntax(text) : try await service.notetypeTemplateSyntax(text)
    }

    public func cardText(css: String, rightToLeft: Bool) async throws -> CardTextRow? {
        try await service.cardText(css: css, rightToLeft: rightToLeft)
    }
    public func editCardText(css: String, change: CardTextChange, rightToLeft: Bool) async throws -> CardTextEdited {
        try await service.editCardText(css: css, change: change, rightToLeft: rightToLeft)
    }

    private func run<Value: Sendable>(_ start: () async throws -> CoreNotetypeReadJob<Value>) async throws -> Value {
        let job = try await start()
        return try await withTaskCancellationHandler {
            try await job.value()
        } onCancel: {
            Task { try? await job.job.cancel() }
        }
    }
}
