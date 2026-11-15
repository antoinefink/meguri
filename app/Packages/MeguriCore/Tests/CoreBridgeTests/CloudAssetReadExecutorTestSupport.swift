import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

struct AssetExecutorRemote: Sendable {
    let record: CoreBridge.CloudRecord
    let payload: Data
    let noteID: Int64
    let fields: Data

    static func make(_ ordinal: UInt32) async throws -> Self {
        try await MaintenanceLane.shared.call("test.cloudAssetReadRemoteFixture") {
            let raw = try MeguriCoreBindings.testCloudAssetReadRemoteFixture(ordinal: ordinal)
            return Self(record: .init(raw.record), payload: raw.payload, noteID: raw.noteId, fields: raw.fields)
        }
    }

    func observe() async throws -> MeguriCoreBindings.CloudAssetReadFixtureObservation {
        try await MaintenanceLane.shared.call("test.cloudAssetReadFixtureObservation") {
            try MeguriCoreBindings.testCloudAssetReadFixtureObservation(name: record.name, noteId: noteID)
        }
    }

    /// Swift copies opaque Rust codec bytes; it never authors/decrypts them.
    func ownedResult(in directory: URL) throws -> CloudAssetReadResult {
        let source = directory.appending(path: "sdk-\(noteID)")
        try payload.write(to: source)
        let staging = try CloudMediaStaging(directory: directory.appending(path: "owned-\(noteID)"), available: { UInt64.max })
        let asset = try staging.copyPayload(source, cancelled: { false })
        try FileManager.default.removeItem(at: source)
        let actual = CoreBridge.CloudRecord(name: record.name, envelope: record.envelope,
            systemFields: record.systemFields, recordType: record.recordType, zone: record.zone, assetURL: asset.url)
        return try .init(record: actual, asset: asset)
    }
}

func assetExecutorPolicy(_ fixture: CloudKitBridgeFixture) async throws -> CoreCloudPolicyService {
    let (policy, _) = try await nativePolicy(fixture)
    try await fixture.qualifyCompleteEpoch()
    let engine = try await fixture.engine()
    await engine.appendFetchScripts([
        .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
    ])
    try await fixture.adapter.requestFetch(zones: ["meguri-root", fixture.zone])
    _ = try await policy.input(.refresh(.cloud))
    return policy
}

func assetExecutorRetryPass(_ fixture: CloudKitBridgeFixture, policy: CoreCloudPolicyService,
                            records: [AssetExecutorRemote]) async throws -> CoreCloudAssetReadPass {
    let execution = try await assetExecutorRetryExecution(fixture, policy: policy, records: records)
    return try await execution.beginAssetReadPass()
}

func assetExecutorRetryExecution(_ fixture: CloudKitBridgeFixture, policy: CoreCloudPolicyService,
                                records: [AssetExecutorRemote]) async throws -> CoreCloudPolicyExecution {
    let sdk = try await fixture.engine().instanceID
    try await fixture.core.fetched(sdk: sdk, zone: fixture.zone, records: records.map(\.record), deleted: [])
    let fetch = try await fixture.core.beginFetch(sdk: sdk, zones: [fixture.zone])
    let group = "asset-executor-drain"
    try await fixture.core.willFetch(sdk: sdk, group: group, manual: true)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: true, failed: false)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: false, failed: false)
    try await fixture.core.didFetch(sdk: sdk, group: group, manual: true)
    try #require(try await fixture.core.finishFetch(sdk: sdk, id: fetch))
    guard case .commands(let commands) = try await policy.input(.refresh(.cloud)) else {
        throw BridgeError.internal("Expected actual retry command output")
    }
    let command = try #require(commands.first { if case .retryAssetEntries = $0.action { true } else { false } })
    let execution = try #require(try await policy.prepareExecution(command))
    return execution
}

/// A deterministic original terminal over an actual owned result. Cancellation
/// never invents a terminal; reply uses the native transport's detached join.
final class AssetExecutorOperation: CloudPreparedOwnOperation, Sendable {
    let terminal = CloudCallbackGate()
    let enqueueCalls = Mutex(0)
    let replyCalls = Mutex(0)
    let cancelled = Mutex(false)
    let accept: Bool
    private let outcome: Mutex<Result<CloudOwnTransportReply, any Error>?>
    private let cancellations: AsyncStream<Void>
    private let didCancel: AsyncStream<Void>.Continuation
    private let acceptedBeforeReturn: @Sendable () -> Void

    init(accept: Bool = true, outcome: Result<CloudOwnTransportReply, any Error>,
         acceptedBeforeReturn: @escaping @Sendable () -> Void = {}) {
        self.acceptedBeforeReturn = acceptedBeforeReturn
        self.accept = accept
        self.outcome = Mutex(outcome)
        (cancellations, didCancel) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
    }

    func enqueue() -> Bool {
        enqueueCalls.withLock { $0 += 1 }
        let accepted = accept && !cancelled.withLock { $0 }
        if accepted { acceptedBeforeReturn() }
        return accepted
    }
    func cancel() {
        cancelled.withLock { $0 = true }
        didCancel.yield(())
    }
    func waitUntilCancelled() async throws {
        var iterator = cancellations.makeAsyncIterator()
        guard await iterator.next() != nil else { throw CloudOwnTransportFailure.missingReply }
    }
    func reply() async throws -> CloudOwnTransportReply {
        replyCalls.withLock { $0 += 1 }
        return try await Task.detached { [self] in
            await terminal.enter()
            let result = outcome.withLock { value in
                let result = value
                value = nil
                return result
            }
            return try #require(result).get()
        }.value
    }
}

final class AssetExecutorTransport: CloudOwnTransport, Sendable {
    let operations: [String: AssetExecutorOperation]
    let preparedNames = Mutex<[String]>([])
    let failPreparation: Bool
    init(_ operations: [String: AssetExecutorOperation] = [:], failPreparation: Bool = false) {
        self.operations = operations; self.failPreparation = failPreparation
    }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .assetRead(let zone, let name) = command else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(zone.hasPrefix("c-"))
        preparedNames.withLock { $0.append(name) }
        if failPreparation { throw CloudOwnTransportFailure.invalidCursor }
        return try #require(operations[name])
    }
}

func expectAssetExecutorFailure<Value>(_ result: Result<Value, any Error>,
                                _ expected: CloudOwnTransportFailure) {
    guard case .failure(let error) = result, let actual = error as? CloudOwnTransportFailure else {
        Issue.record("Expected transport failure \(expected), got \(result)"); return
    }
    switch (actual, expected) {
    case (.closed, .closed), (.busy, .busy), (.queueRejected, .queueRejected),
         (.invalidCursor, .invalidCursor), (.unexpectedReply, .unexpectedReply), (.missingReply, .missingReply): break
    default: Issue.record("Unexpected transport failure \(actual)")
    }
}
