import Foundation
import Instrumentation
internal import MeguriCoreBindings

public protocol ManualTransferService: Sendable {
    func normalizeEndpoint(_ text: String) async throws -> String
    func login(endpoint: String, username: String, password: String, operation: ManualTransferOperation) async throws -> ManualTransferCredentials
    func history(operation: ManualTransferOperation) async throws -> [ManualTransferHistory]
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, scope: ManualTransferScope, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer
    func prepareMedia(_ entry: ManualTransferHistory, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation) async throws -> PreparedManualMedia
    func runMedia(_ preview: PreparedManualMedia, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation, progress: @escaping @Sendable (ManualMediaSnapshot) -> Void) async throws -> ManualTransferRecord
    func cancelPreparedMedia(_ preview: PreparedManualMedia, operation: ManualTransferOperation) async throws -> ManualTransferRecord
    func invalidateMediaOwner(operation: ManualTransferOperation) async throws
    func confirm(endpoint: String, credentials: ManualTransferCredentials, preview: PreparedManualTransfer, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ManualTransferRecord
    func recover(_ entry: ManualTransferHistory, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer
    func cancelPreparation(_ entry: ManualTransferHistory, operation: ManualTransferOperation) async throws
    func discard(_ preview: PreparedManualTransfer, interrupted: Bool, operation: ManualTransferOperation) async throws
    func saveCopy(_ entry: ManualTransferHistory, index: UInt32, to: URL, operation: ManualTransferOperation) async throws
}
// Existing collection-only adapters keep their behavior; selected media requires
// an explicit implementation, so no adapter can silently discard the scope.
public extension ManualTransferService {
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, scope: ManualTransferScope, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer {
        guard scope == .collectionOnly else { throw BridgeError.unavailable(reason: "Media transfer unavailable") }
        return try await prepare(endpoint: endpoint, credentials: credentials, direction: direction, operation: operation, progress: progress)
    }
    func prepareMedia(_ entry: ManualTransferHistory, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation) async throws -> PreparedManualMedia { throw BridgeError.unavailable(reason: "Media transfer unavailable") }
    func runMedia(_ preview: PreparedManualMedia, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation, progress: @escaping @Sendable (ManualMediaSnapshot) -> Void) async throws -> ManualTransferRecord { throw BridgeError.unavailable(reason: "Media transfer unavailable") }
    func cancelPreparedMedia(_ preview: PreparedManualMedia, operation: ManualTransferOperation) async throws -> ManualTransferRecord { throw BridgeError.unavailable(reason: "Media transfer unavailable") }
    func invalidateMediaOwner(operation: ManualTransferOperation) async throws {}
}
public actor CoreManualTransferService: ManualTransferService {
    private let location: CollectionLocation
    private let clock: CollectionClockRelay
    private let space: DiskSpaceRelay
    private var session: MeguriCoreBindings.ManualTransferSession?
    private let destination = CoordinatedExportDestination()
    private let publisher = StagedCollectionExportDestination()
    public init(location: CollectionLocation, clock: any CollectionClock = SystemCollectionClock(), diskSpace: any DiskSpaceProviding = DiskSpaceProvider()) {
        self.location = location; self.clock = CollectionClockRelay(clock); space = DiskSpaceRelay(diskSpace)
    }
    private func owner() async throws -> MeguriCoreBindings.ManualTransferSession {
        if let session { return session }
        let location = location; let clock = clock; let space = space
        let seed = UInt64.random(in: 0...UInt64.max)
        let value = try await MaintenanceLane.shared.call(.manualTransferSessionNew) {
            try MeguriCoreBindings.ManualTransferSession(location: location.raw, clock: clock, diskSpace: space, randomSeed: seed)
        }
        session = value; return value
    }
    public func normalizeEndpoint(_ text: String) async throws -> String {
        try await ControlLane.shared.call(.normalizeManualEndpoint) { try normalizeManualEndpoint(text: text) }
    }
    /// Install event delivery before starting a worker. Wait for its terminal
    /// event and inspect its typed result even when observation setup fails or
    /// the caller stops waiting. Committed activation remains authoritative.
    private func run(_ operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void = { _ in }, mediaProgress: (@Sendable (ManualMediaSnapshot) -> Void)? = nil, start: @escaping @Sendable () throws -> MeguriCoreBindings.ManualTransferJob, call: Instrumentation.BridgeCall) async throws -> MeguriCoreBindings.ManualTransferValue {
        try await CoreEventStream.shared.start(); try operation.check()
        return try await withTaskCancellationHandler {
            let started = try await MaintenanceLane.shared.call(call, start)
            var terminal: JobOutcome?
            do throws(BridgeError) {
                let job = try await Jobs.trackDraining(started.job)
                await operation.attach(job, stop: { cause in
                    try? await ControlLane.shared.call(BridgeCall.ManualMediaCalls.stop) { try started.result.requestStop(interrupted: cause == .system) }
                })
                let updates = Task {
                    for await value in job.progress {
                        progress(value)
                        if let mediaProgress,
                           let snapshot = try? await MaintenanceLane.shared.call(BridgeCall.ManualMediaCalls.readProgress, { try started.result.mediaSnapshot() }) {
                            mediaProgress(ManualMediaSnapshot(snapshot))
                        }
                    }
                }
                terminal = await job.outcome(); await updates.value
            } catch { terminal = .failed(error) }
            if let mediaProgress, let snapshot = try await MaintenanceLane.shared.call(BridgeCall.ManualMediaCalls.readProgress, { try started.result.mediaSnapshot() }) {
                mediaProgress(ManualMediaSnapshot(snapshot))
            }
            if let id = try await MaintenanceLane.shared.call(.manualTransferResultOperationID, { try started.result.operationId() }) {
                operation.identify(id)
            }
            switch try await MaintenanceLane.shared.call(.manualTransferResultOutcome, { try started.result.outcome() }) {
            case .value(let value): return value
            case .failed(let failure): throw ManualTransferFailure(failure)
            case .pending:
                if case .failed(let error) = terminal { throw error }
                if operation.cancellation == .user { throw BridgeError.cancelled }
                throw ManualTransferInterrupted()
            }
        } onCancel: { Task { await operation.cancel(.system) } }
    }
    private func setup(_ operation: ManualTransferOperation) async throws {
        let session = try await owner()
        _ = try await run(operation, start: { try session.startSetup() }, call: .manualTransferSessionStartSetup)
    }
    public func login(endpoint: String, username: String, password: String, operation: ManualTransferOperation) async throws -> ManualTransferCredentials {
        let session = try await owner()
        let result = try await run(operation, start: { try session.startLogin(configured: endpoint, username: username, password: password) }, call: .manualTransferSessionStartLogin)
        guard case .login(let credentials) = result else { throw BridgeError.internal("Unexpected login result") }
        return .init(username: credentials.username, hostKey: credentials.hostKey)
    }
    public func history(operation: ManualTransferOperation) async throws -> [ManualTransferHistory] {
        let session = try await owner()
        let value = try await run(operation, start: { try session.startHistory() }, call: .manualTransferSessionStartHistory)
        guard case .history(let entries) = value else { throw BridgeError.internal("Unexpected history result") }
        var result: [ManualTransferHistory] = []
        for entry in entries {
            let record = try await MaintenanceLane.shared.call(.manualTransferHistoryEntryRecord) { ManualTransferRecord(try entry.record()) }
            result.append(.init(raw: entry, record: record))
        }
        return result.sorted { $0.record.preparedMilliseconds > $1.record.preparedMilliseconds }
    }
    private func preview(_ value: MeguriCoreBindings.ManualTransferValue) async throws -> PreparedManualTransfer {
        guard case .preview(let raw) = value else { throw BridgeError.internal("Unexpected preparation result") }
        let record = try await MaintenanceLane.shared.call(.manualTransferPreviewRecord) { ManualTransferRecord(try raw.record()) }
        return .init(raw: raw, record: record)
    }
    public func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer {
        try await prepare(endpoint: endpoint, credentials: credentials, direction: direction, scope: .collectionAndMedia, operation: operation, progress: progress)
    }
    public func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, scope: ManualTransferScope, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer {
        try await setup(operation)
        let session = try await owner()
        let rawDirection: MeguriCoreBindings.TransferDirection = direction == .import ? .import : .export
        let rawScope: MeguriCoreBindings.TransferScope = scope == .collectionOnly ? .collectionOnly : .collectionAndMedia
        let value = try await run(operation, progress: progress, start: { try session.startPrepare(configured: endpoint, username: credentials.username, hostKey: credentials.hostKey, direction: rawDirection, scope: rawScope) }, call: .manualTransferSessionStartPrepare)
        return try await preview(value)
    }
    public func confirm(endpoint: String, credentials: ManualTransferCredentials, preview: PreparedManualTransfer, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ManualTransferRecord {
        guard let raw = preview.raw else { throw BridgeError.internal("Missing transfer authority") }
        let session = try await owner()
        let value = try await run(operation, progress: progress, start: { try session.startConfirm(configured: endpoint, username: credentials.username, hostKey: credentials.hostKey, preview: raw) }, call: .manualTransferSessionStartConfirm)
        guard case .operation(let record) = value else { throw BridgeError.internal("Unexpected transfer result") }
        return ManualTransferRecord(record)
    }
    public func prepareMedia(_ entry: ManualTransferHistory, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation) async throws -> PreparedManualMedia {
        guard let historyAuthority = entry.raw else { throw BridgeError.internal("Missing history authority") }
        try await setup(operation)
        let session = try await owner()
        let value = try await run(operation, start: { try session.startMediaPreview(configured: endpoint, username: credentials.username, hostKey: credentials.hostKey, entry: historyAuthority) }, call: BridgeCall.ManualMediaCalls.preview)
        guard case .mediaPreview(let raw) = value else { throw BridgeError.internal("Unexpected media decision") }
        let record = try await MaintenanceLane.shared.call(BridgeCall.ManualMediaCalls.readPreview) { ManualTransferRecord(try raw.record()) }
        return .init(raw: raw, record: record)
    }
    public func runMedia(_ preview: PreparedManualMedia, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation, progress: @escaping @Sendable (ManualMediaSnapshot) -> Void) async throws -> ManualTransferRecord {
        guard let raw = preview.raw else { throw BridgeError.internal("Missing media authority") }
        let session = try await owner()
        let value = try await run(operation, mediaProgress: progress, start: { try session.startMedia(configured: endpoint, username: credentials.username, hostKey: credentials.hostKey, preview: raw) }, call: BridgeCall.ManualMediaCalls.run)
        guard case .operation(let record) = value else { throw BridgeError.internal("Unexpected media result") }
        return ManualTransferRecord(record)
    }
    public func cancelPreparedMedia(_ preview: PreparedManualMedia, operation: ManualTransferOperation) async throws -> ManualTransferRecord {
        guard let raw = preview.raw else { throw BridgeError.internal("Missing prepared media decision") }
        let session = try await owner()
        let outcome = try await run(operation, start: {
            try session.startCancelPreparedMedia(preview: raw)
        }, call: BridgeCall.ManualMediaCalls.cancelPrepared)
        guard case .operation(let record) = outcome else {
            throw BridgeError.internal("Missing partial transfer after cancelling the prepared media decision")
        }
        return ManualTransferRecord(record)
    }
    public func invalidateMediaOwner(operation: ManualTransferOperation) async throws {
        let session = try await owner()
        _ = try await run(operation, start: { try session.startInvalidateMediaOwner() }, call: BridgeCall.ManualMediaCalls.forgetOwner)
    }
    public func recover(_ entry: ManualTransferHistory, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer {
        guard let raw = entry.raw else { throw BridgeError.internal("Missing transfer authority") }
        try await setup(operation)
        let session = try await owner()
        let value = try await run(operation, progress: progress, start: { try session.startRecoveryPreview(record: raw) }, call: .manualTransferSessionStartRecoveryPreview)
        return try await preview(value)
    }
    public func cancelPreparation(_ entry: ManualTransferHistory, operation: ManualTransferOperation) async throws {
        guard let raw = entry.raw else { throw BridgeError.internal("Missing transfer authority") }
        try await setup(operation)
        let session = try await owner()
        _ = try await run(operation, start: { try session.startCancelHistory(entry: raw, interrupted: false) }, call: .manualTransferSessionStartCancelHistory)
    }
    public func discard(_ preview: PreparedManualTransfer, interrupted: Bool, operation: ManualTransferOperation) async throws {
        guard let raw = preview.raw else { throw BridgeError.internal("Missing transfer authority") }
        let session = try await owner()
        _ = try await run(operation, start: { try session.startCancelPreview(preview: raw, interrupted: interrupted) }, call: .manualTransferSessionStartCancelPreview)
    }
    public func saveCopy(_ entry: ManualTransferHistory, index: UInt32, to url: URL, operation: ManualTransferOperation) async throws {
        guard let raw = entry.raw else { throw BridgeError.internal("Missing transfer authority") }
        let session = try await owner()
        try await withTaskCancellationHandler {
            try await destination.withDestination(url) { coordinated in
                try await self.publisher.save(to: coordinated, operation: operation) { staged in
                    _ = try await self.run(operation, start: { try session.startSaveCopy(entry: raw, index: index, destination: staged.path, finalDestination: coordinated.path) }, call: .manualTransferSessionStartSaveCopy)
                }
            }
        } onCancel: { Task { await operation.cancel(.system) } }
    }
}
