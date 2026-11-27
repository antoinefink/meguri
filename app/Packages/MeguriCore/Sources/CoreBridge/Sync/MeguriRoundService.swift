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
    public init(outcome: MeguriRoundPhaseOutcome, finalization: MeguriRoundFinalization?, trackingFailure: BridgeError?) {
        self.outcome = outcome; self.finalization = finalization; self.trackingFailure = trackingFailure
    }
}
public struct MeguriRoundReport: Sendable {
    public let collection: MeguriRoundPhaseResult
    public let media: MeguriRoundPhaseResult?
    /// A structure save job's stopped round has a collection phase only (MP7 step 2).
    public init(collection: MeguriRoundPhaseResult, media: MeguriRoundPhaseResult?) {
        self.collection = collection; self.media = media
    }
    public var complete: Bool {
        guard case .collection(let collection) = collection.outcome, collection.succeeded,
              let media, case .media(let media) = media.outcome else { return false }
        return media.complete
    }
}
public enum MeguriRoundAdmissionFailure: Error, Sendable {
    case connectionUnavailable, retired, busy, credentialsMissing
}
/// Direct caller-owned collection then media. No wake source or UI state lives
/// here. Cancellation joins the same native jobs before returning their results.
public actor CoreMeguriRoundService {
    private let raw: MeguriCoreBindings.ProviderConnection
    public let connection: MeguriProviderConnection
    private let credentials: @Sendable () async throws -> MeguriProviderCredentials
    private var retired = false
    private var attempt: (id: UInt64, task: Task<MeguriRoundReport, any Error>)?
    private var nextID: UInt64 = 0
    /// A structure save job that runs on this facade; retirement joins it.
    /// The running structure job (a note type save or a deck package import), joined by a drain.
    var structureWork: Task<Void, Never>?
    /// Raised by every drain: a structure job admitted before it does not start after it.
    var drains: UInt64 = 0
    init(raw: MeguriCoreBindings.ProviderConnection, connection: MeguriProviderConnection,
         credentials: @escaping @Sendable () async throws -> MeguriProviderCredentials) {
        self.raw = raw; self.connection = connection; self.credentials = credentials
    }
    private func current() throws {
        guard !retired else { throw MeguriRoundAdmissionFailure.retired }
        try Task.checkCancellation()
    }
    public typealias Observation = @Sendable (CoreProgress) async -> Void
    public func fullRound(observe: @escaping Observation = { _ in }) async throws -> MeguriRoundReport {
        guard !retired else { throw MeguriRoundAdmissionFailure.retired }
        guard attempt == nil else { throw MeguriRoundAdmissionFailure.busy }
        nextID &+= 1
        let id = nextID
        // The original task owns the secure read as well as both native phases.
        // Reserve its slot before the task can enter the credential closure.
        let task = Task {
            try current()
            let value = try await self.credentials()
            try current()
            guard value.connection == connection else { throw MeguriProviderCredentialFailure.wrongConnection }
            let credentials = MeguriCoreBindings.ProviderCredentials(connection: .init(reference: connection.reference,
                revision: connection.revision, endpoint: connection.endpoint), username: value.username, hostkey: value.hostKey)
            return try await Self.perform(raw, credentials: credentials, observe: observe)
        }
        attempt = (id, task)
        let result = await withTaskCancellationHandler {
            if Task.isCancelled { task.cancel() }
            return await task.result
        } onCancel: { task.cancel() }
        if attempt?.id == id { attempt = nil }
        return try result.get()
    }
    /// The retained connection with the credentials read for it now, for a
    /// job that this facade does not own (the structure save job).
    func admittedInputs() async throws -> (MeguriCoreBindings.ProviderConnection, MeguriCoreBindings.ProviderCredentials) {
        try current()
        let value = try await credentials()
        try current()
        guard value.connection == connection else { throw MeguriProviderCredentialFailure.wrongConnection }
        return (raw, MeguriCoreBindings.ProviderCredentials(connection: .init(reference: connection.reference,
            revision: connection.revision, endpoint: connection.endpoint), username: value.username, hostkey: value.hostKey))
    }
    public func cancelAndDrain() async {
        // A structure save job is asked to stop where MP7 allows, and joined.
        drains &+= 1
        let structure = structureWork
        structure?.cancel()
        _ = await structure?.result
        let original = attempt
        original?.task.cancel()
        _ = await original?.task.result
        if let original, attempt?.id == original.id { attempt = nil }
    }
    /// Permanent revocation; ordinary cancellation never reopens this facade.
    /// Joining the original task includes an uncancellable held secure read.
    func retireAndDrain() async {
        retired = true
        await cancelAndDrain()
    }
    private static func perform(_ connection: MeguriCoreBindings.ProviderConnection,
        credentials: MeguriCoreBindings.ProviderCredentials, observe: @escaping Observation) async throws -> MeguriRoundReport {
        try await CoreEventStream.shared.start()
        try Task.checkCancellation()
        let collection = try await phase(observe: observe) {
            try await MaintenanceLane.shared.call(.providerStartCollectionRound) { try connection.startCollectionRound(credentials: credentials) }
        }
        guard case .collection(let value) = collection.outcome, value.succeeded, !Task.isCancelled else {
            return MeguriRoundReport(collection: collection, media: nil)
        }
        let media = try await phase(observe: observe) {
            try await MaintenanceLane.shared.call(.providerStartMediaRound) { try connection.startMediaRound(credentials: credentials) }
        }
        return MeguriRoundReport(collection: collection, media: media)
    }
    private static func phase(observe: @escaping Observation, _ start: () async throws -> MeguriCoreBindings.ProviderRoundStart) async throws -> MeguriRoundPhaseResult {
        let started = try await start()
        switch started {
        case .refused(let refusal):
            return .init(outcome: .refused(MeguriRoundRefusal(refusal)), finalization: nil, trackingFailure: nil)
        case .started(let handle, let result):
            var trackingFailure: BridgeError?
            let terminal: JobOutcome
            switch await Jobs.trackRetainingDrain(handle) {
            case .tracked(let job):
                // The stream may conflate ticks, but each update retains all
                // observed native milestones, including before listener setup.
                // Keep consuming until the cancelled native job drains too.
                let observations = Task.detached {
                    for await progress in job.progress {
                        await observe(progress)
                    }
                }
                terminal = await withTaskCancellationHandler { await job.outcome() } onCancel: {
                    Task { try? await job.cancel() }
                }
                await observations.value
            case .drained(let error, let outcome): trackingFailure = error; terminal = outcome
            }
            // Real terminal results outrank a Swift cancellation and retain
            // partial effects even when listener setup itself failed.
            let outcome = try await MaintenanceLane.shared.call(.providerRoundOutcome) { try result.outcome() }
            if case .pending = outcome {
                switch terminal {
                case .failed(let error): throw error
                case .cancelled: throw BridgeError.cancelled
                default: throw BridgeError.internal("Meguri round worker returned no terminal result")
                }
            }
            return .init(outcome: MeguriRoundPhaseOutcome(outcome), finalization: MeguriRoundFinalization(result), trackingFailure: trackingFailure)
        }
    }
}
