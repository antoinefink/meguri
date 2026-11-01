import Foundation
import Testing
@testable import AppModel
@testable import CoreBridge

@MainActor
struct SyncCoordinatorTests {
    @Test func missingConfigurationTracksLocalGenerationsWithoutConstructingCloudOwner() async {
        let backend = SyncOwnerFake()
        var factories = 0
        let sync = SyncCoordinator(bootstrap: { throw SyncOwnerFailure.configurationUnavailable },
            makeBackend: { _ in factories += 1; return backend }, defaults: nil)
        await sync.preparePlatform()
        sync.receiveLifecycle(.ready(generation: 4))
        await sync.waitForInputs()
        await sync.waitForReplacement()
        await sync.completeOpenedHost(generation: 4)
        sync.collectionReplaced(5)
        sync.receiveLifecycle(.ready(generation: 5))
        sync.receive(.refresh(.local))
        sync.receive(.network(reachable: true))
        sync.syncNow(source: .command)
        await sync.waitForInputs()
        await sync.waitForReplacement()
        #expect(sync.generation == 5)
        #expect(sync.setupState == .configurationUnavailable && !sync.operationFailed)
        #expect(sync.status == nil && sync.preview == nil && sync.recovery == nil)
        #expect(sync.connectionService == nil)
        #expect(factories == 0 && backend.accountReads == 0 && backend.events.isEmpty)
        #expect(await sync.cancelAndDrain())
        #expect(backend.closeCount == 0)
    }

    @Test func bootstrapDoesNotReadAccountAndOwnerSurvivesLayouts() async throws {
        let backend = SyncOwnerFake()
        var order: [String] = []
        let sync = SyncCoordinator(bootstrap: { order.append("configure") },
            makeBackend: { _ in order.append("retained-host"); return backend }, defaults: nil)
        let navigation = AppNavigation()
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: navigation, sync: sync)
        await sync.preparePlatform()
        #expect(order == ["configure"])
        #expect(backend.accountReads == 0)
        await sync.completeOpenedHost(generation: 4)
        await backend.waitForAccount()
        navigation.adoptLaunchLayout(.compact)
        navigation.adoptLaunchLayout(.sidebar)
        #expect(owner.sync === sync)
        await sync.completeOpenedHost(generation: 4)
        await sync.waitForInputs()
        #expect(order == ["configure", "retained-host"])
        #expect(backend.accountReads == 1)
        #expect(await sync.cancelAndDrain())
    }

    @Test func suspendedAccountReadDoesNotBlockInputsOrReplacement() async {
        let backend = SyncOwnerFake(), successor = SyncOwnerFake()
        backend.holdAccount = true
        var attachments = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            attachments += 1
            if attachments == 1 { return backend }
            #expect(backend.closed && backend.closeCount == 1)
            return successor
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 8)
        await backend.waitForAccount()
        sync.receiveLifecycle(.ready(generation: 9))
        sync.receive(.network(reachable: false))
        sync.receive(.refresh(.local))
        await sync.waitForInputs()
        #expect(sync.generation == 9)
        await sync.waitForReplacement()
        sync.receive(.network(reachable: false))
        sync.receive(.refresh(.local))
        await sync.waitForInputs()
        #expect(successor.events.contains("offline"))
        #expect(backend.events.contains("refresh"))
        #expect(successor.events.contains("refresh"))
        #expect(attachments == 2)
        backend.releaseAccount()
        #expect(await sync.cancelAndDrain())
        #expect(backend.closed)
    }

    @Test func localFlushBoundaryAndExpiryAreOrderedAndDrainIsIdempotent() async {
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 1)
        await sync.waitForInputs()
        backend.events.removeAll()
        // The ReviewCollection lifecycle invokes these only after actual resume/flush.
        await sync.foregroundAfterLocalResume()
        await sync.prepareConnectionAuthority()
        await sync.backgroundAfterLocalFlush(deadlineMilliseconds: 123_000)
        sync.backgroundExpired()
        await sync.foregroundAfterLocalResume()
        await sync.prepareConnectionAuthority()
        await sync.waitForInputs()
        #expect(backend.events.firstIndex(of: "foreground")! < backend.events.firstIndex(of: "background")!)
        #expect(backend.events.firstIndex(of: "background")! < backend.events.firstIndex(of: "expired")!)
        #expect(await sync.cancelAndDrain())
        #expect(await sync.cancelAndDrain())
        #expect(backend.closeCount == 1)
    }


    @Test func operationSourcesKeepCloudAppliesApartFromLocalTriggers() {
        for source in [OpSource.userUndoable, .userNonUndoable, .syncIncoming, .localApi, .optimizer, .rollover, .maintenance] {
            #expect(SyncCoordinator.changeCause(source) == .local)
        }
        #expect(SyncCoordinator.changeCause(.cloudIncoming) == .cloud)
        #expect(SyncCoordinator.changeCause(.cloudRekey) == .cloud)
    }

    @Test func currentOwnerInputFailuresRemainReportedWithoutSuccessorProof() async {
        let errors: [any Error] = [BridgeError.internal("synthetic-input-failure"),
            BridgeError.cloudPolicy(.generationChanged), CloudOwnTransportFailure.unexpectedReply]
        for error in errors {
            let backend = SyncOwnerFake()
            var attachments = 0
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
                attachments += 1
                return backend
            }, defaults: nil)
            await sync.preparePlatform()
            // No actual Core lifecycle can prove a successor to this fake
            // generation, regardless of other tests' process-wide Host state.
            await sync.completeOpenedHost(generation: .max)
            await sync.prepareConnectionAuthority()
            #expect(!sync.operationFailed)
            backend.inputError = error
            sync.receive(.refresh(.local))
            await sync.waitForInputs()
            await sync.waitForReplacement()
            #expect(sync.operationFailed)
            #expect(sync.generation == .max && sync.setupState == .attached)
            #expect(attachments == 1 && backend.closeCount == 0)
            #expect(await sync.cancelAndDrain())
            #expect(sync.operationFailed)
        }
    }

    @Test func secondReplacementDuringFactoryDrainsStaleCandidateBeforeLatestOwnerAttaches() async {
        let original = SyncOwnerFake(), stale = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate(), staleDrain = SyncOwnerGate()
        var callbacks: [@Sendable () async -> Void] = []
        stale.onClose = { await staleDrain.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { persisted in
            callbacks.append(persisted)
            switch callbacks.count {
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
        #expect(sync.generation == 12 && callbacks.count == 2)
        #expect(stale.events.isEmpty && latest.events.isEmpty)
        await callbacks[1]()
        await sync.waitForInputs()
        #expect(stale.persistedReads == 0)
        staleDrain.release()
        await sync.waitForReplacement()
        #expect(callbacks.count == 3 && sync.setupState == .attached)
        #expect(stale.closed && stale.accountReads == 0)
        await callbacks[0]()
        await callbacks[1]()
        await sync.waitForInputs()
        #expect(latest.persistedReads == 0)
        await callbacks[2]()
        await sync.waitForInputs()
        #expect(latest.persistedReads == 1)
        #expect(await sync.cancelAndDrain())
    }

    @Test func attachmentBeforeReplacementClosesStaleCandidateBeforeNextFactory() async {
        let stale = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate(), staleDrain = SyncOwnerGate(), finalDrain = SyncOwnerGate()
        var callbacks: [@Sendable () async -> Void] = []
        var constructing = 0, maximumConstructing = 0
        stale.onClose = { await staleDrain.hold() }
        latest.onClose = { await finalDrain.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { persisted in
            constructing += 1
            maximumConstructing = max(maximumConstructing, constructing)
            defer { constructing -= 1 }
            callbacks.append(persisted)
            if callbacks.count == 1 {
                await factory.hold()
                return stale
            }
            #expect(stale.closed && stale.closeCount == 1)
            return latest
        }, defaults: nil)
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
        #expect(callbacks.count == 1 && constructing == 1)
        factory.release()
        await staleDrain.waitUntilHeld()
        #expect(sync.generation == 32 && callbacks.count == 1)
        #expect(stale.events.isEmpty && stale.accountReads == 0)
        await callbacks[0]()
        await sync.waitForInputs()
        #expect(stale.persistedReads == 0)
        staleDrain.release()
        await attachment.value
        await repeated?.value
        await sync.waitForReplacement()
        #expect(callbacks.count == 2 && maximumConstructing == 1 && constructing == 0)
        #expect(sync.generation == 32 && sync.setupState == .attached && !sync.operationFailed)
        var shutdownReturned = false
        let shutdown = Task {
            let result = await sync.cancelAndDrain()
            shutdownReturned = true
            return result
        }
        await finalDrain.waitUntilHeld()
        #expect(!shutdownReturned && !latest.closed)
        finalDrain.release()
        #expect(await shutdown.value)
        #expect(stale.closed && latest.closed && latest.closeCount == 1)
    }

    @Test func replacementBeforeCompleteOpenedHostJoinsOriginalFactory() async {
        let original = SyncOwnerFake(), successor = SyncOwnerFake(), unexpected = SyncOwnerFake()
        let factory = SyncOwnerGate()
        var factories = 0, constructing = 0, maximumConstructing = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
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
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
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
        #expect(candidate.events.isEmpty && candidate.accountReads == 0)
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
            throw CloudOwnTransportFailure.unexpectedReply
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
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
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
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
        #expect(candidate.events.isEmpty && candidate.accountReads == 0)
        #expect(attachments == 2)
    }

    @Test func deviceSettingsPersistWithoutEnablingOnPreviewOrCancel() async throws {
        let name = "SyncCoordinatorTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        sync.setDeviceSettings(paused: true, automatic: false, media: false)
        await sync.waitForInputs()
        await sync.cancelConnectionPreview()
        #expect(!sync.settings.enabled)
        let restored = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        #expect(restored.settings == .init(enabled: false, paused: true, automatic: false, media: false))
    }
}

@MainActor
final class SyncOwnerFake: SyncCoordinatorBackend {
    var events: [String] = []
    var refreshCauses: [CloudPolicyChangeCause] = []
    private var localRefreshWaiter: CheckedContinuation<Void, Never>?
    var localRefreshCount = 0
    var accountReads = 0
    var holdAccount = false
    var accountResult: SyncCommandResult?
    var quiesced = false
    var closed = false
    var closeCount = 0
    var persistedReads = 0
    var inputError: (any Error)?
    var nextCommands: [CloudPolicyCommand] = []
    var executeCommand: (@MainActor (CloudPolicyCommand) async -> SyncCommandResult)?
    private let executions = AsyncStream<Void>.makeStream()
    func waitForExecution() async { for await _ in executions.stream { return } }
    private let closes = AsyncStream<Void>.makeStream()
    func waitForClose() async { for await _ in closes.stream { return } }
    private let quiescences = AsyncStream<Void>.makeStream()
    func waitForQuiesce() async { for await _ in quiescences.stream { return } }
    var onQuiesce: (@MainActor () async throws -> Void)?
    var onClose: (@MainActor () async throws -> Void)?
    private var account: CheckedContinuation<Void, Never>?
    private var accountStarted: CheckedContinuation<Void, Never>?
    var connection: CoreCloudConnectionService? { nil }
    var own: CloudOwnExecutor? { nil }
    func takeAccountReadRequest() async -> Bool { persistedReads += 1; return false }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        if let inputError { throw inputError }
        switch event {
        case .foreground: events.append("foreground")
        case .background: events.append("background")
        case .backgroundExpiring: events.append("expired")
        case .network(false): events.append("offline")
        case .refresh(let cause):
            events.append("refresh"); refreshCauses.append(cause)
            if cause == .local { localRefreshCount += 1; localRefreshWaiter?.resume(); localRefreshWaiter = nil }
        default: break
        }
        let commands = nextCommands
        nextCommands = []
        return .commands(commands)
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        let result = await executeCommand?(command) ?? .init(outputs: [], error: nil, deferred: false)
        executions.continuation.yield(())
        return result
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 { UInt64(accountReads + 1) }
    func readAccount(_ read: UInt64) async -> SyncCommandResult {
        accountReads += 1
        accountStarted?.resume(); accountStarted = nil
        if holdAccount {
            await withCheckedContinuation { continuation in
                account = continuation
            }
        }
        return accountResult ?? .init(outputs: [.commands([])], error: nil, deferred: false, accountVerified: true)
    }
    func waitForLocalRefresh(after count: Int) async {
        if localRefreshCount > count { return }
        await withCheckedContinuation { localRefreshWaiter = $0 }
    }
    func waitForAccount() async {
        if accountReads > 0 { return }
        await withCheckedContinuation { accountStarted = $0 }
    }
    func releaseAccount() { account?.resume(); account = nil }
    func quiesce() async throws {
        guard !quiesced else { return }
        try await onQuiesce?(); quiesced = true; releaseAccount()
        quiescences.continuation.yield(())
    }
    func close() async throws {
        try await quiesce()
        try await onClose?(); closed = true; closeCount += 1; releaseAccount()
        closes.continuation.yield(())
    }
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
