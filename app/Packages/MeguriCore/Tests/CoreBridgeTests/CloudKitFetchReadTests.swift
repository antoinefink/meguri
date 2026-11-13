import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

/// Delivers the original NativeCloudOwnTransport CKFetchRecordsOperation's
/// callbacks. Only SDK records/errors are injected; Core outcomes stay real.
final class FetchControlTransport: CloudOwnTransport, NativeCloudDatabaseQueue, Sendable {
    enum Reply: Sendable { case found(Data?), failure(CKError.Code) }
    private struct State {
        var replies: [Reply] = []
        var targets: [CloudControlTarget] = []
        var results: [CloudControlRead] = []
    }
    private let state = Mutex(State())
    let gate: CloudCallbackGate?
    init(gate: CloudCallbackGate? = nil) { self.gate = gate }
    func setReplies(_ replies: [Reply]) { state.withLock { $0.replies = replies } }
    var targets: [CloudControlTarget] { state.withLock { $0.targets } }
    var results: [CloudControlRead] { state.withLock { $0.results } }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .read = command else { throw CloudOwnTransportFailure.unexpectedReply }
        return Operation(original: try NativeCloudOwnTransport(queue: self).prepare(command), owner: self)
    }
    func add(_ operation: CKDatabaseOperation) -> Bool {
        guard let operation = operation as? CKFetchRecordsOperation,
              let ids = operation.recordIDs, ids.count == 1, let id = ids.first,
              operation.desiredKeys == nil,
              let receive = operation.perRecordResultBlock,
              let finish = operation.fetchRecordsResultBlock else {
            Issue.record("Expected original all-fields control read")
            return false
        }
        let target = CloudControlTarget(zone: id.zoneID.zoneName, name: id.recordName,
            recordType: id.recordName == "active" ? "MGActive" : "MGEpoch")
        let reply = state.withLock { value -> Reply? in
            value.targets.append(target)
            return value.replies.isEmpty ? nil : value.replies.removeFirst()
        }
        guard let reply else { Issue.record("Unscripted control read"); return false }
        switch reply {
        case .found(let encrypted):
            let record = CKRecord(recordType: target.recordType, recordID: id)
            if let encrypted { record.encryptedValues["e"] = encrypted as CKRecordValue }
            receive(id, .success(record))
        case .failure(let code): receive(id, .failure(CKError(code)))
        }
        finish(.success(()))
        return true
    }
    private struct Operation: CloudPreparedOwnOperation {
        let original: any CloudPreparedOwnOperation
        let owner: FetchControlTransport
        func enqueue() -> Bool { original.enqueue() }
        func cancel() { original.cancel(); if let gate = owner.gate { Task { await gate.open() } } }
        func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
        func reply() async throws -> CloudOwnTransportReply {
            let reply = try await original.reply()
            guard case .read(let result) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
            owner.state.withLock { $0.results.append(result) }
            if let gate = owner.gate { await gate.enter() }
            return reply
        }
    }
}

private func storedBindingState(_ snapshot: CloudDurableSnapshot) throws -> [String: String]? {
    let content = try #require(JSONSerialization.jsonObject(with: snapshot.trackingContent) as? [Any])
    let binding = try #require(content.first as? [Any])
    let facts = try #require(binding.last as? [String: Any])
    return facts["state"] as? [String: String]
}

enum FetchInferenceOrigin: CaseIterable, Sendable, Equatable {
    case creation, joinedRepeat, priorFailedWill, accountReset, savedAccountReset
    var a7: Bool { self == .creation || self == .joinedRepeat }
}
enum FetchReadRevocation: CaseIterable, Sendable, Equatable { case newNoStatePoint, accountLoss, suspend, stop }

extension EventTests {
    @Test func cloudKitLostCollectionActiveTransportFailureReachesOriginalCoreRead() async throws {
        let transport = FetchControlTransport()
        try await withCloudKitFixture(savedState: nil, assetReadTransport: transport) { f in
            let engine = try await f.engine()
            for _ in 0..<3 {
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            }
            transport.setReplies([.failure(.unknownItem), .failure(.networkFailure)])
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let before = try await f.core.durableSnapshot()
            do { try await f.adapter.requestFetch(zones: [f.zone]); Issue.record("Native Active transport failure disappeared") }
            catch { }
            #expect(transport.targets.map(\.name) == ["epoch", "active"])
            let after = try await f.core.durableSnapshot()
            #expect(try storedBindingState(after) == ["Isolated": "ZoneMissing"])
            #expect(after.entities == before.entities && after.serialization == before.serialization)
            let diagnostics = try await f.core.diagnostics().rootReads
            let active = try #require(diagnostics.last)
            #expect(active.active && active.accepted && active.outcome == "failed")
            #expect(active.failure?.code == Int64(CKError.Code.networkFailure.rawValue))
            #expect(try await f.core.fetchDrains().isEmpty)
        }
    }

    /// P3/P12.3/V26.1: active is confirmed and stored through the real Core
    /// path; an actual typed epoch record proves existence even without e.
    /// Neither false drain can finish a no-state absence pass.
    @Test(arguments: FetchInferenceOrigin.allCases)
    func cloudKitNoStateFoundControlsUseLatestOriginIncludingJoinedRepeat(origin: FetchInferenceOrigin) async throws {
        let transport = FetchControlTransport()
        let saved: Data? = origin == .savedAccountReset ? Data([0x73, 0x64, 0x6b]) : nil
        try await withCloudKitFixture(savedState: saved, assetReadTransport: transport) { f in
            let engine = try await f.engine()
            // Actual resolver replies, consumed only by Core-issued fresh reads.
            for _ in 0..<3 {
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            }
            transport.setReplies([.found(f.root.envelope), .found(nil)])
            if origin == .accountReset || origin == .savedAccountReset { await engine.signInSameSyntheticIdentity() }
            if origin == .priorFailedWill {
                await engine.emitRetained(.willFetch(groupID: "scheduled-failed-zone", reason: .scheduled), label: "scheduledWill")
                await engine.emitRetained(.willFetchZone(zone: f.zone), label: "priorCollectionWill")
                await engine.emitRetained(.didFetchZone(zone: f.zone, error: .networkFailure), label: "priorCollectionFailure")
                await engine.emitRetained(.didFetch(groupID: "scheduled-failed-zone", reason: .scheduled), label: "scheduledDid")
            }
            if origin == .joinedRepeat {
                await engine.emitRetained(.willFetch(groupID: "scheduled-before-call", reason: .scheduled), label: "scheduledWill")
                await engine.appendFetchScripts([.joined(group: "scheduled-before-call"), .manual(fetchedZones: [], records: [], group: nil)])
            } else { await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)]) }
            let before = try await f.core.fetchDrains()
            try await f.adapter.requestFetch(zones: ["meguri-root", f.zone])
            #expect(transport.targets.map(\.name) == ["active", "epoch"])
            #expect(transport.targets.map(\.zone) == ["meguri-root", f.zone])
            let drains = Array((try await f.core.fetchDrains()).dropFirst(before.count))
            #expect(drains.map(\.zone) == ["meguri-root", f.zone])
            #expect(drains.allSatisfy { !$0.fetched })
            let after = try await f.core.durableSnapshot()
            #expect(!after.absencePassCompleted)
            let active = try #require(transport.results.first)
            guard case .found(let actual) = active else { Issue.record("Actual active record missing"); return }
            #expect(after.rootEnvelope == actual.encrypted)
            #expect(after.rootSystemFields == actual.systemFields)
            try await f.core.validateSDK(engine.instanceID)
            let diagnostics = try await f.core.diagnostics().rootReads
            #expect(diagnostics.map(\.outcome) == ["found", "found"])
            let allAccepted = diagnostics.allSatisfy { $0.accepted }
            #expect(allAccepted)
            #expect(diagnostics.map(\.a7Failure) == [false, origin.a7])
            let trace = await engine.recordedTrace()
            let calls = trace.filter { if case .fetchReturned = $0 { true } else { false } }
            #expect(calls.count == (origin == .joinedRepeat ? 2 : 1))
            #expect(!trace.contains(.reentrantNetworkCall))
        }
    }

    /// A zone will before the latest reset prevents A7 classification, and
    /// its old completion proves no absence. A later post-point fetch does.
    @Test func cloudKitNoStateFoundEpochAfterPriorWillLogsWithoutA7OrAbsenceCompletion() async throws {
        let transport = FetchControlTransport()
        transport.setReplies([.found(nil)])
        try await withCloudKitFixture(savedState: nil, assetReadTransport: transport) { f in
            let engine = try await f.engine()
            // Actual resolver replies, consumed only by Core-issued fresh reads.
            for _ in 0..<3 {
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            }
            let gate = CloudCallbackGate()
            await engine.appendFetchScripts([.manualPaused(fetchedZones: [f.zone], records: [],
                pauseZone: f.zone, afterDid: true, gate: gate)])
            let before = try await f.core.fetchDrains()
            try await withCloudTask(gates: [gate], operation: {
                try await f.adapter.requestFetch(zones: [f.zone])
            }, whileRunning: {
                try await gate.waitForArrival()
                await engine.signInSameSyntheticIdentity()
                await gate.open()
            })
            let drains = Array((try await f.core.fetchDrains()).dropFirst(before.count))
            #expect(drains.map(\.zone) == [f.zone] && drains.allSatisfy { !$0.fetched })
            #expect(!(try await f.core.durableSnapshot()).absencePassCompleted)
            let read = try #require(try await f.core.diagnostics().rootReads.last)
            #expect(read.outcome == "found" && read.accepted && !read.a7Failure)
            await engine.appendFetchScripts([.manual(fetchedZones: [f.zone], records: [], group: nil)])
            try await f.adapter.requestFetch(zones: [f.zone])
            #expect((try await f.core.durableSnapshot()).absencePassCompleted)
            #expect(transport.targets.map(\.name) == ["epoch"])
        }
    }

    /// K12.5: unknownItem on epoch really reads active again. A same-epoch
    /// record then isolates zone_missing and is retained as the last Active.
    @Test(arguments: [CKError.Code.unknownItem, .zoneNotFound])
    func cloudKitNoStateMissingEpochReadsActualActiveBeforeZoneMissing(code: CKError.Code) async throws {
        let transport = FetchControlTransport()
        try await withCloudKitFixture(savedState: nil, assetReadTransport: transport) { f in
            let engine = try await f.engine()
            // Actual resolver replies, consumed only by Core-issued fresh reads.
            for _ in 0..<3 {
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            }
            transport.setReplies([.failure(code), .found(f.root.envelope)])
            let before = try await f.core.fetchDrains()
            let rootBefore = try await f.core.durableSnapshot()
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            try await f.adapter.requestFetch(zones: [f.zone])
            #expect(transport.targets.map(\.name) == ["epoch", "active"])
            #expect(transport.targets.map(\.zone) == [f.zone, "meguri-root"])
            #expect(try await f.core.fetchDrains() == before)
            let after = try await f.core.durableSnapshot()
            #expect(try storedBindingState(after) == ["Isolated": "ZoneMissing"])
            #expect(!after.absencePassCompleted)
            guard case .found(let actual) = try #require(transport.results.last) else { Issue.record("No active reply"); return }
            // The last applied Active is retained; the root ACK stays unchanged.
            let content = try #require(JSONSerialization.jsonObject(with: after.trackingContent) as? [Any])
            let durable = try #require(content[4] as? [String: Any])
            let active = try #require(durable["active"] as? [String: Any])
            let envelope = try #require(active["envelope"] as? [UInt8])
            let systemFields = try #require(active["system_fields"] as? [UInt8])
            #expect(Data(envelope) == actual.encrypted)
            #expect(Data(systemFields) == actual.systemFields)
            #expect(after.rootEnvelope == rootBefore.rootEnvelope)
            #expect(after.rootSystemFields == rootBefore.rootSystemFields)
            let diagnostics = try await f.core.diagnostics().rootReads
            #expect(diagnostics.map(\.outcome) == [code == .unknownItem ? "unknownItem" : "zoneNotFound", "found"])
            #expect(diagnostics.map(\.lostCollection) == [false, true])
            #expect(diagnostics.first?.failure?.code == Int64(code.rawValue))
            await #expect(throws: (any Error).self) { try await f.core.validateSDK(engine.instanceID) }
        }
    }

    /// An epoch read error produces no drain. P4 allows a later fetch to
    /// check the record again without repeating this qualified fetch.
    @Test func cloudKitNoStateFailedEpochReadWaitsForAnotherCallWithoutRetry() async throws {
        let transport = FetchControlTransport()
        transport.setReplies([.failure(.networkFailure), .found(nil)])
        try await withCloudKitFixture(savedState: nil, assetReadTransport: transport) { f in
            let engine = try await f.engine()
            // Actual resolver replies, consumed only by Core-issued fresh reads.
            for _ in 0..<3 {
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            }
            let before = try await f.core.fetchDrains()
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            do {
                try await f.adapter.requestFetch(zones: [f.zone])
                Issue.record("A failed original epoch read succeeded")
            } catch {
                let actual = try #require(error as? CloudKitSDKOperationFailure)
                #expect(actual.code == Int64(CKError.Code.networkFailure.rawValue))
            }
            #expect(transport.targets.map(\.name) == ["epoch"])
            #expect(try await f.core.fetchDrains() == before)
            #expect(!(try await f.core.durableSnapshot()).absencePassCompleted)
            try await f.core.validateSDK(engine.instanceID)
            let first = try #require(try await f.core.diagnostics().rootReads.last)
            #expect(first.outcome == "failed" && first.accepted && !first.a7Failure)
            #expect(first.failure?.code == Int64(CKError.Code.networkFailure.rawValue))
            #expect((await engine.recordedTrace()).filter { if case .fetchReturned = $0 { true } else { false } }.count == 1)
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            try await f.adapter.requestFetch(zones: [f.zone])
            #expect(transport.targets.map(\.name) == ["epoch", "epoch"])
            let drains = Array((try await f.core.fetchDrains()).dropFirst(before.count))
            #expect(drains.map(\.zone) == [f.zone] && drains.allSatisfy { !$0.fetched })
            #expect(!(try await f.core.durableSnapshot()).absencePassCompleted)
        }
    }

    /// V3.4: keep the actual unknownItem after revocation in diagnostics, but
    /// apply no active_missing and emit no drain from its retained read.
    @Test(arguments: FetchReadRevocation.allCases)
    func cloudKitNoStateReadRejectsRetainedReplyAfterRevocation(change: FetchReadRevocation) async throws {
        let gate = CloudCallbackGate()
        let transport = FetchControlTransport(gate: gate)
        transport.setReplies([.failure(.unknownItem)])
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            let engine = try await f.engine()
            // Actual resolver replies, consumed only by Core-issued fresh reads.
            for _ in 0..<3 {
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            }
            await engine.signInSameSyntheticIdentity()
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let before = try await f.core.fetchDrains()
            try await withCloudTask(gates: [gate], operation: {
                do { try await f.adapter.requestFetch(zones: ["meguri-root"]) }
                catch { if change != .newNoStatePoint && !(error is CancellationError) { throw error } }
            }, whileRunning: {
                try await gate.waitForArrival()
                #expect((await engine.recordedTrace()).contains { if case .fetchReturned = $0 { true } else { false } })
                switch change {
                case .newNoStatePoint: await engine.signInSameSyntheticIdentity(); await gate.open()
                case .accountLoss: try await f.core.accountChanged(sdk: nil, status: .noAccount, identity: nil); await gate.open()
                case .suspend: try await f.adapter.freezeForSuspend()
                case .stop: try await f.adapter.stop()
                }
            })
            #expect(transport.targets.map(\.name) == ["active"])
            #expect(try await f.core.fetchDrains() == before)
            let after = try await f.core.durableSnapshot()
            #expect(try storedBindingState(after) != ["Isolated": "ActiveMissing"])
            #expect(!after.absencePassCompleted)
            let actual = try #require(try await f.core.diagnostics().rootReads.last)
            #expect(actual.outcome == "unknownItem" && !actual.accepted && !actual.a7Failure)
            #expect(actual.failure?.code == Int64(CKError.Code.unknownItem.rawValue))
            if change == .suspend { #expect(await f.adapter.resumeDeliveries()) }
        }
    }
}

#if DEBUG
private func fetchProofCommands(_ output: CloudPolicyOutput) throws -> [CloudPolicyCommand] {
    guard case .commands(let commands) = output else { throw BridgeError.internal("Unexpected policy output") }
    return commands
}

private final class FetchProofAcknowledgements: Sendable {
    private let count = Mutex(0)
    func record(_ amount: Int) { count.withLock { $0 += amount } }
    var value: Int { count.withLock { $0 } }
}

private final class FetchProofOriginalFailure: Error {}

// Inject only at the original transport reply boundary, after its SDK drain.
struct FetchProofThrowingTransport: CloudOwnTransport {
    let base: FetchControlTransport
    let error: any Error
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        Operation(original: try base.prepare(command), error: error)
    }
    private struct Operation: CloudPreparedOwnOperation {
        let original: any CloudPreparedOwnOperation
        let error: any Error
        func enqueue() -> Bool { original.enqueue() }
        func cancel() { original.cancel() }
        func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
        func reply() async throws -> CloudOwnTransportReply {
            _ = try await original.reply()
            throw error
        }
    }
}

enum FetchProofStopCase: CaseIterable, Sendable, Equatable {
    case noAccount, otherAccount, noReceipt, nonAccount, caller
    var qualifies: Bool { self == .noAccount || self == .otherAccount }
}
enum FetchProofFirstFailure: CaseIterable, Sendable, Equatable {
    case provider, providerCancellation, transport, transportCancellation, sdk
    var providerFirst: Bool { self == .provider || self == .providerCancellation }
}

private func fetchProofAccountStop(_ f: CloudKitBridgeFixture, policy: CoreCloudPolicyService,
                                   other: Bool = false) async throws -> CloudPolicyCommand {
    let read = try await f.core.beginIdentityRead(transition: true)
    try #require(try await f.core.completeIdentityRead(id: read, status: other ? .available : .noAccount,
        identity: other ? Data("synthetic-account-B".utf8) : nil))
    let projected = try await policy.projectIdentityRead(read)
    let command = try #require(try fetchProofCommands(projected).first { if case .stopEngine = $0.action { true } else { false } })
    try #require(command.accountRetirement != nil)
    return command
}

private func executeFetchProof(_ command: CloudPolicyCommand, executor: NativeCloudPolicyExecutor,
                               acknowledgements: FetchProofAcknowledgements) async -> CloudPolicyNativeResult {
    await CloudPolicyOutputAcknowledgement.$input.withValue({ work in
        let output = try await work()
        acknowledgements.record(1)
        return output
    }) {
        await executor.execute(command, corePrepared: {}, acknowledgeOutputs: { outputs in
            acknowledgements.record(outputs.count)
        })
    }
}

extension EventTests {
    @Test(arguments: [FetchProofStopCase.caller, .noAccount])
    func nativeFetchProofCancellationRecordsProducerBeforeResponsiveProvider(mode: FetchProofStopCase) async throws {
        let transport = FetchControlTransport()
        try await withNativePolicyFixture(assetReadTransport: transport, appOwnsAccountObservation: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await f.engine()
            let calls = engine.acceptedNetworkCalls.value
            let drains = try await f.core.fetchDrains()
            await engine.signInSameSyntheticIdentity()
            transport.setReplies([.found(f.root.envelope)])
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let command = try #require(try fetchProofCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            let provider = CloudCallbackGate(), cancelled = CloudCallbackGate(), selected = CloudCallbackGate()
            await provider.open(); await cancelled.open()
            let (cancellationWait, releaseProvider) = AsyncStream<Void>.makeStream()
            defer { releaseProvider.finish() }
            await f.adapter.setProofAdmission({ _ in
                await provider.enter()
                // The stream wait wakes on task cancellation.
                var iterator = cancellationWait.makeAsyncIterator()
                _ = await iterator.next()
                await cancelled.enter()
                try Task.checkCancellation()
                throw BridgeError.internal("Fetch proof provider resumed without cancellation")
            }, acknowledgeOutputs: { _ in })
            await f.adapter.setFetchProofCompletionObserver { await selected.enter() }
            let acknowledgements = FetchProofAcknowledgements()
            let fetch = Task { await executeFetchProof(command, executor: executor, acknowledgements: acknowledgements) }
            var stop: Task<CloudPolicyNativeResult, Never>?
            do {
                try await provider.waitForArrival()
                if mode == .caller { fetch.cancel() }
                else {
                    let stopCommand = try await fetchProofAccountStop(f, policy: policy)
                    stop = Task { await executor.execute(stopCommand) }
                }
                try await cancelled.waitForArrival()
                try await selected.waitForArrival()
                #expect(!(await engine.recordedTrace()).contains(.released))
                await selected.open()
                let result = await fetch.value
                #expect(result.error is CancellationError)
                let outcome = try #require(result.fetchProofOutcome)
                #expect(outcome.accountStopCancellation == mode.qualifies)
                #expect((result.nativeCancellation != nil) == (mode == .caller))
                if let cancellation = result.nativeCancellation {
                    #expect(cancellation.matches(command))
                    let other = try #require(try fetchProofCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
                    #expect(!cancellation.matches(other))
                }
                #expect(!outcome.preservesFailure)
                #expect(result.originalGuardRefusal == nil && result.dependentFetchRefusal == nil)
                #expect(!result.outputs.isEmpty && result.acknowledgedOutputCount == result.outputs.count)
                #expect(acknowledgements.value == result.outputs.count)
                if let stop { #expect(await stop.value.error == nil) }
                #expect(transport.targets.map(\.name) == ["active"])
                #expect(engine.acceptedNetworkCalls.value == calls + 1)
                #expect(try await f.core.fetchDrains() == drains)
            } catch {
                fetch.cancel(); releaseProvider.finish(); await selected.open()
                _ = await fetch.value; _ = await stop?.value
                throw error
            }
            await f.adapter.setFetchProofCompletionObserver(nil)
        }
    }

    @Test(arguments: FetchProofStopCase.allCases)
    func nativeFetchProofCancellationRequiresExecutedOriginalAccountStop(mode: FetchProofStopCase) async throws {
        let transport = FetchControlTransport()
        try await withNativePolicyFixture(assetReadTransport: transport, appOwnsAccountObservation: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await f.engine()
            // Setup is complete before recording any operation counts.
            let calls = engine.acceptedNetworkCalls.value
            let drains = try await f.core.fetchDrains()
            await engine.signInSameSyntheticIdentity()
            transport.setReplies([.found(f.root.envelope)])
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let command = try #require(try fetchProofCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            let provider = CloudCallbackGate(), selected = CloudCallbackGate()
            await f.adapter.setProofAdmission({ _ in
                await provider.enter()
                try Task.checkCancellation()
            }, acknowledgeOutputs: { _ in })
            await f.adapter.setFetchProofCompletionObserver { await selected.enter() }
            let acknowledgements = FetchProofAcknowledgements()
            let fetch = Task { await executeFetchProof(command, executor: executor, acknowledgements: acknowledgements) }
            var stop: Task<CloudPolicyNativeResult, Never>?
            do {
                try await provider.waitForArrival()
                if mode == .caller { fetch.cancel() }
                else {
                    let stopCommand: CloudPolicyCommand
                    if mode == .nonAccount {
                        stopCommand = try #require(try fetchProofCommands(try await policy.input(.settings(.init(enabled: true, paused: true, automatic: true, media: false)))).first { if case .stopEngine = $0.action { true } else { false } })
                        #expect(stopCommand.accountRetirement == nil)
                    } else if mode == .noReceipt {
                        try await f.core.accountChanged(sdk: nil, status: .noAccount, identity: nil)
                        stopCommand = try #require(try fetchProofCommands(try await policy.input(.refresh(.cloud))).first { if case .stopEngine = $0.action { true } else { false } })
                        #expect(stopCommand.accountRetirement == nil)
                    } else { stopCommand = try await fetchProofAccountStop(f, policy: policy, other: mode == .otherAccount) }
                    stop = Task { await executor.execute(stopCommand) }
                }
                try await selected.waitForArrival()
                // Stop must release A's exact wait before joining A's network.
                #expect(!(await engine.recordedTrace()).contains(.released))
                await provider.open(); await selected.open()
                let result = await fetch.value
                #expect(result.error is CancellationError)
                let outcome = try #require(result.fetchProofOutcome)
                #expect(outcome.accountStopCancellation == mode.qualifies)
                #expect((result.nativeCancellation != nil) == (mode == .caller))
                if let cancellation = result.nativeCancellation {
                    #expect(cancellation.matches(command))
                    let other = try #require(try fetchProofCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
                    #expect(!cancellation.matches(other))
                }
                #expect(!outcome.preservesFailure)
                #expect(result.originalGuardRefusal == nil && result.dependentFetchRefusal == nil)
                #expect(!result.outputs.isEmpty && result.acknowledgedOutputCount == result.outputs.count)
                #expect(acknowledgements.value == result.outputs.count)
                if let stop { #expect(await stop.value.error == nil) }
                #expect(transport.targets.map(\.name) == ["active"])
                #expect(engine.acceptedNetworkCalls.value == calls + 1)
                #expect(try await f.core.fetchDrains() == drains)
            } catch {
                fetch.cancel(); await provider.open(); await selected.open()
                _ = await fetch.value; _ = await stop?.value
                throw error
            }
            await f.adapter.setFetchProofCompletionObserver(nil)
        }
    }

    @Test(arguments: ["provider", "transport", "cleanup"])
    func nativeFetchProofEarlierCancellationOrCleanupFailureSurvivesCallerCancel(mode: String) async throws {
        let base = FetchControlTransport()
        let transport: any CloudOwnTransport = mode == "transport"
            ? FetchProofThrowingTransport(base: base, error: CancellationError()) : base
        try await withNativePolicyFixture(assetReadTransport: transport, appOwnsAccountObservation: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await f.engine()
            let calls = engine.acceptedNetworkCalls.value
            await engine.signInSameSyntheticIdentity()
            base.setReplies([.found(f.root.envelope)])
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let command = try #require(try fetchProofCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            let provider = CloudCallbackGate(), selected = CloudCallbackGate()
            await f.adapter.setProofAdmission({ _ in
                await provider.enter()
                throw CancellationError()
            }, acknowledgeOutputs: { _ in })
            await f.adapter.setFetchProofCompletionObserver { await selected.enter() }
            let acknowledgements = FetchProofAcknowledgements()
            let injected = Mutex(0)
            let completionFailure = FetchProofOriginalFailure()
            let fault: (@Sendable () throws -> Void)?
            if mode == "cleanup" {
                fault = {
                    injected.withLock { $0 += 1 }
                    throw completionFailure
                }
            } else { fault = nil }
            let fetch = Task {
                await CloudFetchProofCompletionFault.$afterOriginalDiscard.withValue(fault) {
                    await executeFetchProof(command, executor: executor, acknowledgements: acknowledgements)
                }
            }
            do {
                try await provider.waitForArrival()
                if mode == "provider" {
                    await provider.open()
                    try await selected.waitForArrival()
                    fetch.cancel()
                } else {
                    fetch.cancel()
                    try await selected.waitForArrival()
                }
                await provider.open(); await selected.open()
                let result = await fetch.value
                #expect(result.error is CancellationError && result.nativeCancellation == nil)
                let outcome = try #require(result.fetchProofOutcome)
                #expect(outcome.preservesFailure && !outcome.accountStopCancellation)
                #expect(injected.withLock { $0 } == (mode == "cleanup" ? 1 : 0))
                if mode == "cleanup" {
                    // Injected reported failure after successful original-resource discard.
                    #expect((outcome.debugOriginalCompletionFailure as? FetchProofOriginalFailure) === completionFailure)
                }
                #expect(result.acknowledgedOutputCount == result.outputs.count)
                #expect(acknowledgements.value == result.outputs.count)
                #expect(engine.acceptedNetworkCalls.value == calls + 1)
            } catch {
                fetch.cancel(); await provider.open(); await selected.open()
                _ = await fetch.value
                throw error
            }
            await f.adapter.setFetchProofCompletionObserver(nil)
        }
    }

    @Test(arguments: FetchProofFirstFailure.allCases, [false, true])
    func nativeFetchProofFirstFailureSurvivesLaterAccountStopAndClose(mode: FetchProofFirstFailure, close: Bool) async throws {
        let base = FetchControlTransport()
        let original = FetchProofOriginalFailure()
        let failure: any Error = mode == .providerCancellation || mode == .transportCancellation ? CancellationError() : original
        let transport: any CloudOwnTransport = mode == .transport || mode == .transportCancellation
            ? FetchProofThrowingTransport(base: base, error: failure) : base
        try await withNativePolicyFixture(assetReadTransport: transport, appOwnsAccountObservation: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await f.engine()
            let calls = engine.acceptedNetworkCalls.value
            let drains = try await f.core.fetchDrains()
            await engine.signInSameSyntheticIdentity()
            base.setReplies([mode == .sdk ? .failure(.networkFailure) : .found(f.root.envelope)])
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            let command = try #require(try fetchProofCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            let provider = CloudCallbackGate(), selected = CloudCallbackGate(), stopResultGate = CloudCallbackGate()
            await f.adapter.setProofAdmission({ _ in
                await provider.enter()
                if mode.providerFirst { throw failure }
                try Task.checkCancellation()
            }, acknowledgeOutputs: { _ in })
            await f.adapter.setFetchProofCompletionObserver { await selected.enter() }
            let acknowledgements = FetchProofAcknowledgements()
            let fetch = Task { await executeFetchProof(command, executor: executor, acknowledgements: acknowledgements) }
            var stop: Task<CloudPolicyNativeResult, Never>?
            var shutdown: Task<Void, any Error>?
            do {
                try await provider.waitForArrival()
                if mode.providerFirst {
                    await provider.open()
                    // Actual provider failure has won the canonical wait; only
                    // delivery to performFetchRead remains held.
                    try await selected.waitForArrival()
                }
                let stopCommand = try await fetchProofAccountStop(f, policy: policy)
                stop = Task {
                    await MaintenanceCallObservation.$capture.withValue({ call in
                        if call == .cloudPolicyRetirementResult { Task { await stopResultGate.open() } }
                    }) { await executor.execute(stopCommand) }
                }
                if !mode.providerFirst { try await selected.waitForArrival() }
                // Synchronize on the actual stop result read, after the losing
                // cancellation producer ran, without joining A's held delivery.
                await stopResultGate.enter()
                if close {
                    fetch.cancel()
                    shutdown = Task { try await f.adapter.quiesce() }
                }
                await provider.open(); await selected.open()
                let result = await fetch.value
                if mode == .sdk {
                    let error = try #require(result.error as? CloudKitSDKOperationFailure)
                    #expect(error.code == Int64(CKError.Code.networkFailure.rawValue))
                } else if mode == .providerCancellation || mode == .transportCancellation {
                    #expect(result.error is CancellationError)
                } else { #expect((result.error as? FetchProofOriginalFailure) === original) }
                let outcome = try #require(result.fetchProofOutcome)
                #expect(!outcome.accountStopCancellation && outcome.preservesFailure)
                #expect(result.nativeCancellation == nil)
                #expect(result.originalGuardRefusal == nil && result.dependentFetchRefusal == nil)
                #expect(!result.outputs.isEmpty && result.acknowledgedOutputCount == result.outputs.count)
                #expect(acknowledgements.value == result.outputs.count)
                if let stop { #expect(await stop.value.error == nil) }
                try await shutdown?.value
                #expect(engine.acceptedNetworkCalls.value == calls + 1)
                #expect(base.targets.map(\.name) == ["active"])
                #expect(try await f.core.fetchDrains() == drains)
                if mode == .sdk {
                    let read = try #require(try await f.core.diagnostics().rootReads.last)
                    #expect(read.failure?.code == Int64(CKError.Code.networkFailure.rawValue))
                    #expect(read.outcome == "failed" && !read.accepted)
                }
            } catch {
                fetch.cancel(); await provider.open(); await selected.open(); await stopResultGate.open()
                _ = await fetch.value; _ = await stop?.value; _ = try? await shutdown?.value
                throw error
            }
            await f.adapter.setFetchProofCompletionObserver(nil)
        }
    }
}
#endif
