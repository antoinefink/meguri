#if DEBUG && MEGURI_TEST_HOOKS
@testable import AppModel
import CoreBridge
import CloudKit
import Foundation

// This adapter uses the production composition, including its real native SDK
// factory. It adds no account policy, schema setup, retry or cleanup behavior.
@MainActor final class CloudKitSmokeRuntime: AgentRuntime {
    nonisolated enum Dependency: String, Codable {
        case signedProductVerifier = "signed-product-receipt-verifier"
        case protectedAccountMapping = "protected-expected-account-mapping"
        case generatedBase = "verified-generated-small-base"
        case recoveryInspector = "durable-smoke-recovery-inspector"
    }
    let missing: [Dependency] = [.signedProductVerifier, .protectedAccountMapping, .generatedBase, .recoveryInspector]
    private(set) var userSteps: [String] = []
    private let directory: URL
    private var owner: ReviewCollection?
    private var productVerified = false
    private var accountVerified = false
    private var mutation: NoteMutation?
    private var expectedFields: [String] = []
    private var backend: CloudKitSmokeBackend?

    init(directory: URL) { self.directory = directory }
    func verifyProduct(_ header: AgentHeader) throws {
        productVerified = false; accountVerified = false
        userSteps = []
        guard let configuration = CloudBuildConfiguration.fromBundle else {
            userSteps = ["cloudkit-dev-container"]; throw AgentFailure.implementationMissing
        }
        guard case .development = configuration.environment, configuration.container == header.container,
              header.environment == "development" else { throw AgentFailure.wrongContainer }
        if !configuration.schemaConfigured { userSteps.append("cloudkit-dev-container") }
        guard Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String == header.app_build else { throw AgentFailure.binding }
        // Receipt verification and protected scope loading remain unimplemented.
        // Bundle fields cannot supply either proof.
        throw AgentFailure.implementationMissing
    }
    func verifyAccount(_ header: AgentHeader) async throws {
        guard productVerified else { throw AgentFailure.implementationMissing }
        // A fresh read is permitted only once an independently supplied expected
        // scope exists and Core can compare it. No identity lookup is attempted
        // while the protected mapping format is unspecified.
        throw AgentFailure.implementationMissing
    }
    func execute(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        guard productVerified, accountVerified else { throw AgentFailure.implementationMissing }
        switch command.action {
        case .launch:
            guard owner == nil, let configuration = CloudBuildConfiguration.fromBundle, configuration.schemaConfigured else { throw AgentFailure.execution }
            // The host must install an authenticated generated small base in
            // this run's data directory. Never substitute a user's collection.
            let data = directory.appendingPathComponent("data", isDirectory: true)
            guard FileManager.default.fileExists(atPath: data.appendingPathComponent("collection.anki2").path) else { throw AgentFailure.execution }
            let sync = SyncCoordinator(bootstrap: {
                try await CoreCloudWorkflowService.configurePlatform(container: configuration.container,
                    environment: configuration.environment, status: .couldNotDetermine, identity: nil)
            }, makeBackend: { [self] persisted in
                let production = try await NativeCollectionCloudBackend.make(configuration: configuration, directory: data, persisted: persisted)
                let wrapper = CloudKitSmokeBackend(production: production)
                backend = wrapper
                return wrapper
            }, defaults: nil, clock: .system, directory: data, observeSignals: false)
            var launch = LaunchConfiguration()
            launch.collectionLocation = AppModel.CollectionLocation(directory: data)
            let collection = ReviewCollection(configuration: launch, navigation: AppNavigation(), sync: sync)
            owner = collection
            await collection.start()
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
            let record = try await CoreNotetypeService().readNotetype(id: type.id)
            guard record.fields.count == 2, let deck = owner.decks.first(where: { !$0.filtered }) else { throw AgentFailure.execution }
            // Generated in process. Only the fixed entity label and receipt
            // sequence leave the process, never these values or native ids.
            expectedFields = ["synthetic-smoke-Q-" + UUID().uuidString, "synthetic-smoke-A-" + UUID().uuidString]
            let result = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: expectedFields,
                tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
            guard result.written, !result.cards.isEmpty else { throw AgentFailure.execution }
            mutation = result
            try observe(.init(name: .localCommitted, count: result.summary.seq, operation: "local1"))
            try await CoreCollectionService().flushForBackground()
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
    init(production: NativeCollectionCloudBackend) { self.production = production }
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
    func beginAccountRead(transition: Bool) async throws -> UInt64 { try await production.beginAccountRead(transition: transition) }
    func readAccount(_ id: UInt64) async -> SyncCommandResult { await production.readAccount(id) }
    func takeAccountReadRequest() async -> Bool { await production.takeAccountReadRequest() }
    func freezeForSuspend() async throws { try await production.freezeForSuspend() }
    func resumeAfterCoreResume() async { await production.resumeAfterCoreResume() }
    func quiesce() async throws { try await production.quiesce() }
    func close() async throws { try await production.close(); exchanges.finish() }
}
#endif
