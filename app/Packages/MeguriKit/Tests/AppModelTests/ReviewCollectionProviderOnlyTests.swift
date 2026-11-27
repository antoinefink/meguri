@testable import CoreBridge
import Instrumentation
import Synchronization
import Foundation
import Testing
@testable import AppModel

/// Off and Meguri Sync use the local provider owner in every build.
extension ReviewCollectionStartupTests {
    private actor ProviderOnlyCredentials: MeguriProviderCredentialStore {
        private var value: MeguriProviderCredentials?
        private(set) var savedCount = 0
        private(set) var loadedCount = 0
        func load(connection: MeguriProviderConnection) -> MeguriProviderCredentials? {
            loadedCount += 1
            return value
        }
        func save(_ credentials: MeguriProviderCredentials) { value = credentials; savedCount += 1 }
        func delete(connection: MeguriProviderConnection) throws { value = nil }
    }
    @MainActor private final class ProviderOnlyApp {
        let owner: ReviewCollection
        var sync: SyncCoordinator { owner.sync }
        init(root: URL, suite: String, store: any MeguriProviderCredentialStore) {
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
            configuration.timeZone = TimeZone(secondsFromGMT: 0)
            let sync = SyncCoordinator(directory: root,
                defaults: UserDefaults(suiteName: suite), meguriCredentialStore: store)
            owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
                providerDirectory: root.appending(path: "synthetic-installation", directoryHint: .isDirectory))
            owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
            owner.studyPreparation.runsUnprompted = false
        }
    }
    private func withProviderOnlyInstallation(_ body: @MainActor (URL, String, ProviderOnlyCredentials) async throws -> Void) async throws {
        let files = FileManager.default
        let temporary = try #require(realpath(files.temporaryDirectory.path, nil))
        defer { free(temporary) }
        let root = URL(filePath: String(cString: temporary)).appending(path: "synthetic-provider-only-\(UUID().uuidString)")
        try files.createDirectory(at: root.appending(path: "synthetic-installation"), withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        let suite = "synthetic-provider-only-\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        try await body(root, suite, ProviderOnlyCredentials())
    }

    @Test(.timeLimit(.minutes(2))) func firstLaunchSelectsMeguriSyncAndReachesSignIn() async throws {
        try await withProviderOnlyInstallation { root, suite, store in
            let app = ProviderOnlyApp(root: root, suite: suite, store: store)
            do {
                let calls = Mutex<[BridgeCall]>([])
                try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                    // First launch ever: no collection file, nothing selected.
                    await app.owner.start()
                    try #require(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
                    try #require((app.sync.providerState?.selection ?? .localOnly) == .localOnly)
                    #expect(app.sync.setupState == .prepared && !app.sync.operationFailed)
                    // The Service control: Off to Meguri Sync.
                    let chosen = await app.owner.changeSyncService(to: .meguri, from: app.sync.providerState?.selection)
                    #expect(chosen == .changed, "\(String(describing: app.sync.serviceChangeFailure))")
                    await app.sync.waitForInputs(); await app.sync.waitForMeguriAction()
                    let sync = app.sync
                    try #require(sync.providerState?.selection == .meguri && sync.providerState?.phase == .setupRequired)
                    #expect(sync.serviceChangeFailure == nil && sync.meguriSignInAttached && !sync.operationFailed)
                    #expect(sync.setupState == .attached)

                    #expect(try await CoreProviderService.attached().refresh().selection == .meguri)
                    // Sign In reaches the real configured login. A username the
                    // core refuses before any request keeps the test off the network.
                    let endpoint = "https://provider-only.synthetic.example/"
                    app.owner.signInMeguri(endpoint: endpoint, username: "?", password: "synthetic-unused-password")
                    await app.owner.waitForMeguriSignInPreparation()
                    await sync.waitForMeguriAction()
                    #expect(app.sync === sync && !app.owner.meguriSignInPreparing && !sync.meguriBusy)
                    #expect(sync.meguriSignInErrorKey == "sync.meguri.error.credentials")
                    #expect(calls.withLock { $0.filter { $0 == .providerConfigureMeguri }.count } == 1)
                    #expect(calls.withLock { $0.filter { $0 == .providerStartLogin }.count } == 1)
                    #expect(try await CoreProviderService.attached().configuredMeguri()?.connection.endpoint == endpoint)
                    #expect(await store.savedCount == 0)
                    // Selecting a service created no collection, and Start Empty still works.
                    #expect(try await CoreCollectionService().lifecycle() == .noCollection)
                    #expect(!FileManager.default.fileExists(atPath: app.owner.configuration.collectionLocation.collectionFile.path))
                    try #require(app.owner.canStartEmpty)
                    let previousConnection = try await sync.meguriConnection()
                    let previousLoads = await store.loadedCount
                    await app.owner.startEmpty()
                    try #require(app.owner.state == .ready, "\(app.owner.error ?? "Creation did not become ready")")
                    await app.sync.waitForInputs(); await app.sync.waitForReplacement(); await app.sync.waitForMeguriAction()
                    #expect(app.sync.providerState?.selection == .meguri && app.sync.meguriSignInAttached && !app.sync.operationFailed)
                    #expect(app.sync.generation == app.owner.generation)
                    // Collection creation replaces the provider owner. Its
                    // selected connection must reload once after that drain.
                    #expect(app.sync === sync)
                    #expect(try await sync.meguriConnection() !== previousConnection)
                    #expect(await store.loadedCount == previousLoads + 1)
                    #expect(sync.meguriEndpoint == endpoint && !sync.meguriBusy)
                }
                try #require(await app.owner.close())
            } catch { _ = await app.owner.close(); throw error }

            // The next launch: the stored selection, an existing collection.
            let relaunched = ProviderOnlyApp(root: root, suite: suite, store: store)
            do {
                await relaunched.owner.start()
                try #require(relaunched.owner.state == .ready, "\(relaunched.owner.error ?? "Open did not become ready")")
                await relaunched.sync.waitForInputs(); await relaunched.sync.waitForMeguriAction()
                let later = relaunched.sync
                #expect(later.providerState?.selection == .meguri && later.meguriSignInAttached && !later.operationFailed)
                #expect(later.setupState == .attached)
                #expect(later.meguriEndpoint == "https://provider-only.synthetic.example/")
                // A background and foreground cycle leaves the owner attached and quiet.
                relaunched.owner.setForeground(false); await relaunched.owner.waitForLifecycle()
                relaunched.owner.setForeground(true); await relaunched.owner.waitForLifecycle()
                await later.waitForInputs()
                #expect(relaunched.sync === later && later.meguriSignInAttached && !later.operationFailed)
                #expect(relaunched.owner.state == .ready)
                // Back to Off, as the confirmation sheet asks for it.
                #expect(await relaunched.owner.changeSyncService(to: .localOnly, from: .meguri) == .changed)
                #expect(relaunched.sync.providerState?.selection == .localOnly && relaunched.sync.serviceChangeFailure == nil)
                #expect(relaunched.owner.state == .ready && relaunched.owner.error == nil)
                // Quit with a collection open: the provider-only owner must not hold it.
                try #require(await relaunched.owner.closeForQuit())
            } catch { _ = await relaunched.owner.close(); throw error }
        }
    }

    @Test(arguments: ["derived-store", "thread registry"], [false, true])
    func finalSuspendDeadlineResumesWithoutReplacingUIError(phase: String, existingError: Bool) async throws {
        try await withBackgroundSuspendOwner { owner, _ in
            if existingError {
                owner.finalBackgroundSuspend = { (_: UInt32) async throws(BridgeError) -> SuspendOutcome in
                    throw .unavailable(reason: "Synthetic unrelated suspend failure")
                }
                owner.setForeground(false); await owner.waitForLifecycle()
                try #require(owner.error?.contains("Synthetic unrelated suspend failure") == true)
                owner.setForeground(true); await owner.waitForLifecycle()
            }
            let previousError = owner.error
            var calls = 0
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                calls += 1
                // Exercise real teardown and recovery; inject only the final diagnostic.
                _ = try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
                throw .suspendDeadline(phase: phase)
            }
            owner.setForeground(false); await owner.waitForLifecycle()
            #expect(calls == 1)
            #expect(owner.error == previousError)
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == previousError)
            #expect(owner.foreground && !owner.resuming)
            let deck = try await CoreDeckService().addDeck(name: "After deadline \(phase)")
            #expect(deck.id > 0)
        }
    }

    enum BackgroundSuspendBudget: CaseIterable { case unknown, laterEstimate, exhaustedEstimate, remaining, clamped }

    @Test(arguments: BackgroundSuspendBudget.allCases)
    func finalSuspendUsesDefaultOrRemainingWallBudgetAfterFlush(budget: BackgroundSuspendBudget) async throws {
        try await withBackgroundSuspendOwner { owner, backend in
            let deadline: Int64? = switch budget {
            case .unknown, .laterEstimate, .exhaustedEstimate: nil
            case .remaining: Int64(Date().timeIntervalSince1970 * 1_000) + 60_000
            case .clamped: Int64.max
            }
            var flushed = false
            owner.didFlushLocally = { flushed = true; backend.events.append("local-flush") }
            var calls = 0
            var samples = 0
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                calls += 1
                #expect(flushed)
                #expect(backend.events.contains("background"))
                if let deadline {
                    if budget == .clamped { #expect(timeout == UInt32(Int32.max)) }
                    else {
                        let remaining = Double(deadline) - Date().timeIntervalSince1970 * 1_000
                        #expect(timeout > 1_500)
                        #expect(abs(Double(timeout) - remaining) < 250)
                        #expect(timeout <= 60_000)
                    }
                } else if budget == .laterEstimate {
                    #expect(timeout == 30_000)
                } else {
                    #expect(timeout == 1_500)
                }
                return try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
            }
            let lease = owner.beginBackgroundTask(deadlineMilliseconds: deadline, currentBudgetMilliseconds: {
                samples += 1
                #expect(flushed)
                #expect(backend.events.contains("background"))
                switch budget {
                case .laterEstimate: return 30_000
                case .exhaustedEstimate: return 0
                default: return nil
                }
            })
            try #require(lease != nil)
            await owner.waitForLifecycle()
            #expect(samples == (deadline == nil ? 1 : 0))
            #expect(calls == (budget == .exhaustedEstimate ? 0 : 1) && owner.error == nil)
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == nil && !owner.resuming)
            #expect(try await CoreDeckService().addDeck(name: "After budgeted suspend").id > 0)
        }
    }

    enum BackgroundSuspendSkip: CaseIterable { case past, expired, foreground, superseded }

    @Test(arguments: BackgroundSuspendSkip.allCases)
    func finalSuspendSkipsInvalidLeaseAfterLocalFlush(reason: BackgroundSuspendSkip) async throws {
        try await withBackgroundSuspendOwner { owner, backend in
            var calls = 0
            var flushes = 0
            var token: UInt64?
            let deadline = Int64(Date().timeIntervalSince1970 * 1_000) + 60_000
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                calls += 1
                return try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
            }
            owner.didFlushLocally = {
                flushes += 1
                // Only the first transition changes activity; its successor has its own flush.
                owner.didFlushLocally = nil
                switch reason {
                case .expired: if let token { owner.backgroundTaskExpired(token) }
                case .foreground: owner.setForeground(true)
                case .superseded:
                    owner.setForeground(true)
                    _ = owner.beginBackgroundTask(deadlineMilliseconds: 1)
                case .past: break
                }
            }
            token = try #require(owner.beginBackgroundTask(deadlineMilliseconds:
                reason == .past ? 1 : deadline))
            // A callback may queue a foreground/successor transition. Join that too.
            await owner.waitForLifecycle(); await owner.waitForLifecycle()
            #expect(flushes == 1)
            #expect(calls == 0 && owner.error == nil)
            #expect(backend.events.contains("background"))
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == nil && !owner.resuming)
            #expect(try await CoreDeckService().addDeck(name: "After skipped suspend").id > 0)
        }
    }

    @Test func directBackgroundAfterUnknownExpiredLeaseUsesDefaultBudget() async throws {
        try await withBackgroundSuspendOwner { owner, backend in
            var timeouts: [UInt32] = []
            var token: UInt64?
            owner.didFlushLocally = {
                owner.didFlushLocally = nil
                if let token { owner.backgroundTaskExpired(token) }
            }
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                timeouts.append(timeout)
                return try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
            }
            token = try #require(owner.beginBackgroundTask(deadlineMilliseconds: nil))
            await owner.waitForLifecycle()
            #expect(timeouts.isEmpty)
            #expect(backend.events.contains("background"))
            owner.setForeground(true); await owner.waitForLifecycle()
            owner.setForeground(false); await owner.waitForLifecycle()
            #expect(timeouts == [1_500])
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == nil)
            #expect(try await CoreDeckService().addDeck(name: "After direct suspend").id > 0)
        }
    }

    /// These tests extend the serialized real-Core startup suite. Each owner is closed
    /// before its synthetic directory (and sibling backups) are removed, even on failure.
    private func withBackgroundSuspendOwner(
        _ body: (ReviewCollection, SyncOwnerFake) async throws -> Void
    ) async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let directory = root.appending(path: "collection")
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try files.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        // Deliberately far from wall time: OS budgets must not use the study clock.
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
        do {
            await owner.start()
            try #require(owner.state == .ready)
            await sync.waitForInputs()
            try await body(owner, backend)
            #expect(await owner.close())
        } catch { _ = await owner.close(); throw error }
    }
}
