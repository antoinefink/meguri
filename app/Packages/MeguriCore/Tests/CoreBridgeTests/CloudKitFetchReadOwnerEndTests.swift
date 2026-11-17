import CloudKit
import Foundation
import func MeguriCoreBindings.testCloudInstalledSdkOwner
import Synchronization
import Testing
@testable import CoreBridge

private struct OwnerEndQueueRejection: Error, CustomStringConvertible {
    enum Phase: String { case originalFetch, successorStartup }
    enum Target: String { case active, epoch, unknown }
    let phase: Phase
    let target: Target
    var description: String { "queueRejected phase=\(phase.rawValue) target=\(target.rawValue)" }
}

/// Original CK callbacks supply SDK facts only. The optional pause is AFTER
/// the native terminal/reply, but BEFORE the adapter can accept that reply.
/// Cancellation deliberately does not open it: the witness owns its release.
private final class OwnerEndReadTransport: CloudOwnTransport, NativeCloudDatabaseQueue, Sendable {
    enum Reply: Sendable { case found(Data), failure(CKError.Code) }
    private struct State {
        var replies: [Reply] = []
        var completedReads: [CloudControlTarget] = []
        var results: [CloudControlRead] = []
        var diagnosticTarget: OwnerEndQueueRejection.Target = .unknown
    }
    private let state = Mutex(State())
    let activeReply: CloudCallbackGate?
    init(activeReply: CloudCallbackGate? = nil) { self.activeReply = activeReply }
    func setReplies(_ replies: [Reply]) { state.withLock { $0.replies = replies } }
    var completedReads: [CloudControlTarget] { state.withLock { $0.completedReads } }
    var results: [CloudControlRead] { state.withLock { $0.results } }
    func queueRejection(phase: OwnerEndQueueRejection.Phase) -> OwnerEndQueueRejection {
        state.withLock { OwnerEndQueueRejection(phase: phase, target: $0.diagnosticTarget) }
    }

    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .read(let target) = command else { throw CloudOwnTransportFailure.unexpectedReply }
        state.withLock { $0.diagnosticTarget = .init(rawValue: target.name) ?? .unknown }
        return Operation(original: try NativeCloudOwnTransport(queue: self).prepare(command),
                         owner: self, active: target.name == "active")
    }
    func add(_ operation: CKDatabaseOperation) -> Bool {
        guard let read = operation as? CKFetchRecordsOperation,
              let ids = read.recordIDs, ids.count == 1, let id = ids.first,
              read.desiredKeys == nil,
              let receive = read.perRecordResultBlock,
              let finish = read.fetchRecordsResultBlock else { return false }
        let target = CloudControlTarget(zone: id.zoneID.zoneName, name: id.recordName,
            recordType: id.recordName == "active" ? "MGActive" : "MGEpoch")
        let reply = state.withLock { value -> Reply? in
            value.replies.isEmpty ? nil : value.replies.removeFirst()
        }
        guard let reply else { return false }
        switch reply {
        case .found(let encrypted):
            let record = CKRecord(recordType: target.recordType, recordID: id)
            record.encryptedValues["e"] = encrypted as CKRecordValue
            receive(id, .success(record))
        case .failure(let code): receive(id, .failure(CKError(code)))
        }
        finish(.success(()))
        state.withLock { $0.completedReads.append(target) }
        return true
    }
    private struct Operation: CloudPreparedOwnOperation {
        let original: any CloudPreparedOwnOperation
        let owner: OwnerEndReadTransport
        let active: Bool
        func enqueue() -> Bool { original.enqueue() }
        func cancel() { original.cancel() }
        func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
        func reply() async throws -> CloudOwnTransportReply {
            let reply = try await original.reply()
            guard case .read(let result) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
            owner.state.withLock { $0.results.append(result) }
            if active, let gate = owner.activeReply { await gate.enter() }
            return reply
        }
    }
}

private enum OwnerEndWitnessError: Error { case deadline }

/// Reuse the existing first-observation decision and gate/task ownership.
/// Expiry is recorded BEFORE any gate opens, so a stop unblocked by cleanup
/// cannot count as a stop that completed while the resolver was held.
/// This is a test watchdog, never an app deadline or retry.
private func withOwnerEndDeadline<Value: Sendable>(
    gates: [CloudCallbackGate],
    _ body: (CloudPolicyReadinessDecision) async throws -> Value
) async throws -> Value {
    let decision = CloudPolicyReadinessDecision()
    let watchdog = Task {
        do { try await Task.sleep(for: .seconds(10)) } catch { return }
        if decision.expire() {
            for gate in gates { await gate.abort(OwnerEndWitnessError.deadline) }
        }
    }
    do {
        let value = try await body(decision)
        for gate in gates { await gate.open() }
        watchdog.cancel()
        await watchdog.value
        return value
    } catch {
        for gate in gates { await gate.abort(error) }
        watchdog.cancel()
        await watchdog.value
        throw error
    }
}

private func ownerEndBindingState(_ snapshot: CloudDurableSnapshot) throws -> [String: String]? {
    let content = try #require(JSONSerialization.jsonObject(with: snapshot.trackingContent) as? [Any])
    let binding = try #require(content.first as? [Any])
    let facts = try #require(binding.last as? [String: Any])
    return facts["state"] as? [String: String]
}

enum FetchReadOwnerEnd: CaseIterable, Sendable {
    case stop, signedOut, accountSwitch, activeDeleted
    var reason: String {
        switch self {
        case .stop: "ZoneMissing"
        case .signedOut: "SignedOut"
        case .accountSwitch: "AccountChanged"
        case .activeDeleted: "ActiveMissing"
        }
    }
}

extension EventTests {
    /// R1 P1; P3/K12.5/V3.4/V26.1/Q11. A real Active CK terminal
    /// precedes the held resolver. Even the known blocking implementation is
    /// released and joined before the stop-ordering assertion is evaluated.
    @Test func cloudKitFetchReadStopCompletesBeforePostTerminalIdentityResolverReturns() async throws {
        let transport = OwnerEndReadTransport()
        let resolver = CloudCallbackGate()
        let creation = CloudCallbackGate()
        try await withCloudKitFixture(keepAlive: true, savedState: nil, assetReadTransport: transport) { f in
            let old = try await f.engine()
            let before = try await f.core.durableSnapshot()
            let drains = try await f.core.fetchDrains()
            transport.setReplies([.found(f.root.envelope)])
            // A late DIFFERENT identity would isolate the replacement if its
            // old read authority survived stop. No outcome is supplied by fake.
            await f.factory.appendAccountRead(.success(.init(status: .available,
                identity: Data("synthetic-account-B".utf8))), gate: resolver)
            await old.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let observation = try await withOwnerEndDeadline(gates: [resolver]) { decision in
                // Do not let fetch completion's operationSucceeded open the
                // resolver: only the body or watchdog may release this hold.
                try await withCloudTask(gates: [resolver, creation], expectedArrivals: [], operation: {
                    // Revocation may surface as cancellation, stale guard or
                    // expired account read. Authority/data below are the oracle.
                    do { try await f.adapter.requestFetch(zones: ["meguri-root"]) }
                    catch CloudOwnTransportFailure.queueRejected {
                        throw transport.queueRejection(phase: .originalFetch)
                    } catch { }
                }, whileRunning: {
                    try await resolver.waitForArrival()
                    let replyFinishedBeforeStop = transport.completedReads.map(\.name) == ["active"]
                    let fetchReturned = await old.recordedTrace().contains {
                        if case .fetchReturned = $0 { true } else { false }
                    }
                    try await f.adapter.stop()
                    let stoppedBeforeRelease = decision.observeReady()
                    let stoppedDrains = try await f.core.fetchDrains()
                    // On RED, the watchdog has already opened the resolver;
                    // still finish the same cleanup and authority observations.
                    await f.factory.appendAccountRead(.success(.init(status: .available,
                        identity: Data("synthetic-account-A".utf8))))
                    await f.factory.pauseNextCreation(creation)
                    // Start a fresh bounded hold after stop: on RED the resolver's
                    // watchdog has already expired. Keep that first decision intact.
                    let replacement = try await withOwnerEndDeadline(gates: [creation]) { _ in
                        try await withCloudTask(gates: [creation], operation: {
                            do { try await f.adapter.start(automaticallySync: false) }
                            catch CloudOwnTransportFailure.queueRejected {
                                throw transport.queueRejection(phase: .successorStartup)
                            }
                        }, whileRunning: {
                            try await creation.waitForArrival()
                            let successor = try await f.engine()
                            let active = CloudRecord(name: f.root.name, envelope: f.root.envelope,
                                systemFields: f.root.systemFields, recordType: f.root.recordType,
                                zone: "meguri-root", assetURL: f.root.assetURL)
                            await successor.appendFetchScripts([.manual(
                                fetchedZones: ["meguri-root", f.zone], records: [active], group: nil)])
                            await creation.open()
                            return successor
                        })
                    }
                    let successorDrains = try await f.core.fetchDrains()
                    let stable = try await f.core.durableSnapshot()
                    let guardBeforeLateIdentity = try await f.core.currentGuard()
                    await resolver.open()
                    return (replyFinishedBeforeStop, fetchReturned, stoppedBeforeRelease,
                            replacement.instanceID, stable, guardBeforeLateIdentity,
                            stoppedDrains, successorDrains)
                })
            }
            // Resolver, creation, startup, fetch and watchdogs joined on GREEN/RED.
            let after = try await f.core.durableSnapshot()
            try await f.core.validateSDK(observation.3)
            let oldBatch = await old.requestRetainedBatch(zone: f.zone)
            let reads = try await f.core.diagnostics().rootReads
            #expect(observation.0 && observation.1)
            #expect(observation.2, "Stop waited for the post-terminal identity resolver")
            #expect(observation.3 != old.instanceID)
            #expect(await f.adapter.retainedInstanceIDs() == [observation.3])
            #expect(try await f.core.currentGuard() == observation.5)
            #expect(after == observation.4)
            #expect(after.entities == before.entities)
            #expect(observation.6 == drains)
            #expect(try await f.core.fetchDrains() == observation.7)
            #expect(reads.last?.active == true && reads.last?.outcome == "found" && reads.last?.accepted == false)
            #expect(oldBatch == nil)
            #expect(await resolver.pendingWaiterCount() == 0)
            #expect(await creation.pendingWaiterCount() == 0)
        }
    }

    /// R1 P2; K12.5/P3/V3.4/V26.1/Q11. The real epoch failure is
    /// accepted, then Active's original native reply is held until Core retires
    /// the owner. Stop's task is always joined AFTER opening the reply gate.
    @Test(arguments: [CKError.Code.unknownItem, .zoneNotFound], FetchReadOwnerEnd.allCases)
    func cloudKitMissingEpochOwnerEndPersistsIsolationBeforeLateActiveReply(
        code: CKError.Code, end: FetchReadOwnerEnd
    ) async throws {
        let active = CloudCallbackGate()
        let transport = OwnerEndReadTransport(activeReply: active)
        try await withCloudKitFixture(keepAlive: true, savedState: nil, assetReadTransport: transport) { f in
            // K12.5/V26.1: the actual missing epoch requires fresh proof
            // before Core can issue the follow-up Active read held below.
            await f.factory.appendAccountRead(.success(.init(status: .available,
                identity: Data("synthetic-account-A".utf8))))
            let old = try await f.engine()
            let before = try await f.core.durableSnapshot()
            let drains = try await f.core.fetchDrains()
            transport.setReplies([.failure(code), .found(f.root.envelope)])
            await old.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let observation = try await withOwnerEndDeadline(gates: [active]) { decision in
                try await withCloudTask(gates: [active], expectedArrivals: [], operation: {
                    do { try await f.adapter.requestFetch(zones: [f.zone]) } catch { }
                }, whileRunning: {
                    try await active.waitForArrival()
                    switch end {
                    case .stop: break
                    case .signedOut:
                        try await f.core.accountChanged(sdk: nil, status: .noAccount, identity: nil)
                    case .accountSwitch:
                        try await f.core.accountChanged(sdk: nil, status: .available,
                            identity: Data("synthetic-account-B".utf8))
                    case .activeDeleted:
                        try await f.core.fetched(sdk: old.instanceID, zone: "meguri-root", records: [], deleted: ["active"])
                        await f.factory.appendAccountRead(.success(.init(status: .available,
                            identity: Data("synthetic-account-A".utf8))))
                        try await f.adapter.refreshControlIdentity()
                    }
                    let stop = Task { try await f.adapter.stop() }
                    do {
                        try await f.adapter.waitForStopping(instanceID: old.instanceID)
                        let retiredWhileHeld = decision.observeReady()
                        let retired = try await f.core.durableSnapshot()
                        var authorityRefused = false
                        do { try await f.core.validateSDK(old.instanceID) } catch { authorityRefused = true }
                        await active.open()
                        try await stop.value
                        return (retiredWhileHeld, retired, authorityRefused)
                    } catch {
                        // Unblock the held read and join stop before rethrowing
                        // any assertion failure from this witness.
                        await active.open()
                        _ = await stop.result
                        throw error
                    }
                })
            }
            let after = try await f.core.durableSnapshot()
            let reads = try await f.core.diagnostics().rootReads
            let afterDrains = try await f.core.fetchDrains()
            let oldBatch = await old.requestRetainedBatch(zone: f.zone)
            // Reopen the same real tracking store; do not seed a new baseline.
            try await f.collection.close()
            let location = CoreBridge.CollectionLocation(
                collectionDirectory: f.directory.appending(path: "collection"),
                backupsDirectory: f.directory.appending(path: "backups"),
                appGroupSnapshot: f.directory.appending(path: "widget.snapshot"),
                manualTransferDirectory: f.directory.appending(path: "synthetic-provider-installation"))
            try await f.collection.open(location, randomSeed: 115)
            let reopened = CoreCloudWorkflowService()
            let persisted = try await reopened.durableSnapshot()
            let newAuthorityRefused: Bool
            let unexpected: CloudRegistration?
            do {
                unexpected = try await reopened.registerSDK(automatic: false)
                newAuthorityRefused = false
            } catch {
                unexpected = nil
                newAuthorityRefused = true
            }
            if let unexpected {
                // A RED admission must not leak the unexpected raw SDK owner.
                try await reopened.retireSDK(unexpected.instanceID, policyStop: false)
                try await reopened.releaseSDK(unexpected.instanceID)
            }
            let retained = await f.adapter.retainedInstanceIDs()
            // Every gate/task is drained before the expected RED assertions.
            #expect(observation.0 && observation.2)
            #expect(try ownerEndBindingState(observation.1) == ["Isolated": end.reason])
            #expect(try ownerEndBindingState(after) == ["Isolated": end.reason])
            #expect(try ownerEndBindingState(persisted) == ["Isolated": end.reason])
            #expect(newAuthorityRefused)
            #expect(after.entities == before.entities && persisted.entities == before.entities)
            #expect(!before.entities.filter { f.names.contains($0.name) && $0.mark != nil && $0.localRev > $0.ackedRev }.isEmpty)
            #expect(after.binding == before.binding && persisted.binding == before.binding)
            #expect(after.rootEnvelope == observation.1.rootEnvelope && after.rootSystemFields == observation.1.rootSystemFields)
            #expect(persisted.rootEnvelope == after.rootEnvelope && persisted.rootSystemFields == after.rootSystemFields)
            #expect(!after.absencePassCompleted && !persisted.absencePassCompleted)
            #expect(afterDrains == drains)
            #expect(transport.completedReads.map(\.name) == ["epoch", "active"])
            #expect(transport.completedReads.map(\.zone) == [f.zone, "meguri-root"])
            #expect(reads.count == 2)
            #expect(reads.first?.accepted == true && reads.first?.active == false)
            #expect(reads.first?.outcome == (code == .unknownItem ? "unknownItem" : "zoneNotFound"))
            #expect(reads.last?.active == true && reads.last?.lostCollection == true && reads.last?.accepted == false)
            #expect(reads.last?.outcome == "found")
            #expect(oldBatch == nil && retained.isEmpty)
            #expect(await active.pendingWaiterCount() == 0)
            // Swift's observations omit RootConfirmed and sdk_root_confirmed.
            // No claim about its event count: assert refusal, stored Active,
            // durable isolation and absence of drains through the real Core.
        }
    }
}

/// DeviceState reads collection/meguri-sidecar.json strictly: absence is None,
/// but a directory at the leaf fails std::fs::read. Move the ORIGINAL file so
/// restoration retains its inode/bytes. The collection DB is never moved.
private final class OwnerEndPublicationPathFault {
    private let path: URL
    private let original: URL
    private let bytes: Data
    private let device: NSNumber
    private let inode: NSNumber
    private var originalFileMoved = false
    private var directoryInstalled = false

    init(directory: URL) throws {
        path = directory.appending(path: "collection/meguri-sidecar.json")
        original = directory.appending(path: "collection/meguri-sidecar.u114r2-original.json")
        bytes = try Data(contentsOf: path)
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        device = try #require(attributes[.systemNumber] as? NSNumber)
        inode = try #require(attributes[.systemFileNumber] as? NSNumber)
        try #require(attributes[.type] as? FileAttributeType == .typeRegular)
        try #require(!FileManager.default.fileExists(atPath: original.path))
        try FileManager.default.moveItem(at: path, to: original)
        originalFileMoved = true
        do {
            try FileManager.default.createDirectory(at: path, withIntermediateDirectories: false)
            directoryInstalled = true
        } catch {
            try restore()
            throw error
        }
    }
    func restore() throws {
        if directoryInstalled {
            try FileManager.default.removeItem(at: path)
            directoryInstalled = false
        }
        if originalFileMoved {
            try FileManager.default.moveItem(at: original, to: path)
            originalFileMoved = false
        }
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        try #require(try Data(contentsOf: path) == bytes, "Restore exact original publication bytes")
        try #require(attributes[.systemNumber] as? NSNumber == device)
        try #require(attributes[.systemFileNumber] as? NSNumber == inode)
    }
    deinit {
        // Unwinding must also put the publication file back. Explicit restore
        // reports failures before the adapter and Host are torn down.
        if directoryInstalled { try? FileManager.default.removeItem(at: path) }
        if originalFileMoved { try? FileManager.default.moveItem(at: original, to: path) }
    }
}

private func ownerEndInstalledSDK() async throws -> String? {
    try await MaintenanceLane.shared.call("test.cloudInstalledSdkOwner") {
        try MeguriCoreBindings.testCloudInstalledSdkOwner()
    }
}

extension EventTests {
    /// R2 finding 2; K12.5/P3/V3.4/Q11. The original accepted epoch loss
    /// requires Core to inspect publication when retiring. If inspection fails, it
    /// must preserve both SDK owners until this adapter can stop successfully.
    @Test(arguments: [CKError.Code.unknownItem, .zoneNotFound])
    func cloudKitMissingEpochRetirementProbeFailureRetainsOwnersAndSameStopRetries(code: CKError.Code) async throws {
        let active = CloudCallbackGate()
        let transport = OwnerEndReadTransport(activeReply: active)
        try await withCloudKitFixture(keepAlive: true, savedState: nil, assetReadTransport: transport) { f in
            // K12.5/V26.1: the actual missing epoch requires fresh proof
            // before Core can issue the follow-up Active read held below.
            await f.factory.appendAccountRead(.success(.init(status: .available,
                identity: Data("synthetic-account-A".utf8))))
            let old = try await f.engine()
            let before = try await f.core.durableSnapshot()
            let drains = try await f.core.fetchDrains()
            transport.setReplies([.failure(code), .found(f.root.envelope)])
            await old.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            do {
                let observation = try await withOwnerEndDeadline(gates: [active]) { firstDecision in
                    try await withCloudTask(gates: [active], expectedArrivals: [], operation: {
                        do { try await f.adapter.requestFetch(zones: [f.zone]) }
                        catch CloudOwnTransportFailure.queueRejected {
                            throw transport.queueRejection(phase: .originalFetch)
                        } catch { }
                    }, whileRunning: {
                        try await active.waitForArrival()
                        let reads = try await f.core.diagnostics().rootReads
                        try #require(reads.count == 1 && reads[0].accepted && !reads[0].active)
                        try #require(reads[0].outcome == (code == .unknownItem ? "unknownItem" : "zoneNotFound"))
                        try #require(transport.completedReads.map(\.name) == ["epoch", "active"])
                        let fault = try OwnerEndPublicationPathFault(directory: f.directory)
                        do {
                            let firstStop = Task { try await f.adapter.stop() }
                            // R2 waits for Active here; watchdog releases it and
                            // preserves the failed pre-release observation.
                            let firstResult = await firstStop.result
                            let returnedWhileHeld = firstDecision.observeReady()
                            let firstFailed: Bool
                            if case .failure = firstResult { firstFailed = true } else { firstFailed = false }
                            let firstSwift = await f.adapter.retainedInstanceIDs()
                            let firstCore = try await ownerEndInstalledSDK()
                            let firstTrace = await old.recordedTrace()
                            var firstAuthorityLive = false
                            do { try await f.core.validateSDK(old.instanceID); firstAuthorityLive = true } catch { }
                            let firstSnapshot = try await f.core.durableSnapshot()
                            // Always restore before retry, late acceptance or
                            // cleanup. No SQLite path is absent during this fault.
                            try fault.restore()
                            let retry = try await withOwnerEndDeadline(gates: [active]) { retryDecision in
                                let secondStop = Task { try await f.adapter.stop() }
                                do {
                                    try await f.adapter.waitForStopping(instanceID: old.instanceID)
                                    let retiredWhileHeld = retryDecision.observeReady()
                                    var authorityRefused = false
                                    do { try await f.core.validateSDK(old.instanceID) } catch { authorityRefused = true }
                                    let retired = try await f.core.durableSnapshot()
                                    await active.open()
                                    let secondResult = await secondStop.result
                                    let succeeded: Bool
                                    if case .success = secondResult { succeeded = true } else { succeeded = false }
                                    let coreAfter = try await ownerEndInstalledSDK()
                                    let swiftAfter = await f.adapter.retainedInstanceIDs()
                                    let traceAfter = await old.recordedTrace()
                                    return (retiredWhileHeld, authorityRefused, retired, succeeded,
                                            coreAfter, swiftAfter, traceAfter)
                                } catch {
                                    await active.open()
                                    _ = await secondStop.result
                                    throw error
                                }
                            }
                            return (returnedWhileHeld, firstFailed, firstSwift, firstCore, firstTrace,
                                    firstAuthorityLive, firstSnapshot, retry)
                        } catch {
                            try fault.restore()
                            throw error
                        }
                    })
                }
                // Both Stop tasks, original fetch and watchdogs have joined.
                // Read every proof BEFORE raw cleanup can retire a leaked slot.
                let after = try await f.core.durableSnapshot()
                let afterDrains = try await f.core.fetchDrains()
                let reads = try await f.core.diagnostics().rootReads
                let oldBatch = await old.requestRetainedBatch(zone: f.zone)
                let leakedOwner = try await ownerEndInstalledSDK()
                #expect(observation.0)
                #expect(observation.1, "Stop must propagate the real publication read error")
                #expect(observation.2 == [old.instanceID] && observation.3 == old.instanceID,
                        "Swift and Core must still hold the SDK that could not retire")
                #expect(observation.5 && !observation.4.contains(.released))
                #expect(observation.6.entities == before.entities)
                #expect(observation.7.0 && observation.7.1, "Same Stop must actually retire original authority")
                #expect(observation.7.3, "Same adapter Stop must succeed after restoring publication")
                #expect(observation.7.4 == nil && observation.7.5.isEmpty && leakedOwner == nil,
                        "Same Stop must release the actual Core SDK slot and Swift owner")
                #expect(observation.7.6.contains(.released))
                #expect(try ownerEndBindingState(observation.7.2) == ["Isolated": "ZoneMissing"])
                #expect(try ownerEndBindingState(after) == ["Isolated": "ZoneMissing"])
                #expect(after.entities == before.entities)
                #expect(!before.entities.filter { f.names.contains($0.name) && $0.mark != nil && $0.localRev > $0.ackedRev }.isEmpty)
                #expect(after.binding == before.binding && !after.absencePassCompleted)
                #expect(after.rootEnvelope == observation.7.2.rootEnvelope && after.rootSystemFields == observation.7.2.rootSystemFields)
                #expect(afterDrains == drains && oldBatch == nil)
                #expect(reads.count == 2 && reads.first?.accepted == true && reads.last?.accepted == false)
                #expect(reads.last?.active == true && reads.last?.lostCollection == true && reads.last?.outcome == "found")
                #expect(await active.pendingWaiterCount() == 0)
                if let leakedOwner {
                    // RED recorded above. These calls are ONLY failure cleanup;
                    // their effects never prove that the second adapter Stop ran.
                    try await f.core.retireSDK(leakedOwner, policyStop: false)
                    try await f.core.releaseSDK(leakedOwner)
                }
            } catch {
                await active.open()
                // The fault's catch/deinit restored the original sidecar before
                // reaching here. Join adapter cleanup, then close any raw slot.
                _ = try? await f.adapter.stop()
                if let owner = try? await ownerEndInstalledSDK() {
                    try await f.core.retireSDK(owner, policyStop: false)
                    try await f.core.releaseSDK(owner)
                }
                throw error
            }
        }
    }
}
