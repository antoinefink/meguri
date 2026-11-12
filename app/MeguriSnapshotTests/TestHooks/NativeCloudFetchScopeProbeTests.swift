#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)
import AppModel
import CloudKit
@testable import CoreBridge
import CoreFoundation
import Darwin
import Foundation
import Synchronization
import Testing

// Explicit mac-hosted-test selection only; Snapshot skips this suite. This is
// an SDK diagnostic, with no Meguri workflow or signed-lane acceptance authority.
@Suite @MainActor final class NativeCloudFetchScopeProbeTests {
    // Fixed alternate selector. It never enters any scope-probe stage.
    @Test func observeAcknowledgedRecords() async {
        var report = RecordReadReport()
        do {
            guard Bundle.main.bundleIdentifier == "com.antoinefink.meguri.dev",
                  let configuration = CloudBuildConfiguration.fromBundle,
                  case .development = configuration.environment,
                  configuration.schemaConfigured else { throw ScopeProbeFailure.configuration }
            let input = try RecordReadInput.parse(RecordReadInput.readPrivateFile())
            report.slots = input.records.map { _ in RecordReadSlot(error: .invalidResult) }
            // Validate ALL original secure-coded identities before constructing
            // the one bounded exact-ID request. No repair/reconstruction.
            let witnesses = try input.witnesses()
            try Task.checkCancellation()
            let database = CKContainer(identifier: configuration.container).privateCloudDatabase
            report = await RecordReadOperation.run(witnesses, database: database)
            if Task.isCancelled { report.failureCategory = .cancelled }
        } catch {
            report.failureCategory = ScopeProbeFailure.category(error)
        }
        print("MG_SDK_RECORD_READ \(report.json)")
        if !report.completed || report.failureCategory != nil {
            Issue.record("Native acknowledged-record observation refused; see fixed diagnostic report.")
        }
    }

    @Test func acknowledgedRecordInputGuards() throws {
        let valid: [String: Any] = ["version": 1, "collectionZone": "c-" + String(repeating: "a", count: 26),
            "recordType": "MGEntity", "records": [["name": "synthetic", "encrypted": "AQ==", "systemFields": "Ag=="]]]
        func data(_ value: [String: Any]) throws -> Data { try JSONSerialization.data(withJSONObject: value) }
        let parsed = try RecordReadInput.parse(data(valid))
        #expect(parsed.records.count == 1)
        #expect(throws: (any Error).self) { try parsed.witnesses() }
        for (key, value) in [("version", true as Any), ("collectionZone", "c-invalid" as Any),
                             ("recordType", "Other" as Any), ("unknown", 1 as Any),
                             ("records", [] as Any)] {
            var bad = valid
            bad[key] = value
            #expect(throws: (any Error).self) { try RecordReadInput.parse(data(bad)) }
        }
        for encoded in ["", "AQ", "AQ==\n", "AR==", "*"] {
            var bad = valid
            bad["records"] = [["name": "synthetic", "encrypted": encoded, "systemFields": "Ag=="]]
            #expect(throws: (any Error).self) { try RecordReadInput.parse(data(bad)) }
        }
        var duplicate = valid
        duplicate["records"] = Array(repeating: ["name": "synthetic", "encrypted": "AQ==", "systemFields": "Ag=="], count: 2)
        #expect(throws: (any Error).self) { try RecordReadInput.parse(data(duplicate)) }
        #expect(throws: (any Error).self) { try RecordReadInput.parse(Data(repeating: 32, count: 65_537)) }
        // Owned secure-coding helper must reject even a valid archive with the
        // wrong full zone identity, name or record type. No live account needed.
        let target = CloudControlTarget(zone: valid["collectionZone"] as! String, name: "synthetic", recordType: "MGEntity")
        for record in [
            CKRecord(recordType: "Other", recordID: NativeCloudOwnRecords.id(target)),
            CKRecord(recordType: "MGEntity", recordID: .init(recordName: "other", zoneID: NativeCloudOwnRecords.zone(target.zone))),
            CKRecord(recordType: "MGEntity", recordID: .init(recordName: target.name,
                zoneID: .init(zoneName: target.zone, ownerName: "synthetic-other-owner")))
        ] {
            let fields = try NativeCloudKitSystemFields.encode(record)
            #expect(throws: (any Error).self) { try NativeCloudKitSystemFields.restore(fields, target: target) }
        }
    }

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

            phase = .priority
            let priority = await ScopeProbeFetch.run(database: database, state: originalSeedState,
                scope: .zoneIDs([zoneID]), recordID: recordID, expected: Data([2]), prioritizeZone: true)
            report.priority = priority.counts
            try priority.requireSuccess()

            phase = .rebuilt
            let rebuilt = await ScopeProbeFetch.run(database: database, state: originalSeedState,
                scope: .zoneIDs([zoneID]), recordID: recordID, expected: Data([2]),
                prioritizeZone: true, rebuildOptions: true)
            report.rebuilt = rebuilt.counts
            try rebuilt.requireSuccess()

            // Saved-state value2 delivery is an observation, never an SDK assumption.
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
        // CK operations below inspect results at completion and have SDK budgets.
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
    case configuration, create, inventory, save1, seed, save2, limited, wide, priority, rebuilt, fresh, cleanup
}

private enum ScopeProbeFailure: String, Error {
    case configuration, cancelled, sdkFailure, invalidResult, timeout
    case missingExpectedBytes, missingState, incompleteFetch, unexpectedActivity
    case sdkNotAuthenticated, sdkPermissionFailure, sdkMissingEntitlement
    case sdkBadContainer, sdkBadDatabase, sdkNetworkUnavailable, sdkNetworkFailure
    case sdkServiceUnavailable, sdkRequestRateLimited, sdkServerRejectedRequest
    case sdkPartialFailure, sdkZoneNotFound, sdkUnknownItem, sdkInternalError, sdkIncompatibleVersion

    static func category(_ error: any Error) -> Self {
        if let failure = error as? Self { return failure }
        if error is CancellationError { return .cancelled }
        guard let cloud = error as? CKError else { return .sdkFailure }
        switch cloud.code {
        case .operationCancelled: return .cancelled
        case .notAuthenticated: return .sdkNotAuthenticated
        case .permissionFailure: return .sdkPermissionFailure
        case .missingEntitlement: return .sdkMissingEntitlement
        case .badContainer: return .sdkBadContainer
        case .badDatabase: return .sdkBadDatabase
        case .networkUnavailable: return .sdkNetworkUnavailable
        case .networkFailure: return .sdkNetworkFailure
        case .serviceUnavailable: return .sdkServiceUnavailable
        case .requestRateLimited: return .sdkRequestRateLimited
        case .serverRejectedRequest: return .sdkServerRejectedRequest
        case .partialFailure: return .sdkPartialFailure
        case .zoneNotFound: return .sdkZoneNotFound
        case .unknownItem: return .sdkUnknownItem
        case .internalError: return .sdkInternalError
        case .incompatibleVersion: return .sdkIncompatibleVersion
        default: return .sdkFailure
        }
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
    var signIns = 0
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
        "\"callbackErrors\":\(callbackErrors),\"signIns\":\(signIns)," +
        "\"sawExpectedBytes\":\(sawExpectedBytes)," +
        "\"statePresent\":\(statePresent),\"unexpectedActivity\":\(unexpectedActivity)," +
        "\"fetchCompleted\":\(fetchCompleted),\"drained\":\(drained)}"
    }
}

private struct ScopeProbeReport {
    var seed = ScopeProbeCounts()
    var limited = ScopeProbeCounts()
    var wide = ScopeProbeCounts()
    var priority = ScopeProbeCounts()
    var rebuilt = ScopeProbeCounts()
    var fresh = ScopeProbeCounts()
    var failurePhase: ScopeProbePhase?
    var failureCategory: ScopeProbeFailure?
    var cleanupAttempted = false
    var cleanupSucceeded = false
    var cleanupFailure: ScopeProbeFailure?

    // All interpolated strings are closed enums; all observations are counts or
    // booleans. No SDK object, name, serialization or payload reaches output.
    var json: String {
        "{\"seed\":\(seed.json),\"limited\":\(limited.json),\"wide\":\(wide.json)," +
        "\"priority\":\(priority.json),\"rebuilt\":\(rebuilt.json),\"fresh\":\(fresh.json)," +
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
    private let rebuildOptions: Bool
    private var counts = ScopeProbeCounts()
    private var serialization: CKSyncEngine.State.Serialization?

    init(recordID: CKRecord.ID, expected: Data, rebuildOptions: Bool = false) {
        self.recordID = recordID
        self.expected = expected
        self.rebuildOptions = rebuildOptions
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
        case .accountChange(let change):
            switch change.changeType {
            case .signIn:
                counts.signIns += 1
                if counts.signIns > 1 { counts.unexpectedActivity = true }
            case .signOut, .switchAccounts:
                counts.unexpectedActivity = true
            @unknown default:
                counts.unexpectedActivity = true
            }
        case .willSendChanges, .didSendChanges, .sentDatabaseChanges, .sentRecordZoneChanges:
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
        if rebuildOptions {
            var options = CKSyncEngine.FetchChangesOptions(scope: .zoneIDs([recordID.zoneID]),
                                                          operationGroup: context.options.operationGroup)
            options.prioritizedZoneIDs = context.options.prioritizedZoneIDs
            return options
        }
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
                    expected: Data, prioritizeZone: Bool = false,
                    rebuildOptions: Bool = false) async -> ScopeProbeFetchResult {
        let observer = ScopeProbeObserver(recordID: recordID, expected: expected, rebuildOptions: rebuildOptions)
        var configuration = CKSyncEngine.Configuration(database: database, stateSerialization: state, delegate: observer)
        configuration.automaticallySync = false
        let engine = CKSyncEngine(configuration)
        let group = CKOperationGroup()
        group.defaultConfiguration = ScopeProbeOperations.budget()
        var initialOptions = CKSyncEngine.FetchChangesOptions(scope: scope, operationGroup: group)
        if prioritizeZone { initialOptions.prioritizedZoneIDs = [recordID.zoneID] }
        let options = initialOptions
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

// The controller validates duplicate JSON keys and pins the exact immutable
// bytes through execution. Revalidate the closed schema and native identities
// here, in the real development app sandbox, before requesting any record.
private struct RecordReadInput {
    struct Record {
        let name: String
        let encrypted: Data
        let systemFields: Data
    }
    let zone: String
    let records: [Record]

    static func parse(_ data: Data) throws -> Self {
        guard !data.isEmpty, data.count <= 65_536,
              let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(root.keys) == ["version", "collectionZone", "recordType", "records"],
              let version = root["version"] as? NSNumber,
              CFGetTypeID(version) != CFBooleanGetTypeID(), version == 1,
              root["recordType"] as? String == "MGEntity",
              let zone = root["collectionZone"] as? String,
              zone.range(of: "^c-[a-z2-7]{25}[aeimquy4]$", options: .regularExpression) != nil,
              zone.utf8.count == 28,
              let items = root["records"] as? [[String: Any]], (1...8).contains(items.count)
        else { throw ScopeProbeFailure.configuration }
        var names = Set<String>()
        let records = try items.map { item -> Record in
            guard Set(item.keys) == ["name", "encrypted", "systemFields"],
                  let name = item["name"] as? String, (1...255).contains(name.utf8.count),
                  !name.unicodeScalars.contains(where: { $0.value < 32 || $0.value == 127 }),
                  names.insert(name).inserted else { throw ScopeProbeFailure.configuration }
            func decode(_ key: String) throws -> Data {
                guard let text = item[key] as? String, let bytes = Data(base64Encoded: text),
                      !bytes.isEmpty, bytes.count <= 65_536, bytes.base64EncodedString() == text
                else { throw ScopeProbeFailure.configuration }
                return bytes
            }
            return try Record(name: name, encrypted: decode("encrypted"), systemFields: decode("systemFields"))
        }
        return Self(zone: zone, records: records)
    }

    func witnesses() throws -> [RecordReadWitness] {
        try records.map { item in
            let target = CloudControlTarget(zone: zone, name: item.name, recordType: "MGEntity")
            let saved = try NativeCloudKitSystemFields.restore(item.systemFields, target: target)
            guard let tag = saved.recordChangeTag, !tag.isEmpty else { throw ScopeProbeFailure.invalidResult }
            return RecordReadWitness(id: saved.recordID, encrypted: item.encrypted, changeTag: tag)
        }
    }

    static func readPrivateFile() throws -> Data {
        let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                  appropriateFor: nil, create: false)
        let folder = open(support.appendingPathComponent("MeguriDiagnostics", isDirectory: true).path,
                          O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard folder >= 0 else { throw ScopeProbeFailure.configuration }
        defer { close(folder) }
        var directory = stat()
        guard fstat(folder, &directory) == 0, directory.st_uid == getuid(),
              directory.st_mode & 0o777 == 0o700 else { throw ScopeProbeFailure.configuration }
        let fd = openat(folder, "acknowledged-records.json", O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
        guard fd >= 0 else { throw ScopeProbeFailure.configuration }
        let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? file.close() }
        var before = stat()
        guard fstat(fd, &before) == 0, before.st_uid == getuid(), before.st_nlink == 1,
              before.st_mode & S_IFMT == S_IFREG, before.st_mode & 0o777 == 0o600,
              before.st_size > 0, before.st_size <= 65_536 else { throw ScopeProbeFailure.configuration }
        let bytes = try file.read(upToCount: 65_537) ?? Data()
        var after = stat()
        guard fstat(fd, &after) == 0, bytes.count == before.st_size,
              before.st_dev == after.st_dev, before.st_ino == after.st_ino,
              before.st_size == after.st_size, before.st_mode == after.st_mode,
              before.st_nlink == after.st_nlink,
              before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
              before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
              before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
              before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec
        else { throw ScopeProbeFailure.configuration }
        return bytes
    }
}

private struct RecordReadWitness: Sendable {
    let id: CKRecord.ID
    let encrypted: Data
    let changeTag: String
}

private struct RecordReadSlot: Sendable {
    var found = false
    var bytesMatch = false
    var changeTagMatch = false
    var error: ScopeProbeFailure?
    var status: String {
        if found { return bytesMatch && changeTagMatch ? "match" : "different" }
        return error == .sdkUnknownItem ? "missing" : "readError"
    }
    var json: String {
        "{\"found\":\(found),\"bytesMatch\":\(bytesMatch),\"changeTagMatch\":\(changeTagMatch)," +
        "\"status\":\"\(status)\",\"error\":\"\(error?.rawValue ?? "none")\"}"
    }
}

private struct RecordReadReport: Sendable {
    var completed = false
    var failureCategory: ScopeProbeFailure?
    var terminalError: ScopeProbeFailure?
    var slots: [RecordReadSlot] = []
    var json: String {
        "{\"expected\":\(slots.count),\"found\":\(slots.filter(\.found).count)," +
        "\"exactByteMatch\":\(slots.filter(\.bytesMatch).count)," +
        "\"missing\":\(slots.filter { $0.status == "missing" }.count)," +
        "\"different\":\(slots.filter { $0.status == "different" }.count)," +
        "\"readError\":\(slots.filter { $0.status == "readError" }.count)," +
        "\"completed\":\(completed),\"failureCategory\":\"\(failureCategory?.rawValue ?? "none")\"," +
        "\"terminalError\":\"\(terminalError?.rawValue ?? "none")\"," +
        "\"slots\":[\(slots.map(\.json).joined(separator: ","))]}"
    }
}

private enum RecordReadOperation {
    private struct State: Sendable {
        var slots: [Int: RecordReadSlot] = [:]
        var invalid = false
        var terminal: Result<Void, ScopeProbeFailure>?
    }

    static func run(_ witnesses: [RecordReadWitness], database: CKDatabase) async -> RecordReadReport {
        let operation = CKFetchRecordsOperation(recordIDs: witnesses.map(\.id))
        operation.configuration = ScopeProbeOperations.budget()
        // Existing owned exact-read convention includes encrypted e. No query,
        // subscription, engine, state token, retry, or database mutation.
        operation.desiredKeys = nil
        let state = Mutex(State())
        operation.perRecordResultBlock = { id, result in
            state.withLock { value in
                guard let index = witnesses.firstIndex(where: { $0.id == id }), value.slots[index] == nil
                else { value.invalid = true; return }
                let witness = witnesses[index]
                switch result {
                case .success(let record):
                    guard record.recordID == witness.id, record.recordType == "MGEntity",
                          record.recordID.zoneID.ownerName == CKCurrentUserDefaultName
                    else { value.slots[index] = RecordReadSlot(error: .invalidResult); return }
                    value.slots[index] = RecordReadSlot(found: true,
                        bytesMatch: record.encryptedValues["e"] as? Data == witness.encrypted,
                        changeTagMatch: record.recordChangeTag == witness.changeTag)
                case .failure(let error):
                    // Only an original exact per-record unknownItem is missing.
                    value.slots[index] = RecordReadSlot(error: ScopeProbeFailure.category(error))
                }
            }
        }
        operation.fetchRecordsResultBlock = { result in
            state.withLock { $0.terminal = result.mapError { ScopeProbeFailure.category($0) } }
        }
        // Return only from the original CKOperation completion. SDK request and
        // resource budgets are unchanged. Cancellation cancels and still joins.
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                operation.completionBlock = {
                    let report = state.withLock { value -> RecordReadReport in
                        var fallback = ScopeProbeFailure.invalidResult
                        var terminalError: ScopeProbeFailure?
                        if case .failure(let failure) = value.terminal {
                            fallback = failure == .sdkUnknownItem ? .sdkFailure : failure
                            terminalError = failure
                        }
                        let slots = witnesses.indices.map { value.slots[$0] ?? RecordReadSlot(error: fallback) }
                        return RecordReadReport(completed: value.terminal != nil && !value.invalid,
                            failureCategory: value.invalid || value.terminal == nil ? .invalidResult : nil,
                            terminalError: terminalError,
                            slots: slots)
                    }
                    continuation.resume(returning: report)
                }
                if Task.isCancelled { operation.cancel() }
                database.add(operation)
            }
        } onCancel: {
            operation.cancel()
        }
    }
}
#endif
