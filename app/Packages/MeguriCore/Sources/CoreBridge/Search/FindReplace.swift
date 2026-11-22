import Foundation
internal import MeguriCoreBindings

// Find and Replace (`browser/find-replace` F5 to F13, `browser/workspace` V9
// and V17): the sheet's live preview, superseded by each keystroke, and the
// run it confirms, admitted by the review service (`browser/bulk-framework`
// K2, K7). The target is a Browse selection (Selected, or All Found: a
// selection of every row of the results), resolved to its notes in the core,
// or nil for All Notes. No ids cross.

/// Where the find text is looked for (F1's In).
public enum FindReplaceScope: Sendable, Hashable {
    case allFields
    case tags
    /// The field of this name in every target note whose note type has it,
    /// compared ignoring case (F6 step 2).
    case field(String)

    var raw: MeguriCoreBindings.FindReplaceIn {
        switch self {
        case .allFields: .allFields
        case .tags: .tags
        case .field(let name): .field(name: name)
        }
    }
}

/// The sheet's inputs (F1, V9).
public struct FindReplaceRequest: Sendable, Hashable {
    public var find: String
    public var replace: String
    /// Regular Expression (F4 step 2).
    public var regex: Bool
    /// Match Case off (F4 step 3, V9).
    public var ignoreCase: Bool
    /// Whole Words (V9).
    public var wholeWords: Bool
    public var scope: FindReplaceScope

    public init(find: String, replace: String, regex: Bool = false, ignoreCase: Bool = true, wholeWords: Bool = false,
                scope: FindReplaceScope) {
        self.find = find
        self.replace = replace
        self.regex = regex
        self.ignoreCase = ignoreCase
        self.wholeWords = wholeWords
        self.scope = scope
    }

    var raw: MeguriCoreBindings.FindReplaceRequest {
        .init(find: find, replace: replace, regex: regex, ignoreCase: ignoreCase, wholeWords: wholeWords, within: scope.raw)
    }
}

/// Why Find and Replace refused (F11): nothing was written and both undo
/// histories are as they were (F9).
public enum FindReplaceRefusal: Sendable, Hashable {
    /// F7 step 1: a Tags replacement holding a space (U+0020 or U+3000).
    case separatorInReplacement
    /// F4 step 5: the pattern does not compile; the message only (F10).
    case invalidRegex(message: String)
    /// F6 step 7: a matched note whose field count differs from its note
    /// type's.
    case fieldCountMismatch(noteID: Int64)
    /// F6 step 7: a target note whose note type does not exist.
    case notetypeMissing(noteID: Int64)
    /// F11: the run would write more notes than were confirmed; `notes` is
    /// the count it read, for the sheet to show again.
    case countChanged(notes: UInt64)

    init(_ raw: MeguriCoreBindings.FindReplaceRefusal) {
        switch raw {
        case .separatorInReplacement: self = .separatorInReplacement
        case .invalidRegex(let message): self = .invalidRegex(message: message)
        case .fieldCountMismatch(let noteId): self = .fieldCountMismatch(noteID: noteId)
        case .notetypeMissing(let noteId): self = .notetypeMissing(noteID: noteId)
        case .countChanged(let notes): self = .countChanged(notes: notes)
        }
    }
}

/// One run of a preview entry's text (V9).
public struct FindReplaceSpan: Sendable, Hashable {
    public enum Edit: Sendable, Hashable {
        case kept
        /// Only in the old text: crossed out.
        case removed
        /// Only in the new text.
        case inserted
    }

    public let edit: Edit
    public let text: String
    public init(edit: Edit, text: String) {
        self.edit = edit
        self.text = text
    }

    init(_ raw: MeguriCoreBindings.FindReplaceSpan) {
        edit = switch raw.edit {
        case .kept: .kept
        case .removed: .removed
        case .inserted: .inserted
        }
        text = raw.text
    }
}

/// One changed (note, field) pair; Tags count as one field (V9).
public struct FindReplacePreviewEntry: Sendable, Hashable {
    public let noteID: Int64
    /// The note's Sort Field before the change; empty when its note type is
    /// missing.
    public let sortField: String
    /// The changed field's name (shown only with All Fields); nil for Tags.
    public let field: String?
    /// The old and the new text as one line: kept text, and in each run of
    /// changes the removed text before the inserted text.
    public let spans: [FindReplaceSpan]
    public init(noteID: Int64, sortField: String, field: String?, spans: [FindReplaceSpan]) {
        self.noteID = noteID
        self.sortField = sortField
        self.field = field
        self.spans = spans
    }

    init(_ raw: MeguriCoreBindings.FindReplacePreviewEntry) {
        self.init(noteID: raw.noteId, sortField: raw.sortField, field: raw.field, spans: raw.spans.map(FindReplaceSpan.init))
    }
}

/// What a Find and Replace would do (V9, F10), read without writing.
public struct FindReplacePreview: Sendable, Hashable {
    /// The notes the run would write: the sheet's "c of k notes", and the
    /// run's `confirmed` count.
    public let changed: UInt64
    /// The target's notes, deleted ones included; the notes read for All
    /// Notes (F8).
    public let considered: UInt64
    /// The entries of the first 100 changed notes, in result order
    /// (ascending note id for All Notes), each note's in field order.
    public let entries: [FindReplacePreviewEntry]
    public init(changed: UInt64, considered: UInt64, entries: [FindReplacePreviewEntry]) {
        self.changed = changed
        self.considered = considered
        self.entries = entries
    }

    init(_ raw: MeguriCoreBindings.FindReplacePreview) {
        self.init(changed: raw.changed, considered: raw.considered, entries: raw.entries.map(FindReplacePreviewEntry.init))
    }
}

/// What a run wrote (F8, F9).
public struct FindReplaceResult: Sendable, Hashable {
    /// The notes written.
    public let affected: UInt64
    public let considered: UInt64
    /// One undo step, "Find and Replace", or "Update Tag" for Tags.
    public let summary: MutationSummary
    public init(affected: UInt64, considered: UInt64, summary: MutationSummary) {
        self.affected = affected
        self.considered = considered
        self.summary = summary
    }
}

/// A started run: its progress and cancel (honoured until the commit
/// starts), and its result once it ends.
public final class CoreFindReplaceJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.BulkActionJob
    init(job: CoreJob, raw: MeguriCoreBindings.BulkActionJob) {
        self.job = job
        self.raw = raw
    }

    public func value() async throws(BridgeError) -> FindReplaceResult {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.bulkActionJobResult) {
            guard let done = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return FindReplaceResult(affected: done.affected, considered: done.considered,
                                     summary: MutationSummary(done.summary))
        }
    }
}

/// One Find and Replace sheet's previews: each `start` cancels the previous
/// one, in call order. Releasing the object cancels its newest preview.
public final class CoreFindReplacePreviews: Sendable {
    private let raw: MeguriCoreBindings.FindReplacePreviews
    private init(_ raw: MeguriCoreBindings.FindReplacePreviews) {
        self.raw = raw
    }

    public static func open() async throws(BridgeError) -> CoreFindReplacePreviews {
        try await MaintenanceLane.shared.call(.findReplacePreviewsNew) {
            CoreFindReplacePreviews(try MeguriCoreBindings.FindReplacePreviews())
        }
    }

    /// V9's preview of `request` on `target` (nil: All Notes), as a job with
    /// progress and Cancel. It writes nothing. A preview a newer one
    /// superseded ends `cancelled`. A refusal throws
    /// `BridgeError.findReplaceRefused`; a selection that cannot be resolved,
    /// `invalidArgument`.
    public func start(_ request: FindReplaceRequest, on target: BrowseSelection?) async throws(BridgeError)
        -> CoreBrowseReadJob<FindReplacePreview> {
        try await CoreEventStream.shared.start()
        let previews = raw, rawTarget = target?.raw, rawRequest = request.raw
        return try await MaintenanceLane.shared.call(.findReplacePreviewsStart) {
            let start = try previews.start(target: rawTarget, request: rawRequest)
            let result = start.result
            return CoreBrowseReadJob(job: try Jobs.track(start.handle), resultCall: .findReplacePreviewJobResult) {
                try result.result().map(FindReplacePreview.init)
            }
        }
    }
}

public enum CoreFindReplace {
    /// F11: runs `request` on `target` (nil: All Notes) as a bulk job
    /// admitted by the review service. `confirmed` is the preview's
    /// `changed`: a run that would write more notes throws
    /// `.findReplaceRefused(.countChanged)`, writing nothing.
    public static func start(_ request: FindReplaceRequest, on target: BrowseSelection?, confirmed: UInt64)
        async throws(BridgeError) -> CoreFindReplaceJob {
        try await CoreEventStream.shared.start()
        let rawTarget = target?.raw, rawRequest = request.raw
        return try await MaintenanceLane.shared.call(.startFindReplace) {
            let start = try MeguriCoreBindings.startFindReplace(target: rawTarget, request: rawRequest, confirmed: confirmed)
            return CoreFindReplaceJob(job: try Jobs.track(start.handle), raw: start.result)
        }
    }
}
