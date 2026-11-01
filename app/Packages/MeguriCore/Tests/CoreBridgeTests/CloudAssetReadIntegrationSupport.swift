import Foundation
import Instrumentation
import func MeguriCoreBindings.testCloudPolicyExchangeObservation
import Synchronization
import Testing
@testable import CoreBridge

// Source base dc7be3f7 (including its prepared WIP). Transport substitution
// only: all work/commands/receipts and writer/WAL outcomes come from real Rust.
final class WiredAssetReadTransport: CloudOwnTransport, Sendable {
    private let operations = Mutex<[String: AssetExecutorOperation]>([:])
    let preparedNames = Mutex<[String]>([])
    func install(_ operation: AssetExecutorOperation, for name: String) {
        operations.withLock { $0[name] = operation }
    }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .assetRead(let zone, let name) = command else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(zone.hasPrefix("c-"))
        preparedNames.withLock { $0.append(name) }
        return try #require(operations.withLock { $0[name] })
    }
}

actor WiredAssetReadCompletions {
    private var count = 0
    private var waiters: [(Int, CheckedContinuation<Void, Never>)] = []
    func record() {
        count += 1
        let ready = waiters.filter { $0.0 <= count }
        waiters.removeAll { $0.0 <= count }
        for (_, waiter) in ready { waiter.resume() }
    }
    func wait(for target: Int) async {
        if count >= target { return }
        await withCheckedContinuation { waiters.append((target, $0)) }
    }
}

func withWiredAssetReadFixture(
    _ body: (CloudKitBridgeFixture, CoreCloudPolicyService, NativeCloudPolicyExecutor, WiredAssetReadTransport, WiredAssetReadCompletions) async throws -> Void
) async throws {
    try await withNativePolicyFixture { original in
        let transport = WiredAssetReadTransport()
        let completions = WiredAssetReadCompletions()
        let adapter = CloudKitSyncAdapter(core: original.core, engineFactory: original.factory,
            eventDidPersist: { await original.probe.didPersist($0) }, assetReadTransport: transport,
            assetReadDidFinish: { await completions.record() })
        let fixture = CloudKitBridgeFixture(directory: original.directory, collection: original.collection,
            core: original.core, adapter: adapter, factory: original.factory, probe: original.probe,
            names: original.names, decks: original.decks, reviewDeck: original.reviewDeck,
            reviewCard: original.reviewCard, reviewName: original.reviewName, zone: original.zone,
            incoming: original.incoming, root: original.root)
        do {
            let policy = try await assetExecutorPolicy(fixture)
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: adapter)
            try await body(fixture, policy, executor, transport, completions)
        } catch { try? await adapter.stop(); throw error }
        try? await adapter.stop()
    }
}

func wiredAssetRetryCommand(_ fixture: CloudKitBridgeFixture, policy: CoreCloudPolicyService,
                           records: [AssetExecutorRemote] = []) async throws -> CloudPolicyCommand {
    let sdk = try await fixture.engine().instanceID
    try await fixture.core.fetched(sdk: sdk, zone: fixture.zone, records: records.map(\.record), deleted: [])
    let fetch = try await fixture.core.beginFetch(sdk: sdk, zones: [fixture.zone])
    let group = "wired-asset-\(fetch)"
    try await fixture.core.willFetch(sdk: sdk, group: group, manual: true)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: true, failed: false)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: false, failed: false)
    try await fixture.core.didFetch(sdk: sdk, group: group, manual: true)
    try #require(try await fixture.core.finishFetch(sdk: sdk, id: fetch))
    guard case .commands(let commands) = try await policy.input(.refresh(.cloud)) else {
        throw BridgeError.internal("Expected real drain-issued retry")
    }
    return try #require(commands.first { if case .retryAssetEntries = $0.action { true } else { false } })
}

func wiredAssetExchangeRuntimeID(_ command: CloudPolicyCommand) async throws -> UInt64 {
    let raw = command.raw
    return try await MaintenanceLane.shared.call(.testCloudPolicyExchangeObservation) {
        let observed = try MeguriCoreBindings.testCloudPolicyExchangeObservation(command: raw)
        return try #require(observed.mappedRuntimeId)
    }
}
