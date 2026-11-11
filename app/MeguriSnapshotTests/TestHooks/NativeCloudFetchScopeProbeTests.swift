#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)
import AppModel
import CloudKit
import Foundation
import Synchronization
import Testing

// Explicit mac-hosted-test selection only; Snapshot skips this suite. This is
// an SDK diagnostic, with no Meguri workflow or signed-lane acceptance authority.
@Suite final class NativeCloudFetchScopeProbeTests {
    @Test func observeSavedStateFetchScopes() async {
        var report = ScopeProbeReport()
        var phase = ScopeProbePhase.configuration
        var cleanupTarget: (CKDatabase, CKRecordZone.ID)?
        do {
            guard let configuration = CloudBuildConfiguration.fromBundle,
                  case .development = configuration.environment,
                  configuration.schemaConfigured else { throw ScopeProbeFailure.configuration }
            let database = CKContainer(identifier: configuration.container).privateCloudDatabase
            let zoneID = CKRecordZone.ID(zoneName: "mg-sdk-scope-probe-\(UUID().uuidString)",
                                        ownerName: CKCurrentUserDefaultName)
            cleanupTarget = (database, zoneID)
            let recordID = CKRecord.ID(recordName: UUID().uuidString, zoneID: zoneID)

            phase = .create
            try await ScopeProbeOperations.createZone(zoneID, database: database)
            phase = .inventory
            let zones = try await ScopeProbeOperations.listZones(database: database)
            guard zones.contains(zoneID) else { throw ScopeProbeFailure.invalidResult }
            // IDs stay in memory. Exclude every other current zone, including
            // blob zones, while retaining the SDK's database-wide logical scope.
            let wideScope = CKSyncEngine.FetchChangesOptions.Scope.allExcluding(zones.filter { $0 != zoneID })

            phase = .save1
            let record = CKRecord(recordType: "MGEntity", recordID: recordID)
            record.encryptedValues["e"] = Data([1]) as CKRecordValue
            let saved = try await ScopeProbeOperations.save(record, expected: Data([1]), database: database)

            phase = .seed
            let seed = await ScopeProbeFetch.run(database: database, state: nil, scope: wideScope,
                                                recordID: recordID, expected: Data([1]))
            report.seed = seed.counts
            try seed.requireSuccess()
            guard seed.counts.sawExpectedBytes else { throw ScopeProbeFailure.missingExpectedBytes }
            guard let originalSeedState = seed.serialization else { throw ScopeProbeFailure.missingState }

            phase = .save2
            // Reuse the exact original server Saved record, including its
            // system fields/change tag; never reconstruct or edit engine state.
            saved.encryptedValues["e"] = Data([2]) as CKRecordValue
            _ = try await ScopeProbeOperations.save(saved, expected: Data([2]), database: database)

            phase = .limited
            let limited = await ScopeProbeFetch.run(database: database, state: originalSeedState,
                scope: .zoneIDs([zoneID]), recordID: recordID, expected: Data([2]))
            report.limited = limited.counts
            try limited.requireSuccess()

            phase = .wide
            let wide = await ScopeProbeFetch.run(database: database, state: originalSeedState,
                scope: wideScope, recordID: recordID, expected: Data([2]))
            report.wide = wide.counts
            try wide.requireSuccess()

            // A/B value2 delivery is an observation, never an SDK assumption.
            phase = .fresh
            let fresh = await ScopeProbeFetch.run(database: database, state: nil, scope: wideScope,
                                                 recordID: recordID, expected: Data([2]))
            report.fresh = fresh.counts
            try fresh.requireSuccess()
            guard fresh.counts.sawExpectedBytes else { throw ScopeProbeFailure.missingExpectedBytes }
            try Task.checkCancellation()
        } catch {
            report.failurePhase = phase
            report.failureCategory = ScopeProbeFailure.category(error)
        }

        // Every operation above has returned and every original engine has
        // cancel/drained before deletion. Also run this in a cancelled test task:
        // CK operations below join their terminal callback and have SDK budgets.
        if let (database, zoneID) = cleanupTarget {
            report.cleanupAttempted = true
            do {
                try await ScopeProbeOperations.deleteZone(zoneID, database: database)
                report.cleanupSucceeded = true
            } catch {
                report.cleanupFailure = ScopeProbeFailure.category(error)
            }
        }
        if Task.isCancelled && report.failureCategory == nil {
            report.failurePhase = .cleanup
            report.failureCategory = .cancelled
        }
        print("MG_SDK_SCOPE_PROBE \(report.json)")
        if report.failureCategory != nil {
            Issue.record("Native Cloud fetch scope probe failed; see safe diagnostic counts.")
        }
        if report.cleanupAttempted && !report.cleanupSucceeded {
            Issue.record("Native Cloud fetch scope probe cleanup failed.")
        }
    }
}

private enum ScopeProbePhase: String {
    case configuration, create, inventory, save1, seed, save2, limited, wide, fresh, cleanup
}

private enum ScopeProbeFailure: String, Error {
    case configuration, cancelled, sdkFailure, invalidResult, timeout
    case missingExpectedBytes, missingState, incompleteFetch, unexpectedActivity

    static func category(_ error: any Error) -> Self {
        if let failure = error as? Self { return failure }
        if error is CancellationError { return .cancelled }
        if let cloud = error as? CKError, cloud.code == .operationCancelled { return .cancelled }
        return .sdkFailure
    }
}

private struct ScopeProbeCounts: Sendable {
    var willFetch = 0
    var didFetch = 0
    var databaseCallbacks = 0
    var willFetchZone = 0
    var zoneCallbacks = 0
    var didFetchZone = 0
    var generatedRecordModifications = 0
    var fetchOptionsCallbacks = 0
    var batchRequests = 0
    var callbackErrors = 0
    var sawExpectedBytes = false
    var statePresent = false
    var unexpectedActivity = false
    var fetchCompleted = false
    var drained = false

    var json: String {
        "{\"willFetch\":\(willFetch),\"didFetch\":\(didFetch)," +
        "\"databaseCallbacks\":\(databaseCallbacks),\"willFetchZone\":\(willFetchZone)," +
        "\"zoneCallbacks\":\(zoneCallbacks),\"didFetchZone\":\(didFetchZone)," +
        "\"generatedRecordModifications\":\(generatedRecordModifications)," +
        "\"fetchOptionsCallbacks\":\(fetchOptionsCallbacks),\"batchRequests\":\(batchRequests)," +
        "\"callbackErrors\":\(callbackErrors),\"sawExpectedBytes\":\(sawExpectedBytes)," +
        "\"statePresent\":\(statePresent),\"unexpectedActivity\":\(unexpectedActivity)," +
        "\"fetchCompleted\":\(fetchCompleted),\"drained\":\(drained)}"
    }
}

private struct ScopeProbeReport {
    var seed = ScopeProbeCounts()
    var limited = ScopeProbeCounts()
    var wide = ScopeProbeCounts()
    var fresh = ScopeProbeCounts()
    var failurePhase: ScopeProbePhase?
    var failureCategory: ScopeProbeFailure?
    var cleanupAttempted = false
    var cleanupSucceeded = false
    var cleanupFailure: ScopeProbeFailure?

    // All interpolated strings are closed enums; all observations are counts or
    // booleans. No SDK object, name, serialization or payload reaches output.
    var json: String {
        "{\"seed\":\(seed.json),\"limited\":\(limited.json),\"wide\":\(wide.json),\"fresh\":\(fresh.json)," +
        "\"failurePhase\":\"\(failurePhase?.rawValue ?? "none")\"," +
        "\"failureCategory\":\"\(failureCategory?.rawValue ?? "none")\"," +
        "\"cleanupAttempted\":\(cleanupAttempted),\"cleanupSucceeded\":\(cleanupSucceeded)," +
        "\"cleanupFailure\":\"\(cleanupFailure?.rawValue ?? "none")\"}"
    }
}

private struct ScopeProbeFetchResult: Sendable {
    var counts: ScopeProbeCounts
    let serialization: CKSyncEngine.State.Serialization?
    var failure: ScopeProbeFailure?

    func requireSuccess() throws {
        if let failure { throw failure }
        guard counts.fetchCompleted, counts.drained else {
            throw ScopeProbeFailure.incompleteFetch
        }
        guard counts.callbackErrors == 0 else { throw ScopeProbeFailure.sdkFailure }
        guard !counts.unexpectedActivity else {
            throw ScopeProbeFailure.unexpectedActivity
        }
    }
}

// A new observer for each engine; only the generated record's encrypted bytes
// are inspected. There is no adapter, pending work, account lookup or forwarding.
private actor ScopeProbeObserver: CKSyncEngineDelegate {
    private let recordID: CKRecord.ID
    private let expected: Data
    private var counts = ScopeProbeCounts()
    private var serialization: CKSyncEngine.State.Serialization?

    init(recordID: CKRecord.ID, expected: Data) {
        self.recordID = recordID
        self.expected = expected
    }

    func handleEvent(_ event: CKSyncEngine.Event, syncEngine: CKSyncEngine) async {
        switch event {
        case .stateUpdate(let update):
            serialization = update.stateSerialization
            counts.statePresent = true
        case .willFetchChanges:
            counts.willFetch += 1
        case .didFetchChanges:
            counts.didFetch += 1
        case .fetchedDatabaseChanges:
            counts.databaseCallbacks += 1
        case .willFetchRecordZoneChanges(let will):
            counts.willFetchZone += 1
            if will.zoneID != recordID.zoneID { counts.unexpectedActivity = true }
        case .fetchedRecordZoneChanges(let fetched):
            counts.zoneCallbacks += 1
            for modification in fetched.modifications where modification.record.recordID == recordID {
                counts.generatedRecordModifications += 1
                if modification.record.encryptedValues["e"] as? Data == expected {
                    counts.sawExpectedBytes = true
                }
            }
        case .didFetchRecordZoneChanges(let did):
            counts.didFetchZone += 1
            if did.zoneID != recordID.zoneID { counts.unexpectedActivity = true }
            if did.error != nil { counts.callbackErrors += 1 }
        case .accountChange, .willSendChanges, .didSendChanges, .sentDatabaseChanges, .sentRecordZoneChanges:
            counts.unexpectedActivity = true
        default: break
        }
    }

    func nextRecordZoneChangeBatch(_ context: CKSyncEngine.SendChangesContext,
                                   syncEngine: CKSyncEngine) async -> CKSyncEngine.RecordZoneChangeBatch? {
        counts.batchRequests += 1
        return nil
    }

    func nextFetchChangesOptions(_ context: CKSyncEngine.FetchChangesContext,
                                syncEngine: CKSyncEngine) async -> CKSyncEngine.FetchChangesOptions {
        counts.fetchOptionsCallbacks += 1
        return context.options
    }

    func snapshot() -> ScopeProbeFetchResult {
        ScopeProbeFetchResult(counts: counts, serialization: serialization, failure: nil)
    }
}

private enum ScopeProbeFetch {
    private enum Finish: Sendable {
        case completed, failed(ScopeProbeFailure)
    }

    static func run(database: CKDatabase, state: CKSyncEngine.State.Serialization?,
                    scope: CKSyncEngine.FetchChangesOptions.Scope, recordID: CKRecord.ID,
                    expected: Data) async -> ScopeProbeFetchResult {
        let observer = ScopeProbeObserver(recordID: recordID, expected: expected)
        var configuration = CKSyncEngine.Configuration(database: database, stateSerialization: state, delegate: observer)
        configuration.automaticallySync = false
        let engine = CKSyncEngine(configuration)
        let group = CKOperationGroup()
        group.defaultConfiguration = ScopeProbeOperations.budget()
        let options = CKSyncEngine.FetchChangesOptions(scope: scope, operationGroup: group)
        let initiallyEmpty = engine.state.pendingRecordZoneChanges.isEmpty && engine.state.pendingDatabaseChanges.isEmpty

        let finish: Finish
        if Task.isCancelled {
            await engine.cancelOperations()
            finish = .failed(.cancelled)
        } else if !initiallyEmpty {
            await engine.cancelOperations()
            finish = .failed(.unexpectedActivity)
        } else {
            // Structured children: cancel the original SDK, then join BOTH the
            // actual fetch and deadline before returning, including cancellation.
            finish = await withTaskGroup(of: Finish.self) { tasks in
                tasks.addTask {
                    do {
                        try Task.checkCancellation()
                        try await engine.fetchChanges(options)
                        return .completed
                    } catch { return .failed(ScopeProbeFailure.category(error)) }
                }
                tasks.addTask {
                    do {
                        try await Task.sleep(for: .seconds(60))
                        return .failed(.timeout)
                    } catch { return .failed(.cancelled) }
                }
                let first = await tasks.next() ?? .failed(.incompleteFetch)
                tasks.cancelAll()
                await engine.cancelOperations()
                while await tasks.next() != nil {}
                return first
            }
        }
        var result = await observer.snapshot()
        result.counts.drained = true
        switch finish {
        case .completed: result.counts.fetchCompleted = true
        case .failed(let failure): result.failure = failure
        }
        if !engine.state.pendingRecordZoneChanges.isEmpty || !engine.state.pendingDatabaseChanges.isEmpty {
            result.counts.unexpectedActivity = true
        }
        if Task.isCancelled { result.failure = .cancelled }
        return result
    }
}

// Small callback result holder only. Per-item Saved and terminal results must
// both succeed; neither an aggregate success nor a missing callback is Saved.
private final class ScopeProbeItems<Value: Sendable>: Sendable {
    private struct State: Sendable {
        var values: [Value] = []
        var failure: ScopeProbeFailure?
        var terminal: Result<Void, ScopeProbeFailure>?
    }
    private let state = Mutex(State())

    func receive(_ value: Value) { state.withLock { $0.values.append(value) } }
    func fail(_ failure: ScopeProbeFailure) { state.withLock { if $0.failure == nil { $0.failure = failure } } }

    func finish(_ terminal: Result<Void, any Error>) {
        state.withLock { $0.terminal = terminal.mapError { ScopeProbeFailure.category($0) } }
    }

    // Called by CKOperation's completionBlock, after its terminal result block.
    func joined() -> Result<[Value], ScopeProbeFailure> {
        state.withLock { value in
            if let failure = value.failure { return .failure(failure) }
            guard let terminal = value.terminal else { return .failure(.invalidResult) }
            if case .failure(let failure) = terminal { return .failure(failure) }
            return .success(value.values)
        }
    }
}

private enum ScopeProbeOperations {
    static func budget() -> CKOperation.Configuration {
        let configuration = CKOperation.Configuration()
        configuration.timeoutIntervalForRequest = 30
        configuration.timeoutIntervalForResource = 60
        return configuration
    }

    static func createZone(_ id: CKRecordZone.ID, database: CKDatabase) async throws {
        try Task.checkCancellation()
        let operation = CKModifyRecordZonesOperation(recordZonesToSave: [CKRecordZone(zoneID: id)], recordZoneIDsToDelete: nil)
        operation.configuration = budget()
        let items = ScopeProbeItems<CKRecordZone.ID>()
        operation.perRecordZoneSaveBlock = { returnedID, result in
            switch result {
            case .success(let zone):
                if returnedID == id && zone.zoneID == id { items.receive(returnedID) }
                else { items.fail(.invalidResult) }
            case .failure(let error): items.fail(ScopeProbeFailure.category(error))
            }
        }
        operation.modifyRecordZonesResultBlock = { items.finish($0) }
        let result: Result<[CKRecordZone.ID], ScopeProbeFailure> = await withCheckedContinuation { continuation in
            operation.completionBlock = { continuation.resume(returning: items.joined()) }
            database.add(operation)
        }
        guard try result.get() == [id] else { throw ScopeProbeFailure.invalidResult }
        try Task.checkCancellation()
    }

    static func listZones(database: CKDatabase) async throws -> [CKRecordZone.ID] {
        try Task.checkCancellation()
        let operation = CKFetchRecordZonesOperation.fetchAllRecordZonesOperation()
        operation.configuration = budget()
        let items = ScopeProbeItems<CKRecordZone.ID>()
        operation.perRecordZoneResultBlock = { id, result in
            switch result {
            case .success(let zone):
                if zone.zoneID == id { items.receive(id) }
                else { items.fail(.invalidResult) }
            case .failure(let error): items.fail(ScopeProbeFailure.category(error))
            }
        }
        operation.fetchRecordZonesResultBlock = { items.finish($0) }
        let result: Result<[CKRecordZone.ID], ScopeProbeFailure> = await withCheckedContinuation { continuation in
            operation.completionBlock = { continuation.resume(returning: items.joined()) }
            database.add(operation)
        }
        let ids = try result.get()
        guard Set(ids).count == ids.count else { throw ScopeProbeFailure.invalidResult }
        try Task.checkCancellation()
        return ids
    }

    static func save(_ record: CKRecord, expected: Data, database: CKDatabase) async throws -> CKRecord {
        try Task.checkCancellation()
        let id = record.recordID
        let operation = CKModifyRecordsOperation(recordsToSave: [record], recordIDsToDelete: nil)
        operation.configuration = budget()
        operation.savePolicy = .ifServerRecordUnchanged
        operation.isAtomic = true
        let items = ScopeProbeItems<CKRecord>()
        operation.perRecordSaveBlock = { returnedID, result in
            switch result {
            case .success(let saved):
                if returnedID == id && saved.recordID == id && saved.recordType == "MGEntity" &&
                    saved.encryptedValues["e"] as? Data == expected && saved.recordChangeTag != nil {
                    items.receive(saved)
                } else { items.fail(.invalidResult) }
            case .failure(let error): items.fail(ScopeProbeFailure.category(error))
            }
        }
        operation.modifyRecordsResultBlock = { items.finish($0) }
        let result: Result<[CKRecord], ScopeProbeFailure> = await withCheckedContinuation { continuation in
            operation.completionBlock = { continuation.resume(returning: items.joined()) }
            database.add(operation)
        }
        let saved = try result.get()
        guard saved.count == 1, let original = saved.first else { throw ScopeProbeFailure.invalidResult }
        try Task.checkCancellation()
        return original
    }

    static func deleteZone(_ id: CKRecordZone.ID, database: CKDatabase) async throws {
        // Deliberately admit cleanup even if the calling task was cancelled.
        let operation = CKModifyRecordZonesOperation(recordZonesToSave: nil, recordZoneIDsToDelete: [id])
        operation.configuration = budget()
        let items = ScopeProbeItems<CKRecordZone.ID>()
        operation.perRecordZoneDeleteBlock = { returnedID, result in
            switch result {
            case .success:
                if returnedID == id { items.receive(returnedID) }
                else { items.fail(.invalidResult) }
            case .failure(let error): items.fail(ScopeProbeFailure.category(error))
            }
        }
        operation.modifyRecordZonesResultBlock = { items.finish($0) }
        let result: Result<[CKRecordZone.ID], ScopeProbeFailure> = await withCheckedContinuation { continuation in
            operation.completionBlock = { continuation.resume(returning: items.joined()) }
            database.add(operation)
        }
        guard try result.get() == [id] else { throw ScopeProbeFailure.invalidResult }
    }
}
#endif
