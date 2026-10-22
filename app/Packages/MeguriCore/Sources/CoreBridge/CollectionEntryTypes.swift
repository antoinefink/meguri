import Foundation
internal import MeguriCoreBindings

public enum PackageFailure: Sendable, Hashable {
    case tooNew, unsupportedFormat, unsupportedMedia, diskFull, sourceNotFinalised, destinationExists, cancelled
    case schemaUnsupported(CollectionOpenFailure)
    case corrupt(String), io(String)
    case insufficientSpace(needed: UInt64, available: UInt64)
    init(_ raw: MeguriCoreBindings.PackageFailure) {
        switch raw {
        case .tooNew: self = .tooNew
        case .unsupportedFormat: self = .unsupportedFormat
        case .unsupportedMedia: self = .unsupportedMedia
        case .diskFull: self = .diskFull
        case .sourceNotFinalised: self = .sourceNotFinalised
        case .destinationExists: self = .destinationExists
        case .cancelled: self = .cancelled
        case .schemaUnsupported(let gate): self = .schemaUnsupported(CollectionOpenFailure(gate))
        case .corrupt(let detail): self = .corrupt(detail)
        case .io(let detail): self = .io(detail)
        case .insufficientSpace(let needed, let available): self = .insufficientSpace(needed: needed, available: available)
        }
    }
}
public struct ImportBackupReceipt: Sendable, Hashable {
    public let name: String
    public let sizeBytes: UInt64
    public let instantMilliseconds: Int64
    public let snapshotModifiedMilliseconds: Int64
    init(_ raw: MeguriCoreBindings.ImportBackupReceipt) {
        name = raw.name; sizeBytes = raw.sizeBytes; instantMilliseconds = raw.instantMs
        snapshotModifiedMilliseconds = raw.snapshotModifiedMs
    }
}
public enum ImportBackupPublication: Sendable, Hashable {
    case unpublished
    case uncertain(name: String, instantMilliseconds: Int64)
    case durable(ImportBackupReceipt)
    init(_ raw: MeguriCoreBindings.ImportBackupPublication) {
        switch raw {
        case .unpublished: self = .unpublished
        case .uncertain(let name, let instant): self = .uncertain(name: name, instantMilliseconds: instant)
        case .durable(let receipt): self = .durable(ImportBackupReceipt(receipt))
        }
    }
}
public enum ImportBackupCause: Sendable, Hashable {
    case core(BridgeError), package(PackageFailure)
    init(_ raw: MeguriCoreBindings.ImportBackupCause) {
        switch raw {
        case .core(let error): self = .core(BridgeError(error))
        case .package(let error): self = .package(PackageFailure(error))
        }
    }
}
public enum ImportStagingFailure: Sendable, Hashable {
    case crossDevice, stagingCorrupt, stagingNotFinalised, liveWalNotEmpty
    case stagingRefused(CollectionOpenFailure), callerChangeFailed(BridgeError), io(String)
    init(_ raw: MeguriCoreBindings.ImportStagingFailure) {
        switch raw {
        case .crossDevice: self = .crossDevice
        case .stagingCorrupt: self = .stagingCorrupt
        case .stagingNotFinalised: self = .stagingNotFinalised
        case .liveWalNotEmpty: self = .liveWalNotEmpty
        case .stagingRefused(let gate): self = .stagingRefused(CollectionOpenFailure(gate))
        case .callerChangeFailed(let error): self = .callerChangeFailed(BridgeError(error))
        case .io(let detail): self = .io(detail)
        }
    }
}
public enum ImportReplacementFailure: Sendable, Hashable {
    case notExclusive, staging(ImportStagingFailure), core(BridgeError)
    init(_ raw: MeguriCoreBindings.ImportReplacementFailure) {
        switch raw {
        case .notExclusive: self = .notExclusive
        case .staging(let failure): self = .staging(ImportStagingFailure(failure))
        case .core(let error): self = .core(BridgeError(error))
        }
    }
}
public enum ImportIssue: Sendable, Hashable {
    case package(PackageFailure), core(BridgeError), staleConfirmation, consumed, cancelled
    case replacement(ImportReplacementFailure)
    case safetyBackup(cause: ImportBackupCause, publication: ImportBackupPublication)
    init(_ raw: MeguriCoreBindings.ImportIssue) {
        switch raw {
        case .package(let error): self = .package(PackageFailure(error))
        case .core(let error): self = .core(BridgeError(error))
        case .staleConfirmation: self = .staleConfirmation
        case .consumed: self = .consumed
        case .cancelled: self = .cancelled
        case .replacement(let failure): self = .replacement(ImportReplacementFailure(failure))
        case .safetyBackup(let cause, let publication):
            self = .safetyBackup(cause: ImportBackupCause(cause), publication: ImportBackupPublication(publication))
        }
    }
}
public struct CollectionEntryFailure: Error, Sendable, Hashable {
    public let issue: ImportIssue
    public let candidateActive: Bool
    public let generation: UInt64?
    public let backup: ImportBackupReceipt?
    public init(issue: ImportIssue, candidateActive: Bool = false, generation: UInt64? = nil, backup: ImportBackupReceipt? = nil) {
        self.issue = issue; self.candidateActive = candidateActive; self.generation = generation; self.backup = backup
    }
    init(_ raw: MeguriCoreBindings.ImportFailure) {
        self.init(issue: ImportIssue(raw.issue), candidateActive: raw.candidateActive, generation: raw.generation,
                  backup: raw.backup.map(ImportBackupReceipt.init))
    }
}
public struct CollectionInspectionFacts: Sendable, Hashable {
    public let notes: UInt64
    public let cards: UInt64
    public let reviews: UInt64
    public let sourceSchema: Int64
    public let replacingExisting: Bool
    public let sourceSHA256: Data
    // The backend destination path is intentionally private to the service.
    init(_ raw: MeguriCoreBindings.CollectionInspectionFacts) {
        notes = raw.notes; cards = raw.cards; reviews = raw.reviews; sourceSchema = raw.sourceSchema
        replacingExisting = raw.replacingExisting; sourceSHA256 = Data(raw.sourceSha256)
    }
}
public final class InspectedCollection: Sendable {
    let raw: MeguriCoreBindings.CollectionInspection
    public let facts: CollectionInspectionFacts
    init(raw: MeguriCoreBindings.CollectionInspection, facts: CollectionInspectionFacts) { self.raw = raw; self.facts = facts }
    public func discard() async throws(BridgeError) {
        try await MaintenanceLane.shared.call(.collectionInspectionDiscard) { try self.raw.discard() }
    }
}
public struct CollectionActivation: Sendable, Hashable {
    public let generation: UInt64
    public let backup: ImportBackupReceipt?
    init(_ raw: MeguriCoreBindings.CollectionActivation) {
        generation = raw.generation; backup = raw.backup.map(ImportBackupReceipt.init)
    }
}
