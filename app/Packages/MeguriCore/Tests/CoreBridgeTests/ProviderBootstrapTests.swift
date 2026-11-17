import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

private actor BootstrapCredentialStore: MeguriProviderCredentialStore {
    var value: MeguriProviderCredentials?
    var heldRead: CloudCallbackGate?
    var deletions: [MeguriProviderConnection] = []
    var reads: [MeguriProviderConnection] = []
    func hold(_ gate: CloudCallbackGate) { heldRead = gate }
    func load(connection: MeguriProviderConnection) async throws -> MeguriProviderCredentials? {
        reads.append(connection)
        let original = value
        if let gate = heldRead { heldRead = nil; await gate.enter() }
        return original
    }
    func save(_ value: MeguriProviderCredentials) { self.value = value }
    func delete(connection: MeguriProviderConnection) { deletions.append(connection); value = nil }
}
private let bootstrapStarts: Set<BridgeCall> = [
    .providerBootstrapPrepare, .providerBootstrapConfirm, .providerBootstrapMediaPrepare,
    .providerBootstrapMedia, .providerBootstrapComplete,
]
@MainActor private func bootstrapWait(_ condition: () async -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(20)
    while ContinuousClock.now < deadline {
        if await condition() { return }
        await Task.yield()
    }
    throw BridgeError.unavailable(reason: "Synthetic bootstrap rendezvous did not arrive")
}
@MainActor private func withBootstrapCollection(
    _ body: @MainActor (CoreCollectionService, CoreBridge.CollectionLocation, CoreProviderService) async throws -> Void
) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-bootstrap-\(UUID().uuidString)")
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
        try await collection.open(location, randomSeed: 919)
        let core = try await CoreProviderService.attached()
        try await body(collection, location, core)
        try await collection.close()
    } catch { try? await collection.close(); throw error }
}
@MainActor private func bootstrapOwner(_ core: CoreProviderService, _ store: BootstrapCredentialStore) async throws -> MeguriProviderConnectionOwner {
    let owner = MeguriProviderConnectionOwner(service: core, store: store)
    try await owner.configure(endpoint: "https://bootstrap.synthetic.example/")
    let configured = try #require(try await core.configuredMeguri())
    await store.save(.init(connection: configured.connection, username: "synthetic-user", hostKey: "synthetic-key"))
    try await owner.load()
    return owner
}

// All shared Host lifetimes join the existing serialized EventTests family.
extension EventTests {
    @Test @MainActor func ProviderBootstrapNotSelectedHasNoWorkerInEitherDirection() async throws {
        try await withBootstrapCollection { _, _, core in
            let store = BootstrapCredentialStore()
            let owner = try await bootstrapOwner(core, store)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                let facade = try await owner.bootstrapService()
                #expect(try await owner.bootstrapService() === facade)
                guard case .notSelected = try await facade.readiness() else { Issue.record("Readiness lost NotSelected"); return }
                #expect(calls.withLock { Set($0).isDisjoint(with: bootstrapStarts) })
                for direction in [MeguriBootstrapDirection.useServerCollection, .useDeviceCollection] {
                    let result = try await facade.prepare(direction)
                    guard case .refused(.notSelected, let accounting) = result.outcome else {
                        Issue.record("Expected the real synchronous NotSelected refusal"); continue
                    }
                    #expect(accounting == nil && result.jobID == nil && result.retryOperation == nil)
                    #expect(result.mediaProgress == nil && result.progressFailure == nil)
                }
                #expect(calls.withLock { $0.filter { $0 == .providerBootstrapPrepare }.count } == 2)
                #expect(!calls.withLock { $0.contains(.jobProgressSetListener) })
                #expect(calls.withLock { Set($0).intersection(bootstrapStarts) } == [.providerBootstrapPrepare])
            }
            await owner.cancelAndDrain()
        }
    }

    @Test(arguments: ["endpoint", "delete", "retire", "load"])
    @MainActor func ProviderBootstrapHeldReadCannotStartAfterRetirement(action: String) async throws {
        try await withBootstrapCollection { _, _, core in
            let store = BootstrapCredentialStore()
            let owner = try await bootstrapOwner(core, store)
            let facade = try await owner.bootstrapService()
            let original = try #require(await store.value)
            let deletions = await store.deletions.count
            let read = CloudCallbackGate()
            let drained = Mutex(false)
            let calls = Mutex<[BridgeCall]>([])
            await store.hold(read)
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                let preparing = Task { try await facade.prepare(.useDeviceCollection) }
                var retiring: Task<Void, any Error>?
                do {
                    try await bootstrapWait { await read.pendingWaiterCount() == 1 }
                    await #expect(throws: MeguriBootstrapAdmissionFailure.busy) { _ = try await facade.prepare(.useServerCollection) }
                    let base = await owner.revision
                    let task = Task {
                        switch action {
                        case "endpoint": try await owner.configure(endpoint: "https://successor.synthetic.example/")
                        case "delete": try await owner.deleteCredentialsAfterDrain()
                        case "load": try await owner.load()
                        default: await owner.cancelAndDrain()
                        }
                        drained.withLock { $0 = true }
                    }
                    retiring = task
                    try await bootstrapWait { await owner.revision == base + 1 }
                    #expect(!drained.withLock { $0 })
                    #expect(await store.value == original)
                    #expect(await store.deletions.count == deletions)
                    #expect(calls.withLock { Set($0).isDisjoint(with: bootstrapStarts) })
                    await #expect(throws: MeguriBootstrapAdmissionFailure.connectionUnavailable) { _ = try await owner.bootstrapService() }
                    await read.open()
                    await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await preparing.value }
                    try await task.value
                    #expect(drained.withLock { $0 })
                    #expect(calls.withLock { Set($0).isDisjoint(with: bootstrapStarts) })
                    await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await facade.prepare(.useDeviceCollection) }
                    if action == "retire" || action == "delete" { try await owner.load() }
                    let successor = try await owner.bootstrapService()
                    #expect(successor !== facade)
                    if action == "endpoint" {
                        #expect(successor.connection.endpoint == "https://successor.synthetic.example/")
                        #expect(await store.deletions.last == original.connection)
                    } else { #expect(successor.connection == original.connection) }
                    await owner.cancelAndDrain()
                } catch {
                    preparing.cancel(); retiring?.cancel()
                    await read.open()
                    _ = await preparing.result; _ = await retiring?.result
                    await owner.cancelAndDrain()
                    throw error
                }
            }
        }
    }

    @Test @MainActor func ProviderBootstrapFreshHostAttachmentCannotReviveOldFacade() async throws {
        try await withBootstrapCollection { collection, location, core in
            let store = BootstrapCredentialStore()
            let owner = try await bootstrapOwner(core, store)
            let original = try await owner.bootstrapService()
            await owner.cancelAndDrain()
            try await collection.close()
            try await collection.open(location, randomSeed: 920)
            let current = MeguriProviderConnectionOwner(service: try await CoreProviderService.attached(), store: store)
            try await current.load()
            let successor = try await current.bootstrapService()
            #expect(successor !== original)
            guard case .notSelected = try await successor.readiness() else { Issue.record("Fresh Host lost NotSelected"); return }
            await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await original.readiness() }
            await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await original.prepare(.useServerCollection) }
            await current.cancelAndDrain()
        }
    }

    @Test @MainActor func ProviderBootstrapMissingAndMismatchedCredentialsNeverStartFFI() async throws {
        try await withBootstrapCollection { _, _, core in
            let store = BootstrapCredentialStore()
            let owner = try await bootstrapOwner(core, store)
            let facade = try await owner.bootstrapService()
            let exact = try #require(await store.value)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                await store.delete(connection: exact.connection)
                await #expect(throws: MeguriBootstrapAdmissionFailure.credentialsMissing) { _ = try await facade.prepare(.useDeviceCollection) }
                for field in ["reference", "revision", "endpoint"] {
                    let wrong = MeguriProviderConnection(reference: field == "reference" ? "foreign" : exact.connection.reference,
                        revision: exact.connection.revision + (field == "revision" ? 1 : 0),
                        endpoint: field == "endpoint" ? "https://foreign.synthetic.example/" : exact.connection.endpoint)
                    await store.save(.init(connection: wrong, username: exact.username, hostKey: exact.hostKey))
                    await #expect(throws: MeguriProviderCredentialFailure.wrongConnection) { _ = try await facade.prepare(.useServerCollection) }
                }
                #expect(calls.withLock { Set($0).isDisjoint(with: bootstrapStarts) })
            }
            await owner.cancelAndDrain()
        }
    }
}

@Suite struct ProviderBootstrapMappingTests {
    @Test func ProviderBootstrapReceiptKeepsUnknownAcceptanceAndMediaEffects() {
        let receipt = MeguriBootstrapReceipt(.init(direction: .export, phase: .mediaIncomplete, acceptance: .possiblyAccepted,
            boundary: .remoteCompletionUnknown, local: .init(cards: 1, notes: 2, reviews: 3, pendingReviews: 4, pendingRows: [5]),
            remote: .init(modifiedMs: 11, schemaModifiedMs: 12, revision: 13, mediaRevision: 14),
            remoteHasNoCards: false, servedCounts: .init(cards: 6, notes: 7, reviews: 8, pendingReviews: 9, pendingRows: [10]),
            copies: [.init(name: "server.synthetic", length: 42, sha256: Data(repeating: 3, count: 32), verifiedPackage: false, instantMs: 15, server: true)],
            media: .init(stage: .interrupted, uploadedFiles: 16, downloadedFiles: 17, uploadedDeletions: 18, downloadedDeletions: 19,
                         oversized: ["oversized.synthetic"], empty: ["empty.synthetic"], awaiting: 20, remoteOutcomeUnknown: true)))
        #expect(receipt.acceptance == .possiblyAccepted && receipt.boundary == .remoteCompletionUnknown)
        #expect(receipt.direction == .export && receipt.phase == .mediaIncomplete)
        #expect(receipt.local?.pendingRows == [5] && receipt.servedCounts?.pendingRows == [10])
        #expect(receipt.remote?.mediaRevision == 14 && receipt.remoteHasNoCards == false)
        #expect(receipt.copies.first?.server == true && receipt.copies.first?.instantMilliseconds == 15)
        #expect(receipt.media.stage == .interrupted && receipt.media.remoteOutcomeUnknown)
        #expect(receipt.media.uploadedFiles == 16 && receipt.media.downloadedFiles == 17)
        #expect(receipt.media.uploadedDeletions == 18 && receipt.media.downloadedDeletions == 19)
        #expect(receipt.media.oversized == ["oversized.synthetic"] && receipt.media.empty == ["empty.synthetic"] && receipt.media.awaiting == 20)
    }
    @Test func ProviderBootstrapRefusalPreservesAccountingAndNestedCommittedFailure() throws {
        let accounting = MeguriCoreBindings.ProviderMediaAccounting(files: [
            .init(name: "complete.synthetic", lossyName: false, state: .transferred, exclusion: nil),
            .init(name: "pending.synthetic", lossyName: false, state: .pending, exclusion: nil),
            .init(name: "empty.synthetic", lossyName: false, state: .excluded, exclusion: .empty),
            .init(name: "large.synthetic", lossyName: false, state: .excluded, exclusion: .overLimit),
            .init(name: "lossy.synthetic", lossyName: true, state: .excluded, exclusion: .invalidName),
            .init(name: "directory.synthetic", lossyName: false, state: .excluded, exclusion: .notRegularFile),
        ], remotePending: 7, transferred: 1)
        let raw = MeguriCoreBindings.TransferFailure(issue: .mediaIncomplete, boundary: .activationInterrupted,
            generation: 42, candidateActive: true, nextEligibleMs: 99, serverKind: nil, requestMethod: nil, redirect: nil)
        for value in [nil, accounting] as [MeguriCoreBindings.ProviderMediaAccounting?] {
            let mapped = try #require(MeguriProviderBootstrapService.displayOutcome(.refused(failure: .transfer(failure: raw), accounting: value)))
            guard case .refused(.transfer(let failure), let files) = mapped else { Issue.record("Lost typed refusal"); return }
            #expect(failure.candidateActive && failure.generation == 42 && failure.nextEligibleMilliseconds == 99)
            #expect(failure.issue == .mediaIncomplete && failure.boundary == .activationInterrupted)
            #expect((files == nil) == (value == nil))
            if let files {
                #expect(files.files.map(\.name) == accounting.files.map(\.name))
                #expect(files.files.map(\.state) == [.transferred, .pending, .excluded, .excluded, .excluded, .excluded])
                #expect(files.files.map(\.exclusion) == [nil, nil, .empty, .overLimit, .invalidName, .notRegularFile])
                #expect(files.files[4].lossyName && files.remotePending == 7 && files.transferred == 1)
            }
        }
    }
    @Test func ProviderBootstrapProgressHasOnlyActualObservedCounters() {
        let progress = MeguriBootstrapMediaProgress(.init(phase: .confirming, checked: 1, uploadedFiles: 2,
            downloadedFiles: 3, bytesUploaded: 4, bytesDownloaded: 5, awaiting: 6))
        #expect(progress.phase == .confirming && progress.checked == 1)
        #expect(progress.uploadedFiles == 2 && progress.downloadedFiles == 3)
        #expect(progress.bytesUploaded == 4 && progress.bytesDownloaded == 5 && progress.awaiting == 6)
    }
}
