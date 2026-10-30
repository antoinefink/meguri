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

    private init(workflow: CoreCloudWorkflowService, policy: CoreCloudPolicyService,
                 connection: CoreCloudConnectionService, own: CloudOwnExecutor,
                 factory: NativeCloudKitEngineFactory, adapter: CloudKitSyncAdapter,
                 executor: NativeCloudPolicyExecutor) {
        self.workflow = workflow; self.policy = policy; self.connection = connection; self.own = own
        self.accounts = CloudAppAccountReader(workflow: workflow, policy: policy, factory: factory, adapter: adapter)
        self.adapter = adapter; self.executor = executor
    }

    static func make(configuration: CloudBuildConfiguration, directory: URL,
                     persisted: @escaping @Sendable () async -> Void) async throws -> NativeCollectionCloudBackend {
        let workflow = CoreCloudWorkflowService()
        let policy = try await workflow.policy()
        let connection = try await workflow.connection()
        let media = try await workflow.media()
        let container = CKContainer(identifier: configuration.container)
        let database = container.privateCloudDatabase
        let factory = NativeCloudKitEngineFactory(database: database, accountResolver: {
            try await resolveAccount(container)
        })
        let adapter = CloudKitSyncAdapter(core: workflow, engineFactory: factory,
            eventDidPersist: { _ in await persisted() }, exchangeDidJoin: persisted,
            appOwnsAccountObservation: true, accountReadRequested: persisted)
        let staging = try await makeStaging(directory: directory)
        let mediaTransport = try NativeCloudMediaTransport(database: database, staging: staging)
        let executor = NativeCloudPolicyExecutor(policy: policy, adapter: adapter,
            mediaService: media, mediaTransport: mediaTransport)
        let own = CloudOwnExecutor(core: connection, transport: try NativeCloudOwnTransport(database: database))
        return NativeCollectionCloudBackend(workflow: workflow, policy: policy, connection: connection,
            own: own, factory: factory, adapter: adapter, executor: executor)
    }

    func takeAccountReadRequest() async -> Bool { await adapter.takeAccountReadRequest() }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput { try await policy.input(event) }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        async let native = executor.execute(command)
        if case .cancelBackgroundFlush = command.action { await own?.interruptAndDrain() }
        let result = await native
        return .init(outputs: result.outputs, error: result.error, deferred: result.deferred)
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 {
        try await accounts.begin(transition: transition)
    }
    func readAccount(_ id: UInt64) async -> SyncCommandResult {
        let result = await accounts.read(id)
        guard result.verified, result.error == nil, !closed, !Task.isCancelled else {
            return .init(outputs: result.outputs, error: result.error, deferred: false, accountVerified: result.verified)
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
                    return .init(outputs: result.outputs + [refreshed], error: nil, deferred: false, accountVerified: true)
                }
            }
            return .init(outputs: result.outputs, error: nil, deferred: false, accountVerified: true)
        } catch { return .init(outputs: result.outputs, error: error, deferred: false) }
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
    func close() async throws {
        guard !closed else { return }
        await accounts.close()
        await own?.close()
        var replaced = false
        do { _ = try await policy.status() }
        catch BridgeError.cloudPolicy(.generationChanged) { replaced = true }
        catch { throw error }
        do { try await adapter.stop() }
        catch {
            guard replaced, await adapter.retainedInstanceIDs().isEmpty else { throw error }
        }
        // stop joins the original SDK handlers, operations and release; no
        // callbacks are allowed to outlive the Host which owns their handles.
        guard await adapter.retainedInstanceIDs().isEmpty else { throw CloudOwnTransportFailure.busy }
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
