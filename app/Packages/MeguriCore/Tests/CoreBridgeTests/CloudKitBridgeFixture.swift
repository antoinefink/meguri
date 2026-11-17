import Foundation
import Instrumentation
import Testing
import MeguriCoreBindings
@testable import CoreBridge

//
// Generated hook requested (test-hooks only):
// testCloudPrepareAdapterBaseline(entityCount: UInt32, localRevision: UInt64,
//   savedEngineState: Data?, failNextInboxCommit: Bool,
//   failNextStateCommit: Bool) throws -> CloudAdapterBaseline
// Raw result fields mapped here: recordNames: [String], deckIds: [Int64],
// reviewDeckId/reviewCardId: Int64, reviewRecordName/collectionZone: String,
// incomingRecord/rootRecord: generated CloudRecord.
// The two failure switches inject I/O failure at the REAL tracking transaction
// boundary, not a fake persistence result. No host/account values are read.
// Baseline: verified synthetic-account-A, recorded/confirmed MGActive for the
// stable binding, no publication op, no parked work, media disabled. Exactly
// entityCount decks are dirty at revision 3; review rows start clean and the
// seeded review card is due. Remote incomingRecord is an unrelated valid newer
// MGEntity, not this binding's own in-flight save. Seed FSRS admission so the
// enabled review does not introduce unrelated pending work in the 400 test.
// Faults arm for the first corresponding event transaction AFTER preparation.
//
// Future production API requested by these tests (async throws unless noted):
// CoreCloudWorkflowService(): Sendable, lane-backed service; every FFI export
// uses CoreLane.call with a registered BridgeCall, including diagnostic reads.
// instance() -> String; currentGuard() -> CloudGuard (generation/binding/token)
// entityFacts(name: String) -> CloudEntityFacts?
// awaitTracking(generation: UInt64, seq: UInt64) -> Void
// trackingSendables() -> [String: CloudPendingChange]
// pendingTransition(instance: String, kind: CloudUpdateKind,
//                   snapshot: [String: CloudPendingChange]) -> CloudPendingUpdate
// emittedUpdateIDs() -> [UInt64]; observations() -> [CloudWorkflowObservation]
// pollReview() -> Void; stopSyncing() -> Void; setPaused(Bool) -> Void
// exchangeResult(id: UInt64) -> CloudExchangeResult?
// durableSnapshot() -> CloudDurableSnapshot; fetchDrains() -> [CloudFetchDrain]
// diagnostics() -> CloudWorkflowDiagnostics; status() -> CloudWorkflowStatus
//
// Entity facts map Runtime::EntityFacts, not adapter-maintained state:
// localRev/ackedRev: UInt64, mark: optional DirtyMark, inFlight: [CloudFlight]
// (localRev: UInt64), payloadDigest/ackedPayloadDigest/ackedSystemFields: Data.
// DurableSnapshot is an Equatable read of the real binding's tracking rows,
// entity revisions/marks/in-flight, inbox, root and engine_state. It EXCLUDES
// refusal logs and diagnostic counters; includes serialization: Data?,
// inboxNames: Set<String>, trackingStopped/absencePassCompleted: Bool.
// It remains readable after host close via the retained tracking owner.
// FetchDrain: zone: String, fetched: Bool. Diagnostics: missingDidFetchEvents: Int.
// Status: syncingCollection: Bool. ExchangeResult: id: UInt64,
// collection: succeeded | waiting(noAccount) | other P12 results,
// selectedNames/ackedNames/outstandingNames: Set<String>, Equatable, durable.
//
// CloudKitSyncAdapter(core: CoreCloudWorkflowService,
//   engineFactory: any CloudKitEngineFactory,
//   eventDidPersist: @escaping @Sendable (CloudEngineEvent) async -> Void)
// start(automaticallySync: Bool), stop(), configureAutomatic(Bool),
// accountChanged(status: CloudAccountStatus, identity: Data?),
// requestSend(zones: [String]), requestFetch(zones: [String]),
// executePendingActions(), consumePendingUpdates(), waitForIdle(),
// waitForDeferredUpdates(), waitForStopping(instanceID: String) -> Void
// syncNow() -> CloudExchangeResult
// retainedInstanceIDs() async -> Set<String> (actual adapter ownership table).
// waitForIdle joins actual tasks/handlers/durable acks, NOT a timer/poll or
// helper-set success flag. waitForDeferredUpdates joins only returned handlers'
// ordered update work, so it is usable while an SDK send has not returned.
// eventDidPersist is called after real persistence, before handler return;
// all generation/binding/token/instance checks survive its suspension.
private struct CloudBaseline: Sendable {
    let names: [String]
    let decks: [Int64]
    let reviewDeck: Int64
    let reviewCard: Int64
    let reviewName: String
    let zone: String
    let incoming: CoreBridge.CloudRecord
    let root: CoreBridge.CloudRecord
}

extension MaintenanceLane {
    fileprivate func prepareCloudAdapterBaseline(
        count: UInt32, savedState: Data?, failInbox: Bool, failState: Bool
    ) throws -> CloudBaseline {
        try call("test.cloudPrepareAdapterBaseline") {
            let raw = try MeguriCoreBindings.testCloudPrepareAdapterBaseline(
                entityCount: count, localRevision: 3, savedEngineState: savedState,
                failNextInboxCommit: failInbox, failNextStateCommit: failState
            )
            return CloudBaseline(names: raw.recordNames, decks: raw.deckIds,
                                 reviewDeck: raw.reviewDeckId, reviewCard: raw.reviewCardId,
                                 reviewName: raw.reviewRecordName, zone: raw.collectionZone,
                                 incoming: CoreBridge.CloudRecord(raw.incomingRecord), root: CoreBridge.CloudRecord(raw.rootRecord))
        }
    }
}

/// Observes an actual completed durable call, still INSIDE its SDK handler.
/// This internal adapter observation seam may suspend but never supplies a
/// core result. Production uses a no-op. It is not an event implementation.
actor CloudPersistedEventProbe {
    private var sent: CloudCallbackGate?
    private var fetched: CloudCallbackGate?
    func pauseSent(_ gate: CloudCallbackGate) { sent = gate }
    func pauseFetched(_ gate: CloudCallbackGate) { fetched = gate }
    func didPersist(_ event: CloudEngineEvent) async {
        switch event {
        case .sent:
            if let gate = sent { sent = nil; await gate.enter() }
        case .fetched:
            if let gate = fetched { fetched = nil; await gate.enter() }
        default: break
        }
    }
}

/// Q3.6: 2025-01-01T00:00:00Z. Review timestamps share the fixture clock.
let cloudFixtureOriginMilliseconds: Int64 = 1_735_689_600_000

/// Canonical synthetic wire input only; authority still requires Core intake.
func boundEpochBytes(delta: Int64 = 0, publishing: Bool = false) async throws -> Data {
    try await MaintenanceLane.shared.call("test.cloudBoundEpochBytes") {
        try MeguriCoreBindings.testCloudBoundEpochBytes(crtDelta: delta, publishing: publishing)
    }
}
func epochAuthority() async throws -> Int64? {
    try await MaintenanceLane.shared.call("test.cloudEpochCrt") { try MeguriCoreBindings.testCloudEpochCrt() }
}

private struct CloudFixtureClock: CollectionClock {
    func nowMilliseconds() -> Int64 { cloudFixtureOriginMilliseconds }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

struct CloudKitBridgeFixture: Sendable {
    let directory: URL
    let collection: CoreCollectionService
    let core: CoreCloudWorkflowService
    let adapter: CloudKitSyncAdapter
    let factory: CloudKitEngineFakeFactory
    let probe: CloudPersistedEventProbe
    let names: [String]
    let decks: [Int64]
    let reviewDeck: Int64
    let reviewCard: Int64
    let reviewName: String
    let zone: String
    let incoming: CoreBridge.CloudRecord
    let root: CoreBridge.CloudRecord

    static func open(count: UInt32 = 1, a23: Bool = true, keepAlive: Bool = false,
                     savedState: Data? = Data([0x73, 0x64, 0x6b]),
                     failInbox: Bool = false, failState: Bool = false, startAdapter: Bool = true,
                     assetReadTransport: (any CloudOwnTransport)? = nil,
                     delegateWorkValidated: @escaping @Sendable () async -> Void = {},
                     connectedPolicyOpening: Bool = false, appOwnsAccountObservation: Bool = false) async throws -> Self {
        try #require(!connectedPolicyOpening || !startAdapter,
                     "Connected policy fixtures must start only through policy Start actions")
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let data = directory.appending(path: "collection")
        let backups = directory.appending(path: "backups")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        let location = CoreBridge.CollectionLocation(collectionDirectory: data, backupsDirectory: backups,
                                                     appGroupSnapshot: directory.appending(path: "widget.snapshot"),
                                                     manualTransferDirectory: directory.appending(path: "synthetic-provider-installation"))
        let collection = CoreCollectionService(clock: CloudFixtureClock())
        do {
            // Each host clones the process-wide platform. Supply the provider
            // matching this baseline, regardless of the preceding fixture.
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-u114", environment: .development, status: .available, identity: Data("synthetic-account-A".utf8))
            _ = try await CoreProviderService.migrate(directory: location.manualTransferDirectory,
                legacyJSON: #"{"enabled":true,"paused":false,"automatic":false,"media":false}"#)
            try await MaintenanceLane.shared.bootstrapCollection(location)
            // The existing fixture open supplies the declared R6 binding before
            // Host creation. It starts no SDK; each mode keeps its Start owner.
            let clock = CollectionClockRelay(CloudFixtureClock())
            let diskSpace = DiskSpaceRelay(CoreBridge.DiskSpaceProvider())
            let job = try await Jobs.start(.testCloudStartNativePolicyOpen) {
                try MeguriCoreBindings.testCloudStartNativePolicyOpen(
                    location: location.raw, clock: clock, diskSpace: diskSpace, randomSeed: 114
                )
            }
            try await job.value()
            let baseline = try await MaintenanceLane.shared.prepareCloudAdapterBaseline(
                count: count, savedState: savedState, failInbox: failInbox, failState: failState
            )
            let provider = try await CoreProviderService.attached()
            let providerState = try await provider.refresh()
            try #require(providerState.selection == .iCloud && providerState.phase == .iCloud,
                         "Prebound fixture requires its actual retained binding and settled R6 choice")
            let core = CoreCloudWorkflowService()
            let factory = CloudKitEngineFakeFactory(a23: a23, keepAlive: keepAlive)
            let probe = CloudPersistedEventProbe()
            let adapter = CloudKitSyncAdapter(core: core, engineFactory: factory,
                                             eventDidPersist: { await probe.didPersist($0) },
                                             assetReadTransport: assetReadTransport,
                                             appOwnsAccountObservation: appOwnsAccountObservation,
                                             delegateWorkValidated: delegateWorkValidated)
            if startAdapter {
                try await adapter.start(automaticallySync: false)
                try await adapter.waitForIdle()
            }
            return Self(directory: directory, collection: collection, core: core,
                        adapter: adapter, factory: factory, probe: probe,
                        names: baseline.names, decks: baseline.decks,
                        reviewDeck: baseline.reviewDeck, reviewCard: baseline.reviewCard,
                        reviewName: baseline.reviewName, zone: baseline.zone,
                        incoming: baseline.incoming, root: baseline.root)
        } catch {
            try? await collection.close()
            try? FileManager.default.removeItem(at: directory)
            throw error
        }
    }

    func requireNativePolicyBaseline() async throws {
        try await MaintenanceLane.shared.call(.testCloudRequireNativePolicyBaseline) {
            try MeguriCoreBindings.testCloudRequireNativePolicyBaseline()
        }
    }

    func engine() async throws -> CloudKitEngineFake {
        try #require(await factory.engines().last)
    }

    /// Opt in only when the case starts after accepted complete-epoch metadata.
    /// Keep the shared baseline Unknown; this callback must precede fault arming,
    /// held callbacks and observations of the behavior under test.
    func qualifyCompleteEpoch() async throws {
        let engine = try await engine()
        try await core.validateSDK(engine.instanceID)
        try #require(try await epochAuthority() == nil, "Positive qualification must start with Unknown authority")
        let target = CoreBridge.CloudControlTarget(zone: zone, name: "epoch", recordType: "MGEpoch")
        let bytes = try await boundEpochBytes()
        let native = try NativeCloudOwnRecords.save(target: target, encrypted: bytes, systemFields: nil)
        let record = try NativeCloudRecordConversion.decode(native)
        await engine.emitRetained(.fetched(zone: zone, records: [record], deleted: []), label: "positiveCompleteEpoch")
        try await core.validateSDK(engine.instanceID)
        try #require(try await epochAuthority() != nil, "Positive fixture requires accepted complete-epoch authority")
    }

    func armAdapterFaults(failInbox: Bool, failState: Bool) async throws {
        try await MaintenanceLane.shared.call("test.cloudArmAdapterFaults") {
            try MeguriCoreBindings.testCloudArmAdapterFaults(
                failNextInboxCommit: failInbox, failNextStateCommit: failState
            )
        }
    }

    func close() async {
        try? await adapter.stop()
        try? await collection.close()
        try? FileManager.default.removeItem(at: directory)
    }
}

/// Keep process-wide CoreHost/listener ownership in EventTests' existing
/// serialized suite. Cleanup unpauses BEFORE joining on every throwing path.
func withCloudKitFixture(
    count: UInt32 = 1, a23: Bool = true, keepAlive: Bool = false,
    savedState: Data? = Data([0x73, 0x64, 0x6b]), failInbox: Bool = false, failState: Bool = false,
    startAdapter: Bool = true,
    assetReadTransport: (any CloudOwnTransport)? = nil,
    delegateWorkValidated: @escaping @Sendable () async -> Void = {},
    appOwnsAccountObservation: Bool = false,
    _ body: (CloudKitBridgeFixture) async throws -> Void
) async throws {
    let fixture = try await CloudKitBridgeFixture.open(
        count: count, a23: a23, keepAlive: keepAlive, savedState: savedState,
        failInbox: failInbox, failState: failState, startAdapter: startAdapter,
        assetReadTransport: assetReadTransport,
        delegateWorkValidated: delegateWorkValidated,
        appOwnsAccountObservation: appOwnsAccountObservation
    )
    do { try await body(fixture) }
    catch { await fixture.close(); throw error }
    await fixture.close()
}

/// The native policy owns SDK startup; pure adapter fixtures keep their defaults.
func withNativePolicyFixture(
    a23: Bool = true, keepAlive: Bool = false, assetReadTransport: (any CloudOwnTransport)? = nil,
    appOwnsAccountObservation: Bool = false,
    _ body: (CloudKitBridgeFixture) async throws -> Void
) async throws {
    let fixture = try await CloudKitBridgeFixture.open(a23: a23, keepAlive: keepAlive, startAdapter: false,
        assetReadTransport: assetReadTransport, connectedPolicyOpening: true,
        appOwnsAccountObservation: appOwnsAccountObservation)
    do { try await body(fixture) }
    catch { await fixture.close(); throw error }
    await fixture.close()
}

/// P6/P10.1: accepted metadata opens the running engine's gate only when
/// projected through policy; execute that full update before target observations.
func withCloudKitPendingFixture(
    a23: Bool = true, keepAlive: Bool = false,
    _ body: (CloudKitBridgeFixture) async throws -> Void
) async throws {
    try await withNativePolicyFixture(a23: a23, keepAlive: keepAlive) { f in
        let (policy, executor) = try await nativePolicy(f)
        defer { withExtendedLifetime((policy, executor)) {} }
        try await prepareCloudKitPendingFixture(f, policy: policy, executor: executor)
        try await body(f)
    }
}

func prepareCloudKitPendingFixture(
    _ f: CloudKitBridgeFixture, policy: CoreCloudPolicyService, executor: NativeCloudPolicyExecutor
) async throws {
    let engine = try await f.engine()
    try #require(!engine.automaticallySync)
    try #require(await engine.pendingRecordZoneChanges().isEmpty,
                 "Unknown epoch setup must leave SDK pending empty")
    try await f.qualifyCompleteEpoch()
    let output = try await policy.input(.refresh(.cloud))
    guard case .commands(let commands) = output else {
        throw BridgeError.internal("Expected gate-opening policy commands")
    }
    try #require(commands.filter {
        if case .updatePending(.full) = $0.action { true } else { false }
    }.count == 1, "Complete epoch refresh must open the gate with a full update")
    try #require(!commands.contains {
        switch $0.action {
        case .startEngine, .configureAutomatic, .requestFetch, .requestSend: true
        default: false
        }
    }, "Auto-off gate opening must keep the running SDK without network work")
    let starts = try await consumePolicySetup(output, executor: executor)
    try #require(starts == 0)
    try #require(await f.factory.engines().count == 1)
    try #require(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
    try await f.core.validateSDK(engine.instanceID)
    try #require(!(await engine.pendingRecordZoneChanges()).isEmpty)
    try #require(await engine.pendingRecordZoneChanges() == (try await f.core.trackingSendables()))
    try #require(!(await engine.recordedTrace()).contains {
        switch $0 {
        case .fetchEntered, .sendEntered: true
        default: false
        }
    })
}

private enum CloudTaskCompletion<Value: Sendable>: Sendable {
    case operation(Result<Void, any Error>)
    case body(Result<Value, any Error>)
}

/// Own both tasks until they drain. Non-gate waits must honor cancellation.
/// Nested operations can share cleanup gates without owning each other's
/// expected arrivals. Success after arrival leaves normal body work running.
func withCloudTask<Value: Sendable>(
    gates: [CloudCallbackGate], expectedArrivals: [CloudCallbackGate]? = nil,
    operation: @escaping @Sendable () async throws -> Void,
    whileRunning: @escaping @Sendable () async throws -> Value
) async throws -> Value {
    try await withThrowingTaskGroup(of: CloudTaskCompletion<Value>.self) { group in
        group.addTask {
            do {
                try await operation()
                var missedArrival = false
                for gate in expectedArrivals ?? gates {
                    if !(await gate.operationSucceeded()) { missedArrival = true }
                }
                if missedArrival {
                    let error = CloudCallbackGate.ArrivalError.operationCompletedBeforeArrival
                    for gate in gates { await gate.abort(error) }
                    return .operation(.failure(error))
                }
                return .operation(.success(()))
            } catch {
                // Release callback/SDK pauses BEFORE either child is joined.
                for gate in gates { await gate.abort(error) }
                return .operation(.failure(error))
            }
        }
        group.addTask {
            do { return .body(.success(try await whileRunning())) }
            catch { return .body(.failure(error)) }
        }

        var operationResult: Result<Void, any Error>?
        var bodyResult: Result<Value, any Error>?
        for try await completion in group {
            switch completion {
            case .operation(let result):
                operationResult = result
                if case .failure = result { group.cancelAll() }
            case .body(let result):
                bodyResult = result
                switch result {
                case .success:
                    for gate in gates { await gate.open() }
                case .failure(let error):
                    for gate in gates { await gate.abort(error) }
                    group.cancelAll()
                }
            }
        }
        // Both children have returned. Preserve an operation failure rather
        // than a secondary rendezvous/cancellation failure in the body.
        if case .failure(let error) = operationResult! {
            throw error
        }
        return try bodyResult!.get()
    }
}
