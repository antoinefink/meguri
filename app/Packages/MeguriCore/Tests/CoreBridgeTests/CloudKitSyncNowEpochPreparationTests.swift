import CloudKit
import Dispatch
import Foundation
import func MeguriCoreBindings.testCloudRequireUnknownEpochSyncNowBaseline
import Synchronization
import Testing
@testable import CoreBridge

func requireUnknownEpochSyncNowBaseline(_ sdk: String) async throws {
    try await MaintenanceLane.shared.call("test.cloudRequireUnknownEpochSyncNowBaseline") {
        try testCloudRequireUnknownEpochSyncNowBaseline(sdk: sdk)
    }
}

/// Records the actual request and delivers a synthetic SDK error through the
/// original native operation's callbacks. Supplies no Core exchange outcome.
private final class SyncNowEpochFailureTransport: CloudOwnTransport, NativeCloudDatabaseQueue, Sendable {
    private struct State {
        var target: CloudControlTarget?
        var engine: CloudKitEngineFake?
        var prepared: [CloudControlTarget] = []
        var enqueued: [CloudControlTarget] = []
        var replies: [CloudControlRead] = []
    }
    private let state = Mutex(State())
    var prepared: [CloudControlTarget] { state.withLock { $0.prepared } }
    var enqueued: [CloudControlTarget] { state.withLock { $0.enqueued } }
    var replies: [CloudControlRead] { state.withLock { $0.replies } }

    func bind(target: CloudControlTarget, engine: CloudKitEngineFake) {
        state.withLock { $0.target = target; $0.engine = engine }
    }

    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .read(let target) = command else { throw CloudOwnTransportFailure.unexpectedReply }
        let expected = state.withLock { value in
            value.prepared.append(target)
            return value.target
        }
        try #require(target == expected, "Sync Now must request the bound MGEpoch directly")
        return Operation(original: try NativeCloudOwnTransport(queue: self).prepare(command), owner: self)
    }

    func add(_ operation: CKDatabaseOperation) -> Bool {
        let (target, engine) = state.withLock { ($0.target, $0.engine) }
        guard let target, let engine,
              let read = operation as? CKFetchRecordsOperation,
              read.recordIDs == [NativeCloudOwnRecords.id(target)],
              read.desiredKeys == nil,
              let receive = read.perRecordResultBlock,
              let finish = read.fetchRecordsResultBlock else {
            Issue.record("Expected the original all-fields bound-epoch record-ID read")
            return false
        }
        #expect(engine.acceptedNetworkCalls.value == 0, "No SDK fetch/send may qualify epoch preparation")
        state.withLock { $0.enqueued.append(target) }
        receive(NativeCloudOwnRecords.id(target), .failure(CKError(.networkFailure)))
        finish(.success(()))
        return true
    }

    private struct Operation: CloudPreparedOwnOperation {
        let original: any CloudPreparedOwnOperation
        let owner: SyncNowEpochFailureTransport
        func enqueue() -> Bool { original.enqueue() }
        func cancel() { original.cancel() }
        func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
        func reply() async throws -> CloudOwnTransportReply {
            let engine = try #require(owner.state.withLock { $0.engine })
            // The original read is queued, but its result has not reached Core:
            // authority must still be Unknown and no boundary may be captured.
            try await requireUnknownEpochSyncNowBaseline(engine.instanceID)
            let reply = try await original.reply()
            guard case .read(let result) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
            #expect(result == .failed)
            #expect(original.controlReadEvidence()?.failure?.code == Int64(CKError.Code.networkFailure.rawValue))
            owner.state.withLock { $0.replies.append(result) }
            return reply
        }
    }
}

extension EventTests {
    /// APPROVED1176651a: P6 Unknown, P12.3 preparation and V26.1.
    /// A classified failure is sufficient to witness the missing first read;
    /// successful authority, mismatch and cancellation belong with the fix.
    @Test func cloudKitSyncNowUnknownEpochReadsBoundControlBeforeCapture() async throws {
        let transport = SyncNowEpochFailureTransport()
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let originalSDK = try await f.engine()
            #expect(!originalSDK.automaticallySync)
            try await f.core.validateSDK(originalSDK.instanceID)
            try await requireUnknownEpochSyncNowBaseline(originalSDK.instanceID)
            let target = CloudControlTarget(zone: f.zone, name: "epoch", recordType: "MGEpoch")
            transport.bind(target: target, engine: originalSDK)
            let before = try await f.core.durableSnapshot()
            let drains = try await f.core.fetchDrains()
            #expect(originalSDK.acceptedNetworkCalls.value == 0)
            #expect(transport.prepared.isEmpty && transport.enqueued.isEmpty)

            let outcome: Result<CoreBridge.CloudExchangeResult, any Error>
            do { outcome = .success(try await f.adapter.syncNow()) }
            catch { outcome = .failure(error) }

            // Regression check: one epoch read must run before the exchange.
            #expect(transport.enqueued.count == 1, "Sync Now outcome: \(outcome)")
            #expect(transport.prepared == [target])
            #expect(transport.enqueued == [target])
            #expect(transport.replies == [.failed])
            #expect(originalSDK.acceptedNetworkCalls.value == 0)
            #expect(try await f.core.fetchDrains() == drains)
            #expect(try await f.core.durableSnapshot() == before)
            try await requireUnknownEpochSyncNowBaseline(originalSDK.instanceID)
        }
    }
}

/// SDK callbacks remain the sole producer of records/errors. The gate holds
/// the original reply before Core acceptance, and cancellation never opens it.
enum EpochPreparationNativeMalformedShape: CaseIterable, Sendable { case recordType, encryptedField }

final class EpochPreparationTransport: CloudOwnTransport, NativeCloudDatabaseQueue, Sendable {
    enum Reply: Sendable { case found(Data?), failure(CKError.Code), malformed(EpochPreparationNativeMalformedShape) }
    private struct State {
        var reply: Reply = .failure(.networkFailure)
        var engine: CloudKitEngineFake?
        var root: Data?
        var preparation: [CloudControlTarget] = []
        var later: [CloudControlTarget] = []
    }
    private let state = Mutex(State())
    let gate: CloudCallbackGate?
    init(gate: CloudCallbackGate? = nil) { self.gate = gate }
    func bind(_ engine: CloudKitEngineFake, reply: Reply, root: Data) {
        state.withLock { $0.engine = engine; $0.reply = reply; $0.root = root }
    }
    var preparation: [CloudControlTarget] { state.withLock { $0.preparation } }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .read = command else { throw CloudOwnTransportFailure.unexpectedReply }
        let preparation = state.withLock { $0.engine?.acceptedNetworkCalls.value == 0 }
        return Operation(original: try NativeCloudOwnTransport(queue: self).prepare(command), owner: self, preparation: preparation)
    }
    func add(_ operation: CKDatabaseOperation) -> Bool {
        guard let read = operation as? CKFetchRecordsOperation, let ids = read.recordIDs, ids.count == 1,
              let id = ids.first, read.desiredKeys == nil,
              let receive = read.perRecordResultBlock, let finish = read.fetchRecordsResultBlock else { return false }
        let target = CloudControlTarget(zone: id.zoneID.zoneName, name: id.recordName,
            recordType: id.recordName == "active" ? "MGActive" : "MGEpoch")
        let reply = state.withLock { value -> Reply in
            if value.engine?.acceptedNetworkCalls.value == 0 { value.preparation.append(target) }
            else { value.later.append(target) }
            return target.name == "active" ? .found(value.root) : value.reply
        }
        switch reply {
        case .found(let bytes):
            let record = CKRecord(recordType: target.recordType, recordID: id)
            if let bytes { record.encryptedValues["e"] = bytes as CKRecordValue }
            receive(id, .success(record))
        case .failure(let code): receive(id, .failure(CKError(code)))
        case .malformed(let shape):
            let record = CKRecord(recordType: shape == .recordType ? "MGActive" : target.recordType, recordID: id)
            if shape == .encryptedField { record.encryptedValues["e"] = "synthetic-wrong-field-type" as CKRecordValue }
            receive(id, .success(record))
        }
        finish(.success(()))
        return true
    }
    private struct Operation: CloudPreparedOwnOperation {
        let original: any CloudPreparedOwnOperation
        let owner: EpochPreparationTransport
        let preparation: Bool
        func enqueue() -> Bool { original.enqueue() }
        func cancel() { original.cancel() }
        func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
        func reply() async throws -> CloudOwnTransportReply {
            let reply: Result<CloudOwnTransportReply, any Error>
            do { reply = .success(try await original.reply()) }
            catch { reply = .failure(error) }
            if preparation, let gate = owner.gate { await gate.enter() }
            return try reply.get()
        }
    }
}

enum EpochPreparationRefusal: CaseIterable, Sendable {
    case unequal, publishing, absentEncryptedField, invalid, futureFormat, futureSchema, missingEpoch, missingZone, network, quota
}

extension EventTests {
    @Test(arguments: [false, true])
    func cloudKitSyncNowEpochPreparationCommitsEqualBeforeOrdinaryExchange(automatic: Bool) async throws {
        let transport = EpochPreparationTransport()
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            if automatic { try await f.adapter.configureAutomatic(true) }
            let engine = try await f.engine()
            let bytes = try await boundEpochBytes()
            transport.bind(engine, reply: .found(bytes), root: f.root.envelope)
            #expect(try await epochAuthority() == nil)
            #expect(engine.acceptedNetworkCalls.value == 0)
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", f.zone], records: [f.root], group: nil),
                .manual(fetchedZones: ["meguri-root", f.zone], records: [], group: nil),
            ])
            let result = try await f.adapter.syncNow()
            #expect(result.collection == .succeeded)
            #expect(!result.selectedNames.isEmpty && result.outstandingNames.isEmpty)
            #expect(try await epochAuthority() != nil)
            #expect(transport.preparation == [.init(zone: f.zone, name: "epoch", recordType: "MGEpoch")])
            // Known authority skips preparation; later exchange control reads
            // are counted separately by the SDK's actual network phase.
            _ = try await f.adapter.syncNow()
            #expect(transport.preparation.count == 1)
        }
    }

    @Test(arguments: EpochPreparationRefusal.allCases)
    func cloudKitSyncNowEpochPreparationRefusesBeforeCapture(kind: EpochPreparationRefusal) async throws {
        let transport = EpochPreparationTransport()
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            let expected: CloudPolicyEarlyEnd
            let reply: EpochPreparationTransport.Reply
            switch kind {
            case .unequal: reply = .found(try await boundEpochBytes(delta: 1)); expected = .failed(.conflictRecovery)
            case .publishing: reply = .found(try await boundEpochBytes(publishing: true)); expected = .waiting(nil, retryAfterMilliseconds: nil)
            case .absentEncryptedField: reply = .found(nil); expected = .waiting(nil, retryAfterMilliseconds: nil)
            case .invalid: reply = .found(Data([255])); expected = .failed(.conflictRecovery)
            case .futureFormat: reply = .found(Data([8, 2])); expected = .failed(.unsupportedSchema)
            case .futureSchema:
                var bytes = try await boundEpochBytes()
                let schema = try #require(bytes.range(of: Data([40, 18])))
                bytes.replaceSubrange(schema, with: Data([40, 19]))
                reply = .found(bytes); expected = .failed(.unsupportedSchema)
            case .missingEpoch: reply = .failure(.unknownItem); expected = .failed(.conflictRecovery)
            case .missingZone: reply = .failure(.zoneNotFound); expected = .failed(.conflictRecovery)
            case .network: reply = .failure(.networkFailure); expected = .waiting(.offline, retryAfterMilliseconds: nil)
            case .quota: reply = .failure(.quotaExceeded); expected = .failed(.quotaFull)
            }
            transport.bind(engine, reply: reply, root: f.root.envelope)
            let before = try await f.core.durableSnapshot()
            let drains = try await f.core.fetchDrains()
            if kind == .missingEpoch || kind == .missingZone {
                for _ in 0..<2 {
                    await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
                }
            }
            do { _ = try await f.adapter.syncNow(); Issue.record("Refused preparation captured an exchange") }
            catch let error as CloudEpochPreparationError {
                switch (error.outcome, expected) {
                case (.failed(let actual), .failed(let wanted)): #expect(actual == wanted)
                case (.waiting(let actual, let retry), .waiting(let wanted, _)): #expect(actual == wanted && retry == nil)
                default: Issue.record("Wrong preparation outcome")
                }
            }
            #expect(engine.acceptedNetworkCalls.value == 0)
            #expect(try await f.core.fetchDrains() == drains)
            #expect(transport.preparation.first == .init(zone: f.zone, name: "epoch", recordType: "MGEpoch"))
            #expect((try await epochAuthority() != nil) == (kind == .unequal))
            if kind != .unequal && kind != .missingEpoch && kind != .missingZone {
                #expect(try await f.core.durableSnapshot() == before)
                try await requireUnknownEpochSyncNowBaseline(engine.instanceID)
            }
        }
    }

    @Test func cloudKitSyncNowEpochPreparationCancellationJoinsOriginalReply() async throws {
        let gate = CloudCallbackGate(), transport = EpochPreparationTransport(gate: gate)
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(try await boundEpochBytes()), root: f.root.envelope)
            let before = try await f.core.durableSnapshot()
            try await withCloudTask(gates: [gate], operation: {
                do { _ = try await f.adapter.syncNow(); Issue.record("Cancelled preparation captured") }
                catch let error as CloudEpochPreparationError {
                    guard case .cancelled = error.outcome else { throw error }
                }
            }, whileRunning: {
                try await gate.waitForArrival()
                await f.adapter.cancelSyncNow()
                #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
                #expect(try await f.core.durableSnapshot() == before)
                #expect(try await epochAuthority() == nil)
                #expect(engine.acceptedNetworkCalls.value == 0)
                await gate.open()
            })
            #expect(transport.preparation.count == 1)
            #expect(try await f.core.durableSnapshot() == before)
            try await requireUnknownEpochSyncNowBaseline(engine.instanceID)
        }
    }
}

private actor EpochPreparationJoiners {
    private var count = 0
    private let joined = CloudCallbackGate()
    func didJoin() async { count += 1; if count == 2 { await joined.open() } }
    func waitForTwo() async { await joined.enter() }
}

extension EventTests {
    @Test func cloudKitSyncNowEpochPreparationJoinersShareReadAndCancellation() async throws {
        let gate = CloudCallbackGate(), transport = EpochPreparationTransport(gate: gate)
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            try await f.adapter.stop()
            let joins = EpochPreparationJoiners()
            let adapter = CloudKitSyncAdapter(core: f.core, engineFactory: f.factory,
                assetReadTransport: transport, exchangeDidJoin: { await joins.didJoin() })
            do {
                try await adapter.start(automaticallySync: false)
                let engine = try await f.engine()
                transport.bind(engine, reply: .found(try await boundEpochBytes()), root: f.root.envelope)
                try await withCloudTask(gates: [gate], operation: {
                    try await withThrowingTaskGroup(of: Void.self) { group in
                        for _ in 0..<2 {
                            group.addTask {
                                do { _ = try await adapter.syncNow(); Issue.record("Joined preparation captured after cancel") }
                                catch let error as CloudEpochPreparationError {
                                    guard case .cancelled = error.outcome else { throw error }
                                }
                            }
                        }
                        try await group.waitForAll()
                    }
                }, whileRunning: {
                    await joins.waitForTwo()
                    try await gate.waitForArrival()
                    #expect(transport.preparation.count == 1)
                    await adapter.cancelSyncNow()
                    #expect(try await epochAuthority() == nil)
                    await gate.open()
                })
                #expect(transport.preparation.count == 1 && engine.acceptedNetworkCalls.value == 0)
                #expect(try await epochAuthority() == nil)
                try await adapter.stop()
            } catch {
                await gate.open(); await adapter.cancelSyncNow(); try? await adapter.stop()
                throw error
            }
        }
    }

    @Test func cloudKitSyncNowEpochPreparationTrackingFailureKeepsAuthorityUnknown() async throws {
        let transport = EpochPreparationTransport()
        try await withCloudKitFixture(failInbox: true, assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(try await boundEpochBytes()), root: f.root.envelope)
            let drains = try await f.core.fetchDrains()
            do { _ = try await f.adapter.syncNow(); Issue.record("Failed authority commit captured") }
            catch let error as CloudEpochPreparationError {
                guard case .failed(.localStorageFull) = error.outcome else { throw error }
            }
            #expect(try await epochAuthority() == nil)
            #expect((try await f.core.durableSnapshot()).trackingStopped)
            #expect(try await f.core.fetchDrains() == drains)
            #expect(engine.acceptedNetworkCalls.value == 0 && transport.preparation.count == 1)
        }
    }
}

extension EventTests {
    @Test func cloudKitSyncNowEpochPreparationStaleOriginalCannotCommitAuthority() async throws {
        let gate = CloudCallbackGate(), transport = EpochPreparationTransport(gate: gate)
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(try await boundEpochBytes()), root: f.root.envelope)
            let drains = try await f.core.fetchDrains()
            try await withCloudTask(gates: [gate], operation: {
                do { _ = try await f.adapter.syncNow(); Issue.record("Stale original accepted authority") }
                catch let error as CloudEpochPreparationError {
                    guard case .cancelled = error.outcome else { throw error }
                }
            }, whileRunning: {
                try await gate.waitForArrival()
                try await f.core.retireSDK(engine.instanceID, policyStop: false)
                await gate.open()
            })
            #expect(try await epochAuthority() == nil)
            #expect(try await f.core.fetchDrains() == drains)
            #expect(transport.preparation.count == 1 && engine.acceptedNetworkCalls.value == 0)
        }
    }
}

/// A real writer-owned session and answer, never a projected SessionActive fact.
func startEpochPreparationReview(_ f: CloudKitBridgeFixture) async throws -> UInt64 {
    if try await Core.fsrsPolicyStatus() != .ok {
        let enabled = try await Core.enableFsrs(); try await enabled.value()
    }
    guard case .started(let session, _, let head, _) = try await Core.startSession(deckID: f.reviewDeck) else {
        throw BridgeError.internal("Real review session did not start")
    }
    do {
        let card = try #require(head.items.first)
        guard case .accepted = try await Core.answerSession(sessionID: session, cardID: card.cardId, rating: .good,
            stateToken: card.stateToken, answeredAtMs: cloudFixtureOriginMilliseconds, elapsedMs: 100) else {
            throw BridgeError.internal("Real review answer did not commit")
        }
        try await Core.flushReviewForBackground()
        return session
    } catch { try? await Core.endSession(sessionID: session); throw error }
}

extension EventTests {
    @Test func cloudKitSyncNowEpochPreparationSurvivesRealReviewStart() async throws {
        let held = CloudCallbackGate(), transport = EpochPreparationTransport(gate: held)
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(try await boundEpochBytes()), root: f.root.envelope)
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", f.zone], records: [f.root], group: nil),
                .manual(fetchedZones: ["meguri-root", f.zone], records: [], group: nil),
            ])
            let session = Mutex<UInt64?>(nil)
            do {
                try await withCloudTask(gates: [held], operation: {
                    let result = try await f.adapter.syncNow()
                    #expect(result.collection == .incomplete)
                    #expect(result.selectedNames.contains(f.reviewName))
                    #expect(result.outstandingNames.contains(f.reviewName))
                    #expect(!result.ackedNames.contains(f.reviewName))
                    #expect(try await epochAuthority() != nil)
                    #expect(!(await engine.recordedTrace()).contains {
                        if case .handed(_, let names) = $0 { names.contains(f.reviewName) } else { false }
                    })
                }, whileRunning: {
                    try await held.waitForArrival()
                    let started = try await startEpochPreparationReview(f); session.withLock { $0 = started }
                    try await requireUnknownEpochSyncNowBaseline(engine.instanceID)
                    #expect(engine.acceptedNetworkCalls.value == 0)
                    // The NEW direct request is still forbidden by P12.1.
                    do { _ = try await f.core.syncNowRequest(sdk: engine.instanceID); Issue.record("New request admitted during Review") }
                    catch { /* the accepted request above is the sole owner */ }
                    await held.open()
                })
                if let session = session.withLock({ $0 }) { try await Core.endSession(sessionID: session) }
            } catch {
                await held.open()
                if let session = session.withLock({ $0 }) { try? await Core.endSession(sessionID: session) }
                throw error
            }
        }
    }
}

#if DEBUG
extension EventTests {
    @Test(arguments: EpochPreparationNativeMalformedShape.allCases, [false, true])
    func cloudKitSyncNowEpochPreparationNativeMalformedAcceptanceFences(
        shape: EpochPreparationNativeMalformedShape, cancelBeforeAcceptance: Bool
    ) async throws {
        let held = CloudCallbackGate()
        let transport = EpochPreparationTransport(gate: cancelBeforeAcceptance ? held : nil)
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .malformed(shape), root: f.root.envelope)
            let before = try await f.core.durableSnapshot()
            let drains = try await f.core.fetchDrains()
            if !cancelBeforeAcceptance { await f.adapter.setEpochAcceptanceObserver { await held.enter() } }
            do {
                try await withCloudTask(gates: [held], operation: {
                    do { _ = try await f.adapter.syncNow(); Issue.record("Native malformed reply captured an exchange") }
                    catch let error as CloudEpochPreparationError {
                        if cancelBeforeAcceptance { guard case .cancelled = error.outcome else { throw error } }
                        else { guard case .failed(.conflictRecovery) = error.outcome else { throw error } }
                        guard let native = error.original as? NativeCloudKitFailure,
                              case .unsupportedRecord = native else {
                            Issue.record("Expected the original native record-shape refusal")
                            return
                        }
                    }
                }, whileRunning: {
                    try await held.waitForArrival()
                    try await requireUnknownEpochSyncNowBaseline(engine.instanceID)
                    #expect(transport.preparation == [.init(zone: f.zone, name: "epoch", recordType: "MGEpoch")])
                    #expect(engine.acceptedNetworkCalls.value == 0)
                    await f.adapter.cancelSyncNow()
                    await held.open()
                })
                await f.adapter.setEpochAcceptanceObserver(nil)
            } catch {
                await held.open(); await f.adapter.setEpochAcceptanceObserver(nil)
                throw error
            }
            try await requireUnknownEpochSyncNowBaseline(engine.instanceID)
            #expect(try await epochAuthority() == nil)
            #expect(try await f.core.durableSnapshot() == before)
            #expect(try await f.core.fetchDrains() == drains)
            #expect(engine.acceptedNetworkCalls.value == 0 && transport.preparation.count == 1)
        }
    }

    @Test(arguments: [false, true])
    func cloudKitSyncNowEpochPreparationAcceptedRefusalOutranksCancellation(unsupported: Bool) async throws {
        let accepted = CloudCallbackGate(), transport = EpochPreparationTransport()
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(Data(unsupported ? [8, 2] : [255])), root: f.root.envelope)
            await f.adapter.setEpochAcceptanceObserver { await accepted.enter() }
            try await withCloudTask(gates: [accepted], operation: {
                do { _ = try await f.adapter.syncNow(); Issue.record("Accepted K20 refusal continued") }
                catch let error as CloudEpochPreparationError {
                    if unsupported { guard case .failed(.unsupportedSchema) = error.outcome else { throw error } }
                    else { guard case .failed(.conflictRecovery) = error.outcome else { throw error } }
                }
            }, whileRunning: {
                try await accepted.waitForArrival()
                #expect(try await epochAuthority() == nil && engine.acceptedNetworkCalls.value == 0)
                await f.adapter.cancelSyncNow()
                await accepted.open()
            })
            await f.adapter.setEpochAcceptanceObserver(nil)
        }
    }

    @Test(arguments: [false, true])
    func cloudKitSyncNowEpochPreparationDiscardedRefusalStaysCancelled(unsupported: Bool) async throws {
        let held = CloudCallbackGate(), transport = EpochPreparationTransport(gate: held)
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(Data(unsupported ? [8, 2] : [255])), root: f.root.envelope)
            try await withCloudTask(gates: [held], operation: {
                do { _ = try await f.adapter.syncNow(); Issue.record("Discarded refusal continued") }
                catch let error as CloudEpochPreparationError {
                    guard case .cancelled = error.outcome else { throw error }
                }
            }, whileRunning: {
                try await held.waitForArrival()
                await f.adapter.cancelSyncNow()
                await held.open()
            })
            try await requireUnknownEpochSyncNowBaseline(engine.instanceID)
            #expect(engine.acceptedNetworkCalls.value == 0)
        }
    }

    @Test(arguments: [false, true])
    func cloudKitSyncNowEpochPreparationCancellationDoesNotWaitForCoreWork(completion: Bool) async throws {
        let entered = CloudCallbackGate(), release = DispatchSemaphore(value: 0)
        let transport = EpochPreparationTransport()
        let once = Mutex(false)
        let observe: @Sendable () -> Void = {
            let first = once.withLock { value in let first = !value; value = true; return first }
            if first { Task { await entered.enter() }; release.wait() }
        }
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(try await boundEpochBytes()), root: f.root.envelope)
            try await withCloudTask(gates: [entered], operation: {
                let work: @Sendable () async throws -> Void = {
                    do { _ = try await f.adapter.syncNow(); Issue.record("Cancelled Core work continued") }
                    catch let error as CloudEpochPreparationError {
                        guard case .cancelled = error.outcome else { throw error }
                    }
                }
                if completion {
                    try await CloudFetchReadWorkObservation.$beforeCompletion.withValue(observe) { try await work() }
                } else {
                    try await CloudFetchReadWorkObservation.$beforeValidation.withValue(observe) { try await work() }
                }
            }, whileRunning: {
                defer { release.signal() }
                try await entered.waitForArrival()
                // This must return while maintenance remains held. The original
                // callback and Core completion still join after the release.
                await f.adapter.cancelSyncNow()
                await entered.open()
            })
            #expect(try await epochAuthority() == nil && engine.acceptedNetworkCalls.value == 0)
        }
    }

    @Test func cloudKitSyncNowEpochPreparationSerializesSuspendedScheduledFetchThroughAcceptance() async throws {
        let reservation = CloudCallbackGate(), callback = CloudCallbackGate(), reply = CloudCallbackGate(), accepted = CloudCallbackGate()
        let transport = EpochPreparationTransport(gate: reply)
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            transport.bind(engine, reply: .found(try await boundEpochBytes()), root: f.root.envelope)
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", f.zone], records: [f.root], group: nil),
                .manual(fetchedZones: ["meguri-root", f.zone], records: [], group: nil),
            ])
            await f.adapter.setEpochReservationObserver { await reservation.enter() }
            await f.adapter.setFetchReservationObserver { await callback.enter() }
            await f.adapter.setEpochAcceptanceObserver { await accepted.enter() }
            let scheduled = Task {
                try await reservation.waitForArrival()
                await engine.emitRetained(.willFetch(groupID: "scheduled-preparation", reason: .scheduled), label: "scheduledPreparation")
            }
            do {
                try await withCloudTask(gates: [reservation, callback, reply, accepted], operation: {
                    let result = try await f.adapter.syncNow()
                    #expect(result.collection == .succeeded)
                }, whileRunning: {
                    try await reservation.waitForArrival()
                    try await callback.waitForArrival()
                    // Pause after queued events finish and before fetch ownership is reserved.
                    #expect(transport.preparation.isEmpty)
                    await reservation.open()
                    try await reply.waitForArrival()
                    await callback.open()
                    try await f.adapter.waitForBlockedFetchForTesting(engine.instanceID)
                    #expect(try await epochAuthority() == nil)
                    await reply.open()
                    try await accepted.waitForArrival()
                    // Ownership still excludes the scheduled callback after
                    // original reply AND the real durable acceptance.
                    #expect(try await epochAuthority() != nil)
                    #expect(engine.acceptedNetworkCalls.value == 0)
                    #expect(try await f.core.fetchDrains().isEmpty)
                    #expect(!(await engine.recordedTrace()).contains(.eventReturned("scheduledPreparation")))
                    await accepted.open()
                    try await scheduled.value
                    await engine.emitRetained(.didFetch(groupID: "scheduled-preparation", reason: .scheduled), label: "scheduledPreparationDone")
                })
            } catch {
                await reservation.open(); await callback.open(); await reply.open(); await accepted.open()
                scheduled.cancel(); _ = try? await scheduled.value
                throw error
            }
            await f.adapter.setEpochReservationObserver(nil)
            await f.adapter.setFetchReservationObserver(nil)
            await f.adapter.setEpochAcceptanceObserver(nil)
        }
    }
}
#endif
