import Instrumentation
internal import MeguriCoreBindings

public enum MeguriRoundRefusal: Sendable {
    case notSelected, paused, notReady, bindingChanged, connectionChanged
    case accountChanged, publicationChanged, busy, interrupted, storage
    init(_ raw: MeguriCoreBindings.ProviderRoundRefusal) {
        self = switch raw {
        case .notSelected: .notSelected; case .paused: .paused; case .notReady: .notReady
        case .bindingChanged: .bindingChanged; case .connectionChanged: .connectionChanged
        case .accountChanged: .accountChanged; case .publicationChanged: .publicationChanged
        case .busy: .busy; case .interrupted: .interrupted; case .storage: .storage
        }
    }
}
public enum MeguriRoundRecovery: Sendable {
    case bindingChanged, publicationChanged, storage, unavailable
    init(_ raw: MeguriCoreBindings.ProviderRoundRecovery) {
        self = switch raw {
        case .bindingChanged: .bindingChanged; case .publicationChanged: .publicationChanged
        case .storage: .storage; case .unavailable: .unavailable
        }
    }
}
/// Static Core categories and scalar facts only; contains no diagnostic prose.
public struct MeguriRoundFailure: Sendable {
    public let kind: String
    public let method: String?
    public let status: UInt16?
    public let clockOffsetSeconds: Int64?
    public let databaseCheckIsLocal: Bool?
    init(_ raw: MeguriCoreBindings.ProviderRoundFailure) {
        kind = raw.kind; method = raw.method; status = raw.status
        clockOffsetSeconds = raw.clockOffsetSeconds; databaseCheckIsLocal = raw.databaseCheckIsLocal
    }
}
public enum MeguriCollectionRoundState: Sendable { case noChanges, merged, fullSyncRequired, failed }
public struct MeguriCollectionRound: Sendable {
    public let state: MeguriCollectionRoundState
    public let failure: MeguriRoundFailure?
    public let refusal: MeguriRoundRefusal?
    public let recovery: MeguriRoundRecovery?
    public let acceptanceUnknown: Bool
    public let skipped: UInt64
    public var succeeded: Bool {
        switch state {
        case .noChanges, .merged: failure == nil && refusal == nil && recovery == nil && !acceptanceUnknown
        case .fullSyncRequired, .failed: false
        }
    }
    init(_ raw: MeguriCoreBindings.ProviderCollectionRound) {
        state = switch raw.state { case .noChanges: .noChanges; case .merged: .merged
        case .fullSyncRequired: .fullSyncRequired; case .failed: .failed }
        failure = raw.failure.map(MeguriRoundFailure.init); refusal = raw.refusal.map(MeguriRoundRefusal.init)
        recovery = raw.recovery.map(MeguriRoundRecovery.init); acceptanceUnknown = raw.acceptanceUnknown; skipped = raw.skipped
    }
}
public enum MeguriAffectedMediaKind: Sendable { case empty, overLimit, invalidName, notRegularFile, pending, awaiting }
/// Explicit local data for a future affected-files presentation; never logged.
public struct MeguriAffectedMedia: Sendable {
    public let name: String
    public let kind: MeguriAffectedMediaKind
    init(_ raw: MeguriCoreBindings.ProviderAffectedMedia) {
        name = raw.name
        kind = switch raw.kind { case .empty: .empty; case .overLimit: .overLimit
        case .invalidName: .invalidName; case .notRegularFile: .notRegularFile
        case .pending: .pending; case .awaiting: .awaiting }
    }
}
public struct MeguriMediaRound: Sendable {
    public let complete: Bool
    public let failure: MeguriRoundFailure?
    public let refusal: MeguriRoundRefusal?
    public let recovery: MeguriRoundRecovery?
    public let uploadedFiles: UInt64, downloadedFiles: UInt64
    public let uploadedDeletions: UInt64, downloadedDeletions: UInt64
    public let bytesUploaded: UInt64, bytesDownloaded: UInt64, checked: UInt64
    public let resynced: Bool
    /// Nil means fresh promised-inventory accounting failed or never ran.
    public let excluded: UInt64?, pending: UInt64?, awaiting: UInt64?
    public let affected: [MeguriAffectedMedia]
    public let accountingFailure: MeguriRoundFailure?
    init(_ raw: MeguriCoreBindings.ProviderMediaRound) {
        complete = raw.complete; failure = raw.failure.map(MeguriRoundFailure.init)
        refusal = raw.refusal.map(MeguriRoundRefusal.init); recovery = raw.recovery.map(MeguriRoundRecovery.init)
        uploadedFiles = raw.uploadedFiles; downloadedFiles = raw.downloadedFiles
        uploadedDeletions = raw.uploadedDeletions; downloadedDeletions = raw.downloadedDeletions
        bytesUploaded = raw.bytesUploaded; bytesDownloaded = raw.bytesDownloaded; checked = raw.checked; resynced = raw.resynced
        excluded = raw.excluded; pending = raw.pending; awaiting = raw.awaiting
        affected = raw.affected.map(MeguriAffectedMedia.init); accountingFailure = raw.accountingFailure.map(MeguriRoundFailure.init)
    }
}
public enum MeguriRoundPhaseOutcome: Sendable {
    case pending, refused(MeguriRoundRefusal), collection(MeguriCollectionRound), media(MeguriMediaRound)
    init(_ raw: MeguriCoreBindings.ProviderRoundOutcome) {
        self = switch raw { case .pending: .pending
        case .collection(let result): .collection(MeguriCollectionRound(result))
        case .media(let result): .media(MeguriMediaRound(result)) }
    }
}
/// Retains the original task's terminal result for a local metadata retry.
public final class MeguriRoundFinalization: Sendable {
    private let raw: MeguriCoreBindings.ProviderRoundResult
    init(_ raw: MeguriCoreBindings.ProviderRoundResult) { self.raw = raw }
    public func retry() async throws -> MeguriRoundPhaseOutcome {
        let raw = raw
        return try await MaintenanceLane.shared.call(.providerRoundRetryFinalization) {
            MeguriRoundPhaseOutcome(try raw.retryFinalization())
        }
    }
}
public struct MeguriRoundPhaseResult: Sendable {
    public let outcome: MeguriRoundPhaseOutcome
    public let finalization: MeguriRoundFinalization?
    public let trackingFailure: BridgeError?
}
public struct MeguriRoundReport: Sendable {
    public let collection: MeguriRoundPhaseResult
    public let media: MeguriRoundPhaseResult?
    public var complete: Bool {
        guard case .collection(let collection) = collection.outcome, collection.succeeded,
              let media, case .media(let media) = media.outcome else { return false }
        return media.complete
    }
}
/// Direct caller-owned collection then media. No wake source or UI state lives
/// here. Cancellation joins the same native jobs before returning their results.
public actor CoreMeguriRoundService {
    private let connection: MeguriCoreBindings.ProviderConnection
    private var attempt: (id: UInt64, task: Task<MeguriRoundReport, any Error>)?
    private var nextID: UInt64 = 0
    private init(_ connection: MeguriCoreBindings.ProviderConnection) { self.connection = connection }
    public static func attached() async throws -> CoreMeguriRoundService {
        let owner = try await MaintenanceLane.shared.call(.providerOwner) { try providerOwner() }
        let connection = try await MaintenanceLane.shared.call(.providerConfiguredMeguri) { try owner.configuredMeguri() }
        guard let connection else { throw BridgeError.unavailable(reason: "Meguri connection requires setup") }
        return CoreMeguriRoundService(connection)
    }
    public func fullRound(credentials: MeguriProviderCredentials) async throws -> MeguriRoundReport {
        guard attempt == nil else { throw BridgeError.unavailable(reason: "Meguri round is busy") }
        nextID &+= 1
        let id = nextID, connection = connection
        let raw = MeguriCoreBindings.ProviderCredentials(connection: .init(reference: credentials.connection.reference,
            revision: credentials.connection.revision, endpoint: credentials.connection.endpoint),
            username: credentials.username, hostkey: credentials.hostKey)
        let task = Task { try await Self.perform(connection, credentials: raw) }
        attempt = (id, task)
        let result = await withTaskCancellationHandler { await task.result } onCancel: { task.cancel() }
        if attempt?.id == id { attempt = nil }
        return try result.get()
    }
    public func cancelAndDrain() async {
        let original = attempt
        original?.task.cancel()
        _ = await original?.task.result
        if let original, attempt?.id == original.id { attempt = nil }
    }
    private static func perform(_ connection: MeguriCoreBindings.ProviderConnection,
        credentials: MeguriCoreBindings.ProviderCredentials) async throws -> MeguriRoundReport {
        try await CoreEventStream.shared.start()
        try Task.checkCancellation()
        let collection = try await phase {
            try await MaintenanceLane.shared.call(.providerStartCollectionRound) { try connection.startCollectionRound(credentials: credentials) }
        }
        guard case .collection(let value) = collection.outcome, value.succeeded, !Task.isCancelled else {
            return MeguriRoundReport(collection: collection, media: nil)
        }
        let media = try await phase {
            try await MaintenanceLane.shared.call(.providerStartMediaRound) { try connection.startMediaRound(credentials: credentials) }
        }
        return MeguriRoundReport(collection: collection, media: media)
    }
    private static func phase(_ start: () async throws -> MeguriCoreBindings.ProviderRoundStart) async throws -> MeguriRoundPhaseResult {
        let started = try await start()
        switch started {
        case .refused(let refusal):
            return .init(outcome: .refused(MeguriRoundRefusal(refusal)), finalization: nil, trackingFailure: nil)
        case .started(let handle, let result):
            var trackingFailure: BridgeError?
            switch await Jobs.trackRetainingDrain(handle) {
            case .tracked(let job):
                _ = await withTaskCancellationHandler { await job.outcome() } onCancel: {
                    Task { try? await job.cancel() }
                }
            case .drained(let error, _): trackingFailure = error
            }
            // Real terminal results outrank a Swift cancellation and retain
            // partial effects even when listener setup itself failed.
            let outcome = try await MaintenanceLane.shared.call(.providerRoundOutcome) { try result.outcome() }
            return .init(outcome: MeguriRoundPhaseOutcome(outcome), finalization: MeguriRoundFinalization(result), trackingFailure: trackingFailure)
        }
    }
}
