@testable import CoreBridge
import Instrumentation
import Synchronization
import Foundation
import Testing
@testable import AppModel

/// The daily-driver Release build's inputs: its production iCloud container is
/// declared and its schema is not configured, so the installation cannot use
/// iCloud. Off and Meguri Sync must work there as in any other build.
extension ReviewCollectionStartupTests {
    private actor UnconfiguredCloudCredentials: MeguriProviderCredentialStore {
        private var value: MeguriProviderCredentials?
        private(set) var savedCount = 0
        func load(connection: MeguriProviderConnection) -> MeguriProviderCredentials? { value }
        func save(_ credentials: MeguriProviderCredentials) { value = credentials; savedCount += 1 }
        func delete(connection: MeguriProviderConnection) throws { value = nil }
    }
    @MainActor private final class UnconfiguredCloudApp {
        let owner: ReviewCollection
        var sync: SyncCoordinator { owner.sync }
        init(root: URL, suite: String, store: any MeguriProviderCredentialStore) {
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
            configuration.timeZone = TimeZone(secondsFromGMT: 0)
            // The values the Release build of the daily-driver ids resolves:
            // MeguriCloudEnvironment production, MeguriCloudSchemaConfigured false.
            let sync = SyncCoordinator(directory: root,
                defaults: UserDefaults(suiteName: suite), meguriCredentialStore: store)
            owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
                providerDirectory: root.appending(path: "synthetic-installation", directoryHint: .isDirectory))
            owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
            owner.studyPreparation.runsUnprompted = false
        }
    }
    private func withUnconfiguredCloudInstallation(_ body: @MainActor (URL, String, UnconfiguredCloudCredentials) async throws -> Void) async throws {
        let files = FileManager.default
        let temporary = try #require(realpath(files.temporaryDirectory.path, nil))
        defer { free(temporary) }
        let root = URL(filePath: String(cString: temporary)).appending(path: "synthetic-unconfigured-cloud-\(UUID().uuidString)")
        try files.createDirectory(at: root.appending(path: "synthetic-installation"), withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        let suite = "synthetic-unconfigured-cloud-\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        try await body(root, suite, UnconfiguredCloudCredentials())
    }

    @Test(.timeLimit(.minutes(2))) func firstLaunchWithoutICloudSetupSelectsMeguriSyncAndReachesSignIn() async throws {
        try await withUnconfiguredCloudInstallation { root, suite, store in
            let app = UnconfiguredCloudApp(root: root, suite: suite, store: store)
            do {
                let calls = Mutex<[BridgeCall]>([])
                try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                    // First launch ever: no collection file, nothing selected.
                    await app.owner.start()
                    try #require(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
                    try #require((app.sync.providerState?.selection ?? .localOnly) == .localOnly)
                    #expect(app.sync.setupState == .configurationUnavailable && !app.sync.operationFailed)
                    // The Service control: Off to Meguri Sync.
                    let chosen = await app.owner.changeSyncService(to: .meguri, from: app.sync.providerState?.selection)
                    #expect(chosen == .changed, "\(String(describing: app.sync.serviceChangeFailure))")
                    await app.sync.waitForInputs(); await app.sync.waitForMeguriAction()
                    let sync = app.sync
                    try #require(sync.providerState?.selection == .meguri && sync.providerState?.phase == .setupRequired)
                    #expect(sync.serviceChangeFailure == nil && sync.meguriSignInAttached && !sync.operationFailed)
                    // iCloud stays what it was: unavailable to this installation.
                    #expect(sync.setupState == .configurationUnavailable && sync.connectionService == nil && sync.status == nil)
                    #expect(calls.withLock { !$0.contains(.configureCloudPlatform) })
                    #expect(try await CoreProviderService.attached().refresh().selection == .meguri)
                    // Sign In reaches the real configured login. A username the
                    // core refuses before any request keeps the test off the network.
                    let endpoint = "https://unconfigured-cloud.synthetic.example/"
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
                    await app.owner.startEmpty()
                    try #require(app.owner.state == .ready, "\(app.owner.error ?? "Creation did not become ready")")
                    await app.sync.waitForInputs(); await app.sync.waitForReplacement(); await app.sync.waitForMeguriAction()
                    #expect(app.sync.providerState?.selection == .meguri && app.sync.meguriSignInAttached && !app.sync.operationFailed)
                    #expect(app.sync.generation == app.owner.generation)
                }
                try #require(await app.owner.close())
            } catch { _ = await app.owner.close(); throw error }

            // The next launch: the stored selection, an existing collection.
            let relaunched = UnconfiguredCloudApp(root: root, suite: suite, store: store)
            do {
                await relaunched.owner.start()
                try #require(relaunched.owner.state == .ready, "\(relaunched.owner.error ?? "Open did not become ready")")
                await relaunched.sync.waitForInputs(); await relaunched.sync.waitForMeguriAction()
                let later = relaunched.sync
                #expect(later.providerState?.selection == .meguri && later.meguriSignInAttached && !later.operationFailed)
                #expect(later.setupState == .configurationUnavailable && later.connectionService == nil)
                #expect(later.meguriEndpoint == "https://unconfigured-cloud.synthetic.example/")
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

}
