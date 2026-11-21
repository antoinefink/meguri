import Instrumentation
import Synchronization
internal import MeguriCoreBindings

public enum MeguriBootstrapDirection: Sendable { case useServerCollection, useDeviceCollection }
public enum MeguriBootstrapAdmissionFailure: Error, Sendable, Equatable {
    case busy, retired, connectionUnavailable, credentialsMissing
}
public enum MeguriBootstrapFailure: Sendable {
    case notSelected, connectionChanged, busy, alreadyBound, username, noIntent, operationMismatch
    case notComplete, accountChanged, publicationChanged, documentChanged, bindingChanged
    case mediaEpochChanged, mediaEpochMissing, mediaIncomplete, cancelled, diskFull, unavailable, storage
    case transfer(ManualTransferFailure)
    /// MP6.3: the facts of a setup decision changed, or the choice does not fit them.
    case setupLocalChanged, setupRemoteChanged, setupChoice
    /// A collection file is on this device but is closed or damaged.
    case setupLocalUnusable
    init(_ raw: MeguriCoreBindings.ProviderBootstrapFailure) {
        self = switch raw {
        case .notSelected: .notSelected
        case .connectionChanged: .connectionChanged
        case .busy: .busy
        case .alreadyBound: .alreadyBound
        case .username: .username
        case .noIntent: .noIntent
        case .operationMismatch: .operationMismatch
        case .notComplete: .notComplete
        case .accountChanged: .accountChanged
        case .publicationChanged: .publicationChanged
        case .documentChanged: .documentChanged
        case .bindingChanged: .bindingChanged
        case .mediaEpochChanged: .mediaEpochChanged
        case .mediaEpochMissing: .mediaEpochMissing
        case .mediaIncomplete: .mediaIncomplete
        case .cancelled: .cancelled
        case .diskFull: .diskFull
        case .unavailable: .unavailable
        case .storage: .storage
        case .transfer(let failure): .transfer(.init(failure))
        case .setupLocalChanged: .setupLocalChanged
        case .setupRemoteChanged: .setupRemoteChanged
        case .setupChoice: .setupChoice
        case .setupLocalUnusable: .setupLocalUnusable
        }
    }
}
public enum MeguriBootstrapMediaState: Sendable, Equatable { case transferred, pending, excluded }
public enum MeguriBootstrapMediaExclusion: Sendable, Equatable { case invalidName, notRegularFile, empty, overLimit }
/// Filenames are display data, never diagnostics or authority.
public struct MeguriBootstrapMediaFile: Sendable {
    public let name: String, lossyName: Bool
    public let state: MeguriBootstrapMediaState
    public let exclusion: MeguriBootstrapMediaExclusion?
    init(_ raw: MeguriCoreBindings.ProviderMediaFile) {
        name = raw.name; lossyName = raw.lossyName
        state = switch raw.state { case .transferred: .transferred; case .pending: .pending; case .excluded: .excluded }
        exclusion = raw.exclusion.map { switch $0 {
        case .invalidName: .invalidName; case .notRegularFile: .notRegularFile; case .empty: .empty; case .overLimit: .overLimit
        } }
    }
}
public struct MeguriBootstrapMediaAccounting: Sendable {
    public let files: [MeguriBootstrapMediaFile]
    public let remotePending: UInt64, transferred: UInt64
    init(_ raw: MeguriCoreBindings.ProviderMediaAccounting) {
        files = raw.files.map(MeguriBootstrapMediaFile.init); remotePending = raw.remotePending; transferred = raw.transferred
    }
}
public struct MeguriBootstrapMediaEffects: Sendable {
    public let stage: ManualMediaStage
    public let uploadedFiles: UInt64, downloadedFiles: UInt64, uploadedDeletions: UInt64, downloadedDeletions: UInt64
    public let oversized: [String], empty: [String], awaiting: UInt64, remoteOutcomeUnknown: Bool
    init(stage: ManualMediaStage) {
        self.stage = stage; uploadedFiles = 0; downloadedFiles = 0; uploadedDeletions = 0; downloadedDeletions = 0
        oversized = []; empty = []; awaiting = 0; remoteOutcomeUnknown = false
    }
    init(_ raw: MeguriCoreBindings.ProviderBootstrapMediaEffects) {
        stage = .init(raw.stage); uploadedFiles = raw.uploadedFiles; downloadedFiles = raw.downloadedFiles
        uploadedDeletions = raw.uploadedDeletions; downloadedDeletions = raw.downloadedDeletions
        oversized = raw.oversized; empty = raw.empty; awaiting = raw.awaiting; remoteOutcomeUnknown = raw.remoteOutcomeUnknown
    }
}
/// A limited receipt. It intentionally carries no reconstructible operation identity.
public struct MeguriBootstrapReceipt: Sendable {
    public let direction: ManualTransferDirection, phase: ManualTransferPhase, acceptance: ManualTransferAcceptance
    public let boundary: ManualTransferBoundary?, local: ManualTransferCounts?, remote: ManualTransferObservation?
    public let remoteHasNoCards: Bool?, servedCounts: ManualTransferCounts?, copies: [ManualTransferCopy]
    public let media: MeguriBootstrapMediaEffects
    /// A receipt with no core behind it, for tests of the callers of a bridge.
    init(direction: ManualTransferDirection, phase: ManualTransferPhase, acceptance: ManualTransferAcceptance,
         boundary: ManualTransferBoundary? = nil) {
        self.direction = direction; self.phase = phase; self.acceptance = acceptance; self.boundary = boundary
        local = nil; remote = nil; remoteHasNoCards = nil; servedCounts = nil; copies = []
        media = .init(stage: .awaitingConsent)
    }
    init(_ raw: MeguriCoreBindings.ProviderBootstrapRecord) {
        direction = .init(raw.direction); phase = .init(raw.phase); acceptance = .init(raw.acceptance)
        boundary = raw.boundary.map(ManualTransferBoundary.init); local = raw.local.map(ManualTransferCounts.init)
        remote = raw.remote.map(ManualTransferObservation.init); remoteHasNoCards = raw.remoteHasNoCards
        servedCounts = raw.servedCounts.map(ManualTransferCounts.init); copies = raw.copies.map(ManualTransferCopy.init)
        media = .init(raw.media)
    }
}
public struct MeguriBootstrapMediaProgress: Sendable {
    public let phase: ManualMediaPhase
    public let checked: UInt64, uploadedFiles: UInt64, downloadedFiles: UInt64
    public let bytesUploaded: UInt64, bytesDownloaded: UInt64, awaiting: UInt64
    init(_ raw: MeguriCoreBindings.ProviderBootstrapMediaProgress) {
        phase = .init(raw.phase); checked = raw.checked; uploadedFiles = raw.uploadedFiles; downloadedFiles = raw.downloadedFiles
        bytesUploaded = raw.bytesUploaded; bytesDownloaded = raw.bytesDownloaded; awaiting = raw.awaiting
    }
    private init(downloadedFiles: UInt64, awaiting: UInt64) {
        phase = .downloading; checked = downloadedFiles &+ awaiting; uploadedFiles = 0
        self.downloadedFiles = downloadedFiles; bytesUploaded = 0; bytesDownloaded = 0; self.awaiting = awaiting
    }
    /// Rendering fixtures only (`SyncCoordinator.renderFixture`): no transfer stands behind it.
    public static func renderFixture(downloadedFiles: UInt64, awaiting: UInt64) -> MeguriBootstrapMediaProgress {
        .init(downloadedFiles: downloadedFiles, awaiting: awaiting)
    }
}
public final class MeguriBootstrapPreview: Sendable {
    private let handle: MeguriCoreBindings.ProviderBootstrapPreview?
    public let receipt: MeguriBootstrapReceipt
    fileprivate init(_ raw: MeguriCoreBindings.ProviderBootstrapPreview, _ receipt: MeguriBootstrapReceipt) { handle = raw; self.receipt = receipt }
    /// A handle with no core behind it, for tests of the callers of a bridge. The service refuses it.
    init(detached receipt: MeguriBootstrapReceipt) { handle = nil; self.receipt = receipt }
    fileprivate var raw: MeguriCoreBindings.ProviderBootstrapPreview {
        get throws {
            guard let handle else { throw BridgeError.internal("A detached Meguri bootstrap handle has no core") }
            return handle
        }
    }
}
public final class MeguriBootstrapMediaConsent: Sendable {
    fileprivate let raw: MeguriCoreBindings.ProviderBootstrapMediaConsent
    public let receipt: MeguriBootstrapReceipt
    fileprivate init(_ raw: MeguriCoreBindings.ProviderBootstrapMediaConsent, _ receipt: MeguriBootstrapReceipt) { self.raw = raw; self.receipt = receipt }
}
public final class MeguriBootstrapOperation: Sendable {
    private let handle: MeguriCoreBindings.ProviderBootstrapOperation?
    public let receipt: MeguriBootstrapReceipt
    fileprivate init(_ raw: MeguriCoreBindings.ProviderBootstrapOperation, _ receipt: MeguriBootstrapReceipt) { handle = raw; self.receipt = receipt }
    /// A handle with no core behind it, for tests of the callers of a bridge. The service refuses it.
    init(detached receipt: MeguriBootstrapReceipt) { handle = nil; self.receipt = receipt }
    fileprivate var raw: MeguriCoreBindings.ProviderBootstrapOperation {
        get throws {
            guard let handle else { throw BridgeError.internal("A detached Meguri bootstrap handle has no core") }
            return handle
        }
    }
}
/// MP6.3's table row for one finished preparation.
public enum MeguriSetupRow: Sendable, Equatable {
    case download, downloadOverEmptyDevice, uploadToEmptyServer, ask, askUploadOnly
    /// No collection on this device, but media files: download or Cancel.
    case askDownloadOnly
}
public enum MeguriSetupChoice: Sendable, Equatable { case noQuestion, keepDevice, keepServer }
/// One side's counts. A nil value is a fact the core does not have.
public struct MeguriSetupSide: Sendable, Equatable {
    public let notes: UInt64, cards: UInt64, reviews: UInt64
    public let mediaFiles: UInt64?, lastChangeMs: Int64?
    public init(notes: UInt64, cards: UInt64, reviews: UInt64, mediaFiles: UInt64?, lastChangeMs: Int64?) {
        self.notes = notes; self.cards = cards; self.reviews = reviews; self.mediaFiles = mediaFiles; self.lastChangeMs = lastChangeMs
    }
    init(_ raw: MeguriCoreBindings.ProviderSetupSide) {
        self.init(notes: raw.notes, cards: raw.cards, reviews: raw.reviews, mediaFiles: raw.mediaFiles, lastChangeMs: raw.lastChangeMs)
    }
}
/// The display half of a decision; it carries no authority.
public struct MeguriSetupFacts: Sendable, Equatable {
    public let row: MeguriSetupRow
    public let device: MeguriSetupSide?, server: MeguriSetupSide?
    public let unsent: Bool
    public init(row: MeguriSetupRow, device: MeguriSetupSide?, server: MeguriSetupSide?, unsent: Bool) {
        self.row = row; self.device = device; self.server = server; self.unsent = unsent
    }
}
/// The core's decision for one preview. Only the core that made it can use it.
public final class MeguriSetupDecision: Sendable {
    private let handle: MeguriCoreBindings.ProviderSetupDecision?
    public let facts: MeguriSetupFacts
    /// The account that was signed in to when the core decided, as the user typed it.
    public let account: String
    /// A decision with no core behind it, for tests of the callers of a bridge. The service refuses it.
    init(detached facts: MeguriSetupFacts, account: String = "") { handle = nil; self.facts = facts; self.account = account }
    fileprivate var raw: MeguriCoreBindings.ProviderSetupDecision {
        get throws {
            guard let handle else { throw BridgeError.internal("A detached Meguri setup decision has no core") }
            return handle
        }
    }
    fileprivate init(_ raw: MeguriCoreBindings.ProviderSetupDecision, _ facts: MeguriCoreBindings.ProviderSetupFacts,
                     account: String) {
        handle = raw; self.account = account
        let row: MeguriSetupRow = switch facts.row {
        case .download: .download
        case .downloadOverEmptyDevice: .downloadOverEmptyDevice
        case .uploadToEmptyServer: .uploadToEmptyServer
        case .ask: .ask
        case .askUploadOnly: .askUploadOnly
        case .askDownloadOnly: .askDownloadOnly
        }
        self.facts = .init(row: row, device: facts.device.map(MeguriSetupSide.init), server: facts.server.map(MeguriSetupSide.init),
                           unsent: facts.unsent)
    }
}
public enum MeguriSetupDecisionOutcome: Sendable {
    case decision(MeguriSetupDecision), refused(MeguriBootstrapFailure)
}
/// The outcomes of MP6.9's settling step. An unreadable server is an error, not a case here.
public enum MeguriSetupSettlement: Sendable {
    case accepted(MeguriBootstrapOperation, localUnchanged: Bool), notAccepted(MeguriBootstrapOperation)
    /// The upload is recorded as overtaken; the next preparation asks which collection to keep.
    case neither(MeguriBootstrapOperation)
}
public enum MeguriBootstrapOutcome: Sendable {
    case preview(MeguriBootstrapPreview), mediaConsent(MeguriBootstrapMediaConsent), operation(MeguriBootstrapOperation)
    case ready(MeguriBootstrapMediaAccounting)
    case settled(MeguriSetupSettlement)
    case refused(MeguriBootstrapFailure, accounting: MeguriBootstrapMediaAccounting?)
}
public enum MeguriBootstrapReadiness: Sendable {
    case notSelected, unbound(operation: MeguriBootstrapOperation?), ready, stale
}
public struct MeguriBootstrapCommandResult: Sendable {
    public let outcome: MeguriBootstrapOutcome
    /// Nil means no worker was created; the typed outcome determines success or refusal.
    public let jobID: JobID?
    public let mediaProgress: MeguriBootstrapMediaProgress?
    public let progressFailure: BridgeError?
    /// Preparation retry authority only; never a completion assertion.
    public let retryOperation: MeguriBootstrapOperation?
}

/// First-cause stop ownership, including stops before the real result/token attach.
/// No policy operation ID or legacy manual session participates in this work.
private final class BootstrapStop: Sendable {
    private struct State {
        var cause: ManualTransferCancellation?
        var result: MeguriCoreBindings.ProviderBootstrapResult?
        var token: CancelToken?
        var delivery: Task<Void, Never>?
        var finished = false
    }
    private let state = Mutex(State())
    func check() throws {
        if state.withLock({ $0.cause != nil }) { throw BridgeError.cancelled }
    }
    private func delivery(_ state: inout State) -> Task<Void, Never>? {
        guard !state.finished, let cause = state.cause, let result = state.result else { return state.delivery }
        if let delivery = state.delivery { return delivery }
        let token = state.token
        let task = Task {
            try? await ControlLane.shared.call(.providerBootstrapRequestStop) { try result.requestStop(interrupted: cause == .system) }
            try? await token?.cancel()
        }
        state.delivery = task
        return task
    }
    func cancel(_ cause: ManualTransferCancellation) async {
        let task = state.withLock { state in
            if state.cause == nil { state.cause = cause }
            return delivery(&state)
        }
        await task?.value
    }
    func attach(_ result: MeguriCoreBindings.ProviderBootstrapResult, _ token: CancelToken?) async {
        let task = state.withLock { state in
            state.result = result; state.token = token
            return delivery(&state)
        }
        await task?.value
    }
    func finish() async {
        let task = state.withLock { state in state.finished = true; return state.delivery }
        await task?.value
    }
}

/// Pins one real configured connection. Every transfer phase and completion is
/// a separate explicit command; construction and readiness do no transfer.
public actor MeguriProviderBootstrapService {
    public nonisolated let connection: MeguriProviderConnection
    private let raw: MeguriCoreBindings.ProviderConnection
    private let credentials: @Sendable () async throws -> MeguriProviderCredentials
    private var retired = false
    private enum Value: Sendable {
        case command(MeguriBootstrapCommandResult), readiness(MeguriBootstrapReadiness), pending(MeguriBootstrapOperation?)
    }
    private enum WorkerStart: Sendable {
        case credentials(@Sendable (MeguriCoreBindings.ProviderConnection, MeguriCoreBindings.ProviderCredentials) throws -> MeguriCoreBindings.ProviderBootstrapJob)
        case local(@Sendable (MeguriCoreBindings.ProviderConnection) throws -> MeguriCoreBindings.ProviderBootstrapJob)
    }
    private struct Active {
        let identity: UInt64
        let stop: BootstrapStop
        let task: Task<Value, any Error>
    }
    private var active: Active?
    private var sequence: UInt64 = 0
    init(raw: MeguriCoreBindings.ProviderConnection, connection: MeguriProviderConnection,
         credentials: @escaping @Sendable () async throws -> MeguriProviderCredentials) {
        self.raw = raw; self.connection = connection; self.credentials = credentials
    }
    private func admit(_ body: @escaping @Sendable (BootstrapStop) async throws -> Value) async throws -> Value {
        guard !retired else { throw MeguriBootstrapAdmissionFailure.retired }
        guard active == nil else { throw MeguriBootstrapAdmissionFailure.busy }
        sequence &+= 1
        let identity = sequence, stop = BootstrapStop()
        // Reserve before the task can suspend in the credential reader.
        let task = Task {
            do {
                let value = try await body(stop)
                await stop.finish()
                return value
            } catch {
                await stop.finish()
                throw error
            }
        }
        active = Active(identity: identity, stop: stop, task: task)
        defer { if active?.identity == identity { active = nil } }
        return try await withTaskCancellationHandler {
            if Task.isCancelled { await stop.cancel(.system) }
            return try await task.value
        } onCancel: { Task { await stop.cancel(.system) } }
    }
    public func stop(_ cause: ManualTransferCancellation = .user) async { await active?.stop.cancel(cause) }
    /// Retirement is permanent. Keep the task/result slot until all terminal
    /// receipt reads and stop delivery join, including a held credential read.
    func retireAndDrain() async {
        retired = true
        let original = active
        await original?.stop.cancel(.system)
        _ = await original?.task.result
        if let original, active?.identity == original.identity { active = nil }
    }
    private func current(_ stop: BootstrapStop) throws {
        guard !retired else { throw MeguriBootstrapAdmissionFailure.retired }
        try stop.check()
    }
    private func credentialValue(_ stop: BootstrapStop) async throws -> MeguriCoreBindings.ProviderCredentials {
        try current(stop)
        let value = try await credentials()
        try current(stop)
        guard value.connection == connection else { throw MeguriProviderCredentialFailure.wrongConnection }
        return .init(connection: .init(reference: connection.reference, revision: connection.revision, endpoint: connection.endpoint),
                     username: value.username, hostkey: value.hostKey)
    }
    private func operation(_ raw: MeguriCoreBindings.ProviderBootstrapOperation) async throws -> MeguriBootstrapOperation {
        let receipt = try await MaintenanceLane.shared.call(.providerBootstrapOperationRecord) { try raw.record() }
        return .init(raw, .init(receipt))
    }
    // Pure display mapping is also used by focused losslessness tests. It
    // accepts value records only and cannot manufacture opaque authority.
    static func displayOutcome(_ raw: MeguriCoreBindings.ProviderBootstrapOutcome) -> MeguriBootstrapOutcome? {
        switch raw {
        case .ready(let accounting): return .ready(.init(accounting))
        case .refused(let failure, let accounting): return .refused(.init(failure), accounting: accounting.map(MeguriBootstrapMediaAccounting.init))
        default: return nil
        }
    }
    private func outcome(_ raw: MeguriCoreBindings.ProviderBootstrapOutcome) async throws -> MeguriBootstrapOutcome {
        if let display = Self.displayOutcome(raw) { return display }
        switch raw {
        case .preview(let preview):
            let record = try await MaintenanceLane.shared.call(.providerBootstrapPreviewRecord) { try preview.record() }
            return .preview(.init(preview, .init(record)))
        case .mediaConsent(let consent):
            let record = try await MaintenanceLane.shared.call(.providerBootstrapMediaConsentRecord) { try consent.record() }
            return .mediaConsent(.init(consent, .init(record)))
        case .operation(let raw): return .operation(try await operation(raw))
        case .settled(let settlement):
            switch settlement {
            case .accepted(let raw, let localUnchanged): return .settled(.accepted(try await operation(raw), localUnchanged: localUnchanged))
            case .notAccepted(let raw): return .settled(.notAccepted(try await operation(raw)))
            case .neither(let raw): return .settled(.neither(try await operation(raw)))
            }
        case .ready, .refused, .pending: throw BridgeError.internal("Meguri bootstrap has no terminal result")
        }
    }
    public func readiness() async throws -> MeguriBootstrapReadiness {
        let value = try await admit { stop in
            try await self.current(stop)
            let raw = self.raw
            let result = try await MaintenanceLane.shared.call(.providerBootstrapReadiness) {
                try stop.check(); return try raw.bootstrapReadiness()
            }
            let readiness: MeguriBootstrapReadiness
            switch result {
            case .notSelected: readiness = .notSelected
            case .ready: readiness = .ready
            case .stale: readiness = .stale
            case .unbound(let raw):
                if let raw { readiness = .unbound(operation: try await self.operation(raw)) }
                else { readiness = .unbound(operation: nil) }
            }
            return .readiness(readiness)
        }
        guard case .readiness(let readiness) = value else { throw BridgeError.internal("Unexpected bootstrap command") }
        return readiness
    }
    public func pendingPreparation() async throws -> MeguriBootstrapOperation? {
        let value = try await admit { stop in
            let credentials = try await self.credentialValue(stop)
            let raw = self.raw
            let pending = try await MaintenanceLane.shared.call(.providerBootstrapPendingPreparation) {
                try stop.check(); return try raw.pendingBootstrapPreparation(username: credentials.username)
            }
            if let pending { return .pending(try await self.operation(pending)) }
            return .pending(nil)
        }
        guard case .pending(let operation) = value else { throw BridgeError.internal("Unexpected bootstrap command") }
        return operation
    }
    private func localCommand(_ call: BridgeCall,
                              perform: @escaping @Sendable (MeguriCoreBindings.ProviderConnection) throws -> MeguriCoreBindings.ProviderBootstrapOutcome) async throws -> MeguriBootstrapCommandResult {
        let value = try await admit { stop in
            try await self.current(stop)
            let raw = self.raw
            let result = try await MaintenanceLane.shared.call(call) {
                try stop.check(); return try perform(raw)
            }
            // A stop after Rust entered cannot erase cancellation or refusal.
            return .command(.init(outcome: try await self.outcome(result), jobID: nil,
                                  mediaProgress: nil, progressFailure: nil, retryOperation: nil))
        }
        guard case .command(let result) = value else { throw BridgeError.internal("Unexpected bootstrap command") }
        return result
    }
    public func discard(_ preview: MeguriBootstrapPreview) async throws -> MeguriBootstrapCommandResult {
        try await localCommand(.providerBootstrapDiscardPreview) { try $0.discardBootstrapPreview(preview: preview.raw) }
    }
    public func discard(_ operation: MeguriBootstrapOperation) async throws -> MeguriBootstrapCommandResult {
        try await localCommand(.providerBootstrapDiscardOperation) { try $0.discardBootstrapOperation(operation: operation.raw) }
    }
    public func prepareRecovery(_ operation: MeguriBootstrapOperation,
                                progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        try await worker(.providerBootstrapRecovery, progress: progress, mediaProgress: { _ in },
                         start: .local { try $0.startBootstrapRecovery(operation: operation.raw) })
    }
    private func worker(_ call: BridgeCall, preparation: Bool = false,
                        progress: @escaping @Sendable (CoreProgress) -> Void,
                        mediaProgress: @escaping @Sendable (MeguriBootstrapMediaProgress) -> Void,
                        start: WorkerStart) async throws -> MeguriBootstrapCommandResult {
        let value = try await admit { stop in
            let begin: @Sendable (MeguriCoreBindings.ProviderConnection) throws -> MeguriCoreBindings.ProviderBootstrapJob
            switch start {
            case .credentials(let start):
                let credentials = try await self.credentialValue(stop)
                begin = { try start($0, credentials) }
            case .local(let start): begin = start
            }
            try await CoreEventStream.shared.start()
            try await self.current(stop)
            let raw = self.raw
            let started = try await MaintenanceLane.shared.call(call) {
                try stop.check(); return try begin(raw)
            }
            await stop.attach(started.result, started.handle.map { CancelToken(raw: $0.cancel) })
            var terminal: JobOutcome?
            if let handle = started.handle {
                let tracking: DrainedJobTracking
                do throws(BridgeError) {
                    tracking = .tracked(try await MaintenanceLane.shared.call(.jobProgressSetListener) { try Jobs.track(handle) })
                } catch {
                    // Set the real media stop cause before the fallback cancels
                    // and joins this exact token. Never lose an accepted worker.
                    await stop.cancel(.system)
                    tracking = await Jobs.trackRetainingDrain(handle, setup: { _ in throw error })
                }
                switch tracking {
                case .tracked(let job):
                    let updates = Task {
                        for await update in job.progress {
                            progress(update)
                            if let snapshot = try? await MaintenanceLane.shared.call(.providerBootstrapMediaSnapshot, { try started.result.mediaSnapshot() }) {
                                mediaProgress(.init(snapshot))
                            }
                        }
                    }
                    terminal = await job.outcome()
                    await updates.value
                case .drained(_, let actual): terminal = actual
                }
            }
            await stop.finish()
            // These cached reads intentionally survive collection activation.
            // Do not revalidate the owner or test Task.isCancelled here.
            let result = try await MaintenanceLane.shared.call(.providerBootstrapOutcome) { try started.result.outcome() }
            var snapshot: MeguriBootstrapMediaProgress?
            var progressFailure: BridgeError?
            do throws(BridgeError) {
                snapshot = try await MaintenanceLane.shared.call(.providerBootstrapMediaSnapshot) {
                    try started.result.mediaSnapshot().map(MeguriBootstrapMediaProgress.init)
                }
            } catch { progressFailure = error }
            if let snapshot { mediaProgress(snapshot) }
            if case .pending = result {
                switch terminal {
                case .failed(let error): throw error
                case .cancelled: throw BridgeError.cancelled
                default: throw BridgeError.internal("Meguri bootstrap worker returned no terminal result")
                }
            }
            let mapped = try await self.outcome(result)
            var retry: MeguriBootstrapOperation?
            // Only a failed preparation can need this owner-validating lookup.
            // Failure to obtain retry authority cannot erase a terminal receipt.
            if preparation, started.handle != nil, case .refused = result, await self.canReadRetry,
               let retained = try? await MaintenanceLane.shared.call(.providerBootstrapRetainedOperation, { try started.result.retainedOperation() }) {
                retry = try? await self.operation(retained)
            }
            return .command(.init(outcome: mapped, jobID: started.handle.map { .init(rawValue: $0.id) },
                                  mediaProgress: snapshot, progressFailure: progressFailure, retryOperation: retry))
        }
        guard case .command(let result) = value else { throw BridgeError.internal("Unexpected bootstrap command") }
        return result
    }
    private var canReadRetry: Bool { !retired }
    public func prepare(_ direction: MeguriBootstrapDirection, progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        let direction: MeguriCoreBindings.ProviderBootstrapDirection = switch direction {
        case .useServerCollection: .useServerCollection; case .useDeviceCollection: .useDeviceCollection
        }
        return try await worker(.providerBootstrapPrepare, preparation: true, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startBootstrapPrepare(credentials: $1, direction: direction)
        })
    }
    public func confirm(_ preview: MeguriBootstrapPreview, progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        try await worker(.providerBootstrapConfirm, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startBootstrapConfirm(credentials: $1, preview: preview.raw)
        })
    }
    public func prepareMedia(_ operation: MeguriBootstrapOperation, progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        try await worker(.providerBootstrapMediaPrepare, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startBootstrapMediaPrepare(credentials: $1, operation: operation.raw)
        })
    }
    public func transferMedia(_ consent: MeguriBootstrapMediaConsent, progress: @escaping @Sendable (CoreProgress) -> Void = { _ in },
                              mediaProgress: @escaping @Sendable (MeguriBootstrapMediaProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        try await worker(.providerBootstrapMedia, progress: progress, mediaProgress: mediaProgress, start: .credentials {
            try $0.startBootstrapMedia(credentials: $1, consent: consent.raw)
        })
    }
    private static func rawChoice(_ choice: MeguriSetupChoice) -> MeguriCoreBindings.ProviderSetupChoice {
        switch choice { case .noQuestion: .noQuestion; case .keepDevice: .keepDevice; case .keepServer: .keepServer }
    }
    /// MP6.3 preparation. The core picks the direction from what this device holds.
    public func prepareSetup(progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        try await worker(.providerBootstrapPrepare, preparation: true, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startSetupPrepare(credentials: $1)
        })
    }
    /// Local reads only; the preview stays usable.
    public func setupDecision(_ preview: MeguriBootstrapPreview) async throws -> MeguriSetupDecisionOutcome {
        guard !retired else { throw MeguriBootstrapAdmissionFailure.retired }
        guard active == nil else { throw MeguriBootstrapAdmissionFailure.busy }
        let credentials = try await credentials()
        guard !retired else { throw MeguriBootstrapAdmissionFailure.retired }
        guard credentials.connection == connection else { throw MeguriProviderCredentialFailure.wrongConnection }
        let value = MeguriCoreBindings.ProviderCredentials(
            connection: .init(reference: connection.reference, revision: connection.revision, endpoint: connection.endpoint),
            username: credentials.username, hostkey: credentials.hostKey)
        let raw = self.raw
        let outcome = try await MaintenanceLane.shared.call(.providerBootstrapReadiness) {
            try raw.setupDecision(credentials: value, preview: preview.raw)
        }
        switch outcome {
        case .refused(let failure): return .refused(.init(failure))
        case .decision(let decision):
            let facts = try await MaintenanceLane.shared.call(.providerBootstrapReadiness) { try decision.facts() }
            return .decision(.init(decision, facts, account: credentials.username))
        }
    }
    /// Row 1, row 3 or "keep this device's collection".
    public func confirmSetup(_ decision: MeguriSetupDecision, preview: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                             progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        let choice = Self.rawChoice(choice)
        return try await worker(.providerBootstrapConfirm, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startSetupConfirm(credentials: $1, decision: decision.raw, preview: preview.raw, choice: choice)
        })
    }
    /// Row 2 or "keep the server's collection": the download is prepared in place of the upload.
    public func prepareSetupDownload(_ decision: MeguriSetupDecision, preview: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                                     progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        let choice = Self.rawChoice(choice)
        return try await worker(.providerBootstrapPrepare, preparation: true, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startSetupDownloadPrepare(credentials: $1, decision: decision.raw, preview: preview.raw, choice: choice)
        })
    }
    public func confirmSetupDownload(_ decision: MeguriSetupDecision, download: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                                     progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        let choice = Self.rawChoice(choice)
        return try await worker(.providerBootstrapConfirm, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startSetupDownloadConfirm(credentials: $1, decision: decision.raw, download: download.raw, choice: choice)
        })
    }
    /// MP6.9 accepted: confirms the recovery preview only while this device is
    /// still what the upload sent; refused as `setupLocalChanged` otherwise.
    public func confirmSetupActivation(_ recovery: MeguriBootstrapPreview,
                                       progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }) async throws -> MeguriBootstrapCommandResult {
        try await worker(.providerBootstrapConfirm, progress: progress, mediaProgress: { _ in }, start: .credentials {
            try $0.startSetupActivationConfirm(credentials: $1, recovery: recovery.raw)
        })
    }
    /// MP6.9: one read of the server's facts. Starts no upload.
    public func settleSetup() async throws -> MeguriBootstrapCommandResult {
        try await worker(.providerBootstrapRecovery, progress: { _ in }, mediaProgress: { _ in }, start: .credentials {
            try $0.startSetupSettle(credentials: $1)
        })
    }
    public func complete(_ operation: MeguriBootstrapOperation) async throws -> MeguriBootstrapOutcome {
        let value = try await admit { stop in
            let credentials = try await self.credentialValue(stop)
            let raw = self.raw
            let result = try await MaintenanceLane.shared.call(.providerBootstrapComplete) {
                try stop.check(); return try raw.completeBootstrap(credentials: credentials, operation: operation.raw)
            }
            return .command(.init(outcome: try await self.outcome(result), jobID: nil, mediaProgress: nil, progressFailure: nil, retryOperation: nil))
        }
        guard case .command(let result) = value else { throw BridgeError.internal("Unexpected bootstrap command") }
        return result.outcome
    }
}
