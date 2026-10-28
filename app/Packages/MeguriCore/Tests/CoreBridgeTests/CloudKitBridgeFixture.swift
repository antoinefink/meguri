import Foundation
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
    let incoming: CloudRecord
    let root: CloudRecord
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
                                 incoming: CloudRecord(raw.incomingRecord), root: CloudRecord(raw.rootRecord))
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
    let incoming: CloudRecord
    let root: CloudRecord

    static func open(count: UInt32 = 1, a23: Bool = true, keepAlive: Bool = false,
                     savedState: Data? = Data([0x73, 0x64, 0x6b]),
                     failInbox: Bool = false, failState: Bool = false) async throws -> Self {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let data = directory.appending(path: "collection")
        let backups = directory.appending(path: "backups")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        let location = CoreBridge.CollectionLocation(collectionDirectory: data, backupsDirectory: backups,
                                                     appGroupSnapshot: directory.appending(path: "widget.snapshot"))
        let collection = CoreCollectionService(clock: CloudFixtureClock())
        do {
            try await MaintenanceLane.shared.bootstrapCollection(location)
            try await collection.open(location, randomSeed: 114)
            let baseline = try await MaintenanceLane.shared.prepareCloudAdapterBaseline(
                count: count, savedState: savedState, failInbox: failInbox, failState: failState
            )
            let core = CoreCloudWorkflowService()
            let factory = CloudKitEngineFakeFactory(a23: a23, keepAlive: keepAlive)
            let probe = CloudPersistedEventProbe()
            let adapter = CloudKitSyncAdapter(core: core, engineFactory: factory,
                                             eventDidPersist: { await probe.didPersist($0) })
            try await adapter.start(automaticallySync: false)
            try await adapter.waitForIdle()
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

    func engine() async throws -> CloudKitEngineFake {
        try #require(await factory.engines().last)
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
    _ body: (CloudKitBridgeFixture) async throws -> Void
) async throws {
    let fixture = try await CloudKitBridgeFixture.open(
        count: count, a23: a23, keepAlive: keepAlive, savedState: savedState,
        failInbox: failInbox, failState: failState
    )
    do { try await body(fixture) }
    catch { await fixture.close(); throw error }
    await fixture.close()
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
