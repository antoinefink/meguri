import CoreBridge
import Foundation
#if canImport(CloudKit)
import CloudKit

/// The app composition root for one retained Host and one private database.
/// No production mock, account cache, policy reducer or sync scheduler lives here.
@MainActor
final class NativeCollectionCloudBackend: SyncCoordinatorBackend {
    let connection: CoreCloudConnectionService?
    let own: CloudOwnExecutor?
    private let workflow: CoreCloudWorkflowService
    private let policy: CoreCloudPolicyService
    private let accounts: CloudAppAccountReader
    private let adapter: CloudKitSyncAdapter
    private let executor: NativeCloudPolicyExecutor
    private var closed = false
    private var quiesceTask: Task<Void, any Error>?

    // Harness-only progress marks contain no native values or error text.
    enum ShutdownMark: String {
        case untouched, running, finished, superseded, failed
        case releasedAfterSupersession, vacant, occupied
    }
    struct ShutdownTrace {
        var policyProbe: ShutdownMark = .untouched
        var adapterDrain: ShutdownMark = .untouched
        var retentionCheck: ShutdownMark = .untouched
    }
    private(set) var shutdownTrace = ShutdownTrace()

    private init(workflow: CoreCloudWorkflowService, policy: CoreCloudPolicyService,
                 connection: CoreCloudConnectionService, own: CloudOwnExecutor,
                 factory: any CloudKitEngineFactory, adapter: CloudKitSyncAdapter,
                 executor: NativeCloudPolicyExecutor) {
        self.workflow = workflow; self.policy = policy; self.connection = connection; self.own = own
        self.accounts = CloudAppAccountReader(workflow: workflow, policy: policy, factory: factory, adapter: adapter)
        self.adapter = adapter; self.executor = executor
    }

    static func make(configuration: CloudBuildConfiguration, directory: URL,
                     persisted: @escaping @Sendable () async -> Void) async throws -> NativeCollectionCloudBackend {
        let workflow = CoreCloudWorkflowService()
        let container = CKContainer(identifier: configuration.container)
        let database = container.privateCloudDatabase
        let factory = NativeCloudKitEngineFactory(database: database, accountResolver: {
            try await resolveAccount(container)
        })
        let staging = try await makeStaging(directory: directory)
        let ownTransport = try NativeCloudOwnTransport(database: database, staging: staging)
        let mediaTransport = try NativeCloudMediaTransport(database: database, staging: staging)
        return try await make(workflow: workflow, factory: factory, ownTransport: ownTransport,
            mediaTransport: mediaTransport, persisted: persisted)
    }

    static func make(workflow: CoreCloudWorkflowService, factory: any CloudKitEngineFactory,
                     ownTransport: any CloudOwnTransport, mediaTransport: any CloudMediaTransport,
                     persisted: @escaping @Sendable () async -> Void,
                     observeEvent: (@Sendable (CloudSDKObservation) async -> Void)? = nil,
                     observeBatch: (@Sendable (String, String, CloudEngineBatch) async -> Void)? = nil,
                     observeOwn: (@Sendable (CloudOwnObservation) -> Void)? = nil,
                     observeExchangeCapture: (@Sendable (CloudPolicyCommand, UInt64) async -> Void)? = nil) async throws -> NativeCollectionCloudBackend {
        let policy = try await workflow.policy()
        let connection = try await workflow.connection()
        let media = try await workflow.media()
        let adapter = CloudKitSyncAdapter(core: workflow, engineFactory: factory,
            eventDidPersist: { _ in await persisted() }, assetReadTransport: ownTransport,
            assetReadDidFinish: persisted, exchangeDidJoin: persisted,
            appOwnsAccountObservation: true, accountReadRequested: persisted,
            observeEvent: observeEvent, observeBatch: observeBatch)
        let executor = NativeCloudPolicyExecutor(policy: policy, adapter: adapter,
            mediaService: media, mediaTransport: mediaTransport, ownTransport: ownTransport,
            observeExchangeCapture: observeExchangeCapture)
        let own = CloudOwnExecutor(core: connection, transport: ownTransport, observe: observeOwn)
        return NativeCollectionCloudBackend(workflow: workflow, policy: policy, connection: connection,
            own: own, factory: factory, adapter: adapter, executor: executor)
    }

    func takeAccountReadRequest() async -> Bool { await adapter.takeAccountReadRequest() }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        let output = try await policy.input(event)
        switch output {
        case .syncNowUnavailable, .applyDeferred: return output
        case .commands, .coreActionFailed: break
        }
        let activity = await adapter.takePolicyOperationOutputs()
        guard !activity.isEmpty else { return output }
        var commands: [CloudPolicyCommand] = []
        var failure: BridgeError?
        for value in activity + [output] {
            switch value {
            case .commands(let values): commands.append(contentsOf: values)
            case .coreActionFailed(let error, let values): failure = failure ?? error; commands.append(contentsOf: values)
            case .applyDeferred(let error): failure = failure ?? error
            case .syncNowUnavailable(let reason):
                // Activity inputs cannot emit an exchange-unavailable result.
                // Keep an explicit failure if that contract ever changes.
                failure = failure ?? .internal("Unexpected activity output: \(reason)")
            }
        }
        if let failure { return .coreActionFailed(error: failure, commands: commands) }
        return .commands(commands)
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        await execute(command, corePrepared: {})
    }
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void) async -> SyncCommandResult {
        async let native = executor.execute(command, corePrepared: corePrepared)
        if case .cancelBackgroundFlush = command.action { await own?.interruptAndDrain() }
        let result = await native
        return SyncCommandResult(result)
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 {
        try await accounts.begin(transition: transition)
    }
    func readAccount(_ id: UInt64) async -> SyncCommandResult {
        let complete = await prepareAccountRead(id)
        return await complete(true)
    }
    func prepareAccountRead(_ id: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult {
        let complete = await accounts.prepareRead(id)
        return { [self] admitted in
            guard admitted else {
                #if DEBUG
                let error = CancellationError()
                DebugCloudDiagnostics.record(.accountCompletionRefused, error: error)
                return .init(outputs: [], error: error, deferred: false)
                #else
                return .init(outputs: [], error: CancellationError(), deferred: false)
                #endif
            }
            let result = await complete()
            guard result.verified, result.error == nil, !closed, !Task.isCancelled else {
                return .init(outputs: result.outputs, error: result.error, deferred: false, accountVerified: result.verified)
            }
            return .init(outputs: result.outputs, error: result.error, deferred: false, accountVerified: result.verified,
                afterOutputs: { [self] admitted in
                    guard admitted, accounts.isCurrentRead(id), !closed, !Task.isCancelled else {
                        return .init(outputs: [], error: CancellationError(), deferred: false)
                    }
                    return await projectAccountRead(result)
                })
        }
    }
    private func projectAccountRead(_ result: CloudAppAccountReadResult) async -> SyncCommandResult {
        guard result.verified, result.error == nil, !closed, !Task.isCancelled else {
            return .init(outputs: [], error: result.error, deferred: false, accountVerified: result.verified)
        }
        do {
            let lifecycle: CollectionLifecycle = try await CoreCollectionService().lifecycle()
            if case .ready = lifecycle {
                let status: CloudPolicyStatus = try await policy.status()
                if status.controls.bindingUsable == true {
                    // Reconcile the current verified Host before policy can issue
                    // StartEngine. Core settles queued inbox work before its scan.
                    try await workflow.prepareRuntimeBeforePolicy()
                    let refreshed = try await policy.input(.refresh(.cloud))
                    return .init(outputs: [refreshed], error: nil, deferred: false, accountVerified: true)
                }
            }
            return .init(outputs: [], error: nil, deferred: false, accountVerified: true)
        } catch { return .init(outputs: [], error: error, deferred: false) }
    }
    func freezeForSuspend() async throws {
        await accounts.freeze()
        try await adapter.freezeForSuspend()
    }
    func resumeAfterCoreResume() async {
        // Local reviews resume immediately. The app's ordered account lane
        // owns the actual read and opens SDK delivery only after fresh proof.
        accounts.resumeAfterCoreResume()
    }
    func quiesce() async throws {
        if let quiesceTask { try await quiesceTask.value; return }
        let task = Task {
            async let nativeDrain: Void = executor.quiesce()
            await accounts.close()
            await own?.close()
            try await nativeDrain
        }
        quiesceTask = task
        do { try await task.value }
        catch { quiesceTask = nil; throw error }
    }
    func close() async throws {
        guard !closed else { return }
        try await quiesce()
        shutdownTrace = ShutdownTrace()
        var replaced = false
        shutdownTrace.policyProbe = .running
        do {
            try await policy.validateOriginalOwner()
            shutdownTrace.policyProbe = .finished
        } catch BridgeError.cloudPolicy(.generationChanged) {
            shutdownTrace.policyProbe = .superseded
            replaced = true
        } catch {
            shutdownTrace.policyProbe = .failed
            throw error
        }
        shutdownTrace.adapterDrain = .running
        do {
            try await adapter.stop()
            shutdownTrace.adapterDrain = .finished
        } catch {
            shutdownTrace.adapterDrain = .failed
            guard replaced, await adapter.retainedInstanceIDs().isEmpty else { throw error }
            shutdownTrace.adapterDrain = .releasedAfterSupersession
        }
        // stop joins the original SDK handlers, operations and release; no
        // callbacks are allowed to outlive the Host which owns their handles.
        shutdownTrace.retentionCheck = .running
        guard await adapter.retainedInstanceIDs().isEmpty else {
            shutdownTrace.retentionCheck = .occupied
            throw CloudOwnTransportFailure.busy
        }
        shutdownTrace.retentionCheck = .vacant
        closed = true
    }

    nonisolated private static func resolveAccount(_ container: CKContainer) async throws -> CloudResolvedAccount {
        let status = try await container.accountStatus()
        switch status {
        case .available:
            let record = try await container.userRecordID()
            return .init(status: .available, identity: Data(record.recordName.utf8))
        case .noAccount: return .init(status: .noAccount, identity: nil)
        case .restricted: return .init(status: .restricted, identity: nil)
        case .temporarilyUnavailable: return .init(status: .temporarilyUnavailable, identity: nil)
        case .couldNotDetermine: return .init(status: .couldNotDetermine, identity: nil)
        @unknown default: return .init(status: .couldNotDetermine, identity: nil)
        }
    }
    @concurrent private nonisolated static func makeStaging(directory: URL) async throws -> CloudMediaStaging {
        // Under the app-private collection/tracking root, on the same volume.
        let root = directory.appending(path: "cloud-native-staging", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        return try CloudMediaStaging(directory: root)
    }
}
#endif

#if canImport(CloudKit)
extension SyncCommandResult {
    init(_ result: CloudPolicyNativeResult) {
        let afterOutputs: (@MainActor @Sendable (Bool) async -> SyncCommandResult)?
        if let followup = result.afterOutputs {
            afterOutputs = { _ in SyncCommandResult(await followup()) }
        } else {
            afterOutputs = nil
        }
        self.init(outputs: result.outputs, error: result.error, deferred: result.deferred, mediaCancellation: result.mediaCancellation,
            acknowledgedOutputCount: result.acknowledgedOutputCount,
            afterOutputs: afterOutputs)
    }
}
#endif
