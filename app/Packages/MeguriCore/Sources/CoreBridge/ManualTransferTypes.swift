import Foundation
import Synchronization
internal import MeguriCoreBindings

public enum ManualTransferDirection: String, Sendable, Equatable {
    case `import`, export
    init(_ raw: MeguriCoreBindings.TransferDirection) {
        switch raw {
        case .import: self = .import
        case .export: self = .export
        }
    }
}
public enum ManualTransferPhase: String, Sendable, Equatable {
    case preparing, preview, confirmedIntent, transferring, remoteAccepted, activating, collectionComplete, mediaIncomplete, complete, cancelled, failed, needsDecision
    init(_ raw: MeguriCoreBindings.TransferPhase) {
        switch raw {
        case .preparing: self = .preparing
        case .preview: self = .preview
        case .confirmedIntent: self = .confirmedIntent
        case .transferring: self = .transferring
        case .remoteAccepted: self = .remoteAccepted
        case .activating: self = .activating
        case .collectionComplete: self = .collectionComplete
        case .mediaIncomplete: self = .mediaIncomplete
        case .complete: self = .complete
        case .cancelled: self = .cancelled
        case .failed: self = .failed
        case .needsDecision: self = .needsDecision
        }
    }
}
public enum ManualTransferAcceptance: String, Sendable, Equatable {
    case notSent, possiblyAccepted, exactOk, refused
    init(_ raw: MeguriCoreBindings.TransferAcceptance) {
        switch raw {
        case .notSent: self = .notSent
        case .possiblyAccepted: self = .possiblyAccepted
        case .exactOk: self = .exactOk
        case .refused: self = .refused
        }
    }
}
public enum ManualTransferBoundary: String, Sendable, Equatable {
    case localChanged, remoteChanged, identityChanged, artifactChanged, remoteCompletionUnknown, activationInterrupted, mediaProviderUnavailable, preparationInterrupted, cancelled, interrupted, storage, server, invalidDownload
    init(_ raw: MeguriCoreBindings.TransferBoundary) {
        switch raw {
        case .localChanged: self = .localChanged
        case .remoteChanged: self = .remoteChanged
        case .identityChanged: self = .identityChanged
        case .artifactChanged: self = .artifactChanged
        case .remoteCompletionUnknown: self = .remoteCompletionUnknown
        case .activationInterrupted: self = .activationInterrupted
        case .mediaProviderUnavailable: self = .mediaProviderUnavailable
        case .preparationInterrupted: self = .preparationInterrupted
        case .cancelled: self = .cancelled
        case .interrupted: self = .interrupted
        case .storage: self = .storage
        case .server: self = .server
        case .invalidDownload: self = .invalidDownload
        }
    }
}
public enum ManualTransferIssue: String, Sendable, Equatable {
    case busy, stale, artifact, cancelled, interrupted, sessionActive, unavailable, diskFull, safetyBackup, invalidDownload, storage, server, needsDecision, mediaIncomplete, cooldown
    init(_ raw: MeguriCoreBindings.TransferIssue) {
        switch raw {
        case .busy: self = .busy
        case .stale: self = .stale
        case .artifact: self = .artifact
        case .cancelled: self = .cancelled
        case .interrupted: self = .interrupted
        case .sessionActive: self = .sessionActive
        case .unavailable: self = .unavailable
        case .diskFull: self = .diskFull
        case .safetyBackup: self = .safetyBackup
        case .invalidDownload: self = .invalidDownload
        case .storage: self = .storage
        case .server: self = .server
        case .needsDecision: self = .needsDecision
        case .mediaIncomplete: self = .mediaIncomplete
        case .cooldown: self = .cooldown
        }
    }
}
public struct ManualTransferFailure: Error, Sendable {
    public let issue: ManualTransferIssue
    public let boundary: ManualTransferBoundary?
    public let generation: UInt64?
    public let candidateActive: Bool
    public let nextEligibleMilliseconds: Int64?
    public let serverKind: String?
    init(_ raw: MeguriCoreBindings.TransferFailure) {
        issue = .init(raw.issue); boundary = raw.boundary.map(ManualTransferBoundary.init)
        generation = raw.generation; candidateActive = raw.candidateActive
        nextEligibleMilliseconds = raw.nextEligibleMs; serverKind = raw.serverKind
    }
}
public struct ManualTransferCounts: Sendable, Equatable {
    public let cards: UInt64, notes: UInt64, reviews: UInt64, pendingReviews: UInt64
    public let pendingRows: [UInt64]
    init(_ raw: MeguriCoreBindings.TransferCounts) {
        cards = raw.cards; notes = raw.notes; reviews = raw.reviews; pendingReviews = raw.pendingReviews; pendingRows = raw.pendingRows
    }
}
public struct ManualTransferCopy: Sendable, Equatable, Identifiable {
    public let name: String, byteLength: UInt64, sha256: [UInt8], verifiedPackage: Bool, instantMilliseconds: Int64?, server: Bool
    public var id: String { name }
    init(_ raw: MeguriCoreBindings.TransferCopy) {
        name = raw.name; byteLength = raw.length; sha256 = Array(raw.sha256); verifiedPackage = raw.verifiedPackage
        instantMilliseconds = raw.instantMs; server = raw.server
    }
}
public struct ManualTransferObservation: Sendable, Equatable {
    public let modifiedMilliseconds: Int64, schemaModifiedMilliseconds: Int64
    public let revision: Int32, mediaRevision: Int32
    init(_ raw: MeguriCoreBindings.TransferObservation) {
        modifiedMilliseconds = raw.modifiedMs; schemaModifiedMilliseconds = raw.schemaModifiedMs; revision = raw.revision; mediaRevision = raw.mediaRevision
    }
}
public struct ManualTransferRecord: Sendable, Identifiable {
    public let id: String, configuredEndpoint: String, targetEndpoint: String
    public let direction: ManualTransferDirection, phase: ManualTransferPhase, acceptance: ManualTransferAcceptance
    public let boundary: ManualTransferBoundary?, local: ManualTransferCounts?, servedCounts: ManualTransferCounts?
    public let remote: ManualTransferObservation?
    public let remoteHasNoCards: Bool?, preparedMilliseconds: Int64, generation: UInt64?
    public let collectionComplete: Bool, mediaSelected: Bool, copies: [ManualTransferCopy], nextEligibleMilliseconds: Int64?
    init(_ raw: MeguriCoreBindings.TransferRecord) {
        id = raw.id; configuredEndpoint = raw.configuredEndpoint; targetEndpoint = raw.targetEndpoint
        direction = .init(raw.direction); phase = .init(raw.phase); acceptance = .init(raw.acceptance)
        boundary = raw.boundary.map(ManualTransferBoundary.init); local = raw.local.map(ManualTransferCounts.init)
        remote = raw.remote.map(ManualTransferObservation.init)
        servedCounts = raw.servedCounts.map(ManualTransferCounts.init); remoteHasNoCards = raw.remoteHasNoCards
        preparedMilliseconds = raw.preparedMs; generation = raw.generation; collectionComplete = raw.collectionComplete
        mediaSelected = raw.mediaSelected; copies = raw.copies.map(ManualTransferCopy.init); nextEligibleMilliseconds = raw.nextEligibleMs
    }
}
public extension ManualTransferRecord {
    var canInspectRecovery: Bool {
        acceptance != .possiblyAccepted && [.preview, .needsDecision, .remoteAccepted, .activating].contains(phase)
    }
}
/// Raw keys cross this boundary only for the device-only Keychain store.
/// Never use these values in diagnostics or an operation/recovery record.
public struct ManualTransferCredentials: Sendable {
    public let username: String
    public let hostKey: String
    public init(username: String, hostKey: String) { self.username = username; self.hostKey = hostKey }
}
public final class PreparedManualTransfer: Sendable {
    let raw: MeguriCoreBindings.ManualTransferPreview
    public let record: ManualTransferRecord
    init(raw: MeguriCoreBindings.ManualTransferPreview, record: ManualTransferRecord) { self.raw = raw; self.record = record }
}
public final class ManualTransferHistory: Sendable, Identifiable {
    let raw: MeguriCoreBindings.ManualTransferHistoryEntry
    public let record: ManualTransferRecord
    public var id: String { record.id }
    init(raw: MeguriCoreBindings.ManualTransferHistoryEntry, record: ManualTransferRecord) { self.raw = raw; self.record = record }
}
public enum ManualTransferCancellation: String, Sendable { case user, system }
/// Record the earliest stop request and drain its worker. Inspect the worker's
/// result to learn which effects committed; a stop request cannot undo them.
public final class ManualTransferOperation: Sendable, CollectionExportCancellationChecking {
    private struct State { var cause: ManualTransferCancellation?; var job: CoreJob?; var operationID: String? }
    private let state = Mutex(State())
    // Event seam for deterministic cancellation/drain checks. Delivery happens
    // after releasing state; it carries the same first cause as publication.
    let cancellations: AsyncStream<ManualTransferCancellation>
    private let cancellationContinuation: AsyncStream<ManualTransferCancellation>.Continuation
    public init() {
        (cancellations, cancellationContinuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    deinit { cancellationContinuation.finish() }
    public var cancellation: ManualTransferCancellation? { state.withLock { $0.cause } }
    public func cancel(_ cause: ManualTransferCancellation = .user) async {
        let (job, first) = state.withLock { state in
            let first = state.cause == nil
            if first { state.cause = cause }
            return (state.job, first)
        }
        if first { cancellationContinuation.yield(cause); cancellationContinuation.finish() }
        try? await job?.cancel()
    }
    func attach(_ job: CoreJob) async {
        let cancel = state.withLock { state in state.job = job; return state.cause != nil }
        if cancel { try? await job.cancel() }
    }
    public var operationID: String? { state.withLock { $0.operationID } }
    func identify(_ id: String) {
        state.withLock { state in
            precondition(state.operationID == nil || state.operationID == id)
            state.operationID = id
        }
    }
    func check() throws(BridgeError) { if cancellation != nil { throw .cancelled } }
}
public struct ManualTransferInterrupted: Error, Sendable { public init() {} }
