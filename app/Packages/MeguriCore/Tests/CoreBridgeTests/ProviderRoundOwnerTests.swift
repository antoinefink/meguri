import Foundation
import Instrumentation
import Synchronization
import Testing
@testable import CoreBridge

// Only secure-store values and read timing are substituted. Every connection,
// handoff and round refusal below comes from the real CoreProviderService.
private actor RoundCredentialStore: MeguriProviderCredentialStore {
    enum Failure: Error { case read }
    var value: MeguriProviderCredentials?
    var heldRead: (gate: CloudCallbackGate, onCancel: (@Sendable () -> Void)?)?
    var failsRead = false
    var deletions: [MeguriProviderConnection] = []
    var reads: [MeguriProviderConnection] = []
    func hold(_ gate: CloudCallbackGate, onCancel: (@Sendable () -> Void)? = nil) {
        heldRead = (gate, onCancel)
    }
    func failRead(_ value: Bool) { failsRead = value }
    func load(connection: MeguriProviderConnection) async throws -> MeguriProviderCredentials? {
        reads.append(connection)
        let original = value
        if let held = heldRead {
            heldRead = nil
            await withTaskCancellationHandler {
                await held.gate.enter()
            } onCancel: { held.onCancel?() }
        }
        if failsRead { throw Failure.read }
        return original
    }
    func save(_ value: MeguriProviderCredentials) { self.value = value }
    func delete(connection: MeguriProviderConnection) { deletions.append(connection); value = nil }
}

private let roundStarts: Set<BridgeCall> = [.providerStartCollectionRound, .providerStartMediaRound]
private let roundNetworkStarts: Set<BridgeCall> = [
    .providerStartLogin, .providerStartCollectionRound, .providerStartMediaRound,
    .providerBootstrapPrepare, .providerBootstrapConfirm, .providerBootstrapMediaPrepare,
    .providerBootstrapMedia, .providerBootstrapComplete,
]

@MainActor private func roundWait(_ condition: () async -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(20)
    while ContinuousClock.now < deadline {
        if await condition() { return }
        await Task.yield()
    }
    throw BridgeError.unavailable(reason: "Synthetic round rendezvous did not arrive")
}

@MainActor private func withRoundCollection(
    _ body: @MainActor (CoreCollectionService, CoreBridge.CollectionLocation, CoreProviderService) async throws -> Void
) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-round-owner-\(UUID().uuidString)")
    let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
        backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget"),
        manualTransferDirectory: root.appending(path: "installation"))
    for directory in [location.collectionDirectory, location.backupsDirectory, location.manualTransferDirectory] {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }
    defer { try? FileManager.default.removeItem(at: root) }
    _ = try await CoreProviderService.migrate(directory: location.manualTransferDirectory, legacyJSON: nil)
    try await MaintenanceLane.shared.bootstrapCollection(location)
    let collection = CoreCollectionService()
    do {
        try await collection.open(location, randomSeed: 921)
        try await body(collection, location, try await CoreProviderService.attached())
        try await collection.close()
    } catch { try? await collection.close(); throw error }
}

@MainActor private func roundOwner(_ core: CoreProviderService, _ store: RoundCredentialStore) async throws -> MeguriProviderConnectionOwner {
    let owner = MeguriProviderConnectionOwner(service: core, store: store)
    try await owner.configure(endpoint: "https://round.synthetic.example/")
    let configured = try #require(try await core.configuredMeguri())
    await store.save(.init(connection: configured.connection, username: "synthetic-user", hostKey: "synthetic-key"))
    try await owner.load()
    return owner
}

@MainActor private func expectRoundRefusal(_ report: MeguriRoundReport, selected: Bool = false) {
    if selected {
        guard case .refused(.notReady) = report.collection.outcome else { Issue.record("Core lost NotReady refusal"); return }
    } else {
        guard case .refused(.notSelected) = report.collection.outcome else { Issue.record("Core lost NotSelected refusal"); return }
    }
    #expect(report.media == nil && !report.complete)
    #expect(report.collection.finalization == nil && report.collection.trackingFailure == nil)
}

// Shared Host/event listener lifetimes use the existing serialized suite.
// No successful worker, ready binding or server reply is fabricated here.
extension EventTests {
    #if DEBUG
    @Test(arguments: [false, true])
    @MainActor func ProviderRoundFactoryIsCachedAndCoreAloneRefusesUnreadyWork(selected: Bool) async throws {
        try await withRoundCollection { _, _, core in
            let store = RoundCredentialStore()
            let owner = MeguriProviderConnectionOwner(service: core, store: store)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                await #expect(throws: MeguriRoundAdmissionFailure.connectionUnavailable) { _ = try await owner.roundService() }
                try await owner.configure(endpoint: "https://round.synthetic.example/")
                let signedOut = try await owner.roundService()
                #expect(try await owner.roundService() === signedOut)
                #expect(await store.reads.isEmpty)
                try await owner.load()
                let loaded = try await owner.roundService()
                #expect(loaded !== signedOut)
                await #expect(throws: MeguriRoundAdmissionFailure.retired) { _ = try await signedOut.fullRound() }
                let configured = try #require(try await core.configuredMeguri())
                await store.save(.init(connection: configured.connection, username: "synthetic-user", hostKey: "synthetic-key"))
                try await owner.load()
                if selected {
                    let handoff = try await core.beginHandoff(to: .meguri)
                    await owner.cancelAndDrain()
                    _ = try await core.finishHandoff(handoff)
                    try await owner.load()
                }
                let facade = try await owner.roundService()
                #expect(try await owner.roundService() === facade)
                let connection = await facade.connection
                #expect(connection == configured.connection)
                #expect(await owner.phase == .setupRequired)
                #expect(calls.withLock { Set($0).isDisjoint(with: roundNetworkStarts) })
                expectRoundRefusal(try await facade.fullRound(), selected: selected)
                #expect(calls.withLock { $0.filter { $0 == .providerStartCollectionRound }.count } == 1)
                #expect(!calls.withLock { $0.contains(.providerStartMediaRound) || $0.contains(.jobProgressSetListener) })
                await owner.cancelAndDrain()
            }
        }
    }
    #endif

    #if DEBUG
    @Test @MainActor func ProviderRoundMissingWrongAndFailedCredentialReadsNeverStart() async throws {
        try await withRoundCollection { _, _, core in
            let store = RoundCredentialStore()
            let owner = try await roundOwner(core, store)
            let facade = try await owner.roundService()
            let exact = try #require(await store.value)
            let calls = Mutex<[BridgeCall]>([])
            await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                await store.delete(connection: exact.connection)
                await #expect(throws: MeguriRoundAdmissionFailure.credentialsMissing) { _ = try await facade.fullRound() }
                for field in ["reference", "revision", "endpoint"] {
                    let wrong = MeguriProviderConnection(reference: field == "reference" ? "foreign" : exact.connection.reference,
                        revision: exact.connection.revision + (field == "revision" ? 1 : 0),
                        endpoint: field == "endpoint" ? "https://foreign.synthetic.example/" : exact.connection.endpoint)
                    await store.save(.init(connection: wrong, username: exact.username, hostKey: exact.hostKey))
                    await #expect(throws: MeguriProviderCredentialFailure.wrongConnection) { _ = try await facade.fullRound() }
                }
                await store.save(exact)
                await store.failRead(true)
                await #expect(throws: RoundCredentialStore.Failure.read) { _ = try await facade.fullRound() }
                #expect(calls.withLock { Set($0).isDisjoint(with: roundNetworkStarts) })
                await store.failRead(false)
            }
            expectRoundRefusal(try await facade.fullRound())
            await owner.cancelAndDrain()
        }
    }
    #endif

    #if DEBUG
    @Test(arguments: ["endpoint", "delete", "retire", "load", "signIn"])
    @MainActor func ProviderRoundEveryOwnerTransitionJoinsHeldReadAndPermanentlyRetiresOriginal(action: String) async throws {
        try await withRoundCollection { _, _, core in
            let store = RoundCredentialStore()
            let owner = try await roundOwner(core, store)
            let facade = try await owner.roundService()
            let exact = try #require(await store.value)
            let deletionCount = await store.deletions.count
            let read = CloudCallbackGate()
            let finished = Mutex(false)
            let roundFinished = Mutex(false)
            let calls = Mutex<[BridgeCall]>([])
            await store.hold(read)
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                let running = Task {
                    defer { roundFinished.withLock { $0 = true } }
                    return try await facade.fullRound()
                }
                var transition: Task<Void, any Error>?
                do {
                    try await roundWait { await read.pendingWaiterCount() == 1 }
                    let overlap: Result<MeguriRoundReport, any Error>
                    do { overlap = .success(try await facade.fullRound()) } catch { overlap = .failure(error) }
                    let base = await owner.revision
                    let task = Task {
                        defer { finished.withLock { $0 = true } }
                        switch action {
                        case "endpoint": try await owner.configure(endpoint: "https://successor.synthetic.example/")
                        case "delete": try await owner.deleteCredentialsAfterDrain()
                        case "load": try await owner.load()
                        case "signIn":
                            // Real Core rejects this username before transport.
                            try await owner.signIn(username: "", password: "synthetic-password")
                        default: await owner.cancelAndDrain()
                        }
                    }
                    transition = task
                    try await roundWait { await owner.revision == base + 1 }
                    let returnedWhileHeld = finished.withLock { $0 }
                    let valueWhileHeld = await store.value
                    let deletionsWhileHeld = await store.deletions.count
                    let callsWhileHeld = calls.withLock { $0 }
                    let factoryWhileHeld: Result<CoreMeguriRoundService, any Error>
                    do { factoryWhileHeld = .success(try await owner.roundService()) }
                    catch { factoryWhileHeld = .failure(error) }
                    await read.open()
                    try await roundWait { finished.withLock { $0 } && roundFinished.withLock { $0 } }
                    let originalResult = await running.result
                    let transitionResult = await task.result

                    // Release and join every held original before assertions.
                    #expect(!returnedWhileHeld)
                    #expect(valueWhileHeld == exact && deletionsWhileHeld == deletionCount)
                    #expect(Set(callsWhileHeld).isDisjoint(with: roundNetworkStarts))
                    guard case .failure(MeguriRoundAdmissionFailure.busy) = overlap else { throw BridgeError.internal("Held read did not reserve round") }
                    guard case .failure(MeguriRoundAdmissionFailure.connectionUnavailable) = factoryWhileHeld else { throw BridgeError.internal("Transition left factory available") }
                    guard case .failure(MeguriRoundAdmissionFailure.retired) = originalResult else { throw BridgeError.internal("Original read survived retirement") }
                    if action == "signIn" {
                        guard case .failure(MeguriProviderAuthenticationFailure.username) = transitionResult else { throw BridgeError.internal("Expected real Core username refusal") }
                        #expect(calls.withLock { $0.filter { $0 == .providerStartLogin }.count } == 1)
                        #expect(await owner.phase == .blocked)
                    } else { try transitionResult.get() }
                    #expect(calls.withLock { Set($0).isDisjoint(with: roundStarts) })
                    await facade.cancelAndDrain()
                    await #expect(throws: MeguriRoundAdmissionFailure.retired) { _ = try await facade.fullRound() }

                    let configured = try #require(try await core.configuredMeguri())
                    if action == "endpoint" {
                        #expect(configured.connection.endpoint == "https://successor.synthetic.example/")
                        #expect(await store.deletions.last == exact.connection)
                    } else { #expect(configured.connection == exact.connection) }
                    if action == "delete" { #expect(await store.value == nil) }
                    await store.save(.init(connection: configured.connection, username: exact.username, hostKey: exact.hostKey))
                    try await owner.load()
                    let successor = try await owner.roundService()
                    #expect(successor !== facade)
                    #expect(try await owner.roundService() === successor)
                    expectRoundRefusal(try await successor.fullRound())
                    #expect(calls.withLock { $0.filter { $0 == .providerStartCollectionRound }.count } == 1)
                    #expect(!calls.withLock { $0.contains(.providerStartMediaRound) })
                    await #expect(throws: MeguriRoundAdmissionFailure.retired) { _ = try await facade.fullRound() }
                    await owner.cancelAndDrain()
                } catch {
                    running.cancel(); transition?.cancel()
                    await read.open()
                    _ = await running.result; _ = await transition?.result
                    await owner.cancelAndDrain()
                    throw error
                }
            }
        }
    }
    #endif

    #if DEBUG
    @Test @MainActor func ProviderRoundOrdinaryCancellationJoinsReadAndAllowsRetry() async throws {
        try await withRoundCollection { _, _, core in
            let store = RoundCredentialStore()
            let owner = try await roundOwner(core, store)
            let facade = try await owner.roundService()
            let read = CloudCallbackGate()
            let finished = Mutex(false)
            let credentialCancelled = Mutex(false)
            let calls = Mutex<[BridgeCall]>([])
            await store.hold(read, onCancel: { credentialCancelled.withLock { $0 = true } })
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                let running = Task { try await facade.fullRound() }
                var cancelling: Task<Void, Never>?
                do {
                    try await roundWait { await read.pendingWaiterCount() == 1 }
                    let task = Task { await facade.cancelAndDrain(); finished.withLock { $0 = true } }
                    cancelling = task
                    try await roundWait { credentialCancelled.withLock { $0 } }
                    let returnedWhileHeld = finished.withLock { $0 }
                    #expect(!returnedWhileHeld)
                    await read.open()
                    try await roundWait { finished.withLock { $0 } }
                    let result = await running.result
                    await task.value
                    guard case .failure(is CancellationError) = result else { throw BridgeError.internal("Cancelled read reached Core") }
                    #expect(calls.withLock { Set($0).isDisjoint(with: roundStarts) })
                    #expect(try await owner.roundService() === facade)
                    expectRoundRefusal(try await facade.fullRound())
                    await owner.cancelAndDrain()
                } catch {
                    running.cancel(); cancelling?.cancel()
                    await read.open()
                    _ = await running.result; await cancelling?.value
                    await owner.cancelAndDrain()
                    throw error
                }
            }
        }
    }
    #endif

    @Test @MainActor func ProviderRoundFreshReopenedOwnerCannotReviveOldFacade() async throws {
        try await withRoundCollection { collection, location, core in
            let store = RoundCredentialStore()
            let owner = try await roundOwner(core, store)
            let original = try await owner.roundService()
            await owner.cancelAndDrain()
            try await collection.close()
            try await collection.open(location, randomSeed: 922)
            let current = MeguriProviderConnectionOwner(service: try await CoreProviderService.attached(), store: store)
            try await current.load()
            let successor = try await current.roundService()
            #expect(successor !== original)
            expectRoundRefusal(try await successor.fullRound())
            await original.cancelAndDrain()
            await #expect(throws: MeguriRoundAdmissionFailure.retired) { _ = try await original.fullRound() }
            await current.cancelAndDrain()
        }
    }
}
