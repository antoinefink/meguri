import Synchronization
internal import MeguriCoreBindings

public enum MaintenanceError: Error, Sendable, Hashable {
    case cancelled, corrupt, tooNew, schemaUnsupported, unsupportedFormat, invalidIDs, safetyBackupFailed
    case insufficientSpace(needed: UInt64, available: UInt64)
    case busy(runningOperation: String)
    case collectionUnavailable, collectionCorrupt, sessionActive, diskFull, destinationExists
    case invalidArgument(String), io(String)
    init(_ raw: MeguriCoreBindings.MaintenanceError) {
        switch raw {
        case .cancelled: self = .cancelled
        case .insufficientSpace(let needed, let available): self = .insufficientSpace(needed: needed, available: available)
        case .corrupt: self = .corrupt
        case .tooNew: self = .tooNew
        case .schemaUnsupported: self = .schemaUnsupported
        case .unsupportedFormat: self = .unsupportedFormat
        case .invalidIds: self = .invalidIDs
        case .safetyBackupFailed: self = .safetyBackupFailed
        case .busy(let operation): self = .busy(runningOperation: operation)
        case .collectionUnavailable: self = .collectionUnavailable
        case .collectionCorrupt: self = .collectionCorrupt
        case .sessionActive: self = .sessionActive
        case .invalidArgument(let message): self = .invalidArgument(message)
        case .diskFull: self = .diskFull
        case .io(let detail): self = .io(detail)
        case .destinationExists: self = .destinationExists
        }
    }
}
public struct MaintenanceFailure: Error, Sendable, Equatable {
    public let cause: MaintenanceError
    public let candidateActive: Bool
    public let generation: UInt64?
    public let safetyBackup: String?
    public let restored: RestoredBackup?
    public let backupPublication: ImportBackupPublication
    init(_ raw: MeguriCoreBindings.MaintenanceFailure) {
        cause = MaintenanceError(raw.cause); candidateActive = raw.candidateActive; generation = raw.generation
        safetyBackup = raw.safetyBackup; restored = raw.restored.map(RestoredBackup.init)
        backupPublication = ImportBackupPublication(raw.backupPublication)
    }
}
public struct RestoredBackup: Sendable, Equatable {
    public let name: String
    public let safetyBackup: String?
    public let generation: UInt64
    public let cloudPublicationPending: Bool
    public let deadlineMilliseconds: Int64?
    init(_ raw: MeguriCoreBindings.RestoredBackup) {
        name = raw.name; safetyBackup = raw.safetyBackup; generation = raw.generation
        cloudPublicationPending = raw.cloudPublicationPending; deadlineMilliseconds = raw.deadlineMs
    }
}
public enum BackupKind: Sendable, Equatable { case backup, serverCopy, serverCopyUnverified }
public struct BackupWallTime: Sendable, Equatable {
    public let year: UInt16
    public let month: UInt8
    public let day: UInt8
    public let hour: UInt8
    public let minute: UInt8
    public let second: UInt8
    init(_ raw: MeguriCoreBindings.BackupWallTime) {
        year = raw.year; month = raw.month; day = raw.day; hour = raw.hour; minute = raw.minute; second = raw.second
    }
}
public struct BackupEntry: Sendable, Equatable {
    public let name: String
    public let kind: BackupKind
    public let wallTime: BackupWallTime
    public let sizeBytes: UInt64
    init(_ raw: MeguriCoreBindings.BackupEntry) {
        name = raw.name; wallTime = BackupWallTime(raw.wallTime); sizeBytes = raw.sizeBytes
        switch raw.kind { case .backup: kind = .backup; case .serverCopy: kind = .serverCopy; case .serverCopyUnverified: kind = .serverCopyUnverified }
    }
}
public struct BackupLimits: Sendable, Equatable {
    public var daily: UInt32
    public var weekly: UInt32
    public var monthly: UInt32
    public var minimumIntervalMinutes: UInt32
    public init(daily: UInt32, weekly: UInt32, monthly: UInt32, minimumIntervalMinutes: UInt32) {
        self.daily = daily; self.weekly = weekly; self.monthly = monthly; self.minimumIntervalMinutes = minimumIntervalMinutes
    }
    init(_ raw: MeguriCoreBindings.BackupLimits) {
        self.init(daily: raw.daily, weekly: raw.weekly, monthly: raw.monthly, minimumIntervalMinutes: raw.minimumIntervalMins)
    }
    var raw: MeguriCoreBindings.BackupLimits {
        .init(daily: daily, weekly: weekly, monthly: monthly, minimumIntervalMins: minimumIntervalMinutes)
    }
}
public enum MaintenanceTrigger: Sendable, Equatable {
    case opened, background, quit, backgroundProcessing, deadline
    var raw: MeguriCoreBindings.MaintenanceTrigger {
        switch self {
        case .opened: .opened
        case .background: .background
        case .quit: .quit
        case .backgroundProcessing: .backgroundProcessing
        case .deadline: .deadline
        }
    }
}
public struct LastBackup: Sendable, Equatable {
    public let name: String
    public let instantMilliseconds: Int64
    public let snapshotModifiedMilliseconds: Int64
    init(_ raw: MeguriCoreBindings.LastBackup) {
        name = raw.name; instantMilliseconds = raw.instantMs; snapshotModifiedMilliseconds = raw.snapshotModifiedMs
    }
}
public struct LastQuickCheck: Sendable, Equatable {
    public let instantMilliseconds: Int64
    public let passed: Bool
    init(_ raw: MeguriCoreBindings.LastQuickCheck) { instantMilliseconds = raw.instantMs; passed = raw.passed }
}
public struct CheckDatabaseProblems: Sendable, Equatable {
    public let cardProperties: UInt64
    public let newPositionsTooHigh: UInt64
    public let cardLastReviewTime: UInt64
    public let cardsWithoutNote: UInt64
    public let decksMissing: UInt64
    public let fieldCounts: UInt64
    public let duplicateCardOrds: UInt64
    public let cardsWithoutTemplate: UInt64
    public let revlogProperties: UInt64
    public let notesInvalidUTF8: UInt64
    public let futureIDs: UInt64
    public let notetypesRecovered: UInt64
    init(_ raw: MeguriCoreBindings.CheckDatabaseProblems) {
        cardProperties = raw.cardProperties; newPositionsTooHigh = raw.newPositionsTooHigh; cardLastReviewTime = raw.cardLastReviewTime
        cardsWithoutNote = raw.cardsWithoutNote; decksMissing = raw.decksMissing; fieldCounts = raw.fieldCounts
        duplicateCardOrds = raw.duplicateCardOrds; cardsWithoutTemplate = raw.cardsWithoutTemplate; revlogProperties = raw.revlogProperties
        notesInvalidUTF8 = raw.notesInvalidUtf8; futureIDs = raw.futureIds; notetypesRecovered = raw.notetypesRecovered
    }
}
public enum CheckDatabaseResult: Sendable, Equatable {
    case corrupt, problems(CheckDatabaseProblems)
    init(_ raw: MeguriCoreBindings.CheckDatabaseResult) {
        switch raw { case .corrupt: self = .corrupt; case .problems(let counts): self = .problems(CheckDatabaseProblems(counts)) }
    }
}
public struct LastCheckDatabase: Sendable, Equatable {
    public let instantMilliseconds: Int64
    public let result: CheckDatabaseResult
    init(_ raw: MeguriCoreBindings.LastCheckDatabase) { instantMilliseconds = raw.instantMs; result = CheckDatabaseResult(raw.result) }
}
public enum MaintenanceRemedy: Sendable, Equatable { case restore }
public enum MaintenanceStatusField: Sendable, Equatable {
    case lastBackup, lastCheckDatabase, lastOptimize, lastQuickCheck, collectionBytes, mediaBytes, trashBytes, remedies
    init(_ raw: MeguriCoreBindings.MaintenanceStatusField) {
        switch raw {
        case .lastBackup: self = .lastBackup
        case .lastCheckDatabase: self = .lastCheckDatabase
        case .lastOptimize: self = .lastOptimize
        case .lastQuickCheck: self = .lastQuickCheck
        case .collectionBytes: self = .collectionBytes
        case .mediaBytes: self = .mediaBytes
        case .trashBytes: self = .trashBytes
        case .remedies: self = .remedies
        }
    }
}
public struct MaintenanceStatusFailure: Sendable, Equatable {
    public let field: MaintenanceStatusField
    public let cause: MaintenanceError
    init(_ raw: MeguriCoreBindings.MaintenanceStatusFailure) { field = MaintenanceStatusField(raw.field); cause = MaintenanceError(raw.cause) }
}
/// A nil value is absent only when that field has no entry in failures.
public struct MaintenanceStatus: Sendable, Equatable {
    public let lastBackup: LastBackup?
    public let lastCheckDatabase: LastCheckDatabase?
    public let lastOptimizeMilliseconds: Int64?
    public let lastQuickCheck: LastQuickCheck?
    public let collectionBytes: UInt64?
    public let mediaBytes: UInt64?
    public let trashBytes: UInt64?
    public let remedies: [MaintenanceRemedy]
    public let failures: [MaintenanceStatusFailure]
    init(_ raw: MeguriCoreBindings.MaintenanceStatus) {
        lastBackup = raw.lastBackup.map(LastBackup.init); lastCheckDatabase = raw.lastCheckDatabase.map(LastCheckDatabase.init)
        lastOptimizeMilliseconds = raw.lastOptimizeMs; lastQuickCheck = raw.lastQuickCheck.map(LastQuickCheck.init)
        collectionBytes = raw.collectionBytes; mediaBytes = raw.mediaBytes; trashBytes = raw.trashBytes
        remedies = raw.remedies.map { value in switch value { case .restore: .restore } }
        failures = raw.failures.map(MaintenanceStatusFailure.init)
    }
}
public final class MaintenanceOperation: Sendable {
    private struct State { var cancelled = false; var job: CoreJob? }
    private let state = Mutex(State())
    public init() {}
    public func cancel() async {
        let job = state.withLock { s in s.cancelled = true; return s.job }
        try? await job?.cancel()
    }
    func attach(_ job: CoreJob) async {
        let cancelled = state.withLock { s in s.job = job; return s.cancelled }
        if cancelled { try? await job.cancel() }
    }
    func check() throws(BridgeError) { if state.withLock({ $0.cancelled }) { throw .cancelled } }
}
