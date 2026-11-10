import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

// The fake supplies SDK facts through the production seam; Core owns pending
// policy, authority, confirmation and all durable outcomes.
//
// Required DTO shapes (all Sendable; value DTOs Equatable):
// CloudPendingChange: save | delete
// CloudPendingUpdate: id: UInt64, kind: full | changes,
//                     adds: [String: CloudPendingChange], removes: Set<String>
// CloudRecord: name: String, envelope: Data, systemFields: Data?
// CloudEngineBatch: id: UInt64, saves: [CloudRecord], deletes: [String]
// CloudRecordFailure: name: String, code: CloudEngineErrorCode
// CloudFetchOptions: zones: [String], priorityZones: [String], groupID: String
// CloudEngineConfiguration: instanceID: String, automaticallySync: Bool,
//                           serialization: Data?
// See events used below for the delegate event DTO's cases. Opaque bytes are
// never decoded by this fake. No credentials or Apple account API is involved.

/// One-shot deterministic rendezvous. Opening also wakes a not-yet-arrived
/// participant. Every test opens its gates before joining tasks on failure.
actor CloudCallbackGate {
    enum ArrivalError: Error { case operationCompletedBeforeArrival }

    private var arrived = false
    private var opened = false
    private var terminalError: (any Error)?
    private var arrivals: [CheckedContinuation<Void, any Error>] = []
    private var releases: [CheckedContinuation<Void, Never>] = []

    func enter() async {
        arrived = true
        let waiting = arrivals
        arrivals.removeAll()
        for waiter in waiting { waiter.resume() }
        if !opened {
            await withCheckedContinuation { releases.append($0) }
        }
    }

    func waitForArrival() async throws {
        if arrived { return }
        if let terminalError { throw terminalError }
        try await withCheckedThrowingContinuation { arrivals.append($0) }
    }

    /// Completion belongs only to this operation's expected arrivals. An
    /// already reached gate remains valid while its body does deferred work.
    func operationSucceeded() -> Bool {
        if !arrived { failArrival(ArrivalError.operationCompletedBeforeArrival) }
        open()
        return arrived
    }

    func abort(_ error: any Error) {
        failArrival(error)
        open()
    }

    private func failArrival(_ error: any Error) {
        terminalError = error
        let waiting = arrivals
        arrivals.removeAll()
        for waiter in waiting { waiter.resume(throwing: error) }
    }

    func pendingWaiterCount() -> Int { arrivals.count + releases.count }

    func open() {
        opened = true
        let waiting = releases
        releases.removeAll()
        for waiter in waiting { waiter.resume() }
    }
}

/// Q3.3/Q3.8 SDK behavior only. A send makes ONE batch request, at most 250
/// records; the core decides subsequent calls and any smaller retry batch.
/// Successful names survive until the sent handler returns. Seven retryable
/// codes are retained; limitExceeded/batchRequestFailed are dropped. The fake
/// never re-adds a name, compares against the core, acknowledges, or halves a
/// batch. The adapter/core must do all of that.
/// Historical name retained for shared tests; counts actual fake SDK entries, never task admission.
final class CloudAcceptedNetworkCalls: Sendable {
    private let count = Mutex(0)
    func record() { count.withLock { $0 += 1 } }
    var value: Int { count.withLock { $0 } }
}

actor CloudKitEngineFake: CloudKitEngine {
    enum SendReply: Sendable {
        case saved
        case failed(CloudEngineErrorCode)
        case conflict(CloudRecord)
    }

    enum FetchScript: Sendable {
        // Manual overall will/did pair; only these zones get per-zone events.
        // An empty fetchedZones models a saved-state fetch with no changes.
        case manual(fetchedZones: [String], records: [CloudRecord], group: String?)
        // Suspend outside the original serial callback, after its will/did
        // handler returned, so an account event can interrupt this same call.
        case manualPaused(fetchedZones: [String], records: [CloudRecord],
                          pauseZone: String, afterDid: Bool, gate: CloudCallbackGate)
        case joined(group: String)
        case shortCircuited
        case missingDid(group: String)
    }

    enum Trace: Sendable, Equatable {
        case sendEntered(Int)
        case fetchEntered(String)
        case fetchOptions(CloudFetchOptions)
        case batchRequested(String)
        case handed(UInt64, [String])
        case eventEntered(String)
        case eventReturned(String)
        case pendingApplied(UInt64, [String], [String])
        case savedRemoved([String])
        case sendReturned(Int)
        case fetchReturned(String)
        case cancelEntered
        case cancelReturned
        case released
        case reentrantNetworkCall
    }

    nonisolated let instanceID: String
    nonisolated let automaticallySync: Bool
    nonisolated let restoredSerialization: Data?
    let keepAlive: Bool
    let a23: Bool
    private var delegate: (any CloudKitEngineDelegate)?
    private var pending: [String: CloudPendingChange] = [:]
    private var pendingDatabase: Set<String> = []
    private var trace: [Trace] = []
    private var sendReplies: [SendReply] = []
    private var fetchScripts: [FetchScript] = []
    private var eventActive = false
    private var eventWaiters: [CheckedContinuation<Void, Never>] = []
    private var eventQueueObservers: [CheckedContinuation<Void, Never>] = []
    private var sendCount = 0
    private var callGates: [CloudCallbackGate] = []
    private var cancelled = false
    private var released = false
    private var callbackPause: CloudCallbackGate?
    private var afterBatch: CloudCallbackGate?
    private var afterSent: CloudCallbackGate?
    private var lateRemoval: CloudCallbackGate?
    private var cancelPause: CloudCallbackGate?
    private var nativeWorkPause: CloudCallbackGate?
    private var operationTaskPause: CloudCallbackGate?
    #if DEBUG
    private var sendFinalEntryObserver: (@Sendable (CloudKitNetworkLifetime.FinalEntryObservation) -> Void)?
    private var networkEventWaitObserver: (@Sendable () -> Void)?
    #endif
    private var networkEventWaiters: [UUID: CheckedContinuation<Void, Never>] = [:]
    private var nextOperationError: (any Error)?
    private let networkLifetime = CloudKitNetworkLifetime()
    nonisolated let acceptedNetworkCalls = CloudAcceptedNetworkCalls()

    init(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate,
         a23: Bool, keepAlive: Bool) {
        instanceID = configuration.instanceID
        automaticallySync = configuration.automaticallySync
        restoredSerialization = configuration.serialization
        self.delegate = delegate
        self.a23 = a23
        self.keepAlive = keepAlive
    }

    func pendingRecordZoneChanges() -> [String: CloudPendingChange] { pending }
    func pendingDatabaseChanges() -> Set<String> { pendingDatabase }

    // Full updates also remove pending database changes (P4). This is an SDK
    // operation, not implementation of the policy's full comparison.
    func applyPendingUpdate(_ update: CloudPendingUpdate) -> CloudPendingUpdate {
        for name in update.removes { pending.removeValue(forKey: name) }
        for (name, change) in update.adds { pending[name] = change }
        trace.append(.pendingApplied(update.id, update.removes.sorted(), update.adds.keys.sorted()))
        return update
    }

    func removePendingDatabaseChanges() { pendingDatabase.removeAll() }
    func seedPendingDatabaseChange(_ name: String) { pendingDatabase.insert(name) }
    func appendSendReplies(_ replies: [SendReply]) { sendReplies.append(contentsOf: replies) }
    func appendFetchScripts(_ scripts: [FetchScript]) { fetchScripts.append(contentsOf: scripts) }
    func pauseNextCallback(_ gate: CloudCallbackGate) { callbackPause = gate }
    func pauseAfterBatch(_ gate: CloudCallbackGate) { afterBatch = gate }
    func pauseAfterSentHandler(_ gate: CloudCallbackGate) { afterSent = gate }
    func pauseLateSavedRemoval(_ gate: CloudCallbackGate) { lateRemoval = gate }
    func pauseCancellation(_ gate: CloudCallbackGate) { cancelPause = gate }
    func recordedTrace() -> [Trace] { trace }

    private func networkEntryAllowed() -> Bool {
        if eventActive {
            trace.append(.reentrantNetworkCall)
            Issue.record("P4/V3.3: sendChanges/fetchChanges entered inside an SDK event")
            return false
        }
        return !cancelled && (!released || keepAlive)
    }

    // Actor reentrancy is not event serialisation. Keep the SDK's serial
    // delivery slot across suspension in the delegate, without sleeps.
    private func deliver(_ event: CloudEngineEvent, label: String, notifyBookkeeping: Bool = true, savedToRemove: [String] = []) async {
        if eventActive {
            await withCheckedContinuation {
                eventWaiters.append($0)
                let observers = eventQueueObservers; eventQueueObservers.removeAll()
                for observer in observers { observer.resume() }
            }
        } else {
            eventActive = true
            networkLifetime.beginEvent()
        }
        trace.append(.eventEntered(label))
        if let delegate { await delegate.eventWillDeliver(engine: self) }
        if let gate = callbackPause {
            callbackPause = nil
            await gate.enter()
        }
        if let delegate { await delegate.handleEvent(event, engine: self) }
        trace.append(.eventReturned(label))
        if a23 { removeSaved(savedToRemove) }
        if let delegate { await delegate.eventHandlerReturned(event, engine: self) }
        // Transfer ownership directly. Clearing eventActive before resuming
        // would let a newly arriving event overtake the oldest queued one.
        if eventWaiters.isEmpty {
            eventActive = false; networkLifetime.endEvent()
            let pending = networkEventWaiters; networkEventWaiters.removeAll()
            for waiter in pending.values { waiter.resume() }
        } else { eventWaiters.removeFirst().resume() }
        if notifyBookkeeping, let delegate { await delegate.eventBookkeepingCompleted(event, engine: self) }
    }

    func waitForQueuedEvent() async {
        if !eventWaiters.isEmpty { return }
        await withCheckedContinuation { eventQueueObservers.append($0) }
    }

    private func waitInCall(_ gate: CloudCallbackGate) async {
        callGates.append(gate)
        await gate.enter()
        callGates.removeAll { $0 === gate }
    }

    func pauseNativeReturnedWork(_ gate: CloudCallbackGate) { nativeWorkPause = gate }
    func pauseNextOperationTask(_ gate: CloudCallbackGate) { operationTaskPause = gate }
    #if DEBUG
    func observeNextSendFinalEntry(_ observer: (@Sendable (CloudKitNetworkLifetime.FinalEntryObservation) -> Void)?) {
        sendFinalEntryObserver = observer
    }
    func observeNextNetworkEventWait(_ observer: (@Sendable () -> Void)?) { networkEventWaitObserver = observer }
    func networkEventWaiterCount() -> Int { networkEventWaiters.count }
    #endif
    func failNextOperation(_ error: any Error) { nextOperationError = error }
    func waitForEventBookkeeping() async throws {
        repeat { try await prepareNativeWork() } while eventActive
        try Task.checkCancellation()
    }
    private func prepareNativeWork() async throws {
        let id = UUID()
        try await withTaskCancellationHandler {
            while eventActive {
                try Task.checkCancellation()
                await withCheckedContinuation {
                    networkEventWaiters[id] = $0
                    #if DEBUG
                    let observer = networkEventWaitObserver; networkEventWaitObserver = nil
                    observer?()
                    #endif
                }
            }
            try Task.checkCancellation()
        } onCancel: {
            Task { await self.cancelNetworkEventWaiter(id) }
        }
        if let gate = nativeWorkPause { nativeWorkPause = nil; await gate.enter() }
    }
    private func cancelNetworkEventWaiter(_ id: UUID) {
        networkEventWaiters.removeValue(forKey: id)?.resume()
    }
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        cancelled = false
        let lifetime = networkLifetime; let generation = lifetime.authority()
        return CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: { await self.cancelSDKCalls() }) {
                try await self.invokePreparedSend(zones: zones, generation: generation, prepare: prepare, validate: validate)
            }
        }
    }
    private func invokePreparedSend(zones: [String], generation: UInt64, prepare: @Sendable () async throws -> Void,
                                    validate: @Sendable () throws -> Void) async throws {
        if let gate = operationTaskPause { operationTaskPause = nil; await waitInCall(gate) }
        #if DEBUG
        if let observer = sendFinalEntryObserver {
            sendFinalEntryObserver = nil
            observer(networkLifetime.finalEntryObservation(generation: generation))
        }
        #endif
        repeat { try await waitForEventBookkeeping() } while eventActive
        try await prepare()
        // Preparation suspends the actor; join events and returned work again.
        repeat { try await waitForEventBookkeeping() } while eventActive
        // The observer stays before preparation can wait for the held event.
        // Idle admission, final validation and this retained fake's entry share
        // the event actor, with no application suspension between them.
        try networkLifetime.enqueue(generation: generation, validate: validate, begin: {})
        try await sendChanges(zones: zones)
    }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        cancelled = false
        let lifetime = networkLifetime; let generation = lifetime.authority()
        return CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: { await self.cancelSDKCalls() }) {
                try await self.invokePreparedFetch(options: options, generation: generation, prepare: prepare, validate: validate)
            }
        }
    }
    private func invokePreparedFetch(options: CloudFetchOptions, generation: UInt64, prepare: @Sendable () async throws -> Void,
                                    validate: @Sendable () throws -> Void) async throws {
        if let gate = operationTaskPause { operationTaskPause = nil; await waitInCall(gate) }
        repeat { try await waitForEventBookkeeping() } while eventActive
        try await prepare()
        // Preparation suspends the actor; join events and returned work again.
        repeat { try await waitForEventBookkeeping() } while eventActive
        try networkLifetime.enqueue(generation: generation, validate: validate, begin: {})
        try await fetchChanges(options: options)
    }
    func sendChanges(zones: [String]) async throws {
        acceptedNetworkCalls.record()
        if let error = nextOperationError { nextOperationError = nil; throw error }
        guard networkEntryAllowed(), let delegate else { return }
        sendCount += 1
        let call = sendCount
        trace.append(.sendEntered(call))
        for zone in zones {
            await deliver(.willSend(zone: zone), label: "willSend")
            trace.append(.batchRequested(instanceID))
            guard let batch = await delegate.nextRecordZoneChangeBatch(
                pending: pending, zone: zone, engine: self
            ) else { continue }
            if zone == "meguri-media" {
                guard let admission = batch.mediaAdmission, try await admission.deliver(batch) != nil else { continue }
            }
            let names = batch.saves.map(\.name) + batch.deletes
            #expect(names.count <= 250)
            trace.append(.handed(batch.id, names))
            if let gate = afterBatch { afterBatch = nil; await waitInCall(gate) }
            if cancelled { trace.append(.sendReturned(call)); throw CancellationError() }
            let reply = sendReplies.isEmpty ? .saved : sendReplies.removeFirst()
            var saved: [CloudRecord] = []
            var deleted: [String] = []
            var failed: [CloudRecordFailure] = []
            switch reply {
            case .saved:
                // Echo the core-produced envelope, adding only opaque SDK
                // system fields. No fake record payload or core ack is made.
                saved = batch.saves.map {
                    CloudRecord(name: $0.name, envelope: $0.envelope,
                                systemFields: Data("sdk-fields-\(batch.id)-\($0.name)".utf8), recordType: $0.recordType, zone: $0.zone)
                }
                deleted = batch.deletes
            case .conflict(let record):
                for handed in batch.saves {
                    if handed.name == record.name {
                        failed.append(.init(name: handed.name, code: .serverRecordChanged, zone: zone,
                                            nativeCode: 14, serverRecord: record))
                        pending.removeValue(forKey: handed.name)
                    } else {
                        saved.append(CloudRecord(name: handed.name, envelope: handed.envelope,
                            systemFields: Data("sdk-fields-\(batch.id)-\(handed.name)".utf8), recordType: handed.recordType, zone: handed.zone))
                    }
                }
            case .failed(let code):
                failed = names.map { CloudRecordFailure(name: $0, code: code) }
                if !Self.sdkRetains(code) {
                    for name in names { pending.removeValue(forKey: name) }
                }
            }
            let deleteNames = Set(batch.deletes)
            let failedDeletes = failed.filter { deleteNames.contains($0.name) }.map {
                CloudRecordFailure(name: $0.name, code: $0.code, zone: zone)
            }
            failed.removeAll { deleteNames.contains($0.name) }
            let sent = CloudEngineEvent.sent(batchID: batch.id, saved: saved, failed: failed, deleted: deleted, failedDeletes: failedDeletes)
            await deliver(sent, label: "sent", notifyBookkeeping: false, savedToRemove: saved.map(\.name) + deleted)
            if let gate = afterSent { afterSent = nil; await waitInCall(gate) }
            if !a23 {
                // A23=false: tests first await the adapter's deferred list,
                // then open this gate so the SDK erases that re-add late.
                if let gate = lateRemoval { lateRemoval = nil; await waitInCall(gate) }
                removeSaved(saved.map(\.name) + deleted)
            }
            await delegate.eventBookkeepingCompleted(sent, engine: self)
            await deliver(.didSend(zone: zone), label: "didSend")
        }
        trace.append(.sendReturned(call))
    }

    private func removeSaved(_ names: [String]) {
        for name in names { pending.removeValue(forKey: name) }
        if !names.isEmpty { trace.append(.savedRemoved(names.sorted())) }
    }

    private static func sdkRetains(_ code: CloudEngineErrorCode) -> Bool {
        switch code {
        case .notAuthenticated, .accountTemporarilyUnavailable, .networkFailure,
             .networkUnavailable, .requestRateLimited, .serviceUnavailable, .zoneBusy: true
        default: false
        }
    }

    func fetchChanges(options: CloudFetchOptions) async throws {
        acceptedNetworkCalls.record()
        if let error = nextOperationError { nextOperationError = nil; throw error }
        guard networkEntryAllowed(), let delegate else { return }
        trace.append(.fetchEntered(options.groupID))
        let scoped = await delegate.nextFetchChangesOptions(options, engine: self)
        trace.append(.fetchOptions(scoped))
        // Tests explicitly script which zones exist/change. This fake has no
        // database, no cursor decoder, and no fabricated no-state listing.
        let script = fetchScripts.isEmpty
            ? FetchScript.manual(fetchedZones: [], records: [], group: nil)
            : fetchScripts.removeFirst()
        switch script {
        case .shortCircuited: break
        case .joined(let group):
            await deliver(.didFetch(groupID: group, reason: .manual), label: "didFetchJoined")
        case .missingDid(let group):
            await deliver(.willFetch(groupID: group, reason: .manual), label: "willFetch")
        case .manual(let zones, let records, let replacement):
            await deliverManualFetch(scoped: scoped, zones: zones, records: records,
                                     group: replacement ?? scoped.groupID)
        case .manualPaused(let zones, let records, let pauseZone, let afterDid, let gate):
            await deliverManualFetch(scoped: scoped, zones: zones, records: records,
                                     group: scoped.groupID, pause: (pauseZone, afterDid, gate))
        }
        trace.append(.fetchReturned(options.groupID))
    }

    private func deliverManualFetch(
        scoped: CloudFetchOptions, zones: [String], records: [CloudRecord], group: String,
        pause: (zone: String, afterDid: Bool, gate: CloudCallbackGate)? = nil
    ) async {
        await deliver(.willFetch(groupID: group, reason: .manual), label: "willFetch")
        for zone in zones where scoped.zones.contains(zone) {
            await deliver(.willFetchZone(zone: zone), label: "willFetchZone")
            if let pause, pause.zone == zone, !pause.afterDid { await waitInCall(pause.gate) }
            let zoneRecords = records.filter { $0.zone == nil || $0.zone == zone }
            if !zoneRecords.isEmpty {
                await deliver(.fetched(zone: zone, records: zoneRecords, deleted: []), label: "fetched")
            }
            await deliver(.didFetchZone(zone: zone, error: nil), label: "didFetchZone")
            if let pause, pause.zone == zone, pause.afterDid { await waitInCall(pause.gate) }
        }
        await deliver(.didFetch(groupID: group, reason: .manual), label: "didFetch")
    }

    func scheduledFetchOptions(_ options: CloudFetchOptions) async -> CloudFetchOptions {
        guard let delegate else { return options }
        return await delegate.nextFetchChangesOptions(options, engine: self)
    }

    func signInSameSyntheticIdentity() async {
        // Q3.3 A12 default clears SDK state on an account event. Even with an
        // unchanged policy guard, the adapter must detect that SDK state is absent.
        // This generic identity is the one used by the fixture binding.
        pending.removeAll()
        pendingDatabase.removeAll()
        await deliver(.accountChanged(change: .signIn, identity: Data("synthetic-account-A".utf8)),
                      label: "sameIdentitySignIn")
    }

    func signInWithUnresolvedIdentity() async {
        pending.removeAll()
        pendingDatabase.removeAll()
        await deliver(.accountChanged(change: .signIn, identity: nil), label: "unresolvedIdentitySignIn")
    }

    func cancelOperations() async {
        let operations = networkLifetime.beginCancellation()
        defer { networkLifetime.endCancellation() }
        await cancelSDKCalls()
        for operation in operations { await operation.cancelAndDrain() }
    }
    private func cancelSDKCalls() async {
        trace.append(.cancelEntered)
        cancelled = true
        // Release in-flight SDK calls even when cancel completion is paused.
        for gate in callGates { await gate.open() }
        if let gate = operationTaskPause { await gate.open() }
        if let gate = afterSent { await gate.open() }
        if let gate = afterBatch { await gate.open() }
        if let gate = lateRemoval { await gate.open() }
        if let gate = cancelPause { cancelPause = nil; await gate.enter() }
        trace.append(.cancelReturned)
    }

    func release() async {
        networkLifetime.retire()
        await cancelOperations()
        released = true
        trace.append(.released)
        if !keepAlive { delegate = nil }
    }

    // Deliberately bypass cancellation for V3.4's retained old SDK object.
    // This retains the ORIGINAL callback object; it never consults the new one.
    func emitRetained(_ event: CloudEngineEvent, label: String) async {
        await deliver(event, label: label)
    }

    func requestRetainedBatch(zone: String) async -> CloudEngineBatch? {
        trace.append(.batchRequested(instanceID))
        return await delegate?.nextRecordZoneChangeBatch(pending: pending, zone: zone, engine: self)
    }
}

actor CloudKitEngineFakeFactory: CloudKitEngineFactory {
    let a23: Bool
    let keepAlive: Bool
    private var made: [CloudKitEngineFake] = []
    private var accountReads: [(Result<CloudResolvedAccount?, any Error>, CloudCallbackGate?)] = []
    private var accountReadCalls = 0
    private var creationPause: CloudCallbackGate?
    private var nextEngineWrapper: (@Sendable (CloudKitEngineFake) -> any CloudKitEngine)?

    init(a23: Bool = true, keepAlive: Bool = false) {
        self.a23 = a23
        self.keepAlive = keepAlive
    }

    func makeEngine(configuration: CloudEngineConfiguration,
                    delegate: any CloudKitEngineDelegate) async -> any CloudKitEngine {
        let engine = CloudKitEngineFake(configuration: configuration, delegate: delegate,
                                        a23: a23, keepAlive: keepAlive)
        made.append(engine)
        if let gate = creationPause { creationPause = nil; await gate.enter() }
        if let wrap = nextEngineWrapper { nextEngineWrapper = nil; return wrap(engine) }
        return engine
    }

    func wrapNextEngine(_ wrap: @escaping @Sendable (CloudKitEngineFake) -> any CloudKitEngine) {
        nextEngineWrapper = wrap
    }

    func appendAccountRead(_ result: Result<CloudResolvedAccount?, any Error>, gate: CloudCallbackGate? = nil) {
        accountReads.append((result, gate))
    }

    func resolveAccount() async throws -> CloudResolvedAccount? {
        accountReadCalls += 1
        guard !accountReads.isEmpty else { return nil }
        let (result, gate) = accountReads.removeFirst()
        if let gate { await gate.enter() }
        return try result.get()
    }

    func pauseNextCreation(_ gate: CloudCallbackGate) { creationPause = gate }

    func engines() -> [CloudKitEngineFake] { made }
    func resolvedAccountReadCount() -> Int { accountReadCalls }
}

/// Model a removed root through the callbacks captured by the native request.
/// This fixture needs no CloudKit login and supplies no Core completion.
final class CloudKitMissingActiveTransport: CloudOwnTransport, NativeCloudDatabaseQueue, Sendable {
    static let active = CloudControlTarget(zone: "meguri-root", name: "active", recordType: "MGActive")
    enum Trace: Sendable, Equatable {
        case prepared(CloudControlTarget), enqueued(CloudControlTarget)
        case unknownItem(CloudControlTarget), replied(CloudControlRead)
    }
    private let trace = Mutex<[Trace]>([])
    func recordedTrace() -> [Trace] { trace.withLock { $0 } }

    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .read(let target) = command else { throw CloudOwnTransportFailure.unexpectedReply }
        trace.withLock { $0.append(.prepared(target)) }
        try #require(target == Self.active)
        let original = try NativeCloudOwnTransport(queue: self).prepare(command)
        return RecordedOperation(original: original, transport: self)
    }

    func add(_ operation: CKDatabaseOperation) -> Bool {
        guard let read = operation as? CKFetchRecordsOperation,
              read.recordIDs == [NativeCloudOwnRecords.id(Self.active)],
              read.desiredKeys == nil,
              let receive = read.perRecordResultBlock,
              let finish = read.fetchRecordsResultBlock else {
            Issue.record("Expected the original full record-ID active read")
            return false
        }
        trace.withLock { $0.append(.enqueued(Self.active)) }
        // Queue admission is synchronous; the real prepared operation buffers
        // this original callback until the guarded submission returns.
        receive(NativeCloudOwnRecords.id(Self.active), .failure(CKError(.unknownItem)))
        finish(.success(()))
        trace.withLock { $0.append(.unknownItem(Self.active)) }
        return true
    }

    private struct RecordedOperation: CloudPreparedOwnOperation {
        let original: any CloudPreparedOwnOperation
        let transport: CloudKitMissingActiveTransport
        // These protocols expose only Bool admission, not the submitting SDK
        // id or guard. Do not label this trace with guessed authority values.
        func enqueue() -> Bool { original.enqueue() }
        func cancel() { original.cancel() }
        func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
        func reply() async throws -> CloudOwnTransportReply {
            let reply = try await original.reply()
            guard case .read(let read) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
            transport.trace.withLock { $0.append(.replied(read)) }
            return reply
        }
    }
}
