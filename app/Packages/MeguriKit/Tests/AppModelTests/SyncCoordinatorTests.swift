import Foundation
import Testing
@testable import AppModel
@testable import CoreBridge

@MainActor
struct SyncCoordinatorTests {
    @Test func readyBeforeOpenReturnWaitsForRetainedHostThenStillReplacesAndReopens() async {
        let original = SyncOwnerFake(), successor = SyncOwnerFake(), reopened = SyncOwnerFake()
        let openReturn = SyncOwnerGate()
        var factories = 0, hostRetained = false
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            #expect(hostRetained, "Ready alone must never admit backend or SDK work")
            factories += 1
            switch factories {
            case 1: return original
            case 2:
                #expect(original.closed && original.closeCount == 1)
                return successor
            default:
                #expect(factories == 3 && successor.closed)
                return reopened
            }
        }, defaults: nil)
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation(), sync: sync)
        await sync.preparePlatform()
        // Hold the simulated open return, deliver Ready on the production
        // owner's lifecycle path, and fully join the input/replacement lanes.
        let completion = Task {
            await openReturn.hold()
            hostRetained = true
            await sync.completeOpenedHost(generation: 4)
        }
        await openReturn.waitUntilHeld()
        owner.receiveLifecycle(.opening)
        owner.receiveLifecycle(.ready(generation: 4))
        sync.collectionReplaced(4)
        await sync.waitForReplacement()
        #expect(owner.lifecycle == .ready(generation: 4) && owner.generation == 4)
        #expect(sync.generation == 4 && sync.setupState == .prepared && !sync.operationFailed)
        #expect(factories == 0 && original.events.isEmpty && original.attachmentCount == 0)
        openReturn.release()
        await completion.value
        await sync.waitForInputs()
        #expect(factories == 1 && original.attachmentCount == 1)
        #expect(sync.setupState == .attached && !sync.operationFailed)
        owner.receiveLifecycle(.ready(generation: 4))
        await sync.waitForReplacement()
        #expect(factories == 1)

        owner.receiveLifecycle(.ready(generation: 5))
        sync.collectionReplaced(5)
        await sync.waitForReplacement(); await sync.waitForInputs()
        #expect(factories == 2 && successor.attachmentCount == 1)
        #expect(sync.generation == 5 && sync.setupState == .attached && !sync.operationFailed)
        #expect(await sync.cancelAndDrain())

        // A new open after a successful close needs retention admission again.
        hostRetained = false
        await sync.preparePlatform()
        owner.receiveLifecycle(.ready(generation: 6))
        await sync.waitForReplacement()
        #expect(factories == 2 && reopened.events.isEmpty && reopened.attachmentCount == 0)
        hostRetained = true
        await sync.completeOpenedHost(generation: 6)
        await sync.waitForInputs()
        #expect(factories == 3 && reopened.attachmentCount == 1)
        #expect(sync.generation == 6 && sync.setupState == .attached && !sync.operationFailed)
        #expect(await sync.cancelAndDrain())
    }

    @Test func missingConfigurationTracksLocalGenerationsWithoutConstructingCloudOwner() async {
        let backend = SyncOwnerFake()
        var factories = 0
        let sync = SyncCoordinator(bootstrap: { throw SyncOwnerFailure.configurationUnavailable },
            makeBackend: { factories += 1; return backend }, defaults: nil)
        await sync.preparePlatform()
        sync.receiveLifecycle(.ready(generation: 4))
        await sync.waitForInputs()
        await sync.waitForReplacement()
        await sync.completeOpenedHost(generation: 4)
        sync.collectionReplaced(5)
        sync.receiveLifecycle(.ready(generation: 5))
        sync.networkChanged(reachable: true)
        sync.syncNow(source: .command)
        await sync.waitForInputs()
        await sync.waitForReplacement()
        #expect(sync.generation == 5)
        #expect(sync.setupState == .configurationUnavailable && !sync.operationFailed)
        #expect(factories == 0 && backend.attachmentCount == 0 && backend.events.isEmpty)
        #expect(await sync.cancelAndDrain())
        #expect(backend.closeCount == 0)
    }

    @Test func bootstrapDoesNotReadAccountAndOwnerSurvivesLayouts() async throws {
        let backend = SyncOwnerFake()
        var order: [String] = []
        let sync = SyncCoordinator(bootstrap: { order.append("configure") },
            makeBackend: { order.append("retained-host"); return backend }, defaults: nil)
        let navigation = AppNavigation()
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: navigation, sync: sync)
        await sync.preparePlatform()
        #expect(order == ["configure"])
        #expect(backend.attachmentCount == 0)
        await sync.completeOpenedHost(generation: 4)
        #expect(backend.attachmentCount == 1)
        navigation.adoptLaunchLayout(.compact)
        navigation.adoptLaunchLayout(.sidebar)
        #expect(owner.sync === sync)
        await sync.completeOpenedHost(generation: 4)
        await sync.waitForInputs()
        #expect(order == ["configure", "retained-host"])
        #expect(backend.attachmentCount == 1)
        #expect(await sync.cancelAndDrain())
    }

    @Test func secondReplacementDuringFactoryDrainsStaleCandidateBeforeLatestOwnerAttaches() async {
        let original = SyncOwnerFake(), stale = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate(), staleDrain = SyncOwnerGate()
        var factories = 0
        stale.onClose = { await staleDrain.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            factories += 1
            switch factories {
            case 1: return original
            case 2:
                #expect(original.closed)
                await factory.hold()
                return stale
            default:
                #expect(stale.closed && stale.closeCount == 1)
                return latest
            }
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 10)
        await sync.waitForInputs()
        sync.collectionReplaced(11)
        await factory.waitUntilHeld()
        sync.collectionReplaced(12)
        await sync.waitForInputs()
        factory.release()
        await staleDrain.waitUntilHeld()
        #expect(sync.generation == 12 && factories == 2)
        #expect(stale.events.isEmpty && latest.events.isEmpty)

        await sync.waitForInputs()
        staleDrain.release()
        await sync.waitForReplacement()
        #expect(factories == 3 && sync.setupState == .attached)
        #expect(stale.closed && stale.attachmentCount == 0)

        await sync.waitForInputs()

        await sync.waitForInputs()
        #expect(await sync.cancelAndDrain())
    }

    @Test func attachmentBeforeReplacementClosesStaleCandidateBeforeNextFactory() async {
        let stale = SyncOwnerFake(), successor = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate(), staleDrain = SyncOwnerGate(), finalDrain = SyncOwnerGate()
        var factories = 0
        var observations: [SyncCoordinator.PlatformSignalStage] = []
        var constructing = 0, maximumConstructing = 0
        stale.onClose = { await staleDrain.hold() }
        latest.onClose = { await finalDrain.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            constructing += 1
            maximumConstructing = max(maximumConstructing, constructing)
            defer { constructing -= 1 }
            factories += 1
            if factories == 1 {
                await factory.hold()
                return stale
            }
            #expect(stale.closed && stale.closeCount == 1)
            if factories == 2 { return successor }
            #expect(successor.closed && successor.closeCount == 1)
            return latest
        }, defaults: nil, observeSignals: true)
        sync.observePlatformSignalLifetime = { observations.append($0) }
        await sync.preparePlatform()
        let attachment = Task { await sync.completeOpenedHost(generation: 30) }
        await factory.waitUntilHeld()
        sync.receiveLifecycle(.ready(generation: 31))
        sync.collectionReplaced(32)
        sync.receiveLifecycle(.ready(generation: 32))
        await sync.waitForInputs()
        var repeated: Task<Void, Never>?
        await withCheckedContinuation { started in
            repeated = Task {
                started.resume()
                await sync.completeOpenedHost(generation: 30)
            }
        }
        #expect(factories == 1 && constructing == 1)
        factory.release()
        await staleDrain.waitUntilHeld()
        #expect(sync.generation == 32 && factories == 1)
        #expect(stale.events.isEmpty && stale.attachmentCount == 0)
        #expect(observations.isEmpty)

        await sync.waitForInputs()
        staleDrain.release()
        await attachment.value
        await repeated?.value
        await sync.waitForReplacement()
        #expect(factories == 2 && maximumConstructing == 1 && constructing == 0)
        #expect(sync.generation == 32 && sync.setupState == .attached && !sync.operationFailed)
        #expect(observations == [.started])
        sync.collectionReplaced(33)
        await sync.waitForReplacement()
        #expect(factories == 3 && maximumConstructing == 1 && constructing == 0)
        #expect(sync.generation == 33 && sync.setupState == .attached && !sync.operationFailed)
        #expect(observations == [.started])
        var shutdownReturned = false
        let shutdown = Task {
            let result = await sync.cancelAndDrain()
            shutdownReturned = true
            return result
        }
        await finalDrain.waitUntilHeld()
        #expect(!shutdownReturned && !latest.closed)
        #expect(observations == [.started, .drained])
        finalDrain.release()
        #expect(await shutdown.value)
        #expect(stale.closed && successor.closed && latest.closed && latest.closeCount == 1)
        #expect(observations == [.started, .drained])
    }

    @Test func replacementBeforeCompleteOpenedHostJoinsOriginalFactory() async {
        let original = SyncOwnerFake(), successor = SyncOwnerFake(), unexpected = SyncOwnerFake()
        let factory = SyncOwnerGate()
        var factories = 0, constructing = 0, maximumConstructing = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            factories += 1
            constructing += 1
            maximumConstructing = max(maximumConstructing, constructing)
            defer { constructing -= 1 }
            if factories == 1 { return original }
            #expect(factories == 2)
            if factories > 2 { return unexpected }
            #expect(original.closed && original.closeCount == 1)
            await factory.hold()
            return successor
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 40)
        sync.collectionReplaced(41)
        await factory.waitUntilHeld()
        var completion: Task<Void, Never>?
        var completionReturned = false
        await withCheckedContinuation { started in
            completion = Task {
                started.resume()
                await sync.completeOpenedHost(generation: 41)
                completionReturned = true
            }
        }
        sync.receiveLifecycle(.ready(generation: 41))
        sync.collectionReplaced(41)
        await sync.waitForInputs()
        #expect(factories == 2 && constructing == 1 && !completionReturned)
        #expect(successor.events.isEmpty && sync.generation == 41)
        factory.release()
        await completion?.value
        await sync.waitForReplacement()
        // A late retention completion from the old Host cannot roll back or
        // construct another backend after the replacement has attached.
        await sync.completeOpenedHost(generation: 40)
        #expect(factories == 2 && maximumConstructing == 1 && constructing == 0)
        #expect(sync.generation == 41 && sync.setupState == .attached && !sync.operationFailed)
        #expect(await sync.cancelAndDrain())
        #expect(original.closed && successor.closed && successor.closeCount == 1)
    }

    @Test func cancelledAttachmentCallerAndShutdownJoinOriginalFactoryAndCandidateDrain() async {
        let candidate = SyncOwnerFake()
        let factoryGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
        var factories = 0, shutdownReturned = false, attachmentReturned = false
        candidate.onClose = { await closeGate.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            factories += 1
            await factoryGate.hold()
            return candidate
        }, defaults: nil)
        await sync.preparePlatform()
        let attachment = Task {
            await sync.completeOpenedHost(generation: 50)
            attachmentReturned = true
        }
        await factoryGate.waitUntilHeld()
        attachment.cancel()
        var shutdown: Task<Bool, Never>?
        await withCheckedContinuation { started in
            shutdown = Task {
                started.resume()
                let result = await sync.cancelAndDrain()
                shutdownReturned = true
                return result
            }
        }
        #expect(!shutdownReturned && !attachmentReturned && factories == 1)
        factoryGate.release()
        await closeGate.waitUntilHeld()
        #expect(!shutdownReturned && !attachmentReturned && !candidate.closed)
        #expect(candidate.events.isEmpty && candidate.attachmentCount == 0)
        closeGate.release()
        await attachment.value
        #expect(await shutdown?.value == true)
        #expect(candidate.closed && candidate.closeCount == 1 && factories == 1)
        #expect(await sync.cancelAndDrain())
        #expect(candidate.closeCount == 1)
    }

    @Test func staleAttachmentCloseFailureRetainsOwnerUntilSuccessfulDrainBeforeNextFactory() async {
        let stale = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate()
        var factories = 0, closeAttempts = 0
        stale.onClose = {
            closeAttempts += 1
            throw BridgeError.internal("synthetic close refusal")
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            factories += 1
            if factories == 1 { await factory.hold(); return stale }
            #expect(stale.closed && stale.closeCount == 1)
            return latest
        }, defaults: nil)
        await sync.preparePlatform()
        let attachment = Task { await sync.completeOpenedHost(generation: 60) }
        await factory.waitUntilHeld()
        sync.collectionReplaced(61)
        await sync.waitForInputs()
        factory.release()
        await attachment.value
        await sync.waitForReplacement()
        #expect(closeAttempts == 2 && factories == 1 && !stale.closed)
        #expect(sync.operationFailed && sync.setupState == .failed && sync.generation == 61)
        #expect(stale.events.isEmpty && latest.events.isEmpty)
        stale.onClose = nil
        await sync.completeOpenedHost(generation: 61)
        await sync.waitForReplacement()
        #expect(stale.closed && factories == 2 && sync.setupState == .attached)
        #expect(sync.operationFailed && sync.generation == 61)
        #expect(await sync.cancelAndDrain())
        #expect(latest.closed && latest.closeCount == 1)
    }

    @Test func closeDuringSuccessorFactoryDrainsCandidateWithoutAttaching() async {
        let original = SyncOwnerFake(), candidate = SyncOwnerFake()
        let factory = SyncOwnerGate()
        var attachments = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            attachments += 1
            if attachments == 1 { return original }
            await factory.hold()
            return candidate
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 20)
        sync.collectionReplaced(21)
        await factory.waitUntilHeld()
        var close: Task<Bool, Never>?
        await withCheckedContinuation { started in
            close = Task {
                started.resume()
                return await sync.cancelAndDrain()
            }
        }
        factory.release()
        #expect(await close?.value == true)
        #expect(candidate.closed && candidate.closeCount == 1)
        #expect(candidate.events.isEmpty && candidate.attachmentCount == 0)
        #expect(attachments == 2)
    }

    @Test func deviceSettingsPersistWithoutEnablingOnPreviewOrCancel() async throws {
        let name = "SyncCoordinatorTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { SyncOwnerFake() }, defaults: defaults)
        sync.setDeviceSettings(paused: true, automatic: false)
        await sync.waitForInputs()
        #expect(!sync.settings.enabled)
        let restored = SyncCoordinator(bootstrap: {}, makeBackend: { SyncOwnerFake() }, defaults: defaults)
        // P25.1: a stored media off from an earlier build reads as on.
        #expect(restored.settings == .init(enabled: false, paused: true, automatic: false, media: true))
    }

    // sync/policy P25.5, U151's rows.

    private func round2Defaults(_ stored: [String: Bool]?) throws -> (UserDefaults, String) {
        let name = "SyncCoordinatorTests.round2.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        if let stored { defaults.set(stored, forKey: "meguri.cloud.deviceSettings") }
        return (defaults, name)
    }

    @Test func missingAutomaticReadsOnAndTheFirstInputCarriesIt() async throws {
        let (defaults, name) = try round2Defaults(nil)
        defer { defaults.removePersistentDomain(forName: name) }
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { backend }, defaults: defaults)
        #expect(sync.settings.automatic && sync.settings.media && !sync.settings.paused)
        #expect(sync.storedAutomatic && sync.automaticSwitchValue)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        await sync.waitForInputs()
        #expect(backend.appliedSettings?.automatic == true && backend.appliedSettings?.media == true)
        #expect(await sync.cancelAndDrain())
    }

    @Test func storedPauseBecomesAutomaticOffInOneWriteBeforeTheFirstInput() async throws {
        let (defaults, name) = try round2Defaults(["enabled": true, "paused": true, "automatic": true, "media": false])
        defer { defaults.removePersistentDomain(forName: name) }
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { backend }, defaults: defaults)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        await sync.waitForInputs()
        // The first attachment already carries the migrated settings.
        #expect(backend.appliedSettings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(sync.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!sync.storedAutomatic && !sync.updateHold)
        let stored = try #require(defaults.dictionary(forKey: "meguri.cloud.deviceSettings") as? [String: Bool])
        #expect(stored == ["enabled": true, "paused": false, "automatic": false, "media": true])
        #expect(await sync.cancelAndDrain())
        // It runs once: a later launch changes nothing, and a stored off survives.
        let later = SyncCoordinator(bootstrap: {}, makeBackend: { SyncOwnerFake() }, defaults: defaults)
        #expect(later.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!later.storedAutomatic)
    }

    @Test func storedSettingsThatWereNotPausedKeepTheirValues() async throws {
        let (defaults, name) = try round2Defaults(["enabled": true, "paused": false, "automatic": true, "media": false])
        defer { defaults.removePersistentDomain(forName: name) }
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { backend }, defaults: defaults)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        await sync.waitForInputs()
        // Only the removed media value changes; automatic and paused are untouched.
        #expect(backend.appliedSettings == .init(enabled: true, paused: false, automatic: true, media: true))
        #expect(sync.storedAutomatic)
        #expect(await sync.cancelAndDrain())
    }

    private func attach(_ stored: [String: Bool], operation: Result<Bool, any Error>?, defaults: UserDefaults)
        async -> (SyncCoordinator, [SyncDeviceSettings]) {
        defaults.set(stored, forKey: "meguri.cloud.deviceSettings")
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { backend }, defaults: defaults)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        return (sync, backend.appliedSettings.map { [$0] } ?? [])
    }

    @Test func anEarlierOffWithoutPauseIsCarriedToTheDeviceAndSurvivesAServiceChange() async throws {
        // P25.3: the earlier build kept Sync Automatically with the service's settings.
        let (defaults, name) = try round2Defaults(nil)
        defer { defaults.removePersistentDomain(forName: name) }
        let (sync, inputs) = await attach(["enabled": true, "paused": false, "automatic": false, "media": true],
            operation: nil, defaults: defaults)
        #expect(inputs.first == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!sync.storedAutomatic && defaults.object(forKey: "meguri.sync.automatic") as? Bool == false)
        #expect(await sync.cancelAndDrain())
        // Another service's settings arrive with their own default of on.
        // The device's value still reads off, and a setup commits it as off.
        let (other, _) = await attach(["enabled": false, "paused": false, "automatic": true, "media": true],
            operation: nil, defaults: defaults)
        #expect(!other.storedAutomatic && !other.automaticSwitchValue)
        #expect(await other.cancelAndDrain())
        // A device that never turned it off, and was not connected, stores nothing.
        let (fresh, freshName) = try round2Defaults(nil)
        defer { fresh.removePersistentDomain(forName: freshName) }
        let (untouched, _) = await attach(["enabled": false, "paused": false, "automatic": false, "media": true],
            operation: nil, defaults: fresh)
        #expect(untouched.storedAutomatic && fresh.object(forKey: "meguri.sync.automatic") == nil)
        #expect(await untouched.cancelAndDrain())
    }

    @Test func aClearingActBeforeTheMigrationCannotTurnSyncAutomaticallyOn() async throws {
        // The earlier build's pause with auto on, on a device whose migration has
        // not run (or whose write failed). Sync Now clears the hold first.
        let (defaults, name) = try round2Defaults(["enabled": true, "paused": true, "automatic": true, "media": true])
        defer { defaults.removePersistentDomain(forName: name) }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { SyncOwnerFake() }, defaults: defaults)
        #expect(sync.updateHold && sync.settings.automatic)
        #expect(await sync.clearUpdateHold())
        // The device's off is stored before the write, so it can never read on afterwards.
        #expect(sync.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!sync.storedAutomatic && defaults.object(forKey: "meguri.sync.automatic") as? Bool == false)
        // The migration then finds nothing to change, and a later launch keeps off.
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2); await sync.waitForInputs()
        #expect(defaults.bool(forKey: "meguri.sync.round2Migrated"))
        #expect(sync.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(await sync.cancelAndDrain())
        let later = SyncCoordinator(bootstrap: {}, makeBackend: { SyncOwnerFake() }, defaults: defaults)
        #expect(!later.settings.automatic && !later.storedAutomatic && !later.updateHold)
    }

}

extension ReviewCollectionStartupTests {

}

@MainActor
final class SyncOwnerFake: SyncCoordinatorBackend {
    var events: [String] = []
    var localRefreshCount = 0
    private var localRefreshWaiter: CheckedContinuation<Void, Never>?
    var attachmentCount = 0
    var quiesced = false
    var closed = false
    var closeCount = 0
    var appliedSettings: SyncDeviceSettings?
    var onAttached: (@MainActor (SyncDeviceSettings) -> Void)?
    var onQuiesce: (@MainActor () async throws -> Void)?
    var onClose: (@MainActor () async throws -> Void)?
    private let closes = AsyncStream<Void>.makeStream()
    private let quiescences = AsyncStream<Void>.makeStream()
    func waitForClose() async { for await _ in closes.stream { return } }
    func waitForQuiesce() async { for await _ in quiescences.stream { return } }
    func attached(settings: SyncDeviceSettings) {
        attachmentCount += 1; appliedSettings = settings; onAttached?(settings)
    }
    func resumeAfterCoreResume() { events.append("foreground") }
    func freezeForSuspend() { events.append("background") }
    func coreEvent(_ event: CoreEvent) {
        guard case .changes(let changes) = event.kind, changes.source != .syncIncoming else { return }
        events.append("localChange"); localRefreshCount += 1
        localRefreshWaiter?.resume(); localRefreshWaiter = nil
    }
    func waitForLocalRefresh(after count: Int) async {
        if localRefreshCount > count { return }
        await withCheckedContinuation { localRefreshWaiter = $0 }
    }
    func quiesce() async throws {
        guard !quiesced else { return }
        try await onQuiesce?(); quiesced = true
        quiescences.continuation.yield(())
    }
    func close() async throws {
        try await quiesce()
        try await onClose?(); closed = true; closeCount += 1
        closes.continuation.yield(())
    }
}

@MainActor
private final class SyncOwnerReleaseState {
    var releasing = false
}

@MainActor final class SyncOwnerGate {
    private var held = false
    private var continuation: CheckedContinuation<Void, Never>?
    private var observer: CheckedContinuation<Void, Never>?
    func hold() async {
        await withCheckedContinuation {
            continuation = $0; held = true
            observer?.resume(); observer = nil
        }
    }
    func waitUntilHeld() async {
        if held { return }
        await withCheckedContinuation { observer = $0 }
    }
    func release() { continuation?.resume(); continuation = nil }
}
