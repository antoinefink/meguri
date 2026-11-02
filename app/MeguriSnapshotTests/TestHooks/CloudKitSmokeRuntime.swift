#if DEBUG && MEGURI_TEST_HOOKS
@testable import AppModel
import CoreBridge
import CloudKit
import Darwin
import Foundation

// This adapter uses the production composition, including its real native SDK
// factory. The test-only admission fences approved acct1 before Core sees a
// fresh identity; production Core still owns ordered identity acceptance.
@MainActor final class CloudKitSmokeRuntime: AgentRuntime {
    nonisolated enum Dependency: String, Codable {
        case signedProductVerifier = "signed-product-receipt-verifier"
        case generatedBase = "verified-generated-small-base"
        case integrityInspector = "smoke-integrity-inspector"
        case recoveryInspector = "durable-smoke-recovery-inspector"
    }
    let missing: [Dependency] = [.signedProductVerifier, .generatedBase, .integrityInspector, .recoveryInspector]
    private(set) var userSteps: [String] = []
    private let directory: URL
    private var owner: ReviewCollection?
    private let originalHeaderBytes: Data
    private var admission: AgentAccountAdmission?
    private var runningProduct: AgentRunningProductBinding?
    private var mutation: NoteMutation?
    private var expectedFields: [String] = []
    private var backend: CloudKitSmokeBackend?

    init(directory: URL, originalHeaderBytes: Data) {
        self.directory = directory
        self.originalHeaderBytes = originalHeaderBytes
    }
    func verifyProduct(_ header: AgentHeader) throws {
        userSteps = []
        guard try AgentHeader.read(originalHeaderBytes) == header else { throw AgentFailure.binding }
        if let admission {
            try checkActualProduct(header)
            try admission.requireHeader(header)
        } else {
            admission = try AgentAccountAdmission(originalHeaderBytes: originalHeaderBytes, header: header,
                productCheck: { try self.checkActualProduct(header) },
                applicationSupportRoot: { try Self.applicationSupportRoot() })
        }
    }
    private func checkActualProduct(_ header: AgentHeader) throws {
        if let runningProduct {
            try runningProduct.requireHeader(header, originalHeaderBytes: originalHeaderBytes)
        } else {
            let receipts = try AgentProductReceiptBinding(admittedRunDirectory: directory,
                                                         originalHeaderBytes: originalHeaderBytes)
            let observed = try AgentRunningProductBinding.inspectActual(receipts: receipts)
            try observed.requireHeader(header, originalHeaderBytes: originalHeaderBytes)
            runningProduct = observed
        }
        guard let configuration = CloudBuildConfiguration.fromBundle else {
            userSteps = ["cloudkit-dev-container"]; throw AgentFailure.implementationMissing
        }
        guard case .development = configuration.environment, configuration.container == header.container,
              header.environment == "development" else { throw AgentFailure.wrongContainer }
        if !configuration.schemaConfigured { userSteps.append("cloudkit-dev-container") }
        // Live client observations do not admit the independent signature,
        // full delivery, launch-plan or live VM/lease/install counterpart. The
        // four copied roles cannot supply it; preserve implementationMissing.
        guard let runningProduct else { throw AgentFailure.implementationMissing }
        try runningProduct.requireIndependentCounterpart()
    }
    private static func applicationSupportRoot() throws -> URL {
        // The OS chooses the current app-container root, never run.json/env.
        // Canonicalize this trusted platform root only, before fixed private
        // descendants; the protected reader opens those with no-follow bindings.
        do {
            let root = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                   appropriateFor: nil, create: false)
            guard let resolved = realpath(root.path, nil) else { throw AgentFailure.storage }
            defer { free(resolved) }
            return URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
        } catch { throw AgentFailure.storage }
    }
    private func retainedAdmission() throws -> AgentAccountAdmission {
        guard let runningProduct else { throw AgentFailure.implementationMissing }
        try runningProduct.revalidate()
        guard let admission else { throw AgentFailure.implementationMissing }
        try admission.revalidate()
        return admission
    }
    private func resolveAccount(container: CKContainer) async throws -> CloudResolvedAccount {
        let admission = try retainedAdmission()
        let resolved = try await admission.resolveAccount {
            switch try await container.accountStatus() {
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
        _ = try retainedAdmission()
        return resolved
    }
    private func freshAccount() async throws {
        _ = try retainedAdmission()
        guard let configuration = CloudBuildConfiguration.fromBundle else { throw AgentFailure.implementationMissing }
        _ = try await resolveAccount(container: CKContainer(identifier: configuration.container))
    }
    func verifyAccount(_ header: AgentHeader) async throws {
        try retainedAdmission().requireHeader(header)
        // AgentEngine has durably appended started before entering this method.
        try await freshAccount()
    }
    private func requireVerifiedGeneratedBase() throws {
        // File existence alone is not authenticated fixture delivery (J2.3).
        throw AgentFailure.implementationMissing
    }
    func execute(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        // Closing/draining owned work does not depend on renewed authority.
        if command.action != .close {
            try await freshAccount()
            _ = try retainedAdmission()
        }
        switch command.action {
        case .launch:
            guard owner == nil, let configuration = CloudBuildConfiguration.fromBundle, configuration.schemaConfigured else { throw AgentFailure.execution }
            try requireVerifiedGeneratedBase()
            // The host must install an authenticated generated small base in
            // this run's data directory. Never substitute a user's collection.
            let data = directory.appendingPathComponent("data", isDirectory: true)
            guard FileManager.default.fileExists(atPath: data.appendingPathComponent("collection.anki2").path) else { throw AgentFailure.execution }
            let sync = SyncCoordinator(bootstrap: {
                try await CoreCloudWorkflowService.configurePlatform(container: configuration.container,
                    environment: configuration.environment, status: .couldNotDetermine, identity: nil)
            }, makeBackend: { [self] persisted in
                let admission = try retainedAdmission()
                let container = CKContainer(identifier: configuration.container)
                let database = container.privateCloudDatabase
                // Both the app account reader and adapter use this factory.
                // Its fresh result matches the frozen input before Core accepts
                // identity; J2.2 preserves automatic SDK scheduling.
                let factory = NativeCloudKitEngineFactory(database: database, accountResolver: { [self] in
                    try await resolveAccount(container: container)
                })
                let stagingRoot = data.appendingPathComponent("cloud-native-staging", isDirectory: true)
                try admission.revalidate()
                try FileManager.default.createDirectory(at: stagingRoot, withIntermediateDirectories: true,
                                                        attributes: [.posixPermissions: 0o700])
                let staging = try CloudMediaStaging(directory: stagingRoot)
                let ownTransport = try NativeCloudOwnTransport(database: database, staging: staging)
                let mediaTransport = try NativeCloudMediaTransport(database: database, staging: staging)
                try admission.revalidate()
                let production = try await NativeCollectionCloudBackend.make(workflow: CoreCloudWorkflowService(),
                    factory: factory, ownTransport: ownTransport, mediaTransport: mediaTransport, persisted: persisted)
                try admission.revalidate()
                let wrapper = CloudKitSmokeBackend(production: production)
                backend = wrapper
                return wrapper
            }, defaults: nil, clock: .system, directory: data, observeSignals: false)
            var launch = LaunchConfiguration()
            launch.collectionLocation = AppModel.CollectionLocation(directory: data)
            let collection = ReviewCollection(configuration: launch, navigation: AppNavigation(), sync: sync)
            owner = collection
            _ = try retainedAdmission()
            await collection.start()
            _ = try retainedAdmission()
            await sync.prepareConnectionAuthority()
            guard collection.state == .ready, !sync.operationFailed, let backend else { throw AgentFailure.execution }
            // No new recovery/publication decision is invented for an unbound
            // base. The existing production connection must already be usable.
            guard sync.status?.controls.bindingUsable == true else { throw AgentFailure.execution }
            try observe(.init(name: .coreOpened, count: collection.generation))
            try backend.checkFailure()
            // Full integrity inspection has no existing public service here.
            // Opening a collection alone must never be called Q5 integrity.
            throw AgentFailure.implementationMissing
        case .addNote:
            guard let owner, owner.state == .ready, mutation == nil else { throw AgentFailure.execution }
            let types = try await CoreNotetypeService().notetypeNames()
            guard let type = types.first else { throw AgentFailure.execution }
            _ = try retainedAdmission()
            let record = try await CoreNotetypeService().readNotetype(id: type.id)
            guard record.fields.count == 2, let deck = owner.decks.first(where: { !$0.filtered }) else { throw AgentFailure.execution }
            // Generated in process. Only the fixed entity label and receipt
            // sequence leave the process, never these values or native ids.
            expectedFields = ["synthetic-smoke-Q-" + UUID().uuidString, "synthetic-smoke-A-" + UUID().uuidString]
            _ = try retainedAdmission()
            let result = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: expectedFields,
                tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
            guard result.written, !result.cards.isEmpty else { throw AgentFailure.execution }
            mutation = result
            try observe(.init(name: .localCommitted, count: result.summary.seq, operation: "local1"))
            _ = try retainedAdmission()
            try await CoreCollectionService().flushForBackground()
            _ = try retainedAdmission()
            let durable = try await CoreNoteService().readNote(id: result.id)
            guard durable.fields == expectedFields, durable.notetypeId == type.id else { throw AgentFailure.execution }
            let fact = AgentFact(name: .localDurable, count: result.summary.seq, operation: "local1")
            try observe(fact)
            return [.init(name: .localDurability, outcome: .pass, facts: [fact])]
        case .syncNow:
            guard let owner, let backend, let mutation else { throw AgentFailure.execution }
            try backend.prepareForSyncNow()
            owner.sync.syncNow()
            await owner.sync.waitForInputs()
            guard owner.sync.syncNowUnavailable == nil, !owner.sync.operationFailed else { throw AgentFailure.execution }
            // This stream observes the original policy-issued RecordExchange
            // after real execution; it does not supply expected results.
            let exchange = try await backend.requestedExchangeResult()
            await owner.sync.waitForInputs()
            guard !owner.sync.operationFailed,
                  case .succeeded(let selected, let held, _, let incoming, _) = exchange.outcome,
                  selected > 0, held == 0, incoming == 0 else { throw AgentFailure.execution }
            _ = try retainedAdmission()
            let note = try await CoreNoteService().readNote(id: mutation.id)
            guard note.fields == expectedFields else { throw AgentFailure.execution }
            let result = AgentFact(name: .syncResult, count: exchange.exchange, operation: "exchange1")
            try observe(result)
            return [.init(name: .coordinatorResult, outcome: .pass, facts: [result])]
        case .close:
            guard let owner, await owner.close() else { throw AgentFailure.execution }
            self.owner = nil
            return [.init(name: .localDurability, outcome: .pass, facts: [.init(name: .ownedWorkJoined)])]
        }
    }
    func recover(_ command: AgentCommand, started: AgentRecord, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        _ = try retainedAdmission()
        // No guessed reconstruction of an operation from a payload digest, and
        // no reissued mutation. A durable receipt/row inspector is still needed.
        throw AgentFailure.implementationMissing
    }
    func cancelAndJoin() async throws {
        guard let owner else { return }
        guard await owner.sync.cancelAndDrain() else { throw AgentFailure.execution }
        guard await owner.close() else { throw AgentFailure.execution }
        self.owner = nil
    }
}

// Observe existing production command completion without replacing policy,
// native transport, tracking, admission or cancellation.
@MainActor private final class CloudKitSmokeBackend: SyncCoordinatorBackend {
    let production: NativeCollectionCloudBackend
    var connection: CoreCloudConnectionService? { production.connection }
    var own: CloudOwnExecutor? { production.own }
    private let exchanges = AgentExchangeEvents<CloudPolicyExchangeRecord>()
    private var requestedExchange: UInt64?
    init(production: NativeCollectionCloudBackend) {
        self.production = production
    }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        let output = try await production.input(event)
        if case .syncNowRequested(let source) = event {
            requestedExchange = nil
            // Observe only the original command issued for this input. Activity
            // records merged into the output cannot identify the requested work.
            if case .commands(let commands) = output {
                let issued = commands.compactMap { command -> UInt64? in
                    guard case .runExchange(let exchange, let issuedSource) = command.action,
                          issuedSource == source else { return nil }
                    return exchange
                }
                if issued.count == 1 { requestedExchange = issued[0] }
            }
        }
        return output
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        let result = await production.execute(command)
        if result.error != nil { exchanges.fail(.execution) }
        if result.error == nil, !result.deferred, case .recordExchange(let record) = command.action { exchanges.yield(record) }
        return result
    }
    func checkFailure() throws { try exchanges.checkFailure() }
    func prepareForSyncNow() throws {
        try checkFailure()
        requestedExchange = nil
    }
    func requestedExchangeResult() async throws -> CloudPolicyExchangeRecord {
        try checkFailure()
        guard let requestedExchange else { throw AgentFailure.execution }
        return try await exchanges.next(exchange: requestedExchange, identifiedBy: { $0.exchange })
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 {
        try await production.beginAccountRead(transition: transition)
    }
    func readAccount(_ id: UInt64) async -> SyncCommandResult {
        await production.readAccount(id)
    }
    func prepareAccountRead(_ id: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult {
        await production.prepareAccountRead(id)
    }
    func takeAccountReadRequest() async -> Bool { await production.takeAccountReadRequest() }
    func freezeForSuspend() async throws { try await production.freezeForSuspend() }
    func resumeAfterCoreResume() async {
        await production.resumeAfterCoreResume()
    }
    func quiesce() async throws { try await production.quiesce() }
    func close() async throws { try await production.close(); exchanges.finish() }
}
#endif
