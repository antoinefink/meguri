import Foundation
import UniformTypeIdentifiers
internal import MeguriCoreBindings

// Deck package (`.apkg`) import (`package/apkg-import`, `package/transfer-sheets`
// X2): the values the import sheet reads. No generated type is public.

extension UTType {
    /// A deck package, as the app's Info.plist declares it
    /// (`package/file-intake` K2.2). Imported, never exported: the format is
    /// Anki's.
    public static let meguriDeckPackage = UTType(importedAs: "com.antoinefink.meguri.deck-package")
    /// A collection package (K2.2).
    public static let meguriCollectionPackage = UTType(importedAs: "com.antoinefink.meguri.collection-package")
}

/// `package/container` P18: the flow a file belongs to.
public enum PackageFileClass: Sendable, Hashable {
    /// A deck package: `DeckPackageImportService`.
    case deckPackage
    /// A whole collection (`.colpkg`, `collection.apkg`, `backup-*.apkg`):
    /// the collection package flow, with its replacement confirmation
    /// (`package/file-intake` K5).
    case collectionPackage
    case unsupported
    init(_ raw: MeguriCoreBindings.PackageFileClass) {
        self = switch raw {
        case .deckPackage: .deckPackage; case .collectionPackage: .collectionPackage; case .unsupported: .unsupported
        }
    }
}

public enum DeckPackageFormat: Sendable, Hashable { case latest, legacy2, legacy1 }

/// I4's `repaired`.
public struct DeckImportRepairs: Sendable, Hashable {
    public var noteIds: UInt64
    public var cardIds: UInt64
    public var revlogIds: UInt64
    public var fieldCounts: UInt64
    public var schedulerConverted: Bool
    public init(noteIds: UInt64 = 0, cardIds: UInt64 = 0, revlogIds: UInt64 = 0, fieldCounts: UInt64 = 0,
                schedulerConverted: Bool = false) {
        self.noteIds = noteIds; self.cardIds = cardIds; self.revlogIds = revlogIds
        self.fieldCounts = fieldCounts; self.schedulerConverted = schedulerConverted
    }
}

/// I4: what the package holds, known before any option is chosen.
public struct DeckImportFacts: Sendable, Hashable {
    public var format: DeckPackageFormat
    public var notes: UInt64
    public var cards: UInt64
    /// Human deck names, sorted.
    public var decks: [String]
    public var notetypes: [String]
    public var hasScheduling: Bool
    public var hasPresets: Bool
    public var mediaFiles: UInt64
    public var mediaBytes: UInt64
    public var repaired: DeckImportRepairs
    public init(format: DeckPackageFormat = .latest, notes: UInt64, cards: UInt64, decks: [String], notetypes: [String],
                hasScheduling: Bool = false, hasPresets: Bool = false, mediaFiles: UInt64 = 0, mediaBytes: UInt64 = 0,
                repaired: DeckImportRepairs = .init()) {
        self.format = format; self.notes = notes; self.cards = cards; self.decks = decks; self.notetypes = notetypes
        self.hasScheduling = hasScheduling; self.hasPresets = hasPresets; self.mediaFiles = mediaFiles
        self.mediaBytes = mediaBytes; self.repaired = repaired
    }
    init(_ raw: MeguriCoreBindings.DeckImportFacts) {
        self.init(format: { switch raw.format { case .latest: .latest; case .legacy2: .legacy2; case .legacy1: .legacy1 } }(),
                  notes: raw.notes, cards: raw.cards, decks: raw.decks, notetypes: raw.notetypes,
                  hasScheduling: raw.hasScheduling, hasPresets: raw.hasPresets, mediaFiles: raw.mediaFiles,
                  mediaBytes: raw.mediaBytes,
                  repaired: .init(noteIds: raw.repaired.noteIds, cardIds: raw.repaired.cardIds,
                                  revlogIds: raw.repaired.revlogIds, fieldCounts: raw.repaired.fieldCounts,
                                  schedulerConverted: raw.repaired.schedulerConverted))
    }
}

/// I5's three-valued options ("If newer", "Always", "Never").
public enum DeckImportUpdate: Sendable, Hashable, CaseIterable {
    case ifNewer, always, never
    init(_ raw: MeguriCoreBindings.DeckImportUpdate) {
        self = switch raw { case .ifNewer: .ifNewer; case .always: .always; case .never: .never }
    }
    var raw: MeguriCoreBindings.DeckImportUpdate {
        switch self { case .ifNewer: .ifNewer; case .always: .always; case .never: .never }
    }
}

/// I5: the five options, in the sheet's order (X2.2).
public struct DeckImportOptions: Sendable, Hashable {
    /// Import any learning progress.
    public var withScheduling: Bool
    /// Import any deck presets.
    public var withDeckConfigs: Bool
    public var mergeNotetypes: Bool
    public var updateNotes: DeckImportUpdate
    public var updateNotetypes: DeckImportUpdate
    public init(withScheduling: Bool = false, withDeckConfigs: Bool = false, mergeNotetypes: Bool = false,
                updateNotes: DeckImportUpdate = .ifNewer, updateNotetypes: DeckImportUpdate = .ifNewer) {
        self.withScheduling = withScheduling; self.withDeckConfigs = withDeckConfigs
        self.mergeNotetypes = mergeNotetypes; self.updateNotes = updateNotes; self.updateNotetypes = updateNotetypes
    }
    init(_ raw: MeguriCoreBindings.DeckImportOptions) {
        self.init(withScheduling: raw.withScheduling, withDeckConfigs: raw.withDeckConfigs,
                  mergeNotetypes: raw.mergeNotetypes, updateNotes: DeckImportUpdate(raw.updateNotes),
                  updateNotetypes: DeckImportUpdate(raw.updateNotetypes))
    }
    var raw: MeguriCoreBindings.DeckImportOptions {
        .init(mergeNotetypes: mergeNotetypes, updateNotes: updateNotes.raw, updateNotetypes: updateNotetypes.raw,
              withScheduling: withScheduling, withDeckConfigs: withDeckConfigs)
    }
}

/// I14.4: a reason that makes the import a schema change.
public enum DeckImportStructuralReason: Sendable, Hashable {
    case sortFieldChanged, notetypesMerged
    init(_ raw: MeguriCoreBindings.DeckImportStructuralReason) {
        self = switch raw { case .sortFieldChanged: .sortFieldChanged; case .notetypesMerged: .notetypesMerged }
    }
}

/// Why the package collection's gate refused it (`SchemaUnsupported`).
public enum DeckImportSchemaGate: Sendable, Hashable { case tooOld, tooNew, other }

/// I15.1: the limit a package is past.
public enum DeckImportLimit: Sendable, Hashable {
    case rows, guidBytes, noteBytes, tagUses, cardDataBytes, structureRows, notetypeParts, structureBytes, other
}

/// I16, one case per engine error (`package/transfer-sheets` X2.6 words
/// each). Every case but `committedWithoutResult` leaves the collection, its
/// media folder and its backups as they were.
public enum DeckImportRefusal: Error, Sendable, Hashable {
    case unsupportedFile
    /// The file is a whole collection: the collection package flow imports
    /// it (`package/file-intake` K5). Nothing was copied.
    case collectionPackage
    case tooNew
    /// `detail` is for Copy Debug Info, never shown.
    case corrupt(detail: String)
    case schemaUnsupported(DeckImportSchemaGate)
    case insufficientSpace(needed: UInt64, available: UInt64)
    case diskFull
    case stale
    case notetypeKindConflict(incoming: Int64, target: Int64)
    case deckTooDeep(name: String)
    case tooLarge(what: DeckImportLimit, found: UInt64, limit: UInt64)
    case sessionClosed
    case cancelled
    case io(detail: String)
    case busy(runningOperation: String)
    case collectionUnavailable(reason: String)
    case staleGeneration
    case safetyBackupFailed
    /// Any other refusal before the commit, the bridge's own included.
    case engine(BridgeError)
    /// The selected file could not be opened for reading (a folder, a file
    /// that is gone, a provider that refused): `package/file-intake` K3
    /// step 3. Nothing was copied.
    case unreadableFile
    /// The rows are committed and the result cannot be shown: never say
    /// that the collection is unchanged, never offer to import again.
    case committedWithoutResult

    init(_ raw: MeguriCoreBindings.DeckImportRefusal) {
        self = switch raw {
        case .unsupportedFile: .unsupportedFile
        case .collectionPackage: .collectionPackage
        case .tooNew: .tooNew
        case .corrupt(let detail): .corrupt(detail: detail)
        case .schemaUnsupported(let gate):
            .schemaUnsupported({ switch gate { case .tooOld: .tooOld; case .tooNew: .tooNew; case .other: .other } }())
        case .insufficientSpace(let needed, let available): .insufficientSpace(needed: needed, available: available)
        case .diskFull: .diskFull
        case .stale: .stale
        case .notetypeKindConflict(let incoming, let target): .notetypeKindConflict(incoming: incoming, target: target)
        case .deckTooDeep(let name): .deckTooDeep(name: name)
        case .tooLarge(let what, let found, let limit):
            .tooLarge(what: { switch what {
                case .rows: .rows; case .guidBytes: .guidBytes; case .noteBytes: .noteBytes; case .tagUses: .tagUses
                case .cardDataBytes: .cardDataBytes; case .structureRows: .structureRows
                case .notetypeParts: .notetypeParts; case .structureBytes: .structureBytes; case .other: .other
            } }(), found: found, limit: limit)
        case .sessionClosed: .sessionClosed
        case .cancelled: .cancelled
        case .io(let detail): .io(detail: detail)
        case .busy(let runningOp): .busy(runningOperation: runningOp)
        case .collectionUnavailable(let reason): .collectionUnavailable(reason: reason)
        case .staleGeneration: .staleGeneration
        case .safetyBackupFailed: .safetyBackupFailed
        case .engine(let error): .engine(BridgeError(error))
        case .committedWithoutResult: .committedWithoutResult
        }
    }
    /// A bridge error raised around a job, as the refusal it is.
    init(_ error: BridgeError) {
        self = switch error {
        case .cancelled: .cancelled
        case .busy(let runningOperation): .busy(runningOperation: runningOperation)
        case .unavailable(let reason): .collectionUnavailable(reason: reason)
        case .staleGeneration: .staleGeneration
        case .insufficientSpace(let required, let available): .insufficientSpace(needed: required, available: available)
        case .commitCompleted, .mediaCommitPending: .committedWithoutResult
        default: .engine(error)
        }
    }
    /// False only when the import's rows may be committed.
    public var collectionUnchanged: Bool { self != .committedWithoutResult }
}

/// I14.5's counts.
public struct DeckImportCounts: Sendable, Hashable {
    public var notesAdded: UInt64 = 0
    public var notesUpdated: UInt64 = 0
    public var cardsAdded: UInt64 = 0
    public var revlogAdded: UInt64 = 0
    public var notetypesAdded: UInt64 = 0
    public var notetypesUpdated: UInt64 = 0
    public var notetypesCopied: UInt64 = 0
    /// With a merge: target notes moved between note types.
    public var notesMoved: UInt64 = 0
    /// With a merge: emptied note types removed.
    public var notetypesRemoved: UInt64 = 0
    public var decksAdded: UInt64 = 0
    public var decksUpdated: UInt64 = 0
    public var presetsAdded: UInt64 = 0
    public var mediaWritten: UInt64 = 0
    public var mediaReused: UInt64 = 0
    /// Media files that were not added (X2.5's line).
    public var mediaSkipped: UInt64 = 0
    public init() {}
    init(_ raw: MeguriCoreBindings.DeckImportCounts) {
        notesAdded = raw.notesAdded; notesUpdated = raw.notesUpdated; cardsAdded = raw.cardsAdded
        revlogAdded = raw.revlogAdded; notetypesAdded = raw.notetypesAdded; notetypesUpdated = raw.notetypesUpdated
        notetypesCopied = raw.notetypesCopied; notesMoved = raw.notesMoved; notetypesRemoved = raw.notetypesRemoved
        decksAdded = raw.decksAdded; decksUpdated = raw.decksUpdated; presetsAdded = raw.presetsAdded
        mediaWritten = raw.mediaWritten; mediaReused = raw.mediaReused; mediaSkipped = raw.mediaSkipped
    }
}

/// I13's categories. A note is in exactly one of `new`, `updated`,
/// `duplicate`, `conflicting` and `missingNotetype`; one in `missingDeck`
/// is also in its own list (X2.5).
public enum DeckImportLogCategory: Sendable, Hashable, CaseIterable {
    case new, updated, duplicate, conflicting, missingNotetype, missingDeck, firstFieldMatch, emptyFirstField
    var raw: MeguriCoreBindings.DeckImportLogCategory {
        switch self {
        case .new: .new; case .updated: .updated; case .duplicate: .duplicate; case .conflicting: .conflicting
        case .missingNotetype: .missingNotetype; case .missingDeck: .missingDeck
        case .firstFieldMatch: .firstFieldMatch; case .emptyFirstField: .emptyFirstField
        }
    }
}

/// How many entries each category of the log holds; the entries come in
/// pages (`DeckPackageImportService.logPage`).
public struct DeckImportLogCounts: Sendable, Hashable {
    /// The notes found in the file.
    public var foundNotes: UInt64 = 0
    /// Entries listed without their text (the log's text budget, I13).
    public var fieldsOmitted: UInt64 = 0
    public var entries: [DeckImportLogCategory: UInt64] = [:]
    public init(foundNotes: UInt64 = 0, fieldsOmitted: UInt64 = 0, entries: [DeckImportLogCategory: UInt64] = [:]) {
        self.foundNotes = foundNotes; self.fieldsOmitted = fieldsOmitted; self.entries = entries
    }
    public subscript(category: DeckImportLogCategory) -> UInt64 { entries[category] ?? 0 }
    init(_ raw: MeguriCoreBindings.DeckImportLogCounts) {
        self.init(foundNotes: raw.foundNotes, fieldsOmitted: raw.fieldsOmitted, entries: [
            .new: raw.new, .updated: raw.updated, .duplicate: raw.duplicate, .conflicting: raw.conflicting,
            .missingNotetype: raw.missingNotetype, .missingDeck: raw.missingDeck,
            .firstFieldMatch: raw.firstFieldMatch, .emptyFirstField: raw.emptyFirstField,
        ])
    }
}

/// One entry of the log (I13).
public struct DeckImportLogEntry: Sendable, Hashable {
    /// The target note's id, or the package note's for `conflicting` and
    /// `missingNotetype`.
    public var noteId: Int64
    /// Every field, reduced as I13 says; empty once the log's text budget
    /// is spent.
    public var fields: [String]
    /// For `conflicting`: the target note that holds the GUID.
    public var existingId: Int64?
    public init(noteId: Int64, fields: [String], existingId: Int64? = nil) {
        self.noteId = noteId; self.fields = fields; self.existingId = existingId
    }
}

public struct DeckImportLayout: Sendable, Hashable {
    public var notetypeId: Int64
    /// Names changed, positions kept.
    public var identity: Bool
    public init(notetypeId: Int64, identity: Bool) { self.notetypeId = notetypeId; self.identity = identity }
}

/// What the import reported about note type structure (I14.4).
public struct DeckImportStructure: Sendable, Hashable {
    /// Empty for an import that set no `scm`.
    public var reasons: [DeckImportStructuralReason] = []
    public var layouts: [DeckImportLayout] = []
    public var removedNotetypes: [Int64] = []
    /// (old, new) note type of the notes a merge moved.
    public var typeChanges: [[Int64]] = []
    public init() {}
    init(_ raw: MeguriCoreBindings.DeckImportStructure) {
        reasons = raw.reasons.map(DeckImportStructuralReason.init)
        layouts = raw.layouts.map { .init(notetypeId: $0.notetypeId, identity: $0.identity) }
        removedNotetypes = raw.removed
        typeChanges = raw.typeChanges.map { [$0.from, $0.to] }
    }
}

/// I14.5: what a committed import reports.
public struct DeckImportResult: Sendable, Hashable {
    /// The op's seq.
    public var sequence: UInt64
    public var generation: UInt64
    /// The undo step's name ("Import"), when the import changed something.
    /// The sheet offers Undo only then (X2.5).
    public var undoStep: String?
    public var counts: DeckImportCounts
    public var log: DeckImportLogCounts
    public var structure: DeckImportStructure
    /// The safety backup's name, when one was taken or reused.
    public var safetyBackup: String?
    /// The notes are imported and some media files are still being put in
    /// place, which finishes on its own (X2.6's `MediaCommitPending`). Never
    /// offer to import again.
    public var mediaPending: Bool
    public init(sequence: UInt64, generation: UInt64, undoStep: String?, counts: DeckImportCounts = .init(),
                log: DeckImportLogCounts = .init(), structure: DeckImportStructure = .init(),
                safetyBackup: String? = nil, mediaPending: Bool = false) {
        self.sequence = sequence; self.generation = generation; self.undoStep = undoStep; self.counts = counts
        self.log = log; self.structure = structure; self.safetyBackup = safetyBackup; self.mediaPending = mediaPending
    }
    init(_ raw: MeguriCoreBindings.DeckImportResult) {
        self.init(sequence: raw.seq, generation: raw.generation, undoStep: raw.undoStep,
                  counts: DeckImportCounts(raw.counts), log: DeckImportLogCounts(raw.log),
                  structure: DeckImportStructure(raw.structure), safetyBackup: raw.safetyBackup,
                  mediaPending: raw.mediaPending)
    }
}

/// J10: one field whose content the import would delete.
public struct DeckImportEmptiedField: Sendable, Hashable {
    public var notetypeId: Int64
    public var notetypeName: String
    public var position: UInt32
    public var fieldName: String
    public var notes: UInt64
    public init(notetypeId: Int64, notetypeName: String, position: UInt32, fieldName: String, notes: UInt64) {
        self.notetypeId = notetypeId; self.notetypeName = notetypeName; self.position = position
        self.fieldName = fieldName; self.notes = notes
    }
}
/// J10: a note type the merge removes.
public struct DeckImportRemovedNotetype: Sendable, Hashable {
    public var notetypeId: Int64
    public var name: String
    public var movedTo: Int64
    /// The open collection's name for `movedTo`, when it has that note type.
    public var movedToName: String?
    public var notes: UInt64
    public init(notetypeId: Int64, name: String, movedTo: Int64, movedToName: String?, notes: UInt64) {
        self.notetypeId = notetypeId; self.name = name; self.movedTo = movedTo
        self.movedToName = movedToName; self.notes = notes
    }
}
/// J10: notes that hold more values than their note type has fields.
public struct DeckImportDroppedValues: Sendable, Hashable {
    public var notetypeId: Int64
    /// The open collection's name for the note type, when it has it.
    public var notetypeName: String?
    public var notes: UInt64
    public init(notetypeId: Int64, notetypeName: String?, notes: UInt64) {
        self.notetypeId = notetypeId; self.notetypeName = notetypeName; self.notes = notes
    }
}
/// J10: what the import would delete, in the preview's order.
public struct DeckImportPreview: Sendable, Hashable {
    public var emptiedFields: [DeckImportEmptiedField]
    public var removedNotetypes: [DeckImportRemovedNotetype]
    public var droppedValues: [DeckImportDroppedValues]
    public var isEmpty: Bool { emptiedFields.isEmpty && removedNotetypes.isEmpty && droppedValues.isEmpty }
    public init(emptiedFields: [DeckImportEmptiedField] = [], removedNotetypes: [DeckImportRemovedNotetype] = [],
                droppedValues: [DeckImportDroppedValues] = []) {
        self.emptiedFields = emptiedFields; self.removedNotetypes = removedNotetypes; self.droppedValues = droppedValues
    }
    init(_ raw: MeguriCoreBindings.DeckImportPreview) {
        self.init(
            emptiedFields: raw.emptiedFields.map { .init(notetypeId: $0.notetypeId, notetypeName: $0.notetypeName,
                position: $0.position, fieldName: $0.fieldName, notes: $0.notes) },
            removedNotetypes: raw.removedNotetypes.map { .init(notetypeId: $0.notetypeId, name: $0.name,
                movedTo: $0.movedTo, movedToName: $0.movedToName, notes: $0.notes) },
            droppedValues: raw.droppedValues.map { .init(notetypeId: $0.notetypeId, notetypeName: $0.notetypeName,
                notes: $0.notes) })
    }
}

/// I8: the apply wrote nothing and asks first. Show the preview's rows, and
/// with `reasons` the sync paragraph (X2.3); pass this value to the next
/// apply with the same options. It covers exactly what it shows: an apply
/// whose plan changed answers with a new one.
public struct DeckImportConfirmation: Sendable {
    /// Possibly empty when `reasons` is not.
    public let preview: DeckImportPreview
    /// The plan's structural reasons, when the apply asked about structure
    /// (a ready Meguri Sync binding, `sync/meguri-provider` MP7.1); empty
    /// otherwise.
    public let reasons: [DeckImportStructuralReason]
    enum Token: Sendable { case core(MeguriCoreBindings.DeckImportConfirmation), fake(UInt64) }
    let token: Token
    init(_ raw: MeguriCoreBindings.DeckImportConfirmationRequest) {
        preview = DeckImportPreview(raw.preview)
        reasons = raw.reasons.map(DeckImportStructuralReason.init)
        token = .core(raw.confirmation)
    }
    /// A confirmation of `FakeDeckPackageImportService`.
    public init(preview: DeckImportPreview, reasons: [DeckImportStructuralReason], fakeToken: UInt64) {
        self.preview = preview; self.reasons = reasons; token = .fake(fakeToken)
    }
    /// The fake's token, nil for the core's.
    public var fakeToken: UInt64? { if case .fake(let value) = token { value } else { nil } }
    var raw: MeguriCoreBindings.DeckImportConfirmation? { if case .core(let value) = token { value } else { nil } }
}

/// What an apply answered.
public enum DeckImportApplyOutcome: Sendable {
    /// Committed; `mediaPending` says when media still settle.
    case imported(DeckImportResult)
    /// Nothing was written: show the confirmation of X2.3.
    case confirmationRequired(DeckImportConfirmation)
    /// Nothing was written, except for `committedWithoutResult`. The session
    /// stays open for another apply unless it says `sessionClosed`.
    case refused(DeckImportRefusal)
    init(_ raw: MeguriCoreBindings.DeckImportApplyOutcome) {
        self = switch raw {
        case .imported(let result): .imported(DeckImportResult(result))
        case .confirmationRequired(let request): .confirmationRequired(DeckImportConfirmation(request))
        case .refused(let refusal): .refused(DeckImportRefusal(refusal))
        }
    }
}

/// I14.5's stages, in the order they run.
public enum DeckImportStage: Sendable, Hashable {
    /// Inspect: the source copy and the collection entry.
    case extracting
    /// Inspect: the gate, the repairs and the facts.
    case analysing
    /// Apply: reading the package and planning.
    case planning
    /// Apply: media payloads staged, `completed` of `total`.
    case media
    /// Apply: the safety backup, when the plan takes one.
    case backup
    /// Apply: the write phase. It cannot be cancelled.
    case committing
    /// A stage this build does not name.
    case other(String)
    init(_ phase: CoreProgressPhase) {
        self = switch phase {
        case .planning: .planning
        case .committing: .committing
        case .named("extracting"): .extracting
        case .named("analysing"): .analysing
        case .named("media"): .media
        case .named("backup"): .backup
        case .named(let name): .other(name)
        }
    }
}

/// One progress report of an inspect or an apply.
public struct DeckImportProgress: Sendable, Hashable {
    public var stage: DeckImportStage
    public var completed: UInt64
    /// Nil while the amount of work is unknown.
    public var total: UInt64?
    /// False once the write phase began: Cancel is no longer offered.
    public var cancellable: Bool { stage != .committing }
    public init(stage: DeckImportStage, completed: UInt64 = 0, total: UInt64? = nil) {
        self.stage = stage; self.completed = completed; self.total = total
    }
    init(_ update: CoreProgress) {
        self.init(stage: DeckImportStage(update.phase), completed: update.completed, total: update.total)
    }
}

/// An inspected deck package (I2): the handle the apply, the log and close
/// take. It commits at most once. Closing it, or letting the last reference
/// go, removes its staging directory.
public final class DeckImportSession: Sendable {
    public let facts: DeckImportFacts
    /// The file's name as the user saw it, for the sheet's titles.
    public let fileName: String
    enum Backing: Sendable { case core(MeguriCoreBindings.DeckImportSession), fake(UInt64) }
    let backing: Backing
    init(raw: MeguriCoreBindings.DeckImportSession, facts: DeckImportFacts, fileName: String) {
        backing = .core(raw); self.facts = facts; self.fileName = fileName
    }
    /// A session of `FakeDeckPackageImportService`.
    public init(fake id: UInt64, facts: DeckImportFacts, fileName: String) {
        backing = .fake(id); self.facts = facts; self.fileName = fileName
    }
    public var fakeId: UInt64? { if case .fake(let id) = backing { id } else { nil } }
    var raw: MeguriCoreBindings.DeckImportSession? { if case .core(let raw) = backing { raw } else { nil } }
}

/// A running inspect or apply: its progress as a stream and its value.
/// Cancelling it, or the task that awaits `value()`, asks the core to stop;
/// the work still ends with its outcome (`cancelled` when it stopped), and
/// from the write phase on an import completes.
public final class DeckImportTask<Value: Sendable>: Sendable {
    /// The newest report; finishes when the work does.
    public let progress: AsyncStream<DeckImportProgress>
    private let task: Task<Value, any Error>
    /// `work` receives the sink of `progress`.
    public init(_ work: @escaping @Sendable (@escaping @Sendable (DeckImportProgress) -> Void) async throws -> Value) {
        let (stream, continuation) = AsyncStream.makeStream(of: DeckImportProgress.self, bufferingPolicy: .bufferingNewest(1))
        progress = stream
        task = Task {
            defer { continuation.finish() }
            return try await work { continuation.yield($0) }
        }
    }
    public func value() async throws -> Value {
        try await withTaskCancellationHandler { try await task.value } onCancel: { task.cancel() }
    }
    public func cancel() { task.cancel() }
}
