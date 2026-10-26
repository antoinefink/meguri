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
    case mediaRecoveryRequired(recoveryReference: String)
    init(_ raw: MeguriCoreBindings.ImportReplacementFailure) {
        switch raw {
        case .mediaRecoveryRequired(let reference): self = .mediaRecoveryRequired(recoveryReference: reference)
        case .notExclusive: self = .notExclusive
        case .staging(let failure): self = .staging(ImportStagingFailure(failure))
        case .core(let error): self = .core(BridgeError(error))
        }
    }
}
/// Native result delivery failed after the backend accepted an activation job.
/// Neither case establishes which collection or media view is active.
public enum NativeActivationUncertainty: Sendable, Hashable {
    case outcomeReadFailed(BridgeError), outcomePending
}
public enum ImportIssue: Sendable, Hashable {
    case package(PackageFailure), core(BridgeError), staleConfirmation, consumed, cancelled
    case replacement(ImportReplacementFailure)
    case safetyBackup(cause: ImportBackupCause, publication: ImportBackupPublication)
    case nativeActivationUncertain(NativeActivationUncertainty)
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
    public let mediaRecoveryReference: String?
    public let media: CollectionImportMedia?
    /// Observed preparation evidence, never proof of completed publication.
    public let preparedMedia: CollectionImportMedia?
    /// Present only when a successful activation receipt was actually read.
    public let completedActivation: CollectionActivation?
    public var activationOutcomeUncertain: Bool {
        if case .nativeActivationUncertain = issue { return true }
        return false
    }
    public var mediaRecoveryRequired: Bool {
        if case .replacement(.mediaRecoveryRequired) = issue { return true }
        return false
    }
    public var requiresRecovery: Bool { activationOutcomeUncertain || candidateActive || mediaRecoveryRequired }
    public var requiresReopenRecovery: Bool { activationOutcomeUncertain || mediaRecoveryRequired || (candidateActive && generation == nil) }
    public init(issue: ImportIssue, candidateActive: Bool = false, generation: UInt64? = nil,
                backup: ImportBackupReceipt? = nil, mediaRecoveryReference: String? = nil,
                media: CollectionImportMedia? = nil, preparedMedia: CollectionImportMedia? = nil) {
        self.issue = issue; self.candidateActive = candidateActive; self.generation = generation; self.backup = backup
        self.mediaRecoveryReference = mediaRecoveryReference; self.media = media
        self.preparedMedia = preparedMedia; completedActivation = nil
    }
    /// Preserve every authoritative activation field when native delivery/setup fails.
    public init(after activation: CollectionActivation, issue: ImportIssue) {
        self.issue = issue; candidateActive = true; generation = activation.generation; backup = activation.backup
        mediaRecoveryReference = activation.mediaRetentionReference; media = activation.media
        preparedMedia = nil; completedActivation = activation
    }
    init(_ raw: MeguriCoreBindings.ImportFailure) {
        self.init(issue: ImportIssue(raw.issue), candidateActive: raw.candidateActive, generation: raw.generation,
                  backup: raw.backup.map(ImportBackupReceipt.init),
                  mediaRecoveryReference: raw.mediaRecoveryReference, media: raw.media.map(CollectionImportMedia.init),
                  preparedMedia: raw.media.map(CollectionImportMedia.init))
    }
}
public struct CollectionInspectionFacts: Sendable, Hashable {
    public let notes: UInt64
    public let cards: UInt64
    public let reviews: UInt64
    public let sourceSchema: Int64
    public let replacingExisting: Bool
    public let sourceSHA256: Data
    public let mediaCount: UInt64
    public let mediaDecodedBytes: UInt64
    // The backend destination path is intentionally private to the service.
    init(_ raw: MeguriCoreBindings.CollectionInspectionFacts) {
        notes = raw.notes; cards = raw.cards; reviews = raw.reviews; sourceSchema = raw.sourceSchema
        replacingExisting = raw.replacingExisting; sourceSHA256 = Data(raw.sourceSha256)
        mediaCount = raw.mediaCount; mediaDecodedBytes = raw.mediaDecodedBytes
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
public struct CollectionImportMedia: Sendable, Hashable {
    public let incoming: UInt64
    public let decodedBytes: UInt64
    public let reused: UInt64
    public let added: UInt64
    public let trashed: UInt64
    public init(incoming: UInt64, decodedBytes: UInt64, reused: UInt64, added: UInt64, trashed: UInt64) {
        self.incoming = incoming; self.decodedBytes = decodedBytes
        self.reused = reused; self.added = added; self.trashed = trashed
    }
    init(_ raw: MeguriCoreBindings.CollectionImportMedia) {
        self.init(incoming: raw.incoming, decodedBytes: raw.decodedBytes,
                  reused: raw.reused, added: raw.added, trashed: raw.trashed)
    }
}
public struct CollectionActivation: Sendable, Hashable {
    public let generation: UInt64
    public let backup: ImportBackupReceipt?
    public let media: CollectionImportMedia
    public let mediaRetentionReference: String?
    init(_ raw: MeguriCoreBindings.CollectionActivation) {
        generation = raw.generation; backup = raw.backup.map(ImportBackupReceipt.init)
        media = CollectionImportMedia(raw.media); mediaRetentionReference = raw.mediaRetentionReference
    }
}
